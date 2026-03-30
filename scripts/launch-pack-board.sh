#!/usr/bin/env sh
set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BACKEND_PORT="${BACKEND_PORT:-3000}"
FRONTEND_PORT="${FRONTEND_PORT:-4173}"
VITE_HOST="${VITE_HOST:-127.0.0.1}"

cd "$REPO_ROOT"

cleanup() {
  if [ -n "${BACKEND_PID:-}" ] && kill -0 "$BACKEND_PID" 2>/dev/null; then
    kill "$BACKEND_PID" 2>/dev/null || true
    wait "$BACKEND_PID" 2>/dev/null || true
  fi
}
trap cleanup EXIT INT TERM

printf '
>> starting harkonnen api on http://127.0.0.1:%s
' "$BACKEND_PORT"
cargo run -- serve --port "$BACKEND_PORT" > /tmp/harkonnen-pack-board-api.log 2>&1 &
BACKEND_PID=$!

sleep 2
if ! kill -0 "$BACKEND_PID" 2>/dev/null; then
  printf '!! backend failed to start, see /tmp/harkonnen-pack-board-api.log
' >&2
  exit 1
fi

printf '>> api log: /tmp/harkonnen-pack-board-api.log
'
printf '>> starting pack board on http://%s:%s

' "$VITE_HOST" "$FRONTEND_PORT"
cd "$REPO_ROOT/ui"
VITE_API_BASE="http://127.0.0.1:${BACKEND_PORT}/api" npm run dev -- --host "$VITE_HOST" --port "$FRONTEND_PORT"
