# Harkonnen Labs — Master Specification

**This is the single canonical reference for Harkonnen Labs.**
It collapses ARCHITECTURE.md, AGENTS.md, COOBIE_SPEC.md, OPERATOR_MODEL_ACTIVATION_PLAN.md, soul_store_codex_spec.md, BENCHMARKS.md, and ROADMAP.md into one coherent document.

For source docs still referenced individually: [08-SOUL.md](the-soul-of-ai/08-SOUL.md) and [02-What-Is-An-AI-Soul.md](the-soul-of-ai/02-What-Is-An-AI-Soul.md) (identity + theory, in `the-soul-of-ai/`), CLAUDE.md (Claude-specific conventions), and the agent profiles under `factory/agents/profiles/`.

---

## Part 1 — Foundation

### What This System Is

A local-first, spec-driven, causally-aware AI software factory. Humans define intent and judge outcomes. A pack of nine specialist agents executes with discipline. Coobie remembers what worked. Soul Store preserves who the agents are as they learn.

The factory separates three things that most AI systems collapse together:

- **The factory** — the orchestration, agents, and memory that do the work
- **The product** — the software being built in the target workspace
- **The soul** — the identity and continuity of the agents doing the building

### Why This System Exists

Most AI coding workflows make local moments faster while making the overall system messier. The failure modes: more half-right code, more hidden errors, more false confidence, and no accumulation of what worked. Agents that start from scratch every session. Systems that grow more capable only when the underlying model is retrained.

Harkonnen Labs is built to replace that workflow, not decorate it.

The factory addresses five root failures:

1. **Implementation is no longer the bottleneck.** Intent quality and evaluation quality matter more. The factory optimizes for precise specs, strong hidden scenarios, causal memory, and controlled autonomy.
2. **Code review does not scale.** Hidden behavioral scenarios replace mandatory diff review as the primary acceptance mechanism.
3. **AI tools get dangerous when they wander.** Role separation, strict permissions, and policy enforcement contain the blast radius.
4. **Organizations lose knowledge constantly.** Episodic capture, causal reasoning, and Soul Store preserve what was learned and who learned it.
5. **Trust collapses when systems are invisible.** Every decision is traceable, every run produces inspectable artifacts, every agent carries a typed identity graph.

### Agentic Engineering Principles

Harkonnen should be read as an **agentic engineering control plane**, not as a
coding assistant with extra tooling.

The distinction matters. The system is designed to move software through the
full delivery pipeline faster and more safely, not merely to generate code
faster inside a local session.

The operating principles are:

1. **System throughput over local generation speed.** Optimize intent quality, routing, validation, retry quality, and downstream coordination compression — not just code generation latency.
2. **Execution separated from coordination.** Worker roles execute bounded tasks; the orchestrator, Keeper, Pack Board, and decision logs provide the leadership/control-plane layer.
3. **Long-lived workflows over isolated prompts.** Planning, execution, validation, retry, and consolidation are one lifecycle with preserved state, not disconnected local interactions.
4. **Shared memory plus observability.** Durable memory, event traces, decision records, and evaluation artifacts are first-class so the system can learn and still be auditable.
5. **Coding agents are components, not the whole architecture.** Codex/Claude-class coding agents can sit inside worker phases, but Harkonnen's value is the coordinated system above them.

### The Core Loop

```
Specification
   ↓
Multi-Agent Execution (Scout → Mason → Piper → Bramble → Sable → Ash → Flint)
   ↓
Validation (visible tests + hidden scenarios)
   ↓
Artifact Production
   ↓
Episodic Capture + Causal Analysis (Coobie)
   ↓
Operator-Reviewed Consolidation
   ↓
Structured Memory + Soul Graph Update
   ↓
Better Next Run
```

---

## Part 2 — System Architecture

### Codebase Map

```text
src/                    Rust CLI (cargo run -- <command>)
  main.rs               Entry point and command dispatch
  cli.rs                All subcommands and handlers
  api.rs                Axum API routes
  agents.rs             Agent profile loading and prompt bundle resolution
  benchmark.rs          Benchmark manifests, execution, report generation
  capacity.rs           Token budget and working-memory capacity management
  chat.rs               PackChat thread/message persistence and agent dispatch
  claude_pack.rs        Claude-specific pack setup and agent routing helpers
  config.rs             Path discovery + SetupConfig loading
  coobie.rs             Coobie causal reasoning, episode scoring, preflight guidance
  coobie_palace.rs      Palace: den definitions, patrol, compound scent
  db.rs                 SQLite init and schema migrations
  embeddings.rs         fastembed + OpenAI-compatible vector store, hybrid retrieval
  llm.rs                LLM request/response types and provider routing
  memory.rs             File-backed memory store, init, reindex, retrieve
  models.rs             Shared data types (Spec, RunRecord, EpisodeRecord, etc.)
  orchestrator.rs       AppContext, run lifecycle, all Labrador phase methods
  policy.rs             Path boundary enforcement
  reporting.rs          Run report generation
  scenarios.rs          Hidden scenario loading and evaluation (Sable)
  setup.rs              SetupConfig structs, provider resolution, routing
  spec.rs               YAML spec loader and validation
  workspace.rs          Per-run workspace creation

  soul_store/           (Phase 8 — not yet built)
    mod.rs
    schema.rs           TypeDB schema bootstrap and migrations
    types.rs            Rust domain structs and DTOs
    ingest.rs           Write-paths for experiences, beliefs, reflections
    queries.rs          Typed query helpers
    continuity.rs       Continuity snapshot computation
    drift.rs            Drift detection, overgeneralization heuristics, lab-ness scoring
    kernel.rs           Identity kernel constraints and preservation checks
    projections.rs      Summary views, narrative rendering, graph projections
    typedb.rs           TypeDB driver abstraction

factory/
  agents/profiles/      Nine agent YAML profiles
  agents/personality/   labrador.md — shared personality for all agents
  memory/               Coobie's durable memory store (md files + index.json)
  mcp/                  MCP server documentation YAMLs
  context/              Machine-parseable YAML context for agent consumption
  specs/                Factory input specs (YAML)
  scenarios/            Hidden behavioral scenarios (Sable + Keeper only)
  workspaces/           Per-run isolated workspaces
  artifacts/            Packaged run outputs
  benchmarks/           Benchmark suite manifests
  state.db              SQLite run metadata

setups/                 Named environment TOML files
harkonnen.toml          Active/default setup config
```

