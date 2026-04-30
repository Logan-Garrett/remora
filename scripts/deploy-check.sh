#!/usr/bin/env bash
# Post-deploy health check for remora-server.
# Usage: ./scripts/deploy-check.sh [HOST] [TOKEN]
#   HOST  defaults to http://localhost:7200
#   TOKEN defaults to $REMORA_TEAM_TOKEN
set -euo pipefail

HOST="${1:-http://localhost:7200}"
TOKEN="${2:-${REMORA_TEAM_TOKEN:-}}"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

ok()   { echo -e "${GREEN}[pass]${NC} $*"; }
fail() { echo -e "${RED}[fail]${NC} $*"; FAILED=1; }

FAILED=0

# 1. Health endpoint (no auth)
echo "Checking ${HOST}/health ..."
HTTP_CODE=$(curl -s -o /tmp/remora_health.json -w '%{http_code}' "${HOST}/health" 2>/dev/null) || true
if [ "$HTTP_CODE" = "200" ]; then
    STATUS=$(jq -r '.status' /tmp/remora_health.json 2>/dev/null)
    DB=$(jq -r '.db' /tmp/remora_health.json 2>/dev/null)
    if [ "$STATUS" = "ok" ] && [ "$DB" = "connected" ]; then
        ok "Health endpoint: status=$STATUS db=$DB"
    else
        fail "Health endpoint returned unexpected body: status=$STATUS db=$DB"
    fi
else
    fail "Health endpoint returned HTTP $HTTP_CODE (expected 200)"
fi

# 2. Auth rejection (should return 401 without token)
echo "Checking auth enforcement ..."
HTTP_CODE=$(curl -s -o /dev/null -w '%{http_code}' "${HOST}/sessions" 2>/dev/null) || true
if [ "$HTTP_CODE" = "401" ]; then
    ok "Auth enforcement: unauthenticated request rejected (401)"
else
    fail "Auth enforcement: expected 401, got $HTTP_CODE"
fi

# 3. Session lifecycle (requires token)
if [ -n "$TOKEN" ]; then
    echo "Checking session lifecycle ..."

    # Create
    CREATE_RESP=$(curl -s -w '\n%{http_code}' -X POST "${HOST}/sessions" \
        -H "Authorization: Bearer ${TOKEN}" \
        -H "Content-Type: application/json" \
        -d '{"description":"deploy-check"}' 2>/dev/null)
    CREATE_CODE=$(echo "$CREATE_RESP" | tail -1)
    CREATE_BODY=$(echo "$CREATE_RESP" | sed '$d')

    if [ "$CREATE_CODE" = "201" ]; then
        SESSION_ID=$(echo "$CREATE_BODY" | jq -r '.id' 2>/dev/null)
        ok "Create session: id=$SESSION_ID"

        # List
        LIST_CODE=$(curl -s -o /dev/null -w '%{http_code}' "${HOST}/sessions" \
            -H "Authorization: Bearer ${TOKEN}" 2>/dev/null)
        if [ "$LIST_CODE" = "200" ]; then
            ok "List sessions: 200"
        else
            fail "List sessions: expected 200, got $LIST_CODE"
        fi

        # Delete
        DEL_CODE=$(curl -s -o /dev/null -w '%{http_code}' -X DELETE "${HOST}/sessions/${SESSION_ID}" \
            -H "Authorization: Bearer ${TOKEN}" 2>/dev/null)
        if [ "$DEL_CODE" = "204" ] || [ "$DEL_CODE" = "200" ]; then
            ok "Delete session: $DEL_CODE"
        else
            fail "Delete session: expected 204, got $DEL_CODE"
        fi
    else
        fail "Create session: expected 201, got $CREATE_CODE"
    fi
else
    echo "Skipping session lifecycle (no TOKEN provided, set REMORA_TEAM_TOKEN or pass as arg 2)"
fi

echo ""
if [ "$FAILED" -eq 0 ]; then
    echo -e "${GREEN}All checks passed.${NC}"
else
    echo -e "${RED}Some checks failed.${NC}"
    exit 1
fi
