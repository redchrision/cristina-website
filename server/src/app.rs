//! Application state and the HTTP router.

use std::{ops::Deref, path::Path, sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::{MatchedPath, Request, State},
    http::{HeaderName, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use tower_http::{
    catch_panic::CatchPanicLayer,
    compression::CompressionLayer,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    services::{ServeDir, ServeFile},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use utoipa_scalar::{Scalar, Servable};

use crate::{
    api_v1, auth,
    auth::KeyStore,
    config::ServeConfig,
    contact,
    error::{ApiError, ApiResult},
    events::{self, EventContext, IngestEvent, Ingestor, Sink},
    mcp::SiteMcp,
    quota::{DailyQuotas, Policy, RateLimiter},
};

pub struct Inner {
    pub cfg: ServeConfig,
    pub db: SqlitePool,
    pub keys: KeyStore,
    pub limiter: RateLimiter,
    pub quotas: DailyQuotas,
    pub ingest: Ingestor,
    pub sink: Sink,
    pub http: reqwest::Client,
}

#[derive(Clone)]
pub struct AppState(pub Arc<Inner>);

impl Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

const REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// Pageview beacon from the site itself: no key, per-IP rate limit.
const BEACON_POLICY: Policy = Policy::new(2.0, 20.0);

pub fn router(state: AppState, shutdown: CancellationToken) -> Router {
    let (v1, openapi) = api_v1::router().split_for_parts();
    let openapi_json = serde_json::to_value(&openapi).expect("OpenAPI serialises");

    let mut allowed_hosts: Vec<String> = vec!["localhost".into(), "127.0.0.1".into(), "::1".into()];
    allowed_hosts.extend(state.cfg.public_hosts.iter().filter(|h| !h.is_empty()).cloned());
    let mcp_state = state.clone();
    let mcp = StreamableHttpService::new(
        move || Ok(SiteMcp::new(mcp_state.clone())),
        Arc::new(LocalSessionManager::default()),
        // Stateless + plain JSON responses: any replica can answer any MCP
        // request, so the endpoint scales horizontally like the REST API.
        StreamableHttpServerConfig::default()
            .with_cancellation_token(shutdown.child_token())
            .with_allowed_hosts(allowed_hosts)
            .with_legacy_session_mode(false)
            .with_json_response(true)
            .with_max_request_body_bytes(256 * 1024),
    );

    let authed = Router::new()
        .nest("/v1", v1)
        .nest_service("/mcp", mcp)
        .layer(middleware::from_fn_with_state(state.clone(), auth::authenticate));

    let static_dir = &state.cfg.static_dir;
    let index = static_dir.join("index.html");
    let site = ServeDir::new(static_dir)
        .precompressed_br()
        .precompressed_gzip()
        .fallback(ServeFile::new(&index));

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readyz))
        .route("/api/contact", post(contact::submit))
        .route("/api/collect", post(collect))
        .route(
            "/v1/openapi.json",
            get(move || {
                let spec = openapi_json.clone();
                async move { Json(spec) }
            }),
        )
        .merge(Scalar::with_url("/docs", openapi).custom_html(SCALAR_HTML))
        .merge(authed)
        .fallback_service(site)
        .layer(middleware::from_fn_with_state(state.clone(), security_headers))
        .layer(middleware::from_fn(http_metrics))
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, Duration::from_secs(15)))
        .layer(CatchPanicLayer::new())
        .layer(PropagateRequestIdLayer::new(REQUEST_ID))
        .layer(TraceLayer::new_for_http().make_span_with(|req: &Request| {
            let rid = req
                .headers()
                .get(REQUEST_ID)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("-");
            tracing::info_span!("http", method = %req.method(), path = %req.uri().path(), request_id = %rid)
        }))
        .layer(SetRequestIdLayer::new(REQUEST_ID, MakeRequestUuid))
        .with_state(state)
}

/// Scalar's default page with the bundle pinned to an exact version.
const SCALAR_HTML: &str = r#"<!doctype html>
<html>
<head>
  <title>API reference · cristinadelisle.xyz</title>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
</head>
<body>
  <script id="api-reference" type="application/json">$spec</script>
  <script src="https://cdn.jsdelivr.net/npm/@scalar/api-reference@1.73.1"></script>
</body>
</html>"#;

#[derive(Serialize)]
struct Ready {
    database: &'static str,
    clickhouse: &'static str,
}

async fn readyz(State(state): State<AppState>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let ch = match &state.sink {
        Sink::ClickHouse(c) => {
            if c.query("SELECT 1").execute().await.is_ok() {
                "ok"
            } else {
                "unreachable"
            }
        }
        _ => "disabled",
    };
    let status = if db_ok && ch != "unreachable" {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(Ready {
            database: if db_ok { "ok" } else { "unreachable" },
            clickhouse: ch,
        }),
    )
}

#[derive(Debug, Deserialize)]
struct Beacon {
    path: String,
    #[serde(default)]
    referrer: Option<String>,
    #[serde(default)]
    lang: Option<String>,
    #[serde(default)]
    device: Option<String>,
}