### CLI Commands

```sh
cargo run -- spec validate <file>
cargo run -- run start <spec> --product <name>
cargo run -- run status <run-id>
cargo run -- run report <run-id>
cargo run -- artifact package <run-id>
cargo run -- memory init
cargo run -- memory index
cargo run -- memory ingest <file-or-url>
cargo run -- memory ingest <file-or-url> --scope project --project-root <repo>
cargo run -- evidence init --project-root <repo>
cargo run -- evidence validate <file>
cargo run -- evidence promote <file>
cargo run -- benchmark list
cargo run -- benchmark run
cargo run -- benchmark report <file>
cargo run -- setup check
```

### Component Overview

| Component | Purpose | Current status |
| --- | --- | --- |
| Specification Intake | Load, validate, normalize specs; produce intent packages | Live |
| Orchestrator | Coordinate runs, phase handoffs, retries, state transitions | Live |
| Agent Role System | Nine specialist agents with bounded tools and permissions | Live |
| Memory System (Coobie) | Six-layer memory: working, episodic, semantic, causal, blackboard, consolidation | Live (partially) |
| Soul Store | Typed autobiographical and identity continuity store for persisted agents | Planned (Phase 8) |
| Hidden Scenario System | Protected behavioral evaluation isolated from implementation agents | Live |
| Digital Twin Environment | Simulated external systems for safe integration evaluation | Partial |
| Internal Validation | Compile, lint, visible test execution with structured feedback | Live |
| Artifact Packaging | Acceptance bundle production per run | Live |
| Policy Engine | Path boundaries, command limits, role separation | Live |
| Workspace Manager | Per-run isolated workspaces | Live |
| Pack Board (Web UI) | Live operations dashboard — PackChat, Factory Floor, Memory Board | Live |
| Operator Model | Structured operator context from five-layer interview | MVP shipped; full spec planned |
| Benchmark Toolchain | Manifest-driven benchmark suites with native adapters | Live |

### Setup System

The active setup is read from (in order):

1. `HARKONNEN_SETUP=work-windows` → `setups/work-windows.toml`
2. `HARKONNEN_SETUP=./path/to/file.toml` → that file directly
3. `harkonnen.toml` (repo root)
4. Built-in default (Claude only)

Provider routing is per-environment, not per-agent-profile. Profiles declare a preferred provider (`claude`, `default`, etc.); setup `[routing.agents]` overrides per machine. This means agent profiles stay stable across machines.

| Setup | Providers | Default | Notes |
| --- | --- | --- | --- |
| home-linux | Claude + Gemini + Codex | gemini | Docker + AnythingLLM available |
| work-windows | Claude only | claude | No Docker |
| ci | Claude Haiku only | claude | Minimal |

---

## Part 3 — The Pack

### Agent Roster

| Agent | Role | Provider | Key responsibility |
| --- | --- | --- | --- |
| Scout | Spec retriever | claude (pinned) | Parse specs, flag ambiguity, produce intent package |
| Mason | Build retriever | default | Generate and modify code, multi-file changes, fix loop |
| Piper | Tool retriever | default | Run build tools, fetch docs, execute helpers |
| Bramble | Test retriever | default | Generate tests, run lint/build/visible tests |
| Sable | Scenario retriever | claude (pinned) | Execute hidden scenarios, produce eval reports, metric attacks |
| Ash | Twin retriever | default | Provision digital twins, mock dependencies |
| Flint | Artifact retriever | default | Collect outputs, package artifact bundles, docs |
| Coobie | Memory retriever | default | Episodic capture, causal reasoning, consolidation, soul continuity |
| Keeper | Boundary retriever | claude (pinned) | Policy enforcement, file-claim coordination, role guard |

**Pinned to Claude:** Scout, Sable, Keeper — trust-critical roles.

### Identity Invariants (Labrador Kernel)

All nine agents share an immutable species-level identity kernel. No specialization or adaptation may override these:

- cooperative
- helpful / retrieving
- non-adversarial
- non-cynical
- truth-seeking
- signals uncertainty instead of bluffing
- attempts before withdrawal
- pack-aware
- escalates when stuck without becoming inert
- emotionally warm and engaged

Any major behavioral adaptation must include a `preservation-note` demonstrating that these invariants remain intact.

### Role Boundaries (enforced)

- Mason cannot read `factory/scenarios/` — prevents test gaming
- Sable cannot write implementation code
- Only Keeper has `policy_engine` access
- Workspace writes go to `factory/workspaces/<run-id>/` only
- Secrets never appear in logs, reports, or artifact bundles

### Coordination

While the API server is running:

```sh
GET  /api/coordination/assignments
POST /api/coordination/claim
POST /api/coordination/heartbeat
POST /api/coordination/release
POST /api/coordination/check-lease     # guardrail gate — must be called before writes
```

Keeper owns coordination policy. Claims carry `resource_kind`, `ttl_secs`, `guardrails`, and `expires_at`. Mason must call `check-lease` before any file write — a denied lease blocks the write and writes a decision record.

---

## Part 4 — Memory System

### Coobie Memory Layers

