#!/usr/bin/env sh
set -eu

NAME="tau2-bench"
COMMAND_VAR="TAU2_BENCH_COMMAND"
ROOT_VAR="TAU2_BENCH_ROOT"
COMMAND="${TAU2_BENCH_COMMAND:-}"
ROOT="${TAU2_BENCH_ROOT:-}"

if [ -z "$COMMAND" ]; then
  printf '%s adapter not configured. Set %s to the command that runs Harkonnen on this benchmark. Optionally set %s to the cloned benchmark repo root.\n' "$NAME" "$COMMAND_VAR" "$ROOT_VAR" >&2
  exit 10
fi

if [ -n "$ROOT" ]; then
  cd "$ROOT"
fi

exec /bin/sh -lc "$COMMAND"
