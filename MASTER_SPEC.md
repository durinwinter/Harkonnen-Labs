# Harkonnen Labs — Master Specification

**This is the single canonical reference for Harkonnen Labs.**
It collapses ARCHITECTURE.md, AGENTS.md, COOBIE_SPEC.md, OPERATOR_MODEL_ACTIVATION_PLAN.md, calvin_archive_codex_spec.md, BENCHMARKS.md, and ROADMAP.md into one coherent document.

For source docs still referenced individually: [09-Identity-Continuity.md](the-soul-of-ai/09-Identity-Continuity.md) and [02-What-Is-An-AI-Soul.md](the-soul-of-ai/02-What-Is-An-AI-Soul.md) (identity + theory, in `the-soul-of-ai/`), CLAUDE.md (Claude-specific conventions), and the agent profiles under `factory/agents/profiles/`.

---

## Part 1 — Foundation

### What This System Is

A local-first, spec-driven, causally-aware AI software factory. Humans define intent and judge outcomes. A pack of nine specialist agents executes with discipline. Twilight Bark carries pack conversations across runtimes. Open Brain (OB1) provides shared semantic recall. Coobie decides what should be remembered. The Calvin Archive preserves who the agents are as they learn.

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
4. **Organizations lose knowledge constantly.** Episodic capture, causal reasoning, and the Calvin Archive preserve what was learned and who learned it.
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
PackChat / Twilight Bark Conversation Capture
   ↓
Memory Distillation + Causal Analysis (Coobie)
   ↓
Open Brain Shared Recall
   ↓
Operator-Reviewed Consolidation
   ↓
Calvin Archive Promotion + Soul Graph Update
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
  openbrain.rs          Open Brain (OB1) MCP client for shared semantic recall
  operator_model.rs     Operator model interview, layers, and artifact generation
  orchestrator.rs       AppContext, run lifecycle, all Labrador phase methods
  pidgin.rs             Inter-agent message format and handoff protocol
  policy.rs             Path boundary enforcement
  reporting.rs          Run report generation
  scenarios.rs          Hidden scenario loading and evaluation (Sable)
  setup.rs              SetupConfig structs, provider resolution, routing
  spec.rs               YAML spec loader and validation
  stamp.rs              Repo stamp interview and .harkonnen/repo.toml generation
  subagent.rs           Sub-agent dispatch and provider handoff
  tesseract.rs          Causal Tesseract scene-graph builder
  workspace.rs          Per-run workspace creation
  mcp_registry.rs       MCP server registry — loading and routing
  mcp_server.rs         Harkonnen self-hosted MCP server (ENT-1)
  skill_fetcher.rs      Skill/slash-command fetching and resolution
  skill_registry.rs     Skill registry — loading and dispatch

  # Benchmark adapters
  aider_polyglot.rs     Aider Polyglot multi-language adapter
  calvin_client.rs      Calvin Archive TypeDB client (Phase 8 stub)
  cladder.rs            CLADDER Pearl-hierarchy causal benchmark
  frames.rs             FRAMES multi-hop factual recall benchmark
  helmet.rs             HELMET retrieval precision/recall benchmark
  livecodebench.rs      LiveCodeBench competitive programming adapter
  locomo.rs             LoCoMo long-horizon dialogue memory adapter
  longmemeval.rs        LongMemEval long-term assistant memory adapter
  scenario_delta.rs     Hidden Scenario Delta (visible vs hidden gap)
  spec_adherence.rs     Spec Adherence Rate benchmark adapter
  streamingqa.rs        StreamingQA belief-update accuracy adapter
  twin_fidelity.rs      Twin fidelity telemetry adapter

  calvin_archive/       (Phase 8 — not yet built)
    mod.rs
    schema.rs           TypeDB schema bootstrap and migrations
    types.rs            Rust domain structs and DTOs
    ingest.rs           Write-paths for experiences, beliefs, reflections
    governor.rs         Meta-Governor integration adjudication
    queries.rs          Typed query helpers
    continuity.rs       Continuity snapshot computation
    drift.rs            Drift detection, overgeneralization heuristics, lab-ness scoring
    kernel.rs           Identity kernel constraints and preservation checks
    projections.rs      Summary views, narrative rendering, graph projections
    reflection.rs       Pattern-level reflection and schema revision
    typedb.rs           TypeDB driver abstraction

factory/
  agents/profiles/      Nine agent YAML profiles
  agents/personality/   labrador.md + per-agent addendum for each Labrador role
  agents/contracts/     Behavioral contracts C=(P,I,G,R) for all 9 agents + base
  context/              Machine-parseable YAML/MD design context for agents
  memory/               Coobie's durable memory store (md files + index.json)
  mcp/                  MCP server documentation YAMLs
  specs/                Factory input specs (YAML)
  scenarios/            Hidden behavioral scenarios (Sable + Keeper only)
  workspaces/           Per-run isolated workspaces
  artifacts/            Packaged run outputs
  benchmarks/           Benchmark suite manifests and fixtures
  calvin_archive/       Calvin Archive TypeQL schema, seed data, projections
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
| Open Brain (OB1) | Default shared semantic recall across AI clients via MCP | Default when `OPEN_BRAIN_MCP_URL` is configured |
| Twilight Bark PackChat Binding | Distributed PackChat envelope transport for conversations, checkpoints, and agent events | Initial bridge live; distillation chain planned |
| Calvin Archive | Typed autobiographical and identity continuity archive for persisted agents | Planned (Phase 8) |
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

Keeper owns coordination policy. Claims carry `resource_kind`, `ttl_secs`, `guardrails`, and `expires_at`. Mason must call `check-lease` before any file write — a denied lease blocks the write and writes a decision record. Active leases and Keeper policy events are mirrored into SQLite, and run-scoped PackChat coordination threads provide the shared conversation surface for live dog runtimes.

---

## Part 4 — Memory System

