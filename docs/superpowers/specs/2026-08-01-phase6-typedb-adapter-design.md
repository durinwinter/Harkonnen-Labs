# Phase 6 — Live TypeDB Adapter (Design)

## Purpose

Close the first and biggest gap in ROADMAP.md Phase 6 ("TypeDB Semantic Layer"):
Coobie's causal queries currently answer from a SQLite projection-ledger
fallback, never from a real typed graph. This pass wires a live TypeDB 3.x
adapter behind the existing `CausalGraphStore` trait so the fallback becomes
a true fallback again, not the only path.

Scope is deliberately narrow: **read/query path only**. Write-back (promoted
lessons/causal-links written to TypeDB after Phase 5 consolidation approval)
is a separate, later pass — it's a distinct roadmap bullet and shouldn't be
bundled into proving the query path works.

## Non-goals

- GAIA Level 3 live-harness execution (separate roadmap item)
- AgentBench adapters (separate roadmap item)
- Production write-back path (promoted lessons/causal-links → TypeDB)
- Any change to the SQLite projection-ledger fallback behavior — it must
  keep working unchanged when TypeDB is disabled or unavailable
- Open-ended natural-language-to-TypeQL translation — only the question
  shapes the SQLite fallback already recognizes get a TypeQL equivalent

## Existing context this design builds on

- `src/causal_graph.rs` already defines `CausalGraphStore` (trait with one
  method, `query`), `CausalGraphConfig`, `NoopCausalGraphStore`, and all the
  request/response types. No trait changes are needed.
- `factory/coobie_semantic/typedb/schema.tql` is a scaffolded TypeDB 3.x
  schema, written against docs but **never applied to a live instance**.
  Applying it for the first time is itself a compatibility check.
- `calvin/src/archive.rs` (a sibling crate, the Calvin Archive sidecar)
  already has a working, in-production integration with TypeDB 3.x via the
  official `typedb-driver` crate (v3.8.4-rc0, already resolved in
  `Cargo.lock`). Its connection/transaction/query patterns are the direct
  template for this work.
- `[typedb]` config block already exists in `harkonnen.toml` (`url`,
  `database`, `schema_path`, `reasoning_mode`), currently `enabled = false`.
- No TypeDB server is currently running on this machine (confirmed: no
  docker containers, nothing listening on port 1729).
- All three `AppContext` construction sites (`src/orchestrator.rs:1146`,
  `src/orchestrator.rs:31514`, `src/api.rs:8464`) currently hardcode
  `NoopCausalGraphStore` regardless of what `[typedb].enabled` says. This is
  itself a gap this pass closes.

## Architecture

- New `TypeDbCausalGraphStore` implementing `CausalGraphStore`, added either
  as a new `src/causal_graph/typedb_store.rs` submodule or directly in
  `src/causal_graph.rs` (decide at implementation time based on file size).
- Add `typedb-driver = "3.8.4-rc0"` as a direct dependency of the main
  Harkonnen crate (already vendored via calvin-server, no new supply-chain
  risk).
- Connection setup mirrors `calvin/src/archive.rs::connect()`:
  `TypeDBDriver::new(url, credentials, options)`.
- New factory function `causal_graph::build_store(config: &CausalGraphConfig) -> Arc<dyn CausalGraphStore>`:
  - `enabled = false` → `NoopCausalGraphStore`, status `Disabled` (unchanged
    from today).
  - `enabled = true` → attempt connection; on success, return
    `TypeDbCausalGraphStore`; on any failure (connect, schema deploy), log a
    warning with the cause and fall back to `NoopCausalGraphStore` with
    status `Unavailable`. Startup must never fail because TypeDB is down.
- All three hardcoded `NoopCausalGraphStore::new(...)` call sites are
  replaced with `causal_graph::build_store(...)`.
- TypeDB runs locally via Docker (official `typedb/typedb:3.x` image), port
  1729, matching existing config. A `docker run` command (or compose entry)
  gets documented as part of this work — not a new orchestration system.

## Data flow

1. **Startup:** `build_store()` connects; if the configured database doesn't
   exist yet, deploys `factory/coobie_semantic/typedb/schema.tql` via a
   schema transaction (`include_str!` + `tx.query()`, same as Calvin's own
   schema deploy).
2. **Test seed (throwaway, not production write-back):** a small
   hand-written TQL fixture — a couple of episodes, a failure-mode, and a
   causal-link connecting them — inserted once into a scratch test database,
   solely so the live query test has something real to find.
3. **Query translation:** `query()` recognizes the same question shapes the
   SQLite fallback already answers (chiefly "what caused the last N failures
   on this spec"), runs a fixed TypeQL template scoped by `spec_id`/`run_id`,
   walks `causal-link` → `failure-mode`/`episode` relations, and maps rows
   into `CausalGraphHit { label, summary, evidence_refs, confidence }`.
4. **Status reporting:** `GET /api/causal-graph/status` (already exists,
   `src/api.rs:2453`) starts reflecting real state: `Ready` once connected,
   `Unavailable` on connect/deploy failure, `Disabled` unchanged.

## Error handling

- Connection failure at startup never crashes the factory — caught, logged,
  falls back to `NoopCausalGraphStore` / `Unavailable`. Matches the
  "SQLite/memory remain authoritative" philosophy already present in the
  Noop store's note text.
- Schema deploy failure (schema.tql was scaffolded from docs, never
  validated against a live 3.x instance — this is the first real test of
  it) is handled the same way: log, fall back to Noop, don't panic.
- Per-query failures after a good connection return a
  `CausalGraphQueryResult` with `status: Unavailable` and the error in
  `note`, rather than propagating an `Err` — callers (Coobie's briefing
  builder, the causal-questions API) already expect a result object, not a
  fallible `Result`.

## Testing

- Unit tests, no live DB required:
  - `build_store()` returns Noop/`Disabled` when `enabled = false`.
  - TypeQL template construction produces the expected query string for a
    given question shape + `spec_id`/`run_id`.
- One live-DB integration test (`tests/typedb_causal_graph.rs`, gated —
  `#[ignore]` or an env check — since it requires a running TypeDB
  instance): connect, deploy schema into a scratch database, insert the
  seed fixture, call `store.query()`, assert real hits with correct
  labels/confidence come back.
- Manual verification: with TypeDB running via Docker, `cargo run --
  setup check` should report the causal graph backend as `TypeDb3`/`Ready`
  (today it always reports Noop regardless of config), and
  `GET /api/causal-graph/status` should agree.
- Existing SQLite-fallback regression tests must stay green, unchanged —
  this work is additive to that path, not a replacement.

## Open questions for the implementation plan

- Exact TQL template(s) needed to answer "what caused the last N failures
  on this spec" against the scaffolded schema — needs to be derived
  from `schema.tql`'s actual relation names during implementation.
- Whether `TypeDbCausalGraphStore` lives in its own submodule file or
  inline in `src/causal_graph.rs`, decided by resulting file size.
