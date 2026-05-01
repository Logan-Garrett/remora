-- Missing indexes for session-scoped tables
CREATE INDEX IF NOT EXISTS idx_session_repos_session_id     ON session_repos (session_id);
CREATE INDEX IF NOT EXISTS idx_session_runs_session_id      ON session_runs (session_id);
CREATE INDEX IF NOT EXISTS idx_pending_approvals_session_id ON pending_approvals (session_id);
-- Index for global token-usage aggregation (filtered by reset date)
CREATE INDEX IF NOT EXISTS idx_sessions_tokens_reset_date   ON sessions (tokens_reset_date);
