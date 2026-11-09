//! Event ingestion: HTTP handlers → bounded queue → batched ClickHouse inserts.
//!
//! ```text
//!  POST /v1/events ─┐                         ┌──────────────┐
//!  POST /api/collect├─► try_reserve_many(n) ─►│ mpsc (bounded)│─► writer task ─► INSERT (RowBinary)
//!                   │   full? → 503 + Retry-After └──────────────┘   flush on N rows or T ms
//! ```
//!
//! * Handlers never wait on ClickHouse: they validate, enqueue and return
//!   `202 Accepted`. p99 latency is therefore independent of the database.
//! * A batch is enqueued all-or-nothing (`try_reserve_many`), so a client
//!   never gets a partial accept.
//! * The queue is bounded: under overload the API sheds load explicitly
//!   instead of growing memory until the process is killed.
//! * ClickHouse prefers few large inserts over many small ones; the writer
//!   coalesces rows and flushes when a batch is full or the interval elapses.
//! * On shutdown the writer drains the queue before exiting.

use std::{sync::Arc, time::Duration};

use clickhouse::Row;
use serde::{Deserialize, Serialize};
use tokio::{sync::mpsc, task::JoinHandle, time::Instant};
use utoipa::ToSchema;

use crate::error::ApiError;

pub const MAX_BATCH: usize = 1_000;
const MAX_PATH: usize = 512;
const MAX_KIND: usize = 64;

/// One event as sent by API clients.
#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct IngestEvent {
    /// Event type, e.g. `pageview`, `click`, `bid_response`.
    #[schema(example = "pageview")]
    pub kind: String,
    /// Path or logical resource the event refers to.
    #[schema(example = "/#/blog/technovation-judge")]
    pub path: String,
    /// Client timestamp in Unix milliseconds. Defaults to receive time.
    /// Timestamps more than 24 h away from server time are clamped.
    #[serde(default)]
    pub ts: Option<i64>,
    /// Referrer URL; only its host is stored.
    #[serde(default)]
    pub referrer: Option<String>,
    /// Two-letter language code.
    #[serde(default)]
    #[schema(example = "fr")]
    pub lang: Option<String>,
    /// `mobile`, `tablet` or `desktop`.
    #[serde(default)]
    pub device: Option<String>,
    /// Free numeric measure (latency in ms, CPM, ...).
    #[serde(default)]
    pub value: Option<f64>,
}

/// Row layout of the `events` table, written with RowBinary.
#[derive(Debug, Clone, Row, Serialize, Deserialize, PartialEq)]
pub struct EventRow {
    /// DateTime64(3): Unix milliseconds.
    pub ts: i64,
    pub source: String,
    pub kind: String,
    pub path: String,
    pub referrer_host: String,
    pub lang: String,
    pub country: String,
    pub device: String,
    pub value: f64,
}

/// Context the server knows about a request, added to every event.
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    pub source: String,
    /// ISO country code from Cloudflare's `CF-IPCountry`, if trusted.
    pub country: String,
}

fn clean_token(s: Option<&str>, max: usize) -> String {
    s.unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        .take(max)
        .collect::<String>()
        .to_ascii_lowercase()
}

