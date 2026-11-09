use std::{sync::Arc, time::Duration};

use anyhow::Context;
use clap::{Parser, Subcommand};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use site_server::{
    app::{self, AppState, Inner},
    auth::{self, KeyStore, Scope},
    config::{LogFormat, ServeConfig},
    events::{self, Sink, WriterConfig},
    quota::{DailyQuotas, RateLimiter},
};
use tokio_util::sync::CancellationToken;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(version, about = "Backend for cristinadelisle.xyz")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the HTTP server.
    Serve(Box<ServeConfig>),
    /// Manage API keys.
    Keys {
        #[arg(
            long,
            env = "DATABASE_URL",
            default_value = "sqlite://data/site.db?mode=rwc",
            global = true
        )]
        database_url: String,
        #[command(subcommand)]
        cmd: KeysCmd,
    },
    /// Exit 0 if the server answers on /healthz (for container health checks;
    /// the runtime image has no shell or curl).
    Healthcheck {
        #[arg(long, default_value = "http://127.0.0.1:8080/healthz")]
        url: String,
    },
    /// Create the ClickHouse database and tables (idempotent; `serve` does
    /// this on startup too).
    ClickhouseInit {
        #[arg(long, env = "CLICKHOUSE_URL")]
        url: String,
        #[arg(long, env = "CLICKHOUSE_DATABASE", default_value = "site")]
        database: String,
        #[arg(long, env = "CLICKHOUSE_USER", default_value = "default")]
        user: String,
        #[arg(
            long,
            env = "CLICKHOUSE_PASSWORD",
            default_value = "",
            hide_env_values = true
        )]
        password: String,
    },
}

#[derive(Subcommand)]
enum KeysCmd {
    /// Create a key. The secret is printed once and never stored.
    Create {
        #[arg(long)]
        name: String,
        /// Space- or comma-separated: content:read events:write stats:read messages:read
        #[arg(long, value_delimiter = ',', num_args = 1.., default_value = "content:read")]
        scopes: Vec<String>,
        /// Sustained requests per second.
        #[arg(long, default_value_t = 20.0)]
        rate: f64,
        /// Burst size.
        #[arg(long, default_value_t = 100.0)]
        burst: f64,
        /// Daily quota in units (1 per request + 1 per event). Omit for unlimited.
        #[arg(long)]
        daily_quota: Option<u64>,
    },
    /// List keys (never shows secrets).
    List,
    /// Revoke a key by its prefix (e.g. csk_1a2b3c4d).
    Revoke { prefix: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Serve(cfg) => serve(*cfg).await,
        Cmd::Keys { database_url, cmd } => keys(&database_url, cmd).await,
        Cmd::Healthcheck { url } => {
            let res = reqwest::Client::new()
                .get(&url)
                .timeout(Duration::from_secs(3))
                .send()
                .await?;
            anyhow::ensure!(res.status().is_success(), "{url} answered {}", res.status());
            Ok(())
        }
        Cmd::ClickhouseInit {
            url,
            database,
            user,
            password,
        } => {
            let client = clickhouse_client(&url, &database, &user, &password);
            events::init_clickhouse(&client, &database).await?;
            println!("ClickHouse schema ready in database `{database}`");
            Ok(())
        }
    }
}

/// Keeps the OpenTelemetry provider alive; flushes spans on drop.
struct TelemetryGuard {
    #[cfg(feature = "otel")]
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        #[cfg(feature = "otel")]
        if let Some(p) = self.provider.take() {
            let _ = p.shutdown();
        }
    }
}

