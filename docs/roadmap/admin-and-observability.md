# Track 3: Admin and Observability

> Surface the data the server already tracks. The database schema captures token usage, run history, allowlists, and session metadata -- but none of it is visible to operators without SQL access.

---

## Current State

- `sessions` table tracks `tokens_used_today`, `daily_token_cap`, `tokens_reset_date`
- `session_runs` stores `started_at`, `finished_at`, `status`, `context_mode` for every Claude invocation
- `global_allowlist` and `session_allowlist` exist but are only manageable via slash commands in a session
- Token usage is checked per-run in `quota.rs` but not surfaced anywhere
- No admin API -- all management happens through the WebSocket session or direct DB access
- No metrics endpoint, no structured logging for analytics

---

## Milestone 1: Admin REST API

**Priority: Highest** | **Depends on: nothing (but benefits from Auth Track M3 for proper admin auth)**

Expose server management over REST so operators do not need database access.

### Endpoints

| Method | Path | Description |
|---|---|---|
| `GET` | `/admin/sessions` | All sessions with participant count, token usage, status |
| `GET` | `/admin/sessions/:id` | Detailed session info including runs, repos, allowlist |
| `DELETE` | `/admin/sessions/:id` | Force-delete a session (cleanup workspace + sandbox) |
| `PATCH` | `/admin/sessions/:id` | Update per-session token cap, description |
| `GET` | `/admin/runs` | Recent Claude runs across all sessions |
| `GET` | `/admin/usage` | Global token usage summary (today, 7d, 30d) |
| `GET` | `/admin/allowlist` | Global fetch allowlist |
| `POST` | `/admin/allowlist` | Add domain to global allowlist |
| `DELETE` | `/admin/allowlist/:domain` | Remove domain from global allowlist |
| `GET` | `/admin/config` | Current server config (redacted secrets) |

### Implementation plan

1. Add an `/admin` router in `lib.rs` gated by the `REMORA_TEAM_TOKEN` (or admin JWT from Auth Track).
2. Each endpoint calls existing `Database` trait methods -- most of the data is already queryable, just not exposed via REST.
3. Add new trait methods where needed: `list_all_sessions_with_usage()`, `list_recent_runs(limit)`, `get_global_usage_history(days)`.

### Acceptance criteria

- All admin endpoints return JSON
- Endpoints are gated by admin auth (team token or admin role)
- `/admin/config` never exposes `REMORA_TEAM_TOKEN`, `DATABASE_URL`, or `ANTHROPIC_API_KEY`

---

## Milestone 2: Usage Dashboard (Web)

**Priority: High** | **Depends on: M1 (admin API)**

A web page for operators to see token usage and session activity at a glance.

### Views

1. **Overview**: Total sessions (active/expired), global token usage today vs. cap, active Claude runs
2. **Token usage chart**: Daily token burn over the last 30 days (requires storing historical usage, see below)
3. **Session list**: Sortable table with columns: description, status, created, participants, tokens used today, last activity
4. **Session detail**: Token usage, run history (start/end/status/duration), repos, allowlist, participants

### Implementation plan

1. Add a `daily_usage_history` table to persist daily totals before reset:
   ```
   daily_usage_history (
     date        DATE NOT NULL,
     session_id  UUID REFERENCES sessions(id),
     tokens_used BIGINT,
     PRIMARY KEY (date, session_id)
   )
   ```
2. Record a snapshot in the `reset_tokens_if_needed` function (in `quota.rs`) before resetting the counter.
3. The dashboard is a new page in the web client (or a separate lightweight SPA). It consumes the admin REST API from M1.
4. No authentication beyond what the admin API already enforces (token or JWT).

### Acceptance criteria

- Dashboard loads and displays real data from the admin API
- Token usage chart shows at least 7 days of history
- Session list is sortable and filterable by status
- Works on mobile (responsive layout)

---

## Milestone 3: Run Analytics

**Priority: Medium** | **Depends on: M1 (admin API)**

The `session_runs` table already has everything needed for basic analytics.

### Metrics to surface

- **Run success rate**: `completed` vs `failed` vs `timeout` as percentages
- **Average run duration**: `finished_at - started_at` grouped by day/week
- **Runs per session**: Which sessions use Claude most
- **Context mode distribution**: How often `/run` vs `/run-all` is used
- **Peak hours**: Runs grouped by hour-of-day to show usage patterns

### Implementation plan

1. Add aggregate query methods to the `Database` trait:
   - `get_run_stats(days: i32) -> RunStats`
   - `get_run_duration_percentiles(days: i32) -> Vec<(String, f64)>`
   - `get_runs_by_hour(days: i32) -> Vec<(u8, i64)>`
2. Expose via `/admin/analytics/runs` endpoint.
3. Add a "Runs" tab to the usage dashboard.

### Acceptance criteria

- Success/failure/timeout rates are displayed
- Average and p95 run durations are shown
- Data is filterable by date range

---

## Milestone 4: Audit Log

**Priority: Medium** | **Depends on: M1; benefits from Auth Track for user identity**

