# Remora Roadmap

> A living document. Items move, priorities shift — this is direction, not a schedule.

---

## Current State

Remora today is designed as a **single-server, single-team tool**. One server process, one shared team token, one filesystem for workspaces. Everyone who knows the token can read, write, create, and delete any session. It works great for a small trusted team all pointing at the same server — but that assumption is baked in everywhere.

| What works today | What's missing |
|---|---|
| Multiple users sharing a session in real-time | Any concept of "users" or identity |
| Postgres / SQLite / MSSQL backends | MySQL / MariaDB support |
| Web client + Neovim plugin | Desktop app, VS Code / JetBrains plugins |
| Per-session event log, run history in DB | UI to explore or query that history |
| Token usage tracked per session | No dashboard or alerts for quota |
| Docker sandbox isolation | No network egress control inside sandbox |
| Team token, user accounts, JWT, OAuth (GitHub/Google) with popup+postMessage, API keys | SAML / SSO (Okta, Azure AD) |
| Admin dashboard: usage, analytics, session management, user roles, audit log, Prometheus metrics | Allowlist management UI, auto-promote first user to admin |

---

## Phase Overview

```
Phase 1           Phase 2            Phase 3             Phase 4             Phase 5
─────────         ─────────          ─────────           ─────────           ─────────
Auth &            Multi-tenancy      Database &           Admin &             Ecosystem
Identity          & Sharing          Infrastructure       Observability
    │                  │                  │                   │                   │
Per-session       Teams &            MySQL / Redis        Usage dashboard     Desktop app
tokens            namespacing        multi-instance       Run analytics       VS Code ext
                                                                              JetBrains
User accounts     User dashboard     Session export       Audit log           SSE streaming
OAuth / SSO       Cross-team         Helm / Compose       Allowlist UI        Mobile app
                  isolation          deploy               Admin panel
```

---

## Phase 1 — Auth & Identity

> Replace the shared team token with real identity.

The current model is a single `REMORA_TEAM_TOKEN` — one password for everything. This is fine for a single team on a private server but blocks every other use case.

### Per-session invite tokens -- DONE
Each session gets its own token scoped to that session only. You can invite someone to one conversation without giving them access to anything else on the server. The server-level token becomes an admin credential.

Implemented: sessions get an invite token on creation. CRUD endpoints at `/sessions/:id/tokens`. Token validation integrated into `check_any_token()`.

### Per-participant invite tokens
The current trust model uses display names to identify trusted participants. This works but has a brief impersonation window between disconnect and reconnect. Per-participant invite tokens would tie trust to a cryptographic token rather than a name string:

- Each participant gets a unique, random token when invited to a session
- The token authenticates the WebSocket connection and determines the display name
- Trust is granted to the token, not the name -- eliminating the impersonation window
- Tokens can be revoked individually without affecting other participants

This builds on per-session invite tokens (above) and eliminates the last name-based identity assumption in the trust system.

### User accounts -- DONE
- Register / login with email + password (argon2 hashing)
- JWT access tokens + refresh token rotation
- `/auth/register`, `/auth/login`, `/auth/refresh`, `/auth/me` endpoints
- Display names tied to user accounts for JWT/API-key authenticated connections

### Auth service -- DONE
Built-in JWT-based auth integrated into the existing DB. Short-lived access tokens (default 1h) with refresh token rotation (default 7d). Atomic token consumption prevents race conditions. All three DB backends (Postgres, SQLite, MSSQL) supported.

### OAuth / SSO -- MOSTLY DONE
- **OAuth 2.0** -- DONE. GitHub and Google sign-in implemented end-to-end: server-side handlers with HMAC-signed CSRF state (origin embedded), web client OAuth buttons using a popup+postMessage flow with origin validation, and `isAdmin` flag propagated through all login paths
- **SAML / SSO** -- not yet implemented (Okta, Azure AD, Google Workspace)
- **API keys per user** -- DONE. `rmk_` prefixed keys with SHA-256 hashing

### Role-based access -- IN PROGRESS
Role hierarchy and enforcement helpers are implemented. RBAC enforcement in WebSocket command dispatch is not yet wired in (tracked as TODO in `commands.rs`).

| Role | Can do |
|---|---|
| Admin | Full server access, manage users, set quotas |
| Member | Create sessions, invite others, use Claude |
| Viewer | Read-only access to specific sessions |
| Guest | Join a single session via invite token, no create |

---

## Phase 2 — Multi-tenancy & Sharing

> Support multiple teams or projects on one server.

Right now running separate teams means running separate servers. That's fine but wasteful.

### Team namespacing -- DONE
- Sessions scoped to a team via `team_id` foreign key on `sessions` table
- Team admins manage their own members via REST API (add, remove, update roles)
- `teams` and `team_members` tables with full CRUD (12 new REST endpoints)
- Team member roles: admin, member, viewer
- Unique team names enforced at the DB level

