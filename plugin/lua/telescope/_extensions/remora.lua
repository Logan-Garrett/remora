local has_telescope, telescope = pcall(require, "telescope")
if not has_telescope then
  return
end

local pickers = require("telescope.pickers")
local finders = require("telescope.finders")
local conf = require("telescope.config").values
local actions = require("telescope.actions")
local action_state = require("telescope.actions.state")
local previewers = require("telescope.previewers")

local remora = require("remora")

--- Fetch sessions from the server synchronously via curl.
local function fetch_sessions(url, token)
  local cmd = string.format(
    "curl -s '%s/sessions' -H 'Authorization: Bearer %s'",
    url, token
  )
  local handle = io.popen(cmd)
  if not handle then return {} end
  local result = handle:read("*a")
  handle:close()
  if not result or result == "" then return {} end
  local ok, sessions = pcall(vim.fn.json_decode, result)
  if not ok or type(sessions) ~= "table" then return {} end
  return sessions
end

--- Session picker — browse and join sessions.
local function pick_sessions(opts)
  opts = opts or {}
  local cfg = remora.get_config()
  local url = opts.url or cfg.url or "http://localhost:7200"
  local token = opts.token or cfg.token or os.getenv("REMORA_TEAM_TOKEN") or ""

  local sessions = fetch_sessions(url, token)
  if #sessions == 0 then
    vim.notify("remora: no sessions found", vim.log.levels.INFO)
    return
  end

  pickers.new(opts, {
    prompt_title = "Remora Sessions",
    finder = finders.new_table({
      results = sessions,
      entry_maker = function(session)
        local display = string.format(
          "%-36s  %s",
          session.id or "?",
          session.description or "(no description)"
        )
        return {
          value = session,
          display = display,
          ordinal = (session.description or "") .. " " .. (session.id or ""),
        }
      end,
    }),
    sorter = conf.generic_sorter(opts),
    previewer = previewers.new_buffer_previewer({
      title = "Session Details",
      define_preview = function(self, entry)
        local s = entry.value
        local lines = {
          "Session: " .. (s.id or "?"),
          "Description: " .. (s.description or ""),
          "Created: " .. (s.created_at or ""),
          "",
          "Press <CR> to join this session.",
        }
        vim.api.nvim_buf_set_lines(self.state.bufnr, 0, -1, false, lines)
      end,
    }),
    attach_mappings = function(prompt_bufnr)
      actions.select_default:replace(function()
        local selection = action_state.get_selected_entry()
        actions.close(prompt_bufnr)
        if selection and selection.value then
          local session = selection.value
          local name = cfg.name or vim.fn.hostname()
          vim.defer_fn(function()
            remora.join({
              url = url,
              session_id = session.id,
              token = token,
              name = name,
              bridge = cfg.bridge,
            })
          end, 50)
        end
      end)
      return true
    end,
  }):find()
end

--- Command picker — quick access to all remora commands.
local function pick_commands(opts)
  opts = opts or {}

  local commands = {
    { name = "Run Claude",            cmd = "/run",          icon = "▶",  desc = "Invoke Claude with context since last response" },
    { name = "Run Claude (full)",     cmd = "/run-all",      icon = "▶▶", desc = "Invoke Claude with the full event log" },
    { name = "Diff",                  cmd = "/diff",         icon = "📊", desc = "Show git diff across all repos" },
    { name = "Clear context",         cmd = "/clear",        icon = "🧹", desc = "Reset the 'since last run' baseline" },
    { name = "Who's connected",       cmd = "/who",          icon = "👥", desc = "List connected participants" },
    { name = "Session info",          cmd = "/session info",  icon = "ℹ",  desc = "Show current session metadata" },
    { name = "List repos",            cmd = "/repo list",    icon = "📦", desc = "Show repos in this session" },
    { name = "Add file",              cmd = "/add ",         icon = "📎", desc = "Inline a workspace file as context" },
    { name = "Fetch URL",             cmd = "/fetch ",       icon = "🌐", desc = "Fetch and inline URL content" },
    { name = "Add repo",              cmd = "/repo add ",    icon = "➕", desc = "Clone a git repo into the workspace" },
    { name = "Remove repo",           cmd = "/repo remove ", icon = "➖", desc = "Remove a repo from the workspace" },
    { name = "Show allowlist",        cmd = "/allowlist",    icon = "🔒", desc = "Show fetch allowlist" },
    { name = "Allow domain",          cmd = "/allowlist add ", icon = "✅", desc = "Pre-approve a fetch domain" },
    { name = "Approve fetch",         cmd = "/approve ",     icon = "👍", desc = "Approve a pending fetch domain" },
    { name = "Deny fetch",            cmd = "/deny ",        icon = "👎", desc = "Deny a pending fetch domain" },
    { name = "Kick user",             cmd = "/kick ",        icon = "🚫", desc = "Remove a participant" },
    { name = "Switch session",        cmd = "/join ",        icon = "🔀", desc = "Join a different session" },
    { name = "Leave session",         cmd = ":leave",        icon = "🚪", desc = "Disconnect from current session" },
  }

  pickers.new(opts, {
    prompt_title = "Remora Commands",
    finder = finders.new_table({
      results = commands,
      entry_maker = function(entry)
        local display = string.format("%s  %-22s %s", entry.icon, entry.name, entry.desc)
        return {
          value = entry,
          display = display,
          ordinal = entry.name .. " " .. entry.desc,
        }
      end,
    }),
    sorter = conf.generic_sorter(opts),
    attach_mappings = function(prompt_bufnr)
      actions.select_default:replace(function()
        actions.close(prompt_bufnr)
        local selection = action_state.get_selected_entry()
        if not selection then return end
        local entry = selection.value

        if entry.cmd == ":leave" then
          remora.leave()
          return
        end

        -- Commands that need an argument: pre-fill the prompt
        if entry.cmd:match("%s$") then
          vim.ui.input({ prompt = entry.name .. ": " }, function(input)
            if not input or input == "" then return end
            remora.send_command(entry.cmd .. input)
          end)
        else
          remora.send_command(entry.cmd)
        end
      end)
      return true
    end,
  }):find()
end

--- New session picker — create a session with a nice prompt flow.
local function new_session(opts)
  opts = opts or {}
  local cfg = remora.get_config()
  local url = opts.url or cfg.url or "http://localhost:7200"
  local token = opts.token or cfg.token or os.getenv("REMORA_TEAM_TOKEN") or ""

  vim.ui.input({ prompt = "Git repo URL(s) (space-separated): " }, function(repos_str)
    if not repos_str or repos_str == "" then return end
    vim.ui.input({ prompt = "Session description: " }, function(desc)
      if not desc or desc == "" then return end
      local repos = {}
      for u in repos_str:gmatch("%S+") do
        table.insert(repos, u)
      end
      remora.create_session(url, token, repos, desc)
    end)
  end)
end

return telescope.register_extension({
  exports = {
    sessions = pick_sessions,
    commands = pick_commands,
    new = new_session,
  },
})
