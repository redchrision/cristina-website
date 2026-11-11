//! Runtime configuration. Every flag can also be set through an environment
//! variable, which is how the container and the deployment docs set it.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use clap::{Args, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum LogFormat {
    /// One JSON object per line (production, Datadog log pipeline).
    Json,
    /// Human-readable output for local development.
    Pretty,
}

#[derive(Debug, Clone, Args)]
pub struct ServeConfig {
    /// Public listener (site, /api, /v1, /mcp).
    #[arg(long, env = "BIND", default_value = "0.0.0.0:8080")]
    pub bind: SocketAddr,

    /// Internal listener for Prometheus/OpenMetrics scraping. Keep it off the
    /// public internet.
    #[arg(long, env = "METRICS_BIND", default_value = "127.0.0.1:9090")]
    pub metrics_bind: SocketAddr,

    /// Directory with the built frontend (`npm run build` in web/).
    #[arg(long, env = "STATIC_DIR", default_value = "web/dist")]
    pub static_dir: PathBuf,

    /// SQLite database for API keys, quota usage and contact messages.
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "sqlite://data/site.db?mode=rwc"
    )]
    pub database_url: String,

    /// ClickHouse HTTP endpoint. When unset, events are accepted and dropped
    /// and the stats endpoints answer 503.
    #[arg(long, env = "CLICKHOUSE_URL")]
    pub clickhouse_url: Option<String>,

    #[arg(long, env = "CLICKHOUSE_DATABASE", default_value = "site")]
    pub clickhouse_database: String,

    #[arg(long, env = "CLICKHOUSE_USER", default_value = "default")]
    pub clickhouse_user: String,

    #[arg(
        long,
        env = "CLICKHOUSE_PASSWORD",
        default_value = "",
        hide_env_values = true
    )]
    pub clickhouse_password: String,

    /// Bounded in-memory queue between the HTTP handlers and the ClickHouse
    /// writer. When it is full the API sheds load with 503 instead of growing
    /// memory without limit.
    #[arg(long, env = "INGEST_QUEUE_CAPACITY", default_value_t = 200_000)]
    pub ingest_queue_capacity: usize,

    /// Flush a batch to ClickHouse once it has this many rows...
    #[arg(long, env = "INGEST_BATCH_MAX_ROWS", default_value_t = 20_000)]
    pub ingest_batch_max_rows: usize,

    /// ...or once this much time has passed since the first row arrived.
    #[arg(long, env = "INGEST_FLUSH_INTERVAL_MS", default_value_t = 1_000)]
    pub ingest_flush_interval_ms: u64,

    /// Trust `CF-Connecting-IP` / `CF-IPCountry` (only when the origin is
    /// reachable exclusively through Cloudflare).
    #[arg(long, env = "TRUST_CLOUDFLARE", default_value_t = false)]
    pub trust_cloudflare: bool,

    /// Addresses of reverse proxies in front of this server, comma-separated
    /// (e.g. the cjdns address of the proxy machine). Only requests whose TCP
    /// peer is one of these may set the client IP via `X-Forwarded-For` or
    /// `X-Real-IP`; from anyone else those headers are ignored.
    #[arg(
        long,
        env = "TRUSTED_PROXIES",
        value_delimiter = ',',
        default_value = ""
    )]
    pub trusted_proxies: Vec<String>,

    /// Send `Strict-Transport-Security` (enable when served over HTTPS).
    #[arg(long, env = "HSTS", default_value_t = false)]
    pub hsts: bool,

    /// Host names the MCP endpoint accepts (DNS-rebinding protection),
    /// comma-separated. Loopback is always allowed.
    #[arg(long, env = "PUBLIC_HOSTS", value_delimiter = ',', default_value = "")]
    pub public_hosts: Vec<String>,

    /// Optional webhook (ntfy, Slack, Discord, ...) pinged when a contact
    /// message arrives. Only the sender's name is included, never the message.
    #[arg(long, env = "CONTACT_WEBHOOK_URL")]
    pub contact_webhook_url: Option<String>,

    /// Contact messages are deleted after this many days (GDPR storage
    /// limitation).
    #[arg(long, env = "CONTACT_RETENTION_DAYS", default_value_t = 180)]
    pub contact_retention_days: u32,

    #[arg(long, env = "LOG_FORMAT", value_enum, default_value = "pretty")]
    pub log_format: LogFormat,
}

impl ServeConfig {
    /// Parsed `TRUSTED_PROXIES`; invalid or empty entries are ignored.
    pub fn trusted_proxy_ips(&self) -> Vec<std::net::IpAddr> {
        self.trusted_proxies
            .iter()
            .filter_map(|s| s.trim().trim_matches(['[', ']']).parse().ok())
            .collect()
    }

    pub fn flush_interval(&self) -> Duration {
        Duration::from_millis(self.ingest_flush_interval_ms)
    }
}
