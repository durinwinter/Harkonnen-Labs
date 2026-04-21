#!/usr/bin/env sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)

REPO="${DEVBENCH_REPO:-}"
TASK="${DEVBENCH_TASK:-Implementation}"
PROJECT_NAME="${DEVBENCH_PROJECT_NAME:-}"
OUTPUT_DIR="${DEVBENCH_OUTPUT:-}"
SPEC_PATH="${DEVBENCH_SPEC_PATH:-}"
DEVBENCH_SETUP_VALUE="${DEVBENCH_SETUP:-${HARKONNEN_SETUP:-}}"
LAUNCH_VALUE="${DEVBENCH_LAUNCH:-1}"

if [ -z "$REPO" ]; then
  printf 'DevBench runner not configured. Set DEVBENCH_REPO to the DevBench repository path you want to adapt.\n' >&2
  exit 10
fi

export DEVBENCH_LAUNCH="$LAUNCH_VALUE"

set -- python3 "$SCRIPT_DIR/devbench_harkonnen_adapter.py" --repo "$REPO" --task "$TASK"

if [ -n "$PROJECT_NAME" ]; then
  set -- "$@" --project-name "$PROJECT_NAME"
fi

if [ -n "$OUTPUT_DIR" ]; then
  set -- "$@" --output-dir "$OUTPUT_DIR"
fi

if [ -n "$SPEC_PATH" ]; then
  set -- "$@" --spec-path "$SPEC_PATH"
fi

if [ -n "$DEVBENCH_SETUP_VALUE" ]; then
  set -- "$@" --setup "$DEVBENCH_SETUP_VALUE"
fi

if [ -n "${DEVBENCH_HARKONNEN_RUN_COMMAND:-}" ]; then
  set -- "$@" --run-command "$DEVBENCH_HARKONNEN_RUN_COMMAND"
fi

exec "$@"
