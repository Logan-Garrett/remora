local M = {}

-- State
local state = {
  bridge_job = nil,
  log_buf = nil,
  prompt_buf = nil,
  log_win = nil,
  prompt_win = nil,
  name = nil,
  connected = false,
  url = nil,
  token = nil,
  session_id = nil,
  bridge_path = nil,
  reconnect_attempts = 0,
  max_reconnect_attempts = 3,
  reconnect_timer = nil,
  intentional_leave = false, -- set true during leave() to suppress auto-reconnect
  scroll_follow = true, -- auto-scroll to bottom; false when user scrolls up
}

-- ── Highlight groups ──────────────────────────────────────────────────

local function setup_highlights()
  -- Use default=true so user colorscheme overrides take priority.
  -- Link to standard highlight groups for non-truecolor terminal compatibility.
  vim.api.nvim_set_hl(0, "RemoraSystem", { default = true, link = "Comment" })
  vim.api.nvim_set_hl(0, "RemoraAuthor", { default = true, link = "Title" })
  vim.api.nvim_set_hl(0, "RemoraClaudeMsg", { default = true, link = "String" })
  vim.api.nvim_set_hl(0, "RemoraToolCall", { default = true, link = "Function" })
  vim.api.nvim_set_hl(0, "RemoraError", { default = true, link = "ErrorMsg" })
  vim.api.nvim_set_hl(0, "RemoraTimestamp", { default = true, link = "NonText" })
end

-- ── Helpers ───────────────────────────────────────────────────────────

--- Split a string on newlines into a list of lines.
local function split_lines(s)
  local result = {}
  for part in (s .. "\n"):gmatch("([^\n]*)\n") do
    table.insert(result, part)
  end
  return result
end

--- Append a line to the log buffer, optionally applying a highlight group.
--- @param line string
--- @param hl_group string|nil
local function append_log(line, hl_group)
  if not state.log_buf or not vim.api.nvim_buf_is_valid(state.log_buf) then
    return
  end
  -- Split on newlines — nvim_buf_set_lines rejects embedded \n
  local parts = split_lines(line)
  vim.schedule(function()
    if not state.log_buf or not vim.api.nvim_buf_is_valid(state.log_buf) then
      return
    end
    vim.api.nvim_set_option_value("modifiable", true, { buf = state.log_buf })
    local count = vim.api.nvim_buf_line_count(state.log_buf)
    -- If the buffer is empty and the first line is blank, replace it
    local first_line = vim.api.nvim_buf_get_lines(state.log_buf, 0, 1, false)
    if count == 1 and first_line[1] == "" then
      vim.api.nvim_buf_set_lines(state.log_buf, 0, 1, false, parts)
      count = #parts
    else
      vim.api.nvim_buf_set_lines(state.log_buf, -1, -1, false, parts)
      count = count + #parts
    end
    -- Apply highlight to all inserted lines
    local start_line = count - #parts
    for i = 0, #parts - 1 do
      local ln = start_line + i
      local l = parts[i + 1]
      if hl_group then
        vim.api.nvim_buf_add_highlight(state.log_buf, -1, hl_group, ln, 0, -1)
      end
      -- Always apply timestamp highlight to leading [HH:MM:SS]
      local ts_end = l:find("]")
      if l:sub(1, 1) == "[" and ts_end then
        vim.api.nvim_buf_add_highlight(state.log_buf, -1, "RemoraTimestamp", ln, 0, ts_end)
      end
    end
    vim.api.nvim_set_option_value("modifiable", false, { buf = state.log_buf })
    -- Auto-scroll to bottom only if the user hasn't scrolled up
    if state.scroll_follow and state.log_win and vim.api.nvim_win_is_valid(state.log_win) then
      vim.api.nvim_win_set_cursor(state.log_win, { count, 0 })
    end
  end)
end

--- Append multiple lines to the log buffer.
--- @param lines string[]
--- @param hl_group string|nil
local function append_log_lines(lines, hl_group)
  for _, l in ipairs(lines) do
    append_log(l, hl_group)
  end
end

-- ── Event formatting ──────────────────────────────────────────────────

