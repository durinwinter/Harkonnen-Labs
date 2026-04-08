# Coobie Implementation Spec

This document translates Coobie's memory model into implementation-facing architecture for Harkonnen Labs.

It sits one layer below [COOBIE.md](./COOBIE.md):

- `COOBIE.md` explains the memory strategy
- `COOBIE_SPEC.md` explains how to build it

## Current implementation notes

The repo now implements an important subset of this design already:

- file-backed durable memory with indexed Markdown notes
- extracted ingest from files and URLs into either core memory or repo-local project memory
- repo-local causal evidence bundles and annotation validation for teaching Coobie pattern examples and cause/effect windows
- project continuity artifacts under `.harkonnen/` for external repos
- exploration logs, dead-end registry, stale-memory mitigation history, and retriever-forge evidence that Coobie can cite during preflight
- self-tuning manifest signals such as recall counts, load counts, and contribution counts

The sections below still describe the target architecture, but they should be read as a layered build plan on top of a non-trivial current implementation, not a greenfield spec.

## Design Thesis

Coobie should not be implemented as a single vector store with summaries attached.

She should behave like a local-first memory system with explicit division of labor:

- working memory for immediate control
- episodic memory for trace fidelity
- durable semantic memory for facts, policies, and procedures
- causal memory for explanation and intervention
- team blackboard memory for multi-agent coordination
- consolidation for promotion, strengthening, abstraction, and forgetting

## Biology-Inspired Mapping

The most useful biological analogy is division of labor, not one giant memory blob.

### Working Memory / Executive Control

Analogy:

- prefrontal executive control
- tight capacity
- active prioritization
- immediate action selection

Coobie behavior:

- hold only the state needed for the next few decisions
- preserve current hypotheses, blockers, and active references
- summarize aggressively instead of appending blindly

### Episodic Capture

Analogy:

- hippocampal episode binding
- fast capture of who, what, when, where, and result

Coobie behavior:

- capture event traces rapidly before they decay into logs
- preserve sequence, tool context, action, observation, outcome, and confidence
- keep references to artifacts and logs rather than duplicating them everywhere

### Consolidation

Analogy:

- hippocampal-cortical systems consolidation
- remote memory reorganization over time

Coobie behavior:

- promote high-value traces into durable memory after task and run boundaries
- compress repetitive episodes into reusable lessons
- keep learning active instead of freezing a static archive

### Strengthening and Pruning

Analogy:

- LTP-like strengthening of useful links
- weakening of stale or low-value traces

Coobie behavior:

- increase confidence in repeated predictive relationships
- reduce trust in memories that no longer generalize
- prune low-value episodic clutter before it pollutes retrieval

## Memory Layers

### Layer A: Working Memory

Purpose:

- keep the current run coherent
- support the next few actions cheaply
- provide the immediate operational state for the pack

Contents:

- current task summary
- latest agent actions
- unresolved blockers
- current hypotheses
- recent tool outputs
- references to relevant artifacts and durable memory
- role-scoped blackboard slices

Desired properties:

- tiny and fast
- explicit token budget
- aggressively decayed
- not used as an archive

Recommended local backing:

- in-process state for single-process runs
- SQLite row or JSON snapshot keyed by `run_id`
- optional Redis later for cross-process hot state

### Layer B: Episodic Memory

Purpose:

- preserve sequence-rich run experience
- retain action, tool, observation, outcome, and context fidelity
- store causal candidates before later consolidation

Episode record should include:

- `episode_id`
- `run_id`
- `timestamp_range`
- `agents_involved`
- `goals`
- `state_before`
- `action_taken`
- `tool_context`
- `observed_result`
- `confidence`
- `success_label`
- `candidate_causal_links`
- `embedding_ref`
- `artifact_refs`
- `event_refs`

Novel requirement:

Store causal hypotheses as first-class structures, not just text.

Candidate relations should include:

- `because_of`
- `blocked_by`
- `enabled_by`
- `degraded_by`
- `recovered_by`

Recommended local backing:

- append-only SQLite journal
- per-run JSONL or event files under `factory/workspaces/<run-id>/run/episodes/`

### Layer C: Durable Semantic Knowledge

Purpose:

- store stable facts, abstractions, invariants, and learned procedures
- support typed queries and function-backed semantic reasoning
- hold what should still matter next week

This is the best place for TypeDB.

Store:

- agent roles
- tool capabilities
- system invariants
- policies
- recurring patterns
- error families
- artifact classes
- consolidated lessons
- learned procedures
- organization-specific constraints

Role of TypeDB:

- typed schema
- subtyping and polymorphism
- explicit typed relations
- function-backed semantic reasoning in TypeDB 3.x
- query-time reasoning over a knowledge graph instead of brittle app-side joins

TypeDB 3.x is now a much better fit for a Rust-first stack than older JVM-era
assumptions implied, but it is still an external database service with real ops
cost. TypeDB should not be the hot path buffer. It is the durable semantic layer.

### Layer D: Causal Graph

Purpose:

- distinguish similarity from causality
- answer intervention and counterfactual questions
- expose reusable failure and recovery pathways

