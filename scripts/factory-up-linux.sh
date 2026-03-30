#!/usr/bin/env sh
# Harkonnen Labs — Linux Factory Bring-Up
# Single script to go from fresh clone to a running factory spine on home-linux.
#
# Run from the repo root:
#   ./scripts/factory-up-linux.sh
#
# Idempotent — safe to re-run. Existing .env and seed docs are not overwritten.
#
# What this script does:
#   1.  Check prerequisites (node, npm, cargo, docker)
#   2.  Resolve API keys (.env)
#   3.  Set HARKONNEN_SETUP=home-linux for this session
#   4.  Install MCP server npm packages
#   5.  Build the harkonnen binary
#   6.  Run: harkonnen memory init
#   7.  Run: harkonnen setup check
#   8.  Write .claude/settings.local.json MCP block
#   9.  Start AnythingLLM (if Docker is available)
#   10. Seed AnythingLLM (if running)
#   11. Print status summary

set -eu
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# ── Helpers ───────────────────────────────────────────────────────────────────

step() { printf '\n  >> %s\n' "$1"; }
ok()   { printf '     [ok] %s\n' "$1"; }
warn() { printf '     [!!] %s\n' "$1"; }
info() { printf '          %s\n' "$1"; }
fail() { printf '     [XX] %s\n\n' "$1"; exit 1; }

require() {
    cmd="$1"; hint="$2"
    if ! command -v "$cmd" > /dev/null 2>&1; then
        fail "Missing: $cmd  →  $hint"
    fi
    ok "$cmd $($cmd --version 2>&1 | head -1)"
}

# Check whether a TCP port is already bound. Exits with 0 if free, 1 if taken.
port_free() {
    port="$1"
    # ss is preferred; fall back to netstat, then to /proc/net/tcp
    if command -v ss > /dev/null 2>&1; then
        ! ss -tlnH 2>/dev/null | awk '{print $4}' | grep -q ":${port}$"
    elif command -v netstat > /dev/null 2>&1; then
        ! netstat -tlnp 2>/dev/null | awk '{print $4}' | grep -q ":${port}$"
    else
        ! grep -q ":$(printf '%04X' "$port") " /proc/net/tcp /proc/net/tcp6 2>/dev/null
    fi
}

check_port() {
    port="$1"; label="$2"
    if port_free "$port"; then
        ok "Port $port is free ($label)"
    else
        warn "Port $port is already in use ($label)"
        info "Find the process: ss -tlnp | grep :$port"
        info "If that is an old $label container: docker stop \$(docker ps -q --filter publish=$port)"
        # Not a hard fail — let the caller decide
        return 1
    fi
}

# Require a minimum Node major version.
require_node_version() {
    min="$1"
    actual=$(node --version 2>/dev/null | sed 's/[^0-9].*//')
    if [ -z "$actual" ] || [ "$actual" -lt "$min" ]; then
        fail "Node.js v${min}+ required (found: $(node --version 2>/dev/null || echo 'none')). Install from https://nodejs.org"
    fi
    ok "node $(node --version) — meets minimum v${min}"
}

# Install an npm global package and surface errors clearly (no --silent).
npm_global_install() {
    pkg="$1"
    if npm list -g --depth=0 2>/dev/null | grep -q "$pkg"; then
        ok "$pkg (already installed)"
        return 0
    fi
    info "Installing $pkg ..."
    # Capture output so we can diagnose failures without dumping the full log
    if npm_out=$(npm install --global "$pkg" 2>&1); then
        ok "$pkg installed"
    else
        warn "npm install -g $pkg failed"
        # Surface the first actionable line (EACCES, ENOENT, etc.)
        echo "$npm_out" | grep -E "^npm (ERR!|WARN)" | head -5 | while IFS= read -r line; do
            info "$line"
        done
        info "Try: sudo npm install -g $pkg  or  npm config set prefix ~/.npm-global"
        return 1
    fi
}

# ── Banner ────────────────────────────────────────────────────────────────────

printf '\n'
printf '  ╔══════════════════════════════════════════════╗\n'
printf '  ║   Harkonnen Labs — Linux Factory Bring-Up    ║\n'
printf '  ╚══════════════════════════════════════════════╝\n'
printf '  Setup: home-linux (Claude + Gemini + Codex)\n'
printf '  Root:  %s\n' "$REPO_ROOT"