fn referrer_host(r: Option<&str>) -> String {
    let Some(r) = r else { return String::new() };
    let rest = r.split_once("://").map_or(r, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Drop credentials and port.
    let host = host.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    clean_token(Some(host), 253)
}

impl IngestEvent {
    /// Validate and normalise into a storable row. Nothing identifying is
    /// kept: no IP, no user agent, no full referrer URL, no cookie.
    pub fn into_row(self, ctx: &EventContext, now_ms: i64) -> Result<EventRow, String> {
        let kind = clean_token(Some(&self.kind), MAX_KIND);
        if kind.is_empty() {
            return Err("`kind` must be a non-empty identifier".into());
        }
        if self.path.is_empty() || self.path.len() > MAX_PATH {
            return Err(format!("`path` must be 1..={MAX_PATH} bytes"));
        }
        const DAY_MS: i64 = 86_400_000;
        let ts = self
            .ts
            .unwrap_or(now_ms)
            .clamp(now_ms - DAY_MS, now_ms + DAY_MS);
        let device = match self.device.as_deref() {
            Some(d @ ("mobile" | "tablet" | "desktop")) => d.to_owned(),
            _ => String::new(),
        };
        let value = self.value.filter(|v| v.is_finite()).unwrap_or(0.0);
        Ok(EventRow {
            ts,
            source: ctx.source.clone(),
            kind,
            path: self.path,
            referrer_host: referrer_host(self.referrer.as_deref()),
            lang: clean_token(self.lang.as_deref(), 2),
            country: ctx.country.clone(),
            device,
            value,
        })
    }
}

pub fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

/// Where batches go.
#[derive(Clone)]
pub enum Sink {
    ClickHouse(clickhouse::Client),
    /// Keeps rows in memory (tests).
    Memory(Arc<std::sync::Mutex<Vec<EventRow>>>),
    /// No analytics store configured: accept and drop.
    Discard,
}

impl Sink {
    async fn write(&self, rows: &[EventRow]) -> anyhow::Result<()> {
        match self {
            Self::ClickHouse(client) => {
                let mut insert = client.insert::<EventRow>("events").await?;
                for row in rows {
                    insert.write(row).await?;
                }
                insert.end().await?;
                Ok(())
            }
            Self::Memory(store) => {
                store.lock().expect("memory sink poisoned").extend_from_slice(rows);
                Ok(())
            }
            Self::Discard => Ok(()),
        }
    }
}

/// Handle used by request handlers to enqueue events.
#[derive(Clone)]
pub struct Ingestor {
    tx: mpsc::Sender<EventRow>,
    capacity: usize,
}

impl Ingestor {
    /// Enqueue a batch atomically, or fail fast when the queue is full.
    pub fn enqueue(&self, rows: Vec<EventRow>) -> Result<usize, ApiError> {
        let n = rows.len();
        match self.tx.try_reserve_many(n) {
            Ok(permits) => {
                for (permit, row) in permits.zip(rows) {
                    permit.send(row);
                }
                metrics::counter!("ingest_events_accepted_total").increment(n as u64);
                metrics::gauge!("ingest_queue_depth").set((self.capacity - self.tx.capacity()) as f64);
                Ok(n)
            }
            Err(_) => {
                metrics::counter!("ingest_events_rejected_total", "reason" => "queue_full")
                    .increment(n as u64);
                Err(ApiError::Overloaded)
            }
        }
    }
}

pub struct WriterConfig {
    pub capacity: usize,
    pub max_rows: usize,
    pub flush_interval: Duration,
}

/// Start the writer task. Dropping every [`Ingestor`] clone closes the queue;
/// the task then flushes what is left and exits, so await the handle on
/// shutdown.
pub fn spawn(sink: Sink, cfg: WriterConfig) -> (Ingestor, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(cfg.capacity);
    let handle = tokio::spawn(writer(rx, sink, cfg.max_rows, cfg.flush_interval));
    (
        Ingestor {
            tx,
            capacity: cfg.capacity,
        },
        handle,
    )
}

async fn writer(mut rx: mpsc::Receiver<EventRow>, sink: Sink, max_rows: usize, interval: Duration) {
    let mut batch: Vec<EventRow> = Vec::with_capacity(max_rows);
    loop {
        // Wait for the first row of a new batch...
        if rx.recv_many(&mut batch, max_rows).await == 0 {
            break; // channel closed and drained
        }
        // ...then keep collecting until the batch is full or the deadline hits.
        let deadline = Instant::now() + interval;
        while batch.len() < max_rows {
            let room = max_rows - batch.len();
            match tokio::time::timeout_at(deadline, rx.recv_many(&mut batch, room)).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        flush(&sink, &mut batch).await;
    }
    flush(&sink, &mut batch).await;
    tracing::info!("event writer drained and stopped");
}

async fn flush(sink: &Sink, batch: &mut Vec<EventRow>) {
    if batch.is_empty() {
        return;
    }
    let n = batch.len();
    let started = std::time::Instant::now();
    let mut backoff = Duration::from_millis(200);
    for attempt in 1..=5 {
        match sink.write(batch).await {
            Ok(()) => {
                metrics::histogram!("ingest_flush_duration_seconds").record(started.elapsed());
                metrics::histogram!("ingest_flush_rows").record(n as f64);
                metrics::counter!("ingest_events_written_total").increment(n as u64);
                tracing::debug!(rows = n, ms = started.elapsed().as_millis() as u64, "flushed");
                batch.clear();
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, attempt, rows = n, "flush failed, retrying");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(5));
            }
        }
    }
    tracing::error!(rows = n, "dropping batch after repeated ClickHouse failures");
    metrics::counter!("ingest_events_dropped_total").increment(n as u64);
    batch.clear();
}