/// `navigator.sendBeacon` target. Accepts any content type (beacons are sent
/// as `text/plain` to avoid a CORS preflight) and answers 204.
async fn collect(State(state): State<AppState>, req: Request) -> ApiResult<StatusCode> {
    let ip = auth::client_ip(&state, &req);
    if !state.limiter.check(&format!("beacon:{ip}"), BEACON_POLICY).allowed {
        // Silently drop: a beacon cannot act on an error anyway.
        return Ok(StatusCode::NO_CONTENT);
    }
    let country = api_v1::country(&state, &req);
    let body = axum::body::to_bytes(req.into_body(), 4 * 1024)
        .await
        .map_err(|_| ApiError::BadRequest("body too large".into()))?;
    let b: Beacon =
        serde_json::from_slice(&body).map_err(|e| ApiError::BadRequest(format!("invalid JSON: {e}")))?;
    let row = IngestEvent {
        kind: "pageview".into(),
        path: b.path,
        ts: None,
        referrer: b.referrer,
        lang: b.lang,
        device: b.device,
        value: None,
    }
    .into_row(
        &EventContext {
            source: "beacon".into(),
            country,
        },
        events::now_ms(),
    )
    .map_err(ApiError::BadRequest)?;
    state.ingest.enqueue(vec![row])?;
    Ok(StatusCode::NO_CONTENT)
}

const CSP_SITE: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
    img-src 'self' data:; font-src 'self'; connect-src 'self'; object-src 'none'; \
    base-uri 'self'; form-action 'self'; frame-ancestors 'none'";
/// The API reference page loads Scalar from jsDelivr.
const CSP_DOCS: &str = "default-src 'self'; script-src 'self' https://cdn.jsdelivr.net; \
    style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net; img-src 'self' data: https:; \
    font-src 'self' data: https://cdn.jsdelivr.net https://fonts.scalar.com; connect-src 'self'; \
    object-src 'none'; base-uri 'self'; frame-ancestors 'none'";

async fn security_headers(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let path = req.uri().path().to_owned();
    let mut res = next.run(req).await;
    let h = res.headers_mut();
    let csp = if path.starts_with("/docs") { CSP_DOCS } else { CSP_SITE };
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(csp));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("strict-origin-when-cross-origin"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), interest-cohort=()"),
    );
    h.insert(
        HeaderName::from_static("cross-origin-opener-policy"),
        HeaderValue::from_static("same-origin"),
    );
    if state.cfg.hsts {
        h.insert(
            header::STRICT_TRANSPORT_SECURITY,
            HeaderValue::from_static("max-age=63072000; includeSubDomains; preload"),
        );
    }
    if !h.contains_key(header::CACHE_CONTROL) {
        let cc = if path.starts_with("/assets/") {
            // Vite fingerprints asset file names, so they never change.
            "public, max-age=31536000, immutable"
        } else if path.starts_with("/v1") || path.starts_with("/api") || path.starts_with("/mcp") {
            "no-store"
        } else {
            "no-cache"
        };
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cc));
    }
    res
}

/// RED metrics per route: rate, errors, duration (p50/p99 come from the
/// histogram).
async fn http_metrics(req: Request, next: Next) -> Response {
    let started = std::time::Instant::now();
    let method = req.method().as_str().to_owned();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| {
            let p = req.uri().path();
            if p.starts_with("/mcp") {
                "/mcp".into()
            } else if p.starts_with("/v1") {
                "/v1/*".into()
            } else {
                "static".into()
            }
        });
    let res = next.run(req).await;
    let status = res.status().as_u16().to_string();
    metrics::histogram!(
        "http_server_request_duration_seconds",
        "method" => method.clone(), "route" => route.clone(), "status" => status.clone()
    )
    .record(started.elapsed());
    metrics::counter!(
        "http_server_requests_total",
        "method" => method, "route" => route, "status" => status
    )
    .increment(1);
    res
}

/// Periodic housekeeping: key reloads, quota flushes, bucket sweeps,
/// contact-message retention.
pub fn spawn_background(state: AppState, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut keys = tokio::time::interval(Duration::from_secs(30));
        let mut usage = tokio::time::interval(Duration::from_secs(5));
        let mut sweep = tokio::time::interval(Duration::from_secs(60));
        let mut purge = tokio::time::interval(Duration::from_secs(3600));
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = keys.tick() => {
                    if let Err(e) = state.keys.reload(&state.db).await {
                        tracing::warn!(error = %e, "API key reload failed");
                    }
                }
                _ = usage.tick() => {
                    if let Err(e) = state.quotas.flush(&state.db).await {
                        tracing::warn!(error = %e, "quota flush failed");
                    }
                }
                _ = sweep.tick() => {
                    state.limiter.sweep(Duration::from_secs(3600));
                    metrics::gauge!("rate_limiter_buckets").set(state.limiter.len() as f64);
                }
                _ = purge.tick() => {
                    match contact::purge_expired(&state.db, state.cfg.contact_retention_days).await {
                        Ok(0) => {}
                        Ok(n) => tracing::info!(deleted = n, "expired contact messages purged"),
                        Err(e) => tracing::warn!(error = %e, "contact purge failed"),
                    }
                }
            }
        }
        // Persist the last usage deltas before exiting.
        if let Err(e) = state.quotas.flush(&state.db).await {
            tracing::warn!(error = %e, "final quota flush failed");
        }
    })
}

pub async fn open_db(url: &str) -> anyhow::Result<SqlitePool> {
    use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
    use std::str::FromStr;

    if let Some(path) = url.strip_prefix("sqlite://").map(|p| p.split('?').next().unwrap_or(p))
        && let Some(dir) = Path::new(path).parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir)?;
    }
    let opts = SqliteConnectOptions::from_str(url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .busy_timeout(Duration::from_secs(5));
    let pool = SqlitePoolOptions::new().max_connections(8).connect_with(opts).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
