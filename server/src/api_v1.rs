//! Public API, version 1.
//!
//! Conventions (documented in `docs/API.md` and the OpenAPI spec):
//! * Versioned in the path (`/v1`). Additive changes only within a version;
//!   breaking changes go to `/v2`, and `/v1` then answers with `Deprecation`
//!   and `Sunset` headers for at least six months.
//! * Collections are wrapped in `{ "data": [...] }` so metadata (pagination,
//!   warnings) can be added later without breaking clients.
//! * Errors are `application/problem+json` (RFC 9457) with a stable `code`.
//! * Every response carries `RateLimit-*` headers; keyed responses also carry
//!   `X-Quota-*` headers for the daily quota.

use axum::{
    Extension, Json,
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use utoipa::{
    IntoParams, Modify, OpenApi, ToSchema,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{
    app::AppState,
    auth::{Principal, Scope},
    contact::{self, ContactMessage},
    content::{self, BlogPost, BlogPostSummary, Lang, Project, Talk},
    error::{ApiError, ApiResult, Problem},
    events::{self, EventContext, IngestEvent},
    quota,
    stats::{self, DayCount, PathCount, StatsQuery, Summary},
};

macro_rules! list_type {
    ($name:ident, $item:ty) => {
        #[derive(Debug, Serialize, ToSchema)]
        pub struct $name {
            pub data: Vec<$item>,
        }
        impl From<Vec<$item>> for $name {
            fn from(data: Vec<$item>) -> Self {
                Self { data }
            }
        }
    };
}

list_type!(ProjectList, Project);
list_type!(TalkList, Talk);
list_type!(PostList, BlogPostSummary);
list_type!(DayCountList, DayCount);
list_type!(PathCountList, PathCount);
list_type!(MessageList, ContactMessage);

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct LangQuery {
    /// Return plain strings in this language instead of `{en, fr}` objects.
    pub lang: Option<Lang>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PostsQuery {
    pub lang: Option<Lang>,
    /// Only posts with this tag.
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct MessagesQuery {
    /// 1..=200, default 50.
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct EventBatch {
    /// 1..=1000 events. The batch is accepted or rejected as a whole.
    pub events: Vec<IngestEvent>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EventsAccepted {
    pub accepted: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct Me {
    /// `anonymous` or the key prefix.
    pub principal: String,
    pub name: Option<String>,
    pub scopes: Vec<String>,
    /// Requests per second sustained / burst size.
    pub rate_per_sec: f64,
    pub burst: f64,
    pub daily_quota: Option<u64>,
    pub used_today: u64,
}

#[utoipa::path(get, path = "/projects", tag = "content", params(LangQuery),
    responses((status = 200, body = ProjectList), (status = 429, body = Problem)))]
async fn list_projects(Query(q): Query<LangQuery>) -> Json<ProjectList> {
    Json(content::projects(q.lang).into())
}

#[utoipa::path(get, path = "/talks", tag = "content", params(LangQuery),
    responses((status = 200, body = TalkList), (status = 429, body = Problem)))]
async fn list_talks(Query(q): Query<LangQuery>) -> Json<TalkList> {
    Json(content::talks(q.lang).into())
}

#[utoipa::path(get, path = "/posts", tag = "content", params(PostsQuery),
    responses((status = 200, body = PostList), (status = 429, body = Problem)))]
async fn list_posts(Query(q): Query<PostsQuery>) -> Json<PostList> {
    Json(content::posts(q.lang, q.tag.as_deref()).into())
}

#[utoipa::path(get, path = "/posts/{slug}", tag = "content",
    params(("slug" = String, Path, description = "Post slug"), LangQuery),
    responses((status = 200, body = BlogPost), (status = 404, body = Problem)))]
async fn get_post(Path(slug): Path<String>, Query(q): Query<LangQuery>) -> ApiResult<Json<BlogPost>> {
    content::post(&slug, q.lang).map(Json).ok_or(ApiError::NotFound)
}

/// Ingest a batch of events. Each event also counts one unit against the
/// key's daily quota.
#[utoipa::path(post, path = "/events", tag = "events",
    request_body = EventBatch,
    security(("api_key" = ["events:write"])),
    responses(
        (status = 202, body = EventsAccepted),
        (status = 400, body = Problem),
        (status = 401, body = Problem),
        (status = 403, body = Problem),
        (status = 429, description = "Rate limit or quota exceeded", body = Problem),
        (status = 503, description = "Ingestion queue full; retry after `Retry-After`", body = Problem),
    ))]
async fn ingest_events(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    req: Request,
) -> ApiResult<impl IntoResponse> {
    principal.require(Scope::EventsWrite)?;
    let country = country(&state, &req);
    let body = axum::body::to_bytes(req.into_body(), 1024 * 1024)
        .await
        .map_err(|_| ApiError::BadRequest("request body exceeds 1 MiB".into()))?;
    let batch: EventBatch = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON: {e}")))?;
    let n = batch.events.len();
    if n == 0 || n > events::MAX_BATCH {
        return Err(ApiError::BadRequest(format!(
            "`events` must contain 1..={} items",
            events::MAX_BATCH
        )));
    }
    let ctx = EventContext {
        source: principal.label(),
        country,
    };
    let now = events::now_ms();
    let rows = batch
        .events
        .into_iter()
        .enumerate()
        .map(|(i, e)| e.into_row(&ctx, now).map_err(|m| ApiError::BadRequest(format!("events[{i}]: {m}"))))
        .collect::<Result<Vec<_>, _>>()?;

    if let Principal::Key(k) = &principal
        && state.quotas.try_consume(k.id, n as u64, k.daily_quota).is_err()
    {
        return Err(ApiError::QuotaExhausted {
            retry_after_secs: quota::secs_until_utc_midnight(),
        });
    }
    let accepted = state.ingest.enqueue(rows)?;
    Ok((StatusCode::ACCEPTED, Json(EventsAccepted { accepted })))
}