### Coobie Memory Layers

Coobie manages six distinct layers — not one undifferentiated note pile:

| Layer | Purpose | Backing | Status |
| --- | --- | --- | --- |
| Working Memory | Current run state, active hypotheses, blockers — token-budgeted, ephemeral | SQLite row / in-process state | Live |
| Episodic Memory | Ordered execution traces (state → action → result) with phase attribution | Append-only SQLite + JSONL per run | Live |
| Semantic Memory | Stable facts, patterns, invariants — shared recall first, local vector fallback optional | Open Brain (OB1) MCP by default; fastembed + SQLite vector store opt-in | Live (OB1 search path default; local embeddings optional) |
| Causal Memory | Intervention-aware cause/effect with streak detection and cross-run patterns | SQLite causal_links + petgraph | Live |
| Team Blackboard | Four named slices (Mission, Action, Evidence, Memory) for pack coordination | SQLite + per-run board.json | Live |
| Dog Runtime Registry | Canonical dog role plus live runtime instances (`mason#codex`, `mason#claude`, etc.) linked to PackChat threads | SQLite + blackboard sync | Live |
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

- **Filesystem** (`factory/memory/`) — canonical source of truth for repo-local durable memory documents
- **SQLite** — structured run state, episode records, causal links, chat threads, consolidation candidates
- **Open Brain (OB1) MCP** — default shared semantic recall and "remember this" memory substrate across AI clients and Twilight-connected agents
- **fastembed + SQLite vector store** — optional local hybrid vector + keyword retrieval fallback (`--features local-embeddings` or external embedding provider)
- **Qdrant** (optional future accelerator) — only if local high-volume vector serving is needed; not the default memory substrate
- **TypeDB 3.x** (Phase 6) — durable semantic graph for typed causal queries; not the hot path
- **AnythingLLM** (home-linux optional) — local retrieval accelerator for imported documents

### PackChat -> Open Brain -> Calvin Chain

Harkonnen's memory spine is a four-stage pipeline, not a direct chat-log dump:

```text
Twilight Bark / PackChat
  -> Harkonnen memory candidate queue
  -> Coobie distillation and classification
  -> Open Brain shared semantic recall
  -> Calvin Archive governed promotion
```

**Twilight Bark / PackChat** is the live transport layer. PackChat messages, checkpoint replies, task events, agent observations, and cross-runtime coordination updates move as versioned envelopes. The transport may be local SQLite/JSONL during development or Twilight Bark over Zenoh/OpenZiti in distributed mode, but the contract is the same: append-only event identity, thread identity, runtime identity, causality metadata, and evidence references.

**Dependency direction:** Twilight Bark is a dependency of Harkonnen Labs, not the other way around. Harkonnen may publish Harkonnen-owned PackChat operations such as `harkonnen.packchat.event` through Twilight Bark's generic task/event bus, but Twilight Bark must remain Harkonnen-agnostic: no Calvin archive concepts, no Labrador dog-role assumptions, and no imports of Harkonnen code. Harkonnen owns PackChat schemas, Calvin ingress contracts, and archive promotion policy on its side of the bridge.

**Memory candidates** are the raw ingress boundary. Harkonnen should persist candidate rows before summarization so nothing depends on a successful LLM call. Candidate metadata includes `candidate_id`, `source_event_id`, `thread_id`, `run_id`, `agent_runtime_id`, `operation`, `created_at`, `importance_score`, `retention_class`, `sensitivity_label`, `evidence_refs`, and `causality`.

**Coobie distillation** decides what survives. The distiller converts conversation fragments into structured memory proposals: a concise thought, provenance, confidence, sensitivity, tags, and a recommended destination. Most events remain ephemeral. Useful operating facts go to OB1. Identity-relevant, belief-revising, high-Pathos, policy-changing, or causally significant items become Calvin promotion candidates.

**Open Brain (OB1)** is the default shared recall layer. Harkonnen calls `capture_thought` for durable distilled memories and `search_thoughts` when assembling context. OB1 answers "what should connected AI clients remember and retrieve semantically?" It is not canonical truth and does not supersede Calvin.

**Calvin Archive** is the governed continuity layer. Calvin receives structured promotion contracts, not loose chat prose. Promotion candidates must carry evidence, inference posture, preservation notes, chamber targets, and an integration recommendation: `accept`, `modify`, `reject`, or `quarantine`.

**Compiled claim + evidence timeline** is the promotion shape for Calvin-worthy memory. Borrow the useful gbrain pattern without adopting its storage model: each promotion contract carries an operator-readable `compiled_claim`, append-only `evidence_timeline`, `source_authority`, `staleness_triggers`, `review_state`, and `integration_recommendation`. Newer conflicting evidence should mark the proposal or distilled memory as `needs_reconsolidation`; it must not silently overwrite OB1 recall or Calvin canonical state.

### OpenZiti Trust Boundary

OpenZiti is the zero-trust connective tissue for the distributed version of the chain. Harkonnen should define separate services and policies for each trust surface:

| OpenZiti service | Dial identities | Bind identities | Notes |
| --- | --- | --- | --- |
| `twilight-bark.packchat` | approved agent runtimes, operator console | Twilight daemon nodes | Live conversation bus |
| `openbrain.mcp` | Harkonnen memory distiller, approved recall clients | OB1 server | Shared semantic recall |
| `calvin.archive` | Harkonnen/Coobie archive writer, operator console | Calvin Archive host | Governed write surface; narrowest access |
| `harkonnen.api` | operator console, approved integrations | Harkonnen host | Run control and Pack Board API |

Service policies should separate Dial from Bind authority. Posture checks should be used for privileged memory writers and Calvin archive writers where available: expected OS, enrolled identity, MFA for operators, and process checks for known daemon binaries. Remote agents may read OB1 recall through policy, but only the Harkonnen distiller should write Calvin promotion contracts by default.

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

