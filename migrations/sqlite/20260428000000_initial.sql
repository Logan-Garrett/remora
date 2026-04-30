CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    description TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now')),
    daily_token_cap   INTEGER DEFAULT 999999999,
    tokens_used_today INTEGER DEFAULT 0,
    tokens_reset_date TEXT DEFAULT (date('now')),
    idle_since        TEXT
);

CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    timestamp   TEXT NOT NULL DEFAULT (datetime('now')),
    author      TEXT,
    kind        TEXT NOT NULL,
    payload     TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_events_session_id ON events (session_id, id);

CREATE TABLE IF NOT EXISTS session_repos (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    git_url    TEXT NOT NULL,
    added_at   TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(session_id, name)
);

CREATE TABLE IF NOT EXISTS session_runs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id     TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    started_at     TEXT NOT NULL DEFAULT (datetime('now')),
    finished_at    TEXT,
    status         TEXT NOT NULL DEFAULT 'running',
    owner_instance TEXT,
    heartbeat      TEXT NOT NULL DEFAULT (datetime('now')),
    context_mode   TEXT NOT NULL DEFAULT 'since_last'
);

CREATE TABLE IF NOT EXISTS global_allowlist (
    domain TEXT PRIMARY KEY,
    kind   TEXT NOT NULL DEFAULT 'allow'
);

CREATE TABLE IF NOT EXISTS session_allowlist (
    session_id  TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    domain      TEXT NOT NULL,
    approved_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (session_id, domain)
);

CREATE TABLE IF NOT EXISTS pending_approvals (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id   TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    domain       TEXT NOT NULL,
    url          TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    requested_at TEXT NOT NULL DEFAULT (datetime('now')),
    resolved     INTEGER NOT NULL DEFAULT 0,
    approved     INTEGER
);
