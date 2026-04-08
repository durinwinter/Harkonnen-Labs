# Harkonnen Labs — Execution Roadmap

**This is the canonical build order from 2026-04-08 forward.**
Phase 1 backend is shipped. New implementation work starts at Phase 2 unless an
explicit doc-sync or polish task says otherwise.

---

## Why this order

The factory has excellent bones and real memory/causal intelligence. The first
conversational control-plane backend is now in. The remaining gap is that several
downstream phases still depend on stubbed validation, stubbed twins, and
under-modeled episodic/semantic memory. Every phase below unblocks something
downstream — skip one and the next phase is hollow.

Benchmarking is now a parallel execution track, not a postscript. Each roadmap
phase should land with at least one measurable benchmark or regression gate so we
can separate architectural progress from benchmark regressions and publish what
Harkonnen adds over the raw provider.

---

## Benchmark Track

These benchmarks should be wired in alongside the build phases rather than after
Phase 6. The point is to make each phase measurable as it ships.

### Immediate benchmark baseline work

- `Local Regression Gate` runs on every substantial change and remains the hard
  merge gate for the repo.
- `LongMemEval` should be run in paired mode: raw LLM baseline versus Harkonnen
  PackChat/Coobie on the same provider routing and dataset slice.
- The first publishable benchmark target is `longmemeval_s_cleaned.json` with a
  fixed sampled slice for iteration and the full split for reportable runs.

### Phase-aligned benchmark milestones

- `Phase 2` maps to `SWE-bench Verified` readiness for Mason/Piper/Bramble plus
  stronger local regression on build and visible test execution. Also the entry
  point for `LiveCodeBench` and `Aider Polyglot` as contamination-free coding comparisons.
- `Phase 3` maps to repo-native `twin fidelity`, `hidden scenario integrity`, and
  `spec adherence rate` benchmarks because off-the-shelf suites do not measure
  stub realism, scenario black-box integrity, or spec fidelity at all.
  Also the entry point for `DevBench` once Flint produces documentation artifacts.
- `Phase 4` maps to `LongMemEval` and then `LoCoMo QA`, because richer episodic
  memory should improve PackChat/Coobie over the direct baseline.
  Also the entry point for `FRAMES` (vs Mem0), `StreamingQA` (fact-update tracking),
  `HELMET` (retrieval precision), and `CLADDER` (causal hierarchy) — each of which
  requires the enriched episodic layer to be meaningful.
- `Phase 5` maps to promotion-quality and memory-review benchmarks: how often the
  Workbench keeps, edits, or rejects candidates and whether approved lessons help
  future runs. Also the entry point for `E-CARE` (causal explanation quality)
  and the `causal attribution accuracy` internal benchmark.
- `Phase 6` maps to cross-run causal query benchmarks and graph-backed recall,
  plus more ambitious public comparisons once TypeDB-backed semantic recall ships.
  Also the entry point for `GAIA Level 3` and `AgentBench` as multi-agent
  coordination claims.
- `PackChat overall` should be measured on `tau2-bench` once the chat/backend and
  unblock/control-plane flows are stable enough to expose tool trajectories.

### Competitive positioning benchmarks

These benchmarks support direct comparison claims against specific alternatives.

#### vs Mem0 / MindPalace / Zep — memory systems

- `FRAMES` (Google DeepMind) — multi-hop factual recall across long documents. Mem0
  publishes scores here. Coobie's hybrid retrieval needs to beat single-pass vector
  recall, and the multi-hop cases are where the causal + semantic layer combination
  should win. Requires the multi-hop retrieval chain feature (see Phase 4).
- `StreamingQA` — tests whether a memory system correctly *updates* beliefs when
  facts change over time, not just whether it recalls them. Coobie's stale-memory
  model is a structural advantage; no vector-only competitor has explicit
  fact-update tracking. Requires the memory invalidation feature (see Phase 4).
