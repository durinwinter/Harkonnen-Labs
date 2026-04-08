# Harkonnen Labs Architecture

Harkonnen Labs is a local-first, spec-driven AI software factory. Humans define intent, judge outcomes, and evolve the system. Agents perform the implementation work inside a constrained, observable factory.

This document is the conceptual north star for the MVP scaffold in this repository and the next layers we build on top of it.

## Current implementation snapshot

The current build is no longer just a bare scaffold. Today Harkonnen includes:

- core and project-local Coobie memory with a durable split between `factory/memory/` and `<repo>/.harkonnen/project-memory/`
- extracted document and URL ingest for PDFs, text files, office docs, HTML, and websites
- repo-local continuity artifacts such as `project-scan`, `resume-packet`, `strategy-register`, `memory-status`, and `stale-memory-history`
- a residue-style exploration log and dead-end registry
- stale-memory severity scoring plus mitigation planning and outcome tracking
- a bounded retriever-themed inner forge with plan review, execution reports, and hook artifacts

The architecture below still describes the target model, but these capabilities are already live in the current repo.

## What Harkonnen Labs Is

Harkonnen Labs is not a chatbot, not just an IDE assistant, and not a thin wrapper around code generation. It is a factory system with a clear separation between the factory itself and the product it builds.

The system is centered on these ideas:

- humans define intent
- agents perform implementation
- correctness is judged by behavioral outcomes
- the factory stays separate from the product
- the system accumulates memory over time
- hidden scenarios matter more than visible tests

The long-term goal is a Level 5 software factory where the center of gravity shifts away from handwritten implementation and toward stronger specifications, better evaluation, safer autonomy, and reusable engineering memory.

## Why Harkonnen Labs Should Exist

Most AI coding workflows make local moments faster while making the overall system messier. The usual failure modes are familiar:

- more half-right code
- more review burden
- more context switching
- more hidden errors
- more false confidence

Harkonnen Labs exists to replace that workflow rather than decorate it.

### The Problems It Solves

#### 1. Implementation is no longer the main bottleneck

As coding gets cheaper, intent quality and evaluation quality matter more. The factory should optimize for:

- precise intent
- strong specs
- strong evaluation
- strong memory
- controlled autonomy

#### 2. Human code review does not scale in a dark factory

If agents generate large amounts of code quickly, traditional diff review becomes the bottleneck. The factory therefore needs:

- hidden behavioral scenarios
- evidence-based acceptance
- artifact bundles
- optional code inspection instead of mandatory inspection

#### 3. AI tools get dangerous when they wander

Without role separation, agents step on each other. The factory needs:

- named specialist agents
- strict permissions
- tool boundaries
- policy enforcement

#### 4. Organizations lose knowledge constantly

Specs, mistakes, architecture decisions, and workarounds disappear into chat logs and people. The factory needs:

- local memory
- retrieval
- pattern reuse
- failure memory
- decision capture

#### 5. Trust collapses when the system is invisible

A real factory should feel inspectable and legible. The factory needs:

- observable execution
- visible run state
- visible agent behavior
- visible artifact flow
- visible scenario outcomes

## Core Design Principles

- Spec-first development: bad output usually starts with weak specs, not weak coding.
- Outcome-based judgment: acceptance should be driven by evidence, not vibes.
- Local-first operation: state, logs, specs, scenarios, and artifacts should be owned locally.
- Role separation: agents should have explicit jobs and bounded powers.
- Boundary enforcement: factory, product, memory, twin, and hidden scenarios must remain distinct.
- Compound learning: each run should make future runs better.
- Human authority: humans define goals and make acceptance decisions.

## System Components

### 1. Specification Intake Layer

Purpose: load, validate, normalize, and trace human-written specs.

Why it matters: this is the true front door of the factory.

Responsibilities:

- load YAML or Markdown specs
- validate required fields
- normalize into an intent package
- flag ambiguity
- assign run IDs and metadata
- preserve traceability from spec to artifact bundle

Current repo mapping:

- [src/spec.rs](./src/spec.rs)
- [src/models.rs](./src/models.rs)
- [src/cli.rs](./src/cli.rs)

### 2. Orchestrator