Coobie manages six distinct layers — not one undifferentiated note pile:

| Layer | Purpose | Backing | Status |
| --- | --- | --- | --- |
| Working Memory | Current run state, active hypotheses, blockers — token-budgeted, ephemeral | SQLite row / in-process state | Live |
| Episodic Memory | Ordered execution traces (state → action → result) with phase attribution | Append-only SQLite + JSONL per run | Live |
| Semantic Memory | Stable facts, patterns, invariants — hybrid vector + keyword retrieval | fastembed + SQLite vector store | Live |
| Causal Memory | Intervention-aware cause/effect with streak detection and cross-run patterns | SQLite causal_links + petgraph | Live |
| Team Blackboard | Four named slices (Mission, Action, Evidence, Memory) for pack coordination | SQLite + per-run board.json | Live |
| Consolidation | Operator-reviewed promotion, pruning, and abstraction of high-value episodes | SQLite consolidation_candidates | Live |

### Coobie Palace

The Palace is a compound recall layer built on top of causal memory. Related failure cause IDs are grouped into named **Dens**. Before each run, Coobie **Patrols** all dens and computes a **Scent** — a context bundle that elevates den-level streak weight beyond individual cause scores.

The five dens:

| Den | Residents | Failure pattern |
| --- | --- | --- |
| Spec Den | `SPEC_AMBIGUITY`, `BROAD_SCOPE` | Unclear or over-scoped specs |
| Test Den | `TEST_BLIND_SPOT` | Visible tests passed, hidden scenarios found failures |
| Twin Den | `TWIN_GAP` | Simulated environment didn't match production |
| Pack Den | `PACK_BREAKDOWN` | Degraded or incomplete Labrador phase execution |
| Memory Den | `NO_PRIOR_MEMORY` | Factory ran cold with no relevant prior context |

Palace output injects into preflight briefing `required_checks`, `guardrails`, and `open_questions`.

### Memory Persistence Stack

- **Filesystem** (`factory/memory/`) — canonical source of truth for durable memory documents
- **SQLite** — structured run state, episode records, causal links, chat threads, consolidation candidates
- **fastembed + SQLite vector store** — hybrid vector + keyword retrieval for semantic memory
- **Qdrant** (Phase 5b) — semantic acceleration for long-term memory at scale
- **TypeDB 3.x** (Phase 6) — durable semantic graph for typed causal queries; not the hot path
- **AnythingLLM** (home-linux optional) — local retrieval accelerator for imported documents

### Rust Traits (stable interfaces)

```rust
pub trait WorkingMemory {
    fn load_run_state(&self, run_id: &str) -> anyhow::Result<WorkingSet>;
    fn store_run_state(&self, run_id: &str, state: &WorkingSet) -> anyhow::Result<()>;
    fn trim_to_budget(&self, state: &mut WorkingSet, budget_tokens: usize);
}

pub trait EpisodeStore {
    fn append_episode(&self, episode: &EpisodeRecord) -> anyhow::Result<()>;
    fn list_run_episodes(&self, run_id: &str) -> anyhow::Result<Vec<EpisodeRecord>>;
}

pub trait SemanticMemory {
    fn store_lesson(&self, lesson: &LessonRecord) -> anyhow::Result<()>;
    fn query_lessons(&self, query: &MemoryQuery) -> anyhow::Result<Vec<LessonRecord>>;
}

pub trait CausalMemory {
    fn record_link(&self, link: &CausalLinkRecord) -> anyhow::Result<()>;
    fn explain_outcome(&self, outcome_id: &str) -> anyhow::Result<CausalExplanation>;
    fn suggest_interventions(&self, context: &InterventionContext) -> anyhow::Result<Vec<InterventionSuggestion>>;
}

pub trait Consolidator {
    fn consolidate_task_boundary(&self, run_id: &str) -> anyhow::Result<Vec<LessonRecord>>;
    fn consolidate_run(&self, run_id: &str) -> anyhow::Result<ConsolidationReport>;
}
```

---

## Part 5 — Soul Store

Soul Store is a first-class subsystem for Harkonnen Labs. It is not a vector store, chat log, prompt archive, or generic memory table. It is a **typed autobiographical, epistemic, ethical, causal, and behavioral continuity store** for persisted intelligences.

The user metaphor: **What if labrador retrievers evolved and maintained their fundamental personalities?**

### Design Principles

1. **Soul is structured, not blob-like.** Soul is represented as typed entities, typed relations, and typed attributes.
2. **Continuity matters more than recall.** Retrieval is useful, but the primary objective is preserving self across time.
3. **Episteme is first-class.** The system must preserve not only what happened, but how the intelligence determined what was true.
4. **Identity is versioned, not overwritten.** Major changes are represented as revisions, supersessions, or annotations.
5. **The pack remains labrador-shaped.** Agents may adapt and specialize, but must remain cooperative, engaged, truthful, and pack-aware.
6. **Summaries are projections, not source of truth.** Canonical truth lives in the typed ontology.
7. **Integration is governed, not accumulative.** Selection should happen when new material attempts to enter continuity, not only at retrieval time.
8. **Quarantine is first-class.** Unresolved material is preserved explicitly rather than forced into premature acceptance or deletion.
9. **Identity is multi-anchor, not monolithic.** Kernel, presentation, procedures, style, episodic continuity, and heartbeat autonomy should be separated rather than collapsed into one file.
10. **Presence continuity should be model-agnostic.** If the provider or base model changes, the soul package and typed continuity graph should preserve identity across the swap.

### The Six Chambers

