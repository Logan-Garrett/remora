# Remora — Claude Code Context

This file gives Claude Code everything it needs to work effectively in this repo. Read it before making changes.

---

## What This Project Is

Remora is a **collaborative Claude Code session server**. Multiple developers share a single Claude session: they chat, add context, and invoke Claude together. All events are persisted to a database as an append-only log. Clients connect over WebSocket and receive a real-time stream of events.

The server is **stateless across restarts** — reconnecting clients get their history replayed from the database.

---

## Repository Layout

```
remora/
├── server/              Rust — axum HTTP + WebSocket server (main binary: remora-server)
│   └── src/
│       ├── lib.rs       Router, auth, REST handlers (create/list/delete/reactivate session, health)
│       ├── ws.rs        WebSocket upgrade, per-connection send loop, ping keepalive
│       ├── commands.rs  Dispatch ClientMsg variants to handlers (/run, /add, /fetch, etc.)
│       ├── state.rs     AppState: in-memory subscribers/participants/session_owners maps, Config from env
│       ├── quota.rs     Token usage tracking, idle session cleanup loop
│       ├── sandbox.rs   Optional Docker-per-session isolation
│       └── db/
│           ├── mod.rs       DatabaseBackend trait + enum dispatch
│           ├── postgres.rs  Postgres impl (LISTEN/NOTIFY for cross-instance fan-out)
│           ├── sqlite.rs    SQLite impl (in-process broadcast)
│           └── mssql.rs     MSSQL impl (in-process broadcast, manual migration runner)
│
├── bridge/              Rust — tiny stdio↔WebSocket binary used by the Neovim plugin
│   └── src/main.rs      Reads JSON from stdin, forwards to WS; reads WS events, writes to stdout
│
├── common/              Rust — shared types imported by both server and bridge
│   └── src/lib.rs       Event, ClientMsg, ServerMsg, SessionInfo
│
├── plugin/              Lua — Neovim plugin
│   └── lua/remora/
│       └── init.lua     Full plugin: layout, bridge lifecycle, slash command dispatch, reconnect
│
├── web/                 TypeScript + Vite — browser client
│   └── src/
│       ├── main.ts      Entry point: showLogin → showSessions → showChat state machine
│       ├── login.ts     Login form, health-gate, sessionStorage config persistence
│       ├── sessions.ts  Session list, create modal, delete
│       ├── chat.ts      WebSocket connection, event rendering, input bar
│       ├── api.ts       fetch() wrappers for REST + WebSocket URL builder
│       ├── ws.ts        RemoraSocket class wrapping native WebSocket
│       ├── commands.ts  Slash command parser (mirrors Neovim plugin parity)
│       ├── dom.ts       el() helper — XSS-safe DOM builder (textContent only, never innerHTML)
│       └── style.css    Tokyo Night dark theme, mobile responsive (@media max-width: 640px)
│   └── e2e/             Playwright E2E tests
│       ├── login.spec.ts
│       ├── sessions.spec.ts
│       ├── chat.spec.ts
│       └── mobile.spec.ts   Mobile viewport tests (iPhone 12, iPhone 15 Pro, iPhone 15 Pro Max, Pixel 5, Pixel 7, Galaxy S24)
│
├── mcp/                 TypeScript — MCP server (persistent WebSocket client for AI tools)
│   └── src/
│       └── index.ts     MCP server: tools (health, sessions, join, send, run, events, templates)
│
├── templates/           Prompt templates for team workflows (loaded by MCP server)
│   ├── summarize.md     Session activity summary
│   ├── review.md        Code review of last /run
│   ├── pr-description.md  Generate PR title + body
│   ├── explain.md       Explain code to new team member
│   ├── test-plan.md     Generate test plan for changes
│   └── debug.md         Diagnose a bug from context
│
├── migrations/
│   ├── postgres/        SQL migrations run by sqlx::migrate!
│   ├── sqlite/          SQL migrations run by sqlx::migrate!
│   └── mssql/           SQL migrations embedded via include_str! in mssql.rs
│
├── scripts/
│   └── setup.sh         Interactive first-run setup (DB selection, token gen, build, print config)
│
├── docs/
│   └── architecture.md  Deep-dive architecture with Mermaid diagrams (DB schema, /run sequence, state machines)
│
├── .github/
│   ├── workflows/
│   │   ├── ci.yml           GitHub Actions CI pipeline
│   │   └── release.yml      Publishes binaries + web client on v* tag push
│   ├── ISSUE_TEMPLATE/
│   │   ├── bug_report.md
│   │   └── feature_request.md
│   ├── pull_request_template.md
│   ├── dependabot.yml       Weekly Cargo + npm + Actions dependency updates
│   └── CODEOWNERS
│
├── README.md            User-facing docs
├── ROADMAP.md           Planned features — read before adding something new
├── CONTRIBUTING.md      How to contribute, project layout, test guidelines
├── CHANGELOG.md         Version history (Keep a Changelog format)
├── SECURITY.md          Vulnerability reporting + known limitations
├── CODE_OF_CONDUCT.md   Contributor Covenant
├── Dockerfile           Multi-stage build: rust:bookworm builder → debian:bookworm-slim runtime
├── docker-compose.yml   Quick-start: postgres + server + nginx web client
├── rust-toolchain.toml  Pins Rust channel to stable
├── .editorconfig        Consistent editor settings (indent, charset, EOL)
└── CLAUDE.md            This file
```