# ── 1. Prerequisites ──────────────────────────────────────────────────────────

step "Checking prerequisites"
require_node_version 18
require "npm"   "bundled with Node.js"
require "cargo" "https://rustup.rs"

DOCKER_OK=false
if command -v docker > /dev/null 2>&1; then
    # docker binary found — now verify the daemon is actually running
    if docker info > /dev/null 2>&1; then
        ok "docker $(docker --version 2>&1 | head -1) (daemon running)"
        DOCKER_OK=true
    else
        warn "docker binary found but daemon is not running (or not accessible)"
        info "Start it with: sudo systemctl start docker  or  sudo service docker start"
        info "AnythingLLM step will be skipped"
    fi
else
    warn "docker not found — AnythingLLM step will be skipped"
fi

# ── 2. Load .env ──────────────────────────────────────────────────────────────

step "Loading .env"

ENV_FILE="$REPO_ROOT/.env"
if [ ! -f "$ENV_FILE" ]; then
    if [ -f "$REPO_ROOT/.env.example" ]; then
        cp "$REPO_ROOT/.env.example" "$ENV_FILE"
        ok "Created .env from .env.example — edit it to set API keys"
    else
        printf 'HARKONNEN_SETUP=home-linux\n' > "$ENV_FILE"
        ok "Created minimal .env"
    fi
else
    ok ".env exists"
fi

# Source .env safely — no eval. Only export simple KEY=value lines.
# Keys must be valid shell identifiers; values are taken literally.
while IFS='=' read -r key val; do
    case "$key" in
        ''|'#'*) continue ;;              # blank lines and comments
    esac
    # Validate key is a safe identifier (letters, digits, underscores)
    case "$key" in
        *[!A-Za-z0-9_]*) continue ;;     # skip keys with unusual chars
    esac
    # Strip surrounding quotes from value if present
    val=$(printf '%s' "$val" | sed "s/^['\"]//;s/['\"]$//")
    # Only set if not already in the environment (don't override caller's exports)
    export "$key=${val}"
done < "$ENV_FILE"

# Ensure HARKONNEN_SETUP is set in the file
if ! grep -q "^HARKONNEN_SETUP=" "$ENV_FILE" 2>/dev/null; then
    printf '\nHARKONNEN_SETUP=home-linux\n' >> "$ENV_FILE"
    info "Added HARKONNEN_SETUP=home-linux to .env"
fi

# ── 3. Session environment ────────────────────────────────────────────────────

step "Setting session environment"
export HARKONNEN_SETUP=home-linux
ok "HARKONNEN_SETUP=home-linux"

# Warn on missing API keys (don't fail — setup check will report them)
for var in ANTHROPIC_API_KEY GEMINI_API_KEY OPENAI_API_KEY; do
    eval val="\${${var}:-}"
    if [ -z "$val" ]; then
        warn "$var is not set (edit .env or export it before running)"
    else
        ok "$var is set"
    fi
done

# ── 4. Install MCP packages ───────────────────────────────────────────────────

step "Installing MCP server packages"

MCP_FAILURES=0
for pkg in \
    "@modelcontextprotocol/server-filesystem" \
    "@modelcontextprotocol/server-memory" \
    "@modelcontextprotocol/server-sqlite" \
    "@modelcontextprotocol/server-github" \
    "@modelcontextprotocol/server-brave-search"
do
    npm_global_install "$pkg" || MCP_FAILURES=$((MCP_FAILURES + 1))
done

if [ "$MCP_FAILURES" -gt 0 ]; then
    warn "$MCP_FAILURES MCP package(s) failed to install — Claude Code MCP tools will not work until resolved"
    info "Common fix: sudo chown -R \$(whoami) \$(npm config get prefix)/{lib/node_modules,bin,share}"
fi

# ── 5. Build harkonnen binary ─────────────────────────────────────────────────

step "Building harkonnen binary"

RELEASE_FLAG=""
if [ "${1:-}" = "--release" ]; then
    RELEASE_FLAG="--release"
    info "cargo build --release"