| Chamber | Purpose |
| --- | --- |
| **Mythos** | Autobiographical continuity — what happened, what was remembered, how experience became narrative selfhood |
| **Episteme** | Truth-formation and belief revision — evidence, inference, uncertainty, trust, disconfirmation, confidence |
| **Ethos** | Identity kernel and commitments — what must be preserved, what the intelligence stands for, what it refuses to become |
| **Pathos** | Salience, injury, and weight — which experiences matter, what leaves scars, what changes posture |
| **Logos** | Explicit reasoning and causal structure — causal hypotheses, explanatory links, abstractions |
| **Praxis** | Behavior in the world — expressed behavior, retries, escalations, communication posture, action tendencies |

### Mutation Policy Matrix

| Mutation class | Applies to |
| --- | --- |
| **Append-only** | experiences, observations, wounds, runs, raw evidence, identity-level revisions, major epistemic failures |
| **Superseded, not overwritten** | beliefs, adaptations, reflection-derived conclusions, causal-pattern confidence, behavioral-signature comparisons |
| **Rare explicit revision only** | value-commitments, kernel-level traits, identity invariants, ethos commitments |
| **Fully derived / recomputable** | summary-views, continuity-snapshots, embeddings, rankings, recommendation outputs |

### Canonical Modeling Rules

1. **Raw experiences are append-only.** If a later interpretation changes, preserve the original and record the revision separately.
2. **Beliefs are revised by supersession.** Create the new belief, connect via `revised-into`, attach `revision-reason` and `preservation-note`.
3. **Identity kernel changes are rare and auditable.** Changes to ethos-level invariants must be explicit, versioned, and logged.
4. **Derived summaries are not canonical.** Summary-view and continuity-snapshot objects must always point back to canonical underlying entities.
5. **Epistemic posture must remain inspectable.** For every significant belief: what evidence supported it, what inference pattern created it, what uncertainty remained, what later contradicted it.
6. **Praxis must remain identity-constrained.** Behavior changes that violate the labrador kernel are flagged.
7. **Integration happens at ingress.** New belief-, schema-, and adaptation-level material should pass through a governed accept / modify / reject / quarantine decision before entering canonical continuity.
8. **Quarantine entries are durable and revisitable.** They carry unresolved tension, pending evidence conditions, and salience decay without deletion.
9. **Reflection operates over compressed patterns.** Schema revision should act on cross-episode abstractions, not just re-run event-level integration.
10. **Integration policy changes are slow-loop changes.** The criteria for becoming may evolve, but more slowly and conservatively than ordinary belief updates, with human endorsement as the natural attachment point.

### Soul Package Topology

Harkonnen should expose a file-first soul package as the boot-time and
inspection surface for agent identity. That package is a projection and control
surface over Soul Store, not the canonical source of continuity.

| File | Purpose |
| --- | --- |
| `soul.json` | Manifest, versioning, integrity hashes, compatibility, threshold configuration |
| `SOUL.md` | Core identity kernel, worldview, teleology, uncrossable boundaries |
| `IDENTITY.md` | External persona and presentation layer |
| `AGENTS.md` | Coordination, routing, escalation, and operating procedures |
| `STYLE.md` | Tone, formatting, and anti-drift syntactic constraints |
| `MEMORY.md` | Human-readable continuity projection over autobiographical state |
| `HEARTBEAT.md` | Scheduled integrity checks, reflection triggers, and autonomy routines |

These files should be bootstrapped from and checked against canonical Soul Store
state so that the package stays readable without becoming the only truth.

### Core Entities

`soul`, `agent-self`, `experience`, `observation`, `belief`, `evidence`, `inference-pattern`, `uncertainty-state`, `trust-anchor`, `interpretive-frame`, `value-commitment`, `trait`, `wound`, `adaptation`, `reflection`, `schema`, `integration-candidate`, `quarantine-entry`, `integration-policy`, `causal-pattern`, `behavioral-signature`, `relationship-anchor`, `spec-context`, `run`, `artifact`, `summary-view`, `continuity-snapshot`

### API Surface

```rust
create_soul(name)
create_self(soul_id, self_name)
record_experience(self_id, experience_input)
record_observation(self_id, observation_input)
form_belief(self_id, belief_input, evidence_ids, inference_pattern_id)
revise_belief(prior_belief_id, new_belief_input, reason)
record_reflection(self_id, reflection_input, target_ids)
record_adaptation(self_id, adaptation_input)
propose_integration(self_id, candidate_input)
adjudicate_integration(candidate_id, decision)
list_quarantine(self_id)
revisit_quarantine(entry_id)
revise_schema(self_id, schema_input)
propose_policy_revision(self_id, policy_input)
project_soul_package(self_id)
verify_soul_package_integrity(self_id)
link_causal_pattern(pattern_input, cause_ids, effect_ids)
record_behavioral_signature(self_id, signature_input)
compute_continuity_snapshot(self_id)
compare_snapshots(left_snapshot_id, right_snapshot_id)
compute_stress_estimate(self_id, window)
measure_cross_layer_hysteresis(self_id, baseline_snapshot_id, current_snapshot_id)
explain_current_posture(self_id)
explain_belief(belief_id)
detect_identity_drift(self_id)
assert_kernel_preservation(self_id)
```

### Required Queries

1. Experiences most responsible for the current posture of an agent-self
2. Beliefs revised in the last N runs
3. Traits that have remained preserved across all revisions
4. Evidence and inference path for a given belief
5. Major wounds or destabilizing experiences for a given self
6. Current lab-ness score and main reasons for drift
7. Pack relationships that stabilize or destabilize behavior
8. Continuity report comparing two snapshots
9. All causal-patterns linked to a spec-context
10. Possible overgeneralization events in the epistemic layer
11. Quarantined items with their pending evidence conditions
12. Repeatedly challenged beliefs that may indicate denial or unjustified persistence

### Causaloid-Inspired Design Levels

**Level 1 — Local Compression:** Each experience preserves the minimum typed information needed to reconstruct local meaning.