--- Format an event for display in the log buffer.
--- Returns a list of {line, hl_group} tuples.
--- @param event table
--- @return table[]
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
    return { { string.format("[%s] 📎 %s added %s (%s lines)", time, author, path, nlines), nil } }

  elseif kind == "fetch" then
    local nbytes = payload.bytes or payload.size or "?"
    local url = payload.url or "?"
    return { { string.format("[%s] 🌐 %s fetched %s (%s bytes)", time, author, url, nbytes), nil } }

  elseif kind == "diff" then
    local result = { { string.format("[%s] 📊 diff output", time), nil } }
    local text = payload.text or payload.diff or ""
    for diff_line in text:gmatch("[^\n]+") do
      table.insert(result, { diff_line, nil })
    end
    return result

  elseif kind == "claude_request" then
    return { { string.format("[%s] ▶ Claude run started", time), "RemoraClaudeMsg" } }

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
      -- empty text
      table.insert(result, { string.format("[%s] Claude: (empty)", time), "RemoraClaudeMsg" })
    end
    return result

  elseif kind == "tool_call" then
    local tool = payload.tool or payload.name or "?"
    local args = payload.args or payload.arguments or ""
    if type(args) == "table" then
      args = vim.fn.json_encode(args)
    end
    return { { string.format("[%s] 🔧 Claude called %s(%s)", time, tool, args), "RemoraToolCall" } }

  elseif kind == "tool_result" then
    local nlines = payload.lines or "?"
    return { { string.format("[%s] ← tool result (%s lines)", time, nlines), "RemoraToolCall" } }

  elseif kind == "repo_change" then
    local action = payload.action or "changed"
    local name = payload.name or payload.repo or "?"
    return { { string.format("[%s] 📦 repo %s: %s", time, action, name), nil } }

  elseif kind == "allowlist_request" then
    local domain = payload.domain or "?"
    return { { string.format("[%s] ⚠ Approval needed: %s (use /approve or /deny)", time, domain), "RemoraError" } }

  elseif kind == "allowlist_update" then
    local action = payload.action or "updated"
    local domain = payload.domain or "?"
    return { { string.format("[%s] 🔒 allowlist: %s %s", time, domain, action), nil } }

  elseif kind == "clear_marker" then
    return { { string.format("[%s] --- context cleared ---", time), "RemoraSystem" } }

  else
    return { { string.format("[%s] [%s] %s: %s", time, kind, author, vim.fn.json_encode(payload)), nil } }
  end
end

-- ── Bridge communication ──────────────────────────────────────────────

--- Send a JSON message through the bridge stdin.
--- @param msg table
local function bridge_send(msg)
  if not state.bridge_job then
    vim.notify("remora: not connected", vim.log.levels.WARN)
    return
  end
  local encoded = vim.fn.json_encode(msg)
  vim.fn.chansend(state.bridge_job, encoded .. "\n")
end

--- Send a chat message through the bridge.
--- @param text string
local function send_chat(text)
  bridge_send({
    type = "chat",
    author = state.name,
    text = text,
  })
end

-- ── Slash command handling ─────────────────────────────────────────────

