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
| Single team token auth | Per-session tokens, OAuth, SSO |

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

### Per-session invite tokens
Each session gets its own token scoped to that session only. You can invite someone to one conversation without giving them access to anything else on the server. The server-level token becomes an admin credential.

```
Current:  one token → full server access
Target:   server token (admin) + session tokens (scoped invites)
```

The `session_runs.owner_instance` column and the existing per-session DB structure already anticipate this — sessions are first-class objects, the auth layer just hasn't caught up.

### User accounts
- Register / login with email + password
- Display names tied to accounts, not just a connection-time string
- Personal session list showing sessions you created or were invited to

### OAuth / SSO
- **OAuth 2.0** — sign in with GitHub, Google, or any provider
- **SAML / SSO** — enterprise login via Okta, Azure AD, Google Workspace
- **API keys per user** — for CLI and plugin auth without browser flows

### Role-based access
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

### Team namespacing
- Sessions, quotas, and allowlists scoped to a team
- Team admins manage their own members and token limits
- Complete isolation between teams on the same server instance

### User dashboard
A web page showing:
- All sessions you have access to (created or invited)
- Quick join / create
- Usage summary for your sessions
- Pending invites

### Cross-team isolation
Teams cannot see each other's sessions, events, or workspaces. The DB already has per-session scoping everywhere; it just needs a `team_id` column added and propagated.

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

### Usage dashboard
The `sessions` table already tracks `tokens_used_today`, `daily_token_cap`, and `tokens_reset_date`. A dashboard could show:
- Token burn rate per session and globally
- Daily / weekly usage trends
- Quota headroom and alerts when approaching limits

### Run analytics
`session_runs` stores `started_at`, `finished_at`, `status`, and `context_mode` for every Claude invocation. This is enough to show:
- Run success/failure rates
- Average run duration
- Which sessions are most active
- How often runs timeout or fail

### Admin panel
- View all sessions, runs, and participants across the server
- Force-expire or delete sessions
- Adjust per-session token caps without restarting
- View and manage the global and per-session fetch allowlists (currently CLI-only via `/allowlist`)

### Audit log
- Record who created/deleted sessions, who ran Claude, and when
- Exportable for compliance

### Metrics endpoint
A `/metrics` endpoint in Prometheus format so server operators can plug into existing monitoring stacks (Grafana, Datadog, etc.).

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