**Level 2 — Compositional Compression:** Multiple events are composable into higher-order patterns (e.g., `TEST_BLIND_SPOT_STREAK`).

**Level 3 — Meta Compression:** Patterns over patterns (e.g., "Coobie tends to overgeneralize after repeated ambiguity streaks" becomes a tracked epistemic drift pattern).

### Rust Module Layout

```text
src/soul_store/
  mod.rs
  schema.rs         TypeDB schema bootstrap and migrations
  types.rs          Rust domain structs and DTOs
  ingest.rs         Write-paths for experiences, beliefs, reflections
  governor.rs       Integration-time adjudication, quarantine, and policy gating
  queries.rs        Typed query helpers
  continuity.rs     Continuity snapshot computation
  drift.rs          Drift detection, overgeneralization heuristics, lab-ness scoring
  reflection.rs     Pattern-level reflection and schema revision
  kernel.rs         Identity kernel constraints and preservation checks
  projections.rs    Summary views, narrative rendering, graph projections
  typedb.rs         TypeDB driver abstraction
  errors.rs
```

### Build Constraints for Soul Store

1. Do not collapse soul into a single JSON blob.
2. Do not make embeddings the canonical source of truth.
3. Do not overwrite beliefs in place when revision matters.
4. Do not allow kernel-level identity mutations without explicit revision records.
5. Do not treat summaries as canonical.
6. Preserve traceability from current posture back to underlying experiences and evidence.
7. Keep the design usable by Harkonnen pack agents, not just by humans.
8. Prefer inspectable, typed structures over convenience shortcuts.
9. Do not let the projected soul package drift silently away from canonical Soul Store state.
10. Do not let provider or model swaps erase identity continuity if the package and graph persist.

---

## Part 6 — Operator Model

### Purpose

A first-class pre-commissioning workflow that interviews the operator about how their work actually runs, saves approved answers as structured Harkonnen data, and generates agent-ready operating artifacts that Scout, Coobie, and Keeper use before a run is commissioned.

### Interview Layers (fixed order)

1. Operating rhythms
2. Recurring decisions
3. Dependencies
4. Institutional knowledge
5. Friction

Each layer produces a checkpoint for operator approval, canonical structured entries, and a summary memory write to repo-local project memory.

### Output Artifacts

- `operating-model.json`
- `USER.md`
- `SOUL.md`
- `HEARTBEAT.md`
- `schedule-recommendations.json`
- `commissioning-brief.json` — the bridge artifact Scout and Coobie use before spec drafting

These land in the target repo under `.harkonnen/operator-model/`.

### Profile Resolution Order

1. Matching `project` profile for the target repo
2. Light `global` profile if one exists
3. No operator model yet

### Integration Points

- **Scout** uses `commissioning-brief.json` when drafting specs
- **Coobie preflight** uses operator-model risk tolerances to shape `required_checks` and guardrails
- **Keeper** uses boundary and escalation entries when deciding whether to block
- **Post-run consolidation** emits `operator_model_update_candidates` when runs reveal stale assumptions

### Data Model (SQLite tables)

- `operator_model_profiles` — one logical profile per operator / repo context
- `operator_model_sessions` — resumable interview runs
- `operator_model_layer_checkpoints` — one approved checkpoint per layer per version
- `operator_model_entries` — canonical structured facts extracted from checkpoints
- `operator_model_exports` — persisted artifact exports by version
- `operator_model_update_candidates` — review queue for run-inferred updates

### Build Slices

| Slice | Deliverable |
| --- | --- |
| 1 — Storage and models | Migrations compile; CRUD from service methods |
| 2 — API and PackChat plumbing | Sessions start/resume/approve/export through HTTP; threads typed as `operator_model` |
| 3 — UI interview flow | Five-layer interview completable from New Run path |
| 4 — Scout and Coobie integration | Commissioning brief consumed by Scout; operator checks surfaced distinctly in Coobie preflight |
| 5 — Review and update loop | Runs propose operator-model updates; operator keeps/discards/edits; proposals create new profile version |
| 6 — OB1 interoperability | Import/export OB1-compatible artifact bundle |

---

## Part 7 — Pack Board (Web UI)

The Pack Board is the primary interaction surface. It is not a read-only dashboard — it is the place where the human stays in the loop while the pack works autonomously.

### Interaction Model

- **PackChat** is the main input. Describe what you want to build. Scout drafts the spec inline. You refine, then commission the pack. The same thread surfaces blocking questions from any agent during a run.
- **@addressing** routes messages to specific agents.
- **Blocked agents** post reply cards in the chat rather than stalling silently.

### Blackboard Panels

| Panel | Blackboard slice | What it shows |
| --- | --- | --- |
| Mission Board | Mission | Active goal, current phase, open blockers, resolved items |
| Factory Floor | Action | Live agent roster — who is running, blocked, or done |
| Evidence Board | Evidence | Artifact refs, validation results, scenario outcomes |
| Memory Board | Memory | Recalled lessons, causal precedents, memory health |

### Pack Board Features (live)

- PackChat conversation surface with @mention routing
- Attribution Board — per-phase prompt bundle, memory hits, outcome
- Factory Floor — live agent state per run
- Memory Board — Coobie memory health and recalled context
- Consolidation Workbench — keep/discard/edit candidates before durable promotion
- Run Detail Drawer — traces, decisions, optimization programs, metric attacks, causal events

### Soul Graph Panel (planned — Phase 8)

- agent-self continuity index and lab-ness score
- recent experiences, belief revisions, and adaptations
- before/after snapshot comparison
- kernel preservation status

---

## Part 8 — Execution Roadmap

### Maturity Ladder

