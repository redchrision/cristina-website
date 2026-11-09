//! Errors as RFC 9457 `application/problem+json`, so every client (browser,
//! SDK, MCP host) gets the same machine-readable shape.

use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Debug, Serialize, ToSchema)]
pub struct Problem {
    /// URI identifying the problem type.
    #[serde(rename = "type")]
    #[schema(example = "https://cristinadelisle.xyz/problems/rate-limited")]
    pub kind: String,
    #[schema(example = "Too Many Requests")]
    pub title: String,
    #[schema(example = 429)]
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Stable, machine-readable error code.
    #[schema(example = "rate_limited")]
    pub code: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("missing or invalid API key")]
    Unauthorized,
    #[error("this API key lacks the `{0}` scope")]
    Forbidden(&'static str),
    #[error("not found")]
    NotFound,
    #[error("rate limit exceeded")]
    RateLimited { retry_after_secs: u64 },
    #[error("daily quota exhausted")]
    QuotaExhausted { retry_after_secs: u64 },
    #[error("ingestion queue is full, retry shortly")]
    Overloaded,
    #[error("{0} is not configured on this deployment")]
    Unavailable(&'static str),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl ApiError {
    fn parts(&self) -> (StatusCode, &'static str) {
        match self {
            Self::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "insufficient_scope"),
            Self::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Self::RateLimited { .. } => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::QuotaExhausted { .. } => (StatusCode::TOO_MANY_REQUESTS, "quota_exhausted"),
            Self::Overloaded => (StatusCode::SERVICE_UNAVAILABLE, "overloaded"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
            Self::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Self::Internal(e.into())
    }
}

impl From<clickhouse::error::Error> for ApiError {
    fn from(e: clickhouse::error::Error) -> Self {
        Self::Internal(e.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = self.parts();
        let detail = match &self {
            // Never leak internals to clients; log them instead.
            Self::Internal(e) => {
                tracing::error!(error = ?e, "internal error");
                None
            }
            other => Some(other.to_string()),
        };
        let body = Problem {
            kind: format!(
                "https://cristinadelisle.xyz/problems/{}",
                code.replace('_', "-")
            ),
            title: status.canonical_reason().unwrap_or("Error").to_owned(),
            status: status.as_u16(),
            detail,
            code,
        };
        let mut res = (status, Json(body)).into_response();
        let headers = res.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        match self {
            Self::RateLimited { retry_after_secs } | Self::QuotaExhausted { retry_after_secs } => {
                headers.insert(header::RETRY_AFTER, retry_after_secs.max(1).into());
            }
            Self::Overloaded => {
                headers.insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            }
            Self::Unauthorized => {
                headers.insert(
                    header::WWW_AUTHENTICATE,
                    HeaderValue::from_static("Bearer realm=\"api\""),
                );
            }
            _ => {}
        }
        res
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