--- Parse and dispatch a slash command. Returns true if handled.
--- @param text string
--- @return boolean
local function handle_slash_command(text)
  -- Trim leading/trailing whitespace
  local trimmed = text:match("^%s*(.-)%s*$")
  if not trimmed or trimmed:sub(1, 1) ~= "/" then
    return false
  end

  local author = state.name or "anon"

  -- Help
  if trimmed == "/help" or trimmed == "/?" then
    local help = {
      "── Remora Commands ──────────────────────────",
      "/run              Run Claude (since last response)",
      "/run-all          Run Claude (full log)",
      "/clear            Clear context baseline",
      "/diff             Show git diff across repos",
      "/add <path>       Add file as context",
      "/fetch <url>      Fetch URL content",
      "/who              List connected participants",
      "/session info     Current session details",
      "/repo list        List repos",
      "/repo add <url>   Clone a repo into workspace",
      "/repo remove <n>  Remove a repo",
      "/allowlist        Show fetch allowlist",
      "/allowlist add <d> Allow a fetch domain",
      "/approve <domain> Approve pending fetch",
      "/deny <domain>    Deny pending fetch",
      "/kick <name>      Kick a participant",
      "/join <id>        Switch to another session",
      "/sessions         List all sessions",
      "/help             Show this list",
      "─────────────────────────────────────────────",
    }
    for _, line in ipairs(help) do
      append_log(line, "RemoraSystem")
    end
    return true
  end

  -- Simple commands (no arguments)
  if trimmed == "/run" then
    bridge_send({ type = "run", author = author })
    return true
  end

  if trimmed == "/run-all" or trimmed == "/run_all" or trimmed == "/runall" then
    bridge_send({ type = "run_all", author = author })
    return true
  end

  if trimmed == "/clear" then
    bridge_send({ type = "clear", author = author })
    return true
  end

  if trimmed == "/diff" then
    bridge_send({ type = "diff", author = author })
    return true
  end

  if trimmed == "/who" then
    bridge_send({ type = "who", author = author })
    return true
  end

  if trimmed == "/allowlist" then
    bridge_send({ type = "allowlist", author = author })
    return true
  end

  if trimmed == "/repo list" then
    bridge_send({ type = "repo_list", author = author })
    return true
  end

  if trimmed == "/session info" then
    bridge_send({ type = "session_info", author = author })
    return true
  end

  -- Commands with arguments
  local add_path = trimmed:match("^/add%s+(.+)$")
  if add_path then
    bridge_send({ type = "add", author = author, path = add_path })
    return true
  end

  local fetch_url = trimmed:match("^/fetch%s+(.+)$")
  if fetch_url then
    bridge_send({ type = "fetch", author = author, url = fetch_url })
    return true
  end

  local repo_add_url = trimmed:match("^/repo%s+add%s+(.+)$")
  if repo_add_url then
    bridge_send({ type = "repo_add", author = author, git_url = repo_add_url })
    return true
  end

  local repo_remove_name = trimmed:match("^/repo%s+remove%s+(.+)$")
  if repo_remove_name then
    bridge_send({ type = "repo_remove", author = author, name = repo_remove_name })
    return true
  end

  local allowlist_add_domain = trimmed:match("^/allowlist%s+add%s+(.+)$")
  if allowlist_add_domain then
    bridge_send({ type = "allowlist_add", author = author, domain = allowlist_add_domain })
    return true
  end

  local allowlist_remove_domain = trimmed:match("^/allowlist%s+remove%s+(.+)$")
  if allowlist_remove_domain then
    bridge_send({ type = "allowlist_remove", author = author, domain = allowlist_remove_domain })
    return true
  end

  local approve_domain = trimmed:match("^/approve%s+(.+)$")
  if approve_domain then
    bridge_send({ type = "approve", author = author, domain = approve_domain, approved = true })
    return true
  end

  local deny_domain = trimmed:match("^/deny%s+(.+)$")
  if deny_domain then
    bridge_send({ type = "approve", author = author, domain = deny_domain, approved = false })
    return true
  end

  local kick_target = trimmed:match("^/kick%s+(.+)$")
  if kick_target then
    bridge_send({ type = "kick", author = author, target = kick_target })
    return true
  end

  -- /join <session_id> — disconnect and reconnect to a new session
  local join_id = trimmed:match("^/join%s+(.+)$")
  if join_id then
    if state.bridge_job then
      M.leave()
    end
    -- Short delay so the old connection closes cleanly before reconnecting
    vim.defer_fn(function()
      M.join({
        url = state.url,
        session_id = join_id,
        token = state.token,
        name = state.name,
        bridge = state.bridge_path,
      })
    end, 200)
    return true
  end

  -- /sessions — list sessions via REST (curl)
  if trimmed == "/sessions" then
    M.list_sessions(state.url, state.token)
    return true
  end

  -- /session new <git-url> [<git-url>...] "<description>"
  local session_new_args = trimmed:match("^/session%s+new%s+(.+)$")
  if session_new_args then
    -- Parse: everything before the last quoted string is git urls, the quoted string is the description
    local desc = session_new_args:match('"([^"]+)"') or session_new_args:match("'([^']+)'")
    local urls_part = session_new_args:gsub('"[^"]*"', ""):gsub("'[^']*'", "")
    local repos = {}
    for u in urls_part:gmatch("%S+") do
      table.insert(repos, u)
    end
    if desc and #repos > 0 then
      M.create_session(state.url, state.token, repos, desc)
    else
      append_log('Usage: /session new <git-url> [<git-url>...] "<description>"', "RemoraError")
    end
    return true
  end

  -- Unknown slash command
  append_log("Unknown command: " .. trimmed, "RemoraError")
  return true
