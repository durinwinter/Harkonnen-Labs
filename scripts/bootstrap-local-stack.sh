#!/usr/bin/env sh
set -eu

BASE_DIR=${1:-"/media/earthling/Caleb's Files/harkonnen-local"}
OPENCLAW_PREFIX="$BASE_DIR/openclaw"
ANYTHINGLLM_DIR="$BASE_DIR/anythingllm"
ANYTHINGLLM_STORAGE="$ANYTHINGLLM_DIR/storage"
BIN_DIR="$BASE_DIR/bin"
COMPOSE_FILE="$ANYTHINGLLM_DIR/docker-compose.yml"
ENV_FILE="$ANYTHINGLLM_DIR/.env"
OPENCLAW_INSTALLER_URL="https://openclaw.ai/install-cli.sh"
ANYTHINGLLM_IMAGE="mintplexlabs/anythingllm:latest"
ANYTHINGLLM_PORT="${ANYTHINGLLM_PORT:-3001}"
HOST_UID=$(id -u)
HOST_GID=$(id -g)

# ── Helpers ───────────────────────────────────────────────────────────────────

ok()   { printf '  [ok] %s\n' "$1"; }
warn() { printf '  [!!] %s\n' "$1"; }
info() { printf '       %s\n' "$1"; }
fail() { printf '  [XX] %s\n\n' "$1"; exit 1; }

# ── Pre-flight checks ─────────────────────────────────────────────────────────

printf '\n  Harkonnen Labs — Local Stack Bootstrap\n'
printf '  Base dir: %s\n\n' "$BASE_DIR"

# 1. Docker daemon
if ! command -v docker > /dev/null 2>&1; then
    fail "docker not found. Install Docker Engine: https://docs.docker.com/engine/install/"
fi
if ! docker info > /dev/null 2>&1; then
    fail "Docker daemon is not running. Start it with: sudo systemctl start docker"
fi
ok "Docker daemon is running"

# 2. Port availability
port_free() {
    port="$1"
    if command -v ss > /dev/null 2>&1; then
        ! ss -tlnH 2>/dev/null | awk '{print $4}' | grep -q ":${port}$"
    elif command -v netstat > /dev/null 2>&1; then
        ! netstat -tlnp 2>/dev/null | awk '{print $4}' | grep -q ":${port}$"
    else
        ! grep -q ":$(printf '%04X' "$port") " /proc/net/tcp /proc/net/tcp6 2>/dev/null
    fi
}

if port_free "$ANYTHINGLLM_PORT"; then
    ok "Port $ANYTHINGLLM_PORT is free"
else
    warn "Port $ANYTHINGLLM_PORT is already in use"
    info "Another process is listening on $ANYTHINGLLM_PORT. Bootstrap will write the"
    info "compose file anyway, but 'anythingllm-up' may fail until the port is free."
    info "Find it: ss -tlnp | grep :$ANYTHINGLLM_PORT"
fi

# 3. Disk space (need at least 3 GB free for the Docker image + storage)
REQUIRED_KB=3145728  # 3 GB in KB
if command -v df > /dev/null 2>&1; then
    FREE_KB=$(df -k "$BASE_DIR" 2>/dev/null | awk 'NR==2 {print $4}')
    if [ -n "$FREE_KB" ] && [ "$FREE_KB" -lt "$REQUIRED_KB" ]; then
        warn "Low disk space on $(df -k "$BASE_DIR" | awk 'NR==2 {print $6}')"
        info "Available: $(( FREE_KB / 1024 )) MB  Required: ~3 GB (Docker image + storage)"
        info "Free up space before proceeding or the image pull may fail mid-way."
    else
        ok "Disk space OK ($(( ${FREE_KB:-0} / 1024 )) MB free)"
    fi
fi

# 4. Volume mount path is writable
mkdir -p "$OPENCLAW_PREFIX" "$ANYTHINGLLM_STORAGE" "$BIN_DIR"

if [ ! -w "$ANYTHINGLLM_STORAGE" ]; then
    fail "Storage path is not writable: $ANYTHINGLLM_STORAGE
  Fix: sudo chown -R $(id -u):$(id -g) \"$BASE_DIR\""
fi
ok "Storage path writable: $ANYTHINGLLM_STORAGE"

