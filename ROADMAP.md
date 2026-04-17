# Harkonnen Labs — Execution Roadmap

**This is the canonical build order from 2026-04-17 forward.**
Phases 1, 4, 4b, and 5 are shipped. Phases 2 and 3 remain the active numbered build targets.
New implementation work starts at Phase 2 unless an explicit doc-sync, operator-model slice, or polish task says otherwise.

---

## Why this order

The factory has a complete foundation: core pipeline, PackChat control plane, layered Coobie memory, causal graph, Pearl hierarchy labeling, multi-hop retrieval, memory invalidation, operator-reviewed consolidation Workbench, and a manifest-driven benchmark toolchain with several native adapters. The remaining gaps are concrete: Bramble's validation score is still a stub, Sable's twin is a manifest not a running system, long-term memory infrastructure is still SQLite-first, and TypeDB plus real causaloids are still ahead. Every phase below unblocks something downstream.

Benchmarking remains a parallel track. Each phase ships with at least one measurable gate.

---

## Phase 2 — Bramble Real Test Execution

**Unlocks:** Coobie's `validation_passed` score becomes meaningful.
`TEST_BLIND_SPOT` and `PACK_BREAKDOWN` causal signals currently score against stubs.

**What to build:**

- `bramble_run_tests` in orchestrator — reads `spec.test_commands` (same detection logic as Piper) and executes them in the staged workspace
- Stdout/stderr streamed as `LiveEvent::BuildOutput` on the broadcast channel (already exists — Bramble just needs to use it)
- `ValidationSummary` populated from real exit codes and parsed test output, not from scenario results or stubs
- Bramble's phase attribution records `validation_passed: true/false` from actual runs
- Feed result back as `test_coverage_score` into the Coobie episode at ingest time
- **Mason online-judge feedback loop** — parse stdout diff output from competitive programming judges (expected vs actual output) as a first-class failure signal distinct from compiler errors. Feeds into Mason's fix loop as a new `FailureKind::WrongAnswer` variant.
- **LiveCodeBench adapter** — wrapper command that pulls recent problems, runs Mason/Piper, and emits pass/fail per problem into the benchmark runner.
- **Aider Polyglot adapter** — maps Aider's multi-language benchmark format to Harkonnen specs; no structural changes needed.

**Benchmark gate:**

- `local_regression` stays green on every merge
- the code loop should be runnable through the emerging `SWE-bench Verified` adapter, even if scores are unpublished
- `LiveCodeBench` adapter wired and producing artifacts
- `Aider Polyglot` adapter wired for a direct open-source comparison line

**Done when:** A spec with `test_commands` shows real pass/fail in the run report, Coobie's episode scores reflect actual test execution, and Mason's fix loop handles wrong-answer failures.

---

## Phase 3 — Ash Real Twin Provisioning

**Unlocks:** Sable's scenario evaluation becomes grounded.
Right now Sable judges against a twin that is a JSON manifest, not running infrastructure.

**What to build:**

- Ash generates a `docker-compose.yml` in the run workspace from the twin manifest — one service stub per declared external dependency
- `ash_provision_twin` spawns the compose stack before Sable runs, tears it down after
- Network address and port bindings written to `twin_env.json` so Mason/Piper can reference them
- `twin_fidelity_score` derived from which declared dependencies actually had running stubs
- Failure injection: Ash can set env vars on stubs to simulate auth expiry, rate limits, or connection refusal per scenario config
- **Flint documentation phase** — Flint produces a documentation artifact (README, API reference, or inline doc comments) as a first-class phase output. Required for DevBench. Flint reads the spec and Mason's implementation artifacts, then generates docs under `artifacts/docs/`.
- **DevBench adapter** — maps Harkonnen's full run to DevBench's evaluation format. Each Labrador phase maps to a DevBench lifecycle stage.
- **Spec Adherence Rate benchmark** — LLM-as-judge grader that extracts requirements from the spec and scores completeness and precision. Run with and without Scout's formalization step.
- **Hidden Scenario Delta benchmark** — tracks `visible_test_pass_rate` versus `hidden_scenario_pass_rate` across a corpus and surfaces the gap. Requires Phase 2 real test results.

**Benchmark gate:**

- repo-native `twin fidelity` benchmark suite
- repo-native `hidden scenario integrity` benchmark
- `hidden scenario delta` first run published
- `spec adherence rate` first run published
- `DevBench` adapter wired, even if early scores are unpublished

**Done when:** A spec with a twin declaration actually starts Docker containers, Sable's hidden scenarios run against live stubs, Flint produces a doc artifact, and the spec adherence and hidden scenario delta benchmarks have baseline runs.

---

## Phase 4b — Memory Invalidation and Fact-Update Tracking (Shipped)