end

-- ── Bridge stdout/exit handlers ───────────────────────────────────────

--- Handle a line of JSON from the bridge process.
local function on_bridge_stdout(_, data, _)
  if not data then return end
  for _, line in ipairs(data) do
    if line ~= "" then
      local ok, msg = pcall(vim.fn.json_decode, line)
      if ok and msg then
        if msg.type == "event" and msg.data then
          local formatted = format_event(msg.data)
          for _, entry in ipairs(formatted) do
            append_log(entry[1], entry[2])
          end
          -- Notify when Claude responds and the window is hidden
          local kind = msg.data.kind or ""
          if kind == "claude_response" then
            local win_visible = state.log_win and vim.api.nvim_win_is_valid(state.log_win)
            if not win_visible then
              vim.notify("remora: Claude responded", vim.log.levels.INFO)
            end
          end
        elseif msg.type == "error" then
          append_log("ERROR: " .. (msg.message or "unknown"), "RemoraError")
        end
      end
    end
  end
end

local function attempt_reconnect()
  if state.reconnect_attempts >= state.max_reconnect_attempts then
    append_log("-- max reconnect attempts reached, giving up --", "RemoraError")
    state.reconnect_attempts = 0
    return
  end
  if not state.url or not state.session_id or not state.token then
    return
  end
  state.reconnect_attempts = state.reconnect_attempts + 1
  append_log(string.format("-- reconnecting (attempt %d/%d) --",
    state.reconnect_attempts, state.max_reconnect_attempts), "RemoraSystem")
  M.join({
    url = state.url,
    session_id = state.session_id,
    token = state.token,
    name = state.name,
    bridge = state.bridge_path,
    _reconnect = true,
  })
end

local function on_bridge_exit(_, code, _)
  state.connected = false
  state.bridge_job = nil
  append_log("-- disconnected (exit " .. tostring(code) .. ") --", "RemoraSystem")
  -- Only auto-reconnect on unexpected exit, not intentional leave
  if state.intentional_leave then
    state.intentional_leave = false
    state.reconnect_attempts = 0
    return
  end
  if code ~= 0 then
    if state.reconnect_timer then
      vim.fn.timer_stop(state.reconnect_timer)
    end
    state.reconnect_timer = vim.fn.timer_start(2000, function()
      state.reconnect_timer = nil
      vim.schedule(function()
        attempt_reconnect()
      end)
    end)
  else
    state.reconnect_attempts = 0
  end
end

-- ── Prompt setup ──────────────────────────────────────────────────────

--- Handle message submission from the prompt buffer.
--- @param buf number
local function submit_prompt(buf)
  local lines = vim.api.nvim_buf_get_lines(buf, 0, -1, false)
  local text = table.concat(lines, "\n")
  if text:match("^%s*$") then return end
  if not handle_slash_command(text) then
    send_chat(text)
  end
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "" })
end

--- Set up the prompt buffer keymap.
local function setup_prompt(buf)
  vim.keymap.set("n", "<CR>", function()
    submit_prompt(buf)
  end, { buffer = buf, desc = "Send remora message" })

  vim.keymap.set("i", "<CR>", function()
    submit_prompt(buf)
  end, { buffer = buf, desc = "Send remora message" })

  -- Shift-Enter for newline in insert mode (actual multiline), CR sends
  vim.keymap.set("i", "<S-CR>", "<CR>", { buffer = buf, desc = "Newline in prompt" })
end

-- ── Layout ────────────────────────────────────────────────────────────

