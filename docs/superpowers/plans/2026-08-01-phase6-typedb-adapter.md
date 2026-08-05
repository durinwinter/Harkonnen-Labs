# Phase 6 Live TypeDB Adapter Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the always-Noop `CausalGraphStore` with a real TypeDB 3.x-backed implementation so Coobie's causal queries can answer from a live typed graph instead of only the SQLite projection-ledger fallback.

**Architecture:** A new `TypeDbCausalGraphStore` implements the existing `CausalGraphStore` trait (`src/causal_graph.rs`) using the official `typedb-driver` crate, following the exact connection/transaction/query patterns already proven in `calvin/src/archive.rs`. A new `build_store()` factory picks this store when `[typedb].enabled = true` and the connection succeeds, otherwise falls back to `NoopCausalGraphStore` — startup never fails because TypeDB is down. Scope is query-only; no write-back.

**Tech Stack:** Rust, `typedb-driver = "3.12"` (main crate — see version note below), `futures = "0.3"`, TypeDB 3.x via the `typedb` service already defined in `docker-compose.calvin.yml`, tokio async.

**Version note (discovered during Task 1, resolved 2026-08-01):** the plan originally assumed `typedb-driver = "3.8.4-rc0"` to match `calvin/Cargo.toml`. Two facts discovered during Task 1 forced a change. First, the locally running TypeDB container is 3.12.1, and driver 3.8.4-rc0 cannot talk to it — the driver speaks wire protocol 7.1 while the 3.12.1 server speaks 8.2. (Note: 3.8.4-rc0 is *not* yanked from crates.io; it installs fine, it simply cannot negotiate with this server.) Second, Cargo resolves `typedb-driver` to a single shared version across this workspace, so the main crate and `calvin-server` cannot sit on different versions. Decision (approved by the human partner): the whole workspace moves to `typedb-driver = "3.12"`, and `calvin/src/archive.rs`'s `connect()` was migrated to the 3.12 API as part of Task 1. The connection-setup API changed between these versions (`DriverOptions::new` now takes a `DriverTlsConfig` instead of `(bool, Option<_>)`; `TypeDBDriver::new` now takes an `Addresses` instead of a bare `&str`). The transaction/query/row-reading API (`transaction()`, `tx.query(...).await`, `tx.commit().await`, `answer.into_rows()`, `row.get(name)`, `concept.try_get_string()`, `concept.try_get_double()`) is unchanged between the two versions — confirmed directly against the 3.12.1 source. Every code block below already reflects the 3.12 connection API.

**Schema note (discovered during Task 2, resolved 2026-08-01):** `factory/coobie_semantic/typedb/schema.tql` had never been applied to a live server and turned out to be written in TypeDB **2.x** syntax. Two things changed when it was made to actually deploy against TypeDB 3.12.1:

1. **Kind-first declaration syntax.** TypeDB 3.x wants `attribute foo, value string;` / `relation foo, relates bar;` / `entity foo, owns bar, plays rel:role;` — not the 2.x `foo sub attribute` form. The file was also reordered (attributes → relations → entities) because forward references do not resolve within a single `define` query.
2. **`relation` is a reserved TypeQL keyword**, so the `causal-link` entity's `relation` attribute is now named **`relation-kind`**. Tasks 4 and 5's code blocks below already use the new name.

Everything else in the contract is unchanged and verified against the deployed schema: `causally-connects` still relates two `episode`s via roles `cause` / `effect` plus a `causal-link` via role `link`, and all other entity/relation/role/attribute names match the original scaffold.

## Global Constraints

