//! MCP server (Streamable HTTP at `/mcp`) exposing the same operations as the
//! REST API to AI agents such as Claude Code:
//!
//! ```sh
//! claude mcp add --transport http cristina https://cristinadelisle.xyz/mcp \
//!   --header "Authorization: Bearer csk_..."
//! ```
//!
//! Requests pass through the same authentication, rate-limit and quota
//! middleware as `/v1`; each tool checks the scope it needs, so an agent can
//! only do what its key allows.

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::{common::Extension, router::tool::ToolRouter, wrapper::Parameters},
    model::{Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{
    app::AppState,
    auth::{Principal, Scope},
    contact,
    content::{self, Lang},
    error::ApiError,
    stats::{self, StatsQuery},
};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LangArgs {
    /// `en` or `fr`. Omit to get both languages.
    pub lang: Option<Lang>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PostsArgs {
    pub lang: Option<Lang>,
    /// Only posts with this tag, e.g. `open source`.
    pub tag: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PostArgs {
    /// The post slug, as returned by `list_blog_posts`.
    pub slug: String,
    pub lang: Option<Lang>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatsArgs {
    /// Look-back window in days (1-400). Default 30.
    pub days: Option<u32>,
    /// Event kind. Default `pageview`.
    pub kind: Option<String>,
    /// Max rows for top-N results (1-100). Default 10.
    pub limit: Option<u32>,
}

impl From<StatsArgs> for StatsQuery {
    fn from(a: StatsArgs) -> Self {
        let d = StatsQuery::default();
        Self {
            days: a.days.unwrap_or(d.days),
            kind: a.kind.unwrap_or(d.kind),
            limit: a.limit.unwrap_or(d.limit),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MessagesArgs {
    /// Number of most recent messages (1-200). Default 20.
    pub limit: Option<u32>,
}

#[derive(Clone)]
pub struct SiteMcp {
    state: AppState,
    tool_router: ToolRouter<Self>,
}

fn principal(parts: &http::request::Parts) -> Result<Principal, ErrorData> {
    parts
        .extensions
        .get::<Principal>()
        .cloned()
        .ok_or_else(|| ErrorData::internal_error("missing principal", None))
}

fn to_mcp(e: ApiError) -> ErrorData {
    match e {
        ApiError::Unauthorized | ApiError::Forbidden(_) => {
            ErrorData::invalid_request(e.to_string(), None)
        }
        ApiError::BadRequest(m) => ErrorData::invalid_params(m, None),
        other => ErrorData::internal_error(other.to_string(), None),
    }
}

fn json<T: Serialize>(v: &T) -> Result<String, ErrorData> {
    serde_json::to_string_pretty(v).map_err(|e| ErrorData::internal_error(e.to_string(), None))
}

#[tool_router]
impl SiteMcp {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List Cristina DeLisle's projects (name, role, description, tags, links)."
    )]
    async fn list_projects(
        &self,
        Parameters(a): Parameters<LangArgs>,
    ) -> Result<String, ErrorData> {
        json(&content::projects(a.lang))
    }

    #[tool(description = "List Cristina DeLisle's talks and panels, newest first.")]
    async fn list_talks(&self, Parameters(a): Parameters<LangArgs>) -> Result<String, ErrorData> {
        json(&content::talks(a.lang))
    }

    #[tool(
        description = "List blog posts (title, date, excerpt, tags), optionally filtered by tag."
    )]
    async fn list_blog_posts(
        &self,
        Parameters(a): Parameters<PostsArgs>,
    ) -> Result<String, ErrorData> {
        json(&content::posts(a.lang, a.tag.as_deref()))
    }

    #[tool(description = "Get the full text of one blog post by slug.")]
    async fn get_blog_post(
        &self,
        Parameters(a): Parameters<PostArgs>,
    ) -> Result<String, ErrorData> {
        match content::post(&a.slug, a.lang) {
            Some(p) => json(&p),
            None => Err(ErrorData::invalid_params(
                format!("no post with slug `{}`", a.slug),
                None,
            )),
        }
    }

    #[tool(
        description = "Traffic summary: total events by kind and top countries. Requires the stats:read scope."
    )]
    async fn traffic_summary(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(a): Parameters<StatsArgs>,
    ) -> Result<String, ErrorData> {
        principal(&parts)?
            .require(Scope::StatsRead)
            .map_err(to_mcp)?;
        json(
            &stats::summary(&self.state.sink, &a.into())
                .await
                .map_err(to_mcp)?,
        )
    }

    #[tool(description = "Daily event counts for the last N days. Requires the stats:read scope.")]
    async fn traffic_timeseries(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(a): Parameters<StatsArgs>,
    ) -> Result<String, ErrorData> {
        principal(&parts)?
            .require(Scope::StatsRead)
            .map_err(to_mcp)?;
        json(
            &stats::timeseries(&self.state.sink, &a.into())
                .await
                .map_err(to_mcp)?,
        )
    }

    #[tool(description = "Most visited paths over the last N days. Requires the stats:read scope.")]
    async fn top_pages(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(a): Parameters<StatsArgs>,
    ) -> Result<String, ErrorData> {
        principal(&parts)?
            .require(Scope::StatsRead)
            .map_err(to_mcp)?;
        json(
            &stats::top_paths(&self.state.sink, &a.into())
                .await
                .map_err(to_mcp)?,
        )
    }

    #[tool(
        description = "Most recent contact-form messages. Contains personal data; requires the messages:read scope."
    )]
    async fn list_contact_messages(
        &self,
        Extension(parts): Extension<http::request::Parts>,
        Parameters(a): Parameters<MessagesArgs>,
    ) -> Result<String, ErrorData> {
        principal(&parts)?
            .require(Scope::MessagesRead)
            .map_err(to_mcp)?;
        let msgs = contact::list(&self.state.db, a.limit.unwrap_or(20))
            .await
            .map_err(|e| to_mcp(e.into()))?;
        json(&msgs)
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for SiteMcp {
    fn get_info(&self) -> ServerConfig {
        let mut info = Implementation::from_build_env();
        info.name = "cristinadelisle-site".into();
        info.title = Some("cristinadelisle.xyz".into());
        info.version = env!("CARGO_PKG_VERSION").into();
        info.website_url = Some("https://cristinadelisle.xyz".into());
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(info)
            .with_instructions(
                "Tools for Cristina DeLisle's personal website: her projects, talks and blog \
             posts (public), plus site analytics and the contact inbox (scoped API keys).",
            )
    }
}
