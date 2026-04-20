#!/usr/bin/env sh
set -eu

NAME="DevBench"
COMMAND_VAR="DEVBENCH_COMMAND"
ROOT_VAR="DEVBENCH_ROOT"
COMMAND="${DEVBENCH_COMMAND:-}"
ROOT="${DEVBENCH_ROOT:-}"

if [ -z "$COMMAND" ]; then
  printf '%s adapter not configured. Set %s to the exact command that runs the Harkonnen DevBench adapter. Optionally set %s to the benchmark checkout or adapter workspace.\n' "$NAME" "$COMMAND_VAR" "$ROOT_VAR" >&2
  exit 10
fi

if [ -n "$ROOT" ]; then
  cd "$ROOT"
fi

exec /bin/sh -lc "$COMMAND"
