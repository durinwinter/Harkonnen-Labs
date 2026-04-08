#!/usr/bin/env sh
set -eu

if [ -n "${LONGMEMEVAL_COMMAND:-}" ]; then
  exec /bin/sh -lc "$LONGMEMEVAL_COMMAND"
fi

if [ -z "${LONGMEMEVAL_DATASET:-}" ]; then
  printf 'LongMemEval adapter not configured. Set LONGMEMEVAL_DATASET to a local dataset file or LONGMEMEVAL_COMMAND to a custom adapter command.\n' >&2
  exit 10
fi

exec cargo run -- benchmark run --suite coobie_longmemeval "$@"
