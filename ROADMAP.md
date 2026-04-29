# Harkonnen Labs — Execution Roadmap

**Primary goal: structural coordination and trustworthy run governance on the hot path.**
The fastest reasonable path: v1-A (Keeper-backed lease enforcement) → v1-B
(memory invalidation persistence) → v1-D (operator context MVP) → Phase 2
(testable harness) → Phase 5-C (context gating, no new infra) → Phase 5-D
(PackChat conversation memory chain) → Phase 5b (OB1 memory abstraction,
MCP prompts, OCR, memory refactor) → Phase 6 (TypeDB) → Phase 7 (causal
corpus) → Phase 8 (Calvin Archive).
Phase 10 (docs, DevBench, benchmark suites) follows the coordination path rather
than interrupting it. Live twin provisioning is permanently deferred unless a
product explicitly requires running service virtualization. Phase 2's real test
execution IS the testable harness. The Calvin Archive is now a working sidecar
continuity layer, but it is not the current critical-path blocker.

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
| Enforced authority and guardrail boundaries | **Partial** — pre-write lease denial exists in orchestrator, but Mason still needs a Keeper-backed claim/check/release lifecycle and write-path enforcement must depend on an active lease rather than advisory state |
| Live world-state modeling | Deferred — twin is still a manifest; live provisioning is permanently deferred unless a product needs it |
| Closed-loop outcome verification | Partial — observation endpoint deferred to Phase E (TypeDB dependency) |
| Structural multi-agent coordination | Mostly closed — blackboard, heartbeat, claim eviction, DB-backed lease mirrors, and PackChat-linked dog runtime rosters are real; remaining work is richer inter-dog patch/brief exchange and conflict synthesis |
| Economic and cost awareness | Closed — A1 trace spine + cost events |
| Explicit intent → plan → execution separation | Closed — B, C (OptimizationProgram) |
| External system interfaces | Open — Phase v1 External Integrations track |

### How this roadmap closes that gap

- `.harkonnen/gap-closure-progress.md` tracks strategic bridge work phases A–D (all shipped)
- Phase v1 (below) is the structural gate before the factory can be called Tier 4
- After v1, the roadmap drives through grounded execution and scoped context before deeper memory infrastructure
- Phase 10 benchmarks and docs follow the coordination path
- Operator Model and External Integrations are parallel product tracks

---

## Twin Policy

**Bramble's real test execution (Phase 2) is the testable harness.** There is no mandatory dependency on Docker-backed service virtualization anywhere in this roadmap. "Digital twin" in this system means the manifest-based twin fidelity score used for diagnostic telemetry — it does not mean a running containerized replica of the target service. `twin_fidelity_score` remains available as optional telemetry. Phase 10-D exists only as a maintenance note for that signal; it is not a Phase 10 completion gate.

If a specific product built on Harkonnen requires live service virtualization for its own testing needs, that capability can be revisited then with that product's requirements as the driver. It does not belong in the core factory sequence.

---

## Why this order

The factory needs a clear line from coordination authority to durable continuity. That line runs through:

1. **v1-A** — Keeper-backed lease enforcement. Coordination must be structural, not advisory. No write without an active lease.
2. **v1-B** — memory invalidation persistence. Superseded coordination facts are almost as dangerous as missing ones.
3. **v1-D** — operator context MVP. Runs should stop starting from scratch when operator posture is already known.
4. **Phase 2** — Bramble real test execution. This is the testable harness. `validation_passed` means nothing until it reflects real test output rather than stubs.
5. **Phase 5-C** — per-phase context gating. Once roles and leases are real, irrelevant context becomes the next quality drag.
6. **Phase 5-D** — PackChat conversation memory chain. Twilight Bark conversations become durable memory candidates, Coobie distills them, OB1 stores shared recall, and Calvin receives only governed promotion contracts.
7. **Phase 5b** — OB1 memory abstraction, OCR, MCP prompts, and memory refactor. This prepares the semantic layer without returning to the fragile local vector-store default.
8. **Phase 6** — TypeDB semantic layer for cross-run typed causal queries.
9. **Phase 7** — causal attribution corpus so the deeper continuity layer opens with real evidence.
10. **Phase 8** — the Calvin Archive: persisted identity, governed integration, D*/SSA streaming. This remains the long-horizon destination, but no longer blocks coordination-first engineering.
11. **Phase 10** — documentation, DevBench, benchmark suites. Important for external claims and usability but not on the current critical path.

Parallel tracks (Compiled State Synthesis, External Integrations, Operator Model, Hosted/Team, Calvin Archive Visualizer) advance independently of the above sequence and do not block it.

### Synthesis Stance

Harkonnen needs a first-class synthesis function, but not necessarily a tenth Labrador yet. For now, synthesis is treated as a pipeline phase that compiles accepted state into durable operator-readable artifacts using inputs from Coobie, Keeper, Flint, the decision log, the coordination registry, and the operator model. If this work later develops its own trust boundary, benchmark surface, or sustained bottleneck, it can be promoted into a dedicated Labrador with a narrow role. Until then, add synthesis as an explicit phase and artifact family rather than a new generalist agent.

Benchmark wiring advances in lockstep with implementation phases. Each phase ships with at least one measurable gate. The benchmark philosophy remains explicitly agentic-engineering shaped: measure how quickly and safely software moves through the delivery system, not just how quickly code is emitted.

---

## Phase v1 — Tier 4 Finalization

**This is the active build target.** Closes the remaining structural gaps that prevent Harkonnen from being called a genuine Tier 4 agentic workflow.

### v1-A — Guardrail Enforcement (hard blocker for Tier 4)

**Why it's a blocker:** Tier 4 requires agents to operate *inside* explicit guardrails, not just record them. Harkonnen now has a Keeper-backed workspace lease claim/check/release lifecycle with DB-backed lease mirrors, PackChat-linked dog runtime rosters, decision-log coverage for lease and planning outcomes, and Pack Board decision-log surfacing in the run detail drawer.

**What to build:**

- Keeper-backed workspace lease lifecycle is live: Mason claims `resource_kind: "workspace"` before implementation, writes depend on an active lease, and release happens at run completion or failure.
- The write-path guardrail check in `mason_generate_and_apply_edits` now depends on an active Mason workspace lease rather than treating missing coordination state as an automatic allow.
- Policy events are mirrored into SQLite, and decision records now cover Keeper lease outcomes plus the key planning choices (Scout optimization program, Mason plan selection, Sable metric attacks) so the audit trail reflects authority decisions rather than only blackboard intent.
- A canonical dog runtime registry now sits alongside leases: one identity per Labrador role, with support for multiple live worker instances (`mason#1`, `mason#codex`, `mason#claude`, etc.) carrying `thread_id`, ownership, and status through the same coordination surface.
- PackChat run threads now act as the shared conversation surface for those live dog instances so two Masons can coordinate as Mason rather than as disconnected provider personas.
- This slice is now effectively shipped on the current backend and Pack Board path; follow-on work moves from basic guardrail authority into broader coordination synthesis and operator ergonomics.

**Done when:** Mason claims a Keeper-backed workspace lease before implementation begins, a Mason edit attempt against a path that has no active workspace lease is blocked at the orchestrator level, lease and planning outcomes are written into the decision log, and the Pack Board surfaces the decision log per run.

---

### v1-B — Memory Invalidation Persistence (Phase 4b completion)

**Why:** The core persistence path is now live on the main ingest flow, and the benchmark-facing smoke has now been rerun against that stored history. The active close-out work is the operator adjudication loop for supersession events. Broader benchmark enrichment is intentionally deferred until after the current narrow end-to-end Harkonnen pass.

**Shipped on the current path:**

- `memory_updates` table in `src/db.rs`: `(update_id, old_memory_id, new_memory_id, reason, created_at)`
- `invalidated_by: Option<String>` on the memory record schema (references `update_id`)
- Coobie ingest pipeline: before writing a new memory entry, check for semantic near-duplicates with conflicting claims via cosine similarity on the embedding store. If found above threshold, write a supersession record and set `invalidated_by` on the old entry.
- `GET /api/memory/updates` endpoint returning supersession history
- Memory Board UI panel showing supersession history alongside the rest of Coobie's recalled state

**Remaining close-out:**

- No additional v1-B blockers after the operator confirm/reject loop lands; defer wider benchmark report polish and cross-suite metric expansion until after the narrow full-system pass.

**Status:** Core path verified on the current code line. A repeated ingest from the same source path now writes `memory_updates` rows, marks stale notes with `superseded_by`, surfaces the history through `GET /api/memory/updates` and the Pack Board Memory panel, and supports operator confirm/reject review from the Memory Board. The bundled StreamingQA smoke fixture has also been rerun under `lm-studio-local`, producing `1.0000` accuracy, exact match, evidence hit rate, and updated-fact accuracy while persisting the benchmark-local supersession row.

---

### v1-C — FailureKind Classification

**Why:** Mason's fix loop should not handle all failures identically. A wrong-answer failure (test ran, output was wrong) requires a different fix prompt than a compile error (code never ran).

**Shipped on the current path:**

- `FailureKind` enum in `src/models.rs`: `CompileError`, `TestFailure`, `WrongAnswer`, `Timeout`, `Unknown`
- Validation summary construction classifies stdout/stderr-style details from visible checks, including compile/build errors, generic test failures, wrong-answer diffs, and timeouts.
- `WrongAnswer` triggers a distinct Mason validation-fix prompt that asks Mason to study the expected/actual diff and fix implementation logic without modifying tests.
- `failure_kind` is persisted on `ValidationSummary`, included in validation summaries, and recalculated after validation harness mutations so Coobie can pattern-match on failure type in causal records.

**Done when:** A run with a wrong-answer test failure shows `failure_kind: WrongAnswer` in the run summary and Mason's fix attempt uses the diff-focused prompt.

