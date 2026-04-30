#!/bin/bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

echo "=== Rust check ==="
cargo check --all-targets

echo ""
echo "=== Rust clippy ==="
cargo clippy -- -D warnings

echo ""
echo "=== Rust fmt ==="
cargo fmt --check

echo ""
echo "=== Rust unit tests ==="
cargo test --all

echo ""
echo "=== Rust integration tests ==="
if [ -n "${DATABASE_URL:-}" ]; then
  PROVIDER="${REMORA_DB_PROVIDER:-postgres}"
  echo "  Provider: $PROVIDER"
  echo "  URL: $DATABASE_URL"
  cargo test --all -- --ignored --test-threads=1
else
  echo "  SKIPPED — DATABASE_URL not set"
  echo ""
  echo "  Quick-run with SQLite (no setup needed):"
  echo "    DATABASE_URL=sqlite:test.db REMORA_DB_PROVIDER=sqlite ./scripts/test.sh"
  echo ""
  echo "  Run with Postgres:"
  echo "    DATABASE_URL=postgres://user:pass@localhost/remora_test REMORA_DB_PROVIDER=postgres ./scripts/test.sh"
fi

echo ""
echo "=== Neovim plugin tests ==="
if command -v nvim &>/dev/null; then
  chmod +x plugin/tests/run_tests.sh
  plugin/tests/run_tests.sh
else
  echo "  SKIPPED — nvim not found"
fi

echo ""
echo "=== All tests passed ==="
