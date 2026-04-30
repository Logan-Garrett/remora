# Build everything
build:
    cargo build --release

# Run the server (requires .env)
serve:
    @bash -c 'set -a && source .env && set +a && ./target/release/remora-server'

# Run all tests
test:
    ./scripts/test.sh

# Format and lint
lint:
    cargo fmt
    cargo clippy -- -D warnings

# Cross-compile for ARM Linux
cross-arm:
    cargo zigbuild --release --target aarch64-unknown-linux-gnu

# Run the interactive setup script
setup mode="both":
    ./scripts/setup.sh {{mode}}

# Check everything (no tests)
check:
    cargo check --all-targets
    cargo clippy -- -D warnings
    cargo fmt --check

# Generate test coverage report
coverage:
    ./scripts/coverage.sh

# Generate HTML coverage report
coverage-html:
    ./scripts/coverage.sh html