/// Idempotent ClickHouse schema.
pub const CLICKHOUSE_SCHEMA: &[&str] = &[
    r"CREATE TABLE IF NOT EXISTS events
(
    ts            DateTime64(3, 'UTC'),
    received_at   DateTime64(3, 'UTC') DEFAULT now64(3),
    source        LowCardinality(String),
    kind          LowCardinality(String),
    path          String,
    referrer_host LowCardinality(String),
    lang          LowCardinality(String),
    country       LowCardinality(String),
    device        LowCardinality(String),
    value         Float64
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(ts)
ORDER BY (kind, toDate(ts), path, ts)
TTL toDateTime(ts) + INTERVAL 13 MONTH",
    // Pre-aggregated daily rollup: dashboards read this instead of scanning
    // raw events.
    r"CREATE TABLE IF NOT EXISTS events_daily
(
    day     Date,
    kind    LowCardinality(String),
    path    String,
    country LowCardinality(String),
    events  UInt64,
    value   Float64
)
ENGINE = SummingMergeTree
PARTITION BY toYYYYMM(day)
ORDER BY (kind, day, path, country)",
    r"CREATE MATERIALIZED VIEW IF NOT EXISTS events_daily_mv TO events_daily AS
SELECT toDate(ts) AS day, kind, path, country, count() AS events, sum(value) AS value
FROM events
GROUP BY day, kind, path, country",
];

pub async fn init_clickhouse(client: &clickhouse::Client, database: &str) -> anyhow::Result<()> {
    client
        .clone()
        .with_database("default")
        .query(&format!("CREATE DATABASE IF NOT EXISTS `{database}`"))
        .execute()
        .await?;
    for ddl in CLICKHOUSE_SCHEMA {
        client.query(ddl).execute().await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(kind: &str, path: &str) -> IngestEvent {
        IngestEvent {
            kind: kind.into(),
            path: path.into(),
            ts: None,
            referrer: Some("https://user:pw@news.ycombinator.com:443/item?id=1".into()),
            lang: Some("FR-fr".into()),
            device: Some("toaster".into()),
            value: Some(f64::NAN),
        }
    }

    #[test]
    fn rows_are_normalised_and_minimised() {
        let row = ev("PageView", "/").into_row(&EventContext::default(), 1_000).unwrap();
        assert_eq!(row.kind, "pageview");
        assert_eq!(row.referrer_host, "news.ycombinator.com");
        assert_eq!(row.lang, "fr");
        assert_eq!(row.device, "");
        assert_eq!(row.value, 0.0);
        assert_eq!(row.ts, 1_000);
    }

    #[test]
    fn invalid_events_are_rejected() {
        assert!(ev("", "/").into_row(&EventContext::default(), 0).is_err());
        assert!(ev("x", "").into_row(&EventContext::default(), 0).is_err());
        assert!(ev("x", &"a".repeat(600)).into_row(&EventContext::default(), 0).is_err());
    }

    #[tokio::test]
    async fn writer_batches_and_drains_on_shutdown() {
        let store = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (ing, handle) = spawn(
            Sink::Memory(store.clone()),
            WriterConfig {
                capacity: 100,
                max_rows: 10,
                flush_interval: Duration::from_secs(60),
            },
        );
        let rows: Vec<_> = (0..25)
            .map(|i| ev("x", &format!("/{i}")).into_row(&EventContext::default(), 0).unwrap())
            .collect();
        ing.enqueue(rows).unwrap();
        drop(ing);
        handle.await.unwrap();
        assert_eq!(store.lock().unwrap().len(), 25);
    }

    #[tokio::test]
    async fn full_queue_rejects_whole_batch() {
        let (ing, _h) = spawn(
            Sink::Discard,
            WriterConfig {
                capacity: 5,
                max_rows: 5,
                flush_interval: Duration::from_secs(60),
            },
        );
        let rows: Vec<_> = (0..6)
            .map(|_| ev("x", "/").into_row(&EventContext::default(), 0).unwrap())
            .collect();
        assert!(matches!(ing.enqueue(rows), Err(ApiError::Overloaded)));
    }
}
