# Changelog

All notable changes to Remora are documented here.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/). Versions follow [Semantic Versioning](https://semver.org/).

---

## [Unreleased]

### Added
- Playwright E2E test suite — login, sessions, chat, and mobile flows
- Mobile-responsive CSS (`@media max-width: 640px`) — full-width cards, 16px inputs (prevents iOS Safari zoom), 44px tap targets
- Mobile test matrix: iPhone 12, iPhone 15 Pro, iPhone 15 Pro Max, Pixel 5, Pixel 7, Galaxy S24
- `npm audit` + TypeScript type-check CI job (`web-audit`)
- `CLAUDE.md` — full project context, architecture, and contribution guide for AI-assisted development
- `ROADMAP.md` — phased feature roadmap
- `CONTRIBUTING.md` — contributor guide and community health file
- `docs/architecture.md` — deep-dive architecture with Mermaid diagrams
- GitHub issue and PR templates
- DB performance indexes: `session_repos`, `session_runs`, `pending_approvals` (session_id), `sessions` (tokens_reset_date)
- `start.sh` / `stop.sh` deployment scripts
- CI path filtering — skips pipeline for doc-only changes

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

[Unreleased]: https://github.com/Logan-Garrett/remora/compare/v0.7.1...HEAD
[0.7.1]: https://github.com/Logan-Garrett/remora/compare/v0.7.0...v0.7.1
[0.7.0]: https://github.com/Logan-Garrett/remora/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/Logan-Garrett/remora/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/Logan-Garrett/remora/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/Logan-Garrett/remora/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/Logan-Garrett/remora/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/Logan-Garrett/remora/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/Logan-Garrett/remora/releases/tag/v0.1.0
