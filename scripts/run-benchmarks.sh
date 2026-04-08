#!/usr/bin/env sh
set -eu

if [ -z "${HARKONNEN_SETUP:-}" ]; then
  if [ -n "${HARKONNEN_BENCH_SETUP:-}" ]; then
    export HARKONNEN_SETUP="$HARKONNEN_BENCH_SETUP"
  elif [ -f "setups/lm-studio-local.toml" ]; then
    export HARKONNEN_SETUP="lm-studio-local"
  fi
fi

if [ "${HARKONNEN_SETUP:-}" = "lm-studio-local" ] && [ -z "${LM_STUDIO_API_KEY:-}" ]; then
  export LM_STUDIO_API_KEY="lm-studio"
fi

if [ "$#" -eq 0 ]; then
  exec cargo run -- benchmark run --strict
fi

exec cargo run -- benchmark run "$@"
