# Harkonnen Labs — Execution Roadmap

**Primary goal: Calvin Archive as the backing for Coobie's memory.**
The fastest reasonable path: Phase v1 (structural gaps) → Phase 2 (testable harness)
→ Phase 5-C (context gating, no new infra) → Phase 5b (Qdrant, memory refactor)
→ Phase 6 (TypeDB) → Phase 7 (causal corpus) → Phase 8 (Calvin Archive).
Phase 3 (docs, DevBench, benchmark suites) follows the archive path rather than
interrupting it. Live twin provisioning is permanently deferred unless a product
explicitly requires running service virtualization. Phase 2's real test execution
IS the testable harness.

---

## Maturity Ladder

| Maturity phase | Meaning | Harkonnen status |
| --- | --- | --- |
| Phase 1 — Assisted Intelligence | Copilots, chatbots, drafting help | Already surpassed |
| Phase 2 — Automated Intelligence | Rule-based workflows, permissions, governance | Already surpassed as a standalone destination |
| Phase 3 — Augmented Intelligence | Core agent with proactive suggestions, learning loops, human confidence-building | Current baseline |
| Phase 4 — Agentic Intelligence | Self-directed agents inside explicit guardrails, with structural coordination and self-monitoring | Active destination — Phase v1 closes the remaining gap |

### What still separates Harkonnen from Phase 4 (verified in source, 2026-04-18)

A structured gap analysis identified seven practical gaps. Gap-closure phases A–D addressed five of them. Three structural gaps remain before the system can legitimately claim Tier 4:

| Gap | Gap-closure status |
| --- | --- |
| Enforced authority and guardrail boundaries | **Open** — `check_lease` API exists but is never called pre-write; guardrails are advisory only |
| Live world-state modeling | Deferred — twin is still a manifest; live provisioning is permanently deferred unless a product needs it |
| Closed-loop outcome verification | Partial — observation endpoint deferred to Phase E (TypeDB dependency) |
| Structural multi-agent coordination | Mostly closed — blackboard, heartbeat, claim eviction are real |
| Economic and cost awareness | Closed — A1 trace spine + cost events |
| Explicit intent → plan → execution separation | Closed — B, C (OptimizationProgram) |
| External system interfaces | Open — Phase v1 External Integrations track |

### How this roadmap closes that gap

- `.harkonnen/gap-closure-progress.md` tracks strategic bridge work phases A–D (all shipped)
- Phase v1 (below) is the structural gate before the factory can be called Tier 4
- After v1, the roadmap drives straight toward the Calvin Archive (Phases 5-C → 5b → 6 → 7 → 8)
- Phase 3 benchmarks and docs follow the archive path
- Operator Model and External Integrations are parallel product tracks

---

## Twin Policy

**Bramble's real test execution (Phase 2) is the testable harness.** There is no mandatory dependency on Docker-backed service virtualization anywhere in this roadmap. "Digital twin" in this system means the manifest-based twin fidelity score used for diagnostic telemetry — it does not mean a running containerized replica of the target service. `twin_fidelity_score` remains available as optional telemetry. Phase 3-D exists only as a maintenance note for that signal; it is not a Phase 3 completion gate.

If a specific product built on Harkonnen requires live service virtualization for its own testing needs, that capability can be revisited then with that product's requirements as the driver. It does not belong in the core factory sequence.

---

## Why this order

The factory needs a clear line to the Calvin Archive. That line runs through:

1. **v1** — closes structural gaps that make the foundation advisory rather than enforced: guardrails, memory invalidation persistence, failure classification, operator model MVP.
2. **Phase 2** — Bramble real test execution. This is the testable harness. `validation_passed` means nothing until it reflects real test output rather than stubs. The fix loop's wrong-answer path depends on real exit codes. Everything downstream depends on this being grounded.
3. **Phase 5-C** — per-phase context gating requires no new infrastructure. It can be done while the codebase is still pre-refactor. The Sable isolation constraint is a correctness issue that gets harder to retrofit as the memory module grows.
4. **Phase 5b** — Qdrant, OCR, and the memory module refactor prepare the semantic layer for TypeDB. TypeDB pattern queries are only as good as the retrieval they sit on top of. The module refactor also needs to happen before Phase 5-C's `BriefingScope` work lands in a coherent location.
5. **Phase 6** — TypeDB is the Calvin Archive's prerequisite. No TypeDB, no typed chamber schema, no cross-run causal queries.
6. **Phase 7** — build the causal attribution corpus while the TypeDB layer is fresh. This populates the archive with real, labeled evidence before the governance layer opens.
7. **Phase 8** — the Calvin Archive: persisted identity, governed integration, D*/SSA streaming. This is the destination.
8. **Phase 3** — documentation, DevBench, benchmark suites. Important for external claims and usability but not on the Calvin Archive critical path. Ships after the archive is live so benchmarks run against the real system.

Parallel tracks (External Integrations, Operator Model, Hosted/Team, Calvin Archive Visualizer) advance independently of the above sequence and do not block it.

Benchmark wiring advances in lockstep with implementation phases. Each phase ships with at least one measurable gate. The benchmark philosophy remains explicitly agentic-engineering shaped: measure how quickly and safely software moves through the delivery system, not just how quickly code is emitted.

---

## Phase v1 — Tier 4 Finalization

**This is the active build target.** Closes the remaining structural gaps that prevent Harkonnen from being called a genuine Tier 4 agentic workflow.

### v1-A — Guardrail Enforcement (hard blocker for Tier 4)

**Why it's a blocker:** Tier 4 requires agents to operate *inside* explicit guardrails, not just record them. Currently `check_lease` exists in `src/api.rs` but is never called from `src/orchestrator.rs`. Every Mason file write bypasses the lease system. Decision records are written *after* the fact, not enforced *before* the act.

**What to build:**

- Call `POST /api/coordination/check-lease` inside `mason_generate_and_apply_edits` before writing any file — pass `resource_kind: "workspace"`, the staged path prefix, and the run's guardrails from the Coobie briefing. If the response is denied or returns violations, return an error rather than proceeding and write a decision record explaining why the write was blocked.
- Wire the same check in the Mason plan generation path: claim `resource_kind: "workspace"` at plan start with `ttl_secs` derived from the spec's time budget
- Add at least three more `record_decision` call sites: Mason plan selection, Scout optimization program derivation, Sable attack generation. Currently only Coobie critique and consolidation promotion are wired.
- Add `GET /api/runs/:id/decisions` to the Pack Board run detail drawer so operators can inspect the decision audit trail per run

**Done when:** A Mason edit attempt against a path that has no active workspace lease is blocked at the orchestrator level, a decision record is written explaining the block, and the Pack Board surfaces the decision log per run.

---

### v1-B — Memory Invalidation Persistence (Phase 4b completion)

**Why:** The invalidation layer is computed at query time only — there is no `memory_updates` table, no `invalidated_by` column on memory records, and no `GET /api/memory/updates` endpoint. The StreamingQA adapter cannot score belief-update accuracy against a persistence layer that does not exist.

**What to build:**

- `memory_updates` table in `src/db.rs`: `(update_id, old_memory_id, new_memory_id, reason, created_at)`
- `invalidated_by: Option<String>` on the memory record schema (references `update_id`)
- Coobie ingest pipeline: before writing a new memory entry, check for semantic near-duplicates with conflicting claims via cosine similarity on the embedding store. If found above threshold, write a supersession record and set `invalidated_by` on the old entry.
- `GET /api/memory/updates` endpoint returning supersession history
- Memory Board UI panel: distinguish invalidated entries from current entries; allow operator to confirm or reject a supersession
- **StreamingQA native adapter** — streams fact-update events to Coobie's memory, then queries whether the updated belief is correctly recalled. Scores belief-update accuracy separately from static recall.

