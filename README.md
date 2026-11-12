# cristinadelisle.xyz

My personal site, plus the backend behind it: a Rust service that serves the
site, exposes a versioned public API and an MCP server for AI agents, and
ingests privacy-preserving analytics events into ClickHouse.

```text
                     ┌──────────────────────── site-server (Rust · Axum · Tokio) ─────────────────────────┐
 browser ──HTTPS──►  │  /            React + TypeScript site (static, CSP, immutable asset caching)        │
 Cloudflare          │  /api/contact contact form ─► validation, honeypot, per-IP limit ─► SQLite          │
                     │  /v1/*        public API ─┐                                                         │
 API clients ──────► │  /mcp         MCP tools ──┴► auth (scoped keys) ─► token bucket ─► daily quota      │
 Claude Code ──────► │                               │                                                     │
                     │                               ├─ content (embedded /content/*.json)                 │
                     │                               ├─ events ─► bounded queue ─► batch writer ──────────►│──► ClickHouse
                     │                               └─ stats  ◄── events_daily rollup (materialized view) ◄│    (MergeTree +
                     │  :9090/metrics  Prometheus/OpenMetrics · OTLP traces · JSON logs                    │     SummingMergeTree)
                     └────────────────────────────────────────────────────────────────────────────────────┘
```

**Numbers** (details in [docs/BENCHMARKS.md](docs/BENCHMARKS.md)): on the live
6-vCPU OVHcloud VPS, **200 000 events/s** ingested at **p99 8.6 ms** with no
errors, and 34 756 req/s on content reads at p99 4 ms. Under overload the API
sheds load with explicit `503`s instead of growing memory.

## Repository layout

| Path | What |
|---|---|
| [`web/`](web/) | The site: React 18 + TypeScript (strict) + Vite |
| [`content/`](content/) | Projects, talks and blog posts as JSON, shared by the site and the API |
| [`server/`](server/) | `site-server`: the Rust backend |
| [`loadgen/`](loadgen/) | Load generator with HDR histograms (closed- and open-loop) |
| [`docs/API.md`](docs/API.md) | API guide: auth, scopes, quotas, versioning, errors, MCP |
| [`docs/DEPLOY.md`](docs/DEPLOY.md) | Deploying on a VPS (OVH) behind Cloudflare, monitoring with Datadog |
| [`docs/BENCHMARKS.md`](docs/BENCHMARKS.md) | Load-test method and results |

## Run it locally

Requires Node.js 22+, Rust 1.94+ and Docker.

```bash
# 1. ClickHouse
docker compose up -d clickhouse

# 2. Frontend build (the Rust server serves web/dist)
cd web && npm install && npm run build && cd ..

# 3. An API key for yourself (printed once)
cargo run -p site-server -- keys create --name me \
  --scopes content:read,events:write,stats:read,messages:read

# 4. The server
CLICKHOUSE_URL=http://127.0.0.1:8123 CLICKHOUSE_USER=site \
CLICKHOUSE_PASSWORD=local-dev-only cargo run -p site-server -- serve
```

Then open http://localhost:8080 (site) and http://localhost:8080/docs (API
reference). For frontend work with hot reload, run `npm run dev` in `web/`
while the server runs; Vite proxies `/api` and `/v1` to it.

Or run everything in containers: `docker compose up -d --build`.

### Tests

```bash
cargo test --workspace                  # unit + HTTP integration tests
CLICKHOUSE_URL=http://127.0.0.1:8123 \
  cargo test -p site-server --test clickhouse   # end-to-end against ClickHouse
cd web && npm run typecheck
```

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs all of these,
with ClickHouse as a service container, plus `cargo fmt --check`, clippy with
`-D warnings`, and a Docker build that is pushed to GHCR from `main`.

## Editing content

Edit the JSON in [`content/`](content/): see its README. Text that is part of
the interface (navigation, form labels, the About section and the legal page)
lives at the top of [`web/src/App.tsx`](web/src/App.tsx).

## Design decisions

- **One binary, one origin.** The site, API and MCP endpoint share a process
  and an origin: no CORS, one TLS certificate, one thing to deploy. The
  boundaries live in modules (`server/src/*.rs`), so pieces can be split out
  when load requires it.
- **Content as data, embedded at compile time.** The site and the API read
  the same JSON, so they cannot disagree, and serving content needs no
  database round trip.
- **Ingestion never waits on the database.** Handlers validate and enqueue;
  a writer task batches into ClickHouse. A full queue means `503` with
  `Retry-After`, never unbounded memory. See
  [BENCHMARKS.md](docs/BENCHMARKS.md#design-choices-behind-the-numbers).
- **Data minimisation by construction.** Events store no IP, no user agent,
  no cookie and no full referrer URL (host only). Country comes from
  Cloudflare's `CF-IPCountry` and is trusted only when configured. Contact
  messages are deleted after 180 days. The site's own page-view beacon is
  compiled out unless built with `VITE_ANALYTICS=on`, because the privacy
  notice currently says no audience measurement is installed.
- **Keys are hashed, scoped and limited.** Only SHA-256 hashes are stored.
  Each key has scopes, a token-bucket rate limit and an optional daily quota.
  Anonymous callers get read-only content access and a per-IP limit.
- **MCP is stateless.** `/mcp` runs in stateless JSON mode behind the same
  auth middleware as `/v1`, so any replica can answer any request and the
  endpoint scales like the REST API.