Purpose: coordinate every run, phase, and agent handoff.

Why it matters: without orchestration, there is no factory, only a loose pile of tools.

Responsibilities:

- create jobs
- assign phases
- invoke agents in sequence
- pass context packages
- track state transitions
- capture logs and outputs
- handle retries and failures
- prevent role violations

Current repo mapping:

- [src/orchestrator.rs](./src/orchestrator.rs)
- [src/main.rs](./src/main.rs)

### 3. Agent Role System

Purpose: define named specialist agents with constrained responsibilities, tools, and boundaries.

Why it matters: specialist agents are easier to reason about, observe, and constrain than general-purpose autonomous workers.

Current repo mapping:

- [factory/agents/profiles/scout.yaml](./factory/agents/profiles/scout.yaml)
- [factory/agents/profiles/mason.yaml](./factory/agents/profiles/mason.yaml)
- [factory/agents/profiles/piper.yaml](./factory/agents/profiles/piper.yaml)
- [factory/agents/profiles/bramble.yaml](./factory/agents/profiles/bramble.yaml)
- [factory/agents/profiles/sable.yaml](./factory/agents/profiles/sable.yaml)
- [factory/agents/profiles/ash.yaml](./factory/agents/profiles/ash.yaml)
- [factory/agents/profiles/flint.yaml](./factory/agents/profiles/flint.yaml)
- [factory/agents/profiles/coobie.yaml](./factory/agents/profiles/coobie.yaml)
- [factory/agents/profiles/keeper.yaml](./factory/agents/profiles/keeper.yaml)
- [factory/agents/personality/labrador.md](./factory/agents/personality/labrador.md)

#### Agent Pack

##### Scout

Role: spec retriever.

Responsibilities:

- parse specs
- identify ambiguity
- produce normalized intent packages

##### Mason

Role: build retriever.

Responsibilities:

- generate code
- modify code
- implement multi-file changes

##### Piper

Role: tool retriever.

Responsibilities:

- run tools
- fetch docs
- execute build helpers

##### Bramble

Role: test retriever.

Responsibilities:

- generate visible tests
- run lint
- run build
- run internal validation loops

##### Sable

Role: scenario retriever.

Responsibilities:

- execute hidden scenarios
- compare behavior
- produce evaluation reports

##### Ash

Role: twin retriever.

Responsibilities:

- provision twins
- mock dependencies
- prepare test environments

##### Flint

Role: artifact retriever.

Responsibilities:

- collect outputs
- package bundles
- summarize artifacts

##### Coobie

Role: memory retriever.

Responsibilities:

- retrieve prior specs
- retrieve patterns
- retrieve failures
- store new knowledge

##### Keeper

Role: boundary retriever.

Responsibilities:

- enforce policy
- prevent unsafe actions
- protect secrets
- maintain factory-product separation

### 4. Memory System

Purpose: store and retrieve local knowledge that compounds over time.

Why it matters: a factory that forgets everything pays the same intelligence tax on every run.

What it should store:

- prior specs
- prior outcomes
- decisions
- failures
- implementation patterns
- architecture notes
- scenario lessons
- product-specific known constraints

#### Coobie Memory Layers

Coobie should manage three distinct layers instead of one undifferentiated note pile:

- short-term memory: current run state, recent tool outputs, current plan, and active team context
- long-term memory: durable notes, imported assets, run reflections, and reusable patterns
- team memory: shared pack state, handoffs, claims, blockers, and run board status

Recommended local stack:

- filesystem is the source of truth for durable memory
- SQLite holds structured run state and early team memory
- repo-local `.harkonnen/` state carries project-specific continuity when Harkonnen works on an external codebase
- Qdrant is the optional semantic index for long-term memory
- Redis is optional hot memory for shared coordination, not the canonical record
- AnythingLLM remains an optional local retrieval accelerator

Current repo mapping:

- [src/memory.rs](./src/memory.rs)
- [src/orchestrator.rs](./src/orchestrator.rs)
- [factory/agents/profiles/coobie.yaml](./factory/agents/profiles/coobie.yaml)
- [COOBIE.md](./COOBIE.md)
- [factory/memory](./factory/memory)
- project-local `<repo>/.harkonnen/project-memory/` on external codebases