**Done when:** Ingesting a new fact that contradicts an older one persists a supersession record, the old entry is flagged in the DB, `GET /api/memory/updates` returns the history, and StreamingQA has a baseline score.

---

### v1-C — FailureKind Classification

**Why:** Mason's fix loop handles all failures identically. A wrong-answer failure (test ran, output was wrong) requires a different fix prompt than a compile error (code never ran).

**What to build:**

- `FailureKind` enum in `src/models.rs`: `CompileError`, `TestFailure`, `WrongAnswer`, `Timeout`, `Unknown`
- Parser in the fix loop that classifies stdout/stderr: detect "expected … got …", "FAILED", "assertion failed", exit code patterns
- `WrongAnswer` variant triggers a distinct Mason prompt that includes the expected vs actual diff rather than the raw compiler error
- `failure_kind` field on `ValidationSummary` so Coobie can pattern-match on failure type in its causal records

**Done when:** A run with a wrong-answer test failure shows `failure_kind: WrongAnswer` in the run summary and Mason's fix attempt uses the diff-focused prompt.

---

### v1-D — Operator Model Minimum Viable

**Why:** Scout's intent generation and Coobie's preflight have no connection to operator context. Without this, every new spec starts from scratch regardless of how well Coobie knows the operator's patterns.

**What to build (two-layer MVP, not the full five-layer spec):**

- PackChat `interview` command: initiates a two-layer intake (operating rhythms + recurring decisions) with checkpoint approval after each layer
- `commissioning-brief.json` artifact generated from the approved layers: contains operator's primary work patterns, preferred tools, recurring decisions, and risk tolerances
- Scout draft integration: when a `commissioning-brief.json` exists for the operator, Scout includes its top-3 patterns in the intent package prompt
- Coobie preflight integration: operator's stated risk tolerances contribute to `required_checks` and guardrail text
- Update the `operator_model_sessions` and `operator_model_layer_checkpoints` tables (schema exists; no logic is wired)

**Done when:** An operator who has completed the two-layer interview sees their stated patterns reflected in Scout's intent packages and Coobie's required checks on subsequent runs.

---

### v1-E — Transactional Execution And Approval Boundaries

**Why:** Guardrails are stronger when high-impact actions have explicit transaction boundaries rather than relying on best-effort cleanup after a mistake. If a run is about to mutate sensitive code, open a privileged MCP surface, or cross a policy threshold, Harkonnen should be able to pause, request approval, and either commit or roll back from a known boundary. This is the operational analogue of the Soul-of-AI requirement that continuity and policy remain inspectable rather than implicit.

**What to build:**

- Transaction envelope for high-impact phases: capture an explicit pre-action snapshot, planned mutation set, approval state, and rollback note before execution proceeds
- Human-interrupt checkpoint for guarded transitions: if Keeper or Coobie flags a privileged step, the run pauses at a reversible boundary rather than drifting forward and apologizing later
- Rollback artifact written per guarded transition: what was attempted, what state changed, what was restored, and what residual risk remains
- Decision-log integration: approval, commit, rollback, and abort outcomes become explicit decision records rather than only phase logs

**Done when:** A guarded run can pause before a privileged transition, record an approval or rejection, and either commit or roll back from a named boundary with an auditable artifact.

---

### v1 Benchmark / product gate

- Decision audit log surfaced in Pack Board per run
- Memory supersession events returned by `GET /api/memory/updates`
- StreamingQA first run published — belief-update accuracy
- At least one run showing `failure_kind: WrongAnswer` in the validation summary
- At least one run where Scout's intent package references operator model context

---

## Phase 2 — Bramble Real Test Execution

**This is the testable harness.** Until this ships, `validation_passed` reflects scenario results and stubs rather than real test output. Every downstream quality signal (Coobie's `test_coverage_score`, Phase 3 benchmarks, the fix loop's wrong-answer path) depends on real exit codes coming from real test commands.

**What to build:**

- `bramble_run_tests` in orchestrator — reads `spec.test_commands` (same detection logic as Piper) and executes them in the staged workspace
- Stdout/stderr streamed as `LiveEvent::BuildOutput` on the broadcast channel (already exists — Bramble just needs to use it)
- `ValidationSummary` populated from real exit codes and parsed test output, not from scenario results or stubs
- Bramble's phase attribution records `validation_passed: true/false` from actual runs
- Feed result back as `test_coverage_score` into the Coobie episode at ingest time
- **Mason online-judge feedback loop** — `FailureKind::WrongAnswer` (wired in v1-C) feeds into Mason's fix loop with a diff-focused prompt. Phase 2 formalises the loop end-to-end: parse stdout diff output from competitive programming judges as a first-class failure signal.
- **LiveCodeBench adapter** — wrapper command that pulls recent problems, runs Mason/Piper, and emits pass/fail per problem into the benchmark runner.
- **Aider Polyglot adapter** — maps Aider's multi-language benchmark format to Harkonnen specs; no structural changes needed.

**Benchmark gate:**

- `local_regression` stays green on every merge
- the code loop should be runnable through the emerging `SWE-bench Verified` adapter, even if scores are unpublished
- `LiveCodeBench` adapter wired and producing artifacts
- `Aider Polyglot` adapter wired for a direct open-source comparison line

**Done when:** A spec with `test_commands` shows real pass/fail in the run report, Coobie's episode scores reflect actual test execution, and Mason's fix loop handles wrong-answer failures.

---

## Phase 5-C — Per-Phase Context Gating for Coobie Briefings

**Why:** Every agent currently receives the same Coobie preflight briefing regardless of role or phase. Scout, Mason, and Sable have fundamentally different information needs: Scout needs spec history and prior ambiguities; Mason needs failure patterns and workspace guardrails; Sable needs scenario patterns and what Mason changed. Giving each agent the full undifferentiated corpus wastes context window and, more importantly, risks priming agents with information they should not see at their phase — most critically, Sable should not see Mason's implementation reasoning before scoring hidden scenarios.

This is a retrieval-shaping capability, not a storage change. It does not require TypeDB or Qdrant. It is placed here — before the memory module refactor — because the `BriefingScope` enum and filter logic can land in `src/coobie.rs` now and move cleanly into `src/memory/briefing.rs` during Phase 5b's refactor.

**What to build:**

- `BriefingScope` enum in `src/coobie.rs` (migrates to `src/memory/briefing.rs` in Phase 5b): `ScoutPreflight`, `MasonPreflight`, `PiperPreflight`, `SablePreflight`, `CoobiConsolidation`, `OperatorQuery`. Each variant carries a `phase_id` and a `role` tag.
- Scope-keyed retrieval filter: each scope defines an `allow_categories` list (e.g. Scout: `spec_history, prior_ambiguities, operator_model`; Mason: `failure_patterns, fix_patterns, workspace_guardrails, causal_links`; Sable: `scenario_patterns, hidden_scenario_outcomes` — explicitly excludes Mason implementation notes).
- `build_scoped_briefing(scope: BriefingScope, run_id, spec_id) -> BriefingPackage` replaces the current single-path `build_preflight_briefing`. Internally calls the same multi-hop retrieval chain but filters retrieved hits against the scope's `allow_categories` before assembling the briefing text.
- **Stamped project interview context as first-class preflight input** — the repo-stamp interview's Mythos/Pathos/Ethos/Episteme/Praxis material (purpose, stakes, stakeholder attitudes, prohibitions, vertical, skill sources, MCP posture) should be loaded from `.harkonnen/repo.toml` and injected into Scout + Coobie briefing shaping. This keeps project posture inspectable and continuity-aligned rather than leaving it trapped in generated markdown artifacts.
- Wire in orchestrator: pass the correct `BriefingScope` at each phase entry point (Scout, Mason, Sable are the critical three; others can default to `OperatorQuery` for now).
- Coobie episode record: add `briefing_scope` field so causal analysis can distinguish whether a lesson was visible at the relevant phase or not.

