#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/run-visible-benchmark.sh <longmemeval|locomo> [limit]

Environment:
  HARKONNEN_SETUP                 Optional setup name or path
  HARKONNEN_BENCH_SETUP           Fallback setup if HARKONNEN_SETUP is unset
  HARKONNEN_HTTP_TIMEOUT_SECS     Default: 900
  HARKONNEN_BENCH_VERBOSE         Default: 1
  HARKONNEN_BENCH_SHOW_RAW        Default: 1
  HARKONNEN_BENCH_HEARTBEAT_SECS  Default: 30
  LONGMEMEVAL_DATASET             Required for longmemeval unless auto-discovered
  LOCOMO_DATASET                  Required for locomo unless auto-discovered
  LM_STUDIO_API_KEY               Defaults to lm-studio for lm-studio setups
USAGE
}

resolve_setup() {
  if [[ -n "${HARKONNEN_SETUP:-}" ]]; then
    printf '%s\n' "$HARKONNEN_SETUP"
    return
  fi
  if [[ -n "${HARKONNEN_BENCH_SETUP:-}" ]]; then
    printf '%s\n' "$HARKONNEN_BENCH_SETUP"
    return
  fi
  if [[ -f "setups/lm-studio-qwen-link.toml" ]]; then
    printf '%s\n' 'lm-studio-qwen-link'
    return
  fi
  if [[ -f "setups/lm-studio-local.toml" ]]; then
    printf '%s\n' 'lm-studio-local'
    return
  fi
  printf '%s\n' 'harkonnen.toml'
}

resolve_longmemeval_dataset() {
  if [[ -n "${LONGMEMEVAL_DATASET:-}" ]]; then
    printf '%s\n' "$LONGMEMEVAL_DATASET"
    return
  fi

  local candidates=(
    "development_datasets/longmemeval_s_cleaned.json"
    "development_datasets/longmemeval_m_cleaned.json"
    "development_datasets/longmemeval_oracle.json"
    "development_datasets/longmemeval/longmemeval_s_cleaned.json"
    "development_datasets/longmemeval/longmemeval_m_cleaned.json"
    "development_datasets/longmemeval-data-cleaned/data/longmemeval_s_cleaned.json"
    "development_datasets/longmemeval-data-cleaned/data/longmemeval_m_cleaned.json"
    "development_datasets/longmemeval-data-cleaned/data/longmemeval_oracle.json"
    "/tmp/LongMemEval/data/longmemeval_s_cleaned.json"
    "/tmp/LongMemEval/data/longmemeval_m_cleaned.json"
    "/tmp/LongMemEval/data/longmemeval_oracle.json"
    "factory/benchmarks/fixtures/longmemeval-smoke.json"
  )

  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return
    fi
  done

  printf '%s\n' ''
}

resolve_locomo_dataset() {
  if [[ -n "${LOCOMO_DATASET:-}" ]]; then
    printf '%s\n' "$LOCOMO_DATASET"
    return
  fi

  local candidates=(
    "development_datasets/locomo10.json"
    "development_datasets/locomo/data/locomo10.json"
    "/tmp/locomo/data/locomo10.json"
    "factory/benchmarks/fixtures/locomo-smoke.json"
  )

  local candidate
  for candidate in "${candidates[@]}"; do
    if [[ -f "$candidate" ]]; then
      printf '%s\n' "$candidate"
      return
    fi
  done

  printf '%s\n' ''
}

BENCHMARK="${1:-}"
LIMIT="${2:-}"
if [[ -z "$BENCHMARK" ]]; then
  usage
  exit 1
fi

export HARKONNEN_SETUP="$(resolve_setup)"
if [[ "$HARKONNEN_SETUP" == lm-studio* ]] && [[ -z "${LM_STUDIO_API_KEY:-}" ]]; then
  export LM_STUDIO_API_KEY="lm-studio"
fi

export HARKONNEN_HTTP_TIMEOUT_SECS="${HARKONNEN_HTTP_TIMEOUT_SECS:-900}"
export HARKONNEN_BENCH_VERBOSE="${HARKONNEN_BENCH_VERBOSE:-1}"
export HARKONNEN_BENCH_SHOW_RAW="${HARKONNEN_BENCH_SHOW_RAW:-1}"
HEARTBEAT_SECS="${HARKONNEN_BENCH_HEARTBEAT_SECS:-30}"
BENCH_CARGO_ARGS="${HARKONNEN_BENCH_CARGO_ARGS:---no-default-features}"

