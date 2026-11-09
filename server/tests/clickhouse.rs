//! End-to-end ingestion against a real ClickHouse. Runs only when
//! `CLICKHOUSE_URL` is set (CI starts a ClickHouse service container):
//!
//! ```sh
//! CLICKHOUSE_URL=http://127.0.0.1:8123 cargo test -p site-server --test clickhouse
//! ```

use std::time::Duration;

use site_server::{
    events::{self, EventContext, IngestEvent, Sink, WriterConfig},
    stats::{self, StatsQuery},
};

#[tokio::test]
async fn events_round_trip_through_clickhouse() {
    let Ok(url) = std::env::var("CLICKHOUSE_URL") else {
        eprintln!("CLICKHOUSE_URL not set; skipping");
        return;
    };
    let db = format!("site_test_{}", uuid::Uuid::new_v4().simple());
    let client = clickhouse::Client::default()
        .with_url(&url)
        .with_database(&db);
    events::init_clickhouse(&client, &db).await.unwrap();

    let sink = Sink::ClickHouse(Box::new(client.clone()));
    let (ingest, writer) = events::spawn(
        sink.clone(),
        WriterConfig {
            capacity: 10_000,
            max_rows: 500,
            flush_interval: Duration::from_millis(50),
        },
    );
    let ctx = EventContext {
        source: "test".into(),
        country: "FR".into(),
    };
    let now = events::now_ms();
    let rows = (0..1_000)
        .map(|i| {
            IngestEvent {
                kind: "pageview".into(),
                path: if i % 4 == 0 {
                    "/#/blog".into()
                } else {
                    "/".into()
                },
                ts: None,
                referrer: None,
                lang: Some("fr".into()),
                device: Some("mobile".into()),
                value: None,
            }
            .into_row(&ctx, now)
            .unwrap()
        })
        .collect();
    ingest.enqueue(rows).unwrap();
    drop(ingest);
    writer.await.unwrap();

    let raw: u64 = client
        .query("SELECT count() FROM events")
        .fetch_one()
        .await
        .unwrap();
    assert_eq!(raw, 1_000);

    let q = StatsQuery {
        days: 1,
        ..Default::default()
    };
    let top = stats::top_paths(&sink, &q).await.unwrap();
    assert_eq!(top[0].path, "/");
    assert_eq!(top[0].events, 750);
    assert_eq!(top[1].events, 250);

    let summary = stats::summary(&sink, &q).await.unwrap();
    assert_eq!(summary.total_events, 1_000);
    assert_eq!(summary.top_countries[0].country, "FR");

    let series = stats::timeseries(
        &sink,
        &StatsQuery {
            days: 7,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(series.len(), 7, "WITH FILL returns one row per day");
    assert_eq!(series.iter().map(|d| d.events).sum::<u64>(), 1_000);

    client
        .clone()
        .with_database("default")
        .query(&format!("DROP DATABASE `{db}`"))
        .execute()
        .await
        .unwrap();
}
