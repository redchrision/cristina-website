//! HTTP-level tests: the real router, a temporary SQLite file and an
//! in-memory event sink. No network, no ClickHouse.

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use clap::Parser;
use http_body_util::BodyExt;
use serde_json::{Value, json};
use site_server::{
    app::{self, AppState, Inner},
    auth::{self, KeyStore},
    config::ServeConfig,
    events::{self, EventRow, Sink, WriterConfig},
    quota::{DailyQuotas, RateLimiter},
};
use tokio_util::sync::CancellationToken;
use tower::ServiceExt;

#[derive(Parser)]
struct Wrapper {
    #[command(flatten)]
    cfg: ServeConfig,
}

struct TestApp {
    router: Router,
    state: AppState,
    events: Arc<Mutex<Vec<EventRow>>>,
}

async fn create_key(
    state: &AppState,
    scopes: &str,
    rate: f64,
    burst: f64,
    quota: Option<i64>,
) -> String {
    let (key, prefix) = auth::generate_key();
    sqlx::query(
        "INSERT INTO api_keys (name, prefix, secret_hash, scopes, rate_per_sec, burst, daily_quota, created_at)
         VALUES ('test', ?, ?, ?, ?, ?, ?, 0)",
    )
    .bind(&prefix)
    .bind(auth::hash_key(&key))
    .bind(scopes)
    .bind(rate)
    .bind(burst)
    .bind(quota)
    .execute(&state.db)
    .await
    .unwrap();
    state.keys.reload(&state.db).await.unwrap();
    key
}

async fn test_app() -> TestApp {
    let dir = std::env::temp_dir().join(format!("site-test-{}", uuid::Uuid::new_v4()));
    let db_url = format!("sqlite://{}/t.db?mode=rwc", dir.display());
    let static_dir = dir.join("dist");
    std::fs::create_dir_all(&static_dir).unwrap();
    std::fs::write(
        static_dir.join("index.html"),
        "<!doctype html><title>t</title>",
    )
    .unwrap();

    let mut cfg = Wrapper::parse_from(["test"]).cfg;
    cfg.static_dir = static_dir;
    cfg.database_url = db_url.clone();

    let db = app::open_db(&db_url).await.unwrap();
    let store = Arc::new(Mutex::new(Vec::new()));
    let sink = Sink::Memory(store.clone());
    let (ingest, _writer) = events::spawn(
        sink.clone(),
        WriterConfig {
            capacity: 10_000,
            max_rows: 100,
            flush_interval: std::time::Duration::from_millis(10),
        },
    );
    let state = AppState(Arc::new(Inner {
        cfg,
        db,
        keys: KeyStore::default(),
        limiter: RateLimiter::default(),
        quotas: DailyQuotas::default(),
        ingest,
        sink,
        http: reqwest::Client::new(),
    }));
    TestApp {
        router: app::router(state.clone(), CancellationToken::new()),
        state,
        events: store,
    }
}

async fn call(app: &TestApp, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
    let res = app.router.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, headers, body)
}

fn get(uri: &str, key: Option<&str>) -> Request<Body> {
    let mut b = Request::get(uri);
    if let Some(k) = key {
        b = b.header(header::AUTHORIZATION, format!("Bearer {k}"));
    }
    b.body(Body::empty()).unwrap()
}

fn post_json(uri: &str, key: Option<&str>, body: Value) -> Request<Body> {
    let mut b = Request::post(uri).header(header::CONTENT_TYPE, "application/json");
    if let Some(k) = key {
        b = b.header(header::AUTHORIZATION, format!("Bearer {k}"));
    }
    b.body(Body::from(body.to_string())).unwrap()
}

#[tokio::test]
async fn content_is_public_and_localised() {
    let app = test_app().await;
    let (s, h, body) = call(&app, get("/v1/projects?lang=en", None)).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h["api-version"], "1");
    assert!(h.contains_key("ratelimit-remaining"));
    assert_eq!(body["data"][0]["name"], "Something is brewing");

    let (s, _, body) = call(&app, get("/v1/posts/technovation-judge?lang=fr", None)).await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        body["title"]
            .as_str()
            .unwrap()
            .starts_with("Juger Technovation")
    );

    let (s, h, body) = call(&app, get("/v1/posts/nope", None)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert_eq!(h[header::CONTENT_TYPE], "application/problem+json");
    assert_eq!(body["code"], "not_found");
}

