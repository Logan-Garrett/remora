-- Repos per session
CREATE TABLE session_repos (
    id BIGSERIAL PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    git_url TEXT NOT NULL,
    added_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(session_id, name)
);

-- Track Claude runs
CREATE TABLE session_runs (
    id BIGSERIAL PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'running', -- running, completed, failed, timeout
    owner_instance TEXT, -- which server instance owns this run
    heartbeat TIMESTAMPTZ NOT NULL DEFAULT now(),
    context_mode TEXT NOT NULL DEFAULT 'since_last' -- 'since_last' or 'full'
);

-- Fetch allowlists
CREATE TABLE global_allowlist (
    domain TEXT PRIMARY KEY,
    kind TEXT NOT NULL DEFAULT 'allow' -- 'allow' or 'block'
);

CREATE TABLE session_allowlist (
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    domain TEXT NOT NULL,
    approved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (session_id, domain)
);

-- Quotas
ALTER TABLE sessions ADD COLUMN daily_token_cap BIGINT DEFAULT 999999999;
ALTER TABLE sessions ADD COLUMN tokens_used_today BIGINT DEFAULT 0;
ALTER TABLE sessions ADD COLUMN tokens_reset_date DATE DEFAULT CURRENT_DATE;
ALTER TABLE sessions ADD COLUMN idle_since TIMESTAMPTZ;

-- Pending fetch approvals
CREATE TABLE pending_approvals (
    id BIGSERIAL PRIMARY KEY,
    session_id UUID NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    domain TEXT NOT NULL,
    url TEXT NOT NULL,
    requested_by TEXT NOT NULL,
    requested_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    resolved BOOLEAN NOT NULL DEFAULT false,
    approved BOOLEAN
);