--- Close the floating windows if they exist.
local function close_layout()
  for _, win in ipairs({ state.log_win, state.prompt_win }) do
    if win and vim.api.nvim_win_is_valid(win) then
      vim.api.nvim_win_close(win, true)
    end
  end
  state.log_win = nil
  state.prompt_win = nil
end

--- Create a Telescope-style floating layout: bordered log + bordered prompt.
local function create_layout()
  -- Wipe stale buffers from a previous session to avoid "name already exists"
  for _, buf in ipairs({ state.log_buf, state.prompt_buf }) do
    if buf and vim.api.nvim_buf_is_valid(buf) then
      vim.api.nvim_buf_delete(buf, { force = true })
    end
  end
  state.log_buf = nil
  state.prompt_buf = nil

  local ui = vim.api.nvim_list_uis()[1] or {}
  local editor_w = ui.width or vim.o.columns
  local editor_h = ui.height or vim.o.lines

  -- Dimensions — 80% of editor, centered
  local width = math.floor(editor_w * 0.8)
  local total_height = math.floor(editor_h * 0.8)
  local prompt_h = 3
  local log_h = total_height - prompt_h - 2 -- 2 for the gap between windows

  local col = math.floor((editor_w - width) / 2)
  local row = math.floor((editor_h - total_height) / 2)

  -- Log buffer
  state.log_buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_set_option_value("buftype", "nofile", { buf = state.log_buf })
  vim.api.nvim_set_option_value("bufhidden", "hide", { buf = state.log_buf })
  vim.api.nvim_set_option_value("modifiable", false, { buf = state.log_buf })
  vim.api.nvim_buf_set_name(state.log_buf, "remora://log")

  -- Prompt buffer
  state.prompt_buf = vim.api.nvim_create_buf(false, true)
  vim.api.nvim_set_option_value("buftype", "nofile", { buf = state.prompt_buf })
  vim.api.nvim_set_option_value("bufhidden", "hide", { buf = state.prompt_buf })
  vim.api.nvim_buf_set_name(state.prompt_buf, "remora://prompt")

  -- Log floating window
  state.log_win = vim.api.nvim_open_win(state.log_buf, false, {
    relative = "editor",
    width = width,
    height = log_h,
    col = col,
    row = row,
    style = "minimal",
    border = "rounded",
    title = " Remora ",
    title_pos = "center",
  })
  vim.api.nvim_set_option_value("wrap", true, { win = state.log_win })
  vim.api.nvim_set_option_value("cursorline", true, { win = state.log_win })
  vim.api.nvim_set_option_value("winhighlight", "Normal:Normal,FloatBorder:FloatBorder", { win = state.log_win })

  -- Prompt floating window (just below the log)
  state.prompt_win = vim.api.nvim_open_win(state.prompt_buf, true, {
    relative = "editor",
    width = width,
    height = prompt_h,
    col = col,
    row = row + log_h + 2,
    style = "minimal",
    border = "rounded",
    title = " > ",
    title_pos = "left",
  })
  vim.api.nvim_set_option_value("winhighlight", "Normal:Normal,FloatBorder:FloatBorder", { win = state.prompt_win })

  setup_prompt(state.prompt_buf)
  setup_highlights()

  -- Close keymaps on both buffers, both modes
  for _, buf in ipairs({ state.prompt_buf, state.log_buf }) do
    vim.keymap.set("n", "<Esc>", function() close_layout() end, { buffer = buf, desc = "Close remora" })
    vim.keymap.set("n", "q", function() close_layout() end, { buffer = buf, desc = "Close remora" })
  end

  -- Scroll follow: G re-enables, scrolling up disables
  vim.keymap.set("n", "G", function()
    state.scroll_follow = true
    local count = vim.api.nvim_buf_line_count(state.log_buf)
    if state.log_win and vim.api.nvim_win_is_valid(state.log_win) then
      vim.api.nvim_win_set_cursor(state.log_win, { count, 0 })
    end
  end, { buffer = state.log_buf, desc = "Follow new messages" })

  vim.api.nvim_create_autocmd("CursorMoved", {
    buffer = state.log_buf,
    callback = function()
      if not state.log_win or not vim.api.nvim_win_is_valid(state.log_win) then return end
      local cursor = vim.api.nvim_win_get_cursor(state.log_win)[1]
      local total = vim.api.nvim_buf_line_count(state.log_buf)
      state.scroll_follow = (cursor >= total - 1)
    end,
  })
  -- Also close both when either window is closed externally (e.g. :q)
  vim.api.nvim_create_autocmd("WinClosed", {
    buffer = state.log_buf,
    once = true,
    callback = function() vim.schedule(close_layout) end,
  })
  vim.api.nvim_create_autocmd("WinClosed", {
    buffer = state.prompt_buf,
    once = true,
    callback = function() vim.schedule(close_layout) end,
  })

  -- Start in insert mode in the prompt
  vim.cmd("startinsert")