### 5. Hidden Scenario System

Purpose: protect the true behavioral acceptance criteria from implementation agents.

Why it matters: if the build agent can see the exam, it will optimize for the exam.

Responsibilities:

- store hidden behavioral scenarios outside the implementation workspace
- isolate scenario access to Sable and Keeper
- produce clear pass/fail evidence
- preserve hidden truth while still exposing useful diagnostics

Current repo status: implemented via protected scenario files and run-state evaluation, with room for richer black-box behavioral scenarios.

### 6. Digital Twin Environment

Purpose: simulate external systems safely.

Why it matters: autonomous work against live systems is either fake, dangerous, or both.

What it should include:

- mocked APIs
- fake auth or token services
- fixture-backed databases
- queue or event simulators when relevant
- stable endpoints for integration evaluation

Current repo status: partial. The current system builds safe twin manifests, dependency stubs, and optional twin narratives, but not full external-system provisioning.

### 7. Internal Validation Layer

Purpose: provide fast local signals before scenario evaluation.

Why it matters: hidden scenarios are the truth, but fast visible validation is still useful.

Responsibilities:

- compile
- lint
- run visible tests
- capture stdout and stderr
- retry with structured feedback where appropriate

Current repo status: partial through the CLI and Rust build path.

### 8. Artifact Packaging Layer

Purpose: gather the outputs of a run into one inspectable acceptance bundle.

Why it matters: humans in a Level 5 system judge outcomes, so those outcomes need to be easy to inspect.

Bundle contents should include:

- spec used
- summary of what changed
- internal validation report
- hidden scenario report
- logs
- build outputs
- release candidate files
- acceptance status

Current repo mapping:

- [src/reporting.rs](./src/reporting.rs)
- [src/orchestrator.rs](./src/orchestrator.rs)

### 9. Policy Engine

Purpose: enforce path boundaries, command boundaries, permissions, and role separation.

Why it matters: safety must be built into the architecture.

Responsibilities:

- workspace-scoped writes
- no hidden scenario access outside approved roles
- no unrestricted shell execution
- no secret access outside approved boundaries
- no product-factory contamination
- no destructive actions without approval

Current repo mapping:

- [src/policy.rs](./src/policy.rs)
- [factory/agents/profiles/keeper.yaml](./factory/agents/profiles/keeper.yaml)

### 10. Product Workspace Manager

Purpose: create isolated workspaces for each run.

Why it matters: runs should be reproducible, separable, and disposable.

Responsibilities:

- create per-run workspaces
- copy or mount target products
- attach logs and artifacts
- isolate outputs from one another

Current repo mapping:

- [src/workspace.rs](./src/workspace.rs)
- [factory/workspaces](./factory/workspaces) when runs are created

### 11. CLI

Purpose: provide the first usable control surface for the factory.

Why it matters: the CLI is the fastest path to a working factory spine.

Current commands:

- `spec validate`
- `run start`
- `run status`
- `run report`
- `artifact package`
- `memory index`
- `memory search`
- `memory import`
- `setup check`
- `setup init`
- `setup claude-pack`
- `serve`

Current repo mapping:

- [src/cli.rs](./src/cli.rs)

### 12. Web UI / Operations View

Purpose: make the factory observable and trustworthy through a live operations dashboard.

Why it matters: if the system is invisible, people will default back to manual code inspection.

The UI should shift trust from handwritten diff review to:

- process visibility
- behavioral evidence
- role accountability
- scenario results
- artifact summaries

Current repo status: implemented as a React/Vite Pack Board with API-backed run detail views, with further workflow triggers still open for expansion.

## UI Concept: The Pack Board

The web UI should feel like a factory operations dashboard, not a generic admin console. It should look like a glass wall into a machine room where a coordinated pack is working a run.

### Main Regions

#### 1. Top Status Bar

Show:

- current run ID
- product name
- spec title
- current phase
- overall health
- elapsed time
- last updated timestamp

#### 2. Agent Panel Grid

Each agent gets a card showing:

- Labrador icon or illustration
- agent name
- role title
- current state
- current task summary
- latest log line
- progress indicator
- output or artifact count
- warning or error badge when needed

#### 3. Run Timeline