- Read/query path only — do not implement write-back (promoted lessons/causal-links → TypeDB). That is a separate future pass.
- `NoopCausalGraphStore` behavior must not change: `enabled = false` still returns `Disabled` with the existing note text.
- Startup must never fail or panic because TypeDB is unreachable — always fall back to Noop/`Unavailable` on any connect or schema-deploy error, logged via `tracing::warn`.
- Do not touch the SQLite projection-ledger fallback logic in `src/api.rs` (`search_causal_graph_projection_ledger`, `build_causal_failure_history`, etc.) — it must keep working unchanged.
- Match existing code style exactly: `anyhow::Context` for error messages, `tracing` for logging, `#[async_trait]` for the trait impl, TQL built via `format!` string interpolation the same way `calvin/src/archive.rs` does (no new query-builder abstraction).
- TypeDB runs via the existing `typedb` service in `docker-compose.calvin.yml` (port 1729) — do not create new Docker infrastructure.

---

### Task 1: Bring up TypeDB and prove basic connectivity — COMPLETE

**Status:** Done (commits `c3ea803`, `2c8ac39`, `4029b35`, plus review fixes). The whole workspace — including `calvin-server` — now uses `typedb-driver = "3.12"`; see the Version note above. Kept below for reference; do not re-dispatch.

**Files:**
- Test: `tests/typedb_connectivity.rs` (new)

**Interfaces:**
- Consumes: `typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TypeDBDriver}` (from the new `typedb-driver` dependency, added in this task)
- Produces: nothing consumed by later tasks directly — this is a standalone connectivity proof. Later tasks re-implement the same connect logic inside `src/causal_graph.rs` (Task 3), not by calling into this test file.

**What actually shipped** (`typedb-driver = "3.12"`, main crate only — see the version note above the task list):

```rust
// tests/typedb_connectivity.rs
use typedb_driver::{Addresses, Credentials, DriverTlsConfig, DriverOptions, TypeDBDriver};

#[tokio::test]
#[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
async fn connects_to_local_typedb() {
    let credentials = Credentials::new("admin", "password");
    let tls_config = DriverTlsConfig::disabled();
    let options = DriverOptions::new(tls_config);
    let addresses = Addresses::try_from_address_str("localhost:1729").expect("parse address");

    let driver = TypeDBDriver::new(addresses, credentials, options)
        .await
        .expect("connect to local TypeDB");

    let dbs = driver.databases();
    let db_name = "harkonnen_connectivity_check";
    if !dbs.contains(db_name).await.expect("check database exists") {
        dbs.create(db_name).await.expect("create test database");
    }
    assert!(dbs.contains(db_name).await.expect("verify database created"));
}
```

Test result: `test connects_to_local_typedb ... ok`.

---

### Task 2: Fix the TypeDB schema and prove it deploys cleanly

**Files:**
- Modify: `factory/coobie_semantic/typedb/schema.tql`
- Test: `tests/typedb_schema_deploy.rs` (new)

**Interfaces:**
- Consumes: `typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver}`
- Produces: a schema that later tasks (3, 4, 5) can rely on being deployable and having the exact entity/relation/attribute names listed below.

**Why this task exists:** `factory/coobie_semantic/typedb/schema.tql` currently defines entities and relations (`episode`, `outcome`, `failure-mode`, `causal-link`, `produced-outcome`, `classifies-failure`, `causally-connects`, etc.) but never declares which entities `plays` which relation roles. TypeDB requires explicit `plays` declarations — without them, the schema will not deploy. This has never been tested against a live instance before, so this task is the first real proof it works.

- [ ] **Step 1: Add the missing `plays` declarations**

Edit `factory/coobie_semantic/typedb/schema.tql`. Add a `plays` line to each entity definition that participates in a relation. Insert each `plays` clause into the matching existing entity block (do not create new entity blocks):