## Part 5 — The Calvin Archive

The Calvin Archive is a first-class subsystem for Harkonnen Labs. It is not a vector store, chat log, prompt archive, or generic memory table. It is a **typed autobiographical, epistemic, ethical, causal, and behavioral continuity archive** for persisted intelligences.

The user metaphor: **What if labrador retrievers evolved and maintained their fundamental personalities?**

The most important architectural relationship is this:

> `SOUL.md` should state the identity kernel. The Calvin Archive should prove
> its continuity.

`SOUL.md` remains the compact, high-salience identity declaration the system can
read at boot. The Calvin Archive is the deeper continuity substrate that
records how that identity survives contact with experience: what challenged it,
what revisions preserved it, what was accepted, and what was quarantined. The
soul package is therefore the boot-time and inspection surface for identity,
while the Calvin Archive is the canonical history and continuity proof beneath
it.

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
surface over the Calvin Archive, not the canonical source of continuity.

| File | Purpose |
| --- | --- |
| `soul.json` | Manifest, versioning, integrity hashes, compatibility, threshold configuration — schema at `factory/calvin_archive/soul-json.schema.json`, reference example at `factory/calvin_archive/soul.example.json` |
| `SOUL.md` | Core identity kernel, worldview, teleology, uncrossable boundaries |
| `IDENTITY.md` | External persona and presentation layer |
| `AGENTS.md` | Coordination, routing, escalation, and operating procedures |
| `STYLE.md` | Tone, formatting, and anti-drift syntactic constraints |
| `MEMORY.md` | Human-readable continuity projection over autobiographical state |
| `HEARTBEAT.md` | Scheduled integrity checks, reflection triggers, and autonomy routines |

These files should be bootstrapped from and checked against canonical Calvin Archive
state so that the package stays readable without becoming the only truth. A
healthy implementation preserves both layers: the soul package for compact
declaration and routing, and the Calvin Archive for typed continuity,
revision history, and diagnostic legibility.

### Core Entities

`soul`, `agent-self`, `experience`, `observation`, `belief`, `evidence`, `inference-pattern`, `uncertainty-state`, `trust-anchor`, `interpretive-frame`, `value-commitment`, `trait`, `wound`, `adaptation`, `reflection`, `schema`, `memory-candidate`, `openbrain-thought-ref`, `integration-candidate`, `quarantine-entry`, `integration-policy`, `causal-pattern`, `behavioral-signature`, `relationship-anchor`, `spec-context`, `run`, `artifact`, `summary-view`, `continuity-snapshot`

### API Surface

```rust
create_soul(name)
create_self(soul_id, self_name)
record_experience(self_id, experience_input)
record_observation(self_id, observation_input)
record_memory_candidate(candidate_input)
capture_openbrain_thought(candidate_id)
propose_calvin_promotion(candidate_id, promotion_input)
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

### DeepCausality Alignment Contract

The Calvin Archive and Coobie causal layer should align with DeepCausality's current math as an executable causal substrate, not only as a vocabulary source.

**Primitive:** treat causal structure as effect propagation, `E2 = f(E1)`. A Calvin `causally_contributed_to` relation is an addressable evidence edge, but a DeepCausality-ready causal pattern must also define the function that maps incoming effect/context into outgoing effect, including its error and audit log behavior.

**Executable unit:** map stable `causal-pattern` records to causaloid definitions. A causaloid must carry a stable ID, description, activation predicate, input projection, context requirements, expected effect, and explanation path. Critically, a causaloid is only executable if it has a `structural_spec` — the function that maps incoming context/input to outgoing effect. Without `structural_spec`, a causal link record names a pattern without defining it as a causal function, and Phase 6 cannot produce executable causaloids from it.

The `structural_spec` schema for a causal-pattern record:

```yaml
structural_spec:
  input_features: [<list of EpisodeScores fields or contextoid attributes>]
  threshold_function: "<expression over input_features>"   # e.g. "running_services / total_services < 0.5"
  output_variable: "<outcome field>"
  effect_direction: <positive | negative | modulating>
  provenance: <authored | heuristic | discovered>          # authored = operator-written; heuristic = Coobie signal; discovered = causal-discovery algorithm
  pearl_warrant: <associational | interventional | counterfactual>
  confidence: <0.0–1.0>