end

--- Toggle the floating window open/closed without disconnecting.
function M.toggle()
  if state.log_win and vim.api.nvim_win_is_valid(state.log_win) then
    close_layout()
  elseif state.connected and state.log_buf and vim.api.nvim_buf_is_valid(state.log_buf) then
    -- Reopen the windows with existing buffers
    local ui = vim.api.nvim_list_uis()[1] or {}
    local editor_w = ui.width or vim.o.columns
    local editor_h = ui.height or vim.o.lines
    local width = math.floor(editor_w * 0.8)
    local total_height = math.floor(editor_h * 0.8)
    local prompt_h = 3
    local log_h = total_height - prompt_h - 2
    local col = math.floor((editor_w - width) / 2)
    local row = math.floor((editor_h - total_height) / 2)

    state.log_win = vim.api.nvim_open_win(state.log_buf, false, {
      relative = "editor",
      width = width,
      height = log_h,
      col = col,
      row = row,
      style = "minimal",
      border = "rounded",
      title = " Remora ",
      title_pos = "center",
    })
    vim.api.nvim_set_option_value("wrap", true, { win = state.log_win })
    vim.api.nvim_set_option_value("cursorline", true, { win = state.log_win })

    state.prompt_win = vim.api.nvim_open_win(state.prompt_buf, true, {
      relative = "editor",
      width = width,
      height = prompt_h,
      col = col,
      row = row + log_h + 2,
      style = "minimal",
      border = "rounded",
      title = " > ",
      title_pos = "left",
    })

    -- Re-bind close keys
    for _, buf in ipairs({ state.prompt_buf, state.log_buf }) do
      vim.keymap.set("n", "<Esc>", function() close_layout() end, { buffer = buf })
      vim.keymap.set("n", "q", function() close_layout() end, { buffer = buf })
    end

    -- Scroll log to bottom
    local count = vim.api.nvim_buf_line_count(state.log_buf)
    if count > 0 then
      vim.api.nvim_win_set_cursor(state.log_win, { count, 0 })
    end
    vim.cmd("startinsert")
  else
    -- Not connected — open session picker if Telescope is available
    local ok, telescope = pcall(require, "telescope")
    if ok then
      telescope.extensions.remora.sessions()
    else
      vim.notify("remora: not connected. Use <leader>ms to browse sessions.", vim.log.levels.INFO)
    end
  end
end

-- ── Public API ────────────────────────────────────────────────────────

local _setup_opts = {}

--- Return current config for use by Telescope extension.
--- @return table
function M.get_config()
  return {
    url = state.url or _setup_opts.url or os.getenv("REMORA_URL"),
    token = state.token or _setup_opts.token or os.getenv("REMORA_TEAM_TOKEN"),
    name = state.name or _setup_opts.name or os.getenv("REMORA_NAME") or vim.fn.hostname(),
    bridge = state.bridge_path or _setup_opts.bridge,
  }
end

--- Return connection status for use in the statusline.
--- @return string
function M.statusline()
  if not state.connected then return "" end
  return string.format("remora: %s [%s]", state.session_id or "?", state.name or "?")
end