```
agent sub entity,
    owns agent-name,
    owns agent-role,
    plays participated-in:participant;

goal sub entity,
    owns goal-id,
    owns summary,
    plays episode-goal:target-goal;

episode sub entity,
    owns episode-id,
    owns run-id,
    owns spec-id,
    owns phase,
    owns started-at,
    owns ended-at,
    plays participated-in:episode-context,
    plays episode-goal:episode-context,
    plays observed-in:episode-context,
    plays acted-in:episode-context,
    plays produced-outcome:episode-context,
    plays produced-artifact:episode-context,
    plays learned-from:episode-context,
    plays causally-connects:cause,
    plays causally-connects:effect;

observation sub entity,
    owns observation-id,
    owns summary,
    owns score,
    owns observed-at,
    plays observed-in:observation;

action sub entity,
    owns action-id,
    owns summary,
    owns phase,
    plays acted-in:action;

outcome sub entity,
    owns outcome-id,
    owns status,
    owns summary,
    owns confidence,
    plays produced-outcome:outcome,
    plays classifies-failure:outcome;

artifact sub entity,
    owns artifact-id,
    owns path,
    owns artifact-kind,
    plays produced-artifact:artifact;

lesson sub entity,
    owns lesson-id,
    owns summary,
    owns confidence,
    owns provenance,
    plays learned-from:lesson;

failure-mode sub entity,
    owns failure-mode-id,
    owns label,
    owns summary,
    plays classifies-failure:failure;

causal-link sub entity,
    owns causal-link-id,
    owns relation,
    owns pearl-level,
    owns epistemic-warrant,
    owns warrant-gap,
    owns confidence,
    owns structural-spec-json,
    plays causally-connects:link;
```

This makes the following assumption explicit (call it out in the commit message too): `causally-connects` relates two `episode`s (`cause`, `effect`) via a `causal-link` entity (`link`) holding the relation metadata — consistent with ROADMAP.md Phase 4's description of `causal_links` connecting `from_event_id`/`to_event_id` episodes.

- [ ] **Step 2: Write the schema-deploy test**

```rust
// tests/typedb_schema_deploy.rs
use futures::StreamExt;
use typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver};

const SCHEMA_TQL: &str = include_str!("../factory/coobie_semantic/typedb/schema.tql");

#[tokio::test]
#[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
async fn schema_deploys_without_error() {
    let credentials = Credentials::new("admin", "password");
    let options = DriverOptions::new(DriverTlsConfig::disabled());
    let addresses = Addresses::try_from_address_str("localhost:1729").expect("parse address");
    let driver = TypeDBDriver::new(addresses, credentials, options)
        .await
        .expect("connect to local TypeDB");

    let db_name = "harkonnen_schema_deploy_check";
    let dbs = driver.databases();
    if dbs.contains(db_name).await.expect("check database exists") {
        dbs.get(db_name).await.expect("get database").delete().await.expect("delete stale test database");
    }
    dbs.create(db_name).await.expect("create test database");

    let tx = driver
        .transaction(db_name, TransactionType::Schema)
        .await
        .expect("open schema transaction");
    tx.query(SCHEMA_TQL).await.expect("deploy schema");
    tx.commit().await.expect("commit schema");

    // Prove the causally-connects relation and its roles resolved correctly
    // by matching its type definition back out in a read transaction.
    let read_tx = driver
        .transaction(db_name, TransactionType::Read)
        .await
        .expect("open read transaction");
    let answer = read_tx
        .query("match $r sub causally-connects; select $r;")
        .await
        .expect("query causally-connects type");
    let mut rows = answer.into_rows();
    let mut found = false;
    while let Some(row_result) = rows.next().await {
        row_result.expect("row");
        found = true;
    }
    assert!(found, "causally-connects relation type should exist after schema deploy");
}
```

- [ ] **Step 3: Run the test to verify the schema deploys cleanly**

Run: `cargo test --test typedb_schema_deploy -- --ignored`
Expected: `test schema_deploys_without_error ... ok`. If it fails with a TypeQL error, fix `schema.tql` per the error message (most likely a typo in a role or attribute name) and re-run until green.

- [ ] **Step 4: Commit**

```bash
git add factory/coobie_semantic/typedb/schema.tql tests/typedb_schema_deploy.rs
git commit -m "Fix TypeDB schema plays declarations and prove it deploys"
```

