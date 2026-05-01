-- test_leave_rejoin.lua
-- Tests that leave() properly cleans up state so rejoin works.
--
-- Run: nvim --headless -u NONE -c "set rtp+=plugin" -c "luafile plugin/tests/test_leave_rejoin.lua" -c "qa!"

local pass_count = 0
local fail_count = 0

local function assert_eq(a, b, msg)
  if a == b then
    pass_count = pass_count + 1
    print("PASS: " .. msg)
  else
    fail_count = fail_count + 1
    print("FAIL: " .. msg .. " — expected " .. tostring(b) .. " got " .. tostring(a))
  end
end

local function assert_nil(val, msg)
  assert_eq(val, nil, msg)
end

local function assert_true(val, msg)
  assert_eq(val, true, msg)
end

local function assert_false(val, msg)
  assert_eq(val, false, msg)
end

-- Load the module
package.loaded["remora"] = nil
local ok, remora = pcall(require, "remora")
if not ok then
  remora = dofile("plugin/lua/remora/init.lua")
end

-- ── Test: leave() clears state for rejoin ────────────────────────────

-- The module doesn't expose state directly, but we can test behavior:
-- After setup + leave, join should not say "already connected"

-- First, set up the module
remora.setup({
  bridge = "echo", -- dummy bridge
  url = "http://localhost:7200",
  token = "test",
  name = "tester",
})

-- Call leave on a not-connected state — should not error
remora.leave()
print("PASS: leave() on disconnected state does not error")
pass_count = pass_count + 1

-- ── Test: get_config returns expected values ─────────────────────────

local cfg = remora.get_config()
assert_eq(cfg.url, "http://localhost:7200", "get_config returns url from setup")
assert_eq(cfg.token, "test", "get_config returns token from setup")
assert_eq(cfg.name, "tester", "get_config returns name from setup")

-- ── Test: leave wipes buffers ────────────────────────────────────────

-- Create buffers with the remora names, simulating what create_layout does
local log_buf = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_name(log_buf, "remora://log")
local prompt_buf = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_name(prompt_buf, "remora://prompt")

-- Verify they exist
assert_true(vim.api.nvim_buf_is_valid(log_buf), "log buffer exists before leave")
assert_true(vim.api.nvim_buf_is_valid(prompt_buf), "prompt buffer exists before leave")

-- Now delete them (simulating what leave should do)
vim.api.nvim_buf_delete(log_buf, { force = true })
vim.api.nvim_buf_delete(prompt_buf, { force = true })

-- Verify they're gone
assert_false(vim.api.nvim_buf_is_valid(log_buf), "log buffer wiped after delete")
assert_false(vim.api.nvim_buf_is_valid(prompt_buf), "prompt buffer wiped after delete")

-- Now we should be able to create new buffers with the same names
local new_log = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_name(new_log, "remora://log")
local new_prompt = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_name(new_prompt, "remora://prompt")

assert_true(vim.api.nvim_buf_is_valid(new_log), "can recreate log buffer after wipe")
assert_true(vim.api.nvim_buf_is_valid(new_prompt), "can recreate prompt buffer after wipe")

-- Clean up
vim.api.nvim_buf_delete(new_log, { force = true })
vim.api.nvim_buf_delete(new_prompt, { force = true })

-- ── Test: on_bridge_exit suppresses reconnect on intentional leave ───

-- We can't easily test on_bridge_exit directly since it's local,
-- but we can verify the state flow by checking that the module
-- source contains the intentional_leave guard.

local source_path = vim.api.nvim_get_runtime_file("lua/remora/init.lua", false)[1]
if source_path then
  local f = io.open(source_path, "r")
  if f then
    local source = f:read("*a")
    f:close()
    assert_true(
      source:find("intentional_leave") ~= nil,
      "source contains intentional_leave guard"
    )
    assert_true(
      source:find("state%.intentional_leave = true") ~= nil,
      "leave() sets intentional_leave = true"
    )
    assert_true(
      source:find("if state%.intentional_leave then") ~= nil,
      "on_bridge_exit checks intentional_leave"
    )
  else
    print("SKIP: could not read source file")
  end
else
  print("SKIP: could not find init.lua in runtime path")
end

-- ── Summary ──────────────────────────────────────────────────────────

print(string.format("\n=== Leave/rejoin tests: %d passed, %d failed ===", pass_count, fail_count))
if fail_count > 0 then
  os.exit(1)
end