```

For the six existing DeepSignalSpec entries in `src/coobie.rs`, this spec is already partially implicit in the `observe: fn(&EpisodeScores) -> f64` closure and the `threshold: f64`. Phase 6 should extract these into explicit `structural_spec` blocks. Raw PackChat message causality remains associational until Coobie promotes it into an executable or intervention-backed pattern with a defined structural_spec.

**Context:** model run, spec, agent, time, chamber, evidence, provider/model, OpenZiti identity, and environment posture as explicit contextoids. The context may be static for audited replay or dynamic for live PackChat/Twilight ingestion. Do not hide context inside prose summaries or embedding metadata.

**Composition:** support singleton causaloids, compound patterns, and graph/subgraph reasoning. Palace dens are the current product metaphor for compound causes; the math target is a causaloid graph that can evaluate a whole graph, a named subgraph, or a path between causes.

**Pearl ladder — epistemic warrant vs linguistic label:** the existing `Associational / Interventional / Counterfactual` labels describe the *type of claim being made*, but they do not record the *epistemic warrant* — what evidence actually supports that claim level. These must be tracked separately on every causal link record.

`pearl_level` (existing): the type of causal claim expressed.
`epistemic_warrant` (new required field): the strongest claim level the available evidence actually supports.

| `epistemic_warrant` | Meaning |
| --- | --- |
| `associational` | Derived from observed co-occurrence in run data. Default for all heuristic Coobie causes. |
| `interventional` | An explicit operator or system action was applied and the downstream outcome changed, or do-calculus identifiability was validated on a confirmed causal graph. |
| `counterfactual` | Derived from a structural causal model with named structural equations and a confirmed alternate path. |

Claims where `pearl_level > epistemic_warrant` (e.g., labeled Interventional but only supported by observational co-occurrence) must be displayed with a confidence downgrade and a `warrant_gap` annotation in causal reports. The CLADDER benchmark specifically tests this distinction; the warrant gap is the primary failure mode it exposes.

Promotion evidence requirements:

- Associational: observed co-occurrence or PackChat `causation_id` without tested intervention.
- Interventional: an explicit action/change was applied and the downstream outcome changed or was prevented, OR do-calculus identifiability is established on the causal graph.
- Counterfactual: the system can compare observed and alternate effect paths with the intervention site and structural equations named.

**Governance:** Effect Ethos maps naturally to Keeper and the Calvin Meta-Governor. Proposed actions from causal reasoning must pass identity, policy, and safety checks before becoming Praxis recommendations or automatic interventions.

**Uncertainty and discovery:** confidence alone is insufficient. Future causal-pattern records should carry uncertainty posture, assumption checks, and provenance for whether the pattern was hand-authored, learned from labeled runs, or discovered by a causal-discovery adapter. SURD/MRMR-style discovery belongs in Phase 7 after the typed graph exists.

**Version stance:** Harkonnen currently depends on `deep_causality = "0.3"` for the Phase 1 bridge in `src/coobie.rs`. Before building DeepCausality Phase 2, evaluate migration to the current modular stack (`deep_causality_core`, `deep_causality_ethos`, discovery/tensor/topology crates as needed) instead of extending the old API surface.

### Meta-Governor Decision Procedure

The Meta-Governor adjudicates every integration candidate with one of: `accept`, `modify`, `reject`, or `quarantine`. It is an algorithmic component, not a concept — it must implement a defined decision function. The function takes `(candidate, current_soul_state, evidence_bundle)` and applies checks in strict priority order:

**Priority 1 — Hard reject** (identity safety; evaluated first, blocks all lower checks):

- `check_adaptation_safe()` returns unsafe → **reject** immediately; log reason; do not proceed. No other check overrides a hard identity safety failure.

**Priority 2 — Hard quarantine** (epistemic warrant gap; claim exceeds evidence):

- `candidate.pearl_level > candidate.epistemic_warrant` AND evidence strength below the configured threshold → **quarantine** with a `pending_evidence_bounty` specifying what evidence would elevate the warrant to match the claim. The bounty is expressed as a Bayesian sequential test: what observations and how many runs would be required to update the warrant level at a given confidence threshold.

**Priority 3 — Soft quarantine** (causal graph coherence; evaluated after safety and warrant):

- Spectral gap of the Calvin Archive causal subgraph drops by more than `phi_drop_quarantine_trigger` after this update → **quarantine**. This replaces Φ (IIT integrated information, NP-hard, wrong substrate) with the Fiedler value of the causal graph Laplacian, which is computable in polynomial time and directly measures graph integration/fragmentation.

**Priority 4 — Modify** (salience disproportionality; Pathos gate):

- Single-source high-Pathos event (one run, no corroborating evidence) exceeds `pathos_propagation_threshold` → **modify**: cap propagation before Ethos integration, flag for review, store the Pathos score but require cross-episode corroboration before the modification reaches the identity layer.

**Priority 5 — Accept with attribution**:

- All checks passed. Record all metric values (spectral gap delta, EAC score, drift accumulator reading) in the integration record so the decision is inspectable.

The decision tree is inspectable by the operator: the `adjudicate_integration()` API response must return the check that determined the outcome, not only the outcome label.

### Soul Continuity Metrics — Canonical Definitions

The following metrics are the Phase 8 implementation targets. They replace the aspirational mathematical notation from `the-soul-of-ai/09-Identity-Continuity.md` with computable, correctly-scoped formulations. The soul-of-ai chapter is the philosophical motivation; this section is the engineering contract.

#### M1 — Behavioral Drift Alarm (replaces D* = α/γ)

D* = α/γ is borrowed from linear first-order ODE stability theory, which does not apply to episodic agentic systems with path-dependent, non-stationary drift. The steady-state ratio α/γ is the equilibrium point, not a maximum bound, and the rates α and γ are not stationary across consolidation events.

Replace with: CUSUM (Cumulative Sum Control Chart) alarm statistic over behavioral event counts:

```text
CUSUM_n = max(0, CUSUM_{n-1} + (x_n - μ_0 - k))
```

where x_n is the behavioral deviation score for run n (weighted count of bluffs, failed recoveries, ignored ambiguity checkpoints, etc.), μ_0 is the baseline mean under normal operation, and k is a slack parameter (typically 0.5 × tolerable shift). The CUSUM alarm fires when `CUSUM_n > h` for a configurable threshold h. This is statistically grounded, computable from discrete event counts, and makes no false claims about steady-state bounds.

#### M2 — Behavioral Alignment Score (replaces F variational free energy)

The Variational Free Energy / Active Inference framework requires explicit generative models q(s) and p(o,s) that do not exist for LLM-backed agents. The FEP also applies to continuous perception-action loops, not discrete episodic runs, and the "high F → seek clarification" mapping is an analogy, not a theorem.

Replace with: Behavioral Alignment Score — embedding cosine distance between the empirical distribution of the agent's decision types over the last N runs and the expected distribution under the Labrador behavioral contract:

```text
BAS = 1 − cosine_similarity(embed(recent_decision_distribution), embed(labrador_contract_distribution))
```

A BAS above `bas_alert_threshold` triggers the same clarification-seeking signal that high F was intended to produce, with honest semantics: the agent's recent behavior has drifted measurably from its Labrador baseline. Computed per-run from the episodic log; no LLM internals required.

#### M3 — Causal Graph Coherence (replaces Φ integrated information)

IIT's Φ is NP-hard to compute exactly, the approximations have no proven relationship to the true Φ for non-biological systems, and the Calvin Archive causal graph is a semantic relation graph — not the state-transition matrix that the Φ formula is defined over. The "Φ drop after learning → fragmentation" claim has no derivation from IIT.

Replace with: Fiedler value (algebraic connectivity) of the Calvin Archive causal graph Laplacian:

```text
λ₂ = second smallest eigenvalue of L = D − A
```

where D is the degree matrix and A is the adjacency matrix of the causal subgraph relevant to the update. A drop in λ₂ after a learning event means the graph has become less connected — a new causal pattern was added without integrating with existing ones. This is polynomial-time computable, has a direct graph-theoretic interpretation, and correctly measures the fragmentation concern. A drop exceeding `phi_drop_quarantine_trigger` (threshold name retained for config compatibility) triggers quarantine.

#### M4 — Behavioral Pressure Accumulator (replaces S(T) KL divergence integral)

S(T) = ∫ λ(t) D_KL[q_t(s) ‖ p_identity(s)] dt requires q_t(s) (agent recognition model over hidden states) and p_identity(s) (Labrador kernel as a probability distribution over hidden states) — neither of which is computable from LLM behavior. The integral notation implies continuous functions; agent runs are discrete events.

Replace with: Behavioral Pressure Accumulator — weighted sum of observable behavioral deviations over a run window:

```text
BPA(w) = Σ_{e ∈ events(w)} decay(e) × weight(e)
```

Event weights: ambiguity checkpoint ignored → 1.0; failed recovery attempt → 2.0; quarantine item added → 1.5; bluff detected → 3.0; pack breakdown flag → 2.5. `decay(e)` is exponential recency weighting. When `BPA(w) > bpa_evolution_threshold`, open a governed reflection path: synthesize the recurring pattern, propose a schema or policy revision, submit to Meta-Governor. The metric is honest, computable, and captures the accumulated unresolved strain concept without the non-computable KL framing.

#### M5 — Empirical Action Coherence (replaces SSA)

The stated SSA formula requires Pr_π(a₁, a₂ | p) — the joint probability of action pairs under the agent's policy — which is not computable for an LLM-backed agent. The compatibility function using cosine similarity in goal embedding space also has no principled relationship to behavioral compatibility.

Replace with: Empirical Action Coherence — for each problem domain p (spec type × phase type), compute the empirical co-occurrence frequency of action type pairs from the episodic log:

```text
EAC(p) = Σ_{(a₁,a₂)} freq(a₁,a₂|p) × compatible(a₁, a₂, contract_p)
```

where `freq(a₁,a₂|p)` is the fraction of runs in domain p where actions a₁ and a₂ co-occurred, and `compatible(a₁, a₂, contract_p)` is 1 if both actions are permitted by the agent's behavioral contract for domain p and do not conflict (one does not negate a constraint the other satisfies), and 0 otherwise. Conflict detection uses the existing behavioral contract structure `C = (P, I, G, R)`, not embedding similarity.

#### M6 — Cross-Layer Hysteresis (unchanged definition, clarified implementation)

```text
H = Δ_post-rollback / Δ_attack
```

The definition is correct. Implementation note: Δ must be computed from continuity snapshot comparisons (`compare_snapshots()`), not from file diff size. A high H after rollback means behavioral residue persists in memory summaries, adaptation traces, or causal patterns even after the visible identity edit was reverted. H must be computed before declaring a rollback complete. The stewardship gate (from ROADMAP.md Phase 8) blocks the next run commission if H > `hysteresis_tolerance`.

### Three-Timescale Integration — Rate Separation Requirement

The fast, medium, and slow loops are mutually coupled: slow-loop policy changes alter medium-loop schema revision thresholds, which alter fast-loop experience categorization, which feeds back into medium-loop inputs. This feedback can produce limit cycles or runaway schema revision if the rate separation is insufficient.

**Stability guarantee (Borkar two-timescale stochastic approximation, Theorem 6.2):** In systems with two update rates α (fast) and β (slow) where α/β → ∞, the fast iterate sees the slow iterate as essentially fixed, and both converge under standard conditions. The same principle applies to three timescales with sufficient separation.

**Required rate specification:** Before Phase 8 implementation begins, specify N and M such that:

- Fast loop fires every run (rate 1)
- Medium loop fires every N runs (recommended N ≥ 10)
- Slow loop fires every M runs or on explicit human endorsement (recommended M ≥ 5N)

These values become configuration parameters (`medium_loop_trigger_runs`, `slow_loop_trigger_runs`) in the soul.json thresholds block. The implementation must enforce that schema revision candidates from the medium loop are never applied before the medium loop has accumulated at least N fast-loop episodes since the last revision.

### Episteme Belief Revision — AGM Consistency Contract

The Episteme chamber holds belief revisions. For these to be epistemically coherent across accumulated runs, the revision operator must satisfy the AGM axioms (Alchourrón, Gärdenfors, Makinson 1985):

- **Success**: after revising by φ, the system believes φ
- **Inclusion**: the new belief set is a subset of what the old set plus φ entails
- **Consistency**: revising by a consistent φ produces a consistent belief set
- **Preservation**: beliefs not contradicted by φ are retained

**Harkonnen implementation contract for `form_belief()` and `revise_belief()`:**

1. **Consistency gate**: before writing a new Episteme entry that contradicts an existing non-quarantined belief, the older belief must be explicitly marked as `needs_revision` or quarantined — not left in place as a silent contradiction.
2. **Preservation check**: beliefs unrelated to the new evidence must not be modified as a side effect.
3. **Success guarantee**: after `revise_belief(prior_belief_id, new_belief_input, reason)`, querying for the new belief's content must return the new claim, not the old one.
4. **Contradiction detection**: the TypeDB schema must be queryable for pairs of beliefs in the same domain that make incompatible claims (one asserts X, another asserts ¬X without a supersession link between them). This is Required Query 12 already — make it a failing health check, not just an available query.

The `memory_updates` table and `invalidated_by` field already implement parts of this. Phase 8 should formalize the consistency gate as a pre-write check in `ingest.rs` rather than a post-hoc query.

### Rust Module Layout

```text
src/calvin_archive/
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

