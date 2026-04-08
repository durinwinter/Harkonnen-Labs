#!/usr/bin/env sh
set -eu

NAME="LoCoMo"
COMMAND_VAR="LOCOMO_COMMAND"
ROOT_VAR="LOCOMO_ROOT"
COMMAND="${LOCOMO_COMMAND:-}"
ROOT="${LOCOMO_ROOT:-}"

if [ -z "$COMMAND" ]; then
  printf '%s adapter not configured. Set %s to the command that runs Harkonnen on this benchmark. Optionally set %s to the cloned benchmark repo root.\n' "$NAME" "$COMMAND_VAR" "$ROOT_VAR" >&2
  exit 10
fi

if [ -n "$ROOT" ]; then
  cd "$ROOT"
fi

exec /bin/sh -lc "$COMMAND"
