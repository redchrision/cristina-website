//! API keys, scopes and the `/v1` + `/mcp` authentication middleware.
//!
//! Keys look like `csk_1a2b3c4d_<40 hex chars>`. The `csk_1a2b3c4d` prefix is
//! not secret (it identifies the key in listings and logs); only the SHA-256
//! hash of the full key is stored, so a database leak does not leak keys.
//! Requests without a key are served as an anonymous principal with a small
//! per-IP rate limit and read-only access to public content.

use std::{
    collections::HashMap,
    fmt,
    net::{IpAddr, SocketAddr},
    str::FromStr,
    sync::{Arc, RwLock},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use crate::{
    app::AppState,
    error::ApiError,
    quota::{self, Decision, Policy},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Projects, talks, blog posts. Also granted to anonymous callers.
    ContentRead,
    /// Send analytics events.
    EventsWrite,
    /// Query aggregated analytics.
    StatsRead,
    /// Read contact-form messages (personal data: owner only).
    MessagesRead,
}

impl Scope {
    pub const ALL: [Scope; 4] = [
        Scope::ContentRead,
        Scope::EventsWrite,
        Scope::StatsRead,
        Scope::MessagesRead,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContentRead => "content:read",
            Self::EventsWrite => "events:write",
            Self::StatsRead => "stats:read",
            Self::MessagesRead => "messages:read",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Scope {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Scope::ALL
            .into_iter()
            .find(|sc| sc.as_str() == s)
            .ok_or_else(|| format!("unknown scope `{s}`"))
    }
}

#[derive(Debug, Clone)]
pub struct KeyRecord {
    pub id: i64,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<Scope>,
    pub policy: Policy,
    pub daily_quota: Option<u64>,
}

/// Who is calling. Inserted into request extensions by [`authenticate`].
#[derive(Debug, Clone)]
pub enum Principal {
    Key(Arc<KeyRecord>),
    Anonymous { ip: String },
}

pub const ANONYMOUS_POLICY: Policy = Policy::new(5.0, 20.0);

impl Principal {
    pub fn require(&self, scope: Scope) -> Result<(), ApiError> {
        let ok = match self {
            Self::Key(k) => k.scopes.contains(&scope),
            Self::Anonymous { .. } => scope == Scope::ContentRead,
        };
        if ok {
            Ok(())
        } else if matches!(self, Self::Anonymous { .. }) {
            Err(ApiError::Unauthorized)
        } else {
            Err(ApiError::Forbidden(scope.as_str()))
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::Key(k) => k.prefix.clone(),
            Self::Anonymous { .. } => "anonymous".into(),
        }
    }
}

pub fn hash_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

/// Generate a new key. Returns `(full_key, prefix)`.
pub fn generate_key() -> (String, String) {
    // Two v4 UUIDs = 244 bits from the OS CSPRNG.
    let a = uuid::Uuid::new_v4().simple().to_string();
    let b = uuid::Uuid::new_v4().simple().to_string();
    let prefix = format!("csk_{}", &a[..8]);
    let full = format!("{prefix}_{}{}", &a[8..], &b[..16]);
    (full, prefix)
}

/// In-memory view of the active keys, indexed by hash. Reloaded from SQLite
/// periodically so keys created or revoked with the CLI take effect without
/// a restart.
#[derive(Default)]
pub struct KeyStore {
    by_hash: RwLock<HashMap<String, Arc<KeyRecord>>>,
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    id: i64,
    name: String,
    prefix: String,
    secret_hash: String,
    scopes: String,
    rate_per_sec: f64,
    burst: f64,
    daily_quota: Option<i64>,
}

impl KeyStore {
    pub async fn reload(&self, db: &SqlitePool) -> sqlx::Result<usize> {
        let rows: Vec<KeyRow> = sqlx::query_as(
            "SELECT id, name, prefix, secret_hash, scopes, rate_per_sec, burst, daily_quota
             FROM api_keys WHERE revoked_at IS NULL",
        )
        .fetch_all(db)
        .await?;
        let map: HashMap<_, _> = rows
            .into_iter()
            .map(|r| {
                let rec = KeyRecord {
                    id: r.id,
                    name: r.name,
                    prefix: r.prefix,
                    scopes: r
                        .scopes
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect(),
                    policy: Policy::new(r.rate_per_sec, r.burst),
                    daily_quota: r.daily_quota.map(|q| q as u64),
                };
                (r.secret_hash, Arc::new(rec))
            })
            .collect();
        let n = map.len();
        *self.by_hash.write().expect("key store lock poisoned") = map;
        Ok(n)
    }

    pub fn lookup(&self, key: &str) -> Option<Arc<KeyRecord>> {
        let hash = hash_key(key);
        self.by_hash
            .read()
            .expect("key store lock poisoned")
            .get(&hash)
            .cloned()
    }
}

fn presented_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(v) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if let Some(token) = v
            .strip_prefix("Bearer ")
            .or_else(|| v.strip_prefix("bearer "))
        {
            return Some(token.trim());
        }
    }
    headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
}

