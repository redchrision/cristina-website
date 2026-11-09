-- API keys: only a SHA-256 hash of the secret is stored. The prefix is the
-- non-secret part shown in listings and logs ("csk_ab12cd34").
CREATE TABLE api_keys (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT    NOT NULL,
    prefix       TEXT    NOT NULL UNIQUE,
    secret_hash  TEXT    NOT NULL UNIQUE,
    scopes       TEXT    NOT NULL,           -- space-separated
    rate_per_sec REAL    NOT NULL,
    burst        REAL    NOT NULL,
    daily_quota  INTEGER,                    -- NULL = unlimited
    created_at   INTEGER NOT NULL,           -- unix seconds
    revoked_at   INTEGER
);

-- Usage per key per UTC day, in quota units (1 per request, 1 per event).
CREATE TABLE api_usage (
    key_id INTEGER NOT NULL REFERENCES api_keys (id),
    day    INTEGER NOT NULL,                 -- days since the Unix epoch
    units  INTEGER NOT NULL,
    PRIMARY KEY (key_id, day)
);

CREATE TABLE contact_messages (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    public_id   TEXT    NOT NULL UNIQUE,     -- UUIDv7, safe to expose
    name        TEXT    NOT NULL,
    email       TEXT    NOT NULL,
    message     TEXT    NOT NULL,
    received_at INTEGER NOT NULL
);
CREATE INDEX contact_messages_received_at ON contact_messages (received_at);
