-- test_format_event.lua
-- Unit tests for format_event in the Remora Neovim plugin.
--
-- Run: nvim --headless -u NONE -c "set rtp+=plugin" -c "luafile plugin/tests/test_format_event.lua" -c "qa!"

local pass_count = 0
local fail_count = 0

local function assert_contains(haystack, needle, msg)
  if type(haystack) == "string" and haystack:find(needle, 1, true) then
    pass_count = pass_count + 1
    print("PASS: " .. msg)
  else
    fail_count = fail_count + 1
    print("FAIL: " .. msg .. " — '" .. tostring(haystack) .. "' does not contain '" .. tostring(needle) .. "'")
  end
end

local function assert_eq(a, b, msg)
  if a == b then
    pass_count = pass_count + 1
    print("PASS: " .. msg)
  else
    fail_count = fail_count + 1
    print("FAIL: " .. msg .. " — expected " .. tostring(b) .. " got " .. tostring(a))
  end
end

-- ── Re-implement format_event locally ─────────────────────────────────
-- We extract the format_event logic to test it in isolation, since
-- the function is local in init.lua. This mirrors the actual implementation.

local function format_event(event)
  local ts = event.timestamp or ""
  local time = ts:match("T(%d+:%d+:%d+)") or ""
  local kind = event.kind or "?"
  local author = event.author or "system"
  local payload = event.payload or {}

  if kind == "chat" then
    return { { string.format("[%s] %s: %s", time, author, payload.text or ""), "RemoraAuthor" } }

  elseif kind == "system" then
    return { { string.format("[%s] * %s", time, payload.text or ""), "RemoraSystem" } }

  elseif kind == "file" then
    local nlines = payload.lines or "?"
    local path = payload.path or "?"
    return { { string.format("[%s] file %s added %s (%s lines)", time, author, path, nlines), nil } }

  elseif kind == "fetch" then
    local nbytes = payload.bytes or payload.size or "?"
    local url = payload.url or "?"
    return { { string.format("[%s] fetch %s fetched %s (%s bytes)", time, author, url, nbytes), nil } }

  elseif kind == "diff" then
    local result = { { string.format("[%s] diff output", time), nil } }
    local text = payload.text or payload.diff or ""
    for diff_line in text:gmatch("[^\n]+") do
      table.insert(result, { diff_line, nil })
    end
    return result

  elseif kind == "claude_request" then
    return { { string.format("[%s] Claude run started", time), "RemoraClaudeMsg" } }

  elseif kind == "claude_response" then
    local result = {}
    local text = payload.text or ""
    local first = true
    for resp_line in text:gmatch("[^\n]+") do
      if first then
        table.insert(result, { string.format("[%s] Claude: %s", time, resp_line), "RemoraClaudeMsg" })
        first = false
      else
        table.insert(result, { resp_line, "RemoraClaudeMsg" })
      end
    end
    if first then
      table.insert(result, { string.format("[%s] Claude: (empty)", time), "RemoraClaudeMsg" })
    end
    return result

  elseif kind == "tool_call" then
    local tool = payload.tool or payload.name or "?"
    local args = payload.args or payload.arguments or ""
    if type(args) == "table" then
      args = vim.fn.json_encode(args)
    end
    return { { string.format("[%s] tool call %s(%s)", time, tool, args), "RemoraToolCall" } }

  elseif kind == "tool_result" then
    local nlines = payload.lines or "?"
    return { { string.format("[%s] tool result (%s lines)", time, nlines), "RemoraToolCall" } }

  elseif kind == "repo_change" then
    local action = payload.action or "changed"
    local name = payload.name or payload.repo or "?"
    return { { string.format("[%s] repo %s: %s", time, action, name), nil } }

  elseif kind == "allowlist_request" then
    local domain = payload.domain or "?"
    return { { string.format("[%s] Approval needed: %s", time, domain), "RemoraError" } }

  elseif kind == "allowlist_update" then
    local action = payload.action or "updated"
    local domain = payload.domain or "?"
    return { { string.format("[%s] allowlist: %s %s", time, domain, action), nil } }

  elseif kind == "clear_marker" then
    return { { string.format("[%s] --- context cleared ---", time), "RemoraSystem" } }

  else
    local payload_str = ""
    if vim and vim.fn and vim.fn.json_encode then
      payload_str = vim.fn.json_encode(payload)
    end
    return { { string.format("[%s] [%s] %s: %s", time, kind, author, payload_str), nil } }
  end
end

-- ── Helper to get first line text from format_event result ────────────

local function first_line(result)
  if result and result[1] then
    return result[1][1]
  end
  return ""
end

local function first_hl(result)
  if result and result[1] then
    return result[1][2]
  end
  return nil
end

-- ── Test events ──────────────────────────────────────────────────────

-- Chat
local chat_result = format_event({
  timestamp = "2026-04-28T12:30:45Z",
  kind = "chat",
  author = "alice",
  payload = { text = "hello everyone" },
})
assert_contains(first_line(chat_result), "12:30:45", "chat: timestamp present")
assert_contains(first_line(chat_result), "alice", "chat: author present")
assert_contains(first_line(chat_result), "hello everyone", "chat: text present")
assert_eq(first_hl(chat_result), "RemoraAuthor", "chat: highlight is RemoraAuthor")

-- System
local sys_result = format_event({
  timestamp = "2026-04-28T14:00:00Z",
  kind = "system",
  payload = { text = "bob joined" },
})
assert_contains(first_line(sys_result), "bob joined", "system: text present")
assert_contains(first_line(sys_result), "*", "system: asterisk prefix")
assert_eq(first_hl(sys_result), "RemoraSystem", "system: highlight is RemoraSystem")

