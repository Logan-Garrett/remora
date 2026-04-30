#!/usr/bin/env bash
# Remora setup script
# Usage: ./scripts/setup.sh [server|client|both]
set -euo pipefail

# ── Colors ────────────────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m' # No Color

info()  { echo -e "${BLUE}[info]${NC}  $*"; }
ok()    { echo -e "${GREEN}[ok]${NC}    $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC}  $*"; }
err()   { echo -e "${RED}[error]${NC} $*"; }
header(){ echo -e "\n${BOLD}── $* ──${NC}\n"; }

# ── Globals ───────────────────────────────────────────────────────────────────

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# ── Prereq checks ────────────────────────────────────────────────────────────

check_command() {
  if command -v "$1" &>/dev/null; then
    ok "$1 found: $(command -v "$1")"
    return 0
  else
    err "$1 not found. $2"
    return 1
  fi
}

check_server_prereqs() {
  header "Checking server prerequisites"
  local failed=0
  check_command rust  "Install from https://rustup.rs"  2>/dev/null \
    || check_command rustc "Install from https://rustup.rs" || failed=1
  check_command cargo "Install from https://rustup.rs" || failed=1
  check_command git   "Install git for your platform"  || failed=1
  if [ "$failed" -ne 0 ]; then
    err "Missing prerequisites. Install them and re-run."
    exit 1
  fi
}

check_client_prereqs() {
  header "Checking client prerequisites"
  local failed=0
  check_command rustc "Install from https://rustup.rs" 2>/dev/null || true
  check_command cargo "Install from https://rustup.rs" || failed=1
  check_command nvim  "Install Neovim 0.9+ from https://neovim.io" || failed=1
  if [ "$failed" -ne 0 ]; then
    err "Missing prerequisites. Install them and re-run."
    exit 1
  fi
}

# ── Helpers ───────────────────────────────────────────────────────────────────

generate_token() {
  if command -v openssl &>/dev/null; then
    openssl rand -hex 24
  elif [ -r /dev/urandom ]; then
    head -c 24 /dev/urandom | xxd -p 2>/dev/null || LC_ALL=C tr -dc 'a-f0-9' </dev/urandom | head -c 48
  else
    # Fallback: use $RANDOM (not cryptographically secure)
    echo "$(date +%s)${RANDOM}${RANDOM}" | shasum -a 256 | cut -c1-48
  fi
}

# ── Server setup ──────────────────────────────────────────────────────────────

