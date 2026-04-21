---
name: rust
description: "Rust idioms for this repo: async/tokio, anyhow error handling, serde, clippy discipline, and cargo conventions."
user-invocable: false
allowed-tools:
  - Bash(cargo *)
---

# Rust Project Guide

This repo is written in Rust. Apply these conventions.

## Error Handling

- Use `anyhow::Result` for fallible functions that cross module boundaries.
- Use `anyhow::bail!("message")` to return early with a string error.
- Use `.with_context(|| format!("while doing {}", detail))` to annotate error sites.
- No `unwrap()` or `expect()` in non-test code — every panic site is a bug.
- `?` propagates errors; use it consistently rather than `match` on `Err`.

## Async

- Runtime is `tokio`; use `#[tokio::main]` for the entry point.
- Prefer `tokio::fs` over `std::fs` in async contexts.
- Avoid `.await` inside iterators — collect futures into `Vec` and `join_all`.
- `Arc<Mutex<T>>` for shared state; prefer message passing (`mpsc`) when possible.

## Serde

- Derive `Deserialize`, `Serialize` for all config and model types.
- Use `#[serde(default)]` on optional collection fields so missing keys deserialize to empty vec/map.
- Use `#[serde(rename_all = "snake_case")]` for JSON consistency.
- Tag enums with `#[serde(tag = "type")]` for readable JSON output.

## Cargo

- Always specify `edition = "2021"` in `Cargo.toml`.
- Feature flags for optional heavy dependencies (e.g., ML libraries) — keep the default build lean.
- Run `cargo clippy --no-default-features -- -D warnings` in CI.
- Pin workspace dependencies via `[workspace.dependencies]` to avoid version skew.

## Testing

- Unit tests go in the same file as the code under `#[cfg(test)]`.
- Integration tests in `tests/` — they compile as separate crates.
- Use `cargo test -q` for a compact summary; `--nocapture` when debugging output.
- Mock external I/O at the trait boundary, not deep inside implementations.