pub fn country(state: &AppState, req: &Request) -> String {
    if !state.cfg.trust_cloudflare {
        return String::new();
    }
    req.headers()
        .get("cf-ipcountry")
        .and_then(|v| v.to_str().ok())
        .filter(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_alphabetic()))
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or_default()
}

#[utoipa::path(get, path = "/stats/summary", tag = "stats", params(StatsQuery),
    security(("api_key" = ["stats:read"])),
    responses((status = 200, body = Summary), (status = 403, body = Problem), (status = 503, body = Problem)))]
async fn stats_summary(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<Summary>> {
    principal.require(Scope::StatsRead)?;
    Ok(Json(stats::summary(&state.sink, &q).await?))
}

#[utoipa::path(get, path = "/stats/timeseries", tag = "stats", params(StatsQuery),
    security(("api_key" = ["stats:read"])),
    responses((status = 200, body = DayCountList), (status = 403, body = Problem), (status = 503, body = Problem)))]
async fn stats_timeseries(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<DayCountList>> {
    principal.require(Scope::StatsRead)?;
    Ok(Json(stats::timeseries(&state.sink, &q).await?.into()))
}

#[utoipa::path(get, path = "/stats/top-paths", tag = "stats", params(StatsQuery),
    security(("api_key" = ["stats:read"])),
    responses((status = 200, body = PathCountList), (status = 403, body = Problem), (status = 503, body = Problem)))]
async fn stats_top_paths(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<StatsQuery>,
) -> ApiResult<Json<PathCountList>> {
    principal.require(Scope::StatsRead)?;
    Ok(Json(stats::top_paths(&state.sink, &q).await?.into()))
}

#[utoipa::path(get, path = "/messages", tag = "messages", params(MessagesQuery),
    security(("api_key" = ["messages:read"])),
    responses((status = 200, body = MessageList), (status = 401, body = Problem), (status = 403, body = Problem)))]
async fn list_messages(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Query(q): Query<MessagesQuery>,
) -> ApiResult<Json<MessageList>> {
    principal.require(Scope::MessagesRead)?;
    Ok(Json(contact::list(&state.db, q.limit.unwrap_or(50)).await?.into()))
}

/// Describe the calling principal: scopes, limits and today's usage.
#[utoipa::path(get, path = "/me", tag = "account",
    responses((status = 200, body = Me), (status = 401, body = Problem)))]
async fn me(State(state): State<AppState>, Extension(principal): Extension<Principal>) -> Json<Me> {
    Json(match &principal {
        Principal::Key(k) => Me {
            principal: k.prefix.clone(),
            name: Some(k.name.clone()),
            scopes: k.scopes.iter().map(|s| s.to_string()).collect(),
            rate_per_sec: k.policy.rate,
            burst: k.policy.burst,
            daily_quota: k.daily_quota,
            used_today: state.quotas.used_today(k.id),
        },
        Principal::Anonymous { .. } => Me {
            principal: "anonymous".into(),
            name: None,
            scopes: vec![Scope::ContentRead.to_string()],
            rate_per_sec: crate::auth::ANONYMOUS_POLICY.rate,
            burst: crate::auth::ANONYMOUS_POLICY.burst,
            daily_quota: None,
            used_today: 0,
        },
    })
}

struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "api_key",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("csk_…")
                    .description(Some(
                        "`Authorization: Bearer csk_…` (or `X-API-Key`). Without a key, \
                         requests run as an anonymous principal with read-only access \
                         to content and a per-IP rate limit.",
                    ))
                    .build(),
            ),
        );
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "cristinadelisle.xyz API",
        version = "1.0.0",
        description = "Public API behind cristinadelisle.xyz: site content, privacy-preserving \
                       event ingestion into ClickHouse, aggregated stats. The same \
                       operations are exposed to AI agents over MCP at `/mcp`.",
        license(name = "MIT"),
    ),
    servers((url = "/v1", description = "Version 1")),
    modifiers(&SecurityAddon),
    tags(
        (name = "content", description = "Projects, talks and blog posts (public)"),
        (name = "events", description = "Event ingestion"),
        (name = "stats", description = "Aggregated analytics"),
        (name = "messages", description = "Contact-form inbox (owner only)"),
        (name = "account", description = "The calling API key"),
    )
)]
pub struct ApiDoc;

pub fn router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(list_projects))
        .routes(routes!(list_talks))
        .routes(routes!(list_posts))
        .routes(routes!(get_post))
        .routes(routes!(ingest_events))
        .routes(routes!(stats_summary))
        .routes(routes!(stats_timeseries))
        .routes(routes!(stats_top_paths))
        .routes(routes!(list_messages))
        .routes(routes!(me))
}
