#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

echo "=== Neovim plugin tests ==="

# Run each test file in a separate headless Neovim instance.
# We add the plugin directory to the runtime path so `require("remora")` works.

for test_file in "$SCRIPT_DIR"/test_*.lua; do
  name="$(basename "$test_file")"
  echo "--- Running $name ---"
  nvim --headless -u NONE \
    -c "set rtp+=$REPO_ROOT/plugin" \
    -c "luafile $test_file" \
    -c "qa!"
  echo ""
done

echo "=== All Neovim tests passed ==="