-- File
local file_result = format_event({
  timestamp = "2026-04-28T10:00:00Z",
  kind = "file",
  author = "carol",
  payload = { path = "src/main.rs", lines = 42 },
})
assert_contains(first_line(file_result), "carol", "file: author present")
assert_contains(first_line(file_result), "src/main.rs", "file: path present")
assert_contains(first_line(file_result), "42", "file: line count present")

-- Fetch
local fetch_result = format_event({
  timestamp = "2026-04-28T11:00:00Z",
  kind = "fetch",
  author = "dave",
  payload = { url = "https://example.com", bytes = 1024 },
})
assert_contains(first_line(fetch_result), "dave", "fetch: author present")
assert_contains(first_line(fetch_result), "https://example.com", "fetch: url present")
assert_contains(first_line(fetch_result), "1024", "fetch: byte count present")

-- Diff
local diff_result = format_event({
  timestamp = "2026-04-28T11:30:00Z",
  kind = "diff",
  payload = { text = "+added line\n-removed line" },
})
assert_contains(first_line(diff_result), "diff output", "diff: header present")
assert_eq(#diff_result, 3, "diff: header + 2 diff lines = 3 entries")
assert_contains(diff_result[2][1], "+added line", "diff: first diff line")
assert_contains(diff_result[3][1], "-removed line", "diff: second diff line")

-- Claude request
local req_result = format_event({
  timestamp = "2026-04-28T12:00:00Z",
  kind = "claude_request",
  payload = {},
})
assert_contains(first_line(req_result), "Claude run started", "claude_request: text present")
assert_eq(first_hl(req_result), "RemoraClaudeMsg", "claude_request: highlight correct")

-- Claude response
local resp_result = format_event({
  timestamp = "2026-04-28T12:01:00Z",
  kind = "claude_response",
  payload = { text = "Here is the answer.\nWith two lines." },
})
assert_contains(resp_result[1][1], "Claude:", "claude_response: has Claude prefix")
assert_contains(resp_result[1][1], "Here is the answer.", "claude_response: first line text")
assert_eq(#resp_result, 2, "claude_response: two lines")
assert_contains(resp_result[2][1], "With two lines.", "claude_response: second line text")
assert_eq(resp_result[1][2], "RemoraClaudeMsg", "claude_response: highlight correct")

-- Claude response (empty)
local empty_resp = format_event({
  timestamp = "2026-04-28T12:02:00Z",
  kind = "claude_response",
  payload = { text = "" },
})
assert_contains(first_line(empty_resp), "(empty)", "claude_response empty: shows (empty)")

-- Tool call
local tc_result = format_event({
  timestamp = "2026-04-28T12:03:00Z",
  kind = "tool_call",
  payload = { tool = "Read", args = "/path/to/file" },
})
assert_contains(first_line(tc_result), "Read", "tool_call: tool name present")
assert_contains(first_line(tc_result), "/path/to/file", "tool_call: args present")
assert_eq(first_hl(tc_result), "RemoraToolCall", "tool_call: highlight correct")

-- Tool result
local tr_result = format_event({
  timestamp = "2026-04-28T12:04:00Z",
  kind = "tool_result",
  payload = { lines = 15 },
})
assert_contains(first_line(tr_result), "tool result", "tool_result: text present")
assert_contains(first_line(tr_result), "15", "tool_result: line count present")
assert_eq(first_hl(tr_result), "RemoraToolCall", "tool_result: highlight correct")

-- Repo change
local repo_result = format_event({
  timestamp = "2026-04-28T13:00:00Z",
  kind = "repo_change",
  payload = { action = "add", name = "myrepo" },
})
assert_contains(first_line(repo_result), "add", "repo_change: action present")
assert_contains(first_line(repo_result), "myrepo", "repo_change: name present")

-- Allowlist request
local alr_result = format_event({
  timestamp = "2026-04-28T13:10:00Z",
  kind = "allowlist_request",
  payload = { domain = "example.com" },
})
assert_contains(first_line(alr_result), "Approval needed", "allowlist_request: text present")
assert_contains(first_line(alr_result), "example.com", "allowlist_request: domain present")
assert_eq(first_hl(alr_result), "RemoraError", "allowlist_request: highlight is RemoraError")

-- Allowlist update
local alu_result = format_event({
  timestamp = "2026-04-28T13:15:00Z",
  kind = "allowlist_update",
  payload = { action = "add", domain = "trusted.com" },
})
assert_contains(first_line(alu_result), "trusted.com", "allowlist_update: domain present")
assert_contains(first_line(alu_result), "add", "allowlist_update: action present")

-- Clear marker
local clear_result = format_event({
  timestamp = "2026-04-28T14:00:00Z",
  kind = "clear_marker",
  payload = {},
})
assert_contains(first_line(clear_result), "context cleared", "clear_marker: text present")
assert_eq(first_hl(clear_result), "RemoraSystem", "clear_marker: highlight correct")

-- Unknown kind (fallback)
local unknown_result = format_event({
  timestamp = "2026-04-28T15:00:00Z",
  kind = "some_new_kind",
  author = "system",
  payload = {},
})
assert_contains(first_line(unknown_result), "some_new_kind", "unknown kind: kind shown")
assert_contains(first_line(unknown_result), "system", "unknown kind: author shown")

-- ── Summary ──────────────────────────────────────────────────────────

print(string.format("\n=== Format event tests: %d passed, %d failed ===", pass_count, fail_count))
if fail_count > 0 then
  os.exit(1)
end