**Sable isolation constraint (non-negotiable):** `SablePreflight` scope must never include retrieved hits tagged `implementation_notes`, `mason_plan`, or `edit_rationale`. This is the hidden-scenario firewall. If a hit's tag set intersects these, it is dropped regardless of relevance score.

**Done when:** Scout, Mason, and Sable each receive a distinct briefing shaped to their role; stamped repo interview context is visible in the relevant preflight surfaces; a log entry confirms which scope was used per phase; and Sable's briefing verifiably contains no Mason implementation content.

---

## Phase 5b — Memory Infrastructure (Qdrant + OCR)

**Unlocks:** Semantic recall at scale and document ingest completeness. The SQLite vector store is sufficient for current run volume, but it becomes the bottleneck as the memory corpus grows. This phase also creates the clean module structure that TypeDB's semantic layer will build on in Phase 6.

**What to build:**

- **Qdrant integration** — add `src/coobie/qdrant.rs` implementing the semantic index over extracted text and memory summaries. Payload metadata: `org`, `role`, `product`, `spec_id`, `run_id`, `agent`, `memory_type`, `tags`, `created_at`. Qdrant replaces the SQLite vector store for long-term semantic memory (keep SQLite as the short-term and episodic store). Bootstrap script at `scripts/bootstrap-coobie-memory-stack.sh` already exists.
- **OCR pipeline** — add Tesseract-backed OCR for scanned PDFs and images. Current extractors handle text-forward formats but cannot read scanned documents. Wire through the existing `memory ingest` path: detect image-only PDFs, invoke `tesseract`, write extracted text sidecar alongside the imported asset.
- **Memory module refactor** — split the growing `src/memory.rs` into the module tree described in COOBIE_SPEC: `src/memory/mod.rs`, `working.rs`, `episodic.rs`, `semantic.rs`, `causal.rs`, `consolidation.rs`, `blackboard.rs`, `retrieval.rs`, `extraction.rs`. Migrate Phase 5-C's `BriefingScope` into `src/memory/briefing.rs` during this refactor. No behavior change otherwise; this is a maintainability gate before TypeDB lands.

**Benchmark gate:**

- re-run `FRAMES` after Qdrant lands to confirm multi-hop recall improves over the SQLite vector baseline
- `LongMemEval` and `LoCoMo` re-run to confirm semantic recall quality does not regress
- re-run `StreamingQA` to confirm belief-update accuracy does not regress after the module refactor

**Done when:** Qdrant is serving semantic queries for long-term memory, OCR-scanned PDFs can be ingested, and `src/memory.rs` is split into the COOBIE_SPEC module tree with `BriefingScope` in its final home.

---

## Phase 6 — TypeDB Semantic Layer

**Unlocks:** Typed causal queries that vector similarity cannot answer. "Find all runs where TWIN_GAP caused a failure that was fixed by an intervention that held for ≥ 3 runs" requires a graph, not a similarity score. This is also the direct prerequisite for the Calvin Archive's chamber schema.

TypeDB 3.x changes the implementation assumptions: the old JVM burden objection is gone because TypeDB's core is now Rust. It is still an external service with real operational cost, so it stays later in the sequence and should not replace SQLite as the hot path. When this phase opens, use the Rust-based TypeDB 3.x line in a container-first deployment and avoid the legacy Java server/distribution entirely.

**What to build:**

- TypeDB 3.x instance configured in the home-linux setup TOML
- `src/coobie/semantic.rs` implementing the `SemanticMemory` trait from COOBIE_SPEC
- Rust-facing TypeDB adapter using the official TypeDB 3.x driver behind the `SemanticMemory` abstraction
- TypeDB schema from COOBIE_SPEC: entities (agent, goal, episode, observation, action, outcome, artifact, lesson, failure-mode, causal-link), relations as specified
- TypeDB 3.x function-backed semantic reasoning; do not design around legacy rules-engine assumptions
- Write-back: after Phase 5 consolidation approval, promoted lessons and causal links written to TypeDB as well as the file store
- Query surface: `POST /api/coobie/query` routes natural-language causal questions through Coobie's retrieval chain
- Coobie's briefing builder calls TypeDB for cross-run pattern queries before preflight
- **GAIA Level 3 adapter** — maps GAIA's multi-step tool-use tasks to Harkonnen's factory run format; routes sub-tasks to the appropriate Labrador rather than a single generalist. Requires the TypeDB query surface to be live.
- **AgentBench adapters** — OS, database, and web environments, each mapped to a Labrador role.

**Benchmark gate:**

- cross-run causal-query benchmarks comparing SQL aggregate recall versus TypeDB-backed semantic recall
- `GAIA Level 3` first run published
- `AgentBench` first runs across OS, DB, and web environments

**Done when:** You can ask Coobie "what caused the last three failures on this spec" and get an answer from a typed graph; GAIA Level 3 and AgentBench adapters wired and producing artifacts.

---

## Phase 7 — Causal Attribution Corpus and E-CARE

**Unlocks:** The strongest publishable internal benchmark claims, and a populated evidence base for the Calvin Archive. Building the corpus here — immediately after TypeDB is live — means the archive opens with real labeled data rather than starting cold.

**What to build:**

- **Causal attribution accuracy corpus** — 30–50 labeled runs with seeded failures (wrong API version, missing env var, breaking schema change, etc.). Each entry has a spec, a seeded failure, a ground-truth cause label, and the Coobie `diagnose` output. Score top-1 and top-3 accuracy. Start with 10 entries for a first baseline. Lives in `factory/benchmarks/causal-attribution/`.
- **E-CARE native adapter** — maps Coobie's `diagnose` output to E-CARE's evaluation format and scores whether generated causal explanations are judged natural-language coherent. Run after consolidation so promoted lessons can inform subsequent diagnose output.
- Publish before/after comparisons for causal attribution accuracy: pre-Phase 4 (pure semantic recall) versus post-Phase 6 (TypeDB causal graph-augmented).

**Benchmark gate:**

- `E-CARE` first run published — causal explanation coherence score
- `causal attribution accuracy` first run published — top-1 / top-3 vs semantic-only baseline

**Done when:** The corpus has at least 30 labeled entries, the causal attribution accuracy benchmark has a published run, and E-CARE has a published score.

---

## Phase 8 — The Calvin Archive And Governed Integration

**Unlocks:** A persisted intelligence layer that does not merely remember, but
decides what becomes part of itself. This is the phase where Harkonnen moves
from identity continuity as a typed graph to identity continuity as a governed
integration process.

The design for this phase — including the formal metrics and the three-tier data
stack — is specified in [the-soul-of-ai/08-Identity-Continuity.md](the-soul-of-ai/08-Identity-Continuity.md)
and the integration-governance design in [the-soul-of-ai/07-Governed-Integration.md](the-soul-of-ai/07-Governed-Integration.md).

**What to build:**

