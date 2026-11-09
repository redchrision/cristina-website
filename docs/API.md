# API guide

Interactive reference: **`/docs`**. Machine-readable spec:
**`/v1/openapi.json`** (OpenAPI 3.1).

## Authentication

Send an API key as a bearer token (or in `X-API-Key`):

```bash
curl -H "Authorization: Bearer csk_1a2b3c4d_…" https://cristinadelisle.xyz/v1/me
```

Requests without a key run as an **anonymous** principal: read-only access to
content, rate-limited per IP (5 req/s, burst 20).

Keys are created on the server and printed once. Only a SHA-256 hash is
stored, and the `csk_xxxxxxxx` prefix identifies the key in listings and logs:

```bash
site-server keys create --name dashboard --scopes stats:read --rate 5 --burst 20 --daily-quota 10000
site-server keys list
site-server keys revoke csk_1a2b3c4d
```

A running server picks up new and revoked keys within 30 seconds.

### Scopes

| Scope | Grants |
|---|---|
| `content:read` | `GET /v1/projects`, `/v1/talks`, `/v1/posts`, `/v1/posts/{slug}` (also anonymous) |
| `events:write` | `POST /v1/events` |
| `stats:read` | `GET /v1/stats/summary`, `/v1/stats/timeseries`, `/v1/stats/top-paths` |
| `messages:read` | `GET /v1/messages`: contact-form inbox, personal data, owner only |

`GET /v1/me` works for any principal and returns its scopes, limits and
today's usage.

## Rate limits and quotas

Two independent limits apply:

| | Rate limit | Daily quota |
|---|---|---|
| Question it answers | How fast? | How much? |
| Algorithm | Token bucket per key (or per IP when anonymous) | Counter per key per UTC day |
| Units | 1 per request | 1 per request + 1 per ingested event |
| When exceeded | `429`, `code: rate_limited`, `Retry-After` in seconds | `429`, `code: quota_exhausted`, `Retry-After` until UTC midnight |
| Headers on every response | `RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset` | `X-Quota-Limit`, `X-Quota-Remaining` (keys with a quota) |

An event batch counts against the quota all at once: it is either fully
accepted or rejected, never partly.

## Errors

Every error is `application/problem+json` ([RFC 9457](https://www.rfc-editor.org/rfc/rfc9457)),
with a stable `code` that clients can switch on:

```json
{
  "type": "https://cristinadelisle.xyz/problems/insufficient-scope",
  "title": "Forbidden",
  "status": 403,
  "detail": "this API key lacks the `stats:read` scope",
  "code": "insufficient_scope"
}
```

| Status | `code` | Meaning |
|---|---|---|
| 400 | `bad_request` | Validation failed; `detail` names the field (e.g. `events[3]: ...`) |
| 401 | `unauthorized` | Missing or unknown key (also returned when anonymous needs a scope) |
| 403 | `insufficient_scope` | Valid key without the required scope |
| 404 | `not_found` | |
| 429 | `rate_limited` / `quota_exhausted` | See above; honour `Retry-After` |
| 503 | `overloaded` | Ingestion queue full; retry after `Retry-After` (1 s) |
| 503 | `unavailable` | ClickHouse not configured on this deployment |

## Versioning

- The major version is in the path: `/v1`. Responses carry `API-Version: 1`.
- Within `v1`, changes are additive only: new endpoints, new optional
  parameters, new response fields. Clients must ignore unknown fields.
- Collections are wrapped (`{"data": [...]}`) so pagination or metadata can be
  added without breaking anyone.
- A breaking change ships as `/v2`. `/v1` then keeps working for at least six
  months, with `Deprecation` and `Sunset` headers
  ([RFC 9745](https://www.rfc-editor.org/rfc/rfc9745), [RFC 8594](https://www.rfc-editor.org/rfc/rfc8594))
  on every response.

## Sending events

```bash
curl -X POST https://cristinadelisle.xyz/v1/events \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"events": [
        {"kind": "pageview", "path": "/#/blog", "lang": "fr", "referrer": "https://www.linkedin.com/feed/"},
        {"kind": "bid_response", "path": "/slot/top", "value": 1.42}
      ]}'
# 202 {"accepted": 2}
```

- 1 to 1 000 events per request, 1 MiB max body.
- `ts` (Unix ms) is optional and clamped to ±24 h of server time.
- Only the referrer **host** is stored. No IP and no user agent are stored.
- `202` means queued: events reach ClickHouse within about one second.

## MCP

The same operations are available to AI agents as MCP tools over Streamable
HTTP at `/mcp`, behind the same keys, scopes and limits:

```bash
claude mcp add --transport http cristina https://cristinadelisle.xyz/mcp \
  --header "Authorization: Bearer $KEY"
```

| Tool | Scope |
|---|---|
| `list_projects`, `list_talks`, `list_blog_posts`, `get_blog_post` | `content:read` (anonymous OK) |
| `traffic_summary`, `traffic_timeseries`, `top_pages` | `stats:read` |
| `list_contact_messages` | `messages:read` |

The endpoint is stateless (no MCP session affinity), so it scales
horizontally like the REST API.