setup_server() {
  check_server_prereqs

  header "Database configuration"

  echo "Which database provider?"
  echo "  1) sqlite  (easiest, single-instance, no setup needed)"
  echo "  2) postgres (recommended for production / multi-instance)"
  echo ""
  read -rp "Choice [1]: " db_choice
  db_choice="${db_choice:-1}"

  local db_provider db_url

  case "$db_choice" in
    2|postgres|pg)
      db_provider="postgres"

      if ! check_command psql "psql is needed for Postgres setup" 2>/dev/null; then
        warn "psql not found -- skipping automatic database creation."
        warn "Create the database manually, then set DATABASE_URL in .env."
      fi

      read -rp "Postgres host [127.0.0.1]: " pg_host
      pg_host="${pg_host:-127.0.0.1}"
      read -rp "Postgres port [5432]: " pg_port
      pg_port="${pg_port:-5432}"
      read -rp "Postgres database name [remora]: " pg_db
      pg_db="${pg_db:-remora}"
      read -rp "Postgres user [remora]: " pg_user
      pg_user="${pg_user:-remora}"
      read -rsp "Postgres password [changeme]: " pg_pass
      pg_pass="${pg_pass:-changeme}"
      echo ""

      db_url="postgres://${pg_user}:${pg_pass}@${pg_host}:${pg_port}/${pg_db}"

      # Try to create user/database if psql is available
      if command -v psql &>/dev/null; then
        echo ""
        read -rp "Attempt to create Postgres user/database now? [Y/n]: " do_create
        do_create="${do_create:-Y}"
        if [[ "$do_create" =~ ^[Yy] ]]; then
          info "Creating Postgres user (errors are OK if it already exists)..."
          sudo -u postgres psql -c "CREATE USER ${pg_user} WITH PASSWORD '${pg_pass}';" 2>/dev/null || warn "User may already exist."
          info "Creating Postgres database..."
          sudo -u postgres psql -c "CREATE DATABASE ${pg_db} OWNER ${pg_user};" 2>/dev/null || warn "Database may already exist."
          ok "Postgres setup attempted. Migrations will create tables and extensions on first run."
        fi
      fi
      ;;
    *)
      db_provider="sqlite"
      read -rp "SQLite database path [remora.db]: " sqlite_path
      sqlite_path="${sqlite_path:-remora.db}"
      db_url="sqlite:${sqlite_path}"
      ok "Using SQLite at ${sqlite_path}"
      ;;
  esac

  header "Authentication"

  local team_token
  team_token="$(generate_token)"
  info "Generated team token: ${team_token}"
  read -rp "Use this token? [Y/n]: " use_gen
  use_gen="${use_gen:-Y}"
  if [[ ! "$use_gen" =~ ^[Yy] ]]; then
    read -rp "Enter your team token: " team_token
  fi

  header "Server bind address"
  read -rp "Bind address [0.0.0.0:7200]: " bind_addr
  bind_addr="${bind_addr:-0.0.0.0:7200}"

  header "Workspace directory"
  local default_workspace
  if [ "$db_provider" = "sqlite" ]; then
    default_workspace="${REPO_ROOT}/workspaces"
  else
    default_workspace="/var/lib/remora/workspaces"
  fi
  read -rp "Workspace dir [${default_workspace}]: " workspace_dir
  workspace_dir="${workspace_dir:-${default_workspace}}"

  header "Writing .env"

  local env_file="${REPO_ROOT}/.env"
  if [ -f "$env_file" ]; then
    warn ".env already exists."
    read -rp "Overwrite? [y/N]: " overwrite
    overwrite="${overwrite:-N}"
    if [[ ! "$overwrite" =~ ^[Yy] ]]; then
      info "Keeping existing .env. Skipping write."
    else
      write_env=true
    fi
  else
    write_env=true
  fi

  if [ "${write_env:-false}" = "true" ]; then
    cat > "$env_file" <<EOF
# Remora server configuration
DATABASE_URL=${db_url}
REMORA_TEAM_TOKEN=${team_token}
REMORA_BIND=${bind_addr}
REMORA_DB_PROVIDER=${db_provider}
REMORA_WORKSPACE_DIR=${workspace_dir}
REMORA_RUN_TIMEOUT_SECS=600
REMORA_IDLE_TIMEOUT_SECS=1800
REMORA_GLOBAL_DAILY_CAP=10000000
REMORA_CLAUDE_CMD=claude
REMORA_SKIP_PERMISSIONS=true
EOF
    ok "Wrote ${env_file}"
  fi

  header "Building server"

  cd "$REPO_ROOT"
  info "Running: cargo build --release -p remora-server"
  cargo build --release -p remora-server
  ok "Server binary built: ${REPO_ROOT}/target/release/remora-server"

  header "Server setup complete"

  echo ""
  echo -e "${GREEN}To start the server:${NC}"
  echo ""
  echo "  cd ${REPO_ROOT}"
  echo "  source .env && ./target/release/remora-server"
  echo ""
  echo -e "${YELLOW}Your team token:${NC} ${team_token}"
  echo -e "${YELLOW}Share this token with your team so they can connect.${NC}"
  echo ""
}

# ── Client setup ──────────────────────────────────────────────────────────────