**Storage layer (three-tier):**

- **TimescaleDB hypertable bootstrap** — episodic behavioral telemetry store for agent events, drift samples, stress accumulation, and SSA snapshots. Hypertable compression policy (7-day chunks, 30-day retention window). Provides the time-series foundation for D* estimation and stress computation.
- **TypeDB Calvin Archive schema** — typed ontological layer for the six chambers (Mythos, Episteme, Ethos, Pathos, Logos, Praxis), integration candidates, quarantine entries, revision graphs, and causal patterns. Schema spec in MASTER_SPEC Part 5.
- **Materialize streaming SQL views** — real-time `D*` drift monitoring (sliding window over TimescaleDB events via SUBSCRIBE), live Meta-Governor alert views, and SSA tracking views. `D*` and SSA are the two primary continuous signals; Φ and F are computed on-demand.

**Governance and integration:**

- Calvin Archive Meta-Governor with explicit `accept`, `modify`, `reject`, and `quarantine` outcomes for identity-relevant integration events
- File-first soul package projection with `soul.json`, `SOUL.md`, `IDENTITY.md`, `AGENTS.md`, `STYLE.md`, `MEMORY.md`, and `HEARTBEAT.md`, generated from and checked against canonical continuity state
- Integrity-hash verification and heartbeat audits so the projected soul package cannot drift silently away from the Calvin Archive
- Explicit continuity contract: `SOUL.md` declares the identity kernel; the Calvin Archive proves its continuity through experience, revision, and quarantine history
- Quarantine ledger: unresolved items persist with pending evidence conditions, salience decay, and re-evaluation triggers
- Pattern-level reflection over compressed cross-episode structures so schema revision is distinct from ordinary belief revision
- Stress-estimator computation (backed by TimescaleDB) so recurring unresolved strain triggers governed reflection instead of ad hoc self-rewrite
- Slow-loop integration-policy revision flow, more conservative than ordinary updates and naturally attachable to human endorsement
- Cross-layer hysteresis measurement so rollback quality is judged by residual behavioral drift, not only by restored file contents
- Presence continuity checks so model/provider swaps preserve identity semantics rather than resetting the pack by accident
- Pathology detection for trauma-analog overweighting, denial, fragmentation, and hyper-local overfitting

**Metrics implementation (from chapter 07):**

- **`D*` (Drift Bound)** — `D* = α/γ`, where α is behavioral deviation rate (from episodic log) and γ is recovery rate (from consolidation events). Materialize view watches `D*` continuously; Meta-Governor triggered if session drift exceeds bound.
- **SSA (Semantic Soul Alignment)** — cross-domain weighted action-pattern consistency against Labrador persona goals. Computed per run window and stored as a TimescaleDB event.
- **F (Variational Free Energy)** — KL divergence between agent's generative model and actual observations; high F signals that the agent must seek clarification or update beliefs. Computed on-demand, not streamed.
- **Φ (Integrated Information)** — bipartition-minimized causal integration measure over the Calvin Archive graph. Used to gate Calvin Archive updates: a post-learning drop in Φ triggers quarantine rather than direct integration.

**Benchmark gate:**

- D* (unjustified-drift score) published — continuous via Materialize view
- SSA baseline score published — per-run, stored in TimescaleDB
- healthy quarantine-rate / resolution-rate baseline published
- schema-revision stability benchmark published
- stress / hysteresis recovery benchmark published
- Φ post-learning drop detection wired (quarantine trigger, not yet a published score)

**Done when:** Harkonnen can distinguish accepted, rejected, modified, and quarantined identity changes; the projected soul package is verifiable against canonical continuity state; D* and SSA are instrumented and streaming; reflection can revise schemas without overwriting raw experience; rollback quality is measured through hysteresis rather than assumed; and policy-level revision is slower, more conservative, and explicitly reviewable.

---

## Phase 3 — Documentation, Evaluation, and Lifecycle Benchmarks

**Sequenced after Phase 8** so benchmarks run against the complete system rather than a pre-archive baseline. The DevBench adapter and spec adherence scores are most meaningful once the factory's full memory and governance stack is live. Phase 3 items with no archive dependency (Flint docs, 3-B, 3-C) can begin earlier if capacity allows, but Phase 3 is not a gate on the Calvin Archive path.

**Twin policy note:** Live twin provisioning is not a Phase 3 completion gate. Phase 3-D (twin fidelity benchmark) is optional diagnostic telemetry only — see Twin Policy above.

### 3-A — Flint documentation phase

- After `self.package_artifacts(run_id)` in the Flint phase, call a new `flint_generate_docs` method
- `flint_generate_docs` reads the spec and Mason's implementation artifacts from the run dir, calls the Flint LLM agent to generate a `README.md` and optionally an `API.md`
- Writes output to `artifacts/docs/<run_id>/README.md` and `artifacts/docs/<run_id>/API.md`
- Adds `docs/README.md` to `blackboard.artifact_refs`
- Required for DevBench — must land before the DevBench gate

### 3-B — `src/spec_adherence.rs` — LLM-as-judge benchmark

New builtin benchmark module (follows the same pattern as `cladder.rs`).

- Loads a JSONL file where each line is `{ "run_id": "...", "spec_path": "...", "output_path": "..." }`, OR if no dataset is provided, queries the local SQLite DB for the last N completed runs
- For each entry: reads the spec's `acceptance_criteria` list and Mason's primary output artifact, asks an LLM judge to score each criterion as met/partial/unmet
- Metrics: `completeness` (fraction of criteria met or partial), `precision` (fraction fully met)
- Env: `SPEC_ADHERENCE_DATASET`, `SPEC_ADHERENCE_LIMIT`, `SPEC_ADHERENCE_OUTPUT`, `SPEC_ADHERENCE_MIN_COMPLETENESS`
- Builtin name: `"spec_adherence"`
- Also supports a `without_scout` mode to measure what Scout's formalization step contributes

### 3-C — `src/scenario_delta.rs` — Hidden Scenario Delta benchmark

New builtin benchmark module — Harkonnen-native, no external dataset.

- Queries `coobie_episode_scores` in the local SQLite DB for runs where both `validation_passed` and `scenario_passed` are recorded
- Computes: `visible_pass_rate` (fraction where `validation_passed = 1`), `hidden_pass_rate` (fraction where `scenario_passed = 1`), `delta = visible_pass_rate - hidden_pass_rate`
- A large positive delta means Bramble passes things that Sable catches — proves the hidden scenario value
- Writes `scenario_delta_report.md` and `scenario_delta_summary.json` to artifact dir
- Builtin name: `"scenario_delta"`
- Env: `SCENARIO_DELTA_LIMIT` (max runs to include), `SCENARIO_DELTA_OUTPUT`

### 3-D — `src/twin_fidelity.rs` — Optional twin telemetry benchmark

- Keep `twin_fidelity_score` honest by counting only services whose status is `"running"`
- Retain a Harkonnen-native summary suite for historical comparison and future revisit
- **Not a Phase 3 blocker.** Live twin provisioning is deferred per Twin Policy above.

### 3-E — `suites.yaml` entries

- `harkonnen_spec_adherence` — Spec Adherence Rate (harkonnen-native, builtin: `spec_adherence`)
- `harkonnen_scenario_delta` — Hidden Scenario Delta (harkonnen-native, builtin: `scenario_delta`)
- `harkonnen_twin_fidelity` — Twin Fidelity telemetry (harkonnen-native, builtin: `twin_fidelity`)
- `harkonnen_devbench` — DevBench wrapper suite (script-based external adapter)

