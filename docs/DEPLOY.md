# Deploying

The target is one small VPS (for example an OVH VPS in France) running Docker,
with Cloudflare in front. The same image would run on Kubernetes unchanged:
it is stateless apart from `/data` (SQLite) and ClickHouse.

## 1. Server

```bash
# On the VPS (Debian/Ubuntu), as a sudo user
curl -fsSL https://get.docker.com | sh
git clone https://github.com/redchrision/cristina-website.git && cd cristina-website
cp .env.example .env && chmod 600 .env    # fill in the values
```

Generate the ClickHouse password with `openssl rand -base64 32`.

## 2. Cloudflare in front

**Recommended: Cloudflare Tunnel.** The VPS opens no inbound ports at all:
`cloudflared` connects out to Cloudflare, and Cloudflare terminates TLS.

1. Cloudflare dashboard → Zero Trust → Networks → Tunnels → *Create a tunnel*
   (type *Cloudflared*), and copy its token into `CLOUDFLARE_TUNNEL_TOKEN`.
2. Add a public hostname: `cristinadelisle.xyz` → `http://site:8080`.
3. Start everything:

   ```bash
   docker compose --profile tunnel up -d --build
   ```

4. Close the firewall, keeping SSH only:
   `ufw default deny incoming && ufw allow OpenSSH && ufw enable`.

Because the origin is reachable only through the tunnel, `TRUST_CLOUDFLARE=true`
is safe: the server then reads the client IP from `CF-Connecting-IP` (for
per-IP limits) and the country from `CF-IPCountry` (for stats). Without the
tunnel, leave it `false`, or anyone could spoof those headers.

Also in Cloudflare:

- SSL/TLS → *Full (strict)*; Edge certificates → *Always Use HTTPS*.
- Set `HSTS=true` in `.env` once HTTPS works.
- Caching: the server already sends `immutable` for `/assets/*`, `no-cache`
  for HTML and `no-store` for `/v1`, `/api` and `/mcp`, so the default cache
  rules do the right thing.
- Optionally add a WAF rate-limiting rule on `/api/contact` as a second layer.

## 3. API keys

```bash
docker compose exec site site-server keys create --name owner \
  --scopes content:read,events:write,stats:read,messages:read
```

## 4. Monitoring with Datadog

| Signal | Source | Datadog setup |
|---|---|---|
| Metrics | `:9090/metrics` (OpenMetrics) | Agent OpenMetrics check on `http://site:9090/metrics` |
| Traces | OTLP/HTTP when `OTEL_EXPORTER_OTLP_ENDPOINT` is set | Agent with OTLP ingest enabled; set `OTEL_EXPORTER_OTLP_ENDPOINT=http://datadog-agent:4318` |
| Logs | JSON on stdout, with `request_id` | Agent Docker log collection |

Metrics worth alerting on:

| Metric | Why |
|---|---|
| `http_server_request_duration_seconds` (histogram, by route and status) | p99 latency and error rate per route |
| `ingest_events_rejected_total{reason="queue_full"}` | ClickHouse can't keep up: load is being shed |
| `ingest_events_dropped_total` | Batches lost after 5 failed retries. **Should always be 0.** |
| `ingest_queue_depth` | Rising means trouble is coming |
| `ingest_flush_duration_seconds`, `ingest_flush_rows` | Writer health and batch efficiency |
| `api_rate_limited_total`, `api_quota_exhausted_total` | Abusive or misconfigured clients |
| `contact_messages_total{outcome}` | Stored vs honeypot vs rate-limited |

`/healthz` is liveness and `/readyz` is readiness (it checks SQLite and
ClickHouse), ready for a load balancer or Kubernetes probes.

## 5. Updating

CI pushes `ghcr.io/redchrision/cristina-website:latest` on every commit to
`main`. On the VPS:

```bash
docker compose pull site && docker compose up -d site
```

On `SIGTERM` the server stops accepting connections, finishes in-flight
requests, drains the event queue into ClickHouse and flushes quota counters
before exiting, so a restart loses no accepted events.

## Backups

- SQLite (keys, quota usage, contact messages): copy the `site-data` volume
  (`docker run --rm -v cristina-website_site-data:/data -v $PWD:/out alpine
  tar czf /out/site-data.tgz -C /data .`). The runtime image has no shell.
- ClickHouse: `BACKUP DATABASE site TO Disk('backups', 'site.zip')`, or
  `clickhouse-backup`.