Record administrative and security-relevant actions for compliance and debugging.

### Events to log

- Session created / deleted
- Participant joined / left / kicked
- Claude run started / completed / failed
- Trust granted / revoked
- Allowlist modified
- Admin actions (quota changes, force-delete)
- Auth events (login, logout, failed login) -- requires Auth Track

### Implementation plan

1. New table:
   ```
   audit_log (
     id         BIGSERIAL PRIMARY KEY,
     timestamp  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
     actor      TEXT NOT NULL,        -- user/admin who performed the action
     action     TEXT NOT NULL,        -- 'session.create', 'participant.kick', etc.
     resource   TEXT,                 -- session_id or other identifier
     details    JSONB,               -- action-specific metadata
     ip_address TEXT                  -- client IP (from X-Forwarded-For or socket)
   )
   ```
2. Add `audit_log()` helper function called from `commands.rs`, `lib.rs` (REST handlers), and `ws.rs` (connect/disconnect).
3. Admin API endpoint: `GET /admin/audit?actor=...&action=...&since=...&limit=...`
4. Export: `GET /admin/audit/export?format=csv&since=...` for compliance downloads.

### Acceptance criteria

- All security-relevant actions are logged
- Audit log is append-only (no updates or deletes)
- Admin API supports filtering by actor, action, and date range
- CSV export works

---

## Milestone 5: Metrics Endpoint (Prometheus)

**Priority: Medium-Low** | **Depends on: nothing**

A `/metrics` endpoint in Prometheus exposition format for operators with existing monitoring stacks.

### Metrics to expose

| Metric | Type | Description |
|---|---|---|
| `remora_sessions_active` | gauge | Number of active sessions |
| `remora_sessions_total` | counter | Total sessions created |
| `remora_participants_connected` | gauge | Total connected WebSocket clients |
| `remora_runs_total` | counter | Total Claude runs (by status label) |
| `remora_run_duration_seconds` | histogram | Claude run duration distribution |
| `remora_tokens_used_total` | counter | Total tokens consumed |
| `remora_events_total` | counter | Total events inserted |
| `remora_ws_connections_total` | counter | Total WebSocket connections |

### Implementation plan

1. Add `prometheus` and `metrics` crates (or use `axum-prometheus` for integration).
2. Instrument key code paths: `ws.rs` (connections), `commands.rs` (runs, events), `quota.rs` (tokens).
3. Expose `GET /metrics` as an unauthenticated endpoint (standard for Prometheus scraping, can be restricted by bind address or network policy).
4. Add a sample Grafana dashboard JSON in `deploy/grafana/`.

### Acceptance criteria

- `/metrics` returns valid Prometheus exposition format
- Grafana dashboard template is included and works out of the box
- Metrics are accurate under load (tested with the compose stack)

---

## Milestone 6: Alerting and Notifications

**Priority: Low** | **Depends on: M2 (dashboard) or M5 (metrics)**

Notify operators when thresholds are crossed.

### Alerts

- Global daily token cap at 80% / 100%
- Per-session token cap at 90%
- Claude run failure rate > 20% in the last hour
- Session count approaching `REMORA_MAX_SESSIONS`
- Stale Claude run (running > 2x `REMORA_RUN_TIMEOUT_SECS`)

### Implementation plan

- **Option A (Prometheus)**: Provide alerting rules in `deploy/prometheus/alerts.yml` for use with Alertmanager. No server changes needed -- just rules on top of M5 metrics.
- **Option B (Built-in)**: Add a simple webhook notification system. `REMORA_ALERT_WEBHOOK_URL` receives JSON payloads when thresholds are crossed. Checked in the idle cleanup loop (`quota.rs`).
- Both options can coexist.

### Acceptance criteria

- At least one alerting path works (webhook or Prometheus rules)
- Token cap alerts fire correctly
- No alert storms (debounce repeated threshold crossings)

---

## Dependency Graph

```
M1 (Admin REST API)
  ├── M2 (Usage dashboard)
  │     └── M6 (Alerting - Option B)
  ├── M3 (Run analytics)
  └── M4 (Audit log)

M5 (Metrics endpoint)     -- independent
  └── M6 (Alerting - Option A)
```

M1 is the foundation. M5 is independent and can ship at any time. M6 has two implementation paths that depend on different milestones.

---

## Risks and Open Questions

- **Historical usage data**: The current schema resets `tokens_used_today` daily with no history. M2 requires either a new table or an external time-series store. The new table approach is simpler and sufficient for most deployments.
- **Dashboard hosting**: Should the admin dashboard be part of the existing web client (same SPA, different route) or a separate app? Same SPA is simpler but mixes admin and user concerns. Recommend a separate route (`/admin`) in the same SPA, gated by auth.
- **Prometheus vs. built-in**: Small deployments (single Pi) probably don't run Prometheus. The built-in webhook (M6 Option B) is more accessible. Consider making the webhook the default and Prometheus the advanced option.
- **Audit log volume**: A busy server generates many events. Consider a retention policy (`REMORA_AUDIT_RETENTION_DAYS`) and periodic cleanup.
