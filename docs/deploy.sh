#!/usr/bin/env bash
# Deploy Remora docs to Raspberry Pi
# Usage: bash docs/deploy.sh

set -euo pipefail

REMOTE="lg3@raspberrypi.local"
REMOTE_DIR="~/remora-docs"
PORT=8080
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "==> Copying docs to $REMOTE:$REMOTE_DIR ..."
scp -r "$SCRIPT_DIR/" "$REMOTE:$REMOTE_DIR/"

echo "==> Checking for python3 on Pi..."
ssh "$REMOTE" "command -v python3 >/dev/null 2>&1 && echo 'python3 found' || echo 'python3 NOT found'"

echo "==> Killing any existing server on port $PORT..."
ssh "$REMOTE" "kill \$(lsof -t -i:$PORT) 2>/dev/null || true"

echo "==> Starting HTTP server on port $PORT..."
ssh "$REMOTE" "cd $REMOTE_DIR && nohup python3 -m http.server $PORT > /dev/null 2>&1 &"

echo "==> Waiting for server to start..."
sleep 2

echo "==> Verifying..."
if curl -s -o /dev/null -w "%{http_code}" "http://raspberrypi.local:$PORT/" | grep -q 200; then
  echo "SUCCESS: Site is live at http://raspberrypi.local:$PORT/"
else
  echo "WARNING: Could not verify. Try opening http://raspberrypi.local:$PORT/ in your browser."
fi