mkdir -p factory/artifacts/benchmarks/live-logs
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
LOG_PATH="factory/artifacts/benchmarks/live-logs/${BENCHMARK}-${STAMP}.log"

case "$BENCHMARK" in
  longmemeval)
    DATASET="$(resolve_longmemeval_dataset)"
    if [[ -z "$DATASET" ]]; then
      printf '%s\n' 'Could not find a LongMemEval dataset. Set LONGMEMEVAL_DATASET to a local JSON file.' >&2
      exit 1
    fi
    export LONGMEMEVAL_DATASET="$DATASET"
    if [[ -n "$LIMIT" ]]; then
      export LONGMEMEVAL_LIMIT="$LIMIT"
    fi
    CMD=(cargo run ${BENCH_CARGO_ARGS} -- benchmark run --suite coobie_longmemeval --suite longmemeval_raw_llm)
    ;;
  locomo)
    DATASET="$(resolve_locomo_dataset)"
    if [[ -z "$DATASET" ]]; then
      printf '%s\n' 'Could not find a LoCoMo dataset. Set LOCOMO_DATASET to a local JSON file.' >&2
      exit 1
    fi
    export LOCOMO_DATASET="$DATASET"
    if [[ -n "$LIMIT" ]]; then
      export LOCOMO_LIMIT="$LIMIT"
    fi
    CMD=(cargo run ${BENCH_CARGO_ARGS} -- benchmark run --suite coobie_locomo --suite locomo_raw_llm)
    ;;
  *)
    usage
    exit 1
    ;;
esac

printf 'Benchmark: %s\n' "$BENCHMARK" | tee "$LOG_PATH"
printf 'Setup: %s\n' "$HARKONNEN_SETUP" | tee -a "$LOG_PATH"
printf 'HTTP timeout: %s\n' "$HARKONNEN_HTTP_TIMEOUT_SECS" | tee -a "$LOG_PATH"
printf 'Verbose progress: %s\n' "$HARKONNEN_BENCH_VERBOSE" | tee -a "$LOG_PATH"
printf 'Show raw output: %s\n' "$HARKONNEN_BENCH_SHOW_RAW" | tee -a "$LOG_PATH"
printf 'Heartbeat seconds: %s\n' "$HEARTBEAT_SECS" | tee -a "$LOG_PATH"
printf 'Cargo args: %s\n' "$BENCH_CARGO_ARGS" | tee -a "$LOG_PATH"
if [[ "$BENCHMARK" == 'longmemeval' ]]; then
  printf 'Dataset: %s\n' "$LONGMEMEVAL_DATASET" | tee -a "$LOG_PATH"
  printf 'Limit: %s\n' "${LONGMEMEVAL_LIMIT:-<full>}" | tee -a "$LOG_PATH"
else
  printf 'Dataset: %s\n' "$LOCOMO_DATASET" | tee -a "$LOG_PATH"
  printf 'Limit: %s\n' "${LOCOMO_LIMIT:-<full>}" | tee -a "$LOG_PATH"
fi
printf 'Log: %s\n' "$LOG_PATH" | tee -a "$LOG_PATH"
printf 'Command: %s\n\n' "${CMD[*]}" | tee -a "$LOG_PATH"

(
  set -o pipefail
  "${CMD[@]}" 2>&1 | tee -a "$LOG_PATH"
) &
RUN_PID=$!

(
  while kill -0 "$RUN_PID" 2>/dev/null; do
    sleep "$HEARTBEAT_SECS"
    if kill -0 "$RUN_PID" 2>/dev/null; then
      printf '[heartbeat][%s] %s still running. Log: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$BENCHMARK" "$LOG_PATH" | tee -a "$LOG_PATH"
    fi
  done
) &
HEARTBEAT_PID=$!

STATUS=0
wait "$RUN_PID" || STATUS=$?
kill "$HEARTBEAT_PID" 2>/dev/null || true
wait "$HEARTBEAT_PID" 2>/dev/null || true
exit "$STATUS"
