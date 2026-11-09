//! Read side of the analytics: aggregate queries over the `events_daily`
//! rollup. Shared by the `/v1/stats` endpoints and the MCP tools.

use clickhouse::Row;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

use crate::{error::ApiError, events::Sink};

#[derive(Debug, Clone, Deserialize, IntoParams, JsonSchema)]
#[into_params(parameter_in = Query)]
pub struct StatsQuery {
    /// Look-back window in days (1..=400).
    #[serde(default = "default_days")]
    #[param(default = 30, minimum = 1, maximum = 400)]
    pub days: u32,
    /// Event kind to aggregate.
    #[serde(default = "default_kind")]
    #[param(default = "pageview")]
    pub kind: String,
    /// Maximum rows for top-N queries (1..=100).
    #[serde(default = "default_limit")]
    #[param(default = 10, minimum = 1, maximum = 100)]
    pub limit: u32,
}

fn default_days() -> u32 {
    30
}
fn default_kind() -> String {
    "pageview".into()
}
fn default_limit() -> u32 {
    10
}

impl Default for StatsQuery {
    fn default() -> Self {
        Self {
            days: default_days(),
            kind: default_kind(),
            limit: default_limit(),
        }
    }
}

impl StatsQuery {
    fn validated(&self) -> Result<(u32, &str, u32), ApiError> {
        if !(1..=400).contains(&self.days) {
            return Err(ApiError::BadRequest("`days` must be between 1 and 400".into()));
        }
        if !(1..=100).contains(&self.limit) {
            return Err(ApiError::BadRequest("`limit` must be between 1 and 100".into()));
        }
        Ok((self.days, self.kind.as_str(), self.limit))
    }
}

#[derive(Debug, Row, Serialize, Deserialize, ToSchema, JsonSchema)]
pub struct DayCount {
    #[schema(example = "2026-11-09")]
    pub day: String,
    pub events: u64,
}

#[derive(Debug, Row, Serialize, Deserialize, ToSchema, JsonSchema)]
pub struct PathCount {
    pub path: String,
    pub events: u64,
}

#[derive(Debug, Row, Serialize, Deserialize, ToSchema, JsonSchema)]
pub struct KindCount {
    pub kind: String,
    pub events: u64,
}

#[derive(Debug, Row, Serialize, Deserialize, ToSchema, JsonSchema)]
pub struct CountryCount {
    pub country: String,
    pub events: u64,
}

#[derive(Debug, Serialize, ToSchema, JsonSchema)]
pub struct Summary {
    pub days: u32,
    pub total_events: u64,
    pub by_kind: Vec<KindCount>,
    pub top_countries: Vec<CountryCount>,
}

fn client(sink: &Sink) -> Result<&clickhouse::Client, ApiError> {
    match sink {
        Sink::ClickHouse(c) => Ok(c),
        _ => Err(ApiError::Unavailable("the analytics store (ClickHouse)")),
    }
}

pub async fn timeseries(sink: &Sink, q: &StatsQuery) -> Result<Vec<DayCount>, ApiError> {
    let (days, kind, _) = q.validated()?;
    let rows = client(sink)?
        .query(
            "SELECT toString(d) AS day, events FROM (
                 SELECT day AS d, sum(events) AS events
                 FROM events_daily
                 WHERE kind = ? AND day >= today() - ?
                 GROUP BY d ORDER BY d
                 WITH FILL FROM today() - ? TO today() + 1
             )",
        )
        .bind(kind)
        .bind(days - 1)
        .bind(days - 1)
        .fetch_all::<DayCount>()
        .await?;
    Ok(rows)
}

pub async fn top_paths(sink: &Sink, q: &StatsQuery) -> Result<Vec<PathCount>, ApiError> {
    let (days, kind, limit) = q.validated()?;
    let rows = client(sink)?
        .query(
            "SELECT path, sum(events) AS events
             FROM events_daily
             WHERE kind = ? AND day >= today() - ?
             GROUP BY path ORDER BY events DESC, path LIMIT ?",
        )
        .bind(kind)
        .bind(days - 1)
        .bind(limit)
        .fetch_all::<PathCount>()
        .await?;
    Ok(rows)
}

pub async fn summary(sink: &Sink, q: &StatsQuery) -> Result<Summary, ApiError> {
    let (days, _, limit) = q.validated()?;
    let ch = client(sink)?;
    let by_kind = ch
        .query(
            "SELECT kind, sum(events) AS events FROM events_daily
             WHERE day >= today() - ? GROUP BY kind ORDER BY events DESC",
        )
        .bind(days - 1)
        .fetch_all::<KindCount>()
        .await?;
    let top_countries = ch
        .query(
            "SELECT if(country = '', 'unknown', country) AS country, sum(events) AS events
             FROM events_daily WHERE day >= today() - ?
             GROUP BY country ORDER BY events DESC LIMIT ?",
        )
        .bind(days - 1)
        .bind(limit)
        .fetch_all::<CountryCount>()
        .await?;
    Ok(Summary {
        days,
        total_events: by_kind.iter().map(|k| k.events).sum(),
        by_kind,
        top_countries,
    })
}