--- Connect to a remora session.
--- @param opts table { url: string, session_id: string, token: string, name: string, bridge: string|nil, _reconnect: boolean|nil }
function M.join(opts)
  if state.bridge_job then
    vim.notify("remora: already connected. Use :RemoraLeave first.", vim.log.levels.WARN)
    return
  end

  state.name = opts.name or "anon"
  state.url = opts.url
  state.token = opts.token
  state.session_id = opts.session_id
  state.bridge_path = opts.bridge or "remora-bridge"

  local ws_url = string.format(
    "%s/sessions/%s?token=%s&name=%s",
    opts.url:gsub("^http", "ws"),
    opts.session_id,
    opts.token,
    state.name
  )

  -- Only create layout on first join, not on reconnects
  if not opts._reconnect then
    create_layout()
  end
  append_log("-- connecting to " .. opts.session_id .. " --", "RemoraSystem")

  state.bridge_job = vim.fn.jobstart({ state.bridge_path, ws_url }, {
    on_stdout = on_bridge_stdout,
    on_exit = on_bridge_exit,
    stdout_buffered = false,
  })

  if state.bridge_job <= 0 then
    append_log("-- failed to start bridge binary --", "RemoraError")
    state.bridge_job = nil
    return
  end

  state.connected = true
  state.reconnect_attempts = 0
end

--- Disconnect from the current session.
function M.leave()
  -- Cancel any pending reconnect timer
  if state.reconnect_timer then
    vim.fn.timer_stop(state.reconnect_timer)
    state.reconnect_timer = nil
  end
  state.reconnect_attempts = 0
  state.intentional_leave = true
  if state.bridge_job then
    vim.fn.jobstop(state.bridge_job)
    -- bridge_job is cleared by on_bridge_exit callback
  end
  state.connected = false
  -- Wipe buffers so rejoin can create fresh ones
  for _, buf in ipairs({ state.log_buf, state.prompt_buf }) do
    if buf and vim.api.nvim_buf_is_valid(buf) then
      vim.api.nvim_buf_delete(buf, { force = true })
    end
  end
  state.log_buf = nil
  state.prompt_buf = nil
  state.log_win = nil
  state.prompt_win = nil
end

--- List sessions from the server via REST (curl).
--- @param url string   Base HTTP URL of the remora server
--- @param token string  Auth token
function M.list_sessions(url, token)
  if not url or not token then
    vim.notify("remora: url and token required", vim.log.levels.ERROR)
    return
  end
  local curl_args = {
    "curl", "-s",
    url .. "/sessions",
    "-H", "Authorization: Bearer " .. token,
  }
  local stdout_chunks = {}
  vim.fn.jobstart(curl_args, {
    on_stdout = function(_, data)
      if data then
        for _, chunk in ipairs(data) do
          if chunk ~= "" then
            table.insert(stdout_chunks, chunk)
          end
        end
      end
    end,
    on_exit = function()
      vim.schedule(function()
        local body = table.concat(stdout_chunks, "")
        if body == "" then
          append_log("-- no sessions or request failed --", "RemoraSystem")
          return
        end
        local ok, sessions = pcall(vim.fn.json_decode, body)
        if not ok or type(sessions) ~= "table" then
          append_log("-- failed to parse sessions response --", "RemoraError")
          return
        end
        append_log("-- sessions --", "RemoraSystem")
        for _, s in ipairs(sessions) do
          local id = s.id or "?"
          local desc = s.description or ""
          local created = s.created_at or ""
          append_log(string.format("  %s  %s  (%s)", id, desc, created), nil)
        end
        append_log("-- end sessions --", "RemoraSystem")
      end)
    end,
    stdout_buffered = false,
  })
end