# Verify Docker can actually mount the path (catches FUSE / overlay / remote-fs issues)
if ! docker run --rm \
        -v "${ANYTHINGLLM_STORAGE}:/check" \
        --entrypoint sh \
        alpine:3.19 -c "touch /check/.mount-test && rm /check/.mount-test" \
        > /dev/null 2>&1; then
    warn "Docker volume mount test failed for $ANYTHINGLLM_STORAGE"
    info "This can happen on remote filesystems, certain FUSE mounts, or SELinux-enforcing hosts."
    info "AnythingLLM storage may not persist across container restarts."
    info "Continuing — check 'docker logs harkonnen-anythingllm' if you see storage errors."
else
    ok "Docker volume mount verified"
fi

# ── Write config files ────────────────────────────────────────────────────────

printf 'Writing AnythingLLM config into %s\n' "$ANYTHINGLLM_DIR"
cat > "$ENV_FILE" <<ENV
STORAGE_LOCATION=$ANYTHINGLLM_STORAGE
CONTAINER_NAME=harkonnen-anythingllm
HOST_PORT=$ANYTHINGLLM_PORT
HOST_UID=$HOST_UID
HOST_GID=$HOST_GID
ENV

cat > "$COMPOSE_FILE" <<'COMPOSE'
services:
  anythingllm:
    image: mintplexlabs/anythingllm:latest
    container_name: ${CONTAINER_NAME}
    restart: unless-stopped
    ports:
      - "${HOST_PORT}:3001"
    cap_add:
      - SYS_ADMIN
    environment:
      STORAGE_DIR: /app/server/storage
      UID: "${HOST_UID}"
      GID: "${HOST_GID}"
    volumes:
      - "${STORAGE_LOCATION}:/app/server/storage"
COMPOSE

# ── Pull image ────────────────────────────────────────────────────────────────

printf 'Pulling AnythingLLM image %s\n' "$ANYTHINGLLM_IMAGE"
printf '(this is ~1-2 GB — may take a few minutes on first run)\n'
if ! docker pull "$ANYTHINGLLM_IMAGE"; then
    fail "docker pull failed. Check your network connection and Docker Hub access."
fi
ok "Image pulled: $ANYTHINGLLM_IMAGE"

# ── Write wrapper scripts ─────────────────────────────────────────────────────

cat > "$BIN_DIR/anythingllm-up" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$ANYTHINGLLM_DIR"
docker compose up -d
WRAP

cat > "$BIN_DIR/anythingllm-down" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$ANYTHINGLLM_DIR"
docker compose down
WRAP

cat > "$BIN_DIR/anythingllm-logs" <<WRAP
#!/usr/bin/env sh
set -eu
cd "$ANYTHINGLLM_DIR"
docker compose logs -f
WRAP

chmod +x "$BIN_DIR/anythingllm-up" "$BIN_DIR/anythingllm-down" "$BIN_DIR/anythingllm-logs"

# ── Install OpenClaw ──────────────────────────────────────────────────────────

printf 'Installing OpenClaw into %s\n' "$OPENCLAW_PREFIX"
if curl -fsSL "$OPENCLAW_INSTALLER_URL" | bash -s -- --prefix "$OPENCLAW_PREFIX" --no-onboard; then
    ln -sfn "$OPENCLAW_PREFIX/bin/openclaw" "$BIN_DIR/openclaw"
    ok "OpenClaw installed: $BIN_DIR/openclaw"
else
    warn "OpenClaw install failed (network issue or installer error)"
    info "Retry manually: curl -fsSL $OPENCLAW_INSTALLER_URL | bash -s -- --prefix \"$OPENCLAW_PREFIX\" --no-onboard"
fi

# ── Summary ───────────────────────────────────────────────────────────────────

printf '\nBootstrap complete.\n'
printf '  OpenClaw:        %s\n' "$BIN_DIR/openclaw"
printf '  AnythingLLM up:  %s\n' "$BIN_DIR/anythingllm-up"
printf '  AnythingLLM down:%s\n' "$BIN_DIR/anythingllm-down"
printf '  AnythingLLM logs:%s\n' "$BIN_DIR/anythingllm-logs"
printf '\nNext:\n'
printf '  1. %s        (start AnythingLLM)\n' "$BIN_DIR/anythingllm-up"
printf '  2. Open http://localhost:%s and create an Admin API key\n' "$ANYTHINGLLM_PORT"
printf '  3. export ANYTHINGLLM_API_KEY=<key>\n'
printf '  4. ./scripts/factory-up-linux.sh   (seeds documents + full bring-up)\n'