---

## Architecture

### Event Flow

```
Client (browser/Neovim)
  │  WebSocket (ClientMsg JSON)
  ▼
server/src/ws.rs          — receives message, calls commands::dispatch()
  │
server/src/commands.rs    — handles each ClientMsg variant, writes Event to DB
  │
DB (Postgres LISTEN/NOTIFY │ SQLite in-process broadcast)
  │
server/src/state.rs       — run_event_listener() receives notification, calls dispatch()
  │
AppState.subscribers      — fan-out to all connected WebSocket clients for that session
  │
Client                    — renders ServerMsg::Event
```

### Web Client is Server-Agnostic

The web client is a static site with no hardcoded server URL. The URL, token, and display name are entered at login and stored in `sessionStorage`. This means:
- One deployed copy of the web client can connect to **any** Remora server
- Users don't need to deploy the frontend to use it against their own server
- The frontend and backend are completely independent deployments

### Auth

Single shared `REMORA_TEAM_TOKEN`. Every REST request requires `Authorization: Bearer <token>`. WebSocket upgrade passes token as `?token=` query param (browser WebSocket API cannot set headers). The `/health` endpoint is unauthenticated.

**Known limitation**: one token = full server access. Per-session tokens are on the roadmap.

### Database

The `DatabaseBackend` trait in `db/mod.rs` abstracts all three backends. Adding a new backend (MySQL) means implementing that trait. SQLite runs migrations from `migrations/sqlite/`, Postgres from `migrations/postgres/`. MSSQL uses a custom runner in `mssql.rs` with a hardcoded list — **adding a new MSSQL migration requires updating that list in mssql.rs**.

### In-Memory State

`AppState` holds three `RwLock<HashMap>` — `subscribers` (WebSocket sender channels), `participants` (display names), and `session_owners` (used for owner-only command authorization: `/trust`, `/untrust`, `/kick`). These are **process-local**. Multiple server instances work with Postgres (LISTEN/NOTIFY crosses processes) but `/who` will only see participants on the same instance. SQLite and MSSQL are single-instance only. Session ownership can be claimed via `owner_key` (a UUID generated at session creation and stored in the DB). A client connecting with a valid `owner_key` in the WebSocket query params becomes the session owner, overriding any existing in-memory owner. Without an `owner_key`, the first participant to join becomes owner (backward compatible). The `session_owners` map is cleared when all participants leave a session.

---

## Key Invariants

- **Never use `innerHTML`** anywhere in the web client. All DOM construction goes through `dom.ts`'s `el()` helper which uses `textContent`/`createTextNode`. This is the XSS safety guarantee.
- **Events are append-only**. Nothing is ever updated or deleted from the `events` table. Session and workspace cleanup happens at the application layer.
- **Migrations must be written for all three backends** (Postgres, SQLite, MSSQL) when the schema changes.
- **`cargo fmt` and `cargo clippy -- -D warnings`** must pass before any push. Same for `tsc --noEmit` on the web client.
- **Scan for secrets** in diffs before any push. Never commit tokens, passwords, or API keys.
- **Display names are unique per session** (enforced at WS connect). A second connection with the same name is rejected with `ServerMsg::Error`.
- **`/trust` and `/untrust` are restricted to the session owner** (first participant to join). Other participants receive a system error if they attempt these commands.
- **All PRs require a security review and documentation review before merge/push.** No exceptions for any code changes.

---

## Environment Variables (Server)