--- Create a new session on the server via REST (curl), then auto-join.
--- @param url string        Base HTTP URL
--- @param token string      Auth token
--- @param repos string[]    List of git URLs
--- @param description string Session description
function M.create_session(url, token, repos, description)
  if not url or not token then
    vim.notify("remora: url and token required", vim.log.levels.ERROR)
    return
  end
  local body = vim.fn.json_encode({
    repos = repos,
    description = description,
  })
  local curl_args = {
    "curl", "-s",
    "-X", "POST",
    url .. "/sessions",
    "-H", "Authorization: Bearer " .. token,
    "-H", "Content-Type: application/json",
    "-d", body,
  }
  local stdout_chunks = {}
  vim.fn.jobstart(curl_args, {
    on_stdout = function(_, data)
      if data then
        for _, chunk in ipairs(data) do
          if chunk ~= "" then
            table.insert(stdout_chunks, chunk)
          end
        end
      end
    end,
    on_exit = function()
      vim.schedule(function()
        local resp_body = table.concat(stdout_chunks, "")
        if resp_body == "" then
          append_log("-- session creation failed (empty response) --", "RemoraError")
          return
        end
        local ok, result = pcall(vim.fn.json_decode, resp_body)
        if not ok or type(result) ~= "table" then
          append_log("-- failed to parse create-session response --", "RemoraError")
          return
        end
        local new_id = result.id
        if not new_id then
          append_log("-- session created but no id returned --", "RemoraError")
          return
        end
        append_log("-- session created: " .. tostring(new_id) .. " --", "RemoraSystem")
        -- Auto-join the new session
        if state.bridge_job then
          M.leave()
        end
        vim.defer_fn(function()
          M.join({
            url = url,
            session_id = tostring(new_id),
            token = token,
            name = state.name,
            bridge = state.bridge_path,
          })
        end, 200)
      end)
    end,
    stdout_buffered = false,
  })
end

--- Check if currently connected to a session.
--- @return boolean
function M.is_connected()
  return state.connected
end

--- Send a slash command or chat message programmatically.
--- @param text string
function M.send_command(text)
  if not state.connected then
    vim.notify("remora: not connected", vim.log.levels.WARN)
    return
  end
  if not handle_slash_command(text) then
    send_chat(text)
  end
end

-- ── User command setup ────────────────────────────────────────────────

--- Register all user commands and load Telescope extension.
function M.setup(opts)
  opts = opts or {}
  _setup_opts = opts

  vim.api.nvim_create_user_command("RemoraJoin", function(cmd)
    local args = vim.split(cmd.args, " ")
    if #args < 3 then
      vim.notify("Usage: :RemoraJoin <url> <session_id> <token> [name]", vim.log.levels.ERROR)
      return
    end
    M.join({
      url = args[1],
      session_id = args[2],
      token = args[3],
      name = args[4] or opts.name or vim.fn.hostname(),
      bridge = opts.bridge,
    })
  end, { nargs = "+" })

  vim.api.nvim_create_user_command("RemoraLeave", function()
    M.leave()
  end, {})

  vim.api.nvim_create_user_command("RemoraSend", function(cmd)
    if not handle_slash_command(cmd.args) then
      send_chat(cmd.args)
    end
  end, { nargs = "+" })

  vim.api.nvim_create_user_command("RemoraSessions", function(cmd)
    local args = vim.split(cmd.args, " ")
    local url = args[1]
    local token = args[2] or state.token
    if not url then
      vim.notify("Usage: :RemoraSessions <url> [token]", vim.log.levels.ERROR)
      return
    end
    M.list_sessions(url, token)
  end, { nargs = "+" })

  vim.api.nvim_create_user_command("RemoraNew", function(cmd)
    -- Parse: :RemoraNew <url> <git-url> [<git-url>...] "<description>"
    local raw = cmd.args
    -- Extract the quoted description
    local desc = raw:match('"([^"]+)"') or raw:match("'([^']+)'")
    if not desc then
      vim.notify('Usage: :RemoraNew <url> <git-url> [<git-url>...] "<description>"', vim.log.levels.ERROR)
      return
    end
    -- Remove the quoted part and parse remaining tokens
    local rest = raw:gsub('"[^"]*"', ""):gsub("'[^']*'", "")
    local tokens = {}
    for tok in rest:gmatch("%S+") do
      table.insert(tokens, tok)
    end
    if #tokens < 2 then
      vim.notify('Usage: :RemoraNew <url> <git-url> [<git-url>...] "<description>"', vim.log.levels.ERROR)
      return
    end
    local url = tokens[1]
    local token = state.token
    local repos = {}
    for i = 2, #tokens do
      table.insert(repos, tokens[i])
    end
    M.create_session(url, token, repos, desc)
  end, { nargs = "+" })
end

return M