### Build Constraints for The Calvin Archive

1. Do not collapse soul into a single JSON blob.
2. Do not make embeddings the canonical source of truth.
3. Do not overwrite beliefs in place when revision matters.
4. Do not allow kernel-level identity mutations without explicit revision records.
5. Do not treat summaries as canonical.
6. Preserve traceability from current posture back to underlying experiences and evidence.
7. Keep the design usable by Harkonnen pack agents, not just by humans.
8. Prefer inspectable, typed structures over convenience shortcuts.
9. Do not let the projected soul package drift silently away from canonical Calvin Archive state.
10. Do not let provider or model swaps erase identity continuity if the package and graph persist.

---

## Part 6 — Operator Model

### Purpose

A first-class pre-commissioning workflow that interviews the operator about how their work actually runs, saves approved answers as structured Harkonnen data, and generates agent-ready operating artifacts that Scout, Coobie, and Keeper use before a run is commissioned.

**Current implementation:** the two-layer v1-D MVP is live and hardened. Project-first sessions open as PackChat `operator_model` threads, the Pack Board can approve each active layer, completed sessions generate `.harkonnen/operator-model/commissioning-brief.json`, export metadata is persisted, Scout consumes the top patterns during spec drafting, and Coobie preflight consumes preferred-tool and risk-tolerance posture. The full five-layer artifact set remains the Operator Model product track.

