//! Contact form backend: validation, honeypot, per-IP rate limit, storage
//! with a retention limit, optional notification webhook.

use axum::{
    Json,
    extract::{Request, State},
    http::StatusCode,
    response::IntoResponse,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use utoipa::ToSchema;

use crate::{
    app::AppState,
    auth::client_ip,
    error::{ApiError, ApiResult},
    quota::Policy,
};

/// 3 messages in a burst, then one every 20 minutes per IP.
const CONTACT_POLICY: Policy = Policy::new(1.0 / 1200.0, 3.0);

#[derive(Debug, Deserialize, ToSchema)]
pub struct ContactForm {
    pub name: String,
    pub email: String,
    pub message: String,
    /// Honeypot: hidden from humans, so any value means a bot.
    #[serde(default)]
    pub website: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ContactAccepted {
    #[schema(example = "received")]
    pub status: &'static str,
}

fn plausible_email(e: &str) -> bool {
    let Some((local, domain)) = e.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !e.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl ContactForm {
    fn validate(&self) -> Result<(), String> {
        let name = self.name.trim();
        if name.is_empty() || name.chars().count() > 200 {
            return Err("name must be 1-200 characters".into());
        }
        if self.email.len() > 254 || !plausible_email(self.email.trim()) {
            return Err("email address is not valid".into());
        }
        let len = self.message.trim().chars().count();
        if !(10..=5_000).contains(&len) {
            return Err("message must be 10-5000 characters".into());
        }
        Ok(())
    }
}

#[utoipa::path(
    post,
    path = "/api/contact",
    tag = "site",
    request_body = ContactForm,
    responses(
        (status = 202, description = "Message stored", body = ContactAccepted),
        (status = 400, description = "Validation failed", body = crate::error::Problem),
        (status = 429, description = "Too many messages from this address", body = crate::error::Problem),
    )
)]
pub async fn submit(State(state): State<AppState>, req: Request) -> ApiResult<impl IntoResponse> {
    let ip = client_ip(&state, &req);
    let body = axum::body::to_bytes(req.into_body(), 16 * 1024)
        .await
        .map_err(|_| ApiError::BadRequest("request body too large".into()))?;
    let form: ContactForm = serde_json::from_slice(&body)
        .map_err(|e| ApiError::BadRequest(format!("invalid JSON: {e}")))?;

    let accepted = (
        StatusCode::ACCEPTED,
        Json(ContactAccepted { status: "received" }),
    );
    if !form.website.is_empty() {
        // Pretend success so bots learn nothing.
        metrics::counter!("contact_messages_total", "outcome" => "honeypot").increment(1);
        return Ok(accepted);
    }
    form.validate().map_err(ApiError::BadRequest)?;

    let d = state
        .limiter
        .check(&format!("contact:{ip}"), CONTACT_POLICY);
    if !d.allowed {
        metrics::counter!("contact_messages_total", "outcome" => "rate_limited").increment(1);
        return Err(ApiError::RateLimited {
            retry_after_secs: d.reset_secs,
        });
    }

    let public_id = uuid::Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO contact_messages (public_id, name, email, message, received_at)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(&public_id)
    .bind(form.name.trim())
    .bind(form.email.trim())
    .bind(form.message.trim())
    .bind(time::OffsetDateTime::now_utc().unix_timestamp())
    .execute(&state.db)
    .await?;
    metrics::counter!("contact_messages_total", "outcome" => "stored").increment(1);
    tracing::info!(message_id = %public_id, "contact message stored");

    if let Some(url) = state.cfg.contact_webhook_url.clone() {
        let http = state.http.clone();
        let name = form.name.trim().to_owned();
        tokio::spawn(async move {
            // Keep personal data out of third-party services: name only.
            let payload = serde_json::json!({ "text": format!("New contact message from {name}") });
            if let Err(e) = http.post(&url).json(&payload).send().await {
                tracing::warn!(error = %e, "contact webhook failed");
            }
        });
    }
    Ok(accepted)
}

#[derive(Debug, Serialize, sqlx::FromRow, ToSchema, JsonSchema)]
pub struct ContactMessage {
    pub id: String,
    pub name: String,
    pub email: String,
    pub message: String,
    /// Unix seconds.
    pub received_at: i64,
}

pub async fn list(db: &SqlitePool, limit: u32) -> sqlx::Result<Vec<ContactMessage>> {
    sqlx::query_as(
        "SELECT public_id AS id, name, email, message, received_at
         FROM contact_messages ORDER BY received_at DESC, id DESC LIMIT ?",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(db)
    .await
}

/// Delete messages past the retention period.
pub async fn purge_expired(db: &SqlitePool, retention_days: u32) -> sqlx::Result<u64> {
    let cutoff =
        time::OffsetDateTime::now_utc().unix_timestamp() - i64::from(retention_days) * 86_400;
    let res = sqlx::query("DELETE FROM contact_messages WHERE received_at < ?")
        .bind(cutoff)
        .execute(db)
        .await?;
    Ok(res.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_check_is_permissive_but_sane() {
        assert!(plausible_email("a@b.co"));
        assert!(plausible_email("first.last+tag@sub.example.fr"));
        assert!(!plausible_email("no-at.example.com"));
        assert!(!plausible_email("a@b"));
        assert!(!plausible_email("a b@c.de"));
        assert!(!plausible_email("@b.co"));
    }
}