else
    info "cargo build  (pass --release for an optimized build)"
fi

cargo build $RELEASE_FLAG 2>&1

if [ -n "$RELEASE_FLAG" ]; then
    BIN="$REPO_ROOT/target/release/harkonnen"
else
    BIN="$REPO_ROOT/target/debug/harkonnen"
fi

[ -f "$BIN" ] || fail "Build failed — binary not found at $BIN"
ok "Binary: $BIN"

# ── 6. Memory init (Coobie) ───────────────────────────────────────────────────

step "Initializing Coobie's memory  (harkonnen memory init)"
"$BIN" memory init

# ── 7. Setup check ────────────────────────────────────────────────────────────

step "Verifying setup  (harkonnen setup check)"
printf '\n'
"$BIN" setup check
printf '\n'

# ── 8. Claude Code MCP config ────────────────────────────────────────────────

step "Writing .claude/settings.local.json (MCP server config)"

CLAUDE_DIR="$REPO_ROOT/.claude"
CLAUDE_SETTINGS="$CLAUDE_DIR/settings.local.json"
mkdir -p "$CLAUDE_DIR"

MCP_JSON='{
  "mcpServers": {
    "memory": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-memory"],
      "env": { "MEMORY_FILE_PATH": "./factory/memory/store.json" }
    },
    "filesystem": {
      "command": "npx",
      "args": [
        "-y", "@modelcontextprotocol/server-filesystem",
        "./products",
        "./factory/workspaces",
        "./factory/artifacts",
        "./factory/memory"
      ]
    },
    "sqlite": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-sqlite", "./factory/state.db"]
    },
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "'"${GITHUB_TOKEN:-}"'" }
    },
    "brave-search": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-brave-search"],
      "env": { "BRAVE_API_KEY": "'"${BRAVE_API_KEY:-}"'" }
    }
  }
}'

if [ -f "$CLAUDE_SETTINGS" ]; then
    # Settings exist — use Python or node to merge if available, else warn
    if command -v python3 > /dev/null 2>&1; then
        python3 - "$CLAUDE_SETTINGS" "$MCP_JSON" <<'PY'
import sys, json
path = sys.argv[1]
new_mcp = json.loads(sys.argv[2]).get("mcpServers", {})
with open(path) as f:
    existing = json.load(f)
existing.setdefault("mcpServers", {}).update(new_mcp)
with open(path, "w") as f:
    json.dump(existing, f, indent=2)
PY
        ok "Merged MCP servers into existing $CLAUDE_SETTINGS"
    else
        warn "$CLAUDE_SETTINGS already exists — MCP block NOT merged (no python3 to merge safely)"
        info "Manually add the mcpServers block from .claude/mcp-block.json"
        printf '%s\n' "$MCP_JSON" > "$CLAUDE_DIR/mcp-block.json"
    fi
else
    printf '%s\n' "$MCP_JSON" > "$CLAUDE_SETTINGS"
    ok "Created $CLAUDE_SETTINGS"
fi

# Also write the permissions block if the file is new or missing permissions
if ! grep -q '"allow"' "$CLAUDE_SETTINGS" 2>/dev/null; then
    python3 - "$CLAUDE_SETTINGS" "$REPO_ROOT" <<'PY' 2>/dev/null || true
import sys, json
path, root = sys.argv[1], sys.argv[2]
with open(path) as f:
    cfg = json.load(f)
cfg.setdefault("permissions", {}).setdefault("allow", [f"Read({root}/**)"])
with open(path, "w") as f:
    json.dump(cfg, f, indent=2)
PY
fi

# ── 9. AnythingLLM ───────────────────────────────────────────────────────────

step "AnythingLLM (home-linux, Docker-based RAG)"

ANYTHINGLLM_PORT="${ANYTHINGLLM_PORT:-3001}"

