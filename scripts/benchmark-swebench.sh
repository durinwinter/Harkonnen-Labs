#!/usr/bin/env sh
set -eu

NAME="SWE-bench"
COMMAND_VAR="SWEBENCH_COMMAND"
ROOT_VAR="SWEBENCH_ROOT"
COMMAND="${SWEBENCH_COMMAND:-}"
ROOT="${SWEBENCH_ROOT:-}"

if [ -z "$COMMAND" ]; then
  printf '%s adapter not configured. Set %s to the command that runs Harkonnen on this benchmark. Optionally set %s to the cloned benchmark repo root.\n' "$NAME" "$COMMAND_VAR" "$ROOT_VAR" >&2
  exit 10
fi

if [ -n "$ROOT" ]; then
  cd "$ROOT"
fi

exec /bin/sh -lc "$COMMAND"