### 3-F — DevBench adapter wiring

- Add `scripts/benchmark-devbench.sh` following the same skip-and-delegate pattern as the existing SWE-bench and tau2 wrappers
- `DEVBENCH_COMMAND` supplies the exact local or hosted command that runs Harkonnen on DevBench
- Optional `DEVBENCH_ROOT` points at the benchmark checkout or adapter workspace
- The wrapper exits with skip code `10` when DevBench is not configured so Phase 3 can be wired before the full external harness is installed

### 3-G — Comparative Control-Style Benchmarking

- Add benchmark suites that compare three execution styles on the same tasks where practical: pure-LLM baseline, rule-heavy baseline, and Harkonnen's hybrid pack/control-plane path
- Publish not only task success but also recovery rate, guardrail violation rate, operator interruption count, and time-to-correctness
- Treat this as a factory benchmark, not a model-only benchmark: the question is how safely and efficiently the delivery system moves, not only how strong one model is in isolation

### 3-H — Adversarial Tool-Use And Stakeholder-Alignment Evaluation

- Add adversarial smokes that probe unsafe tool invocation, policy-bypass attempts, MCP misuse, and recovery behavior after intentionally hostile prompts or malformed tool outputs
- Add stakeholder-alignment reporting per run: did the plan respect recorded project purpose, operator stakes, stakeholder attitudes, prohibitions, and approved MCP posture?
- Include a report section that distinguishes technical correctness from project-posture correctness so Harkonnen can fail visibly when it solves the wrong problem the "right" way
- Publish baseline scores for both adversarial resilience and stakeholder-alignment adherence once enough runs exist

**Benchmark gate:**

- `spec_adherence` first run published — completeness and precision against local run corpus
- `scenario_delta` first run published — visible vs hidden pass rate gap across recent runs
- `DevBench` adapter wired (script-based, not builtin)
- comparative control-style benchmark suite wired
- adversarial tool-use smoke suite wired
- stakeholder-alignment reporting visible in run or benchmark artifacts

**Done when:** Flint produces a doc artifact per run, `spec_adherence` and `scenario_delta` have first-run baselines, the DevBench adapter is wired so local or hosted runs can be launched through the benchmark manifest, and the benchmark surface can distinguish pure correctness from governed, stakeholder-aligned correctness.

---

## Parallel Product Track — Calvin Archive Visualizer

**Why this is a prerequisite for Phase 8 debuggability:** The Calvin Archive is a six-chamber typed graph with revision history, quarantine ledger, causal links, and continuous D*/SSA signals. Without a visual surface, failures in integration governance are invisible — you cannot tell whether a quarantine is growing pathologically, whether a chamber is fragmenting, or whether D* drift is localised to one persona axis. The Pack Board's current flat list views cannot represent this structure. If you cannot see the archive, you cannot debug it.