/// Best-effort client IP. Behind Cloudflare the TCP peer is a Cloudflare edge,
/// so the real address comes from `CF-Connecting-IP` (only trusted when
/// configured, otherwise anyone could spoof it).
pub fn client_ip(state: &AppState, req: &Request) -> String {
    if state.cfg.trust_cloudflare
        && let Some(ip) = req
            .headers()
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<IpAddr>().ok())
    {
        return ip.to_string();
    }
    req.extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".into())
}

fn set_rate_headers(headers: &mut HeaderMap, d: &Decision) {
    headers.insert("ratelimit-limit", d.limit.into());
    headers.insert("ratelimit-remaining", d.remaining.into());
    headers.insert("ratelimit-reset", d.reset_secs.min(86_400).into());
}

/// Authenticate, rate-limit and count quota for every `/v1` and `/mcp`
/// request, then annotate the response with the limits that applied.
pub async fn authenticate(State(state): State<AppState>, mut req: Request, next: Next) -> Response {
    let principal = match presented_key(req.headers()) {
        Some(key) => match state.keys.lookup(key) {
            Some(rec) => Principal::Key(rec),
            None => {
                metrics::counter!("api_auth_failures_total").increment(1);
                return ApiError::Unauthorized.into_response();
            }
        },
        None => Principal::Anonymous {
            ip: client_ip(&state, &req),
        },
    };

    let (bucket_key, policy) = match &principal {
        Principal::Key(k) => (format!("k:{}", k.id), k.policy),
        Principal::Anonymous { ip } => (format!("ip:{ip}"), ANONYMOUS_POLICY),
    };
    let decision = state.limiter.check(&bucket_key, policy);
    if !decision.allowed {
        metrics::counter!("api_rate_limited_total", "principal" => principal.label()).increment(1);
        let mut res = ApiError::RateLimited {
            retry_after_secs: decision.reset_secs,
        }
        .into_response();
        set_rate_headers(res.headers_mut(), &decision);
        return res;
    }

    let mut quota_headers = None;
    if let Principal::Key(k) = &principal {
        match state.quotas.try_consume(k.id, 1, k.daily_quota) {
            Ok(used) => {
                quota_headers = k
                    .daily_quota
                    .map(|limit| (limit, limit.saturating_sub(used)))
            }
            Err(_) => {
                metrics::counter!("api_quota_exhausted_total", "principal" => principal.label())
                    .increment(1);
                let mut res = ApiError::QuotaExhausted {
                    retry_after_secs: quota::secs_until_utc_midnight(),
                }
                .into_response();
                if let Some(limit) = k.daily_quota {
                    res.headers_mut().insert("x-quota-limit", limit.into());
                    res.headers_mut().insert("x-quota-remaining", 0u64.into());
                }
                return res;
            }
        }
    }

    req.extensions_mut().insert(principal);
    let mut res = next.run(req).await;
    let headers = res.headers_mut();
    set_rate_headers(headers, &decision);
    if let Some((limit, remaining)) = quota_headers {
        headers.insert("x-quota-limit", limit.into());
        headers.insert("x-quota-remaining", remaining.into());
    }
    headers.insert("api-version", HeaderValue::from_static("1"));
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_keys_have_expected_shape_and_are_unique() {
        let (k1, p1) = generate_key();
        let (k2, _) = generate_key();
        assert!(k1.starts_with(&p1));
        assert_eq!(p1.len(), "csk_".len() + 8);
        assert_eq!(k1.len(), p1.len() + 1 + 24 + 16);
        assert_ne!(k1, k2);
        assert_ne!(hash_key(&k1), hash_key(&k2));
    }

    #[test]
    fn scopes_round_trip() {
        for s in Scope::ALL {
            assert_eq!(s.as_str().parse::<Scope>().unwrap(), s);
        }
        assert!("admin".parse::<Scope>().is_err());
    }

    #[test]
    fn anonymous_can_only_read_content() {
        let anon = Principal::Anonymous {
            ip: "1.2.3.4".into(),
        };
        assert!(anon.require(Scope::ContentRead).is_ok());
        assert!(matches!(
            anon.require(Scope::StatsRead),
            Err(ApiError::Unauthorized)
        ));
    }
}
