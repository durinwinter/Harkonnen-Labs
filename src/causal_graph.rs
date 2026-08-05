use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use typedb_driver::{
    Addresses, Credentials, DriverOptions, DriverTlsConfig, TransactionType, TypeDBDriver,
};

use crate::setup::TypeDbConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CausalGraphBackend {
    TypeDb3,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CausalGraphStatus {
    Ready,
    Unavailable,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphConfig {
    pub backend: CausalGraphBackend,
    pub enabled: bool,
    pub url: String,
    pub database: String,
    pub schema_path: String,
    pub reasoning_mode: String,
}

impl From<&TypeDbConfig> for CausalGraphConfig {
    fn from(config: &TypeDbConfig) -> Self {
        Self {
            backend: if config.enabled {
                CausalGraphBackend::TypeDb3
            } else {
                CausalGraphBackend::Disabled
            },
            enabled: config.enabled,
            url: config.url.clone(),
            database: config.database.clone(),
            schema_path: config.schema_path.clone(),
            reasoning_mode: config.reasoning_mode.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphQuery {
    pub question: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub spec_id: Option<String>,
    #[serde(default)]
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphHit {
    pub label: String,
    pub summary: String,
    pub evidence_refs: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphQueryResult {
    pub status: CausalGraphStatus,
    pub backend: CausalGraphBackend,
    pub database: String,
    pub query: String,
    pub hits: Vec<CausalGraphHit>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphStatusResponse {
    pub status: CausalGraphStatus,
    pub backend: CausalGraphBackend,
    pub enabled: bool,
    pub url: String,
    pub database: String,
    pub schema_path: String,
    pub reasoning_mode: String,
    pub projection_count: u64,
    pub latest_projection: Option<CausalGraphProjectionSummary>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphProjectionRecord {
    pub run_id: String,
    pub backend: CausalGraphBackend,
    pub status: String,
    pub database: String,
    pub schema_path: String,
    pub graph_json: serde_json::Value,
    pub episode_count: u64,
    pub event_count: u64,
    pub link_count: u64,
    pub hypothesis_count: u64,
    pub projected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphProjectionSummary {
    pub run_id: String,
    pub backend: CausalGraphBackend,
    pub status: String,
    pub database: String,
    pub schema_path: String,
    pub episode_count: u64,
    pub event_count: u64,
    pub link_count: u64,
    pub hypothesis_count: u64,
    pub projected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalGraphProjectionInspection {
    #[serde(flatten)]
    pub record: CausalGraphProjectionRecord,
    pub highlights: Vec<CausalGraphHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalSpecFailureHistory {
    pub anchor_run_id: String,
    pub spec_id: String,
    pub projection_count: u64,
    pub failure_run_count: u64,
    pub repeated_causes: Vec<CausalRepeatedCause>,
    pub runs: Vec<CausalFailureRunSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalRepeatedCause {
    pub cause_id: String,
    pub count: u64,
    pub average_confidence: f64,
    pub run_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalFailureRunSummary {
    pub run_id: String,
    pub projected_at: DateTime<Utc>,
    pub failed_episode_count: u64,
    pub hypothesis_count: u64,
    pub top_causes: Vec<CausalGraphHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalFailureHistoryReplayExport {
    pub schema: String,
    pub generated_at: DateTime<Utc>,
    pub anchor_run_id: String,
    pub spec_id: String,
    pub projection_source: String,
    pub typedb_schema_path: String,
    pub history: CausalSpecFailureHistory,
    pub typedb_targets: Vec<String>,
    pub replay_queries: Vec<CausalReplayQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalReplayQuery {
    pub label: String,
    pub purpose: String,
    pub typeql: String,
}

impl From<&CausalGraphProjectionRecord> for CausalGraphProjectionSummary {
    fn from(record: &CausalGraphProjectionRecord) -> Self {
        Self {
            run_id: record.run_id.clone(),
            backend: record.backend.clone(),
            status: record.status.clone(),
            database: record.database.clone(),
            schema_path: record.schema_path.clone(),
            episode_count: record.episode_count,
            event_count: record.event_count,
            link_count: record.link_count,
            hypothesis_count: record.hypothesis_count,
            projected_at: record.projected_at,
        }
    }
}

#[async_trait]
pub trait CausalGraphStore: Send + Sync + std::fmt::Debug {
    fn config(&self) -> &CausalGraphConfig;
    async fn query(&self, query: CausalGraphQuery) -> Result<CausalGraphQueryResult>;
    /// Reports whether the backing store is reachable *right now*. Unlike
    /// `query()` with `run_id: None` (which for `TypeDbCausalGraphStore`
    /// short-circuits before ever touching the network), this must perform a
    /// real, bounded round-trip against the live store so callers such as a
    /// status endpoint can distinguish "was reachable at startup" from "is
    /// reachable this instant".
    async fn ping(&self) -> CausalGraphStatus;
}

#[derive(Debug, Clone)]
pub struct NoopCausalGraphStore {
    config: CausalGraphConfig,
}

impl NoopCausalGraphStore {
    pub fn new(config: CausalGraphConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl CausalGraphStore for NoopCausalGraphStore {
    fn config(&self) -> &CausalGraphConfig {
        &self.config
    }

    async fn query(&self, query: CausalGraphQuery) -> Result<CausalGraphQueryResult> {
        let status = if self.config.enabled {
            CausalGraphStatus::Unavailable
        } else {
            CausalGraphStatus::Disabled
        };
        let note = if self.config.enabled {
            // Reached only via `build_store()`'s connect-failure fallback: the
            // live TypeDB adapter *is* wired in this build, so the note must
            // describe the actual condition (configured but unreachable at
            // startup), not a missing implementation. Operator-visible through
            // `sources[].note` on causal queries.
            Some(
                "TypeDB 3.x is configured but was unreachable at startup; SQLite projection ledger and memory retrieval remain authoritative.".to_string(),
            )
        } else {
            Some("TypeDB semantic graph is disabled; SQLite and memory retrieval remain authoritative.".to_string())
        };

        Ok(CausalGraphQueryResult {
            status,
            backend: self.config.backend.clone(),
            database: self.config.database.clone(),
            query: query.question,
            hits: Vec::new(),
            note,
        })
    }

    async fn ping(&self) -> CausalGraphStatus {
        if self.config.enabled {
            CausalGraphStatus::Unavailable
        } else {
            CausalGraphStatus::Disabled
        }
    }
}

const SCHEMA_TQL: &str = include_str!("../factory/coobie_semantic/typedb/schema.tql");

/// Escapes a string for safe interpolation inside a TQL string literal.
/// `run_id` (and other query inputs) can originate from user-facing API
/// input (see `answer_general_coobie_query` in `src/api.rs`), so it must not
/// be spliced into `format!`-built TQL unescaped — an unescaped `"` would
/// close the string literal early and let the rest of the value be
/// interpreted as TQL. Mirrors the private `escape_tql` helper already used
/// for the same reason in `calvin/src/archive.rs`.
fn escape_tql(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Builds the "the typed graph could not answer this one" result that every
/// per-query failure path in `TypeDbCausalGraphStore::query()` collapses to.
///
/// Factored out so all four failure arms (timeout, task panic, task
/// cancellation, driver/decode error) are provably identical apart from the
/// `note` wording — the design spec's contract is that a per-query failure
/// after a good connection is a *result*, never an `Err`, and having one
/// constructor makes that hard to break by accident. `hits` is always empty:
/// a failed query has no verified typed evidence, and callers fall back to the
/// SQLite projection ledger they already gathered.
fn unavailable_query_result(
    config: &CausalGraphConfig,
    question: String,
    note: String,
) -> CausalGraphQueryResult {
    CausalGraphQueryResult {
        status: CausalGraphStatus::Unavailable,
        backend: config.backend.clone(),
        database: config.database.clone(),
        query: question,
        hits: Vec::new(),
        note: Some(note),
    }
}

#[derive(Debug)]
pub struct TypeDbCausalGraphStore {
    // `Arc`-wrapped (rather than owned `TypeDBDriver`) so `ping()` can clone
    // a handle into a detached `spawn_blocking` task — see the comment on
    // `ping()` for why that indirection is load-bearing, not decorative.
    driver: std::sync::Arc<TypeDBDriver>,
    config: CausalGraphConfig,
    /// Single-flight guard *and* short-TTL result cache for `ping()`, in one
    /// primitive. Held across the probe's `.await`, so:
    ///   * only one probe is ever in flight at a time (concurrent callers
    ///     queue on the mutex instead of each spawning their own probe), and
    ///   * a caller that queued behind an in-flight probe finds that probe's
    ///     freshly-stored result on acquiring the lock and returns it without
    ///     probing again (that's the "share the in-flight result" half).
    ///
    /// A `tokio::sync::Mutex` rather than `std::sync::Mutex` specifically
    /// because it must be held across an `.await`; it is also FIFO-fair, so
    /// no caller can be starved by a hot polling loop.
    ping_cache: tokio::sync::Mutex<Option<(std::time::Instant, CausalGraphStatus)>>,
}

/// Bounds the *unary* RPCs the typedb-driver issues while connecting
/// (connection open, `databases_contains`, `databases_create`, transaction
/// open). Per the driver's own doc comment on `DriverOptions::request_timeout`,
/// this does NOT bound operations inside an open transaction (queries,
/// commits) — that's why `build_store()` additionally wraps the whole
/// `connect()` call in an outer `tokio::time::timeout` using this same
/// duration as its budget.
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Bounds `TypeDbCausalGraphStore::ping()` — a liveness check meant to be
/// polled by a status endpoint, not a one-time startup connect. 10s
/// (`CONNECT_TIMEOUT`) is far too long for that use case (an operator
/// dashboard polling `/api/causal-graph/status` would hang or appear
/// unresponsive waiting on a dead backend), so `ping()` gets its own, much
/// shorter budget.
const PING_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(750);

/// Bounds `TypeDbCausalGraphStore::query()`, the production causal-question
/// hot path (reached from `POST /api/coobie/query`, `/api/chat`, and
/// `/api/agents/coobie/chat`). Sits deliberately between the other two
/// budgets, because all three answer different questions:
///
///   * `CONNECT_TIMEOUT` (10s) is a one-time startup cost paid before the
///     service is serving anything, so it can afford to be generous.
///   * `PING_TIMEOUT` (750ms) is a liveness *signal* polled by a dashboard;
///     it does no real work, so anything slower than "instant" is already the
///     answer ("unavailable") and waiting longer buys nothing.
///   * `QUERY_TIMEOUT` (3s) bounds an actual multi-relation TQL join with a
///     sort, on behalf of a waiting HTTP request. It has to leave room for a
///     genuinely-working-but-loaded TypeDB to answer (750ms would flap into
///     spurious `Unavailable` under load and silently drop real causal hits),
///     while still failing over to the SQLite projection ledger fast enough
///     that a chat/query request degrades rather than appearing to hang.
///
/// Like `CONNECT_TIMEOUT`, this is enforced by an *outer*
/// `tokio::time::timeout`, because `DriverOptions::request_timeout` does not
/// bound operations inside an already-open transaction — which is exactly
/// what this call is.
const QUERY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// How long a `ping()` result stays reusable before the next call probes the
/// live store again. Deliberately short: it exists only to collapse a burst of
/// dashboard polls into a single probe (see `ping_cache`), not to memoize
/// liveness. At 1s, a recovery of a previously-down TypeDB is reflected within
/// one second of the first poll after recovery — indistinguishable from live
/// for an operator dashboard — while sustained polling of a *down* TypeDB
/// produces at most one blocking probe per second instead of one per request.
const PING_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(1);

/// Cheap read query used to detect whether a database that already exists
/// actually has the Coobie semantic schema deployed to it. If `connect()`
/// created the database but a subsequent failure (e.g. a timeout) killed the
/// process before the schema transaction committed, the database would be
/// left permanently schemaless — every future `connect()` would see
/// `dbs.contains() == true` and skip deployment forever. This query detects
/// that condition so deployment can be retried.
const SCHEMA_PRESENCE_CHECK_TQL: &str = "match $e sub episode; select $e; limit 1;";

impl TypeDbCausalGraphStore {
    pub async fn connect(config: CausalGraphConfig) -> Result<Self> {
        let credentials = Credentials::new("admin", "password");
        let options =
            DriverOptions::new(DriverTlsConfig::disabled()).request_timeout(CONNECT_TIMEOUT);
        let addresses = Addresses::try_from_address_str(&config.url)
            .with_context(|| format!("parsing TypeDB address '{}'", config.url))?;
        let driver = TypeDBDriver::new(addresses, credentials, options)
            .await
            .with_context(|| format!("connecting to TypeDB at {}", config.url))?;

        let dbs = driver.databases();
        let already_existed = dbs
            .contains(&config.database)
            .await
            .with_context(|| format!("checking TypeDB database '{}'", config.database))?;

        if !already_existed {
            dbs.create(&config.database)
                .await
                .with_context(|| format!("creating TypeDB database '{}'", config.database))?;
            tracing::info!("Created TypeDB database '{}'", config.database);
        }

        // Deploy the schema if the database is new, OR if it already existed
        // but is missing the schema (self-repair for a prior partial failure,
        // e.g. create() succeeded but the schema transaction was interrupted
        // by a timeout before it could commit).
        let needs_schema =
            !already_existed || !Self::schema_is_present(&driver, &config.database).await?;

        if needs_schema {
            if already_existed {
                tracing::warn!(
                    "TypeDB database '{}' exists but is missing the Coobie semantic schema; repairing",
                    config.database
                );
            }

            let tx = driver
                .transaction(&config.database, TransactionType::Schema)
                .await
                .context("opening TypeDB schema transaction")?;
            tx.query(SCHEMA_TQL)
                .await
                .context("deploying TypeDB schema")?;
            tx.commit().await.context("committing TypeDB schema")?;
            tracing::info!(
                "Deployed Coobie semantic schema to database '{}'",
                config.database
            );
        }

        Ok(Self {
            driver: std::sync::Arc::new(driver),
            config,
            ping_cache: tokio::sync::Mutex::new(None),
        })
    }

    /// Runs a cheap read query for a known schema type and returns whether it
    /// resolved. Used both for the initial "was schema deployment interrupted"
    /// check and would be reused by any future health-check tooling.
    ///
    /// Empirically verified against live TypeDB 3.12.1: querying a `sub`
    /// relationship against a type that doesn't exist (i.e. schema not
    /// deployed) does NOT return an empty answer — it errors at query
    /// analysis time (`[INF2] Type label 'episode' not found`). So a query
    /// error here is treated as "schema absent" (returns `Ok(false)`) rather
    /// than propagated, which is what drives the self-repair path in
    /// `connect()`. If the error instead reflects a genuine connectivity
    /// problem, the subsequent schema (re)deploy attempt will fail loudly and
    /// `connect()` will return that error, which is the correct fallback
    /// behavior either way.
    async fn schema_is_present(driver: &TypeDBDriver, database: &str) -> Result<bool> {
        let tx = driver
            .transaction(database, TransactionType::Read)
            .await
            .context("opening TypeDB read transaction for schema presence check")?;
        let answer = match tx.query(SCHEMA_PRESENCE_CHECK_TQL).await {
            Ok(answer) => answer,
            Err(err) => {
                tracing::debug!(
                    "TypeDB schema presence check query failed on database '{database}' \
                     (treating as schema absent): {err:#}"
                );
                return Ok(false);
            }
        };
        let mut rows = answer.into_rows();
        let mut found = false;
        while let Some(row_result) = rows.next().await {
            row_result.context("reading schema presence check row")?;
            found = true;
        }
        Ok(found)
    }
}

#[async_trait]
impl CausalGraphStore for TypeDbCausalGraphStore {
    fn config(&self) -> &CausalGraphConfig {
        &self.config
    }

    /// Answers "what caused the failures on this run" by joining, per failed
    /// episode outcome on `run_id`: its classified failure mode, and any
    /// causal link where that episode is the effect end of a
    /// `causally-connects` relation.
    ///
    /// ## Failure contract: `Unavailable`, never `Err`
    ///
    /// Per the phase-6 design spec, "per-query failures after a good
    /// connection return a `CausalGraphQueryResult` with `status: Unavailable`
    /// and the error in `note`, rather than propagating an `Err` — callers
    /// already expect a result object, not a fallible `Result`". This matters
    /// concretely: the callers of this method (`answer_general_coobie_query`
    /// → `execute_coobie_query`) have *already* gathered SQLite
    /// projection-ledger hits by the time they consult the typed graph, and an
    /// `Err` bubbling out of here turns the whole request into an HTTP 500,
    /// throwing that good work away. `format_causal_graph_note` in
    /// `src/api.rs` has an `Unavailable` arm that surfaces `result.note`
    /// verbatim; that arm exists precisely for this path. Every failure below
    /// — timeout, join/panic, transaction-open error, query error, row-decode
    /// error — therefore collapses to `Ok(Unavailable)` with a diagnostic
    /// note. The signature stays `Result` only because the trait is shared
    /// with implementors that may legitimately fail.
    ///
    /// ## Why the driver work runs on `spawn_blocking`
    ///
    /// Identical reasoning to `probe_liveness()` (see its doc comment for the
    /// full argument, which applies verbatim here): the typedb-driver's
    /// reconnect path calls `std::thread::sleep` for ~2s when a
    /// previously-live connection goes away, which `tokio::time::timeout`
    /// cannot preempt, and which parks whichever thread is polling it. Awaited
    /// directly — as this method used to do — that thread is a *core worker*
    /// from the fixed-size (`num_cpus`) pool, borrowed from an axum request
    /// handler. Concurrent causal questions against a dead TypeDB would park
    /// one core worker each until the pool is exhausted and partial
    /// degradation becomes a full-service hang. `spawn_blocking` moves the
    /// work to the dynamically-sized blocking pool, where a parked probe is
    /// harmless, and leaves the async side free to honor the outer timeout.
    ///
    /// `Handle::current()` is captured here, in async context, and
    /// `Handle::block_on` appears *only* inside the `spawn_blocking` closure —
    /// it panics if called on a core worker, but blocking-pool threads are not
    /// an async execution context, so the placement is the safe one.
    ///
    /// Unlike `ping()`, this gets no cache and no single-flight: distinct
    /// queries have distinct answers, so there is nothing to share.
    async fn query(&self, query: CausalGraphQuery) -> Result<CausalGraphQueryResult> {
        let Some(run_id) = query.run_id.clone() else {
            let mut note = "typed causal graph query requires a run_id scope".to_string();
            if query.spec_id.is_some() {
                note.push_str("; spec_id filter is not applied by this query");
            }
            return Ok(CausalGraphQueryResult {
                status: CausalGraphStatus::Ready,
                backend: self.config.backend.clone(),
                database: self.config.database.clone(),
                query: query.question,
                hits: Vec::new(),
                note: Some(note),
            });
        };

        let limit = query.limit.max(1);
        let run_id_escaped = escape_tql(&run_id);
        let tql = format!(
            r#"match
                $episode isa episode, has run-id "{run_id_escaped}";
                $outcome isa outcome, has status "failed";
                (episode-context: $episode, outcome: $outcome) isa produced-outcome;
                $failure isa failure-mode, has label $flabel, has summary $fsummary;
                (failure: $failure, outcome: $outcome) isa classifies-failure;
                (cause: $cause, effect: $episode, link: $link) isa causally-connects;
                $link isa causal-link, has relation-kind $rel, has confidence $conf;
               select $flabel, $fsummary, $rel, $conf;
               sort $conf desc;
               limit {limit};"#
        );

        let driver = std::sync::Arc::clone(&self.driver);
        let database = self.config.database.clone();
        let row_run_id = run_id.clone();
        // Captured here, in async context, because `Handle::current()` is only
        // valid inside the runtime; the closure below runs on a blocking-pool
        // thread and uses this handle to drive the async driver calls.
        let handle = tokio::runtime::Handle::current();

        let work = tokio::task::spawn_blocking(move || {
            // SAFETY-OF-PLACEMENT: this `block_on` executes only on a
            // blocking-pool thread (inside `spawn_blocking`), never on a core
            // worker, so it cannot panic with "Cannot block the current
            // thread from within a runtime" and cannot deadlock the executor.
            handle.block_on(async move {
                let tx = driver
                    .transaction(&database, TransactionType::Read)
                    .await
                    .context("opening TypeDB read transaction")?;

                let answer = tx.query(&tql).await.context("running causal graph query")?;

                let mut hits = Vec::new();
                let mut rows = answer.into_rows();
                while let Some(row_result) = rows.next().await {
                    let row = row_result.context("reading causal graph row")?;
                    // Every one of these columns is bound by a match constraint
                    // in the TQL above (`has label $flabel, has summary
                    // $fsummary, has relation-kind $rel, has confidence
                    // $conf`), so TypeDB guarantees any row it returns has all
                    // four present with the expected value type. A decode
                    // failure here therefore always indicates a real bug
                    // (schema/query drift or a driver behavior change), never
                    // legitimately-absent data — surface it (as an
                    // `Unavailable` result carrying this context, per the
                    // failure contract on this method) instead of silently
                    // substituting a default, which would fabricate a
                    // plausible-looking but fake hit.
                    let label = row
                        .get("flabel")
                        .context("reading flabel column")?
                        .context("flabel not bound in result row")?
                        .try_get_string()
                        .context("flabel was not a string")?
                        .to_string();
                    let summary = row
                        .get("fsummary")
                        .context("reading fsummary column")?
                        .context("fsummary not bound in result row")?
                        .try_get_string()
                        .context("fsummary was not a string")?
                        .to_string();
                    let relation = row
                        .get("rel")
                        .context("reading rel column")?
                        .context("rel not bound in result row")?
                        .try_get_string()
                        .context("rel was not a string")?
                        .to_string();
                    let confidence = row
                        .get("conf")
                        .context("reading conf column")?
                        .context("conf not bound in result row")?
                        .try_get_double()
                        .context("conf was not a double")?;

                    hits.push(CausalGraphHit {
                        label,
                        summary: format!("{summary} (causal link: {relation})"),
                        evidence_refs: vec![format!("run:{row_run_id}")],
                        confidence,
                    });
                }

                Ok::<Vec<CausalGraphHit>, anyhow::Error>(hits)
            })
        });

        // Racing the `JoinHandle` itself (rather than a oneshot) means a
        // timeout simply drops the handle, detaching the blocking task to
        // finish and be discarded on its own — and it lets a genuine panic be
        // told apart from an ordinary lost race, below. Every non-success arm
        // returns `Ok(Unavailable)`; see the failure contract documented above.
        let hits = match tokio::time::timeout(QUERY_TIMEOUT, work).await {
            Ok(Ok(Ok(hits))) => hits,
            Ok(Ok(Err(err))) => {
                tracing::warn!("TypeDB causal graph query failed for run {run_id}: {err:#}");
                return Ok(unavailable_query_result(
                    &self.config,
                    query.question,
                    format!("typed causal graph query failed: {err:#}"),
                ));
            }
            Ok(Err(join_error)) if join_error.is_panic() => {
                // A real bug in the query task, not a race loss — worth a
                // louder level and a distinct message than the timeout path.
                tracing::warn!(
                    "TypeDB causal graph query task panicked for run {run_id}: {join_error}"
                );
                return Ok(unavailable_query_result(
                    &self.config,
                    query.question,
                    format!("typed causal graph query failed: query task panicked: {join_error}"),
                ));
            }
            Ok(Err(join_error)) => {
                tracing::warn!(
                    "TypeDB causal graph query task was cancelled for run {run_id}: {join_error}"
                );
                return Ok(unavailable_query_result(
                    &self.config,
                    query.question,
                    format!(
                        "typed causal graph query failed: query task was cancelled: {join_error}"
                    ),
                ));
            }
            Err(_elapsed) => {
                tracing::warn!(
                    "TypeDB causal graph query for run {run_id} exceeded {}ms budget; abandoning it on the blocking pool",
                    QUERY_TIMEOUT.as_millis()
                );
                return Ok(unavailable_query_result(
                    &self.config,
                    query.question,
                    format!(
                        "typed causal graph query failed: exceeded {}ms budget (TypeDB unreachable or unresponsive)",
                        QUERY_TIMEOUT.as_millis()
                    ),
                ));
            }
        };

        let mut note = if hits.is_empty() {
            format!("no typed causal graph hits found for run {run_id}")
        } else {
            format!("typed causal graph returned {} hit(s)", hits.len())
        };
        if query.spec_id.is_some() {
            note.push_str("; spec_id filter is not applied by this query");
        }
        let note = Some(note);

        Ok(CausalGraphQueryResult {
            status: CausalGraphStatus::Ready,
            backend: self.config.backend.clone(),
            database: self.config.database.clone(),
            query: query.question,
            hits,
            note,
        })
    }

    /// Real, bounded liveness check, de-duplicated across concurrent callers.
    ///
    /// The actual network probe lives in `probe_liveness()`; this method owns
    /// the single-flight/TTL policy that keeps a polling dashboard from
    /// launching one probe per request. Holding `ping_cache` across the probe
    /// gives both halves at once: the first caller through the door probes
    /// while everyone else queues on the mutex, and each queued caller then
    /// finds the just-stored result fresh and returns it without probing.
    /// Sustained polling of a down TypeDB therefore costs at most one probe
    /// per `PING_CACHE_TTL` (1s), no matter the request rate — which is what
    /// keeps abandoned probes from accumulating in the first place.
    ///
    /// The staleness this admits is bounded by `PING_CACHE_TTL +
    /// PING_TIMEOUT` ≈ 1.75s, not by the TTL alone: a cached entry is served
    /// until the TTL expires, and the caller that then finds it expired must
    /// still wait out the probe it triggers (up to `PING_TIMEOUT`) before a
    /// fresher answer exists. So a TypeDB that just died can read `ready`, and
    /// one that just recovered can read `unavailable`, for up to ~1.75s in
    /// either direction — still well inside the polling interval of any
    /// human-facing status view.
    ///
    /// Note the *scope* of that liveness claim. `build_store()` chooses
    /// between `NoopCausalGraphStore` and `TypeDbCausalGraphStore` exactly
    /// once, at bootstrap, and that choice is permanent for the life of the
    /// process. Freshness here therefore only tracks a TypeDB that was
    /// reachable at startup and later changed state. If TypeDB was *down* at
    /// startup, the store is a `NoopCausalGraphStore` forever: its `ping()`
    /// returns `Unavailable` unconditionally, no TTL expiry will ever
    /// re-attempt a connection, and the status stays `unavailable` until the
    /// process is restarted — no matter how healthy TypeDB becomes.
    ///
    /// Never panics and never propagates: `probe_liveness()` already collapses
    /// every failure mode to `Unavailable`, and `tokio::sync::Mutex` has no
    /// poisoning, so there is no error path to leak out of here either.
    async fn ping(&self) -> CausalGraphStatus {
        let mut cache = self.ping_cache.lock().await;
        if let Some((probed_at, status)) = cache.as_ref() {
            if probed_at.elapsed() < PING_CACHE_TTL {
                return status.clone();
            }
        }
        let status = self.probe_liveness().await;
        *cache = Some((std::time::Instant::now(), status.clone()));
        status
    }
}

impl TypeDbCausalGraphStore {
    /// The network half of `ping()`: opens a fresh read transaction and runs
    /// the same cheap presence-check query (`SCHEMA_PRESENCE_CHECK_TQL`) used
    /// by `connect()`'s self-repair logic, rather than inventing a new query
    /// string — it's already known-cheap (`limit 1`) and known-safe. Deliberately
    /// does NOT call the `schema_is_present` helper: that helper treats a
    /// query error as "schema absent" and swallows it into `Ok(false)`, which
    /// is correct for its own self-repair use case but wrong here — a query
    /// error during a liveness ping must be reported as `Unavailable`, not
    /// papered over.
    ///
    /// ## Why this runs the probe on a `spawn_blocking` task
    ///
    /// A plain `tokio::time::timeout(PING_TIMEOUT, probe).await` around the
    /// driver call is *not* sufficient to bound this call's latency, despite
    /// looking correct. Verified empirically against `typedb-driver` 3.12.1:
    /// when a previously-live connection goes away (the exact "TypeDB died
    /// mid-session" scenario this method exists to detect), the driver's
    /// reconnect path (`ServerManager::seek_primary_replica_in` in
    /// `connection/server/server_manager.rs`, and likewise
    /// `connection/network/transmitter/transaction.rs`) retries via
    /// `wait_for_primary_replica_selection`, which calls `std::thread::sleep`
    /// — a genuine OS-level blocking sleep, not `tokio::time::sleep` — for a
    /// hardcoded 2 seconds (the crate's own source has a `// FIXME: blocking
    /// sleep! Can't do agnostic async sleep.` comment on that exact line). A
    /// blocking sleep inside a `poll()` call cannot be preempted by
    /// `tokio::time::timeout`: the executor can't run the timer check until
    /// the blocking call returns control, so the *caller's* observed latency
    /// ends up governed by the driver's internal ~2s retry, not our 750ms
    /// budget. The probe therefore has to run somewhere else, and be raced
    /// against the timeout from a task that is still schedulable.
    ///
    /// It specifically must NOT be a plain `tokio::spawn`. That schedules onto
    /// the runtime's *core worker* pool, which under `#[tokio::main]`'s
    /// multi-threaded runtime is fixed at `num_cpus` and does not grow. Each
    /// timed-out ping abandons a probe that is still parked inside the
    /// driver's blocking sleep, and a core worker parked in synchronous code
    /// cannot run *any* other task. Under exactly the workload this endpoint
    /// exists for — a dashboard polling while TypeDB is down — those orphans
    /// accumulate, and once they reach the worker-pool size the whole
    /// application stalls, including the very `timeout`/join futures this
    /// method depends on to bound itself. A partial degradation would become a
    /// full-service hang.
    ///
    /// `tokio::task::spawn_blocking` uses the runtime's separate blocking
    /// pool, which is dynamically sized (512 threads by default) and is *for*
    /// code that blocks, so a parked probe can never starve the async
    /// runtime. The driver call is `async`, so the closure drives it with
    /// `Handle::block_on`. That placement is the safe one and the only one:
    /// `Handle::block_on` panics on a core worker thread, but blocking-pool
    /// threads are not an async execution context, so `block_on` is permitted
    /// there. The handle is captured *before* entering the closure
    /// (`Handle::current()` needs the async context that `ping()` itself
    /// runs in), and `block_on` is called nowhere else in this file.
    ///
    /// Abandoned probes are still possible (a `spawn_blocking` task cannot be
    /// cancelled), but they are now (a) harmless — they occupy a blocking
    /// thread, not a worker — and (b) rare, because the single-flight cache
    /// above admits at most one probe per `PING_CACHE_TTL`.
    ///
    /// Bounded by `PING_TIMEOUT` (750ms) as observed by the caller, much
    /// shorter than `CONNECT_TIMEOUT` (10s), since this is meant to be
    /// polled by a status endpoint, not run once at startup. Never panics
    /// and never propagates an error — any timeout, transaction error, or
    /// query error collapses to `Unavailable`, matching the "never fail or
    /// hang" contract this store already keeps elsewhere (`build_store`'s
    /// connect-timeout fallback).
    async fn probe_liveness(&self) -> CausalGraphStatus {
        let driver = std::sync::Arc::clone(&self.driver);
        let database = self.config.database.clone();
        // Captured here, in async context, because `Handle::current()` is only
        // valid inside the runtime; the closure below runs on a blocking-pool
        // thread and uses this handle to drive the async driver call.
        let handle = tokio::runtime::Handle::current();

        let probe = tokio::task::spawn_blocking(move || {
            // SAFETY-OF-PLACEMENT: this `block_on` executes only on a
            // blocking-pool thread (inside `spawn_blocking`), never on a core
            // worker, so it cannot panic with "Cannot block the current
            // thread from within a runtime" and cannot deadlock the executor.
            handle.block_on(async move {
                let transaction = driver
                    .transaction(&database, TransactionType::Read)
                    .await
                    .context("opening TypeDB read transaction for liveness ping")?;
                let answer = transaction
                    .query(SCHEMA_PRESENCE_CHECK_TQL)
                    .await
                    .context("running TypeDB liveness ping query")?;
                let mut rows = answer.into_rows();
                while let Some(row_result) = rows.next().await {
                    row_result.context("reading liveness ping row")?;
                }
                Ok::<(), anyhow::Error>(())
            })
        });

        // Racing the `JoinHandle` itself (rather than a oneshot) means a
        // timeout simply drops the handle, detaching the blocking task to
        // finish and be discarded on its own — and it lets a genuine panic be
        // told apart from an ordinary lost race, below.
        match tokio::time::timeout(PING_TIMEOUT, probe).await {
            Ok(Ok(Ok(()))) => CausalGraphStatus::Ready,
            Ok(Ok(Err(err))) => {
                tracing::debug!("TypeDB causal graph ping failed: {err:#}");
                CausalGraphStatus::Unavailable
            }
            Ok(Err(join_error)) if join_error.is_panic() => {
                // A real bug in the probe, not a race loss — worth a louder
                // level and a distinct message than the timeout path.
                tracing::warn!("TypeDB causal graph ping task panicked: {join_error}");
                CausalGraphStatus::Unavailable
            }
            Ok(Err(join_error)) => {
                tracing::debug!("TypeDB causal graph ping task was cancelled: {join_error}");
                CausalGraphStatus::Unavailable
            }
            Err(_elapsed) => {
                tracing::debug!(
                    "TypeDB causal graph ping exceeded {}ms budget; abandoning probe on the blocking pool",
                    PING_TIMEOUT.as_millis()
                );
                CausalGraphStatus::Unavailable
            }
        }
    }
}

pub async fn build_store(config: CausalGraphConfig) -> std::sync::Arc<dyn CausalGraphStore> {
    if !config.enabled {
        return std::sync::Arc::new(NoopCausalGraphStore::new(config));
    }
    // `TypeDbCausalGraphStore::connect()` sets `DriverOptions::request_timeout`,
    // but per the driver's own docs that only bounds unary RPCs (connection
    // open, database checks, transaction open) — NOT operations inside an
    // open transaction (schema query, commit), which simply `.await` the next
    // stream item with no timeout at all. A blackholed host (packets dropped,
    // no RST) can therefore hang `connect()` indefinitely even with
    // `request_timeout` set. Wrap the whole call in an outer timeout so
    // `build_store()` keeps its "never fail or hang startup" guarantee.
    match tokio::time::timeout(
        CONNECT_TIMEOUT,
        TypeDbCausalGraphStore::connect(config.clone()),
    )
    .await
    {
        Ok(Ok(store)) => std::sync::Arc::new(store),
        Ok(Err(err)) => {
            tracing::warn!(
                "TypeDB causal graph unavailable ({err:#}); falling back to SQLite/memory retrieval"
            );
            std::sync::Arc::new(NoopCausalGraphStore::new(config))
        }
        Err(_) => {
            tracing::warn!(
                "TypeDB causal graph connect timed out after {}s; falling back to SQLite/memory retrieval",
                CONNECT_TIMEOUT.as_secs()
            );
            std::sync::Arc::new(NoopCausalGraphStore::new(config))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IMPORTANT-3 regression test: pins the exact output of `escape_tql` for
    /// a crafted quote-breakout attempt. Without this, a future refactor of
    /// the two chained `.replace()` calls (e.g. simplifying or reordering
    /// them) could silently reintroduce TQL injection with nothing failing.
    #[test]
    fn escape_tql_escapes_quote_breakout_attempt() {
        let input = r#"x"; match $e isa episode; select $e; #"#;
        let escaped = escape_tql(input);
        assert_eq!(escaped, r#"x\"; match $e isa episode; select $e; #"#);
        assert!(
            !escaped.contains("x\";"),
            "escaped output must not contain the raw quote-semicolon breakout sequence"
        );
    }

    /// IMPORTANT-3 regression test: a lone backslash must be doubled.
    #[test]
    fn escape_tql_doubles_lone_backslash() {
        assert_eq!(escape_tql(r#"a\b"#), r#"a\\b"#);
    }

    /// IMPORTANT-3 regression test: this is the case that proves escape
    /// *order* is correct. Backslashes must be escaped before quotes — if
    /// quotes were escaped first, the backslash the quote-escape inserts
    /// would itself get doubled by a subsequent backslash pass, producing the
    /// wrong output. `escape_tql` must produce exactly `a\\\"b`: the original
    /// backslash doubled to `\\`, followed by the quote escaped to `\"`.
    #[test]
    fn escape_tql_orders_backslash_escape_before_quote_escape() {
        assert_eq!(escape_tql(r#"a\"b"#), r#"a\\\"b"#);
    }

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

    #[tokio::test]
    async fn noop_graph_reports_unavailable_when_typedb_configured() {
        let store = NoopCausalGraphStore::new(CausalGraphConfig {
            backend: CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "localhost:1729".to_string(),
            database: "harkonnen_semantic".to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        });

        let result = store
            .query(CausalGraphQuery {
                question: "what caused recent failures?".to_string(),
                run_id: None,
                spec_id: None,
                limit: 5,
            })
            .await
            .expect("query");

        assert_eq!(result.status, CausalGraphStatus::Unavailable);
        assert_eq!(result.backend, CausalGraphBackend::TypeDb3);
        assert!(result.hits.is_empty());
    }

    /// CRITICAL regression test: `build_store()` must never hang forever when
    /// the configured TypeDB host is unreachable in a way that produces no
    /// response at all (as opposed to an immediate connection refusal).
    /// `10.255.255.1` is an RFC1918-adjacent non-routable address that
    /// reliably blackholes traffic (packets dropped, no RST, no ICMP
    /// unreachable) rather than refusing the connection outright — this is
    /// exactly the failure mode `request_timeout` alone cannot bound, since
    /// per the driver's docs that timeout doesn't cover in-transaction
    /// operations. Does not require a live TypeDB instance; runs in the
    /// normal suite.
    #[tokio::test]
    async fn build_store_falls_back_to_noop_on_blackholed_host() {
        let config = CausalGraphConfig {
            backend: CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "10.255.255.1:1729".to_string(),
            database: "harkonnen_semantic_blackhole_test".to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };

        let started = std::time::Instant::now();
        let store = build_store(config).await;
        let elapsed = started.elapsed();

        let result = store
            .query(CausalGraphQuery {
                question: "what caused recent failures?".to_string(),
                run_id: None,
                spec_id: None,
                limit: 5,
            })
            .await
            .expect("query");

        assert_eq!(result.status, CausalGraphStatus::Unavailable);
        // Budget tracks CONNECT_TIMEOUT rather than a free-floating constant:
        // the bound this test defends *is* CONNECT_TIMEOUT, so a change to it
        // should move the assertion automatically. The 5s margin covers the
        // outer timeout firing and the fallback to Noop. The previous bound
        // was a flat 30s against a 10s timeout — a 3x margin that would still
        // have passed if the real elapsed time regressed to 25s.
        let budget = CONNECT_TIMEOUT + std::time::Duration::from_secs(5);
        assert!(
            elapsed < budget,
            "build_store() against a blackholed host took {elapsed:?}, expected under {budget:?} (CONNECT_TIMEOUT + 5s)"
        );
    }

    /// CRITICAL regression test (part 1 of 2): documents *why* the
    /// `Unavailable`-not-`Err` contract test below has to be a live test.
    ///
    /// The obvious unit test — build a `TypeDbCausalGraphStore` pointed at a
    /// closed port and query it — is not constructible: `connect()` performs a
    /// real driver handshake (`TypeDBDriver::new` →
    /// `ServerManager::new(...).await?`) before it can hand back a store, and
    /// there is no store without an `Arc<TypeDBDriver>` to put in it. This
    /// test pins that fact so the reasoning stays checkable: a closed port
    /// yields `Err` from `connect()`, fast, and therefore never yields a store
    /// whose `query()` could be exercised. Making one constructible anyway
    /// would mean making the driver field optional or mock-shaped in
    /// production code purely for test convenience — a weakening, so it was
    /// not done. Requires no live TypeDB.
    #[tokio::test]
    async fn connect_cannot_build_a_store_against_a_closed_port() {
        let config = CausalGraphConfig {
            backend: CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "localhost:19999".to_string(),
            database: "harkonnen_semantic_closed_port_test".to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };

        let started = std::time::Instant::now();
        let outcome = TypeDbCausalGraphStore::connect(config).await;
        let elapsed = started.elapsed();

        assert!(
            outcome.is_err(),
            "connect() to a closed port must fail; if this ever starts succeeding, \
             the Unavailable-not-Err contract test can and should be rewritten as a \
             non-live unit test"
        );
        assert!(
            elapsed < CONNECT_TIMEOUT,
            "connect() to a closed port should fail fast (connection refused), took {elapsed:?}"
        );
    }

    /// CRITICAL regression test (part 2 of 2, pure half): pins the shape of
    /// the result every per-query failure path collapses to. The design spec
    /// requires `status: Unavailable` with the error text in `note` and no
    /// hits — never an `Err` — so that `format_causal_graph_note`'s
    /// `Unavailable` arm in `src/api.rs` can surface it and the caller can
    /// keep the SQLite projection-ledger hits it already gathered. Requires no
    /// live TypeDB. (The routing half — that `query()` actually reaches this
    /// constructor rather than propagating `?` — is covered by
    /// `query_returns_unavailable_not_err_when_typedb_is_gone` below.)
    #[test]
    fn unavailable_query_result_has_the_spec_mandated_shape() {
        let config = CausalGraphConfig {
            backend: CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "localhost:1729".to_string(),
            database: "harkonnen_semantic".to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };

        let result = unavailable_query_result(
            &config,
            "what caused recent failures?".to_string(),
            "typed causal graph query failed: boom".to_string(),
        );

        assert_eq!(result.status, CausalGraphStatus::Unavailable);
        assert_eq!(result.backend, CausalGraphBackend::TypeDb3);
        assert_eq!(result.database, "harkonnen_semantic");
        assert_eq!(result.query, "what caused recent failures?");
        assert!(
            result.hits.is_empty(),
            "a failed query must not fabricate hits"
        );
        assert_eq!(
            result.note.as_deref(),
            Some("typed causal graph query failed: boom"),
            "the error text must reach the operator-visible note"
        );
    }

    /// CRITICAL regression test (part 2 of 2, routing half): a `query()` with
    /// `run_id: Some(..)` — i.e. one that reaches the transaction path rather
    /// than the `run_id: None` short-circuit — must return
    /// `Ok(status: Unavailable)` when the typed graph cannot answer, NOT an
    /// `Err`. Before this fix, `?` on transaction-open / query / row decode
    /// bubbled out through `answer_general_coobie_query` →
    /// `execute_coobie_query` → HTTP 500, discarding SQLite projection-ledger
    /// hits that had already been computed.
    ///
    /// Live-only by necessity, not by preference: see
    /// `connect_cannot_build_a_store_against_a_closed_port` above — a store
    /// cannot exist without a successful driver handshake, so the only way to
    /// get a store whose backing graph is unusable is to connect to a real
    /// TypeDB and then take the graph away. This does that by deleting the
    /// database out from under the live store, which makes the *first* driver
    /// call in `query()` (transaction open) fail — deterministically, in
    /// milliseconds, and without stopping the container out from under the
    /// other live tests in this suite.
    #[tokio::test]
    #[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
    async fn query_returns_unavailable_not_err_when_typedb_is_gone() {
        let db_name = "harkonnen_semantic_query_failure_check";

        let config = CausalGraphConfig {
            backend: CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "localhost:1729".to_string(),
            database: db_name.to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };

        let store = TypeDbCausalGraphStore::connect(config)
            .await
            .expect("connect and deploy schema");

        // Take the graph away from the connected store. The driver handle
        // stays valid; the database it points at no longer exists, so the
        // transaction open inside query() fails.
        store
            .driver
            .databases()
            .get(db_name)
            .await
            .expect("get database")
            .delete()
            .await
            .expect("delete database out from under the live store");

        let started = std::time::Instant::now();
        let result = store
            .query(CausalGraphQuery {
                question: "what caused recent failures on this run?".to_string(),
                run_id: Some("seed-run-1".to_string()),
                spec_id: None,
                limit: 5,
            })
            .await
            .expect("query() must return Ok(Unavailable), never Err — an Err here is an HTTP 500");
        let elapsed = started.elapsed();

        assert_eq!(
            result.status,
            CausalGraphStatus::Unavailable,
            "a per-query failure must be reported as Unavailable, got {:?}",
            result.status
        );
        assert_eq!(result.backend, CausalGraphBackend::TypeDb3);
        assert_eq!(result.database, db_name);
        assert_eq!(result.query, "what caused recent failures on this run?");
        assert!(
            result.hits.is_empty(),
            "a failed query must not fabricate hits"
        );
        let note = result
            .note
            .expect("failure note must be present for the Unavailable arm");
        assert!(
            note.contains("typed causal graph query failed"),
            "note should be diagnostic, got: {note}"
        );
        assert!(
            elapsed < QUERY_TIMEOUT,
            "a driver-level failure should surface well inside QUERY_TIMEOUT, took {elapsed:?}"
        );
    }

    /// IMPORTANT-1 regression test: a database that exists but is missing the
    /// Coobie semantic schema (simulating a prior partial failure — e.g.
    /// `dbs.create()` succeeded but the schema transaction never committed)
    /// must be self-repaired by `connect()`, not silently accepted as
    /// "already provisioned". Constructs that exact state directly via the
    /// raw driver (create the database, deliberately skip schema deployment),
    /// then calls `TypeDbCausalGraphStore::connect()` against it and confirms
    /// the schema is present afterward.
    #[tokio::test]
    #[ignore = "requires a live TypeDB instance: docker compose -f docker-compose.calvin.yml up -d typedb"]
    async fn connect_repairs_database_left_without_schema() {
        let db_name = "harkonnen_semantic_repair_check";

        // Set up the broken state directly against the raw driver, bypassing
        // TypeDbCausalGraphStore::connect() entirely so no schema is deployed.
        {
            let credentials = Credentials::new("admin", "password");
            let options = DriverOptions::new(DriverTlsConfig::disabled());
            let addresses =
                Addresses::try_from_address_str("localhost:1729").expect("parse address");
            let driver = TypeDBDriver::new(addresses, credentials, options)
                .await
                .expect("connect to local TypeDB");
            let dbs = driver.databases();
            if dbs.contains(db_name).await.expect("check exists") {
                dbs.get(db_name)
                    .await
                    .expect("get")
                    .delete()
                    .await
                    .expect("delete stale test database");
            }
            dbs.create(db_name)
                .await
                .expect("create database without schema");

            // Confirm the broken state is real before testing the repair.
            let present = TypeDbCausalGraphStore::schema_is_present(&driver, db_name)
                .await
                .expect("schema presence check");
            assert!(
                !present,
                "test setup invariant: database should have no schema yet"
            );
        }

        let config = CausalGraphConfig {
            backend: CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "localhost:1729".to_string(),
            database: db_name.to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };

        let store = TypeDbCausalGraphStore::connect(config)
            .await
            .expect("connect() should repair the missing schema, not error");

        let repaired = TypeDbCausalGraphStore::schema_is_present(&store.driver, db_name)
            .await
            .expect("schema presence check after repair");
        assert!(
            repaired,
            "connect() should have deployed the schema to the pre-existing, schemaless database"
        );

        // Clean up the test database.
        store
            .driver
            .databases()
            .get(db_name)
            .await
            .expect("get for cleanup")
            .delete()
            .await
            .expect("cleanup");
    }
}