| Variable | Default | Notes |
|---|---|---|
| `DATABASE_URL` | required | Postgres: `postgres://user:pass@host/db`, SQLite: `sqlite:file.db` |
| `REMORA_TEAM_TOKEN` | required | Shared auth secret |
| `REMORA_DB_PROVIDER` | `postgres` | `postgres`, `sqlite`, or `mssql` |
| `REMORA_BIND` | `0.0.0.0:7200` | Listen address |
| `REMORA_WORKSPACE_DIR` | `/var/lib/remora/workspaces` | **Must be writable** — crashes on start if it can't be created |
| `REMORA_CLAUDE_CMD` | `claude` | Path to Claude CLI binary |
| `REMORA_SKIP_PERMISSIONS` | `true` | Pass `--dangerously-skip-permissions` to Claude |
| `REMORA_USE_SANDBOX` | `false` | Docker isolation per session |
| `REMORA_RUN_TIMEOUT_SECS` | `600` | Max wall-clock time per Claude run |
| `REMORA_IDLE_TIMEOUT_SECS` | `1800` | Seconds before idle session workspace is deleted and session marked `expired`. Set to a very large number to disable. |
| `REMORA_GLOBAL_DAILY_CAP` | `10000000` | Global daily token limit across all sessions |
| `REMORA_DOCKER_IMAGE` | `ubuntu:22.04` | Docker image for sandbox containers |
| `REMORA_BACKFILL_LIMIT` | `500` | Max events sent to a client on WebSocket connect |
| `REMORA_MAX_SESSIONS` | `100` | Max concurrent sessions (returns 429 when reached) |

---

## Docker Compose (Quick-Start)

`docker-compose.yml` spins up three services: Postgres, the Remora server, and nginx serving the web client.

```bash
# 1. Build the web client (only needed once, or after web changes)
cd web && npm install && npm run build && cd ..

# 2. Start the stack
REMORA_TEAM_TOKEN=yourtoken docker compose up -d

# 3. Open the web client
open http://localhost:3000
# Server API is at http://localhost:7200

# 4. Stop the stack
docker compose down          # keeps data volumes
docker compose down -v       # also wipes Postgres data + workspaces
```

**Claude CLI** is installed inside the Docker image (Node.js 20 + `@anthropic-ai/claude-code`). Your host's `~/.claude` directory is mounted read-only into the container so Claude can use your authentication. If you haven't logged in on the host, run `claude login` first.

For testing without real Claude credentials, set `REMORA_CLAUDE_CMD=echo`.

**Smoke test** (no real Claude needed):
```bash
bash scripts/compose-test.sh
```
This builds the image, starts the stack with `REMORA_CLAUDE_CMD=echo`, runs 13 checks (health, auth, CRUD), then tears everything down.

---

## Running Locally

```bash
# SQLite — no external DB needed
DATABASE_URL=sqlite:dev.db \
REMORA_DB_PROVIDER=sqlite \
REMORA_TEAM_TOKEN=localdev \
REMORA_WORKSPACE_DIR=/tmp/remora-workspaces \
cargo run -p remora-server

# Web client (separate terminal)
cd web && npm install && npm run dev
# Opens at http://localhost:5173
# Connect to http://localhost:7200 with token "localdev"
```

---

## Testing

### Rust

```bash
# Unit + integration (SQLite)
DATABASE_URL=sqlite:test.db REMORA_DB_PROVIDER=sqlite cargo test --all

# Integration tests are marked #[ignore] — run explicitly
DATABASE_URL=sqlite:test.db REMORA_DB_PROVIDER=sqlite \
  cargo test --all -- --ignored --test-threads=1
```

### Web (TypeScript)

```bash
cd web
npx tsc --noEmit          # type-check only
npm run build             # full build — catches type + bundler errors
```

### E2E (Playwright)

The E2E tests need both the server and Vite dev server running:

```bash
# Terminal 1 — server
DATABASE_URL=sqlite:e2e.db REMORA_DB_PROVIDER=sqlite \
REMORA_TEAM_TOKEN=e2e-test-token REMORA_BIND=127.0.0.1:7200 \
REMORA_WORKSPACE_DIR=/tmp/remora-e2e REMORA_CLAUDE_CMD=echo \
cargo run -p remora-server

# Terminal 2 — web
cd web && npx vite --port 3333

# Terminal 3 — tests
cd web && REMORA_SERVER_URL=http://127.0.0.1:7200 \
REMORA_TEAM_TOKEN=e2e-test-token npm run test:e2e
```

Test files and what they cover:
- `login.spec.ts` — health gate, form validation, auth rejection, successful login
- `sessions.spec.ts` — create, delete, leave, rejoin sessions
- `chat.spec.ts` — send messages, /help, /who, WebSocket connected status
- `mobile.spec.ts` — same flows on iPhone 12, iPhone 15 Pro, iPhone 15 Pro Max, Pixel 5, Pixel 7, Galaxy S24 viewports, no overflow

### Adding a New E2E Test

1. Add a `test.describe` block to the relevant spec file (or create a new `web/e2e/*.spec.ts`)
2. Use the `login()` helper from sessions/chat specs — it handles the full login flow
3. Clean up any sessions your test creates (call `leaveAndDelete()` or equivalent)
4. Mobile-specific tests go in `mobile.spec.ts` — desktop chromium ignores that file

