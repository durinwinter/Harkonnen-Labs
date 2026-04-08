#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage:
  scripts/run-visible-benchmark-docker.sh <longmemeval|locomo> [limit]

This builds a benchmark runner image, mounts the repo, and executes the normal
visible benchmark launcher from inside Docker so host glibc/runtime mismatches
stop mattering.

Useful environment:
  HARKONNEN_SETUP                 Defaults to lm-studio-qwen-link-docker
  LM_STUDIO_API_KEY               Defaults to lm-studio
  HARKONNEN_BENCH_HEARTBEAT_SECS  Forwarded into the container
  HARKONNEN_BENCH_VERBOSE         Forwarded into the container
  HARKONNEN_BENCH_SHOW_RAW        Forwarded into the container
  HARKONNEN_HTTP_TIMEOUT_SECS     Forwarded into the container
  LONGMEMEVAL_DATASET             Forwarded into the container
  LOCOMO_DATASET                  Forwarded into the container
  LONGMEMEVAL_DIRECT_MAX_CHARS    Forwarded into the container
  HARKONNEN_BENCH_CARGO_ARGS      Defaults to --no-default-features
USAGE
}

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO_ROOT=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)
IMAGE_TAG=${HARKONNEN_BENCH_DOCKER_IMAGE:-harkonnen-benchmark-runner:bookworm}
SETUP_NAME=${HARKONNEN_SETUP:-lm-studio-qwen-link-docker}
BENCHMARK=${1:-}
LIMIT=${2:-}

if [[ -z "$BENCHMARK" ]]; then
  usage
  exit 1
fi

DOCKER_CMD=(docker)
if ! command -v docker >/dev/null 2>&1; then
  if command -v flatpak-spawn >/dev/null 2>&1; then
    DOCKER_CMD=(flatpak-spawn --host docker)
  else
    printf '%s\n' 'docker is not available, and flatpak-spawn host fallback is also unavailable.' >&2
    exit 1
  fi
fi

BUILD_CMD=("${DOCKER_CMD[@]}" build -f docker/benchmark-runner.Dockerfile -t "$IMAGE_TAG" "$REPO_ROOT")
RUN_CMD=(
  "${DOCKER_CMD[@]}" run --rm -it
  --add-host host.docker.internal:host-gateway
  -v "$REPO_ROOT:/workdir"
  -v harkonnen-cargo-registry:/cargo/registry
  -v harkonnen-cargo-git:/cargo/git
  -v harkonnen-target-docker:/workdir/target-docker
  -e "LM_STUDIO_API_KEY=${LM_STUDIO_API_KEY:-lm-studio}"
  -e "HARKONNEN_SETUP=$SETUP_NAME"
  -e "HARKONNEN_BENCH_HEARTBEAT_SECS=${HARKONNEN_BENCH_HEARTBEAT_SECS:-10}"
  -e "HARKONNEN_BENCH_VERBOSE=${HARKONNEN_BENCH_VERBOSE:-1}"
  -e "HARKONNEN_BENCH_SHOW_RAW=${HARKONNEN_BENCH_SHOW_RAW:-1}"
  -e "HARKONNEN_HTTP_TIMEOUT_SECS=${HARKONNEN_HTTP_TIMEOUT_SECS:-900}"
  -e "HARKONNEN_BENCH_CARGO_ARGS=${HARKONNEN_BENCH_CARGO_ARGS:---no-default-features}"
)

for forwarded in LONGMEMEVAL_DATASET LOCOMO_DATASET LONGMEMEVAL_DIRECT_MAX_CHARS; do
  if [[ -n "${!forwarded:-}" ]]; then
    RUN_CMD+=( -e "$forwarded=${!forwarded}" )
  fi
done

RUN_CMD+=(
  "$IMAGE_TAG"
  bash -c
  "scripts/run-visible-benchmark.sh '$BENCHMARK' '$LIMIT'"
)

printf 'Building image: %s\n' "$IMAGE_TAG"
"${BUILD_CMD[@]}"

printf 'Running benchmark in Docker with setup %s\n' "$SETUP_NAME"
printf 'Docker command: %s\n' "${RUN_CMD[*]}"
"${RUN_CMD[@]}"