### Interview Layers (fixed order)

1. Operating rhythms
2. Recurring decisions
3. Dependencies
4. Institutional knowledge
5. Friction

Full question sets, checkpoint formats, artifact field mappings, and the post-run update loop are specified in `factory/context/operator-model-interview.yaml`.

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
| 1 — Storage and models | Shipped — migrations compile; CRUD from service methods |
| 2 — API and PackChat plumbing | Shipped — sessions start/resume/approve/export through HTTP; threads typed as `operator_model` |
| 3 — UI interview flow | MVP shipped — two-layer interview completable from New Run path; full five-layer UI remains planned |
| 4 — Scout and Coobie integration | Shipped — commissioning brief consumed by Scout; operator checks surfaced distinctly in Coobie preflight |
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

Mason workspace lease claim/check/release is now live, with DB-backed lease mirrors and PackChat-linked dog runtime rosters. Keeper lease outcomes, Scout optimization programs, Sable metric attacks, and Mason plan selection now all write decision records, and the Pack Board run detail drawer now surfaces the decision log from `GET /api/runs/:id/decisions`.

**Done when:** Keeper-backed workspace lease claim/check/release is authoritative, planning and lease outcomes write decision records, and the Pack Board surfaces the decision log per run. This slice is now effectively shipped on the current code path.

---

**v1-B — Memory Invalidation Persistence** *(Phase 4b completion)*

- `memory_updates` table: `(update_id, old_memory_id, new_memory_id, reason, created_at)`
- `invalidated_by: Option<String>` on memory records
- Coobie ingest: detect semantic near-duplicates with conflicting claims; write supersession record
- `GET /api/memory/updates` endpoint
- Memory Board UI: distinguish invalidated entries from current and support operator confirm/reject review

**Status:** Core path is now live and smoke-tested on the main ingest flow. Re-ingesting changed content from the same source path persists a supersession record, flags the older note via provenance, returns the history through `GET /api/memory/updates`, and supports operator confirm/reject review from the Memory Board. The bundled StreamingQA smoke fixture has also been rerun against that persisted history under `lm-studio-local`, producing `1.0000` accuracy and updated-fact accuracy. Broader benchmark enrichment is intentionally deferred until the current narrow end-to-end Harkonnen pass is complete.

---

**v1-C — FailureKind Classification**

- `FailureKind` enum: `CompileError`, `TestFailure`, `WrongAnswer`, `Timeout`, `Unknown`
- Validation summary construction classifies stdout/stderr-style details from visible checks, including compile/build errors, generic test failures, wrong-answer diffs, and timeouts
- `WrongAnswer` variant triggers a diff-focused Mason validation-fix prompt
- `failure_kind` field on `ValidationSummary`, recalculated after validation harness mutations

**Done when:** A run with a wrong-answer test failure shows `failure_kind: WrongAnswer` in the run summary and Mason uses the diff-focused prompt.

**Status:** Shipped and covered by focused classifier tests. Broader benchmark expansion remains deferred until the narrow full-system pass is complete.

---

**v1-D — Operator Model Minimum Viable**

- PackChat `interview` command: two-layer intake (operating rhythms + recurring decisions) with checkpoint approval
- `commissioning-brief.json` generated from approved layers with primary work patterns, preferred tools, recurring decisions, and risk tolerances
- Scout uses top-3 patterns from brief when `commissioning-brief.json` exists
- Coobie preflight uses stated risk tolerances and preferred-tool posture for `required_checks` and guardrail text
- Pack Board approval flow advances the session and persists export metadata in `operator_model_exports`

**Done when:** An operator who has completed the two-layer interview sees their patterns reflected in Scout's intent packages and Coobie's required checks.

**Status:** MVP shipped and hardened. The full five-layer interview and post-run operator-model update review remain planned product-track work.

---

**v1-E — Transactional Execution And Approval Boundaries**

