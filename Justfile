# ── Build ─────────────────────────────────────────────────────────

# Build everything (server + bridge)
build:
    cargo build --release

# Cross-compile for ARM Linux (Raspberry Pi, etc.)
cross-arm:
    cargo zigbuild --release --target aarch64-unknown-linux-gnu

# ── Run ──────────────────────────────────────────────────────────

# Run the server (requires .env)
serve:
    @bash -c 'set -a && source .env && set +a && ./target/release/remora-server'

# Run the server in dev mode (SQLite, no .env needed)
dev:
    DATABASE_URL=sqlite:dev.db \
    REMORA_DB_PROVIDER=sqlite \
    REMORA_TEAM_TOKEN=localdev \
    REMORA_WORKSPACE_DIR=/tmp/remora-workspaces \
    cargo run -p remora-server

# Run the web client dev server (http://localhost:5173)
web:
    cd web && npm install && npm run dev

# Build the MCP server
mcp-build:
    cd mcp && npm install && npm run build

# Type-check the MCP server
mcp-check:
    cd mcp && npx tsc --noEmit

# Start everything with Docker Compose
up token="localdev":
    cd web && npm install && npm run build
    REMORA_TEAM_TOKEN={{token}} docker compose up -d --build

# Stop Docker Compose
down:
    REMORA_TEAM_TOKEN=x docker compose down -v

# ── Test ─────────────────────────────────────────────────────────

# Run all Rust tests (SQLite)
test:
    DATABASE_URL=sqlite:test.db REMORA_DB_PROVIDER=sqlite REMORA_TEAM_TOKEN=test \
    cargo test --all -- --include-ignored --test-threads=1

# Type-check the web client
web-check:
    cd web && npx tsc --noEmit

# Run Playwright E2E tests (requires server + vite running)
e2e:
    cd web && npm run test:e2e

# Run the Docker Compose smoke test
compose-test:
    ./scripts/compose-test.sh

# ── Lint ─────────────────────────────────────────────────────────

# Format and lint everything
lint:
    cargo fmt
    cargo clippy -- -D warnings
    cd web && npx tsc --noEmit

# Check everything without modifying (CI-style)
check:
    cargo check --all-targets
    cargo clippy -- -D warnings
    cargo fmt --check
    cd web && npx tsc --noEmit

# ── Coverage ─────────────────────────────────────────────────────

# Generate test coverage summary
coverage:
    ./scripts/coverage.sh

# Generate HTML coverage report
coverage-html:
    ./scripts/coverage.sh html

# ── Setup ────────────────────────────────────────────────────────

# Run the interactive setup script
setup mode="both":
    ./scripts/setup.sh {{mode}}
