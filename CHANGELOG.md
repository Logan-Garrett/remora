# Changelog

All notable changes to Remora are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/). Versions follow [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

---

## [0.9.2] — 2026-05-01

### Added
- **Owner key entry UX** — web UI shows "Enter Owner Key" button for sessions where the key isn't stored locally; Neovim `:RemoraJoin` accepts optional 5th argument for owner_key
- **TOCTOU fix** — duplicate name check and participant join are now atomic (single write lock)
- **Session owner cleanup** — owner entry is cleared from AppState when all participants leave and when a session is deleted
- **Updated screenshots** — new desktop, mobile (iPhone 15 Pro), and Neovim mockup images showing trust features, Owner Key button, /who with trusted list

### Changed
- Versions bumped from 0.9.0 to 0.9.2 (server, bridge, common, web)
- Owner key prevents first-joiner fallback: if a session has an owner_key in the DB, only the key holder can be owner

---

## [0.9.0] — 2026-05-01

### Added
- **Trusted participants** — `/trust <name>` and `/untrust <name>` commands; trusted users' messages reach Claude as plain instructions, untrusted messages are wrapped in `<untrusted_content>` tags
- **Session ownership via owner_key** — sessions get a unique `owner_key` UUID on creation; pass it in WebSocket query params to claim persistent ownership (survives reconnects and server restarts). Without the key, first-joiner becomes owner (backward compatible). Only the owner can `/trust`, `/untrust`, and `/kick`
- **Owner key in clients** — web UI stores owner_key in sessionStorage and auto-passes it on connect; "Owner Key" button in chat header copies key to clipboard. Neovim plugin stores key in state and provides `:RemoraOwnerKey` command. `/info` command shows key to the current owner
- **Unique display names** — server rejects WebSocket connections if someone with the same name is already connected to the session
- **Session expired UX** — sessions marked `expired` on idle cleanup; connecting to an expired session shows a friendly message instead of "session not found". Workspace auto-recreated on `/run` if idle cleanup removed it
- `Dockerfile` + `docker-compose.yml` — one-command local stack (Postgres + server + nginx web client, Claude CLI built in)
- `scripts/compose-test.sh` — 13-point smoke test for the compose stack; used in CI and locally
- `docker-compose-test` CI job — builds image and runs smoke test on every push
- `release.yml` GitHub Actions workflow — publishes binaries (linux-amd64, linux-arm64, macos-arm64) + web client tarball + Docker image to GHCR on `v*` tag push
- `test-windows` CI job — full Windows test suite: Rust lint, SQLite tests, native MSSQL (SQL Server Express via Chocolatey), Playwright E2E on MSSQL, web client build + audit
- `dependabot-automerge.yml` — auto-merge patch/minor dependency PRs that pass CI
- `codeql.yml` — GitHub CodeQL SAST for TypeScript on push, PR, and weekly schedule
- `scorecard.yml` — OpenSSF Scorecard security grading
- `typos` CI job — spell-check source code and docs
- `coverage` CI job — test coverage via cargo-llvm-cov → Codecov
- `SECURITY.md` — vulnerability reporting policy, known limitations, trust model documentation
- `CODE_OF_CONDUCT.md` — Contributor Covenant 2.1
- `.editorconfig` — consistent indent/charset/EOL settings across editors
- `.github/dependabot.yml` — weekly automated updates for Cargo, npm, and GitHub Actions
- `rust-toolchain.toml` — pins Rust channel to stable
- Justfile rewrite — organized sections with `dev`, `web`, `up`/`down`, `e2e`, `compose-test`, `web-check` targets

### Changed
- Cargo.toml versions bumped from 0.1.0 to 0.9.0 (server, bridge, common)
- web/package.json version bumped from 0.1.0 to 0.9.0

---

## [0.8.0] — 2026-04-30

### Added
- Playwright E2E test suite — login, sessions, chat, and mobile flows
- Mobile-responsive CSS (`@media max-width: 640px`) — full-width cards, 16px inputs (prevents iOS Safari zoom), 44px tap targets
- Mobile test matrix: iPhone 12, iPhone 15 Pro, iPhone 15 Pro Max, Pixel 5, Pixel 7, Galaxy S24
- `npm audit` + TypeScript type-check CI job (`web-audit`)
- `CLAUDE.md` — full project context, architecture, and contribution guide for AI-assisted development
- `ROADMAP.md` — phased feature roadmap
- `CONTRIBUTING.md` — contributor guide and community health file
- `CHANGELOG.md` — this file; version history in Keep a Changelog format
- `docs/architecture.md` — deep-dive architecture with Mermaid diagrams (DB schema, `/run` sequence, state machines, multi-instance)
- GitHub issue templates (bug report, feature request) and PR template
- DB performance indexes: `session_repos`, `session_runs`, `pending_approvals` (session_id), `sessions` (tokens_reset_date)
- `start.sh` / `stop.sh` deployment scripts
- CI path filtering — skips pipeline for doc-only changes (`**.md`, `docs/**`, `assets/**`, `LICENSE`)
- README: "Why Remora?" section (fish metaphor), hosted client link, latest release / stars / issues badges, architecture doc link
- Web client documented as server-agnostic — one deployed copy connects to any Remora server URL

### Changed
- License changed from MIT to custom Source Available license — free for personal/internal/non-commercial use; commercial distribution requires a written revenue-sharing agreement

### Fixed
- CI: server crashed on start in E2E job due to missing `REMORA_WORKSPACE_DIR` (defaulted to `/var/lib/remora/workspaces`, permission denied)
- Playwright selector mismatch: description input placeholder now contains "description"
- Desktop Chromium project no longer runs mobile-only specs

---

## [0.7.1] — 2026-04-30

### Added
- `/help` and `/?` slash commands — prints all available commands to the session log
- `Help` variant added to `ClientMsg` enum in `common/`
- WebSocket 30-second ping keepalive — prevents Cloudflare and proxy idle timeouts

### Fixed
- Bridge binary panic on `wss://` URLs — rustls 0.23 requires explicit crypto provider registration (`ring`)
- Bridge now handles `SIGTERM` cleanly — no more zombie processes after `:RemoraLeave`

---

## [0.7.0] — 2026-04-30

### Added
- Web client (TypeScript + Vite) — browser-based alternative to the Neovim plugin
- Web client is server-agnostic — enter any server URL at login, no frontend deployment required
- CORS support via `tower-http` `CorsLayer`
- Tool call / tool result rendering in the web client event log
- Session create modal matching Neovim plugin flow (description + optional git repos)

### Fixed
- Neovim plugin: "buffer with this name already exists" error on leave + rejoin
- Neovim plugin: auto-reconnect no longer fires after intentional `/leave`
- Neovim plugin: env var fallbacks for `REMORA_URL`, `REMORA_TEAM_TOKEN`, `REMORA_NAME`

---

## [0.6.0] — 2026-04-29

### Added
- `/health` endpoint (unauthenticated) — returns `{"status":"ok","db":"connected"}` or 503
- `db.ping()` on all three backends (`SELECT 1`)
- Deploy check script (`scripts/deploy_check.sh`)

---

## [0.5.0] — 2026-04-29

### Security
- Fixed all remaining HIGH / MEDIUM / LOW audit findings
- `cargo audit` added to CI with advisory ignore list for known unpatched upstream issues
- Removed `dependency-review` action (replaced by audit)

---

## [0.4.0] — 2026-04-28

### Added
- Docker sandbox isolation — optional per-session container via `REMORA_USE_SANDBOX=true`
- Permission modes — `REMORA_PERMISSION_MODE` and `REMORA_ALLOWED_TOOLS`
- Sandbox E2E test in CI — full container lifecycle with fake API key
- Coverage tooling (`cargo-llvm-cov` script + Justfile targets)
- 79 integration tests total

### Security
- Fixed prompt injection via crafted chat messages
- Fixed sandbox isolation bypass
- Fixed author impersonation in WebSocket messages
- Atomic run guard — prevents race condition allowing two simultaneous Claude runs

---

## [0.3.0] — 2026-04-28

### Added
- MSSQL backend — full support including custom migration runner (sqlx doesn't support MSSQL natively)
- 57 integration tests covering all three DB backends
- `.gitignore` entries for SQLite WAL files

### Fixed
- MSSQL connection string handling — `TrustServerCertificate`, encryption flags
- CI: `sqlcmd` installation on GitHub Actions runner for MSSQL test database creation
- Integration tests now run serially to prevent DB race conditions

---

## [0.2.0] — 2026-04-27

### Added
- SQLite backend — single-file DB, no external service required
- Database abstraction layer — `DatabaseBackend` trait makes backends swappable
- `--skip-permissions` flag passed to Claude CLI
- GitHub Actions CI — Rust lint, format, unit + integration tests for Postgres and SQLite
- Documentation site (`docs/`)
- Test harness with initial integration test coverage

### Fixed
- Deploy script: removed hardcoded hostname
- CI: integration tests run serially to prevent DB race conditions

---

## [0.1.0] — 2026-04-26

### Added
- Initial release
- Rust/axum WebSocket server with Postgres backend
- Append-only event log — sessions, events, repos, runs, allowlists
- `/run`, `/run-all`, `/clear`, `/add`, `/diff`, `/fetch`, `/who`, `/kick`, `/session`, `/repo`, `/allowlist`, `/approve`, `/deny` commands
- Neovim plugin with Telescope integration and bridge binary
- Docker sandbox scaffolding
- Per-session token quota tracking
- Idle session cleanup
- Postgres `LISTEN/NOTIFY` for real-time cross-instance event fan-out
- `REMORA_USE_SANDBOX`, `REMORA_SKIP_PERMISSIONS`, `REMORA_BIND`, and all other env vars
- MIT license

[Unreleased]: https://github.com/Logan-Garrett/remora/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/Logan-Garrett/remora/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/Logan-Garrett/remora/compare/v0.7.1...v0.8.0
[0.7.1]: https://github.com/Logan-Garrett/remora/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/Logan-Garrett/remora/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/Logan-Garrett/remora/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Logan-Garrett/remora/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Logan-Garrett/remora/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Logan-Garrett/remora/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Logan-Garrett/remora/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Logan-Garrett/remora/releases/tag/v0.1.0