**Status:** Shipped and covered by focused classifier tests. Keep broader benchmark expansion deferred until the narrow full-system pass is complete.

---

### v1-D — Operator Model Minimum Viable

**Why:** Scout's intent generation and Coobie's preflight have no connection to operator context. Without this, every new spec starts from scratch regardless of how well Coobie knows the operator's patterns.

**Shipped on the current path (two-layer MVP, not the full five-layer spec):**

- PackChat `interview` command: initiates a two-layer intake (operating rhythms + recurring decisions) with checkpoint approval after each layer
- `commissioning-brief.json` artifact generated from the approved layers: contains operator's primary work patterns, preferred tools, recurring decisions, and risk tolerances
- Scout draft integration: when a `commissioning-brief.json` exists for the operator, Scout includes its top-3 patterns in the intent package prompt
- Coobie preflight integration: operator's stated risk tolerances contribute to `required_checks` and guardrail text
- `operator_model_sessions`, `operator_model_layer_checkpoints`, and `operator_model_exports` are now exercised by the approval/export path; completed sessions stamp the project under `.harkonnen/operator-model/`

**Done when:** An operator who has completed the two-layer interview sees their stated patterns reflected in Scout's intent packages and Coobie's required checks on subsequent runs.

**Status:** MVP shipped and hardened. The Pack Board operator-model flow can now approve the active layer, advance the session, generate the commissioning brief, persist export metadata, and surface preferred-tool / risk-tolerance signals into Coobie preflight. Full five-layer interview and post-run update review remain in the parallel Operator Model product track.

---

### v1-E — Transactional Execution And Approval Boundaries

**Why:** Guardrails are stronger when high-impact actions have explicit transaction boundaries rather than relying on best-effort cleanup after a mistake. If a run is about to mutate sensitive code, open a privileged MCP surface, or cross a policy threshold, Harkonnen should be able to pause, request approval, and either commit or roll back from a known boundary. This is the operational analogue of the Soul-of-AI requirement that continuity and policy remain inspectable rather than implicit.

**What to build:**

- Transaction envelope for high-impact phases: capture an explicit pre-action snapshot, planned mutation set, approval state, and rollback note before execution proceeds. **Shipped for implementation-phase Mason LLM edits** via `transaction_implementation.json`, `transaction_implementation.md`, and a run-local `transaction_backups/implementation_pre_action` restore point.
- Human-interrupt checkpoint for guarded transitions: if Keeper or Coobie flags a privileged step, the run pauses at a reversible boundary rather than drifting forward and apologizing later. **Shipped:** Coobie implementation blockers now create a `transaction_approval_required` checkpoint before Mason edits are applied.
- Operator checkpoint resolution: **shipped for implementation transactions.** Resolving the checkpoint with approve rehydrates `spec.yaml`, `target_source.json`, `intent.json`, `coobie_briefing.json`, and `implementation_plan.md`, applies the Mason edit lane to the staged workspace, finalizes the transaction artifact, resumes Bramble visible validation, then continues through Sable hidden scenarios, Flint artifacts, and Coobie causal reporting when the tool boundary is approved. Reject aborts without mutation. Revise records operator guidance and leaves the run in a revision-requested state.
- Rollback execution and artifact written per guarded transition: what was attempted, what state changed, what was restored, and what residual risk remains. **Shipped:** rollback restores the staged `product/` workspace from the transaction backup, verifies it against the pre-action snapshot, and records `rolled_back` or `rolled_back_with_drift`.
- Privileged MCP/tool transaction envelope: **shipped at the tool-surface boundary.** The tools phase now writes `tool_transaction.json` and `tool_transaction.md`, classifies configured MCP servers and relevant host commands, auto-approves read-only/local surfaces, opens `tool_transaction_approval_required` when write, network, secret-bearing, or external-process surfaces are present, and resumes hidden-scenario/artifact continuation after operator approval when visible validation is already complete.
- Invocation-level gateway: **shipped for host-command execution inside the run loop.** Build and validation commands now write `tool_invocations.json` and `tool_invocations.md`, classify each actual invocation at execution time, auto-approve common local build/test commands, and require an approved tool transaction before higher-risk external-process invocations proceed.
- Decision-log integration: approval, commit, rollback, and abort outcomes become explicit decision records rather than only phase logs. **Shipped:** implementation transaction boundary, operator approval/reject/revise/rollback, transaction commit, transaction rollback, tool transaction boundary, and tool approval/reject/revise outcomes are recorded in the run decision log.
- Remaining work: extend the same invocation-level gateway to proxied third-party MCP calls if Harkonnen becomes the runtime broker for external MCP traffic rather than only recording/enforcing host-command invocations inside the run loop.

**Done when:** A guarded run can pause before a privileged transition, record an approval or rejection, and either commit or roll back from a named boundary with an auditable artifact.

**Status:** Implementation transaction approval, visible-validation continuation, hidden-scenario/artifact/causal-report continuation, rollback execution, privileged tool-surface transaction envelopes, and invocation-level host-command gateway enforcement are shipped. Harkonnen now opens auditable boundaries around Mason LLM edits and records/enforces actual build/validation tool invocations inside the run loop.

---

### v1 Benchmark / product gate

- Decision audit log surfaced in Pack Board per run
- Memory supersession events returned by `GET /api/memory/updates`
- StreamingQA first run published — belief-update accuracy
- At least one run showing `failure_kind: WrongAnswer` in the validation summary
- At least one run where Scout's intent package references operator model context

---

## Phase 2 — Bramble Real Test Execution

**This is the testable harness.** Until this ships, `validation_passed` reflects scenario results and stubs rather than real test output. Every downstream quality signal (Coobie's `test_coverage_score`, Phase 10 benchmarks, the fix loop's wrong-answer path) depends on real exit codes coming from real test commands.

**What to build:**

- `bramble_run_tests` in orchestrator — reads `spec.test_commands` (same detection logic as Piper) and executes them in the staged workspace
  Shipped: explicit Bramble test harness now runs raw `spec.test_commands` through shell-preserving execution, records `real_test_commands` / `passed_real_test_commands` in `ValidationSummary`, and writes corpus results from those runs.
- Stdout/stderr streamed as `LiveEvent::BuildOutput` on the broadcast channel (already exists — Bramble just needs to use it)
- `ValidationSummary` populated from real exit codes and parsed test output, not from scenario results or stubs
- Bramble's phase attribution records `validation_passed: true/false` from actual runs
- Feed result back as `test_coverage_score` into the Coobie episode at ingest time
  Shipped: Coobie now prefers explicit real-test counts over generic scored-check counts when `spec.test_commands` were present, and run reports show explicit test-command totals.
- **Mason online-judge feedback loop** — `FailureKind::WrongAnswer` (wired in v1-C) now carries structured wrong-answer evidence from Bramble's explicit test-command harness into `validation.json`, the run report, and Mason's diff-focused repair prompt. The loop also records `validation_repair_attempts.{json,md}`, classifies each retry as `resolved / improved / stalled / regressed`, feeds that guidance into the next Mason attempt, and stops early after repeated non-improving retries.
- **LiveCodeBench adapter** — native builtin now wired through the benchmark manifest and report path; generates per-problem artifacts plus suite-level pass@1 breakdowns in benchmark reports.
- **Benchmark posture** — keep `LiveCodeBench` as the single active external coding canary while the narrow end-to-end Harkonnen pass matures. Additional public coding benchmarks stay adapter-ready but are not a near-term build gate unless they answer a question the current canary cannot.
- **Aider Polyglot adapter** — remains adapter-ready for a later comparison lane once the core run path is stable enough to justify broader external measurement.

**Benchmark gate:**

- `local_regression` stays green on every merge
- `LiveCodeBench` remains wired and producing artifacts as the active external coding canary
- additional coding benchmark expansion stays deferred until the core run path is practical and trustworthy in daily use
- `SWE-bench Verified`, `SWE-bench Pro`, and `Aider Polyglot` remain comparison-ready backlog items rather than near-term gates

**Done when:** A spec with `test_commands` shows real pass/fail in the run report, Coobie's episode scores reflect actual test execution, and Mason's fix loop handles wrong-answer failures. The explicit test harness, structured wrong-answer evidence path, retry-improvement tracking, and LiveCodeBench canary lane are now shipped; broader benchmark expansion remains intentionally deferred behind core factory maturity.

---

## Phase 5-C — Per-Phase Context Gating + Sub-Agent Dispatch

**Why:** Three compounding problems hit the orchestrator at this phase. First, every agent receives the same Coobie preflight briefing regardless of role — Scout, Mason, and Sable have fundamentally different information needs, and the undifferentiated corpus wastes context window and risks priming Sable with Mason's implementation reasoning before hidden-scenario scoring. Second, briefing construction, Sable evaluation, and Mason failure diagnosis each inflate the orchestrator's main context window with exploration that never needs to cross back: the orchestrator only needs the finished output, not the retrieval trace. Third — and most subtly — even a correctly scoped briefing can be wrong-sized: too many hits dilutes the relevant signal just as much as wrong categories. The ideal briefing has scope (right categories), relevance (right entries within those categories re-ranked against the specific task), and volume (right total token count). All three must hold simultaneously. Both the scoping and volume problems share the same solution phase: replace the flat `BriefingScope` parameter with a `ContextTarget` that carries all three dimensions, and isolate the high-context work in sub-agents.

This is a retrieval-shaping and isolation capability, not a storage change. It does not require TypeDB or Qdrant. It is placed here — before the memory module refactor — because the `BriefingScope` enum and filter logic can land in `src/coobie.rs` now and move cleanly into `src/memory/briefing.rs` during Phase 5b's refactor. The `SubAgentDispatcher` lands in `src/subagent.rs`; orchestrator call sites are thin wrappers.