- `HELMET` — holistic precision/recall on long-context retrieval tasks. Validates
  whether Coobie's compound Palace patrol reduces retrieval noise compared to flat
  vector similarity. Runnable after Phase 4.

#### vs OpenCode / Aider / single-agent coding tools

- `LiveCodeBench` — recent competitive programming problems from Codeforces,
  LeetCode, and AtCoder that postdate training cutoffs. Fairer than HumanEval
  because problems are genuinely new. Mason's iterative fix loop plus Piper's real
  build execution should outperform single-pass code generation.
  Requires Mason online-judge feedback loop (see Phase 2).
- `Aider Polyglot` — Aider's own multi-language benchmark with a public leaderboard.
  Running Mason/Piper through the same harness gives a clean apples-to-apples
  comparison against one of the most credible open-source coding agents.
  Runnable after Phase 2.
- `DevBench` — full software development lifecycle: requirements → design →
  implementation → testing → documentation. SWE-bench measures one phase; DevBench
  measures the whole pipeline. This is the structural argument against single-agent
  tools. Requires Flint documentation artifacts (see Phase 3).

#### vs general agent frameworks

- `GAIA Level 3` — multi-step tool use and planning where single-agent tools fail
  because they cannot delegate. Expensive to run but independently verifiable.
  The hardest Level 3 tasks map to Harkonnen's Scout → Mason → Piper → Sable chain.
  Runnable after Phase 6.
- `AgentBench` — eight environments (OS, DB, web, etc.) testing specialist
  coordination versus a single generalist. Maps directly to the Labrador role
  separation claim. Runnable after Phase 6.

#### Causal reasoning — unique claim, no competitor benchmarks this