**Unlocks:** StreamingQA, and the structural claim that Coobie correctly *updates* beliefs rather than just *recalls* them. This work was scoped during the Phase 4 episodic enrichment design and landed as the dedicated 4b follow-on slice. Keep it here as the benchmark and maintenance reference point.

**What to build:**

- `invalidated_by` field on memory records — when new information contradicts an existing fact, the old record is marked invalidated rather than silently overwritten
- `memory_updates` table in SQLite: `(update_id, old_memory_id, new_memory_id, reason, created_at)` — tracks when a stored fact is superseded
- Coobie ingest pipeline updated: before writing a new memory entry, check for semantic near-duplicates with conflicting claims; if found, write the supersession record and flag the old entry
- `GET /api/memory/updates` — surfaces invalidation history to the Pack Board
- Memory Board UI panel: show invalidated entries distinctly from current entries; allow operator to confirm or reject a supersession
- **StreamingQA native adapter** — streams fact-update events to Coobie's memory, then queries whether the updated belief is correctly recalled. Scores belief-update accuracy separately from static recall. This is the primary benchmark for this phase.

**Benchmark gate:**

- `StreamingQA` first run published — belief-update accuracy, no competitor publishes this
- re-run `LongMemEval` to confirm invalidation tracking does not regress static recall

**Done when:** Ingesting a new fact that contradicts an older one marks the old fact as invalidated, the operator can review the supersession, and StreamingQA has a baseline score.

---

## Phase 5b — Memory Infrastructure (Qdrant + OCR)

**Unlocks:** Semantic recall at scale and document ingest completeness. These are COOBIE.md Phase C and Phase B gaps that have been deferred because the SQLite vector store is sufficient for current run volume, but they become the bottleneck as the memory corpus grows.

**What to build:**

- **Qdrant integration** — add `src/coobie/qdrant.rs` implementing the semantic index over extracted text and memory summaries. Payload metadata: `org`, `role`, `product`, `spec_id`, `run_id`, `agent`, `memory_type`, `tags`, `created_at`. Qdrant replaces the SQLite vector store for long-term semantic memory (keep SQLite as the short-term and episodic store). Bootstrap script at `scripts/bootstrap-coobie-memory-stack.sh` already exists.
- **OCR pipeline** — add Tesseract-backed OCR for scanned PDFs and images. Current extractors handle text-forward formats but cannot read scanned documents. Wire through the existing `memory ingest` path: detect image-only PDFs, invoke `tesseract`, write extracted text sidecar alongside the imported asset.
- **Memory module refactor** — split the growing `src/memory.rs` into the module tree described in COOBIE_SPEC: `src/memory/mod.rs`, `working.rs`, `episodic.rs`, `semantic.rs`, `causal.rs`, `consolidation.rs`, `blackboard.rs`, `retrieval.rs`, `extraction.rs`. No behavior change; this is a maintainability gate before the codebase grows further.

**Benchmark gate:**

- re-run `FRAMES` after Qdrant lands to confirm multi-hop recall improves over the SQLite vector baseline
- `LongMemEval` and `LoCoMo` re-run to confirm semantic recall quality does not regress

**Done when:** Qdrant is serving semantic queries for long-term memory, OCR-scanned PDFs can be ingested, and `src/memory.rs` is split into the COOBIE_SPEC module tree.

---

## Phase 6 — TypeDB Semantic Layer (Layer C)

**Unlocks:** Typed causal queries that vector similarity cannot answer. "Find all runs where TWIN_GAP caused a failure that was fixed by an intervention that held for ≥ 3 runs" requires a graph, not a similarity score.

TypeDB 3.x changes the implementation assumptions: the old JVM burden objection is gone because TypeDB's core is now Rust. It is still an external service with real operational cost, so it stays later in the sequence and should not replace SQLite as the hot path.

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

**Unlocks:** The strongest publishable internal benchmark claims. The causal attribution corpus and E-CARE adapter are both spec'd in Phase 5 but can be built incrementally and do not depend on TypeDB.

**What to build:**

- **Causal attribution accuracy corpus** — 30–50 labeled runs with seeded failures (wrong API version, missing env var, breaking schema change, etc.). Each entry has a spec, a seeded failure, a ground-truth cause label, and the Coobie `diagnose` output. Score top-1 and top-3 accuracy. Start with 10 entries for a first baseline. Lives in `factory/benchmarks/causal-attribution/`.
- **E-CARE native adapter** — maps Coobie's `diagnose` output to E-CARE's evaluation format and scores whether generated causal explanations are judged natural-language coherent. Run after consolidation so promoted lessons can inform subsequent diagnose output.
- Publish before/after comparisons for causal attribution accuracy: pre-Phase 4 (pure semantic recall) versus post-Phase 4 (causal graph-augmented).

**Benchmark gate:**

- `E-CARE` first run published — causal explanation coherence score
- `causal attribution accuracy` first run published — top-1 / top-3 vs semantic-only baseline

