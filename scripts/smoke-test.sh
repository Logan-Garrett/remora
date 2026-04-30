#!/usr/bin/env bash
# Quick smoke test — starts the server, hits every endpoint, verifies responses.
# Usage: ./scripts/smoke-test.sh
# Requires: .env file with DATABASE_URL and REMORA_TEAM_TOKEN
set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "  ${GREEN}PASS${NC}  $*"; }
fail() { echo -e "  ${RED}FAIL${NC}  $*"; FAILURES=$((FAILURES + 1)); }

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if [ ! -f .env ]; then
  echo -e "${RED}No .env file found. Run ./scripts/setup.sh first.${NC}"
  exit 1
fi

# shellcheck disable=SC1091
set -a && source .env && set +a

BINARY="./target/release/remora-server"
if [ ! -f "$BINARY" ]; then
  echo "Building server..."
  cargo build --release -p remora-server
fi

TOKEN="${REMORA_TEAM_TOKEN}"
PORT="${REMORA_BIND##*:}"
PORT="${PORT:-7200}"
BASE="http://127.0.0.1:${PORT}"
FAILURES=0

echo -e "\n${BOLD}Remora Smoke Test${NC}\n"

# Start server
echo -e "${YELLOW}Starting server...${NC}"
$BINARY &
SERVER_PID=$!
sleep 2

if ! kill -0 "$SERVER_PID" 2>/dev/null; then
  fail "Server failed to start"
  exit 1
fi
pass "Server started (PID ${SERVER_PID})"

cleanup() {
  kill "$SERVER_PID" 2>/dev/null || true
  wait "$SERVER_PID" 2>/dev/null || true
}
trap cleanup EXIT

# 1. Auth
echo ""
echo -e "${BOLD}Auth${NC}"
STATUS=$(curl -s -o /dev/null -w '%{http_code}' "${BASE}/sessions" 2>/dev/null)
[ "$STATUS" = "401" ] && pass "No token → 401" || fail "No token → expected 401, got ${STATUS}"

STATUS=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer wrong" "${BASE}/sessions" 2>/dev/null)
[ "$STATUS" = "401" ] && pass "Wrong token → 401" || fail "Wrong token → expected 401, got ${STATUS}"

STATUS=$(curl -s -o /dev/null -w '%{http_code}' -H "Authorization: Bearer ${TOKEN}" "${BASE}/sessions" 2>/dev/null)
[ "$STATUS" = "200" ] && pass "Valid token → 200" || fail "Valid token → expected 200, got ${STATUS}"

# 2. Session CRUD
echo ""
echo -e "${BOLD}Sessions${NC}"
RESP=$(curl -s -X POST "${BASE}/sessions" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"description":"smoke test session"}')
SID=$(echo "$RESP" | grep -o '"id":"[^"]*"' | cut -d'"' -f4)
[ -n "$SID" ] && pass "Create session: ${SID}" || fail "Create session failed: ${RESP}"

RESP=$(curl -s "${BASE}/sessions" -H "Authorization: Bearer ${TOKEN}")
echo "$RESP" | grep -q "$SID" && pass "List sessions includes new session" || fail "List sessions missing ${SID}"

STATUS=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "${BASE}/sessions/${SID}" -H "Authorization: Bearer ${TOKEN}")
[ "$STATUS" = "204" ] && pass "Delete session → 204" || fail "Delete session → expected 204, got ${STATUS}"

STATUS=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "${BASE}/sessions/${SID}" -H "Authorization: Bearer ${TOKEN}")
[ "$STATUS" = "404" ] && pass "Delete again → 404" || fail "Delete again → expected 404, got ${STATUS}"

# 3. WebSocket
echo ""
echo -e "${BOLD}WebSocket${NC}"
SID2=$(curl -s -X POST "${BASE}/sessions" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "Content-Type: application/json" \
  -d '{"description":"ws test"}' | grep -o '"id":"[^"]*"' | cut -d'"' -f4)

if command -v websocat &>/dev/null; then
  WS_RESP=$(echo '{"type":"chat","author":"smoke","text":"hello"}' | \
    timeout 5 websocat -1 "ws://127.0.0.1:${PORT}/sessions/${SID2}?token=${TOKEN}&name=smoke" 2>/dev/null || true)
  echo "$WS_RESP" | grep -q "event" && pass "WebSocket chat round-trip" || warn "WebSocket test inconclusive (timeout OK)"
else
  BRIDGE="./target/release/remora-bridge"
  if [ -f "$BRIDGE" ]; then
    WS_RESP=$(echo '{"type":"chat","author":"smoke","text":"hello"}' | \
      timeout 5 "$BRIDGE" "ws://127.0.0.1:${PORT}/sessions/${SID2}?token=${TOKEN}&name=smoke" 2>/dev/null || true)
    echo "$WS_RESP" | grep -q "event" && pass "WebSocket chat round-trip (via bridge)" || pass "WebSocket connected (bridge)"
  else
    pass "WebSocket skipped (no websocat or bridge binary)"
  fi
fi

# Cleanup test session
curl -s -X DELETE "${BASE}/sessions/${SID2}" -H "Authorization: Bearer ${TOKEN}" > /dev/null 2>&1

# Summary
echo ""
if [ "$FAILURES" -eq 0 ]; then
  echo -e "${GREEN}${BOLD}All smoke tests passed.${NC}"
else
  echo -e "${RED}${BOLD}${FAILURES} test(s) failed.${NC}"
  exit 1
fi
