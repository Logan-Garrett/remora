#!/usr/bin/env bash
# Deploy Remora docs to a remote host
# Usage: bash docs/deploy.sh [user@host]
# Example: bash docs/deploy.sh pi@myserver.local

set -euo pipefail

REMOTE="${1:?Usage: deploy.sh <user@host>}"
REMOTE_DIR="~/remora-docs"
PORT=8080
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> Copying docs to $REMOTE:$REMOTE_DIR ..."
scp -r "$SCRIPT_DIR/" "$REMOTE:$REMOTE_DIR/"

echo "==> Killing any existing server on port $PORT..."
ssh "$REMOTE" "kill \$(lsof -t -i:$PORT) 2>/dev/null || true"

echo "==> Starting HTTP server on port $PORT..."
ssh "$REMOTE" "cd $REMOTE_DIR && nohup python3 -m http.server $PORT > /dev/null 2>&1 &"

sleep 2
HOST=$(echo "$REMOTE" | cut -d@ -f2)
echo "==> Site should be live at http://$HOST:$PORT/"