/// Logs to stdout (JSON in production); traces over OTLP/HTTP when
/// `OTEL_EXPORTER_OTLP_ENDPOINT` is set (e.g. a Datadog Agent with OTLP
/// ingestion enabled, or an OpenTelemetry Collector).
fn init_tracing(format: LogFormat) -> anyhow::Result<TelemetryGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,sqlx=warn"));
    let fmt = match format {
        LogFormat::Json => tracing_subscriber::fmt::layer()
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .boxed(),
        LogFormat::Pretty => tracing_subscriber::fmt::layer().boxed(),
    };

    #[cfg(feature = "otel")]
    {
        use opentelemetry::trace::TracerProvider as _;
        let provider = if std::env::var_os("OTEL_EXPORTER_OTLP_ENDPOINT").is_some() {
            let exporter = opentelemetry_otlp::SpanExporter::builder()
                .with_http()
                .build()
                .context("building OTLP exporter")?;
            Some(
                opentelemetry_sdk::trace::SdkTracerProvider::builder()
                    .with_batch_exporter(exporter)
                    .with_resource(
                        opentelemetry_sdk::Resource::builder()
                            .with_service_name("site-server")
                            .build(),
                    )
                    .build(),
            )
        } else {
            None
        };
        let otel = provider
            .as_ref()
            .map(|p| tracing_opentelemetry::layer().with_tracer(p.tracer("site-server")));
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt)
            .with(otel)
            .init();
        if provider.is_some() {
            tracing::info!("exporting traces over OTLP");
        }
        Ok(TelemetryGuard { provider })
    }
    #[cfg(not(feature = "otel"))]
    {
        tracing_subscriber::registry().with(filter).with(fmt).init();
        Ok(TelemetryGuard {})
    }
}

fn clickhouse_client(url: &str, database: &str, user: &str, password: &str) -> clickhouse::Client {
    clickhouse::Client::default()
        .with_url(url)
        .with_database(database)
        .with_user(user)
        .with_password(password)
}

fn install_metrics() -> anyhow::Result<PrometheusHandle> {
    let latency = [
        0.0005, 0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0,
    ];
    PrometheusBuilder::new()
        .set_buckets_for_metric(Matcher::Suffix("duration_seconds".into()), &latency)?
        .set_buckets_for_metric(
            Matcher::Full("ingest_flush_rows".into()),
            &[
                1.0, 10.0, 100.0, 1_000.0, 5_000.0, 10_000.0, 20_000.0, 50_000.0,
            ],
        )?
        .install_recorder()
        .context("installing Prometheus recorder")
}

async fn serve(cfg: ServeConfig) -> anyhow::Result<()> {
    let _telemetry = init_tracing(cfg.log_format)?;
    let prom = install_metrics()?;

    let db = app::open_db(&cfg.database_url)
        .await
        .context("opening SQLite")?;

    let sink = match &cfg.clickhouse_url {
        Some(url) => {
            let client = clickhouse_client(
                url,
                &cfg.clickhouse_database,
                &cfg.clickhouse_user,
                &cfg.clickhouse_password,
            );
            // Don't refuse to start if ClickHouse is briefly down: the writer
            // retries, and /readyz reports it.
            match events::init_clickhouse(&client, &cfg.clickhouse_database).await {
                Ok(()) => tracing::info!(%url, "ClickHouse schema ready"),
                Err(e) => {
                    tracing::warn!(error = %e, "ClickHouse init failed; will retry on writes")
                }
            }
            Sink::ClickHouse(Box::new(client))
        }
        None => {
            tracing::warn!("CLICKHOUSE_URL not set: events are accepted and discarded");
            Sink::Discard
        }
    };

    let (ingest, writer) = events::spawn(
        sink.clone(),
        WriterConfig {
            capacity: cfg.ingest_queue_capacity,
            max_rows: cfg.ingest_batch_max_rows,
            flush_interval: cfg.flush_interval(),
        },
    );

    let keys = KeyStore::default();
    let n = keys.reload(&db).await?;
    let quotas = DailyQuotas::default();
    quotas.load(&db).await?;
    tracing::info!(active_keys = n, "API keys loaded");

    let bind = cfg.bind;
    let metrics_bind = cfg.metrics_bind;
    let state = AppState(Arc::new(Inner {
        cfg,
        db,
        keys,
        limiter: RateLimiter::default(),
        quotas,
        ingest,
        sink,
        http: reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?,
    }));

    let shutdown = CancellationToken::new();
    let background = app::spawn_background(state.clone(), shutdown.clone());
    let metrics_task = tokio::spawn(serve_metrics(metrics_bind, prom, shutdown.clone()));

    let router = app::router(state.clone(), shutdown.clone());
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("binding {bind}"))?;
    tracing::info!(%bind, %metrics_bind, "listening");

    let token = shutdown.clone();
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        shutdown_signal().await;
        tracing::info!("shutdown signal received, draining");
        token.cancel();
    })
    .await?;

    // Drop the last Ingestor so the writer drains the queue and exits.
    let _ = background.await;
    drop(state);
    if tokio::time::timeout(Duration::from_secs(20), writer)
        .await
        .is_err()
    {
        tracing::warn!("event writer did not drain within 20 s");
    }
    let _ = metrics_task.await;
    tracing::info!("bye");
    Ok(())
}