Full design: `factory/context/briefing-scope-design.md` (BriefingScope) and `factory/context/sub-agent-dispatch-design.md` (SubAgentDispatcher).

**Explicit sub-slice order so this phase does not blur again:**

- **Phase 5-C1 — shipped:** `ContextTarget` metadata for Coobie preflight, budgeted memory-hit shaping, stamped-context section injection tracking, and attribution observability (`briefing_scope`, token budget/usage, hits provided).
- **Phase 5-C2 — shipped:** distinct Scout, Mason, and Sable briefing projections are now materialized as run artifacts, Scout and Mason now consume their scoped briefings on the hot path, Sable's generated-scenario prompt now receives only the scenario-pure scoped summary, and repo-local prompt support / retriever bundles are filtered by role so hidden-scenario work is not primed with Mason implementation context.
- **Phase 5-C3 — SHIPPED 2026-04-28:** `SubAgentDispatcher` in `src/subagent.rs` with `dispatch()` method and three-tier resolution order (profile `dispatch:` block → `[sub_agents.*]` in harkonnen.toml → `default_mode`). `SubAgentConfig` + `SubAgentTaskConfig` parsed into `SetupConfig`. `dispatch:` field added to `AgentProfile`. `SubAgentDispatcher` added to `AppContext`. Both high-context call sites wired: Coobie briefing construction → `dispatch_coobie_briefing()` (DirectLlm: behavioral no-op; ClaudeCodeAgent: isolated call with clean context window); Sable scenario evaluation → `isolation_prefix` injected into `sable_generate_and_evaluate()` when backend is ClaudeCodeAgent (Sable firewall: drops implementation_notes, mason_plan, edit_rationale from context). All 120 existing tests pass.

**What to build:**

- `BriefingScope` enum in `src/coobie.rs` (migrates to `src/memory/briefing.rs` in Phase 5b): `ScoutPreflight`, `MasonPreflight`, `PiperPreflight`, `SablePreflight`, `CoobiConsolidation`, `OperatorQuery`. Each variant carries a `phase_id` and a `role` tag. `BriefingScope` defines the *category filter only* — it remains the clean enum it is now.
- Scope-keyed retrieval filter: each scope defines an `allow_categories` list (e.g. Scout: `spec_history, prior_ambiguities, operator_model`; Mason: `failure_patterns, fix_patterns, workspace_guardrails, causal_links`; Sable: `scenario_patterns, hidden_scenario_outcomes` — explicitly excludes Mason implementation notes).
- **`ContextTarget`** struct wrapping `BriefingScope` with the two missing dimensions:

  ```rust
  pub struct ContextTarget {
      pub scope: BriefingScope,
      pub task_description: String,  // re-ranks hits by similarity to THIS task, not a generic query
      pub token_budget: u32,         // hard cap; hits truncated in relevance order after scope filter
      pub min_hits: u32,             // always include at least N hits regardless of score
      pub required_sections: Vec<ContextSection>, // injected first, outside the token budget
  }
  ```

  `build_targeted_briefing(target: ContextTarget, run_id, spec_id) -> BriefingPackage` replaces the current `build_preflight_briefing`. Internally: scope filter → re-rank hits by `task_description` embedding similarity → inject `required_sections` → fill remaining budget with top-ranked hits. The orchestrator constructs a `ContextTarget` per phase entry point using a `phase_defaults()` function that provides sane budgets and required sections per scope.

- **Stamped project interview context as first-class preflight input** — the repo-stamp interview's Mythos/Pathos/Ethos/Episteme/Praxis material (purpose, stakes, stakeholder attitudes, prohibitions, vertical, skill sources, MCP posture) should be loaded from `.harkonnen/repo.toml` and injected into Scout + Coobie briefing shaping as a `required_sections` entry (always present, outside the ranked budget). This keeps project posture inspectable and continuity-aligned rather than leaving it trapped in generated markdown artifacts.
- Wire in orchestrator: construct a `ContextTarget` at each phase entry point (Scout, Mason, Sable are the critical three; others can default to `OperatorQuery` scope with a conservative `token_budget`). The `task_description` is the spec title + active task for that phase.
- Coobie episode record: add `briefing_scope`, `briefing_tokens_used`, and `briefing_hits_provided` fields so causal analysis can distinguish whether a lesson was visible at the relevant phase and whether the briefing was over- or under-loaded.
- **`SubAgentDispatcher`** in `src/subagent.rs` with `dispatch(task, input) -> SubAgentResult`. Backends: `DirectLlm` (current behavior, no isolation), `ClaudeCodeAgent { model, max_turns }`, `CodexPlanAgent { model, context_paths }`, `GeminiAgent { model }`, `ExternalMcp { server, tool }`. Sub-agents read only; all memory and SQLite writes remain in the orchestrator.
- **`[sub_agents]` config section** in `harkonnen.toml`: `default_mode = "direct_llm"` plus named task entries (`coobie_briefing`, `sable_evaluation`). Per-environment overrides via `setups/` follow the existing named-setup pattern.
- **Agent profile `dispatch:` block** — coobie and sable profiles declare per-task backend preferences that take priority over the global TOML config. Resolution order: profile `dispatch.<task>` > `[sub_agents.<name>]` > `[sub_agents] default_mode`.
- Wire Phase 5-C orchestrator call sites: `BriefingConstruction` dispatches to `ClaudeCodeAgent`; `ScenarioEvaluation` dispatches to `ClaudeCodeAgent` (isolation-critical). `DirectLlm` is the fallback for all other tasks.
- `SubAgentResult` fields (`backend_used`, `tokens_used`, `duration_ms`) appended to `agent_traces` table for cost and performance observability.

**Sable isolation constraint (non-negotiable):** `SablePreflight` scope must never include retrieved hits tagged `implementation_notes`, `mason_plan`, or `edit_rationale`. This is the hidden-scenario firewall. If a hit's tag set intersects these, it is dropped regardless of relevance score. The `ClaudeCodeAgent` backend for `ScenarioEvaluation` enforces this at the sub-agent system prompt level in addition to the scope filter.

**Memory write discipline:** Sub-agents dispatched via `SubAgentDispatcher` may not write to memory, SQLite, or the Calvin Archive. Their system prompts list these as `disallowed_tools`. The orchestrator receives `SubAgentResult.output` and decides what to persist.

**Done when:**

- Scout, Mason, and Sable each receive a distinct briefing shaped to their role; stamped repo interview context is visible in the relevant preflight surfaces; run artifacts now include `scout_briefing.{json,md}`, `mason_briefing.{json,md}`, and `sable_briefing.{json,md}`; scoped repo-local prompt support is filtered per role; and Sable's briefing verifiably contains no Mason implementation content.
- `ContextTarget` struct in `src/coobie.rs`; `build_targeted_briefing()` replaces `build_preflight_briefing`; `phase_defaults()` provides sane per-scope budgets; episode record captures `briefing_tokens_used` and `briefing_hits_provided`.
- `SubAgentDispatcher` struct in `src/subagent.rs` with `dispatch()` method; `[sub_agents]` section parsed from `harkonnen.toml` into `SetupConfig`; `coobie_briefing` and `sable_evaluation` tasks dispatch to `ClaudeCodeAgent` backend.
- Agent profile `dispatch:` blocks parsed for coobie and sable; resolution order enforced.
- All existing tests pass (`DirectLlm` backend is a behavioral no-op vs. current calls).

---

## Prediction System — **SHIPPED 2026-04-28**

Closes the learning loop by making Coobie's expectations explicit before each run and measuring how wrong they were afterward.

**What was built:**

- **TypeDB schema** — `prediction_record` + `prediction_result` entities, `prediction_for_run` + `prediction_verified` relations. New attributes: `predicted-outcome`, `actual-outcome`, `prediction-error`, `risk-score`, `failure-phase`, `failure-kind-label`, `source-cause-ids`.
- **Calvin sidecar** — `record_prediction()`, `record_prediction_result()`, `get_prediction()` in `archive.rs`; `POST /runs/{id}/predictions`, `GET /runs/{id}/predictions`, `POST /runs/{id}/prediction-result` in `api.rs`.
- **Calvin client** — `RunPrediction` and `PredictionOutcome` structs + three client methods in `calvin_client.rs`.
- **Prediction synthesis** (`synthesize_run_prediction()` in `orchestrator.rs`) — heuristic risk scoring from `prior_causes` frequency and pass rate + `required_checks` count + `open_questions` count → predicted outcome (`pass`/`uncertain`/`fail`) + risk score 0–1. Marked `basic_heuristic`; Phase 7 replaces with a model-driven classifier trained on the causal attribution corpus.
- **Prediction error** (`compute_prediction_error()`) — asymmetric: predicted `pass`, got `fail` → 1.0 (worst); false alarm → 0.6; `uncertain` → max 0.2. Missing a failure is penalised more than raising a false alarm.
- **Orchestrator wiring** — prediction emitted after every `dispatch_coobie_briefing()`; prediction result emitted at both run completion paths (alongside `try_close_calvin_run()`).
- **E2E tests** — 4 new tests in `tests/e2e_integration.rs`: `prediction_recorded_before_run`, `prediction_result_recorded_after_run`, `prediction_error_is_maximum_for_false_confidence`, `full_prediction_round_trip_in_calvin`. 128/128 tests passing.

**What this enables:** every run now produces a `prediction_error` score. Runs where the error is high (unexpected failures) are natural candidates for intensive causal annotation in Phase 7. The error signal is the primary input for Phase 8 posture adaptation.

---

## Phase 5-D — PackChat Memory Distillation Chain