This is the bridge between “this looks similar” and “this action changed the outcome.”

Core causal levels:

- association: A co-occurred with B
- intervention: when X changed, Y changed
- counterfactual: had X not happened, would Y still have happened?

Recommended graph edge types:

- `caused`
- `contributed_to`
- `prevented`
- `preceded`
- `invalidated`
- `depended_on`
- `corrected`
- `escalated`

Recommended local backing:

- in-memory causal graph with `petgraph`
- persisted causal structures in TypeDB and/or SQLite projections

### Layer E: Consolidation Engine

Purpose:

- decide what survives promotion
- strengthen useful relationships
- compress repeated episodes into lessons
- prune low-value noise

Inputs:

- episodic traces
- run outcomes
- recurrence counts
- novelty scores
- causal salience
- blackboard importance flags

Outputs:

- consolidated lessons
- updated causal links
- strengthened or weakened confidence weights
- archival and pruning decisions

Operating modes:

- micro-consolidation after significant task boundaries
- batch consolidation after run completion

### Layer F: Team Blackboard

Purpose:

- coordinate the pack on a shared operational view
- keep agents aligned without forcing all of them to reread everything
- serve as shared state, not durable truth

Board slices:

- Mission Board: goals, constraints, acceptance status
- Action Board: who is doing what now
- Evidence Board: artifacts, logs, results, scenario outcomes
- Memory Board: recalled lessons, causal precedents, policy reminders

Role-scoped views:

- Scout sees ambiguity, requirements, and intent gaps
- Mason sees implementation plan, blockers, and relevant lessons
- Bramble sees validation focus and recurring failure patterns
- Sable sees scenario deltas and evaluation evidence references
- Keeper sees policy state and boundary flags
- Coobie sees memory references and memory health across all boards

Recommended local backing:

- phase 1: SQLite plus per-run `board.json`
- phase 2: optional Redis cache for hot shared state

## Causal AI Requirements

Pure semantic retrieval is not enough for Coobie.

Semantic recall answers:

- what looks similar?

Causal memory should answer:

- what led to what?
- which intervention changed the outcome?
- what would likely have happened otherwise?

Design rule:

- semantic retrieval finds related material
- causal memory turns related material into reusable lessons

## Storage Roles

### Filesystem

Role:

- canonical source of truth for memory documents, imports, extracted text sidecars, and reflections

Recommended paths:

- `factory/memory/`
- `factory/memory/imports/`
- `factory/memory/index.json`
- `<repo>/.harkonnen/project-memory/`
- `<repo>/.harkonnen/project-memory/imports/`
- `<repo>/.harkonnen/project-manifest.json` and related continuity artifacts

### SQLite

Role:

- short-term memory snapshots
- append-only episodic journal
- team board state
- agent claims and checkpoints
- run-local causal projections when needed

### TypeDB

Role:

- durable semantic graph
- typed relations and TypeDB 3.x function-backed reasoning
- inferred higher-order lessons

Integration note:

- prefer the official TypeDB 3.x Rust-facing driver surface behind a local adapter
- keep SQLite as the fast local run/episode store
- do not assume TypeDB is embedded into the Rust process just because the core is now Rust

### Qdrant

Role:

- semantic acceleration over extracted text, summaries, and episodic embeddings
- retrieval by semantic neighborhood plus payload filters

Qdrant is not the canonical store.

### Redis

Role:

- optional hot cache for working memory and blackboard coordination
- transient claims, locks, and low-latency shared state

Redis is not the durable truth layer.

### AnythingLLM

Role:

- optional local retrieval and ingestion surface
- local-model document query path for imported material

## Rust Module Layout

Suggested module tree:

```text
src/
  memory/
    mod.rs
    types.rs
    working.rs
    episodic.rs
    semantic.rs
    causal.rs
    consolidation.rs
    blackboard.rs
    retrieval.rs
    extraction.rs
    td.rs
    qdrant.rs
    redis.rs
```

Suggested responsibilities:

- `working.rs`: current run working set, budgets, summaries, active hypotheses
- `episodic.rs`: append-only episode capture and journaling
- `semantic.rs`: durable lesson and policy retrieval API
- `causal.rs`: causal graph build/update/query logic
- `consolidation.rs`: promotion, reflection, weighting, pruning
- `blackboard.rs`: role-scoped team memory surfaces
- `retrieval.rs`: hybrid retrieval orchestration
- `extraction.rs`: PDF/OCR/document text extraction
- `td.rs`: TypeDB 3.x adapter and query helpers
- `qdrant.rs`: Qdrant indexing and semantic lookup
- `redis.rs`: optional hot-state backend

## Rust Traits

These are the core interfaces I would stabilize first.

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

pub trait BlackboardStore {
    fn load_board(&self, run_id: &str) -> anyhow::Result<BlackboardState>;
    fn update_board(&self, run_id: &str, patch: &BlackboardPatch) -> anyhow::Result<()>;
    fn role_view(&self, run_id: &str, role: AgentRole) -> anyhow::Result<RoleScopedBoardView>;
}
```

## Core Rust Types

```rust
pub enum MemoryTier {
    Working,
    Episodic,
    Semantic,
    Causal,
    Team,
}

