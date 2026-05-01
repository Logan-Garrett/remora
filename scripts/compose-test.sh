#!/usr/bin/env bash
# compose-test.sh — smoke-test the docker-compose stack.
#
# Works on macOS, Linux, and Windows (Git Bash / WSL).
# Uses REMORA_CLAUDE_CMD=echo so no real Claude credentials are needed.
# Runs against a real Postgres instance (spun up by docker compose).
#
# Usage:
#   ./scripts/compose-test.sh
#
# On success: exits 0 and tears down all containers.
# On failure: exits 1, leaves containers running for inspection.
#             Clean up with: REMORA_TEAM_TOKEN=x docker compose down -v

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMPOSE_FILE="$REPO_ROOT/docker-compose.yml"
PASS=0
FAIL=0

# Export so docker compose can interpolate in all invocations, including cleanup
export REMORA_TEAM_TOKEN="compose-test-token"
# Override claude command to a harmless builtin — the server starts fine,
# and /run would just return echo output instead of a real Claude response.
export REMORA_CLAUDE_CMD="echo"
# Create a dummy .claude dir so the volume mount doesn't fail on fresh machines
export HOME="${HOME:-/tmp}"
mkdir -p "$HOME/.claude" 2>/dev/null || true

SERVER_URL="http://localhost:7200"
WEB_URL="http://localhost:3000"

step()  { echo ""; echo "▶  $*"; }
pass()  { echo "   PASS: $*"; PASS=$((PASS+1)); }
fail()  { echo "   FAIL: $*"; FAIL=$((FAIL+1)); }

cleanup() {
  echo ""
  if [[ $FAIL -gt 0 ]]; then
    echo "❌  $FAIL check(s) failed ($PASS passed). Containers left running — inspect with:"
    echo "    docker compose logs server"
    echo "    REMORA_TEAM_TOKEN=x docker compose down -v"
    exit 1
  else
    echo "✅  All $PASS checks passed. Tearing down..."
    docker compose -f "$COMPOSE_FILE" down -v --remove-orphans 2>/dev/null || true
    echo "Done."
  fi
}
trap cleanup EXIT

# ── Build the web client (needed for nginx volume) ──────────────────────────
step "Building web client"
cd "$REPO_ROOT/web"
npm ci --silent
npm run build --silent
cd "$REPO_ROOT"
pass "web client built"

# ── Start the stack ──────────────────────────────────────────────────────────
step "Starting docker compose stack (first build compiles Rust — this takes a few minutes)"
docker compose -f "$COMPOSE_FILE" up -d --build
pass "docker compose up"

# ── Wait for server health ───────────────────────────────────────────────────
step "Waiting for server health endpoint (up to 60s)"
for i in $(seq 1 30); do
  if curl -sf "$SERVER_URL/health" > /dev/null 2>&1; then
    pass "server is healthy"
    break
  fi
  echo "   ... waiting ($i/30)"
  sleep 2
  if [[ $i -eq 30 ]]; then
    fail "server did not become healthy in 60s"
    docker compose -f "$COMPOSE_FILE" logs server 2>/dev/null | tail -20
  fi
done

# ── Health endpoint body ──────────────────────────────────────────────────────
step "Health endpoint response body"
HEALTH=$(curl -sf "$SERVER_URL/health")
echo "$HEALTH" | grep -q '"status":"ok"'    && pass 'body contains "status":"ok"'    || fail "missing status:ok in: $HEALTH"
echo "$HEALTH" | grep -q '"db":"connected"' && pass 'body contains "db":"connected"' || fail "missing db:connected in: $HEALTH"

# ── Unauthenticated request is rejected ──────────────────────────────────────
step "Auth: unauthenticated request rejected"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$SERVER_URL/sessions")
[[ "$STATUS" == "401" ]] && pass "GET /sessions → 401 (no token)" || fail "expected 401, got $STATUS"

# ── Authenticated request succeeds ───────────────────────────────────────────
step "Auth: authenticated request accepted"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer $REMORA_TEAM_TOKEN" "$SERVER_URL/sessions")
[[ "$STATUS" == "200" ]] && pass "GET /sessions → 200" || fail "expected 200, got $STATUS"

# ── Wrong token is rejected ───────────────────────────────────────────────────
step "Auth: wrong token rejected"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" \
  -H "Authorization: Bearer wrong-token" "$SERVER_URL/sessions")
[[ "$STATUS" == "401" ]] && pass "GET /sessions → 401 (bad token)" || fail "expected 401, got $STATUS"

# ── Create a session ─────────────────────────────────────────────────────────
step "Create a session"
RESPONSE=$(curl -sf -X POST "$SERVER_URL/sessions" \
  -H "Authorization: Bearer $REMORA_TEAM_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"description":"compose-test session","repos":[]}')
SESSION_ID=$(echo "$RESPONSE" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
[[ -n "$SESSION_ID" ]] && pass "created session $SESSION_ID" || fail "could not parse session id from: $RESPONSE"

# ── Session appears in list ───────────────────────────────────────────────────
step "Session appears in list"
LIST=$(curl -sf -H "Authorization: Bearer $REMORA_TEAM_TOKEN" "$SERVER_URL/sessions")
echo "$LIST" | grep -q "$SESSION_ID" && pass "session visible in GET /sessions" || fail "session $SESSION_ID not in list"

# ── Delete the session ────────────────────────────────────────────────────────
step "Delete the session"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
  -H "Authorization: Bearer $REMORA_TEAM_TOKEN" "$SERVER_URL/sessions/$SESSION_ID")
[[ "$STATUS" == "200" || "$STATUS" == "204" ]] \
  && pass "DELETE /sessions/$SESSION_ID → $STATUS" \
  || fail "expected 200/204, got $STATUS"

# ── Session no longer in list ─────────────────────────────────────────────────
step "Session gone after delete"
LIST=$(curl -sf -H "Authorization: Bearer $REMORA_TEAM_TOKEN" "$SERVER_URL/sessions")
echo "$LIST" | grep -q "$SESSION_ID" && fail "session still visible after delete" || pass "session removed from list"

# ── Web client served by nginx ────────────────────────────────────────────────
step "Web client (nginx)"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" "$WEB_URL/")
[[ "$STATUS" == "200" ]] && pass "GET / → 200 from nginx" || fail "expected 200, got $STATUS"