**Unlocks:** The full conversation-to-continuity path. Twilight Bark carries live PackChat events, Harkonnen persists them as memory candidates, Coobie distills them into durable thoughts, Open Brain (OB1) stores shared semantic recall, and Calvin receives only governed promotion contracts.

This is now the critical memory slice. It replaces the old assumption that the next step must be a larger local vector store. The local fastembed/SQLite vector store remains opt-in; OB1 is the shared recall default.

**Twilight Bark dependency rule:** Twilight Bark is an upstream transport dependency for Harkonnen Labs, not a downstream consumer of Harkonnen concepts. Harkonnen-owned operation labels and PackChat/Calvin schemas may ride over Twilight's generic task/event surface, but Twilight Bark must stay free of Harkonnen imports, Calvin archive assumptions, and Labrador-specific runtime policy.

### Calvin Archive sidecar correctness (pre-5-D gate) — **SHIPPED 2026-04-28**

End-to-end integration tests (`tests/e2e_integration.rs`) confirmed and drove two correctness fixes to the live Calvin sidecar.

**Fix 1 — `get_active_beliefs` agent-scoped** (`archive.rs:241`) — **done**.
Added `(source: $b, target: $a) isa stabilizes` join to the TypeQL query. Foreign beliefs from other `agent_self` instances no longer appear in any agent's result set. Test: `calvin::beliefs_scoped_to_named_agent` — green.

**Fix 2 — `check_adaptation_safe` semantic negation patterns** (`archive.rs:323`) — **done**.
Expanded the deny-list to `avoid`, `without`, `less`, `deprioritise`/`deprioritize`, `reduce`, `replace`, `instead of`, `abandon`, `drop` in addition to the original `not`/`remove`/`eliminate`. Marked `basic_heuristic`; Phase 6 upgrades to an embedding classifier. Test: `calvin::adaptation_safety_catches_semantic_negation` — green.

**Fix 3 — PackChat causal-link addressability** (`archive.rs:141`, `archive.rs:350`) — **done**.
`record_experience` now preserves a caller-supplied `episode_id` as the Calvin `experience.uuid` and links the experience to its `run_record` via `belongs_to_run`. `record_causal_link` now inserts the schema-valid `(cause: $cause, effect: $effect) isa causally_contributed_to` relation instead of attempting to match generated random UUIDs or use non-schema relation syntax. The TypeDB schema now declares `experience` as a player for causal cause/effect and run-link roles, and `run_record` as the run-link role player. This makes PackChat `causation_id -> message_id` links addressable on the real TypeDB path, not only in mocks.

**Fix 4 — status update upsert semantics** (`archive.rs:379`) — **done**.
`update_agent_status` no longer requires an existing `status` attribute. It deletes an old status when present, then inserts the requested status, so the Twilight presence TTL watcher can mark newly seeded or statusless agents offline/active without silently doing nothing.

### Data path

```text
Twilight Bark / PackChat envelope
  -> memory_candidates table
  -> Coobie distillation worker
  -> Open Brain capture_thought
  -> Open Brain search_thoughts in briefings
  -> Calvin promotion contract
```

### What to build

- `memory_candidates` table keyed by `candidate_id`, with `source_event_id`, `thread_id`, `run_id`, `agent_runtime_id`, `operation`, `raw_payload`, `importance_score`, `retention_class`, `sensitivity_label`, `evidence_refs`, `causality`, `status`, and timestamps.
- PackChat/Twilight ingest hook that writes candidate rows idempotently from local PackChat messages and remote Twilight Bark envelopes.
- Coobie distillation worker that groups nearby conversation fragments, summarizes memory-worthy content, dedupes against recent candidates and OB1 recall, classifies retention as `ephemeral`, `working`, `shared_recall`, or `calvin_candidate`, and preserves provenance.
- OB1 writer path using `OpenBrainClient::capture_thought` for `shared_recall` candidates. Captured content must include source thread/run/event provenance and tags, but not raw secrets or unapproved sensitive payloads.
- OB1 reader path already started by `OpenBrainClient::search_thoughts`; complete the ranking so OB1 hits sit beside repo-local memory, PackChat recency, and Calvin-approved facts in targeted Coobie briefings.
- Calvin promotion contract for `calvin_candidate` rows. The contract includes proposed chamber targets, evidence refs, inference posture, confidence, Pathos score, preservation note, and recommended governance outcome: `accept`, `modify`, `reject`, or `quarantine`.
- Operator review surface in the Consolidation Workbench for memory candidates and Calvin promotions. OB1 shared recall may be automatic for low-risk approved classes; Calvin promotion remains governed.
- **Automatic candidate processing on run close** — **SHIPPED 2026-04-28**. `try_process_memory_candidates_on_close(run_id)` is now called immediately after `try_close_calvin_run()` at both successful and failed run completion paths in `orchestrator.rs`. Processing failures log a warning and leave candidates as `pending` for manual retry (retry scheduler remains future work). E2E test: `memory_candidates::run_close_triggers_candidate_processing` — green.
- **Candidate retry semantics** — **SHIPPED 2026-04-28**. OB1 capture failures and Calvin promotion enqueue failures now mark candidates as `retry_pending`, and the candidate processor scans both `pending` and `retry_pending` rows so manual/API-triggered processing retries transient failures instead of silently stalling. `OpenBrainClient` now uses a configurable short timeout (`open_brain.timeout_ms`, default 2500 ms) so OB1 outages do not block productive run close for 30 seconds. Regression test: `chat::tests::pending_memory_candidate_scan_includes_retry_pending` — green.
- **Candidate retry/operator surface** — **SHIPPED 2026-04-28**. `GET /api/runs/{id}/memory/candidates` now returns `status_counts`, `retryable`, and `actionable` totals covering `pending`, `retry_pending`, `waiting_openbrain`, `held_for_review`, `captured_openbrain`, and `promotion_pending`; `POST /api/runs/{id}/memory/candidates/retry` is a clear retry alias for the processing endpoint. The Run Detail drawer now includes a Memory tab with candidate counts, status chips, recent candidate previews, and a one-click retry action.
- **Candidate review actions** — **SHIPPED 2026-04-28**. `POST /api/runs/{id}/memory/candidates/{cid}/approve` releases `held_for_review`, `retry_pending`, or `waiting_openbrain` candidates back to processing and immediately retries them; `POST /api/runs/{id}/memory/candidates/{cid}/discard` marks uncaptured/unpromoted candidates as discarded. The Memory tab now shows approve/discard actions on reviewable candidates. Regression test: `chat::tests::held_memory_candidate_can_be_approved_or_discarded` — green.
- **Memory chain readiness signal** — **SHIPPED 2026-04-28**. `GET /api/runs/{id}/memory/candidates` now returns `memory_chain_status` (`clear`, `processing`, `needs_review`, `retry_pending`, `waiting_openbrain`, `calvin_review`) plus `memory_chain_blockers`; the Memory tab renders a readiness banner so operators can see at a glance whether the PackChat → OB1 → Calvin chain is clear, waiting on service configuration, or waiting on review.
- **OB1 reader dedupe in briefings** — **SHIPPED 2026-04-28**. Coobie briefing assembly now deduplicates memory hits by normalized content before adding source labels, so the same fact from repo memory/core memory/OB1 is not repeated just because it arrived through multiple recall paths. Provenance suffixes such as PackChat source refs are stripped for the dedupe key while preserved in the displayed hit. Regression test: `orchestrator::tests::normalize_briefing_hit_key_dedupes_source_labels_and_provenance` — green.
- **Twilight ingest loop write-back to Calvin** — **SHIPPED 2026-04-28**. `run_twilight_ingest_once()` in `chat.rs` now checks each inbound wire envelope for `archive_contract.schema == "harkonnen.calvin.ingress.v1"` and calls `calvin_client.record_experience()` with the mapped `ArchiveExperience`. Non-null `causation_id` values are forwarded as `causally_contributed_to` links via `POST /runs/{id}/causal-links` (Pearl level: `Associational`, carried in the relation scope until Phase 7 adds a dedicated Pearl-level attribute). Calvin is now initialised before the ingest loop spawns so the client is available from the first poll. New endpoints added: `POST /runs/{id}/causal-links` on Calvin, `record_causal_link()` on `CalvinClient`. E2E tests: `twilight::twilight_ingest_loop_writes_to_calvin`, `memory_candidates::causation_id_written_to_calvin_causal_graph` — both green.
- **Complete chamber mapping for all six chambers** — **SHIPPED 2026-04-28**. Added `BeliefRevised` and `DriftDetected` variants to `PackChatBusEventKind`. `calvin_chamber_for_packchat_event()` now maps by event kind first: `ThreadOpened → mythos`, `ThreadRosterSynced → ethos`, `BeliefRevised → episteme`, `DriftDetected → pathos`, `CheckpointResolved → praxis`, `MessageAppended → logos` (role-refined). All six chambers are now reachable. E2E test: `twilight::chamber_mapping_covers_all_six_chambers` — green.
- **Agent presence TTL watcher → Calvin agent status** — **SHIPPED 2026-04-28**. `spawn_twilight_ingest_loop()` now maintains a `HashMap<agent_id, Instant>` presence tracker that persists across reconnect cycles. On each loop iteration, agents not seen for > 600 s are marked `"offline"` via `PATCH /agents/{name}/status`. Activity events reset the last-seen timestamp. New endpoint added: `PATCH /agents/{name}/status` on Calvin, `update_agent_status()` on `CalvinClient` and `ArchiveStore`. E2E test: `twilight::agent_presence_expiry_updates_calvin_agent_status` — green.
- **Twilight Bark dependency-direction guard** — **SHIPPED 2026-04-28**. `src/chat.rs` now names the Harkonnen-owned PackChat topic root and Twilight operation label explicitly, and the publish path builds a generic Twilight `publish_task` command carrying an opaque `harkonnen.packchat.event` payload. Regression test: `chat::tests::twilight_bridge_uses_harkonnen_owned_operation_over_generic_task_ipc` — green. This protects the boundary that Harkonnen depends on Twilight Bark as transport while Twilight Bark remains Harkonnen-agnostic.