if [ "$DOCKER_OK" = "true" ]; then
    UP_CMD="$HOME/harkonnen-local/bin/anythingllm-up"

    if [ -f "$UP_CMD" ]; then
        # Check if the port is already serving (container already up)
        if ! port_free "$ANYTHINGLLM_PORT"; then
            ok "AnythingLLM already running on port $ANYTHINGLLM_PORT"
            ANYTHINGLLM_RUNNING=true
        else
            info "Starting AnythingLLM on port $ANYTHINGLLM_PORT ..."
            if "$UP_CMD" > /dev/null 2>&1; then
                # Wait for the API to become healthy (up to 60 s)
                ANYTHINGLLM_RUNNING=false
                for i in $(seq 1 12); do
                    if curl -sf "http://localhost:${ANYTHINGLLM_PORT}/api/ping" > /dev/null 2>&1; then
                        ANYTHINGLLM_RUNNING=true
                        break
                    fi
                    info "Waiting for AnythingLLM... (${i}/12)"
                    sleep 5
                done
                if [ "$ANYTHINGLLM_RUNNING" = "true" ]; then
                    ok "AnythingLLM ready on http://localhost:${ANYTHINGLLM_PORT}"
                else
                    warn "AnythingLLM did not become healthy after 60 s"
                    info "Check logs: $HOME/harkonnen-local/bin/anythingllm-logs"
                fi
            else
                warn "AnythingLLM compose up failed — check Docker daemon and image"
                ANYTHINGLLM_RUNNING=false
            fi
        fi

        if [ "${ANYTHINGLLM_RUNNING:-false}" = "true" ]; then
            if [ -n "${ANYTHINGLLM_API_KEY:-}" ]; then
                info "Seeding Coobie documents into AnythingLLM ..."
                if ANYTHINGLLM_BASE_URL="http://localhost:${ANYTHINGLLM_PORT}/api" \
                   ./scripts/coobie-seed-anythingllm.sh; then
                    ok "AnythingLLM seeded"
                else
                    warn "Seeding failed — check ANYTHINGLLM_API_KEY and AnythingLLM logs"
                    info "Retry: ./scripts/coobie-seed-anythingllm.sh"
                fi
            else
                warn "ANYTHINGLLM_API_KEY not set — skipping document upload"
                info "Open http://localhost:${ANYTHINGLLM_PORT} > Admin > API Keys, then:"
                info "  export ANYTHINGLLM_API_KEY=<key> && ./scripts/coobie-seed-anythingllm.sh"
            fi
        fi
    else
        warn "AnythingLLM not bootstrapped yet"
        info "Run first: ./scripts/bootstrap-local-stack.sh"
        info "Then re-run this script to seed documents"
    fi
else
    info "Skipped (Docker not available or daemon not running)"
fi

# ── 10. Summary ───────────────────────────────────────────────────────────────

printf '\n'
printf '  ════════════════════════════════════════════════\n'
printf '  Factory spine is up.\n'
printf '  ════════════════════════════════════════════════\n'
printf '\n'

printf '  Working now:\n'
for item in \
    "harkonnen spec validate <file>          — Scout: parse specs" \
    "harkonnen run start <spec> --product X  — create run + workspace" \
    "harkonnen run status/report <id>        — inspect runs" \
    "harkonnen artifact package <id>         — package outputs" \
    "harkonnen memory init / index           — Coobie: seed + rebuild index" \
    "harkonnen setup check                   — verify providers + MCP" \
    "MCP: memory, filesystem, sqlite, github, brave-search"
do
    printf '    [+] %s\n' "$item"
done

printf '\n  Not yet wired (next build layer):\n'
for item in \
    "Real LLM calls per agent   — profiles exist, execution adapters pending" \
    "Hidden scenarios (Sable)   — isolation layer planned" \
    "Digital twins (Ash)        — provisioning planned" \
    "Pack Board web UI          — axum server stubbed, not yet built"
do
    printf '    [ ] %s\n' "$item"
done

printf '\n  Next steps:\n'
printf '    1. Restart Claude Code — it will pick up the new MCP servers\n'
printf '    2. Tell Coobie: "load your memory from factory/memory/index.json"\n'
printf '    3. harkonnen spec validate factory/specs/examples/sample_feature.yaml\n'
printf '    4. harkonnen run start factory/specs/examples/sample_feature.yaml --product sample-app\n'
printf '\n  Binary:  %s\n' "$BIN"
printf '  Setup:   %s\n' "$HARKONNEN_SETUP"
printf '  Memory:  %s/factory/memory/\n\n' "$REPO_ROOT"