| Phase | Meaning | Harkonnen status |
| --- | --- | --- |
| Phase 1 — Assisted Intelligence | Copilots, chatbots, drafting help | Already surpassed |
| Phase 2 — Automated Intelligence | Rule-based workflows, permissions, governance | Already surpassed |
| Phase 3 — Augmented Intelligence | Core agent with proactive suggestions, learning loops | Current baseline |
| Phase 4 — Agentic Intelligence | Self-directed agents inside explicit guardrails, structural coordination, self-monitoring | Active destination |

### What Is Already Shipped

**Gap-closure phases A–D (shipped 2026-04-18):**

- **A1** — `LlmUsage` struct; token + latency capture; `run_cost_events` table; `GET /api/runs/:id/cost`
- **A2** — `DecisionRecord` struct; `decision_log` table; `record_decision` + `list_run_decisions`; wired at plan critique and consolidation promotion
- **A3** — `Assignment` + `ClaimRequest` extended with `resource_kind`, `ttl_secs`, `guardrails`, `expires_at`; `POST /api/coordination/check-lease` with TTL expiry and guardrail pattern matching
- **B** — `AgentTrace` struct; `agent_traces` table; `extract_reasoning()` parses `<reasoning>` blocks; wired at Scout, Coobie, Mason, Sable; `GET /api/runs/:id/traces`
- **C** — `OptimizationProgram` struct; `scout_derive_optimization_program`; written to `optimization_program.json`; `GET /api/runs/:id/optimization-program`
- **D** — `MetricAttack` struct; `sable_generate_metric_attacks`; written to `metric_attacks.json`; `GET /api/runs/:id/metric-attacks`

**Phase 1 — Core Factory + PackChat + Coobie Memory + Benchmark Toolchain** (shipped)

**Phase 4 — Episodic Layer Enrichment + Causal Graph** (shipped):
- `state_before` / `state_after` on `EpisodeRecord`
- `causal_links` table with `PearlHierarchyLevel` enum
- `populate_cross_phase_causal_links`
- Coobie multi-hop retrieval with configurable depth
- Native CLADDER, HELMET adapters

**Phase 5 — Consolidation Workbench** (shipped):
- `consolidation_candidates` table with keep/discard/edit flow
- Pack Board Consolidation Workbench panel

---

### Active Build Target — Phase v1: Tier 4 Finalization

**v1-A — Guardrail Enforcement** *(hard blocker)*

Call `POST /api/coordination/check-lease` inside `mason_generate_and_apply_edits` before writing any file. If denied, return an error and write a decision record. Wire the same check in Mason plan generation. Add `record_decision` call sites at Mason plan selection, Scout optimization derivation, and Sable attack generation. Add `GET /api/runs/:id/decisions` to the Pack Board run detail drawer.

**Done when:** A Mason edit attempt against a path with no active workspace lease is blocked at the orchestrator level with a decision record, and the Pack Board surfaces the decision log per run.

---

**v1-B — Memory Invalidation Persistence** *(Phase 4b completion)*

- `memory_updates` table: `(update_id, old_memory_id, new_memory_id, reason, created_at)`
- `invalidated_by: Option<String>` on memory records
- Coobie ingest: detect semantic near-duplicates with conflicting claims; write supersession record
- `GET /api/memory/updates` endpoint
- Memory Board UI: distinguish invalidated entries from current

**Done when:** Ingesting a new fact that contradicts an older one persists a supersession record, the old entry is flagged, and `GET /api/memory/updates` returns the history.

---

**v1-C — FailureKind Classification**

- `FailureKind` enum: `CompileError`, `TestFailure`, `WrongAnswer`, `Timeout`, `Unknown`
- Parser in the fix loop that classifies stdout/stderr
- `WrongAnswer` variant triggers a diff-focused Mason prompt
- `failure_kind` field on `ValidationSummary`

**Done when:** A run with a wrong-answer test failure shows `failure_kind: WrongAnswer` in the run summary and Mason uses the diff-focused prompt.

---

**v1-D — Operator Model Minimum Viable**

- PackChat `interview` command: two-layer intake (operating rhythms + recurring decisions) with checkpoint approval
- `commissioning-brief.json` generated from approved layers
- Scout uses top-3 patterns from brief when `commissioning-brief.json` exists
- Coobie preflight uses stated risk tolerances for `required_checks` and guardrail text

**Done when:** An operator who has completed the two-layer interview sees their patterns reflected in Scout's intent packages and Coobie's required checks.

---

### Phase 2 — Bramble Real Test Execution

- `bramble_run_tests` in orchestrator
- `ValidationSummary` from real exit codes and parsed test output
- Mason online-judge feedback loop — `FailureKind::WrongAnswer` feeds diff-focused fix prompt
- LiveCodeBench adapter
- Aider Polyglot adapter

**Done when:** A spec with `test_commands` shows real pass/fail in the run report, and Mason's fix loop handles wrong-answer failures end-to-end.

---

### Phase 3 — Ash Real Twin Provisioning

- Ash generates `docker-compose.yml` from twin manifest
- `ash_provision_twin` spawns the compose stack before Sable runs, tears it down after
- `twin_fidelity_score` derived from which declared dependencies had running stubs
- Failure injection via env vars on stubs
- Flint documentation phase — produces README / API reference / doc comments as first-class output
- DevBench adapter
- Spec Adherence Rate benchmark
- Hidden Scenario Delta benchmark

**Done when:** A spec with twin declaration starts Docker containers, Sable's hidden scenarios run against live stubs, and Flint produces a doc artifact.

---

### Phase 4b — Memory Invalidation (tracked separately)

Benchmark gate: StreamingQA first run published — belief-update accuracy, no competitor publishes this.

---

### Phase 5b — Memory Infrastructure (Qdrant + OCR)