---

### Task 3: Implement TypeDbCausalGraphStore connection + build_store() factory

**Files:**
- Modify: `src/causal_graph.rs`

**Interfaces:**
- Consumes: `CausalGraphConfig` (existing, `src/causal_graph.rs:24`), `TypeDbConfig` (existing, `src/setup.rs:187`)
- Produces:
  - `pub struct TypeDbCausalGraphStore` (fields private)
  - `impl TypeDbCausalGraphStore { pub async fn connect(config: CausalGraphConfig) -> anyhow::Result<Self> }`
  - `pub async fn build_store(config: CausalGraphConfig) -> std::sync::Arc<dyn CausalGraphStore>` — infallible; used by Task 6.

- [ ] **Step 1: Add imports and the `TypeDbCausalGraphStore` struct**

Add to the top of `src/causal_graph.rs` (after the existing `use` lines):

```rust
use anyhow::Context;
use futures::StreamExt;
use typedb_driver::{Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver};
```

Add after `NoopCausalGraphStore`'s `impl CausalGraphStore for NoopCausalGraphStore` block (end of current file, before the `#[cfg(test)]` module):

```rust
const SCHEMA_TQL: &str = include_str!("../factory/coobie_semantic/typedb/schema.tql");

#[derive(Debug)]
pub struct TypeDbCausalGraphStore {
    driver: TypeDBDriver,
    config: CausalGraphConfig,
}

impl TypeDbCausalGraphStore {
    pub async fn connect(config: CausalGraphConfig) -> Result<Self> {
        let credentials = Credentials::new("admin", "password");
        let options = DriverOptions::new(DriverTlsConfig::disabled());
        let addresses = Addresses::try_from_address_str(&config.url)
            .with_context(|| format!("parsing TypeDB address '{}'", config.url))?;
        let driver = TypeDBDriver::new(addresses, credentials, options)
            .await
            .with_context(|| format!("connecting to TypeDB at {}", config.url))?;

        let dbs = driver.databases();
        if !dbs
            .contains(&config.database)
            .await
            .with_context(|| format!("checking TypeDB database '{}'", config.database))?
        {
            dbs.create(&config.database)
                .await
                .with_context(|| format!("creating TypeDB database '{}'", config.database))?;
            tracing::info!("Created TypeDB database '{}'", config.database);

            let tx = driver
                .transaction(&config.database, TransactionType::Schema)
                .await
                .context("opening TypeDB schema transaction")?;
            tx.query(SCHEMA_TQL).await.context("deploying TypeDB schema")?;
            tx.commit().await.context("committing TypeDB schema")?;
            tracing::info!("Deployed Coobie semantic schema to database '{}'", config.database);
        }

        Ok(Self { driver, config })
    }
}
```

- [ ] **Step 2: Write the failing test for `build_store`'s disabled path**

Add inside the existing `#[cfg(test)] mod tests` block in `src/causal_graph.rs`:

