# Benchmarks

Measured on 2026-11-09 with the release build, using the `loadgen` crate in
this repository. Raw output is in [`bench-results/`](bench-results/).

## Setup

| | |
|---|---|
| Machine | 4 vCPU Intel Xeon @ 2.1 GHz, 16 GB RAM (cloud VM) |
| Topology | Server, ClickHouse 24.8 and the load generator **on the same 4 vCPUs**, over loopback |
| Server | `site-server` release build, `RUST_LOG=warn`, JSON logs, default ingest settings (queue 200 000 rows, batches of 20 000 rows or 1 s) |
| Auth | Every request carries an API key and goes through auth, rate limiting and quota accounting |

Because the load generator and ClickHouse compete for the same CPUs as the
server, these numbers are a **floor**, not a ceiling. They are benchmark
numbers on one machine, not production traffic.

## Results

### Event ingestion, open loop (fixed arrival rate)

`POST /v1/events`, 100 events per request, 30 s measured after a 3 s warm-up.
Latency is measured from the *scheduled* send time, so it is not hidden by
coordinated omission.

| Target | Events/s | p50 | p90 | p99 | p99.9 | Errors |
|---:|---:|---:|---:|---:|---:|---:|
| 1 000 req/s | 100 000 | 2.0 ms | 2.7 ms | 4.4 ms | 6.8 ms | 0 |
| 2 000 req/s | 200 000 | 1.9 ms | 3.0 ms | 5.9 ms | 10.7 ms | 0 |
| 2 500 req/s | **250 000** | 1.9 ms | 3.4 ms | **7.1 ms** | 18.1 ms | 0 |

Server memory (RSS) after these runs: about 112 MB.

ClickHouse row count after the runs matched `ingest_events_accepted_total`
exactly (16 000 000 accepted = 16 000 000 written = 16 000 000 rows): no event
that got a `202` was lost. Flushes averaged 20 000 rows and 49 ms each.

### Event ingestion, overload (closed loop, 32 workers)

Pushing as fast as possible (32 concurrent clients, 100 events per request)
saturates the ClickHouse writer. The bounded queue fills, and the API
**sheds load** instead of buffering without limit:

| Accepted | Rejected (503 + `Retry-After: 1`) | p99 (all responses) |
|---:|---:|---:|
| 266 000 events/s | 70 063 requests | 14.5 ms |

Accepted events were still all written (6 100 000 accepted = 6 100 000 rows).
Latency stays flat under overload because handlers never wait on ClickHouse.

### Content API (closed loop, 64 workers)

`GET /v1/projects` (JSON serialisation of the embedded content, with auth and
rate limiting on every call):

| Req/s | p50 | p99 | p99.9 |
|---:|---:|---:|---:|
| 40 000 | 1.5 ms | 3.5 ms | 4.9 ms |

### Stats query (closed loop, 16 workers)

`GET /v1/stats/top-paths?days=30` after ingesting 24.25 M raw events. Queries
read the `events_daily` rollup (500 rows), filled by a materialized view at
insert time, instead of scanning the raw table:

| Req/s | p50 | p99 |
|---:|---:|---:|
| 839 | 18 ms | 42 ms |

## Reproduce

```bash
docker compose up -d clickhouse
cargo build --release
export DATABASE_URL="sqlite://data/bench.db?mode=rwc"
KEY=$(./target/release/site-server keys create --name bench \
      --scopes content:read,events:write,stats:read --rate 1000000 --burst 1000000)
CLICKHOUSE_URL=http://127.0.0.1:8123 RUST_LOG=warn ./target/release/site-server serve &

./target/release/loadgen --url http://127.0.0.1:8080/v1/events --key "$KEY" \
  --events-per-request 100 --rate 2000 --duration 30
```

## Design choices behind the numbers

- **Handlers only enqueue.** Validation happens on the request path; the
  ClickHouse insert happens in a separate writer task. Request latency does
  not depend on database latency.
- **All-or-nothing batches.** `try_reserve_many(n)` reserves queue space for a
  whole batch before sending any of it, so a client never gets a partial
  accept to reconcile.
- **Bounded queue, explicit shedding.** When ClickHouse cannot keep up, clients
  get `503` with `Retry-After` within milliseconds, and memory stays bounded.
- **Few large inserts.** ClickHouse merges parts in the background; thousands
  of tiny inserts per second create too many parts. The writer coalesces
  rows into batches of up to 20 000 rows (or 1 s), sent in RowBinary.
- **Pre-aggregation.** A materialized view keeps a `SummingMergeTree` daily
  rollup, so dashboard queries read hundreds of rows, not millions.
- **Sharded rate limiter.** Token buckets sit in 32 mutex-protected shards,
  keyed by API key or IP, so concurrent requests rarely contend.
- **Quota accounting off the hot path.** Daily usage is counted in memory and
  flushed to SQLite every 5 s.

## What I would change at Pubstack scale (billions of requests/day)

- Put a durable log (Kafka/Redpanda) between the API and ClickHouse, so a
  ClickHouse outage costs latency, not data, and consumers can replay.
- Move rate-limit state to a shared store (Redis, or consistent hashing of
  keys to replicas) once there is more than one replica. In-process buckets
  are only exact per instance.
- Use ClickHouse `async_insert` or a dedicated inserter tier, and shard the
  table with `Distributed`.
- Serialise with a schema'd binary format (Protobuf, as OpenRTB adapters do)
  instead of JSON on the hottest paths.