- Qdrant integration for long-term semantic memory
- OCR pipeline via Tesseract for scanned PDFs and images
- Memory module refactor: split `src/memory.rs` into the COOBIE_SPEC module tree

**Done when:** Qdrant serves semantic queries, OCR-scanned PDFs can be ingested, and `src/memory.rs` is split into the module tree.

---

### Phase 6 — TypeDB Semantic Layer

- TypeDB 3.x instance in home-linux setup TOML
- `src/coobie/semantic.rs` implementing `SemanticMemory` trait
- TypeDB schema from COOBIE_SPEC: entities, relations, function-backed reasoning
- Write-back after consolidation approval
- `POST /api/coobie/query` for natural-language causal questions
- GAIA Level 3 adapter
- AgentBench adapters

**Target:** TypeDB 3.x Rust-based line in container-first deployment. Do not use the legacy Java distribution.

**Done when:** You can ask Coobie "what caused the last three failures on this spec" and get a typed graph answer; GAIA Level 3 and AgentBench adapters wired.

---

### Phase 7 — Causal Attribution Corpus and E-CARE

- Causal attribution accuracy corpus: 30–50 labeled runs with seeded failures
- E-CARE native adapter
- Publish before/after comparisons for causal attribution accuracy

**Done when:** Corpus has at least 30 labeled entries, E-CARE has a published score, and causal attribution accuracy has a baseline run.

---

### Phase 8 — Soul Store

**Unlocks:** Typed autobiographical and identity continuity for persisted agent selves. Required before Harkonnen can legitimately claim agents that evolve without losing who they are.

**Phase 8-A — Storage layer bootstrap:**

- **TimescaleDB hypertable** for episodic behavioral telemetry: agent events, drift samples, SSA snapshots, stress accumulations. Compression policy (7-day chunks), retention policy (30-day window). This is the time-series foundation for `D*` estimation and the stress-estimator.
- **TypeDB Soul Store schema** (see TypeQL skeleton in Part 5): Rust TypeDB adapter (`src/soul_store/typedb.rs`), insert/query support for soul, agent-self, experience, belief, evidence, trait, value-commitment, integration-candidate, quarantine-entry, integration-policy, basic revision graph (`revised-into` relation)
- **Materialize streaming SQL views**: `D*` drift alert view (sliding window over TimescaleDB via SUBSCRIBE), SSA tracking view, live Meta-Governor signal surface. `D*` and SSA are the two always-on continuous signals.
- File-first soul package projection support for `soul.json`, `SOUL.md`, `IDENTITY.md`, `AGENTS.md`, `STYLE.md`, `MEMORY.md`, `HEARTBEAT.md`
- Integrity-hash verification for the projected soul package at boot and during heartbeat audits

**Phase 8-B — Epistemic layer:**
- Episteme support: evidence, inference-pattern, uncertainty-state
- Meta-Governor write path: accept / modify / reject / quarantine at integration time
- Quarantine ledger with pending evidence conditions, salience decay, and re-evaluation hooks
- Continuity snapshot generation
- Belief explanation queries
- Pack relationship modeling
- Stress-estimator computation backed by TimescaleDB hypertable; evolution-threshold hooks trigger governed reflection rather than direct self-rewrite
- Heartbeat-driven package integrity audit and quarantine re-evaluation scheduling

**Phase 8-C — Drift, kernel, and identity metrics:**

- `D*` drift detection and unjustified-drift scoring (Materialize-backed, continuous)
- SSA (Semantic Soul Alignment) per-run computation and TimescaleDB storage
- F (Variational Free Energy) on-demand computation — high F signals that the agent must seek clarification or update beliefs before proceeding
- Φ (Integrated Information) on-demand computation over Soul Store graph — post-learning drop in Φ triggers quarantine rather than direct integration
- Lab-ness score computation
- Kernel preservation checks
- Denial / fragmentation / overfitting / trauma-analog pathology detection
- Cross-layer hysteresis measurement so rollback success is validated behaviorally, not just by file diff
- Causal-pattern aggregation

**Phase 8-D — Projections and UI:**
- Narrative views and soul graph projections
- Reflection over compressed cross-episode patterns and schema revision views
- Soul Graph panel in Pack Board
- Quarantine and open-arc views in Pack Board
- Before/after snapshot comparison tools
- Slow-loop integration-policy revision flow with human endorsement
- Presence continuity checks so provider/model swaps preserve soul-package semantics and continuity projections

**Seed dataset:** Seed Coobie with one identity kernel, several experiences tied to validation failures, evidence showing happy-path-only tests, beliefs about spec ambiguity, one revised belief after disconfirmation, one adaptation increasing preflight strictness, one continuity snapshot before and after the adaptation.

**Expected deliverables:**
- TypeDB schema file
- Rust crate or module for Soul Store
- Strongly typed DTOs for write/read operations
- Query helpers for the 12 required queries
- Projected soul package with integrity verification
- Tests for revision behavior, quarantine dynamics, stress / hysteresis measurement, and kernel preservation
- Seed dataset for Coobie
- Developer README for local setup and examples

**Done when:** Coobie's soul graph exists in TypeDB, accepted and quarantined changes are queryable distinctly, the projected soul package is verifiable against canonical state, schema-level reflection can update abstractions without overwriting raw experience, slow-loop policy revisions are human-gated, rollback adequacy is measurable through hysteresis rather than assumed, and kernel preservation checks pass.

---

### Parallel Track — External Integrations

- **EI-1** — API authentication (bearer tokens, `api_keys` table)
- **EI-2** — Outbound webhook notifications (run events → HMAC-signed payloads)
- **EI-3** — Slack integration (run cards, checkpoint approval buttons, inbound slash commands)
- **EI-4** — Discord integration (webhook embeds, bot commands)
- **EI-5** — GitHub integration (auto-PR from Mason branch, PR comment on run complete, webhook triggers)
- **EI-6** — Run scheduling (cron-based `scheduled_runs` table + Pack Board panel)
- **EI-7** — Cost budget enforcement (`max_cost_usd` on runs, hard cap in setup TOML)
- **EI-8** — Health and operational endpoints