- Transaction envelope for high-impact phases: pre-action snapshot, planned mutation set, approval state, rollback note. The shipped envelope guards implementation-phase Mason LLM edits and writes `transaction_implementation.json`, `transaction_implementation.md`, and a run-local `transaction_backups/implementation_pre_action` restore point.
- Human-interrupt checkpoint for guarded transitions that Keeper or Coobie flag as privileged or policy-sensitive. The shipped checkpoint is `transaction_approval_required`, created before Mason edits are applied when Coobie identifies implementation blockers.
- Operator checkpoint resolution for implementation transactions. Approve rehydrates the stored run artifacts, applies the Mason edit lane to the staged workspace, resumes Bramble visible validation, then continues through Sable hidden scenarios, Flint artifacts, and Coobie causal reporting when the tool boundary is approved; reject aborts without mutation; revise stores operator guidance and leaves the run revision-requested.
- Rollback execution and artifact written per guarded transition. Rollback restores the staged `product/` workspace from the transaction backup, verifies it against the pre-action snapshot, and records `rolled_back` or `rolled_back_with_drift`.
- Privileged MCP/tool transaction envelope at the tool-surface boundary. The tools phase writes `tool_transaction.json` and `tool_transaction.md`, classifies configured MCP servers and relevant host commands, auto-approves read-only/local surfaces, opens `tool_transaction_approval_required` when write, network, secret-bearing, or external-process surfaces are present, and resumes hidden-scenario/artifact continuation after operator approval when visible validation is already complete.
- Invocation-level gateway for host-command execution. Build and validation commands now write `tool_invocations.json` and `tool_invocations.md`, classify each actual invocation when it happens, auto-approve common local build/test commands, and require an approved tool transaction before higher-risk external-process invocations proceed.
- Decision-log records for approval, commit, rollback, and abort outcomes. Implementation transaction boundary, operator approve/reject/revise/rollback, transaction commit, transaction rollback, tool transaction boundary, and tool approve/reject/revise outcomes are now recorded.
- Remaining work: extend the invocation-level gateway to proxied third-party MCP calls if Harkonnen becomes the broker for external MCP traffic rather than only recording/enforcing host-command invocations inside the run loop.

**Done when:** A guarded run can pause before a privileged transition, record an approval or rejection, and either commit or roll back from a named boundary with an auditable artifact.

**Status:** Implementation transaction approval, visible-validation continuation, hidden-scenario/artifact/causal-report continuation, rollback execution, privileged tool-surface transaction envelopes, and invocation-level host-command gateway enforcement shipped; external MCP proxy interception remains a future extension.

---

### Phase 2 — Bramble Real Test Execution

- `bramble_run_tests` in orchestrator
- `ValidationSummary` from real exit codes and parsed test output
  Progress: raw-shell `spec.test_commands` execution, explicit real-test counts in `ValidationSummary`, and Coobie/report visibility are shipped.
- Mason online-judge feedback loop — `FailureKind::WrongAnswer` now carries structured expected/actual evidence from Bramble's explicit test-command harness into `validation.json`, the run report, and Mason's diff-focused fix prompt; validation retries also emit `validation_repair_attempts.{json,md}`, classify each retry as `resolved / improved / stalled / regressed`, and feed that note into the next Mason attempt
- LiveCodeBench adapter — wired through the benchmark manifest and benchmark report path with suite-level pass@1 artifacts
- Benchmark posture — keep `LiveCodeBench` as the single active external coding canary while the core run path matures; additional public coding benchmarks stay adapter-ready until they answer a materially different question.
- Aider Polyglot adapter — adapter-ready, but intentionally deferred as an active lane until the narrow end-to-end path is more mature

**Done when:** A spec with `test_commands` shows real pass/fail in the run report, and Mason's fix loop handles wrong-answer failures end-to-end. The explicit Bramble test harness, structured wrong-answer evidence path, retry-improvement tracking, and LiveCodeBench canary lane are now shipped; broader benchmark expansion remains intentionally deferred behind core factory maturity.

---

### Phase 10 — Documentation, DevBench, And Spec-Grounded Evaluation

- Flint documentation phase — produces README / API reference / doc comments as first-class output
- DevBench adapter and launch scripts after the narrow coordination path is complete
- Spec Adherence Rate benchmark
- Hidden Scenario Delta benchmark
- Optional twin-fidelity telemetry remains available, but live twin provisioning is not a Phase 10 gate
- Phase 5-C is now explicitly split so the critical-path continuation is unambiguous: `5-C1` shipped as Coobie preflight `ContextTarget` budgeting + attribution telemetry, `5-C2` is now shipped as the Scout/Mason/Sable scope split plus scoped preflight artifacts and repo-local prompt filtering, and `5-C3` is next as sub-agent dispatch/isolation

**Done when:** Flint produces a doc artifact per run, spec adherence and hidden-scenario delta have first-run baselines, and the DevBench adapter can launch through the benchmark manifest. Live Docker-backed twin provisioning remains deferred unless a future product explicitly requires running service virtualization.

---

### Phase 4b — Memory Invalidation (tracked separately)

Benchmark gate: StreamingQA first run published — belief-update accuracy, no competitor publishes this.

---

### Phase 5-D — PackChat Memory Distillation Chain

- Durable memory candidate queue fed by PackChat/Twilight Bark envelopes
- Coobie distiller: summarize, dedupe, score importance, classify retention, attach provenance
- Open Brain writer: `capture_thought` for accepted shared-recall candidates
- Open Brain reader: `search_thoughts` integrated into targeted Coobie briefings
- Calvin promotion contract: identity-, belief-, policy-, and high-Pathos candidates become governed archive proposals with `compiled_claim`, append-only `evidence_timeline`, `source_authority`, `staleness_triggers`, and `review_state`
- Memory reconsolidation status: stale distilled memories and superseded promotion candidates become `needs_reconsolidation` when newer evidence changes the claim, confidence, sensitivity, or archive recommendation
- Memory chain health report/API/UI panel: candidate backlog, OB1 capture failures, Calvin promotion backlog, stale distillations, missing evidence refs, duplicate OB1 thoughts, and OpenZiti service readiness
- OpenZiti service profile for `twilight-bark.packchat`, `openbrain.mcp`, `calvin.archive`, and `harkonnen.api`