setup_client() {
  check_client_prereqs

  header "Building bridge binary"

  cd "$REPO_ROOT"
  info "Running: cargo build --release -p remora-bridge"
  cargo build --release -p remora-bridge

  local bridge_path="${REPO_ROOT}/target/release/remora-bridge"
  ok "Bridge binary built: ${bridge_path}"

  header "Neovim plugin configuration"

  # Detect Neovim config directory
  local nvim_config=""
  if [ -n "${XDG_CONFIG_HOME:-}" ] && [ -d "${XDG_CONFIG_HOME}/nvim" ]; then
    nvim_config="${XDG_CONFIG_HOME}/nvim"
  elif [ -d "${HOME}/.config/nvim" ]; then
    nvim_config="${HOME}/.config/nvim"
  fi

  if [ -n "$nvim_config" ]; then
    info "Detected Neovim config at: ${nvim_config}"
  else
    warn "Could not detect Neovim config directory."
  fi

  echo ""
  echo -e "${BOLD}Add this to your lazy.nvim plugin spec:${NC}"
  echo ""
  cat <<EOF
{
  "Logan-Garrett/remora",
  dependencies = {
    "nvim-telescope/telescope.nvim",
    "nvim-lua/plenary.nvim",
  },
  config = function()
    require("remora").setup({
      bridge = "${bridge_path}",
    })
    require("telescope").load_extension("remora")
  end,
}
EOF

  echo ""
  header "Server connection"

  read -rp "Server URL [http://localhost:7200]: " server_url
  server_url="${server_url:-http://localhost:7200}"
  read -rp "Team token: " client_token
  read -rp "Your display name [$(hostname)]: " display_name
  display_name="${display_name:-$(hostname)}"

  echo ""
  echo -e "${BOLD}Updated plugin spec with connection details:${NC}"
  echo ""
  cat <<EOF
{
  "Logan-Garrett/remora",
  dependencies = {
    "nvim-telescope/telescope.nvim",
    "nvim-lua/plenary.nvim",
  },
  config = function()
    require("remora").setup({
      bridge = "${bridge_path}",
      url = "${server_url}",
      token = "${client_token}",
      name = "${display_name}",
    })
    require("telescope").load_extension("remora")
  end,
}
EOF

  echo ""
  echo -e "${BOLD}Suggested keybindings:${NC}"
  echo ""
  cat <<'EOF'
vim.keymap.set("n", "<leader>mm", function() require("remora").toggle() end, { desc = "Toggle Remora" })
vim.keymap.set("n", "<leader>ms", "<CMD>Telescope remora sessions<CR>", { desc = "Browse sessions" })
vim.keymap.set("n", "<leader>mc", "<CMD>Telescope remora commands<CR>", { desc = "Remora commands" })
vim.keymap.set("n", "<leader>mn", "<CMD>Telescope remora new<CR>", { desc = "New session" })
vim.keymap.set("n", "<leader>mr", function() require("remora").send_command("/run") end, { desc = "Run Claude" })
vim.keymap.set("n", "<leader>ml", "<CMD>RemoraLeave<CR>", { desc = "Leave session" })
EOF

  echo ""
  ok "Client setup complete. Add the plugin spec and keybindings to your Neovim config."
}

# ── Main ──────────────────────────────────────────────────────────────────────

main() {
  echo -e "${BOLD}"
  echo "  ╔═══════════════════════════════╗"
  echo "  ║       Remora Setup            ║"
  echo "  ║  Collaborative Claude Code    ║"
  echo "  ╚═══════════════════════════════╝"
  echo -e "${NC}"

  local mode="${1:-both}"

  case "$mode" in
    server)
      setup_server
      ;;
    client)
      setup_client
      ;;
    both)
      setup_server
      echo ""
      echo -e "${BOLD}═══════════════════════════════════════════${NC}"
      echo ""
      setup_client
      ;;
    *)
      err "Unknown mode: ${mode}"
      echo "Usage: $0 [server|client|both]"
      exit 1
      ;;
  esac
}

main "$@"