---

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs on every push and PR:

| Job | What it checks |
|---|---|
| `typos` | Spell check (source code + docs) via [typos](https://github.com/crate-ci/typos) |
| `rust-check` | `cargo fmt`, `cargo clippy`, `cargo check` |
| `test-postgres` | Full test suite against Postgres 15 |
| `test-sqlite` | Full test suite against SQLite |
| `test-mssql` | Full test suite against MSSQL (continue-on-error) |
| `neovim-test` | Neovim plugin Lua tests |
| `sandbox-e2e` | Docker sandbox container lifecycle |
| `security-audit` | `cargo audit` |
| `web-audit` | `npm audit --audit-level=high` + `tsc --noEmit` |
| `e2e-web` | Playwright E2E (chromium, iPhone 12, iPhone 15 Pro, iPhone 15 Pro Max, Pixel 5, Pixel 7, Galaxy S24) |
| `test-windows` | Windows: Rust lint, SQLite tests, MSSQL (native SQL Server Express), web build + audit |
| `docker-compose-test` | `scripts/compose-test.sh` — builds image, starts stack, checks health + auth + CRUD |
| `coverage` | Test coverage (cargo-llvm-cov with SQLite) → Codecov |
| `build` | Release build |

`.github/workflows/release.yml` fires on `v*` tags and publishes binaries + web client to GitHub Releases:
- `remora-server` and `remora-bridge` for linux-amd64, linux-arm64 (via zigbuild), macos-arm64
- `remora-web.tar.gz` — pre-built web client static files
- SHA256 checksums for each artifact

---

## Deploy (Self-Hosted Example)

The server binary and web client are deployed separately. The web client is a static site that can connect to any Remora server URL — it is not tied to a specific host. The URL, token, and display name are entered at login time.

Below is an example deployment on a Raspberry Pi, but the same pattern applies to any Linux host:

- The server binary runs on the host and exposes a port (default `7200`, configurable via `REMORA_BIND`)
- The web client is a static `dist/` folder served by any HTTP server (nginx, python, caddy, etc.)
- A reverse proxy or tunnel (e.g. Cloudflare Tunnel, nginx, Caddy) handles TLS and forwards to the local ports
- Secrets live in a `~/.remora.env` file (chmod 600) sourced by the start script

### Start / stop scripts

Place these on the server host. The start script reads `~/.remora.env` which must contain `DATABASE_URL` and `REMORA_TEAM_TOKEN`.

```bash
# start.sh
#!/bin/bash
set -e
set -a; source ~/.remora.env; set +a

export REMORA_BIND=0.0.0.0:7200
export REMORA_DB_PROVIDER=sqlite   # or postgres
export REMORA_WORKSPACE_DIR=/var/lib/remora/workspaces
export REMORA_SKIP_PERMISSIONS=true

nohup /path/to/remora-server >> server.log 2>&1 &
nohup python3 -m http.server 3000 --directory /path/to/web/dist >> web.log 2>&1 &
```

```bash
# stop.sh
kill $(pgrep -f remora-server) 2>/dev/null || true
kill $(pgrep -f 'http.server.*3000') 2>/dev/null || true
```

### Cross-compile for ARM64 (Raspberry Pi, etc.)

```bash
cargo zigbuild --release --target aarch64-unknown-linux-gnu -p remora-server
scp target/aarch64-unknown-linux-gnu/release/remora-server user@host:~/remora/remora-server
scp -r web/dist/* user@host:~/remora/web/
ssh user@host "~/remora/stop.sh && ~/remora/start.sh"
```

---

## Claude Code Skill

A `/remora` skill is included at `.claude/skills/remora-chat.md`. Since `.claude/` is in `.gitignore`, install it manually:

```bash
# Global install (works from any directory)
mkdir -p ~/.claude/skills
cp .claude/skills/remora-chat.md ~/.claude/skills/

# Or project-level install
cp -r .claude/skills/ <your-project>/.claude/skills/
```

Once installed, use `/remora` in Claude Code to list sessions, send messages, and trigger `/run` on a remote Remora server. The skill uses `scripts/remora-cli.sh` which wraps the REST API and the `remora-bridge` binary for WebSocket.

---

## Common Mistakes to Avoid

- **Don't use `innerHTML`** anywhere in the web client — ever.
- **Don't add a schema change without migrations for all three DB backends.**
- **Don't forget `REMORA_WORKSPACE_DIR`** in test/CI environments — the default `/var/lib/remora/workspaces` requires root and will crash the server.
- **Don't add MSSQL migrations** without updating the hardcoded list in `server/src/db/mssql.rs`.
- **Don't push without running fmt/clippy/lint** — CI will fail and it's noise.
- **Don't commit secrets** — scan the diff before every push.