pub struct EpisodeRecord {
    pub episode_id: String,
    pub run_id: String,
    pub agent: String,
    pub goal_ids: Vec<String>,
    pub state_before: String,
    pub action_taken: String,
    pub tool_context: Vec<String>,
    pub observed_result: String,
    pub success: bool,
    pub confidence: f32,
    pub artifact_refs: Vec<String>,
    pub candidate_links: Vec<CausalLinkRecord>,
}

pub struct CausalLinkRecord {
    pub link_id: String,
    pub kind: CausalLinkKind,
    pub from_event_id: String,
    pub to_event_id: String,
    pub confidence: f32,
    pub evidence_refs: Vec<String>,
}

pub enum CausalLinkKind {
    Caused,
    ContributedTo,
    Prevented,
    Preceded,
    Invalidated,
    DependedOn,
    Corrected,
    Escalated,
}
```

## TypeDB 3 Schema Sketch

TypeDB should hold the durable semantic graph and function-backed reasoning layer.

The following is an implementation sketch, not a final locked schema.

```typeql
entity agent,
  owns agent-id,
  owns name;

entity role,
  owns role-name;

entity goal,
  owns goal-id,
  owns summary,
  owns status;

entity task,
  owns task-id,
  owns summary,
  owns status;

entity episode,
  owns episode-id,
  owns started-at,
  owns ended-at,
  owns confidence,
  owns success;

entity observation,
  owns observation-id,
  owns summary;

entity action,
  owns action-id,
  owns summary;

entity tool-call,
  owns tool-call-id,
  owns tool-name,
  owns summary;

entity outcome,
  owns outcome-id,
  owns summary,
  owns success;

entity artifact,
  owns artifact-id,
  owns path,
  owns artifact-kind;

entity policy,
  owns policy-id,
  owns summary;

entity constraint,
  owns constraint-id,
  owns summary;

entity lesson,
  owns lesson-id,
  owns summary,
  owns confidence,
  owns memory-tier;

entity failure-mode,
  owns failure-mode-id,
  owns summary;

entity causal-link,
  owns causal-link-id,
  owns link-kind,
  owns confidence;

relation performed-action,
  relates performer,
  relates performed;

relation action-produced-outcome,
  relates cause,
  relates effect;

relation outcome-updated-goal,
  relates outcome-source,
  relates updated-goal;

relation episode-contained-observation,
  relates parent-episode,
  relates contained-observation;

relation lesson-derived-from-episode,
  relates derived-lesson,
  relates source-episode;

relation causal-link-connects-events,
  relates link,
  relates source-event,
  relates target-event;

relation policy-constrained-action,
  relates governing-policy,
  relates constrained-action;

relation artifact-supported-outcome,
  relates evidence-artifact,
  relates supported-outcome;
```

## Query Patterns Coobie Must Support

Examples of queries Coobie should answer locally:

- retrieve lessons relevant to a product, role, and failure pattern
- find episodes where Mason recovered after a blocked validation loop
- explain which interventions most often preceded successful recovery
- identify repeated policy constraints affecting a given tool call type
- return Scout-only blackboard view for the current run
- return all memory-bearing events for a run regardless of subtype

## Retrieval Strategy

Hybrid retrieval should combine:

- exact and typed filtering from TypeDB
- semantic recall from Qdrant
- causal-path lookup from episodic and causal memory
- current-run working memory and blackboard state

Ordering rule:

1. working memory for immediate control
2. blackboard for current shared context
3. typed filters for exact constraints and lessons
4. semantic recall for related memories
5. causal lookup for explanation and intervention planning

## Extraction Plan

To keep the system local, imported assets should be processed by local extractors.

Recommended tools:

- `pdftotext` from `poppler-utils`
- `tesseract` for OCR
- optional `libreoffice --headless` or `pandoc` for office documents

Extraction output should be written into local sidecars, then fed into:

- filesystem source of truth
- Qdrant indexing
- optional AnythingLLM sync

## Observability and Safety

Every memory mutation should be reconstructable.

Recommended approach:

- append-only event journal for memory mutations
- `tracing` spans around retrieval and consolidation
- explicit provenance on lessons and causal links
- role-scoped blackboard views to reduce unnecessary memory exposure

## Recommended Build Sequence

1. stabilize working memory and blackboard schemas in SQLite
2. add append-only episodic capture
3. add micro- and run-level consolidation
4. add local extraction pipeline
5. add Qdrant integration for semantic acceleration
6. add TypeDB 3.x semantic graph layer
7. add Redis only if hot shared-state pressure justifies it

## Strongest Implementation Rule

Do not let Coobie collapse into one storage primitive.

The design only works if each layer keeps its job:

- working memory controls
- episodic memory preserves traces
- semantic memory stores durable knowledge
- causal memory explains and suggests interventions
- team memory coordinates the pack
- consolidation decides what deserves to survive