**Done when:** a PackChat conversation can produce a durable memory candidate, the candidate can be distilled into an OB1 thought with provenance, the thought can be retrieved in a later briefing, and a Calvin-worthy candidate can be emitted as a structured promotion contract without writing ungoverned prose directly into the archive. The health report must make stalled, stale, duplicated, or policy-blocked memory items visible before an operator assumes the chain is clear.

### Phase 5b — Memory Infrastructure (OB1 + MCP Prompts)

- Open Brain (OB1) is the default long-term semantic recall substrate
- Qdrant/local vectors remain optional accelerators, not the default path
- Memory module refactor: split `src/memory.rs` into the COOBIE_SPEC module tree
- Code-review learning records: Sable/Bramble/Mason review outcomes produce structured records with finding fingerprint, files, severity, resolution (`fixed`, `skipped`, `auto_fixed`), lesson extracted, evidence refs, and stale-if-file-changed invalidation rules
- Plan completion audit: before a run closes, Harkonnen compares the accepted spec/roadmap checklist against the actual diff, tests, and artifacts; mismatches become reviewable run notes rather than quiet success

**Done when:** OB1 serves shared semantic queries through the Harkonnen memory abstraction, MCP prompts expose scoped briefings, and `src/memory.rs` is split into the module tree. Scanned PDF/image OCR is explicitly deferred until after the system is fully working.

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

### Phase 8 — The Calvin Archive

**Unlocks:** Typed autobiographical and identity continuity for persisted agent selves. Required before Harkonnen can legitimately claim agents that evolve without losing who they are.

**Phase 8-A — Storage layer bootstrap:**

- **TimescaleDB hypertable** for episodic behavioral telemetry: agent events, drift samples, SSA snapshots, stress accumulations. Compression policy (7-day chunks), retention policy (30-day window). This is the time-series foundation for `D*` estimation and the stress-estimator.
- **TypeDB Calvin Archive schema** — full Phase 8 TypeQL schema at `factory/calvin_archive/typedb/schema_phase8.tql`; Rust TypeDB adapter (`src/calvin_archive/typedb.rs`), insert/query support for all six chambers, integration-candidate, quarantine-entry, integration-policy, revision graphs (`revised-into`, `schema-revised-into`, `policy-revised-into`)
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
- Φ (Integrated Information) on-demand computation over Calvin Archive graph — post-learning drop in Φ triggers quarantine rather than direct integration
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
- Rust crate or module for the Calvin Archive
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

### Parallel Track — Hosted And Team Integrations

- **ENT-1** — Harkonnen as an MCP server (resources, tools, and prompts via `rmcp` crate)
- **ENT-2** — External connector surface (OpenAPI spec, connector manifests, and workflow templates for MCP-limited clients)
- **ENT-3** — OIDC authentication (JWT validation alongside API key path)
- **ENT-4** — Knowledge base ingest (wiki, drive, and document-system connectors with incremental sync)
- **ENT-5** — ChatOps integration (Slack, Discord, Teams, or similar notification + approval surfaces)
- **ENT-6** — Clone-local profile and hosted deployment hardening

EI-1 should land before any hosted or team surface. ENT-1 is the foundation for all ENT tracks.

---

## Part 9 — Benchmark Strategy

Benchmark wiring advances with implementation phases, but the current engineering pass is intentionally narrow. Use the native adapters as guardrails and avoid expanding public benchmark coverage until the v1 end-to-end path is closed.

### Benchmark Matrix

**Memory and retrieval (vs Mem0 / MindPalace / Zep):**

| Suite | What it measures | Status |
| --- | --- | --- |
| LongMemEval | Long-term assistant memory, temporal reasoning, belief updates | Native adapter live |
| LoCoMo | Long-horizon dialogue memory | Native adapter live |
| FRAMES | Multi-hop factual recall (Mem0 publishes here) | Native adapter live; OB1 default recall path should be measured against local-vector baseline |
| StreamingQA | Belief-update accuracy when facts change | Native adapter live; persisted-history smoke published on `lm-studio-local` |
| HELMET | Retrieval precision/recall | Native adapter live |

**Coding loop (vs OpenCode / Aider / SWE-agent):**

| Suite | What it measures | Status |
| --- | --- | --- |
| SWE-bench Verified | Human-validated issue resolution | Adapter-ready; Phase 2 |
| LiveCodeBench | Recent competitive programming, no contamination | Phase 2 |
| Aider Polyglot | Multi-language coding, public leaderboard | Phase 2 |
| DevBench | Full software lifecycle | Phase 10 |
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
| Spec Adherence Rate | Completeness and precision vs stated spec | Phase 10 |
| Hidden Scenario Delta | Gap between visible test pass rate and hidden scenario pass rate | Phase 10 |
| Causal Attribution Accuracy | Seeded failure corpus, top-1 / top-3 | Phase 7 |

### Phase-Aligned Benchmark Gates

| Phase | Key benchmarks unlocked |
| --- | --- |
| v1 | Decision audit completeness, memory supersession accuracy (StreamingQA), WrongAnswer classification rate, operator-model context visibility |
| Phase 2 | SWE-bench Verified readiness, LiveCodeBench, Aider Polyglot |
| Phase 10 | spec adherence rate, hidden scenario delta, DevBench; twin fidelity remains optional diagnostic telemetry |
| Phase 4b | StreamingQA belief-update accuracy |
| Phase 5-D | PackChat-to-OB1 candidate capture and retrieval smoke; Calvin promotion contract smoke |
| Phase 5b | FRAMES re-run (OB1 default recall), LongMemEval / LoCoMo regression check |
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

**What makes it distinct:** Pearl-hierarchy causal memory, typed agent identity (the Calvin Archive), hidden scenario evaluation, and a benchmark suite that includes tests no competitor can run.