**Done when:** The corpus has at least 30 labeled entries, the causal attribution accuracy benchmark has a published run, and E-CARE has a published score.

---

## Parallel Product Track — Operator Model Activation

**Unlocks:** Better commissioning, fewer mid-run clarification failures, and a reusable operator context layer that Scout, Coobie, and Keeper can all consume.

This is not a replacement for Phase 2 or Phase 3. It is a control-plane/product track that fills the upstream gap between "I installed the factory" and "I can describe my work clearly enough to commission it."

**What to build:**

- Native PackChat-based elicitation workflow with five fixed layers: operating rhythms, recurring decisions, dependencies, institutional knowledge, friction
- Approval checkpoints after each layer, reusing the existing checkpoint and unblock flow
- Structured SQLite store for operator-model profiles, sessions, approved layer checkpoints, canonical entries, exports, and update candidates
- Artifact generation for `operating-model.json`, `USER.md`, `SOUL.md`, `HEARTBEAT.md`, `schedule-recommendations.json`, plus a Harkonnen-specific `commissioning-brief.json`
- Scout draft integration so spec generation can use an approved operator model as first-class context
- Coobie preflight integration so operator-model assumptions contribute to `required_checks`, guardrails, and escalation rules
- Review loop after runs: consolidation can propose operator-model updates, which the operator can keep/discard/edit before promotion
- Import/export compatibility with OB1-style operating artifacts, but no direct code dependency on OB1

**Benchmark / product gate:**

- reduced operator clarification burden during commissioning and run execution
- measurable drop in open checkpoints per run for projects using an approved operator model
- spec draft quality and spec adherence compared with and without the operator model

**Done when:** A user can choose `Interview Me First` in the current commissioning flow, complete the five-layer interview with approvals, generate operating artifacts, and see those artifacts materially influence Scout draft quality and Coobie preflight behavior.

## Benchmark Track (cross-phase)

Benchmarks should advance in lockstep with implementation phases. When a phase ships, at least one benchmark gate tied to it should ship too.

### Phase-aligned milestones summary

| Phase | Key benchmarks unlocked |
| --- | --- |
| Phase 2 | SWE-bench Verified readiness, LiveCodeBench, Aider Polyglot |
| Phase 3 | twin fidelity, hidden scenario delta, spec adherence rate, DevBench |
| Phase 4b | StreamingQA belief-update accuracy |
| Phase 5b | FRAMES re-run (Qdrant), LongMemEval / LoCoMo regression check |
| Phase 6 | GAIA Level 3, AgentBench |
| Phase 7 | E-CARE, causal attribution accuracy |

### Always-on benchmarks

- `Local Regression Gate` — hard merge gate, runs on every substantial change
- `LongMemEval` paired mode (Coobie vs raw LLM) — run on every memory-relevant change
- `LoCoMo QA` paired mode — longer-horizon memory regression check

### Competitive positioning benchmarks

#### vs Mem0 / MindPalace / Zep

- `FRAMES` — multi-hop factual recall; Mem0 publishes here. Native adapter live. Requires Phase 5b Qdrant for best results.
- `StreamingQA` — belief-update accuracy; no competitor tracks this. Phase 4b.
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
- Native StreamingQA adapter + memory invalidation / fact-update tracking for superseded beliefs
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

**Phase 5 — Consolidation Workbench:**

- `consolidation_candidates` table: `(candidate_id, run_id, kind, status, content_json, edited_json, confidence, label, created_at, reviewed_at)`
- `generate_consolidation_candidates`, `list_consolidation_candidates`, `review_consolidation_candidate`, `edit_consolidation_candidate`, `promote_kept_candidates`
- INSERT OR IGNORE idempotency on candidate generation
- API routes: `GET /api/runs/:id/consolidation/candidates`, `POST .../candidates` (generate), `POST .../candidates/:id/keep`, `.../discard`, `.../edit`, `POST /api/runs/:id/consolidate` (promote)
- Pack Board Consolidation Workbench panel: candidate cards with keep/discard/edit controls, confidence bars, expandable JSON, filter bar, promote footer
- `RunDetailDrawer` updated with workbench tab

---

## Tracking

Each active implementation phase gets its own git branch: `phase/2-bramble-tests`, `phase/3-ash-twins`, etc.
A phase is merged to `main` when its "Done when" condition is verifiably met.
This file is updated when a phase ships — move it from the numbered list above into the "already done" section.

Benchmark wiring should advance in lockstep with implementation:

- when a phase ships, add or tighten at least one benchmark gate tied to it
- when a public benchmark is still adapter-only, capture that explicitly here rather than implying it is fully integrated
- benchmark artifacts belong in `factory/artifacts/benchmarks/` and should be linked from release notes once they support a public claim
