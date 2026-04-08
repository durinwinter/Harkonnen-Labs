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
  stronger local regression on build and visible test execution.
- `Phase 3` maps to repo-native `twin fidelity` and `hidden scenario integrity`
  benchmarks because off-the-shelf suites do not measure stub realism well.
- `Phase 4` maps to `LongMemEval` and then `LoCoMo QA`, because richer episodic
  memory should improve PackChat/Coobie over the direct baseline.
- `Phase 5` maps to promotion-quality and memory-review benchmarks: how often the
  Workbench keeps, edits, or rejects candidates and whether approved lessons help
  future runs.
- `Phase 6` maps to cross-run causal query benchmarks and graph-backed recall,
  plus more ambitious public comparisons once TypeDB-backed semantic recall ships.
- `PackChat overall` should be measured on `tau2-bench` once the chat/backend and
  unblock/control-plane flows are stable enough to expose tool trajectories.

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

**Benchmark gate for this phase:**

- `local_regression` must stay green on every merge
- the code loop should be runnable through the emerging `SWE-bench Verified`
  adapter, even if early scores are still unpublished
- benchmark artifacts should record build/test latency, not just pass/fail

**Done when:** A spec with `test_commands` shows real pass/fail in the run report,
and Coobie's episode scores reflect actual test execution.

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

**Benchmark gate for this phase:**

- add a repo-native `twin fidelity` benchmark suite that scores whether declared
  dependencies become reachable running stubs
- add a repo-native `hidden scenario integrity` benchmark so we can measure whether
  scenarios are still truly black-box relative to Mason/Bramble

**Done when:** A spec with a twin declaration actually starts Docker containers
and Sable's hidden scenarios run against live stubs.

---

## Phase 4 — Episodic Layer Enrichment

**Unlocks:** Layer D (causal graph) can be a real graph, not a flat hypothesis list.
The current episode record is missing the fields needed for causal link candidates.

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

**Benchmark gate for this phase:**

- paired `LongMemEval` runs should be repeated here to test whether richer episodes
  improve Coobie over the direct baseline
- wire `LoCoMo QA` next, because this phase is where longer-horizon memory should
  start to outperform raw transcript prompting

**Done when:** After a run you can query `GET /api/runs/:id/causal-events` and see
a graph of what caused what, not just a ranked list of hypotheses.

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

**Benchmark gate for this phase:**

- add a consolidation-quality benchmark that tracks keep/edit/discard decisions
  and whether approved lessons later improve run outcomes
- publish before/after comparisons for memory promotion quality, not just UI status

**Done when:** After a run, you sit in the Workbench, review what Coobie wants to
remember, make changes, and commit. Nothing enters durable memory without your approval.

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

**Benchmark gate for this phase:**

- add cross-run causal-query benchmarks that compare SQL aggregate recall versus
  TypeDB-backed semantic recall on the same questions
- only make stronger public memory/causal claims once this layer beats the raw and
  pre-TypeDB baselines on fixed evaluation prompts

**Done when:** You can ask Coobie "what caused the last three failures on this spec"
and get an answer sourced from a typed graph, not keyword matches or flat SQL counts.

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