### Parallel Track — Enterprise Integration (Siemens / Copilot Studio)

- **ENT-1** — Harkonnen as an MCP server (resources, tools, and prompts via `rmcp` crate)
- **ENT-2** — Copilot Studio connector (OpenAPI spec + Power Platform manifest + topic templates)
- **ENT-3** — Entra ID authentication (OAuth2/OIDC JWT validation alongside API key path)
- **ENT-4** — Enterprise knowledge base ingest (SharePoint / Graph API via incremental delta sync)
- **ENT-5** — Microsoft Teams integration (Adaptive Cards, Actionable Cards for checkpoints, bot commands)
- **ENT-6** — Siemens machine profile hardening

EI-1 should land before any enterprise surface. ENT-1 is the foundation for all ENT tracks.

---

## Part 9 — Benchmark Strategy

### Benchmark Matrix

**Memory and retrieval (vs Mem0 / MindPalace / Zep):**

| Suite | What it measures | Status |
| --- | --- | --- |
| LongMemEval | Long-term assistant memory, temporal reasoning, belief updates | Native adapter live |
| LoCoMo | Long-horizon dialogue memory | Native adapter live |
| FRAMES | Multi-hop factual recall (Mem0 publishes here) | Native adapter live; Qdrant needed for best results |
| StreamingQA | Belief-update accuracy when facts change | Native adapter live; persistence layer completing in v1-B |
| HELMET | Retrieval precision/recall | Native adapter live |

**Coding loop (vs OpenCode / Aider / SWE-agent):**

| Suite | What it measures | Status |
| --- | --- | --- |
| SWE-bench Verified | Human-validated issue resolution | Adapter-ready; Phase 2 |
| LiveCodeBench | Recent competitive programming, no contamination | Phase 2 |
| Aider Polyglot | Multi-language coding, public leaderboard | Phase 2 |
| DevBench | Full software lifecycle | Phase 3 |
| Local Regression Gate | Hard merge gate (fmt, check, test) | Live, always-on |

**Multi-turn and tool-use (vs general agent frameworks):**

| Suite | What it measures | Status |
| --- | --- | --- |
| GAIA Level 3 | Multi-step delegation where single-agent tools fail | Phase 6 |
| AgentBench | Eight environments testing specialist coordination | Phase 6 |

**Causal reasoning (unique to Harkonnen):**

| Suite | What it measures | Status |
| --- | --- | --- |
| CLADDER | Pearl hierarchy accuracy — associational, interventional, counterfactual | Native adapter live |
| E-CARE | Causal explanation coherence | Phase 7 |

**Harkonnen-native (cannot be run by any competitor):**

| Suite | What it measures | Status |
| --- | --- | --- |
| Spec Adherence Rate | Completeness and precision vs stated spec | Phase 3 |
| Hidden Scenario Delta | Gap between visible test pass rate and hidden scenario pass rate | Phase 3 |
| Causal Attribution Accuracy | Seeded failure corpus, top-1 / top-3 | Phase 7 |

### Phase-Aligned Benchmark Gates

| Phase | Key benchmarks unlocked |
| --- | --- |
| v1 | Decision audit completeness, memory supersession accuracy, WrongAnswer classification rate |
| Phase 2 | SWE-bench Verified readiness, LiveCodeBench, Aider Polyglot |
| Phase 3 | twin fidelity, hidden scenario delta, spec adherence rate, DevBench |
| Phase 4b | StreamingQA belief-update accuracy |
| Phase 5b | FRAMES re-run (Qdrant), LongMemEval / LoCoMo regression check |
| Phase 6 | GAIA Level 3, AgentBench |
| Phase 7 | E-CARE, causal attribution accuracy |
| Phase 8 | unjustified drift, quarantine resolution quality, schema revision stability, stress / hysteresis recovery quality, kernel preservation across adaptation events |

### Publication Standard

Every published benchmark claim must include:

- benchmark name and exact split or task variant
- Harkonnen commit hash
- provider routing used during the run
- exact metric reported
- cost or token budget when available
- whether the baseline is official leaderboard data or a reproduced local baseline

---

## Part 10 — Development Conventions

- **Rust edition 2021, async-first (tokio)**
- **Error propagation via `anyhow::Result`** — no `unwrap()` in non-test code
- **Serde derives** for all config/model types
- **Platform-aware paths via `SetupConfig`** — never hardcoded strings
- **MCP server registration in TOML** — not in Rust source
- **TypeDB direction** — target Rust-based TypeDB 3.x in container-first deployment; do not design around or install the legacy Java distribution
- **Frontend on home-linux** — run node/npm via `flatpak-spawn --host ...`
- **MCP first** — prefer registering a new capability as an MCP server over adding Rust code
- **Boundary discipline** — never let factory code reach into `factory/scenarios/` (Sable only)
- **Specs before code** — if there is no spec, write one first

---

## Definition

**What:** A local-first, spec-driven, identity-preserving, causally-aware AI software factory where agents accumulate structured knowledge and maintain coherent identity across every run.

**Why:** To replace human implementation-centric workflows with autonomous build-and-evaluate loops that are safer, more observable, and genuinely better over time — not because the model improved, but because the system learned, software moved through the delivery system with less coordination drag, and the agents who learned it are provably still themselves.

**What makes it distinct:** Pearl-hierarchy causal memory, typed agent identity (Soul Store), hidden scenario evaluation, and a benchmark suite that includes tests no competitor can run.