```rust
#[tokio::test]
async fn build_store_returns_noop_when_disabled() {
    let config = CausalGraphConfig {
        backend: CausalGraphBackend::Disabled,
        enabled: false,
        url: "localhost:1729".to_string(),
        database: "harkonnen_semantic".to_string(),
        schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
        reasoning_mode: "function_backed".to_string(),
    };

    let store = build_store(config).await;
    let result = store
        .query(CausalGraphQuery {
            question: "what caused recent failures?".to_string(),
            run_id: None,
            spec_id: None,
            limit: 5,
        })
        .await
        .expect("query");

    assert_eq!(result.status, CausalGraphStatus::Disabled);
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test --lib causal_graph::tests::build_store_returns_noop_when_disabled`
Expected: FAIL to compile — `cannot find function build_store in module causal_graph` (it doesn't exist yet).

- [ ] **Step 4: Implement `build_store()` to make the test pass**

Add at the bottom of `src/causal_graph.rs`, before the `#[cfg(test)]` module:

```rust
pub async fn build_store(config: CausalGraphConfig) -> std::sync::Arc<dyn CausalGraphStore> {
    if !config.enabled {
        return std::sync::Arc::new(NoopCausalGraphStore::new(config));
    }
    match TypeDbCausalGraphStore::connect(config.clone()).await {
        Ok(store) => std::sync::Arc::new(store),
        Err(err) => {
            tracing::warn!(
                "TypeDB causal graph unavailable ({err:#}); falling back to SQLite/memory retrieval"
            );
            std::sync::Arc::new(NoopCausalGraphStore::new(config))
        }
    }
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test --lib causal_graph::tests::build_store_returns_noop_when_disabled`
Expected: `test causal_graph::tests::build_store_returns_noop_when_disabled ... ok`

- [ ] **Step 6: Run the full existing test suite to confirm no regressions**

Run: `cargo test --quiet`
Expected: all existing tests still pass (232 passing prior to this change), plus the new test.

- [ ] **Step 7: Commit**

```bash
git add src/causal_graph.rs
git commit -m "Implement TypeDbCausalGraphStore connection and build_store factory"
```

---

### Task 4: Implement the causal query TQL template

**Files:**
- Modify: `src/causal_graph.rs`

**Interfaces:**
- Consumes: `TypeDbCausalGraphStore` (Task 3), schema entity/relation/attribute names from `factory/coobie_semantic/typedb/schema.tql` (Task 2)
- Produces: `impl CausalGraphStore for TypeDbCausalGraphStore` — the real `query()` method later tasks (5, 6) rely on being present and matching the trait signature `async fn query(&self, query: CausalGraphQuery) -> Result<CausalGraphQueryResult>`.

- [ ] **Step 1: Implement `CausalGraphStore` for `TypeDbCausalGraphStore`**

Add after the `impl TypeDbCausalGraphStore` block from Task 3:

```rust
#[async_trait]
impl CausalGraphStore for TypeDbCausalGraphStore {
    fn config(&self) -> &CausalGraphConfig {
        &self.config
    }

    async fn query(&self, query: CausalGraphQuery) -> Result<CausalGraphQueryResult> {
        let Some(run_id) = query.run_id.clone() else {
            return Ok(CausalGraphQueryResult {
                status: CausalGraphStatus::Ready,
                backend: self.config.backend.clone(),
                database: self.config.database.clone(),
                query: query.question,
                hits: Vec::new(),
                note: Some("typed causal graph query requires a run_id scope".to_string()),
            });
        };

        let tx = self
            .driver
            .transaction(&self.config.database, TransactionType::Read)
            .await
            .context("opening TypeDB read transaction")?;

        let limit = query.limit.max(1);
        let tql = format!(
            r#"match
                $episode isa episode, has run-id "{run_id}";
                $outcome isa outcome, has status "failed";
                (episode-context: $episode, outcome: $outcome) isa produced-outcome;
                $failure isa failure-mode, has label $flabel, has summary $fsummary;
                (failure: $failure, outcome: $outcome) isa classifies-failure;
                $cause isa episode;
                (cause: $cause, effect: $episode, link: $link) isa causally-connects;
                $link isa causal-link, has relation-kind $rel, has confidence $conf;
               select $flabel, $fsummary, $rel, $conf;
               sort $conf desc;
               limit {limit};"#
        );

        let answer = tx.query(&tql).await.context("running causal graph query")?;

        let mut hits = Vec::new();
        let mut rows = answer.into_rows();
        while let Some(row_result) = rows.next().await {
            let row = row_result.context("reading causal graph row")?;
            let label = row
                .get("flabel")
                .ok()
                .flatten()
                .and_then(|concept| concept.try_get_string().map(str::to_string))
                .unwrap_or_default();
            let summary = row
                .get("fsummary")
                .ok()
                .flatten()
                .and_then(|concept| concept.try_get_string().map(str::to_string))
                .unwrap_or_default();
            let relation = row
                .get("rel")
                .ok()
                .flatten()
                .and_then(|concept| concept.try_get_string().map(str::to_string))
                .unwrap_or_default();
            let confidence = row
                .get("conf")
                .ok()
                .flatten()
                .and_then(|concept| concept.try_get_double())
                .unwrap_or(0.0);

            hits.push(CausalGraphHit {
                label,
                summary: format!("{summary} (causal link: {relation})"),
                evidence_refs: vec![format!("run:{run_id}")],
                confidence,
            });
        }

        let note = if hits.is_empty() {
            Some(format!("no typed causal graph hits found for run {run_id}"))
        } else {
            Some(format!("typed causal graph returned {} hit(s)", hits.len()))
        };

        Ok(CausalGraphQueryResult {
            status: CausalGraphStatus::Ready,
            backend: self.config.backend.clone(),
            database: self.config.database.clone(),
            query: query.question,
            hits,
            note,
        })
    }
}
```

**Note on `try_get_double`:** confirmed directly against the installed `typedb-driver` 3.12.1 source (`concept/mod.rs`) — `Concept::try_get_string(&self) -> Option<&str>` and `Concept::try_get_double(&self) -> Option<f64>` both exist with these exact names and signatures, unchanged from the row-reading API `calvin/src/archive.rs` already uses. No further verification needed for this step.

- [ ] **Step 2: Compile-check**

Run: `cargo build --lib`
Expected: compiles cleanly. Fix any `Concept` accessor method name mismatch found in Step 1's note here.

- [ ] **Step 3: Commit**

```bash
git add src/causal_graph.rs
git commit -m "Implement TypeDB-backed causal query for recent-failures-by-run"
```

---

### Task 5: Seed fixture and live end-to-end query test

**Files:**
- Create: `factory/coobie_semantic/typedb/test_seed.tql`
- Test: `tests/typedb_causal_graph.rs` (new)

**Interfaces:**
- Consumes: `TypeDbCausalGraphStore::connect` (Task 3), `CausalGraphStore::query` (Task 4)
- Produces: nothing consumed by later tasks — this is the end-to-end proof.

- [ ] **Step 1: Write the test-only seed fixture**

```
# factory/coobie_semantic/typedb/test_seed.tql
# Throwaway fixture data for the Phase 6 live-query integration test.
# NOT part of the production write-back path — hand-written for testing only.

insert
  $ep1 isa episode, has episode-id "seed-ep-1", has run-id "seed-run-1", has spec-id "seed-spec", has phase "implementation";
  $ep2 isa episode, has episode-id "seed-ep-2", has run-id "seed-run-1", has spec-id "seed-spec", has phase "validation";
  $outcome isa outcome, has outcome-id "seed-outcome-1", has status "failed", has summary "validation failed: assertion mismatch", has confidence 0.9;
  (episode-context: $ep2, outcome: $outcome) isa produced-outcome;
  $failure isa failure-mode, has failure-mode-id "seed-fm-1", has label "WrongAnswer", has summary "Test asserted the wrong value";
  (failure: $failure, outcome: $outcome) isa classifies-failure;
  $link isa causal-link, has causal-link-id "seed-link-1", has relation-kind "phase_sequence", has pearl-level "Associational", has epistemic-warrant "Associational", has warrant-gap false, has confidence 0.85, has structural-spec-json "{}";
  (cause: $ep1, effect: $ep2, link: $link) isa causally-connects;
```

- [ ] **Step 2: Write the end-to-end integration test**

```rust
// tests/typedb_causal_graph.rs
use harkonnen_labs::causal_graph::{
    CausalGraphBackend, CausalGraphConfig, CausalGraphQuery, CausalGraphStatus, CausalGraphStore,
    TypeDbCausalGraphStore,
};

const SEED_TQL: &str = include_str!("../factory/coobie_semantic/typedb/test_seed.tql");

#[tokio::test]
#[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
async fn typed_query_returns_seeded_causal_hit() {
    let config = CausalGraphConfig {
        backend: CausalGraphBackend::TypeDb3,
        enabled: true,
        url: "localhost:1729".to_string(),
        database: "harkonnen_e2e_query_check".to_string(),
        schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
        reasoning_mode: "function_backed".to_string(),
    };

    let store = TypeDbCausalGraphStore::connect(config)
        .await
        .expect("connect and deploy schema");

    // Seed via a raw write transaction against the same database the store just created.
    // (Uses typedb_driver directly since seeding is test-only, not part of the store's API.)
    let credentials = typedb_driver::Credentials::new("admin", "password");
    let options = typedb_driver::DriverOptions::new(typedb_driver::DriverTlsConfig::disabled());
    let addresses =
        typedb_driver::Addresses::try_from_address_str("localhost:1729").expect("parse address");
    let driver = typedb_driver::TypeDBDriver::new(addresses, credentials, options)
        .await
        .expect("connect for seeding");
    let tx = driver
        .transaction("harkonnen_e2e_query_check", typedb_driver::TransactionType::Write)
        .await
        .expect("open write transaction");
    tx.query(SEED_TQL).await.expect("insert seed data");
    tx.commit().await.expect("commit seed data");

    let result = store
        .query(CausalGraphQuery {
            question: "what caused recent failures on this spec?".to_string(),
            run_id: Some("seed-run-1".to_string()),
            spec_id: None,
            limit: 6,
        })
        .await
        .expect("query");

    assert_eq!(result.status, CausalGraphStatus::Ready);
    assert_eq!(result.hits.len(), 1);
    assert_eq!(result.hits[0].label, "WrongAnswer");
    assert!(result.hits[0].summary.contains("phase_sequence"));
}
```

- [ ] **Step 3: Expose `TypeDbCausalGraphStore` and the trait publicly if not already**

Confirm `src/lib.rs` has `pub mod causal_graph;` (it already does, per the module already being usable from `src/api.rs`/`src/orchestrator.rs`) — no change needed unless the compiler reports a visibility error, in which case add `pub use causal_graph::{CausalGraphStore, TypeDbCausalGraphStore};` re-exports where the compiler indicates.

- [ ] **Step 4: Run the test against the live container**

Run: `cargo test --test typedb_causal_graph -- --ignored`
Expected: `test typed_query_returns_seeded_causal_hit ... ok`

- [ ] **Step 5: Commit**

```bash
git add factory/coobie_semantic/typedb/test_seed.tql tests/typedb_causal_graph.rs
git commit -m "Add end-to-end live TypeDB causal query test with seed fixture"
```

---

### Task 6: Wire build_store() into AppContext construction

**Files:**
- Modify: `src/orchestrator.rs:1146` (inside `bootstrap_with_options`)
- Modify: `src/orchestrator.rs:31514` (test-fixture construction)
- Modify: `src/api.rs:8464` (inside a second `AppContext` constructor)

**Interfaces:**
- Consumes: `causal_graph::build_store(config: CausalGraphConfig) -> Arc<dyn CausalGraphStore>` (Task 3)
- Produces: nothing new — this task only changes how `AppContext.causal_graph` gets populated.

- [ ] **Step 1: Replace the hardcoded Noop construction at `src/orchestrator.rs:1146`**

Change:

```rust
        let causal_graph = Arc::new(crate::causal_graph::NoopCausalGraphStore::new(
            crate::causal_graph::CausalGraphConfig::from(&paths.setup.typedb),
        ));
```

to:

```rust
        let causal_graph = crate::causal_graph::build_store(
            crate::causal_graph::CausalGraphConfig::from(&paths.setup.typedb),
        )
        .await;
```

- [ ] **Step 2: Replace the same pattern at `src/orchestrator.rs:31514`**

Apply the identical change (this call site is inside a test-fixture-building function — confirm it's `async fn` before editing; it already is, since it awaits other setup calls nearby).

- [ ] **Step 3: Replace the same pattern at `src/api.rs:8464`**

Apply the identical change.

- [ ] **Step 4: Build to confirm no leftover references to the old pattern**

Run: `grep -n "NoopCausalGraphStore::new" src/orchestrator.rs src/api.rs`
Expected: no output (all three call sites converted). `NoopCausalGraphStore` itself remains defined and used only inside `build_store()` and the existing `#[cfg(test)]` module — that's expected and correct.

- [ ] **Step 5: Run the full test suite**

Run: `cargo test --quiet`
Expected: all tests pass (with `[typedb].enabled = false` in the default test config, `build_store()` returns `NoopCausalGraphStore` immediately without attempting any network connection, so no test becomes network-dependent).

- [ ] **Step 6: Commit**

```bash
git add src/orchestrator.rs src/api.rs
git commit -m "Wire build_store factory into AppContext construction call sites"
```

---

### Task 7: Enable TypeDB in the home-linux setup and verify end-to-end

**Files:**
- Modify: `setups/home-linux.toml`
- Modify: `harkonnen.toml`

**Interfaces:**
- Consumes: everything from Tasks 1-6
- Produces: nothing further — this is the final manual verification.

- [ ] **Step 1: Enable TypeDB in both setup files**

In `setups/home-linux.toml`, change:

```toml
[typedb]
enabled = false
```

to:

```toml
[typedb]
enabled = true
```

Apply the identical change in `harkonnen.toml`'s `[typedb]` block.

- [ ] **Step 2: Confirm TypeDB is running**

Run: `docker compose -f docker-compose.calvin.yml ps typedb`
Expected: state `running (healthy)`. If not, run `docker compose -f docker-compose.calvin.yml up -d typedb` first.

- [ ] **Step 3: Manually verify `setup check` reports the real backend**

Run: `cargo run -- setup check`
Expected: no longer silently forced to Noop — inspect the causal graph section of the output (added by this plan's work) and confirm it reports `TypeDb3` and does not report a connection error. (If `setup check`'s existing output doesn't yet print causal graph backend status, that's fine — the authoritative check is Step 4's HTTP call, not this one.)

- [ ] **Step 4: Manually verify the status API**

Run: `cargo run -- serve --port 3057 &` then `curl -s http://localhost:3057/api/causal-graph/status | jq .status`
Expected: `"ready"` (not `"unavailable"` or `"disabled"`). Stop the server afterward (`kill %1` or the equivalent job-control command for your shell).

- [ ] **Step 5: Run the full test suite one final time**

Run: `cargo test --quiet`
Expected: all tests pass (232+ from before this plan, plus the new tests added in Tasks 3, 5).

- [ ] **Step 6: Commit**

```bash
git add setups/home-linux.toml harkonnen.toml
git commit -m "Enable TypeDB backend for home-linux setup"
```

---

## Plan Self-Review Notes

- **Spec coverage:** Architecture (Tasks 3, 6), data flow / startup+schema-deploy+seed+query+status (Tasks 1, 2, 4, 5, 7), error handling / never-fail-startup fallback (Task 3's `build_store`), testing (unit tests in Task 3, live integration tests in Tasks 1/2/5, manual verification in Task 7) — all spec sections have a task.
- **Deferred by design, not a gap:** write-back path, GAIA/AgentBench work — explicitly out of scope per the spec's non-goals.
- **Known risk flagged inline:** `try_get_double`'s exact method name on `Concept` is unconfirmed against the installed `typedb-driver` version (only `try_get_string` was directly observed in `calvin/src/archive.rs`) — Task 4 Step 2 calls this out explicitly as a compile-check-and-fix step rather than assuming it silently works.