### OpenZiti service profile

Define the distributed trust model before broad deployment:

| Service | Dial | Bind | Access posture |
| --- | --- | --- | --- |
| `twilight-bark.packchat` | approved agent runtimes, Pack Board | Twilight daemon nodes | Pack conversation and event bus |
| `openbrain.mcp` | Harkonnen distiller, approved recall clients | OB1 server | Shared recall; write permission narrower than read |
| `calvin.archive` | Coobie/Harkonnen archive writer, operator console | Calvin host | Governed archive write path |
| `harkonnen.api` | operator console, approved integrations | Harkonnen host | Run control and review UI |

OpenZiti Dial and Bind policies should be separate. Privileged writers should carry posture checks where available: enrolled identity, expected OS, MFA for operators, and known process checks for daemons. Remote agents can read OB1 recall through policy, but only the Harkonnen distiller writes Calvin promotion contracts by default.

### Benchmark / product gate

- **E2E integration test suite green — DONE 2026-04-28:** all 20 tests in `tests/e2e_integration.rs` pass. The six gap tests that were failing at roadmap entry are now green: `calvin::beliefs_scoped_to_named_agent`, `calvin::adaptation_safety_catches_semantic_negation`, `twilight::twilight_ingest_loop_writes_to_calvin`, `twilight::chamber_mapping_covers_all_six_chambers`, `twilight::agent_presence_expiry_updates_calvin_agent_status`, `memory_candidates::run_close_triggers_candidate_processing`.
- A PackChat thread with at least five messages produces one or more memory candidates.
- A candidate marked `shared_recall` is captured in OB1 and later retrieved by `search_thoughts` during a targeted briefing.
- A candidate marked `calvin_candidate` produces a structured promotion contract without directly mutating Calvin canonical state.
- Candidate dedupe prevents repeated chat phrasing from producing duplicate OB1 thoughts.
- Sensitivity labels prevent secrets and high-risk payloads from being sent to OB1 without review.
- Closing a run automatically triggers candidate processing; transient failures are marked `retry_pending` and retried by the same processing endpoint until zero remain after a clean run close.
- A `causation_id`-bearing wire envelope produces a `causally_contributed_to` link in the Calvin causal graph.
- OpenZiti policy documentation exists for all four services, including Dial/Bind identity roles.

**Done when:** a live PackChat/Twilight conversation can become a distilled OB1 memory with provenance, the memory can improve a later briefing, identity-relevant material is routed to Calvin as a governed promotion proposal rather than as unstructured prose, and all six E2E integration gap tests pass.

---

## Phase 5b — Memory Infrastructure, MCP Prompts + Rust-Native Servers

**Unlocks:** Four things that must land before TypeDB (Phase 6) is viable: a clean memory module structure, document ingest completeness (OCR), a live MCP prompt surface, and an OB1-backed memory abstraction that keeps shared recall available across Claude, ChatGPT, Codex, Cursor, Twilight-connected agents, and Harkonnen itself.

This phase also eliminates Harkonnen's remaining local `npx` MCP helper processes in favour of a single compiled Rust binary where Harkonnen owns the tool surface. OB1 itself may still be reached through its MCP endpoint or a `supergateway` bridge; that is an external service boundary, not a Harkonnen hot-path runtime dependency.

**What to build:**

### Memory module refactor

Split the growing `src/memory.rs` into the module tree described in COOBIE_SPEC:

```text
src/memory/
  mod.rs          # re-exports; MemoryStore trait
  working.rs      # short-term blackboard (SQLite-backed)
  episodic.rs     # run episodes, briefing_scope field
  semantic.rs     # SemanticMemory trait; OB1 default impl, local vector fallback optional
  semantic_openbrain.rs # Open Brain (OB1) MCP-backed shared recall implementation
  semantic_local.rs     # optional fastembed/OpenAI-compatible local vector fallback
  causal.rs       # causal links, failure patterns
  consolidation.rs
  blackboard.rs
  retrieval.rs    # build_targeted_briefing() migrates here from src/coobie.rs
  extraction.rs
  briefing.rs     # BriefingScope enum + ContextTarget struct (migrated from Phase 5-C)
  context_budget.rs  # phase_defaults(), ContextSection, token counting utilities
```

No behaviour change beyond preserving OB1 as the default semantic recall path. This is the maintainability gate that lets TypeDB's typed causal query implementation slot in cleanly in Phase 6.

### Open Brain (OB1) semantic integration

Add `src/memory/semantic_openbrain.rs` implementing the `SemanticMemory` trait against Open Brain via the existing `OpenBrainClient`. Payload metadata fields: `org`, `role`, `product`, `spec_id`, `run_id`, `thread_id`, `source_event_id`, `agent`, `memory_type`, `tags`, `sensitivity_label`, `created_at`. OB1 replaces the local vector store as the default long-term semantic memory. SQLite remains the short-term, episodic, causal, and review store. The fastembed/SQLite vector path remains available as `semantic_local.rs` behind `--features local-embeddings`, and Qdrant becomes an optional accelerator only if a future deployment needs local high-volume vector serving.

### Scanned document ingestion (`pdfium-render` + Claude vision)

The existing `memory ingest` path already handles text-forward PDFs — files where the text layer is real and selectable. The gap is image-only PDFs: scanned documents where every page is a rasterized image with no extractable text. The fix is a two-stage pipeline wired into the existing `memory ingest` path:

```text
memory ingest <file.pdf>
  │
  ├─ pdfium-render → text layer present?
  │     yes → extract directly (current behaviour, unchanged)
  │
  └─ no text layer (scanned)
        │
        ├─ pdfium-render → rasterize pages to images
        │
        ├─ Claude vision API → extract text per page
        │     structured output: { page, text, confidence }
        │
        └─ fallback: tesseract-rs (if vision API unavailable /
               rate-limited / cost-constrained)
```