- `CLADDER` — Pearl's causal hierarchy: associational ("what correlates"),
  interventional ("what happens if we do X"), and counterfactual ("what would have
  happened"). Maps directly to Coobie's Layer D design. This is the benchmark no
  memory or agent competitor runs. Requires Pearl hierarchy structuring in
  Coobie's `diagnose` output (see Phase 4).
- `E-CARE` (Explainable Causal Reasoning) — tests whether causal explanations are
  natural-language coherent, not just structurally correct. Validates the quality
  of Coobie's `diagnose` output as human-readable rationale. Runnable after Phase 5.

### Harkonnen-native internal benchmarks

These do not exist as off-the-shelf suites because no other system is built this way.
They are the most differentiating publishable claims because competitors cannot run them.

- `Spec Adherence Rate` — given a spec: completeness (did it implement all stated
  requirements?) and precision (did it add things *not* in the spec?). Compare
  Harkonnen with and without Scout's formalization step to isolate the spec-first
  contribution. Target: Phase 3.
- `Hidden Scenario Delta` — the gap between visible test pass rate and hidden
  scenario pass rate across a corpus of runs. A large delta proves Sable catches
  failures that Bramble's tests miss. Requires real test execution from Phase 2.
  Target: Phase 3.
- `Causal Attribution Accuracy` — a labeled corpus of seeded failures; scores whether
  Coobie's `diagnose` correctly ranks the true cause in top-1 or top-3. The most
  direct test of whether causal memory adds value over semantic recall alone.
  Target: Phase 5.

### Reporting standard

Every reportable benchmark claim should include:

- the raw-LLM baseline on the same provider when that baseline is meaningful
- the Harkonnen setup name and routing
- the benchmark split or slice used
- the commit hash and benchmark artifact path
- latency and cost where available, not just accuracy

## Phase 2 — Bramble Real Test Execution

**Unlocks:** Coobie's `validation_passed` score becomes meaningful.
`TEST_BLIND_SPOT` and `PACK_BREAKDOWN` causal signals currently score against stubs.

**What to build:**

- `bramble_run_tests` method in orchestrator — reads `spec.test_commands` (same
  detection logic as Piper's build phase) and executes them in the staged workspace
- Stdout/stderr streamed as `LiveEvent::BuildOutput` on the broadcast channel
  (already exists — Bramble just needs to use it)
- `ValidationSummary` populated from real exit codes and parsed test output,
  not from scenario results or stubs
- Bramble's phase attribution records `validation_passed: true/false` from actual runs
- Feed result back as `test_coverage_score` into the Coobie episode at ingest time
- **Mason online-judge feedback loop** — parse stdout diff output from competitive
  programming judges (expected vs actual output) as a first-class failure signal
  distinct from compiler errors. Required for LiveCodeBench and Aider Polyglot.
  Feeds into Mason's fix loop as a new `FailureKind::WrongAnswer` variant so
  Mason generates output-correction attempts rather than build-fix attempts.
- **LiveCodeBench adapter** — wrapper command that pulls recent problems, runs
  Mason/Piper against them, and emits pass/fail per problem into the benchmark runner.
  Problems should be fetched fresh to avoid contamination from cached datasets.
- **Aider Polyglot adapter** — runs Mason/Piper through Aider's published multi-language
  benchmark harness. Requires no structural changes; the adapter is a thin shell
  script that maps Aider's input format to Harkonnen specs.

**Benchmark gate for this phase:**

- `local_regression` must stay green on every merge
- the code loop should be runnable through the emerging `SWE-bench Verified`
  adapter, even if early scores are still unpublished
- `LiveCodeBench` adapter wired and producing artifacts, even if scores are unpublished
- `Aider Polyglot` adapter wired for a direct open-source comparison line
- benchmark artifacts should record build/test latency, not just pass/fail

**Done when:** A spec with `test_commands` shows real pass/fail in the run report,
Coobie's episode scores reflect actual test execution, and Mason's fix loop handles
wrong-answer failures from online-judge-style tests.

---

## Phase 3 — Ash Real Twin Provisioning

**Unlocks:** Sable's scenario evaluation becomes grounded.
Right now Sable judges against a twin that is a JSON manifest, not running infrastructure.

**What to build:**

- Ash generates a `docker-compose.yml` (or equivalent) in the run workspace from the
  twin manifest — one service stub per declared external dependency
- `ash_provision_twin` spawns the compose stack before Sable runs, tears it down after
- Network address and port bindings written to `twin_env.json` so Mason/Piper can
  reference them in build/test commands
- `twin_fidelity_score` in Coobie's episode scoring derived from which declared
  dependencies actually had running stubs (not just declared in manifest)
- Failure injection: Ash can set environment variables on stubs to simulate
  auth expiry, rate limits, or connection refusal per scenario config
- **Flint documentation phase** — Flint produces a documentation artifact (README,
  API reference, or inline doc comments) as a first-class phase output, not an
  afterthought. Required for DevBench, which scores the full lifecycle including docs.
  Flint reads the spec and Mason's implementation artifacts, then generates docs in
  the run workspace under `artifacts/docs/`.
- **DevBench adapter** — maps Harkonnen's full run (Scout → Mason → Piper → Bramble
  → Flint) to DevBench's evaluation format. DevBench scores design, implementation,
  testing, and documentation separately; each maps to a Labrador phase.
- **Spec Adherence Rate benchmark** — LLM-as-judge grader that extracts requirements
  from the spec and scores the output on completeness (all requirements implemented?)
  and precision (nothing added beyond the spec?). Run with and without Scout's
  formalization step to isolate the spec-first contribution.
- **Hidden Scenario Delta benchmark** — tracks `visible_test_pass_rate` versus
  `hidden_scenario_pass_rate` across a corpus of runs and surfaces the gap. Requires
  Bramble real test results (Phase 2) to be stored alongside Sable results in a
  comparable format. The delta is the proof that Sable catches what Bramble misses.

**Benchmark gate for this phase:**

- repo-native `twin fidelity` benchmark suite scoring whether declared dependencies
  become reachable running stubs
- repo-native `hidden scenario integrity` benchmark measuring whether scenarios
  remain truly black-box relative to Mason/Bramble
- `hidden scenario delta` first run published — visible vs hidden pass rate gap
- `spec adherence rate` first run published — completeness and precision scores
- `DevBench` adapter wired, even if early scores are unpublished

**Done when:** A spec with a twin declaration actually starts Docker containers,
Sable's hidden scenarios run against live stubs, Flint produces a doc artifact,
and the spec adherence and hidden scenario delta benchmarks have baseline runs.

---

## Phase 4 — Episodic Layer Enrichment

**Unlocks:** Layer D (causal graph) can be a real graph, not a flat hypothesis list.
The current episode record is missing the fields needed for causal link candidates.
This phase also unlocks the FRAMES, StreamingQA, HELMET, and CLADDER benchmarks,
which each require the enriched episodic layer to produce meaningful results.

**What to build:**

- Add to `EpisodeRecord` / `run_events` schema:
  - `state_before: Option<serde_json::Value>` — snapshot of relevant state before action
  - `state_after: Option<serde_json::Value>` — snapshot after
  - `candidate_causal_links: Vec<String>` — event IDs that may have caused this one
- Populate `candidate_causal_links` during `record_event` using temporal proximity
  and phase co-occurrence (simple heuristic first, graph traversal later)
- Add `causal_link` table to SQLite:
  `(from_event_id, to_event_id, relation, confidence, created_at)`
  Relations: `caused`, `contributed_to`, `prevented`, `preceded`, `invalidated`,
  `depended_on`, `corrected`, `escalated` (per COOBIE_SPEC Layer D spec)
- Coobie's `diagnose` reads the causal link table in addition to episode scores
- DeepCausality Phase 2: use real causaloids built from the link table, not just
  per-run scoring signals
- **Coobie multi-hop retrieval chain** — current hybrid retrieval is single-pass
  (vector similarity + keyword). Add a chaining step where a retrieved fact can
  trigger a second retrieval using its content as the query. Required for FRAMES,
  where the benchmark specifically tests multi-hop factual recall that flat vector
  search cannot resolve. Implement as a configurable `retrieval_depth` param
  (default 1 = current behavior, 2 = one chain step).
- **Memory invalidation / fact-update tracking** — add an `invalidated_by` field to
  memory records and a `memory_updates` table tracking when a stored fact is
  superseded. When Coobie ingests new information that contradicts an existing
  memory, the old record is marked invalidated rather than silently overwritten.
  Required for StreamingQA, which directly tests whether belief updates are correct.
- **Coobie `diagnose` Pearl hierarchy structuring** — restructure `diagnose` output
  to classify each causal hypothesis by Pearl's hierarchy level:
  `associational` (correlation observed), `interventional` (action taken and result
  recorded), or `counterfactual` (what would have happened). Required for CLADDER.
  The classification uses the `causal_link.relation` field — `caused` /
  `contributed_to` map to interventional; `preceded` maps to associational;
  `prevented` / `invalidated` map to counterfactual.
- **FRAMES adapter** — wrapper that loads FRAMES evaluation sets, routes questions
  through Coobie's retrieval chain (including multi-hop), and scores factual accuracy.
  Run in paired mode: Coobie vs raw-LLM baseline, same provider.
- **StreamingQA adapter** — streams fact-update events to Coobie's memory, then
  queries whether the updated belief is correctly recalled. Scores belief-update
  accuracy as a separate metric from static recall.
- **HELMET adapter** — runs Coobie retrieval against HELMET's long-context tasks
  and measures precision/recall on retrieved passages.
- **CLADDER adapter** — maps CLADDER's causal questions to Coobie's Pearl-structured
  diagnose output and scores associational, interventional, and counterfactual
  accuracy separately.

**Benchmark gate for this phase:**

- paired `LongMemEval` runs repeated here to verify richer episodes improve Coobie
  over the direct baseline
- `LoCoMo QA` adapter wired — longer-horizon memory should start outperforming raw
  transcript prompting at this layer
- `FRAMES` first run published — multi-hop retrieval accuracy vs Mem0 comparison
- `StreamingQA` first run published — belief-update accuracy, no competitor has this
- `CLADDER` first run published — Pearl hierarchy accuracy, unique claim
- `HELMET` adapter wired and producing retrieval precision/recall artifacts

**Done when:** After a run you can query `GET /api/runs/:id/causal-events` and see
a graph of what caused what; Coobie's diagnose output labels each hypothesis by
Pearl hierarchy level; and FRAMES, StreamingQA, and CLADDER have baseline scores.

---

## Phase 5 — Post-Run Consolidation Workbench

**Unlocks:** Intentional memory. Right now Coobie auto-promotes everything;
the operator review loop the architecture describes does not exist.

**What to build:**

- `GET /api/runs/:id/consolidation/candidates` — surface what Coobie proposes to
  promote: new lessons, causal links, pattern extractions, with confidence scores
- `POST /api/runs/:id/consolidation/candidates/:id/keep` — operator approves
- `POST /api/runs/:id/consolidation/candidates/:id/discard` — operator rejects
- `POST /api/runs/:id/consolidation/candidates/:id/edit` — operator edits before
  promoting (changes the memory content inline)
- `POST /api/runs/:id/consolidate` — runs only after review; promotes approved
  candidates into `factory/memory/` and re-indexes
- Pack Board Workbench panel: card per candidate with approve/discard/edit controls
- **E-CARE adapter** — maps Coobie's `diagnose` output to E-CARE's evaluation format
  and scores whether the generated causal explanations are judged coherent by the
  official evaluator. Run after consolidation so promoted lessons can inform
  subsequent diagnose output and the improvement is measurable.
- **Causal attribution accuracy corpus** — a curated set of 30–50 labeled runs with
  intentionally seeded failures (wrong API version, missing env var, breaking schema
  change, etc.). Each failure has a ground-truth cause label. Scores whether
  Coobie's `diagnose` top-1 and top-3 match the ground truth. Build and maintain
  this corpus in `factory/benchmarks/causal-attribution/`. This is the most direct
  test of whether causal memory adds value over semantic recall alone, and cannot
  be reproduced by any competitor.

**Benchmark gate for this phase:**

- consolidation-quality benchmark tracking keep/edit/discard decisions and whether
  approved lessons later improve run outcomes
- `E-CARE` first run published — causal explanation coherence score
- `causal attribution accuracy` first run published against seeded failure corpus —
  top-1 and top-3 accuracy; compare pre- and post-Phase 4 causal graph
- publish before/after comparisons for memory promotion quality, not just UI status

**Done when:** After a run, you sit in the Workbench, review what Coobie wants to
remember, make changes, and commit. Nothing enters durable memory without your
approval. The causal attribution corpus has baseline scores, and E-CARE has a
published run.

---

## Phase 6 — TypeDB Semantic Layer (Layer C)

**Unlocks:** Typed causal queries that vector similarity cannot answer.
"Find all runs where TWIN_GAP caused a failure that was fixed by an intervention
that held for ≥ 3 runs" — this requires a graph, not a similarity score.

TypeDB 3.x changes the implementation assumptions here: the old "JVM burden"
objection is no longer a reason to avoid the layer, because TypeDB's core moved
to Rust. It is still an external database service with real operational cost, so
it stays later in the sequence and should not replace SQLite as the hot path.

**What to build:**

- TypeDB 3.x instance/service configured in the home-linux setup TOML
- `src/coobie/semantic.rs` implementing the `SemanticMemory` trait from COOBIE_SPEC
- Rust-facing TypeDB adapter using the official TypeDB 3.x driver surface behind
  the `SemanticMemory` abstraction
- TypeDB schema from COOBIE_SPEC: entities (agent, goal, episode, observation, action,
  outcome, artifact, lesson, failure-mode, causal-link), relations as specified
- TypeDB 3.x function-backed semantic reasoning where inference is needed; do not
  design this layer around legacy "rules engine" assumptions
- Write-back: after Phase 5 consolidation approval, promoted lessons and causal links
  are written to TypeDB as well as the file store
- Query surface: `POST /api/coobie/query` routes natural-language causal questions
  through Coobie's retrieval chain: working → blackboard → typed lessons → semantic
  recall → causal lookup
- Coobie's briefing builder can call TypeDB for cross-run pattern queries before
  preflight, replacing the current SQL aggregate approach for complex patterns

**What to also build for competitive benchmark readiness:**

- **GAIA Level 3 adapter** — maps GAIA's multi-step tool-use tasks to Harkonnen's
  factory run format. The hardest Level 3 tasks require delegation that single-agent
  tools cannot do; the adapter routes sub-tasks to the appropriate Labrador rather
  than a single generalist. Requires the TypeDB query surface to be live so Coobie
  can answer cross-run context questions mid-task.
- **AgentBench adapters** — environment adapters for AgentBench's OS, database, and
  web environments. Each environment maps to a Labrador role (OS → Mason/Piper,
  DB → Ash, web → Flint). Adapters translate AgentBench's action/observation loop
  into Harkonnen's phase event stream.

**Benchmark gate for this phase:**

- cross-run causal-query benchmarks comparing SQL aggregate recall versus TypeDB-backed
  semantic recall on the same questions
- `GAIA Level 3` first run published — multi-step delegation accuracy
- `AgentBench` first runs published across OS, DB, and web environments
- only make stronger public memory/causal claims once this layer beats the raw and
  pre-TypeDB baselines on fixed evaluation prompts

**Done when:** You can ask Coobie "what caused the last three failures on this spec"
and get an answer sourced from a typed graph; GAIA Level 3 and AgentBench adapters
are wired and producing artifacts.

---

## What is already done (do not redo)

- PackChat backend persistence in SQLite: `chat_threads` and `chat_messages`
- `src/chat.rs` ChatStore plus multi-turn `dispatch_message` routing using
  conversation history, `@mentions`, and Coobie default fallback
- PackChat API routes:
  `GET/POST /api/chat/threads`,
  `GET /api/chat/threads/:id`,
  `GET/POST /api/chat/threads/:id/messages`,
  `POST /api/agents/:id/chat`
- Existing checkpoint/reply/unblock routes now documented as part of the
  PackChat control-plane backend:
  `GET /api/runs/:id/checkpoints`,
  `POST /api/runs/:id/checkpoints/:checkpoint_id/reply`,
  `POST /api/agents/:id/unblock`
- Spec loading, validation, run lifecycle, SQLite persistence
- Phase-level attribution recording
- LLM routing for Claude, Gemini, OpenAI
- Scout, Mason, Piper, Sable, Ash, Flint LLM calls
- Mason opt-in file writes with staged workspace isolation
- Piper real build execution with stdout/stderr streaming (Phase 1 execution layer)
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
- Evidence bootstrap, annotation bundle validation, evidence promotion
- `harkonnen memory init` with pre-embedding on fresh clone
- First-class benchmark toolchain (`benchmark list/run/report`, manifest-driven suites, CI workflow)
- Native LongMemEval adapter plus paired raw-LLM versus Harkonnen comparison mode
- Native LoCoMo QA adapter plus paired raw-LLM versus Harkonnen comparison mode
- LM Studio/OpenAI-compatible benchmark routing for both chat and embedding backends

---

## Tracking

Each active implementation phase gets its own git branch:
`phase/2-bramble-tests`, `phase/3-ash-twins`, etc.
A phase is merged to `main` when its "Done when" condition is verifiably met.
This file is updated when a phase ships — move it from the numbered list above
into the "already done" section.

Benchmark wiring should advance in lockstep with implementation:

- when a phase ships, add or tighten at least one benchmark gate tied to it
- when a public benchmark is still adapter-only, capture that explicitly here
  rather than implying it is already fully integrated
- benchmark artifacts belong in `factory/artifacts/benchmarks/` and should be
  linked from release notes or README once they support a public claim
