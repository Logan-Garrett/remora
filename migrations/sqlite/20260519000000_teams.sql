CREATE TABLE teams (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    daily_token_cap INTEGER NOT NULL DEFAULT 999999999,
    created_at TEXT NOT NULL,
    created_by TEXT REFERENCES users(id)
);