Add `pdfium-render` to `Cargo.toml` as the rasterization layer (Rust bindings to Google's pdfium — the same engine Chrome uses). Claude vision extraction is substantially better than Tesseract on complex layouts, multi-column text, tables, and degraded scans. The existing `src/tesseract.rs` becomes the offline fallback rather than the primary path.

No new CLI surface. The `memory ingest` command detects the absence of a text layer, runs the pipeline silently, and writes the extracted text sidecar alongside the imported asset. A `confidence` field in the sidecar records whether extraction came from the text layer, vision API, or Tesseract fallback, so downstream retrieval can weight hits accordingly.

### MCP prompts — live dynamic briefings from `mcp_server.rs` — **SHIPPED 2026-04-28**

`get_prompt` is now async and state-aware. Four live-hydrated prompts added to `src/mcp_server.rs`:

| Prompt | Arguments | What it returns |
| --- | --- | --- |
| `coobie/briefing` | `keywords`, `phase`, `run_id?`, `max_tokens?` | Real memory hits (file store + OB1) + top prior causes from SQLite, budget-capped |
| `sable/eval-setup` | `run_id` | Run artifacts + OB1 scenario patterns; firewall drops implementation_notes, mason_plan, edit_rationale, fix_patterns |
| `scout/preflight` | `spec_id?`, `run_id?` | Spec-scoped memory + OB1 recall + operator model commissioning brief patterns |
| `keeper/policy-check` | `action`, `context?` | Policy-scoped memory + OB1 recall for the proposed action |

All prompts: file-backed memory search → OB1 `search_thoughts` → deduplication → scope isolation → token budget (≈4 chars/token). Registered in `prompt_descriptors()` with argument schemas. The two static templates (`briefing_for_spec`, `diagnose_run`) are preserved for backwards compatibility.

### `memory_pull` — on-demand context retrieval mid-task — **SHIPPED 2026-04-28**

New MCP tool in `src/mcp_server.rs`:

```text
tool:    memory_pull
args:    query (string, required), scope (string, default "general"), max_tokens (integer, default 500)
returns: top-ranked memory hits relevant to query, scoped and budget-capped
```

Searches file-backed memory → OB1 → deduplicates → filters by scope (sable scope drops mason content; mason scope drops scenario content) → truncates to `max_tokens` budget. Each call is logged via `tracing::info!` with query, scope, hits_returned, and token count for context utilization analysis. Registered in `tool_descriptors()` with full JSON Schema.

Pull records are logged (not yet persisted to SQLite episode store — episode record extension remains in the context utilization tracking item below).

### Context utilization tracking

The episode record gains a `ContextUtilization` section:

```rust
pub struct ContextUtilization {
    pub briefing_hits_provided: u32,
    pub briefing_tokens: u32,
    pub mid_task_pulls: Vec<PullRecord>,
    pub utilization_rate: f32,  // fraction of briefing hits referenced in agent output
}
```

`utilization_rate` is computed post-run by Coobie: scan the agent's output for references to the content of each briefing hit (embedding similarity above a threshold). A briefing with `utilization_rate < 0.2` over multiple runs for the same scope is a signal that the budget is too high or the category filter is too loose. This data feeds the Phase 7 causal corpus and the Phase 8 Episteme chamber's slow-loop policy revision for scope configuration.

### Rust-native MCP server consolidation (`rmcp`)

Replace the three `npx @modelcontextprotocol/server-*` processes (filesystem, memory, sqlite) with a single `harkonnen mcp serve` invocation backed by the `rmcp` crate (Anthropic's official Rust MCP SDK). The consolidated server:

- Exposes the same tool aliases (`filesystem_read`, `workspace_write`, `artifact_writer`, `memory_store`, `metadata_query`, `db_read`) so no harkonnen.toml changes are needed at the agent-routing level
- Serves the new `prompts` surface (see above) over the same transport
- Removes Node.js / `npx` from the runtime dependency list entirely
- Uses `sqlx` directly for SQLite access rather than shelling out to a Node sqlite server

`harkonnen.toml` is updated to replace the three `npx` server entries with a single self-server entry pointing at `harkonnen mcp serve --transport sse`. The three old entries are removed from `common_mcp_templates` in `src/cli.rs`; the self-server config block becomes the default for all setups.

### `llm.rs` multi-provider extension

Extend `src/llm.rs` with a unified multi-provider completion interface to back the `SubAgentBackend` variants introduced in Phase 5-C without shelling out to external CLIs:

```rust
pub enum ProviderBackend {
    Anthropic { model: String },   // reqwest → messages API
    OpenAi    { model: String, base_url: Option<String> },  // reqwest → chat completions
    Gemini    { model: String },   // reqwest → generateContent
}

pub async fn complete(backend: &ProviderBackend, messages: &[Message]) -> Result<String>
```

Each variant is a typed `reqwest` call to the provider's REST API. `SubAgentBackend::CodexPlanAgent` routes through `ProviderBackend::OpenAi` (model: `o4-mini` or `gpt-4o`) rather than spawning the codex CLI process. `SubAgentBackend::GeminiAgent` routes through `ProviderBackend::Gemini`. The existing `SubAgentBackend::DirectLlm` path continues to use the current `llm.rs` call site unchanged — this is additive, not a rewrite.

**Benchmark gate:**

- Re-run `FRAMES` after OB1 lands as the default semantic recall path to confirm multi-hop recall improves over the SQLite/local-vector baseline
- `LongMemEval` and `LoCoMo` re-run to confirm semantic recall quality does not regress
- Re-run `StreamingQA` to confirm belief-update accuracy does not regress after the module refactor
- MCP prompt round-trip test: `coobie/briefing` for a known run returns a briefing containing at least one memory hit and zero items tagged with Mason-scoped categories
- Token budget enforcement test: `coobie/briefing` called with `max_tokens=500` returns ≤ 500 tokens of ranked content with required sections present regardless of budget
- `memory_pull` latency: p95 round-trip under 500ms on home-linux against SQLite + OB1, with local-cache hits under 200ms when available
- Context utilization baseline: record `utilization_rate` for 10 runs across Scout, Mason, and Sable scopes; establish the floor before Phase 7 causal corpus work begins

**Done when:**

- `src/memory.rs` is split into the COOBIE_SPEC module tree; `BriefingScope` and `ContextTarget` live in `src/memory/briefing.rs`; `phase_defaults()` and token budget utilities in `src/memory/context_budget.rs`
- `build_targeted_briefing()` is the sole briefing entry point; no call site uses the old `build_preflight_briefing` or `build_scoped_briefing`
- OB1 is serving semantic queries for long-term shared recall through the `SemanticMemory` abstraction
- local fastembed/SQLite vectors remain opt-in and compile-disabled by default
- OCR-scanned PDFs can be ingested via `memory ingest`
- `mcp_server.rs` serves all four named prompts with `ContextTarget` budget enforcement; `memory_pull` tool is live; `/mcp coobie/briefing` works in a Claude Code session and respects `max_tokens`
- Episode records include `ContextUtilization` with `utilization_rate`; 10-run baseline collected
- Harkonnen-owned local MCP helper entries are replaced by `harkonnen mcp serve` in `harkonnen.toml`; Node.js is no longer required for Harkonnen's local helper surface
- `llm.rs` exposes `ProviderBackend` with Anthropic, OpenAI, and Gemini variants; `SubAgentBackend::CodexPlanAgent` routes through `ProviderBackend::OpenAi` with no subprocess spawn

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
- **DeepCausality alignment pass** — before deriving executable causaloids from TypeDB or Calvin links, migrate or explicitly pin the DeepCausality API target. Harkonnen currently has a Phase 1 bridge on `deep_causality = "0.3"`, while the current DeepCausality repo centers the modular `PropagatingEffect` / `PropagatingProcess` stack, Effect Ethos, discovery, tensors, and topology crates. The Phase 6 graph should preserve enough structure for that math: effect propagation (`E2 = f(E1)`), contextoids, assumption checks, explanation paths, and Pearl-level promotion evidence.
- **Semantic adaptation safety classifier** — upgrade `check_adaptation_safe` in `archive.rs` from heuristic string-matching to an embedding-based classifier. Represent each Labrador kernel trait as a vector; score the proposed adaptation summary against the negation-space of each trait using cosine similarity. Flag any adaptation that scores above a configurable negation threshold as unsafe regardless of literal phrasing. This replaces the `basic_heuristic` marker added in the Phase 5-D correctness fix. E2E test: `calvin::adaptation_safety_catches_semantic_negation` (currently passing with the mock; the real TypeDB path must pass the same assertion).
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
- **DeepCausality discovery adapter spike** — evaluate whether SURD/MRMR-style discovery can propose candidate causal-pattern records from the labeled corpus. Discovered patterns stay `quarantine` or `review_required` until Coobie/Keeper/operator promotion supplies evidence and governance.
- Publish before/after comparisons for causal attribution accuracy: pre-Phase 4 (pure semantic recall) versus post-Phase 6 (TypeDB causal graph-augmented).

**Benchmark gate:**

- `E-CARE` first run published — causal explanation coherence score
- `causal attribution accuracy` first run published — top-1 / top-3 vs semantic-only baseline

**Done when:** The corpus has at least 30 labeled entries, the causal attribution accuracy benchmark has a published run, and E-CARE has a published score.

---

## Phase 8 Design Prerequisites

**Resolve these before implementation begins.** These gaps were identified in a soul-of-ai audit (2026-04-22) as missing or under-specified relative to what Phase 8 requires. None are code work; resolving them means specifying them in MASTER_SPEC Part 5 or equivalent design documents before the build phase opens.

### P8-P1 — Behavioral contract structure per agent

Chapter 09 of the-soul-of-ai defines `C = (P, I, G, R)` — preconditions, invariants, governance policies, recovery mechanisms — as the formal behavioral contract per agent. D* and SSA both presuppose this structure. Specify how `BehavioralContract` is represented (likely a struct in `src/models.rs`) and what the `R` (recovery mechanism) set looks like for each Labrador role before wiring the metrics.

### P8-P2 — Three-timescale integration architecture

Chapter 08 of the-soul-of-ai distinguishes three architecturally distinct loops: fast (per experience: belief/disposition updates), medium (per reflection cycle: schema revision, cross-episode pattern integration operating on compressed representations), slow (per meta-reflection with human endorsement: integration policy revision). Phase 8 covers the slow loop explicitly. The **medium loop** — how compressed cross-episode patterns are created, stored, and fed into schema revision — needs explicit specification before the Calvin Archive schema is finalized. Schema revision must be structurally distinct from ordinary belief revision.

### P8-P3 — Pathos propagation mechanism

The Pathos chamber is not a passive store. It is a weighting layer that determines how far an experience propagates through the other chambers. High-Pathos events reach Ethos; low-Pathos events inform priors without dominating. Without this propagation mechanism, the six chambers are six separate stores rather than stages in a pipeline. Specify the Pathos score computation and the threshold logic that gates propagation to Ethos before the TypeDB schema is written.

### P8-P4 — F (Variational Free Energy) approximation decision

The-soul-of-ai/09 explicitly flags `symthaea-fep` as a non-existent aspirational crate and calls F "computed on-demand." Before Phase 8 opens, decide: (a) build a tractable approximation (e.g., KL divergence between agent's recent action priors and the Labrador baseline embedding as a proxy), (b) defer F as aspirational-only and remove it from the Phase 8 "done when" criteria, or (c) scope a minimal Active Inference runtime. The current Phase 8 benchmark gate does not mention F — if it stays out-of-scope, remove it from the metrics implementation list to avoid confusion.

### P8-P5 — Φ (Integrated Information) approximation strategy

Chapter 09 flags exact Φ as NP-hard and says any real implementation requires approximations. Phase 8 lists "Φ post-learning drop detection wired" as a milestone but gives no path. Before Phase 8 opens, specify the approximation method (e.g., small-graph bipartition over the Calvin Archive causal subgraph for a given update, with a configurable node limit) and what constitutes a "drop" that triggers quarantine.

### P8-P6 — Pending evidence bounty mechanism

Chapter 08 requires each quarantined item to carry a "pending evidence bounty" — specific future observations that would resolve the quarantine — with salience decay and resurrection triggers. The quarantine ledger in Phase 8 mentions "pending evidence conditions" but does not specify how conditions are expressed, how incoming experience is matched against them, or what triggers re-evaluation. This needs a schema-level decision before the TypeDB quarantine entity is defined.

### P8-P7 — Integration policy as versioned artifact

Chapter 08's slow loop revises the *policies* about what earns quarantine, what thresholds trigger escalation, and what counts as coherent change. These policies must exist as explicit, versioned artifacts distinct from memory entries. Specify how integration policies are stored (separate TypeDB entity type? a `integration_policies` SQLite table?), versioned, and attached to the slow-loop human endorsement flow before Phase 8 implementation begins.

### P8-P8 — `soul.json` manifest schema

The soul package includes `soul.json` as a manifest with version, integrity hashes, compatibility thresholds, and package wiring. Phase 8 generates this from canonical continuity state, but the schema for `soul.json` is not specified anywhere. Define the fields before the projection logic is written.

---

## Phase 7b — Continuous Learning v2 & Memory Compaction

**Unlocks:** Formalized instinct-to-skill pipeline and token-aware memory persistence, mapping the `everything-claude-code` extraction strategies into the Harkonnen native coordination loop before launching Phase 8.

**What to build:**

- **Instinct Extractor (Continuous Learning):** Passively observe tool usage success on the coordination bus. Store victorious problem-solving sequences as episodic "instincts" with confidence scores.
- **Skill Clustering ("Evolve" loop):** A periodic synthesis pass that compresses raw instincts into reusable, semantic "skills" broadcasted back to agents.
- **Strategic Context Compaction:** Hydrate agent sessions efficiently, handling background summarization of working memory while preserving causal invariants to prevent context-window bloat over long runs.

**Done when:** Harkonnen can automatically identify patterns and evolve skills from raw operator usage on the hot path, and long-running sessions strategically compact their history without losing structural context.

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

## Phase 9 — Cross-Machine Pack Coordination (Zenoh + Buffa)

**Unlocks:** The full nine-agent pack split across home-linux and work-windows
operating as one coherent factory. Today all routing is intra-process or
intra-machine. Phase 9 makes machine boundaries transparent: Scout on
home-linux can dispatch Mason on work-windows; Coobie's briefing travels over a
wire; run events stream to both machines in real time; and Calvin Archive
continuity holds across the boundary.

The transport choice is **Zenoh** (pub/sub with shmem, TCP, TLS, and QUIC
backends; clean Rust SDK; keyexpr routing maps directly to the Labrador topic
hierarchy). The wire format is **Buffa** (Anthropic's pure-Rust protobuf with
zero-copy `MessageView<'a>`; no `protoc` binary required; editions support for
forward-compatible schema evolution). Both are Rust-native — no new runtime
dependencies.

This phase is the point established in Phase 5b: Zenoh and Buffa become worth
their complexity overhead only when agents genuinely span machines. Before this
phase opens, the single-machine setup should be complete and stable.

**Twilight Bark alignment note:** [`Twilight Bark`](https://github.com/durinwinter/twilight-bark) is a plausible concrete implementation target for this future databus because it already ships a Zenoh-powered bus, a traffic controller/registry, MCP-native access, and JSONL event logging. Harkonnen should therefore keep PackChat bus-facing contracts transport-agnostic and shaped around: stable thread/message/checkpoint envelopes, role/runtime identity, topic/keyexpr routing, and append-only eventlog semantics. The current local PackChat path now emits those envelopes to a local JSONL bus log so the eventual Phase 9 switchover can target Twilight Bark rather than a bespoke second transport.

### Proto schema (`factory/proto/`)

Define Buffa proto schema for all cross-machine wire types:

```proto
// factory/proto/labrador.proto
message SubAgentInput  { ... }   // Phase 5-C types on the wire
message SubAgentResult { ... }
message RunEvent       { ... }   // phase transitions, checkpoint signals
message PackChatMessage { ... }  // operator chat delivery
message BriefingPackage { ... }  // Coobie briefing over the wire
message CheckpointNotification { ... }
message MemoryHit      { ... }   // single retrieval hit
```

Buffa `MessageView<'a>` is used on the receive path (zero-copy from wire);
owned types are used for construction and serialisation. Proto schema lives in
`factory/proto/`; generated Rust types land in `src/transport/proto.rs` via a
`build.rs` step that invokes `buffa-build`.

### Zenoh transport layer (`src/transport/`)

```text
src/transport/
  mod.rs          # PackTransport trait
  zenoh.rs        # ZenohTransport — one Zenoh session per Harkonnen instance
  local.rs        # LocalTransport (tokio channels) — existing in-process path
  proto.rs        # generated Buffa types (from build.rs)
```

**Key-expression convention:**

```text
harkonnen/{setup_name}/agent/{agent_name}/input
harkonnen/{setup_name}/agent/{agent_name}/result
harkonnen/{setup_name}/run/{run_id}/event
harkonnen/{setup_name}/chat/{thread_id}/message
harkonnen/{setup_name}/memory/briefing/{run_id}
```

`setup_name` scopes all traffic to the originating machine, preventing
cross-contamination between home-linux and work-windows sessions on the same
LAN.

**Transport selection:** the `PackTransport` trait has two implementations —
`ZenohTransport` for cross-machine and `LocalTransport` (tokio channels) for
the existing intra-process path. `SubAgentDispatcher` gains a
`RemoteAgent { machine: String }` backend that routes through
`ZenohTransport`. All existing `DirectLlm` / `ClaudeCodeAgent` call sites are
unchanged.

### `harkonnen.toml` remote agent routing

Extend `SetupConfig` with a `[remote_machines]` section:

```toml
[remote_machines.work-windows]
zenoh_endpoint = "tcp/192.168.1.x:7447"
agents         = ["mason", "piper", "bramble", "ash", "flint"]

[remote_machines.home-linux]
zenoh_endpoint = "tcp/192.168.1.y:7447"
agents         = ["scout", "sable", "keeper", "coobie"]
```

`SubAgentDispatcher` resolution order gains a fourth step:

1. Agent profile `dispatch.<task>`
2. `[sub_agents.<name>]` in harkonnen.toml
3. Check `[remote_machines.*].agents` — if the target agent is listed on a
   remote machine, wrap in `RemoteAgent { machine }` and route through Zenoh
4. `[sub_agents] default_mode`

No change to agent profiles or skill files.

### PackChat distributed mode

`src/chat.rs` now has a PackChat bus seam with future-facing envelope emission;
Phase 9 upgrades that seam from local JSONL/eventlog output to a real
publisher/subscriber transport alongside the existing SQLite store. Messages
written on work-windows are published to
`harkonnen/{setup}/chat/{thread_id}/message` and received by home-linux in real
time without polling. SQLite remains the durable store; Zenoh is the delivery
layer. The MCP `post_chat_message` and `list_chat_messages` tools continue to
work unchanged — the Zenoh subscription fires a write-through to the local
SQLite replica.

If Twilight Bark is adopted here, Harkonnen should map:

- PackChat `thread_opened`, `thread_roster_synced`, `message_appended`, and `checkpoint_resolved` envelopes onto Twilight Bark bus topics without changing PackChat API semantics.
- canonical Labrador role + `agent_runtime_id` onto Twilight Bark agent identity / presence records.
- local `packchat-bus.jsonl` observability onto Twilight Bark's `twilight-eventlog` so replay and audit stay append-only.
- PackChat thread topics onto Zenoh keyexprs directly rather than introducing a second naming scheme.
- Keep Harkonnen-owned operation labels and Calvin ingress contracts in Harkonnen adapter code only; Twilight Bark remains a generic transport and should not grow Harkonnen-specific dependencies.

### Calvin Archive cross-machine consistency

Write authority for the Calvin Archive stays on the machine that owns Coobie
(home-linux by default). Remote machines receive a read-only event stream:

- Run episodes published on `harkonnen/{setup}/memory/episode/{run_id}` by the
  orchestrator after each run
- Home-linux Coobie subscribes, consolidates, and writes to the archive
- Work-windows receives a `soul.json` snapshot over Zenoh on each archive
  update so remote agents boot with current continuity state
- `harkonnen archive status` gains a `--remote` flag that queries the Zenoh
  session for each machine's last-seen snapshot timestamp

Identity continuity invariant: the Calvin Archive on home-linux is the single
source of truth. Remote machines consume its projections; they do not write to
it.

### `llm.rs` provider routing across machines

When `SubAgentBackend::RemoteAgent` dispatches a `BriefingConstruction` task to
a remote machine, the receiving `ZenohTransport` handler deserialises the
`SubAgentInput`, calls the appropriate `ProviderBackend` (from the Phase 5b
`llm.rs` extension) using the remote machine's API keys, and publishes the
`SubAgentResult` back. API keys never cross the wire — each machine uses its
own configured credentials. This is the correct credential isolation model for
a home/work split where API billing accounts differ.

### `setup check` cross-machine status

`harkonnen setup check` gains a cross-machine section:

```text
Remote Machines:
  [ok   ] work-windows   tcp/192.168.1.x:7447   agents: mason, piper, bramble, ash, flint
  [UNREACHABLE] ...
```

Reachability is determined by a Zenoh ping on the configured endpoint. If a
remote machine is unreachable, the dispatcher falls back to `DirectLlm` on the
local machine for that agent's tasks and logs a `remote_fallback` event in
`agent_traces`.

**Benchmark gate:**

- Round-trip latency benchmark: `SubAgentInput` → remote machine → `SubAgentResult` over Zenoh, measured at p50/p95/p99 over 100 runs
- Buffa encode/decode throughput for `BriefingPackage` (the largest message type) vs. serde_json baseline — confirm the zero-copy view path delivers measurable improvement at briefing size
- PackChat delivery latency: message posted on work-windows appears in home-linux `list_chat_messages` within 100ms under normal LAN conditions
- Calvin Archive consistency check: after 10 cross-machine runs, `soul.json` on both machines matches within one snapshot cycle

**Done when:**

- A run can start on home-linux (Scout, Coobie, Keeper) and dispatch
  implementation phases to work-windows (Mason, Bramble) transparently via
  Zenoh, with the same `start_run` API call and no operator configuration
  beyond `[remote_machines]` in harkonnen.toml
- PackChat messages are delivered cross-machine in real time; SQLite replicas
  on both machines stay in sync
- Calvin Archive write authority is on home-linux; work-windows receives
  `soul.json` snapshots and uses them for agent boot continuity
- `harkonnen setup check` reports remote machine reachability and falls back
  gracefully if a remote machine is offline
- Buffa proto schema covers all cross-machine wire types; no `serde_json`
  serialisation on the hot path for `SubAgentInput`/`SubAgentResult`

---

## Phase 10 — Documentation, Evaluation, and Lifecycle Benchmarks

**Sequenced after Phase 8** so benchmarks run against the complete system rather than a pre-archive baseline. The DevBench adapter and spec adherence scores are most meaningful once the factory's full memory and governance stack is live. Phase 10 items with no archive dependency (Flint docs, 10-B, 10-C) can begin earlier if capacity allows, but Phase 10 is not a gate on the Calvin Archive path.

**Twin policy note:** Live twin provisioning is not a Phase 10 completion gate. Phase 10-D (twin fidelity benchmark) is optional diagnostic telemetry only — see Twin Policy above.

### 10-A — Flint documentation phase

- After `self.package_artifacts(run_id)` in the Flint phase, call a new `flint_generate_docs` method
- `flint_generate_docs` reads the spec and Mason's implementation artifacts from the run dir, calls the Flint LLM agent to generate a `README.md` and optionally an `API.md`
- Writes output to `artifacts/docs/<run_id>/README.md` and `artifacts/docs/<run_id>/API.md`
- Adds `docs/README.md` to `blackboard.artifact_refs`
- Required for DevBench — must land before the DevBench gate

### 10-B — `src/spec_adherence.rs` — LLM-as-judge benchmark

New builtin benchmark module (follows the same pattern as `cladder.rs`).

- Loads a JSONL file where each line is `{ "run_id": "...", "spec_path": "...", "output_path": "..." }`, OR if no dataset is provided, queries the local SQLite DB for the last N completed runs
- For each entry: reads the spec's `acceptance_criteria` list and Mason's primary output artifact, asks an LLM judge to score each criterion as met/partial/unmet
- Metrics: `completeness` (fraction of criteria met or partial), `precision` (fraction fully met)
- Env: `SPEC_ADHERENCE_DATASET`, `SPEC_ADHERENCE_LIMIT`, `SPEC_ADHERENCE_OUTPUT`, `SPEC_ADHERENCE_MIN_COMPLETENESS`
- Builtin name: `"spec_adherence"`
- Also supports a `without_scout` mode to measure what Scout's formalization step contributes

### 10-C — `src/scenario_delta.rs` — Hidden Scenario Delta benchmark

New builtin benchmark module — Harkonnen-native, no external dataset.

- Queries `coobie_episode_scores` in the local SQLite DB for runs where both `validation_passed` and `scenario_passed` are recorded
- Computes: `visible_pass_rate` (fraction where `validation_passed = 1`), `hidden_pass_rate` (fraction where `scenario_passed = 1`), `delta = visible_pass_rate - hidden_pass_rate`
- A large positive delta means Bramble passes things that Sable catches — proves the hidden scenario value
- Writes `scenario_delta_report.md` and `scenario_delta_summary.json` to artifact dir
- Builtin name: `"scenario_delta"`
- Env: `SCENARIO_DELTA_LIMIT` (max runs to include), `SCENARIO_DELTA_OUTPUT`

### 10-D — `src/twin_fidelity.rs` — Optional twin telemetry benchmark

- Keep `twin_fidelity_score` honest by counting only services whose status is `"running"`
- Retain a Harkonnen-native summary suite for historical comparison and future revisit
- **Not a Phase 10 blocker.** Live twin provisioning is deferred per Twin Policy above.

### 10-E — `suites.yaml` entries

- `harkonnen_spec_adherence` — Spec Adherence Rate (harkonnen-native, builtin: `spec_adherence`)
- `harkonnen_scenario_delta` — Hidden Scenario Delta (harkonnen-native, builtin: `scenario_delta`)
- `harkonnen_twin_fidelity` — Twin Fidelity telemetry (harkonnen-native, builtin: `twin_fidelity`)
- `harkonnen_devbench` — DevBench wrapper suite (script-based external adapter)

### 10-F — DevBench adapter wiring

- Add `scripts/benchmark-devbench.sh` following the same skip-and-delegate pattern as the existing SWE-bench and tau2 wrappers
- `DEVBENCH_COMMAND` supplies the exact local or hosted command that runs Harkonnen on DevBench
- Optional `DEVBENCH_ROOT` points at the benchmark checkout or adapter workspace
- The wrapper exits with skip code `10` when DevBench is not configured so Phase 10 can be wired before the full external harness is installed

### 10-G — Comparative Control-Style Benchmarking

- Add benchmark suites that compare three execution styles on the same tasks where practical: pure-LLM baseline, rule-heavy baseline, and Harkonnen's hybrid pack/control-plane path
- Publish not only task success but also recovery rate, guardrail violation rate, operator interruption count, and time-to-correctness
- Treat this as a factory benchmark, not a model-only benchmark: the question is how safely and efficiently the delivery system moves, not only how strong one model is in isolation

### 10-H — Adversarial Tool-Use And Stakeholder-Alignment Evaluation

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

**Current state:** DB schema is complete (`operator_model_profiles`, `operator_model_sessions`, `operator_model_layer_checkpoints`, `operator_model_entries`, `operator_model_exports`, `operator_model_update_candidates` tables all exist). Phase v1-D shipped the two-layer MVP: project-first session creation, PackChat operator-model threads, layer approval, `commissioning-brief.json` generation, export metadata, and Scout/Coobie consumption. The full five-layer spec follows.

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

## Parallel Product Track — Compiled State Synthesis

**Unlocks:** A durable human-readable state surface that compiles accepted run state, coordination outcomes, and memory changes into something an operator can browse without manually reconstructing the story from raw logs and tables.

**Current stance:** build this as a pipeline phase first, not a new Labrador. Coobie provides semantic and causal summaries, Keeper provides authoritative coordination state, and Flint renders the artifact surface. Promote synthesis into its own Labrador only if it becomes a durable bottleneck or needs its own trust boundary.

**What to build:**

- `factory/compiled_docs/` artifact family with at least `run/`, `project/`, and `daily/` outputs
- synthesis job that reads `decision_log`, `phase_attributions`, coordination registry tables, operator model tables, and memory invalidation history
- conflict headers in compiled docs when coordination events or decision records show unresolved tension rather than smoothing it away
- compiled summaries that distinguish current accepted state, superseded state, and open questions
- explicit input provenance so a compiled document can point back to the run, decision, or coordination event that produced each major conclusion

**Benchmark / product gate:**

- A completed run produces a compiled state artifact without manual intervention
- Operators can inspect run/project state from compiled docs without opening SQLite or raw JSON logs
- Contradictions surface as explicit unresolved sections rather than disappearing in prose

**Done when:** a completed run produces a compiled state artifact that summarizes what changed, what was decided, which coordination conflicts occurred, what memory was superseded, and what remains unresolved.

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
| Phase 5-D | PackChat-to-OB1 candidate capture and retrieval smoke; Calvin promotion contract smoke |
| Phase 5b | FRAMES re-run (OB1 default recall), LongMemEval / LoCoMo regression check |
| Phase 6 | GAIA Level 3, AgentBench, TypeDB vs SQL causal recall comparison |
| Phase 7 | E-CARE, causal attribution accuracy (top-1 / top-3) |
| Phase 8 | D* drift score, SSA baseline, quarantine resolution quality, schema revision stability |
| Phase 10 | Spec adherence rate, hidden scenario delta, DevBench |

### Always-on benchmarks

- `Local Regression Gate` — hard merge gate, runs on every substantial change
- `LongMemEval` paired mode (Coobie vs raw LLM) — run on every memory-relevant change
- `LoCoMo QA` paired mode — longer-horizon memory regression check

### Competitive positioning benchmarks

#### vs Mem0 / MindPalace / Zep

- `FRAMES` — multi-hop factual recall; Mem0 publishes here. Native adapter live. Re-run after OB1 becomes the default shared recall path and compare against the local-vector baseline.
- `StreamingQA` — belief-update accuracy; no competitor tracks this. Phase v1-B.
- `HELMET` — retrieval precision/recall. Native adapter live.
- `LongMemEval` — long-term assistant memory. Native adapter live.
- `LoCoMo QA` — long-horizon dialogue memory. Native adapter live.

#### vs OpenCode / Aider / single-agent coding tools

- `LiveCodeBench` — recent competitive programming problems; contamination-resistant. Phase 2.
- `Aider Polyglot` — Aider's own multi-language leaderboard. Phase 2.
- `DevBench` — full software lifecycle; structural argument against single-phase tools. Phase 10.
- `SWE-bench Verified` / `SWE-bench Pro` — industry-standard code loop benchmarks. Phase 2.

#### vs general agent frameworks

- `GAIA Level 3` — multi-step delegation; single-agent tools fail here. Phase 6.
- `AgentBench` — eight environments; tests Labrador role separation. Phase 6.

#### Causal reasoning — unique claim, no competitor benchmarks this

- `CLADDER` — Pearl hierarchy accuracy. Native adapter live.
- `E-CARE` — causal explanation coherence. Phase 7.

#### Harkonnen-native — cannot be run by any competitor

- `Spec Adherence Rate` — completeness and precision vs spec. Phase 10.
- `Hidden Scenario Delta` — visible vs hidden pass rate gap. Phase 10.
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
- Semantic memory (Open Brain / OB1 by default; fastembed or OpenAI-compatible embeddings + SQLite vector store optional)
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
- Native StreamingQA adapter (query-time invalidation reasons plus persisted-history smoke published on `lm-studio-local`)
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

**Phase 4b — Memory Invalidation (query-time layer shipped; persistence layer now live on the main ingest path):**

- `invalidation_reasons` field on `MemoryRetrievalHit` — computed at retrieval time from `superseded_by` / `challenged_by` provenance fields
- `memory_invalidation_reasons()` helper in orchestrator surfaces reasons per hit
- Persistence layer (`memory_updates` table, `invalidated_by` / `superseded_by` provenance, `GET /api/memory/updates`, Memory Board panel) is live; StreamingQA persisted-history smoke is published, and the operator confirm/reject loop is now part of the shipped path

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
