# Deploying

There are two ways to host the site:

| | Static (current) | Full server |
|---|---|---|
| Where | Porkbun Static Hosting | A small VPS (e.g. OVH) with Docker |
| What runs | The website only | Website, contact form, `/v1` API, `/docs`, MCP, ClickHouse |
| Contact | Email address shown instead of the form | Contact form |

## Static: Porkbun Static Hosting

The workflow [`.github/workflows/static-site.yml`](../.github/workflows/static-site.yml)
builds the site on every push to `main` (with `VITE_CONTACT_FORM=off`) and
publishes the files to the **`porkbun-site`** branch. Porkbun deploys that
branch.

One-time setup in Porkbun:

1. Account → Domain Management → `cristinadelisle.xyz` → **Static Hosting**
   icon (Website column).
2. Scroll to **GitHub Connect** → *connect*. On GitHub choose *Only select
   repositories* → `cristina-website` → *Install*.
3. Back on Porkbun, under GitHub Connect choose repository
   `redchrision/cristina-website` and branch **`porkbun-site`**.

After that, every merge into `main` updates the live site within a few
minutes. To redeploy by hand: GitHub → Actions → *Static site (Porkbun)* →
*Run workflow*.

## Full server behind a reverse proxy (cjdns)

When a separate machine terminates HTTPS and forwards visitors to this server
(for example over cjdns), use the proxy override, which runs the site on the
host's network so it listens on every interface and sees the proxy's real
address:

```bash
git clone https://github.com/redchrision/cristina-website.git /opt/cristina-website
cd /opt/cristina-website
cat > .env <<EOT
CLICKHOUSE_PASSWORD=$(openssl rand -hex 24)
PUBLIC_HOSTS=cristinadelisle.xyz,www.cristinadelisle.xyz
COMPOSE_FILE=docker-compose.yml:docker-compose.proxy.yml
TRUSTED_PROXIES=<cjdns address of the proxy machine>
EOT
chmod 600 .env
docker compose up -d --build
```

The site listens on port 8080 on all addresses, including the cjdns one.
ClickHouse (8123) and metrics (9090) stay on localhost.

What the proxy should do:

- forward `cristinadelisle.xyz` (and `www.`) to `http://[<this server's fc… address>]:8080`;
- keep the original `Host` header (the MCP endpoint checks it against
  `PUBLIC_HOSTS`);
- set `X-Forwarded-For` (or `X-Real-IP`) to the visitor's address.

`X-Forwarded-For` is only honoured when the connection comes from an address in
`TRUSTED_PROXIES`, so other cjdns nodes connecting directly cannot fake their
IP to dodge rate limits. Once the site is served over HTTPS, add `HSTS=true`
to `.env` (or let the proxy send the header).

## Full server

The target is one small VPS (for example an OVH VPS in France) running Docker,
with Cloudflare in front. The same image would run on Kubernetes unchanged:
it is stateless apart from `/data` (SQLite) and ClickHouse.

### 1. Server

```bash
# On the VPS (Debian/Ubuntu), as a sudo user
curl -fsSL https://get.docker.com | sh
git clone https://github.com/redchrision/cristina-website.git && cd cristina-website
cp .env.example .env && chmod 600 .env    # fill in the values
```

Generate the ClickHouse password with `openssl rand -base64 32`.

### 2. Cloudflare in front

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

### 3. API keys

```bash
docker compose exec site site-server keys create --name owner \
  --scopes content:read,events:write,stats:read,messages:read
```

### 4. Monitoring with Datadog

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

### 5. Updating

CI pushes `ghcr.io/redchrision/cristina-website:latest` on every commit to
`main`. On the VPS:

```bash
docker compose pull site && docker compose up -d site
```

On `SIGTERM` the server stops accepting connections, finishes in-flight
requests, drains the event queue into ClickHouse and flushes quota counters
before exiting, so a restart loses no accepted events.

### Backups

- SQLite (keys, quota usage, contact messages): copy the `site-data` volume
  (`docker run --rm -v cristina-website_site-data:/data -v $PWD:/out alpine
  tar czf /out/site-data.tgz -C /data .`). The runtime image has no shell.
- ClickHouse: `BACKUP DATABASE site TO Disk('backups', 'site.zip')`, or
  `clickhouse-backup`.
