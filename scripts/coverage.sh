#!/usr/bin/env bash
# Generate test coverage report
# Usage: ./scripts/coverage.sh [html|summary]
set -euo pipefail

MODE="${1:-summary}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if ! command -v cargo-llvm-cov &>/dev/null; then
  echo "Installing cargo-llvm-cov..."
  cargo install cargo-llvm-cov
fi

# Default to SQLite for coverage (no external DB needed)
export DATABASE_URL="${DATABASE_URL:-sqlite:coverage_test.db}"
export REMORA_DB_PROVIDER="${REMORA_DB_PROVIDER:-sqlite}"
export REMORA_TEAM_TOKEN="${REMORA_TEAM_TOKEN:-test-token}"

rm -f coverage_test.db

case "$MODE" in
  html)
    cargo llvm-cov --all --html -- --include-ignored --test-threads=1
    echo "Coverage report: target/llvm-cov/html/index.html"
    ;;
  lcov)
    cargo llvm-cov --all --lcov --output-path lcov.info -- --include-ignored --test-threads=1
    echo "LCOV output: lcov.info"
    ;;
  *)
    cargo llvm-cov --all --summary-only -- --include-ignored --test-threads=1
    ;;
esac

rm -f coverage_test.db
