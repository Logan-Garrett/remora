# Contributing to Remora

Thanks for your interest in contributing. This document covers how to get started, what areas need help, and how to submit changes.

## Before You Start

Check [ROADMAP.md](ROADMAP.md) to understand where the project is headed. If you want to work on something from the roadmap or have a new idea, open an issue first so we can align on approach before you invest time writing code.

## Getting Started

### Prerequisites

- Rust 1.75+
- Node.js 20+
- SQLite (easiest) or Postgres 15+

### Local setup

```bash
git clone https://github.com/Logan-Garrett/remora.git
cd remora
./scripts/setup.sh   # interactive setup — picks SQLite by default
```

Or manually:

```bash
cp .env.example .env
# Edit .env: set DATABASE_URL and REMORA_TEAM_TOKEN

# Run the server
cargo run -p remora-server

# Run the web client (separate terminal)
cd web && npm install && npm run dev
```

### Running tests

```bash
# Rust unit + integration tests (SQLite)
DATABASE_URL=sqlite:test.db REMORA_DB_PROVIDER=sqlite cargo test --all

# Web type-check
cd web && npx tsc --noEmit

# Web E2E (requires server running on 127.0.0.1:7200)
cd web && npm run test:e2e
```

## Project Layout

```
remora/
├── server/          Rust — axum HTTP + WebSocket server
├── bridge/          Rust — stdio↔WebSocket binary for the Neovim plugin
├── common/          Rust — shared types (Event, ClientMsg, ServerMsg)
├── plugin/          Lua — Neovim plugin
├── web/             TypeScript — Vite web client
│   └── e2e/         Playwright E2E tests
├── migrations/
│   ├── postgres/    SQL migrations for Postgres
│   ├── sqlite/      SQL migrations for SQLite
│   └── mssql/       SQL migrations for MSSQL
└── scripts/         Setup and deployment helpers
```

## How to Contribute

1. **Fork** the repo and create a branch off `master`
2. **Make your changes** — see the guidelines below
3. **Test** — run the relevant test suite before opening a PR
4. **Open a PR** — describe what changed and why; link any related issue

No CLA, no special process. Keep it clean and explain what you changed.

## Guidelines

- **Rust**: run `cargo fmt` and `cargo clippy -- -D warnings` before pushing
- **TypeScript**: run `tsc --noEmit` to check types; no `any` without a comment explaining why
- **SQL**: any schema change needs migration files for all three backends (Postgres, SQLite, MSSQL)
- **Security**: no secrets in commits; no new `unsafe` without a justification comment
- **Tests**: new server behaviour should have an integration test; new web flows should have a Playwright test

## Areas That Need Help

See [ROADMAP.md](ROADMAP.md) for the full picture. Near-term areas where PRs are most welcome:

- **MySQL / MariaDB backend** — add a fourth `DatabaseBackend` implementation
- **Per-session tokens** — scoped invite tokens instead of one server-wide secret
- **Token-level streaming** — stream Claude's response token-by-token instead of per-turn
- **VS Code extension** — port the Neovim plugin to VS Code
- **Admin dashboard** — surface the token usage and run analytics already tracked in the DB

## Reporting Bugs

Open a GitHub issue with:
- What you expected to happen
- What actually happened
- Steps to reproduce
- Your OS, Rust version, and DB backend