Show the phases of a run, such as:

- spec intake
- memory enrichment
- build
- internal validation
- twin setup
- scenario evaluation
- artifact packaging
- ready for decision

#### 4. Evidence and Artifact Drawer

Show:

- generated reports
- scenario results
- logs
- bundle contents
- summaries
- downloadable artifacts

#### 5. System Map / Boundaries View

Show:

- factory side
- product side
- scenario vault
- twin environment
- memory store

This reinforces the architecture and makes boundary violations easier to understand.

## Agent Card Design

Each card should use a distinct but stylistically consistent black-labrador motif. The UI should feel industrial but warm, operational rather than cute.

Each card should include:

### Header

- dog icon
- agent name
- role subtitle

### Body

- current action
- run phase
- tools in use
- last completed step
- input and output summary

### Footer

- status chip: idle, running, blocked, failed, or complete
- elapsed task time
- view logs action

## Labrador Icon Concepts

- Scout: satchel or rolled spec tube
- Mason: blueprint, wrench, or hard-hat motif
- Piper: utility harness or toolkit tag
- Bramble: goggles or checkmark motif
- Sable: sentinel posture with shield or clipboard
- Ash: terrain lines, cubes, or network nodes
- Flint: parcel, bundle, or document canister
- Coobie: archive box, tag, or buried-bone map motif
- Keeper: gate, shield, or key iconography

## Recommended UI Components

### Global Components

- `RunHeader`
- `PhaseTimeline`
- `HealthStatusBar`
- `ArtifactDrawer`
- `SystemBoundaryMap`

### Agent Components

- `AgentGrid`
- `AgentCard`
- `AgentDetailDrawer`
- `AgentLogPanel`
- `AgentToolUsagePanel`

### Evaluation Components

- `ScenarioSummaryPanel`
- `ValidationSummaryPanel`
- `EvidenceViewer`
- `AcceptanceDecisionPanel`

### Knowledge Components

- `MemoryHitsPanel`
- `RetrievedPatternsPanel`
- `PriorFailuresPanel`

### Ops Components

- `TwinStatusPanel`
- `PolicyEventsPanel`
- `WorkspaceStatusPanel`

## Suggested Visual Language

The Pack Board should feel:

- industrial but warm
- operational, not cute
- muted or dark-toned
- card-based and inspectable
- iconographically consistent
- readable at a glance

Think control room for an AI machine shop, not pet-adoption website with logs.

## Delivery Priorities

### Phase 1

- run header
- agent grid
- status chips
- log panel
- phase timeline

### Phase 2

- artifact drawer
- scenario summary
- memory hits
- policy events

### Phase 3

- system map
- twin status
- richer per-agent visual state
- run comparison history

## MVP Spine in This Repository

The current scaffold already establishes the factory spine:

- spec loading and validation
- run creation and persistence in SQLite
- per-run workspace creation
- artifact packaging
- PackChat backend thread/message persistence plus directed agent replies
- checkpoint reply and unblock control-plane routes
- causal memory and project/core memory ingestion
- policy boundary enforcement and Keeper coordination
- agent profile definitions
- CLI entry points
- Pack Board web UI

The next practical layers to build are:

1. Bramble real test execution
2. Ash live twin provisioning
3. episodic layer enrichment and causal-link persistence
4. operator-reviewed consolidation workbench
5. TypeDB 3.x semantic layer

## Final Definition

### What

A local-first, spec-driven, Level 5 AI software factory.

### Why

To replace human implementation-centric workflows with autonomous build-and-evaluate loops that are safer, more observable, and more reusable over time.

### What to Include

- spec intake
- orchestrator
- role agents
- memory layer
- hidden scenarios
- digital twins
- internal validation
- artifact packaging
- policy engine
- workspace isolation
- CLI
- Pack Board web UI

### Why Those Components

Each one removes a specific failure mode:

- ambiguity
- orchestration drift
- role confusion
- knowledge loss
- test gaming
- unsafe integration
- invisible failure
- unsafe autonomy
- trust collapse

### UI

A live web-based Pack Board where each Labrador agent has a distinct graphic, current task, health, logs, and outputs, with the whole system visible as a working software factory instead of a mystery box.