#[tokio::test]
async fn auth_and_scopes_are_enforced() {
    let app = test_app().await;
    let reader = create_key(&app.state, "content:read", 100.0, 100.0, None).await;

    let (s, h, _) = call(&app, get("/v1/stats/summary", None)).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    assert!(h.contains_key(header::WWW_AUTHENTICATE));

    let (s, _, _) = call(&app, get("/v1/projects", Some("csk_bogus_key"))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, _, body) = call(&app, get("/v1/stats/summary", Some(&reader))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "insufficient_scope");

    let (s, _, body) = call(&app, get("/v1/me", Some(&reader))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(body["scopes"], json!(["content:read"]));
}

#[tokio::test]
async fn rate_limit_returns_429_with_retry_after() {
    let app = test_app().await;
    let key = create_key(&app.state, "content:read", 0.1, 2.0, None).await;
    for _ in 0..2 {
        assert_eq!(
            call(&app, get("/v1/talks", Some(&key))).await.0,
            StatusCode::OK
        );
    }
    let (s, h, body) = call(&app, get("/v1/talks", Some(&key))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rate_limited");
    assert_eq!(h["ratelimit-remaining"], "0");
    assert!(
        h[header::RETRY_AFTER]
            .to_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            >= 1
    );
}

#[tokio::test]
async fn daily_quota_counts_requests_and_events() {
    let app = test_app().await;
    let key = create_key(&app.state, "events:write", 1000.0, 1000.0, Some(10)).await;
    let batch = |n: usize| json!({ "events": (0..n).map(|i| json!({"kind":"click","path":format!("/{i}")})).collect::<Vec<_>>() });

    // 1 request + 5 events = 6 units
    let (s, h, _) = call(&app, post_json("/v1/events", Some(&key), batch(5))).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(h["x-quota-limit"], "10");
    // 1 + 5 more would exceed 10: rejected, nothing enqueued
    let (s, _, body) = call(&app, post_json("/v1/events", Some(&key), batch(5))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "quota_exhausted");
}

#[tokio::test]
async fn events_are_validated_minimised_and_written() {
    let app = test_app().await;
    let key = create_key(&app.state, "events:write", 1000.0, 1000.0, None).await;

    let (s, _, body) = call(
        &app,
        post_json("/v1/events", Some(&key), json!({"events": []})),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "bad_request");

    let (s, _, body) = call(
        &app,
        post_json(
            "/v1/events",
            Some(&key),
            json!({"events": [{"kind":"pageview","path":"/","referrer":"https://example.org/a?b=c","lang":"fr"}]}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::ACCEPTED);
    assert_eq!(body["accepted"], 1);

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let rows = app.events.lock().unwrap().clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].referrer_host, "example.org");
    assert!(rows[0].source.starts_with("csk_"));
}

#[tokio::test]
async fn stats_report_unavailable_without_clickhouse() {
    let app = test_app().await;
    let key = create_key(&app.state, "stats:read", 100.0, 100.0, None).await;
    let (s, _, body) = call(&app, get("/v1/stats/summary", Some(&key))).await;
    assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body["code"], "unavailable");
}

#[tokio::test]
async fn contact_form_validates_stores_and_traps_bots() {
    let app = test_app().await;
    let ok = json!({"name":"Ada","email":"ada@example.com","message":"Hello there, nice site!"});

    let (s, _, _) = call(&app, post_json("/api/contact", None, ok.clone())).await;
    assert_eq!(s, StatusCode::ACCEPTED);

    let bot = json!({"name":"Bot","email":"b@example.com","message":"Buy cheap things now!!","website":"spam.example"});
    let (s, _, _) = call(&app, post_json("/api/contact", None, bot)).await;
    assert_eq!(s, StatusCode::ACCEPTED, "honeypot hits look like success");

    let (s, _, body) = call(
        &app,
        post_json(
            "/api/contact",
            None,
            json!({"name":"A","email":"nope","message":"long enough message"}),
        ),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(body["detail"], "email address is not valid");

    let owner = create_key(&app.state, "messages:read", 100.0, 100.0, None).await;
    let (_, _, body) = call(&app, get("/v1/messages", Some(&owner))).await;
    let msgs = body["data"].as_array().unwrap();
    assert_eq!(msgs.len(), 1, "the bot message must not be stored");
    assert_eq!(msgs[0]["name"], "Ada");

    // 3-message burst per IP, then 429.
    for _ in 0..2 {
        call(&app, post_json("/api/contact", None, ok.clone())).await;
    }
    let (s, _, _) = call(&app, post_json("/api/contact", None, ok)).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn site_responses_carry_security_headers() {
    let app = test_app().await;
    let (s, h, _) = call(&app, get("/", None)).await;
    assert_eq!(s, StatusCode::OK);
    let csp = h[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(csp.contains("default-src 'self'") && csp.contains("frame-ancestors 'none'"));
    assert_eq!(h[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert!(h.contains_key("x-request-id"));
    assert!(
        !h.contains_key(header::STRICT_TRANSPORT_SECURITY),
        "HSTS off unless configured"
    );

    let (_, h, _) = call(&app, get("/v1/projects", None)).await;
    assert_eq!(h[header::CACHE_CONTROL], "no-store");
}

#[tokio::test]
async fn openapi_document_lists_every_v1_route() {
    let app = test_app().await;
    let (s, _, spec) = call(&app, get("/v1/openapi.json", None)).await;
    assert_eq!(s, StatusCode::OK);
    let paths: Vec<&str> = spec["paths"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for p in [
        "/projects",
        "/talks",
        "/posts",
        "/posts/{slug}",
        "/events",
        "/stats/summary",
        "/stats/timeseries",
        "/stats/top-paths",
        "/messages",
        "/me",
    ] {
        assert!(paths.contains(&p), "missing {p} in {paths:?}");
    }
    assert!(spec["components"]["securitySchemes"]["api_key"].is_object());
}

#[tokio::test]
async fn mcp_tools_respect_scopes() {
    let app = test_app().await;
    let reader = create_key(&app.state, "content:read", 100.0, 100.0, None).await;
    let rpc = |id: u32, method: &str, params: Value| {
        Request::post("/mcp")
            .header(header::HOST, "localhost")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-06-18")
            .header(header::AUTHORIZATION, format!("Bearer {reader}"))
            .body(Body::from(
                json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}).to_string(),
            ))
            .unwrap()
    };

    let (s, _, body) = call(&app, rpc(1, "tools/list", json!({}))).await;
    assert_eq!(s, StatusCode::OK);
    let names: Vec<&str> = body["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"list_projects") && names.contains(&"list_contact_messages"));

    let (_, _, body) = call(
        &app,
        rpc(
            2,
            "tools/call",
            json!({"name":"list_talks","arguments":{"lang":"en"}}),
        ),
    )
    .await;
    assert_eq!(body["result"]["isError"], false);

    let (_, _, body) = call(
        &app,
        rpc(
            3,
            "tools/call",
            json!({"name":"list_contact_messages","arguments":{}}),
        ),
    )
    .await;
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("messages:read")
    );
}