**Reference approach:** [pascalorg/editor](https://github.com/pascalorg/editor) demonstrates the right architectural pattern: a React Three Fiber + Three.js + WebGPU stack rendering a navigable spatial graph where structural regions (levels, in their case) map to distinct visual zones. The six Calvin Archive chambers map directly to that model — each chamber is a navigable region, memory entries are nodes, causal links are edges, quarantine items are visually flagged, and the revision graph is a traversable history layer.

**What to build:**

- **Chamber map view** — six spatial zones (Mythos, Episteme, Ethos, Pathos, Logos, Praxis) rendered as distinct regions in a 3D canvas using React Three Fiber. Nodes within each chamber represent memory entries; edge thickness encodes confidence; quarantine items rendered with a distinct glyph and salience-decay color fade.
- **Causal link traversal** — click a node to expand its inbound/outbound causal links. Link labels show `PearlHierarchyLevel` (Associational / Interventional / Counterfactual). Paths that contributed to a quarantine are highlighted.
- **Revision history rail** — a time-axis rail alongside the chamber map showing integration events (accept / modify / reject / quarantine) as stamped markers. Scrubbing the rail replays chamber state at that point in time using snapshots from TypeDB.
- **Live D\* and SSA overlay** — Materialize SUBSCRIBE feed drives a real-time drift indicator per chamber. Chambers approaching the D\* bound shift color; an alert badge appears when the Meta-Governor fires.
- **Quarantine ledger panel** — side panel listing open quarantine items with pending evidence conditions, salience decay progress, and a one-click "resolve / promote / dismiss" action that calls the Meta-Governor API.
- **PackChat integration** — `@coobie what is in Ethos right now?` routes to a chamber query that highlights matching nodes in the visualizer. The visualizer and PackChat share a run context so Coobie's answers can be spatially anchored.

**Technology notes:**

- React Three Fiber + Three.js is the right rendering layer — WebGPU acceleration optional but worth targeting for large archives.
- Zustand for local visualizer state (selected node, active chamber, time cursor). The archive data itself comes from `GET /api/coobie/query` and the TypeDB query surface from Phase 6.
- The visualizer can be developed independently of the full Calvin Archive backend: stub the data layer with the existing SQLite memory entries and causal links (Phase 4) so the UI can be built and tested before Phase 8 ships.
- Ship as a new Pack Board tab ("Archive") rather than a standalone app — it shares the same auth surface and avoids a separate deployment.

**Dependency:** TypeDB query surface (Phase 6) required for chamber queries and revision history. D*/SSA live overlay requires Materialize (Phase 8). The stub-data path (SQLite causal links) can be used to develop the chamber map and traversal views before those phases land.

**Done when:** An operator can open the Archive tab, navigate the six chambers, click through causal links, scrub the revision history rail, see live D* drift per chamber, and resolve a quarantine item without opening a database client.

---

## Parallel Product Track — Operator Model Activation

**Unlocks:** Better commissioning, fewer mid-run clarification failures, and a reusable operator context layer that Scout, Coobie, and Keeper can all consume.

**Current state:** DB schema is complete (`operator_model_profiles`, `operator_model_sessions`, `operator_model_layer_checkpoints`, `operator_model_entries`, `operator_model_exports`, `operator_model_update_candidates` tables all exist). No interview logic, no layer progression, no artifact generation, no Scout/Coobie integration is wired. Phase v1-D ships the two-layer MVP. The full five-layer spec follows.

**Full five-layer spec (post-v1):**

- Native PackChat-based elicitation workflow with five fixed layers: operating rhythms, recurring decisions, dependencies, institutional knowledge, friction
- Approval checkpoints after each layer, reusing the existing checkpoint and unblock flow
- Artifact generation for `operating-model.json`, `USER.md`, `SOUL.md`, `HEARTBEAT.md`, `schedule-recommendations.json`, plus a Harkonnen-specific `commissioning-brief.json`
- Scout draft integration so spec generation can use an approved operator model as first-class context
- Coobie preflight integration so operator-model assumptions contribute to `required_checks`, guardrails, and escalation rules

**Current shipped slice:** project-first operator-model resolution now influences Scout draft generation and Coobie preflight guidance. The remaining product work is the checkpoint/export/review loop that turns the interview into durable stamped artifacts with operator approval.

- Review loop after runs: consolidation can propose operator-model updates, which the operator can keep/discard/edit before promotion
- Import/export compatibility with OB1-style operating artifacts, but no direct code dependency on OB1

**Benchmark / product gate:**

- Measurable drop in open checkpoints per run for projects using an approved operator model
- Spec draft quality and spec adherence compared with and without the operator model

**Done when:** A user can complete the five-layer interview with approvals, generate operating artifacts, and see those artifacts materially influence Scout draft quality and Coobie preflight behavior.

---

## Parallel Product Track — External Integrations

**Unlocks:** The factory becomes observable and controllable from outside the Pack Board. Without outbound notifications, every run outcome requires a human to poll the UI. Without inbound triggers, specs must be started manually. Without auth, the API is open to anyone on the network.

This is a usability prerequisite for any team or multi-machine deployment. Most items are small and independent; they do not need to ship as a block.

### EI-1 — API Authentication

**Why first:** The HTTP API is currently unauthenticated. Every other integration that touches the API needs auth to be safe.

- API key authentication middleware in `src/api.rs` — bearer token checked on all non-health routes
- `api_keys` table in SQLite: `(key_id, key_hash, label, created_at, last_used_at, revoked)`
- `POST /api/auth/keys` (create), `GET /api/auth/keys` (list), `DELETE /api/auth/keys/:id` (revoke)
- `GET /health` and the SSE stream remain unauthenticated (monitoring and browser clients)
- CLI flag `--api-key` or env var `HARKONNEN_API_KEY` for local development bypass

### EI-1b — MCP Authentication And Gateway Policy Parity

**Why next to API auth:** Harkonnen is increasingly MCP-first. API auth without MCP auth leaves the external control surface uneven and undermines Keeper's policy role.

- Authenticated MCP profiles for privileged servers: local-only trusted surfaces remain simple, but remote or high-impact MCP routes require explicit credentials or signed session context
- Gateway policy layer for MCP invocations: approval, deny, and audit outcomes should be symmetrical whether the request arrived over HTTP or MCP
- Policy-aware MCP metadata in setup TOML so machine-local surfaces can remain convenient while shared or remote surfaces become explicitly governed
- Audit trail for MCP decisions: server name, requested tool, approval outcome, actor, and timestamp

### EI-2 — Outbound Webhook Notifications

- `webhooks` table: `(webhook_id, url, secret, events: JSON array, created_at, enabled)`
- `POST /api/webhooks`, `GET /api/webhooks`, `DELETE /api/webhooks/:id`
- Events emitted: `run.started`, `run.completed`, `run.failed`, `checkpoint.created`, `checkpoint.resolved`, `metric_attack.detected`, `consolidation.ready`
- Payload: `{ event, run_id, spec_id, timestamp, summary, pack_board_url }`
- HMAC-SHA256 signature on the `X-Harkonnen-Signature` header (same pattern as GitHub webhooks)
- Retry with exponential backoff on 5xx or connection failure (up to 3 attempts)

### EI-3 — Slack Integration

**Outbound (Slack notifies operator):**

- Rich block-kit messages on `run.completed`: summary card with pass/fail, agent trace count, cost, decision count, link to Pack Board
- Checkpoint alert with inline Approve / Reject buttons that call back to the Harkonnen API
- `metric_attack.detected` alert: which metric was attacked, which exploit fired, suggested mitigation
- `run.failed` with Coobie's top causal diagnosis (from the latest `diagnose` output)

**Inbound (operator controls factory from Slack):**

- Slash command `/harkonnen run <spec-id>` — triggers a run, responds with run ID and Pack Board link
- `/harkonnen status <run-id>` — returns current phase and latest event
- `/harkonnen ask <question>` — routes to Coobie's `dispatch_message` as a PackChat message
- `/harkonnen checkpoint approve <id>` / `reject <id>` — resolves checkpoints without opening the browser

**Config:** Slack app credentials stored in setup TOML under `[integrations.slack]`. Webhook URL and bot token. No hardcoded values.

### EI-4 — Discord Integration

**Outbound:**

- Webhook embeds for `run.completed`, `checkpoint.created`, `run.failed` — same content as Slack but using Discord embed format (color-coded by outcome)
- Thread-per-run option: create a Discord thread for the run and post phase updates as the run progresses

**Inbound (bot commands in a designated channel):**

- `!run <spec-id>` — triggers run
- `!status <run-id>` — current phase and last event
- `!approve <checkpoint-id>` / `!reject <checkpoint-id>`
- `!ask <question>` — routes to Coobie

**Config:** `[integrations.discord]` in setup TOML. Bot token and guild/channel IDs.

### EI-5 — GitHub Integration

**Outbound:**

- After a run completes with Mason edits applied: optionally create a PR from the Mason branch using the GitHub API. PR body includes the spec title, run ID, decision log summary, Coobie critique outcome, and Pack Board link.
- Post a run summary as a PR comment when a run is triggered by a PR webhook (see inbound). Comment includes pass/fail, cost, and the top advisory concern from Coobie.

**Inbound:**

- `POST /api/integrations/github/webhook` receives GitHub webhook events
- On `push` to a configured branch: trigger a spec run for any spec whose `code_under_test` paths overlap the changed files
- On `pull_request.opened` or `pull_request.synchronize`: trigger the relevant spec run and post result as a PR comment
- Webhook secret verified via HMAC (same pattern as EI-2)

**Config:** `[integrations.github]` in setup TOML. Personal access token or GitHub App credentials. Repo and branch filter.

### EI-6 — Run Scheduling

- `scheduled_runs` table: `(schedule_id, spec_id, cron_expression, enabled, last_run_at, next_run_at)`
- `POST /api/schedules`, `GET /api/schedules`, `PUT /api/schedules/:id`, `DELETE /api/schedules/:id`
- Cron evaluator runs on a background tokio task; fires `POST /api/runs` when the schedule triggers
- Pack Board schedule manager panel: add/edit/disable schedules, see last run outcome

### EI-7 — Cost Budget Enforcement

- `max_cost_usd: Option<f64>` on `RunRequest` and in spec YAML
- After each LLM call, `get_run_cost_summary` checks accumulated cost against the budget. If exceeded: abort the current phase gracefully, write a `budget_exceeded` blocker to the blackboard, send a `run.failed` event with reason `budget_exceeded`
- `cost_hard_cap_usd` global config in setup TOML as a safety ceiling above any per-run budget
- Pack Board run overview shows budget consumed vs limit as a progress bar

### EI-8 — Health and Operational Endpoints

**Shipped (2026-04-20):**

- `GET /health` — probes DB (`SELECT 1`) and `memory/index.json`; returns `{ status, version, uptime_secs, db_ok, memory_index_ok }`. Responds `503` if DB probe fails. `AppContext.started_at` tracks server boot time.
- `GET /api/status` — returns `{ active_runs, agent_claim_count, memory_entry_count, last_benchmark_run }`. All queries fail-soft. Authentication deferred to EI-1.

**Remaining:**

- CORS configuration in setup TOML: `[server.cors]` with `allowed_origins` list, defaulting to `localhost` only
- Structured JSON logging option (for log aggregators): `[server.logging] format = "json"` in setup TOML
- Wire `GET /api/status` behind EI-1 auth (viewer role and above)

---

## Parallel Product Track — Hosted And Team Integrations

**Context:** Harkonnen should be usable beyond the Pack Board and local CLI.
This track formalizes the bridge from the local-first factory into external
control planes, workflow tools, shared knowledge systems, and chat surfaces
without hard-coding any one vendor or employer.

The architecture is: Harkonnen exposes itself **as an MCP server** first. That
lets Claude Desktop, Claude Code, VS Code, workflow tools, and any other
MCP-capable client consume factory operations through one protocol instead of
bespoke per-client integrations. EI-1 (auth) should land first because every
hosted or shared surface needs it.

### ENT-1 — Harkonnen as an MCP Server

**What to build:**

- `src/mcp_server.rs` — implements the MCP server protocol (JSON-RPC 2.0 over stdio or SSE transport). The MCP spec has a Rust SDK (`rmcp` crate); use that rather than writing the transport layer by hand.
- **Resources** (read-only, queryable by clients):
  - `harkonnen://runs` — list of recent runs with status
  - `harkonnen://runs/{run_id}` — full run detail including traces, decisions, optimization program, metric attacks
  - `harkonnen://memory/lessons` — promoted lessons from the consolidation workbench
  - `harkonnen://memory/causal` — recent causal patterns Coobie has identified
  - `harkonnen://specs` — available specs for commissioning
- **Tools** (callable actions):
  - `run_spec(spec_id, options)` — triggers a factory run, returns run_id
  - `get_run_status(run_id)` — current phase + latest event
  - `resolve_checkpoint(checkpoint_id, decision, note)` — approve or reject a checkpoint from any MCP client
  - `ask_coobie(question, context)` — routes to Coobie's `dispatch_message`, returns the response
  - `ingest_memory(content, source, tags)` — pushes a document or note into Coobie's memory ingest pipeline
  - `list_decisions(run_id)` — returns the decision audit log for a run
- **Prompts** (parameterized prompt templates for external clients):
  - `briefing_for_spec(spec_id)` — pre-built Coobie briefing prompt
  - `diagnose_run(run_id)` — causal diagnosis prompt for a completed run
- MCP server transport registered in setup TOML under `[mcp.self]`:

```toml
[mcp.self]
enabled = true
transport = "sse"          # "stdio" for Claude Desktop / VS Code; "sse" for hosted clients
port = 3001                # separate port from the main HTTP API
auth_required = true       # reuses EI-1 API key
```

- CLI command `harkonnen mcp serve` starts the MCP server as a standalone process alongside the main server

**Done when:** Claude Desktop, Claude Code, or VS Code can list factory runs, trigger a run, and ask Coobie a question via MCP tool calls.

### ENT-2 — External Connector Surface

- `factory/connectors/harkonnen-openapi.json` — OpenAPI 3.0 spec covering the key Harkonnen endpoints
- `factory/connectors/manifest.yaml` — connector manifest with display names, descriptions, and action categories
- `factory/connectors/workflow-templates/` — starter workflow templates: `run-spec.yaml`, `ask-coobie.yaml`, `checkpoint-review.yaml`
- Authentication: OAuth2 client credentials flow using the generic OIDC path from ENT-3
- Documentation at `factory/connectors/README.md`

**Done when:** An external workflow client can trigger a Harkonnen run, ask Coobie a question, and approve a checkpoint without touching the Pack Board.

### ENT-3 — OIDC Authentication

- OAuth2/OIDC JWT validation middleware in `src/api.rs` — alongside the existing API key path
- `[auth.oidc]` section in setup TOML with `issuer`, `client_id`, `audience`
- JWT validation: fetch JWKS from the configured issuer, validate signature, `aud`, `iss`, and expiry
- Role claims: `Harkonnen.Operator` (full access), `Harkonnen.Viewer` (read-only), `Harkonnen.Agent` (service principal)
- `GET /api/auth/me` — returns the authenticated identity and resolved role

**Done when:** An external connector authenticating as an OIDC service principal can call the Harkonnen API without an API key, and a viewer-role principal cannot trigger runs or approve checkpoints.

### ENT-4 — Knowledge Base Ingest

- `src/integrations/knowledge.rs` — generic knowledge-source client layer with provider adapters
- CLI: `harkonnen memory ingest --source docs --collection <id>`, `--source wiki --space <id>`, `--source search --query "<terms>"`
- Incremental sync state table so repeated runs fetch only changed or added documents
- `[integrations.knowledge]` section in setup TOML

**Done when:** Running a knowledge-source ingest adds shared documents into Coobie's retrievable memory, and subsequent runs against related specs can cite those documents in the briefing.

### ENT-5 — ChatOps Integration

- Rich-card messages on run events (completed, failed, checkpoint, metric attack) to a configured chat surface
- Checkpoint actionable card — operator clicks Approve/Reject directly in chat
- Bot commands: `@Harkonnen run`, `@Harkonnen status`, `@Harkonnen ask`, `@Harkonnen checkpoints`
- `[integrations.chatops]` in setup TOML with generic webhook + bot credential fields

**Done when:** A completed run posts a rich message to the configured chat surface, a checkpoint can be approved from chat, and `@Harkonnen ask` routes to Coobie.

### ENT-6 — Clone-Local Profile And Hosted Deployment Hardening

- Keep generated machine profiles under `setups/machines/` and out of Git by default
- Add optional `[auth.oidc]`, `[integrations.chatops]`, `[integrations.knowledge]`, and `[mcp.self]` blocks to generated profiles when the setup interview selects them
- `cargo run -- setup check` extended to validate selected integrations and MCP self-server startup for the active local profile

**Done when:** Running `cargo run -- setup check` on a locally generated profile reports green for the selected integrations, and a second machine can be provisioned from the same public templates without inheriting private state.

---

## Benchmark Track (cross-phase)

Benchmarks advance in lockstep with implementation phases. Each phase ships with at least one measurable gate.

### Phase-aligned milestones summary

| Phase | Key benchmarks unlocked |
| --- | --- |
| v1 | Decision audit completeness, memory supersession accuracy (StreamingQA), WrongAnswer classification rate |
| Phase 2 | SWE-bench Verified readiness, LiveCodeBench, Aider Polyglot |
| Phase 5-C | Briefing scope log per run (correctness verification, not a scored benchmark) |
| Phase 5b | FRAMES re-run (Qdrant), LongMemEval / LoCoMo regression check |
| Phase 6 | GAIA Level 3, AgentBench, TypeDB vs SQL causal recall comparison |
| Phase 7 | E-CARE, causal attribution accuracy (top-1 / top-3) |
| Phase 8 | D* drift score, SSA baseline, quarantine resolution quality, schema revision stability |
| Phase 3 | Spec adherence rate, hidden scenario delta, DevBench |

### Always-on benchmarks

- `Local Regression Gate` — hard merge gate, runs on every substantial change
- `LongMemEval` paired mode (Coobie vs raw LLM) — run on every memory-relevant change
- `LoCoMo QA` paired mode — longer-horizon memory regression check

### Competitive positioning benchmarks

#### vs Mem0 / MindPalace / Zep

- `FRAMES` — multi-hop factual recall; Mem0 publishes here. Native adapter live. Requires Phase 5b Qdrant for best results.
- `StreamingQA` — belief-update accuracy; no competitor tracks this. Phase v1-B.
- `HELMET` — retrieval precision/recall. Native adapter live.
- `LongMemEval` — long-term assistant memory. Native adapter live.
- `LoCoMo QA` — long-horizon dialogue memory. Native adapter live.

#### vs OpenCode / Aider / single-agent coding tools

- `LiveCodeBench` — recent competitive programming problems; contamination-resistant. Phase 2.
- `Aider Polyglot` — Aider's own multi-language leaderboard. Phase 2.
- `DevBench` — full software lifecycle; structural argument against single-phase tools. Phase 3.
- `SWE-bench Verified` / `SWE-bench Pro` — industry-standard code loop benchmarks. Phase 2.

#### vs general agent frameworks

- `GAIA Level 3` — multi-step delegation; single-agent tools fail here. Phase 6.
- `AgentBench` — eight environments; tests Labrador role separation. Phase 6.

#### Causal reasoning — unique claim, no competitor benchmarks this

- `CLADDER` — Pearl hierarchy accuracy. Native adapter live.
- `E-CARE` — causal explanation coherence. Phase 7.

#### Harkonnen-native — cannot be run by any competitor

- `Spec Adherence Rate` — completeness and precision vs spec. Phase 3.
- `Hidden Scenario Delta` — visible vs hidden pass rate gap. Phase 3.
- `Causal Attribution Accuracy` — seeded failure corpus, top-1 / top-3. Phase 7.

### Reporting standard

Every reportable benchmark claim should include:

- the raw-LLM baseline on the same provider when meaningful
- the Harkonnen setup name and routing
- the benchmark split or slice used
- the commit hash and benchmark artifact path
- latency and cost where available, not just accuracy

---

## What is already done (do not redo)

**Gap-closure phases A–D (shipped 2026-04-18):**

- **A1** — `LlmUsage` struct; token + latency capture on all three providers; `run_cost_events` table; `GET /api/runs/:id/cost`
- **A2** — `DecisionRecord` struct; `decision_log` table; `record_decision` + `list_run_decisions`; `GET /api/runs/:id/decisions`; wired at plan critique and consolidation promotion
- **A3** — `Assignment` + `ClaimRequest` extended with `resource_kind`, `ttl_secs`, `guardrails`, `expires_at`; `POST /api/coordination/check-lease` handler with TTL expiry and guardrail pattern matching
- **B** — `AgentTrace` struct; `agent_traces` table + index; `record_agent_trace` + `list_run_traces`; `extract_reasoning()` parses `<reasoning>` blocks; wired at Scout intake, Coobie briefing, Coobie critique, Mason plan, Mason edits, Sable; `GET /api/runs/:id/traces`
- **C** — `OptimizationProgram` struct; `scout_derive_optimization_program` (LLM-backed, stub fallback); written to `optimization_program.json`; Coobie critique flags when plan doesn't address objective metric; `GET /api/runs/:id/optimization-program`
- **D** — `MetricAttack` struct; `sable_generate_metric_attacks` (2–3 attacks per run, exploit + detection signals + mitigations); written to `metric_attacks.json`; `GET /api/runs/:id/metric-attacks`

---

**Phase 1 — Core Factory + PackChat + Coobie Memory + Benchmark Toolchain:**

- Spec loading, validation, run lifecycle, SQLite persistence
- Phase-level attribution recording
- LLM routing for Claude, Gemini, OpenAI, and OpenAI-compatible local endpoints
- Scout, Mason, Piper, Sable, Ash, Flint LLM calls
- Mason opt-in file writes with staged workspace isolation
- Piper real build execution with stdout/stderr streaming
- Mason fix loop (up to 3 iterations on build failure)
- Live event broadcast (`LiveEvent`) + SSE endpoint `/api/runs/:id/events/stream`
- Coobie causal reasoning Phase 1 (heuristic rules, episode scoring)
- Coobie causal streaks and cross-run pattern detection
- Coobie Phase 3 preflight guidance (spec-scoped cause history → required checks)
- Coobie Palace (`src/coobie_palace.rs`) — den-based compound recall, patrol, scents
- Semantic memory (fastembed or OpenAI-compatible embeddings + SQLite vector store, hybrid retrieval)
- Causal feedback loop (causal reports + Sable rationale written back to project memory)
- Keeper coordination API (claims, heartbeats, conflict detection, release)
- Pack Board React UI (PackChat panel, Attribution Board, Factory Floor, Memory Board)
- PackChat backend: `chat_threads`, `chat_messages`, multi-turn `dispatch_message`, `@mention` routing, Coobie default fallback
- PackChat API routes: `GET/POST /api/chat/threads`, `GET /api/chat/threads/:id`, `GET/POST /api/chat/threads/:id/messages`, `POST /api/agents/:id/chat`
- Checkpoint/reply/unblock routes as PackChat control-plane backend
- Evidence bootstrap, annotation bundle validation, evidence promotion
- `harkonnen memory init` with pre-embedding on fresh clone
- First-class benchmark toolchain (`benchmark list/run/report`, manifest-driven suites, CI workflow)
- Native LongMemEval adapter + paired raw-LLM vs Harkonnen comparison mode
- Native LoCoMo QA adapter + paired raw-LLM vs Harkonnen comparison mode
- Native FRAMES adapter + paired raw-LLM vs Harkonnen comparison mode
- Native StreamingQA adapter (query-time invalidation reasons; persistence layer completing in v1-B)
- LM Studio / OpenAI-compatible benchmark routing for chat and embedding backends

**Phase 4 — Episodic Layer Enrichment + Causal Graph + Benchmarks:**

- `state_before` / `state_after` on `EpisodeRecord` and episodes table (workspace state snapshots via FNV-64 hash walk)
- `causal_links` table: `(link_id, run_id, from_event_id, to_event_id, relation, confidence, hierarchy_level, key, created_at)`
- `PearlHierarchyLevel` enum (Associational / Interventional / Counterfactual) on causal links
- `populate_cross_phase_causal_links` — auto-emits phase_sequence and failure_triggered links across run episodes
- `get_run_causal_graph` — returns event graph with Pearl-labeled edges; surfaced via `GET /api/runs/:id/causal-events`
- Coobie multi-hop retrieval: `retrieve_context_multihop(query, embedding_store, depth)` — configurable chain depth (1–3)
- Native CLADDER adapter — Pearl hierarchy causal benchmark, paired Harkonnen vs raw-LLM mode
- Native HELMET adapter — retrieval precision/recall benchmark

**Phase 4b — Memory Invalidation (query-time layer, shipped; persistence layer completing in v1-B):**

- `invalidation_reasons` field on `MemoryRetrievalHit` — computed at retrieval time from `superseded_by` / `challenged_by` provenance fields
- `memory_invalidation_reasons()` helper in orchestrator surfaces reasons per hit
- Persistence layer (`memory_updates` table, `invalidated_by` column, `GET /api/memory/updates`, Memory Board panel, StreamingQA adapter) completing in v1-B above

**Phase 5 — Consolidation Workbench:**

- `consolidation_candidates` table: `(candidate_id, run_id, kind, status, content_json, edited_json, confidence, label, created_at, reviewed_at)`
- `generate_consolidation_candidates`, `list_consolidation_candidates`, `review_consolidation_candidate`, `edit_consolidation_candidate`, `promote_kept_candidates`
- INSERT OR IGNORE idempotency on candidate generation
- API routes: `GET /api/runs/:id/consolidation/candidates`, `POST .../candidates` (generate), `POST .../candidates/:id/keep`, `.../discard`, `.../edit`, `POST /api/runs/:id/consolidate` (promote)
- Pack Board Consolidation Workbench panel: candidate cards with keep/discard/edit controls, confidence bars, expandable JSON, filter bar, promote footer
- `RunDetailDrawer` updated with workbench tab

---

## Tracking

Each active implementation phase gets its own git branch: `phase/v1-guardrails`, `phase/2-bramble-tests`, `phase/5c-briefing-scope`, etc.
A phase is merged to `main` when its "Done when" condition is verifiably met.
This file is updated when a phase ships — move it from the numbered list above into the "already done" section.

Benchmark wiring should advance in lockstep with implementation:

- when a phase ships, add or tighten at least one benchmark gate tied to it
- when a public benchmark is still adapter-only, capture that explicitly here rather than implying it is fully integrated
- benchmark artifacts belong in `factory/artifacts/benchmarks/` and should be linked from release notes once they support a public claim