async fn serve_metrics(
    bind: std::net::SocketAddr,
    prom: PrometheusHandle,
    shutdown: CancellationToken,
) {
    use axum::{Router, routing::get};
    let upkeep = prom.clone();
    let st = shutdown.clone();
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        loop {
            tokio::select! {
                _ = st.cancelled() => break,
                _ = tick.tick() => upkeep.run_upkeep(),
            }
        }
    });
    let app = Router::new().route("/metrics", get(move || std::future::ready(prom.render())));
    match tokio::net::TcpListener::bind(bind).await {
        Ok(l) => {
            let _ = axum::serve(l, app)
                .with_graceful_shutdown(async move { shutdown.cancelled().await })
                .await;
        }
        Err(e) => tracing::error!(error = %e, %bind, "metrics listener failed"),
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

/// prefix, name, scopes, rate, burst, daily quota, revoked at
type KeyListRow = (String, String, String, f64, f64, Option<i64>, Option<i64>);

async fn keys(database_url: &str, cmd: KeysCmd) -> anyhow::Result<()> {
    let db = app::open_db(database_url).await?;
    match cmd {
        KeysCmd::Create {
            name,
            scopes,
            rate,
            burst,
            daily_quota,
        } => {
            let scopes: Vec<Scope> = scopes
                .iter()
                .flat_map(|s| s.split_whitespace())
                .map(|s| s.parse().map_err(anyhow::Error::msg))
                .collect::<anyhow::Result<_>>()?;
            anyhow::ensure!(!scopes.is_empty(), "at least one scope is required");
            anyhow::ensure!(
                rate > 0.0 && burst >= 1.0,
                "rate must be > 0 and burst >= 1"
            );
            let (key, prefix) = auth::generate_key();
            let scope_str = scopes
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            sqlx::query(
                "INSERT INTO api_keys (name, prefix, secret_hash, scopes, rate_per_sec, burst, daily_quota, created_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&name)
            .bind(&prefix)
            .bind(auth::hash_key(&key))
            .bind(&scope_str)
            .bind(rate)
            .bind(burst)
            .bind(daily_quota.map(|q| q as i64))
            .bind(time::OffsetDateTime::now_utc().unix_timestamp())
            .execute(&db)
            .await?;
            eprintln!("Created key `{name}` ({prefix}) with scopes: {scope_str}");
            eprintln!(
                "Store it now; it cannot be shown again. A running server picks it up within 30 s."
            );
            println!("{key}");
        }
        KeysCmd::List => {
            let rows: Vec<KeyListRow> = sqlx::query_as(
                "SELECT prefix, name, scopes, rate_per_sec, burst, daily_quota, revoked_at
                 FROM api_keys ORDER BY id",
            )
            .fetch_all(&db)
            .await?;
            println!(
                "{:<14} {:<20} {:<8} {:<7} {:<12} {:<8} SCOPES",
                "PREFIX", "NAME", "RATE/S", "BURST", "DAILY", "STATUS"
            );
            for (prefix, name, scopes, rate, burst, quota, revoked) in rows {
                println!(
                    "{prefix:<14} {name:<20} {rate:<8} {burst:<7} {:<12} {:<8} {scopes}",
                    quota.map_or("unlimited".into(), |q| q.to_string()),
                    if revoked.is_some() {
                        "revoked"
                    } else {
                        "active"
                    },
                );
            }
        }
        KeysCmd::Revoke { prefix } => {
            let res = sqlx::query(
                "UPDATE api_keys SET revoked_at = ? WHERE prefix = ? AND revoked_at IS NULL",
            )
            .bind(time::OffsetDateTime::now_utc().unix_timestamp())
            .bind(&prefix)
            .execute(&db)
            .await?;
            anyhow::ensure!(
                res.rows_affected() == 1,
                "no active key with prefix {prefix}"
            );
            println!("Revoked {prefix}. A running server stops accepting it within 30 s.");
        }
    }
    Ok(())
}
