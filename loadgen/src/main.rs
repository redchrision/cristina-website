//! Small HTTP load generator with HDR latency histograms.
//!
//! Two modes:
//! * **closed loop** (`--concurrency N`): N workers send back-to-back. Finds
//!   max throughput.
//! * **open loop** (`--rate R`): requests are scheduled at a fixed rate and
//!   latency is measured from the *scheduled* send time, so a slow server
//!   cannot hide its stalls by slowing the load down (coordinated omission).
//!
//! ```sh
//! loadgen --url http://127.0.0.1:8080/v1/events --key csk_... \
//!         --events-per-request 50 --rate 2000 --duration 30
//! ```

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context;
use clap::Parser;
use hdrhistogram::Histogram;
use serde_json::json;

#[derive(Parser, Clone)]
#[command(about = "HTTP load generator with p50/p99/p99.9 latency")]
struct Args {
    #[arg(long)]
    url: String,
    /// Bearer API key.
    #[arg(long, env = "API_KEY")]
    key: Option<String>,
    /// POST a synthetic event batch of this size (0 = plain GET).
    #[arg(long, default_value_t = 0)]
    events_per_request: usize,
    /// Closed-loop workers (ignored when --rate is set).
    #[arg(long, default_value_t = 64)]
    concurrency: usize,
    /// Open-loop target, requests per second.
    #[arg(long)]
    rate: Option<u64>,
    /// Measured duration in seconds.
    #[arg(long, default_value_t = 30)]
    duration: u64,
    /// Unmeasured warm-up in seconds.
    #[arg(long, default_value_t = 3)]
    warmup: u64,
    /// Print a JSON summary instead of text.
    #[arg(long)]
    json: bool,
}

#[derive(Default)]
struct Stats {
    hist: Option<Histogram<u64>>,
    status: BTreeMap<String, u64>,
}

impl Stats {
    fn new() -> Self {
        Self {
            hist: Some(Histogram::new_with_bounds(1, 60_000_000, 3).expect("histogram bounds")),
            status: BTreeMap::new(),
        }
    }
    fn record(&mut self, micros: u64, status: String) {
        if let Some(h) = self.hist.as_mut() {
            h.saturating_record(micros.max(1));
        }
        *self.status.entry(status).or_default() += 1;
    }
}

fn body(n: usize, seq: u64) -> String {
    let events: Vec<_> = (0..n)
        .map(|i| {
            json!({
                "kind": if i % 10 == 0 { "click" } else { "pageview" },
                "path": format!("/bench/{}", (seq as usize * n + i) % 500),
                "lang": if i % 3 == 0 { "fr" } else { "en" },
                "device": "desktop",
                "referrer": "https://example.org/somewhere",
                "value": (i % 100) as f64,
            })
        })
        .collect();
    json!({ "events": events }).to_string()
}

async fn send(client: &reqwest::Client, args: &Args, seq: u64) -> String {
    let req = if args.events_per_request > 0 {
        client
            .post(&args.url)
            .header("content-type", "application/json")
            .body(body(args.events_per_request, seq))
    } else {
        client.get(&args.url)
    };
    let req = match &args.key {
        Some(k) => req.bearer_auth(k),
        None => req,
    };
    match req.send().await {
        Ok(res) => {
            let s = res.status().as_u16().to_string();
            let _ = res.bytes().await;
            s
        }
        Err(e) if e.is_timeout() => "timeout".into(),
        Err(_) => "conn_error".into(),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let client = reqwest::Client::builder()
        .pool_max_idle_per_host(4096)
        .timeout(Duration::from_secs(10))
        .build()?;
    let stats = Arc::new(Mutex::new(Stats::new()));

    let start = Instant::now();
    let measure_from = start + Duration::from_secs(args.warmup);
    let end = measure_from + Duration::from_secs(args.duration);

    let mut tasks = Vec::new();
    match args.rate {
        Some(rate) => {
            anyhow::ensure!(rate > 0, "--rate must be > 0");
            let interval = Duration::from_secs_f64(1.0 / rate as f64);
            let limit = Arc::new(tokio::sync::Semaphore::new(20_000));
            let mut seq = 0u64;
            let mut next = start;
            while next < end {
                tokio::time::sleep_until(next.into()).await;
                let scheduled = next;
                next += interval;
                seq += 1;
                let Ok(permit) = limit.clone().try_acquire_owned() else {
                    // Too many in flight: count as a client-side drop.
                    if scheduled >= measure_from {
                        stats.lock().unwrap().record(10_000_000, "client_overload".into());
                    }
                    continue;
                };
                let (client, args, stats) = (client.clone(), args.clone(), stats.clone());
                tasks.push(tokio::spawn(async move {
                    let status = send(&client, &args, seq).await;
                    if scheduled >= measure_from {
                        let us = scheduled.elapsed().as_micros() as u64;
                        stats.lock().unwrap().record(us, status);
                    }
                    drop(permit);
                }));
            }
        }
        None => {
            for w in 0..args.concurrency {
                let (client, args, stats) = (client.clone(), args.clone(), stats.clone());
                tasks.push(tokio::spawn(async move {
                    let mut seq = w as u64 * 1_000_000;
                    while Instant::now() < end {
                        let t0 = Instant::now();
                        let status = send(&client, &args, seq).await;
                        seq += 1;
                        if t0 >= measure_from {
                            stats.lock().unwrap().record(t0.elapsed().as_micros() as u64, status);
                        }
                    }
                }));
            }
        }
    }
    for t in tasks {
        t.await.context("worker panicked")?;
    }

    let s = stats.lock().unwrap();
    let h = s.hist.as_ref().unwrap();
    let total = h.len();
    let secs = args.duration as f64;
    let ok: u64 = s
        .status
        .iter()
        .filter(|(k, _)| k.starts_with('2'))
        .map(|(_, v)| v)
        .sum();
    let ms = |q: f64| h.value_at_quantile(q) as f64 / 1000.0;
    let summary = json!({
        "url": args.url,
        "mode": if args.rate.is_some() { "open-loop" } else { "closed-loop" },
        "target_rps": args.rate,
        "concurrency": if args.rate.is_none() { Some(args.concurrency) } else { None },
        "events_per_request": args.events_per_request,
        "duration_s": args.duration,
        "requests": total,
        "rps": (total as f64 / secs).round(),
        "ok_rps": (ok as f64 / secs).round(),
        "events_per_s": (ok as f64 * args.events_per_request as f64 / secs).round(),
        "latency_ms": {
            "p50": ms(0.50), "p90": ms(0.90), "p99": ms(0.99), "p999": ms(0.999),
            "max": h.max() as f64 / 1000.0,
        },
        "status": s.status,
    });
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("{} {} for {}s", summary["mode"], args.url, args.duration);
        println!(
            "  requests {total}  ({} req/s, {} ok req/s, {} events/s)",
            summary["rps"], summary["ok_rps"], summary["events_per_s"]
        );
        println!(
            "  latency ms  p50 {:.2}  p90 {:.2}  p99 {:.2}  p99.9 {:.2}  max {:.2}",
            ms(0.5),
            ms(0.9),
            ms(0.99),
            ms(0.999),
            h.max() as f64 / 1000.0
        );
        println!("  status {:?}", s.status);
    }
    Ok(())
}
