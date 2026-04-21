#!/usr/bin/env sh
set -eu

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${HARKONNEN_PORT:-3000}"
SETUP_NAME="${HARKONNEN_SETUP:-lm-studio-local}"

export HARKONNEN_SETUP="$SETUP_NAME"
export LM_STUDIO_API_KEY="${LM_STUDIO_API_KEY:-lm-studio}"

cd "$REPO_ROOT"

printf '\n>> Harkonnen local launch\n'
printf '   setup: %s\n' "$HARKONNEN_SETUP"
printf '   API:   http://127.0.0.1:%s\n' "$PORT"
printf '   MCP:   cargo run -- mcp serve\n'
printf '   UI:    ./scripts/launch-pack-board.sh\n\n'

cargo run -- serve --port "$PORT"