### User dashboard -- DONE
Backend `GET /dashboard` endpoint returns all sessions the authenticated user has access to across all their teams, with team name annotations. Admin users also have access to the admin dashboard panel in the web client (separate from the per-user dashboard).

### Cross-team isolation -- DONE
Teams cannot see each other's sessions. Enforced at multiple layers:
- REST endpoints check team membership before returning team data
- WebSocket upgrade checks team membership for team-scoped sessions
- Admin token bypasses team checks (server-level access)
- Session-scoped tokens bypass team checks (already scoped to one session)
- Team deletion detaches sessions (sets `team_id` to NULL) rather than cascade-deleting them

---

## Phase 3 — Database & Infrastructure

> Broaden the backend options and make deployment easier.

### MySQL / MariaDB
A fourth database backend alongside Postgres, SQLite, and MSSQL. MySQL is the most common self-hosted DB and is missing from the current list. The pluggable `DatabaseBackend` trait makes this addable without touching anything else.

### Multi-instance with Redis pub/sub
Currently `participants` and `subscribers` are in-memory — multiple server instances behind a load balancer would have split state. Fix:
- Move participant presence to the DB (or Redis)
- Use Redis pub/sub as a fifth notification backend that actually works multi-instance
- `owner_instance` in `session_runs` is already there for run coordination

### Session export & workspace persistence
Before idle cleanup deletes a workspace, optionally:
- Push all repo branches to their remotes
- Export the event log as a Markdown transcript
- Archive the workspace to object storage (S3 / R2)

### Deployment tooling
- `docker-compose.yml` for one-command local setup
- Helm chart for Kubernetes deployments
- Pre-built Docker images on GHCR for each release

---

## Phase 4 — Admin & Observability

> Expose what the database is already tracking.

The schema already captures a surprising amount — it just isn't surfaced anywhere.

### Usage dashboard -- DONE
Global and per-session token usage surfaced via `GET /admin/usage`. The admin dashboard Overview tab shows a global daily usage summary and a per-session breakdown table.

### Run analytics -- DONE
`GET /admin/analytics` returns success/failure/timeout counts and average run duration from `session_runs`. Displayed in the admin dashboard Overview tab.

### Admin panel -- DONE
Full admin panel in the web client (`web/src/admin.ts`) with four tabs:
- **Overview**: global usage stats and run analytics
- **Sessions**: list all sessions; edit per-session quotas; force-expire or force-delete
- **Users**: list all registered users; change roles via dropdown
- **Audit Log**: paginated log of admin-initiated actions

Admin endpoints (`/admin/*`) accept the shared team token or any JWT/API key with `role == "admin"`.

### Audit log -- DONE
`audit_events` table records admin actions (quota updates, role changes, forced session operations). Exposed via `GET /admin/audit` with pagination. Migration added for all three DB backends.

### Metrics endpoint -- DONE
`GET /metrics` returns Prometheus text-format gauges and counters: session totals, active sessions, WebSocket connections, total tokens used today, and run counts by status.

### Remaining Phase 4 work
- Allowlist management UI (currently CLI-only via `/allowlist` slash command)
- Auto-promote first registered user to admin role
- Alert thresholds and notifications when quota headroom is low

---

## Phase 5 — Ecosystem & Clients

> More ways to access Remora.

### Desktop app
A native desktop app (Tauri) wrapping the web client. Benefits over the browser:
- Lives in the menu bar / taskbar
- Native notifications when Claude finishes a run
- No need to manage browser tabs
- Deep-links to open specific sessions

### VS Code extension
Join and interact with Remora sessions from inside VS Code. Mirrors the Neovim plugin feature set — session picker, shared chat panel, `/run` from the command palette.

### JetBrains plugin
Same as VS Code, for IntelliJ-based IDEs.

### Token-level streaming
Currently Claude's response arrives per-turn (one chunk per agentic step). Token-level streaming would show Claude typing in real-time — much more interactive.

### Mobile app
A lightweight React Native or PWA mobile client for joining sessions on the go. Read the event log, send messages, and kick off Claude runs from a phone.

---

## What's Not on the Roadmap

Things explicitly out of scope for the foreseeable future:

- **Self-hosting Claude** — Remora shells out to the Claude CLI; it is not a model server
- **Branching / forking sessions** — the append-only log is intentional
- **Real-time collaborative editing** — that's what the session workspace + Claude is for; Remora is not a code editor

---

## Contributing

If you want to work on any of the above, open an issue first to discuss approach. The codebase is Rust (server), TypeScript (web), and Lua (Neovim plugin). No special process — just keep it clean and explain what you changed.
