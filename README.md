# Harkonnen Labs

## A Local-First, Causally-Aware AI Software Factory (WIP)

Harkonnen Labs is a **multi-agent software execution system** that transforms specifications into validated software artifacts while accumulating **structured operational knowledge** across runs.

At its core, Harkonnen is designed to solve a specific failure mode in modern LLM systems:

> LLM pipelines are *stateless, similarity-driven, and non-causal* — they cannot reliably improve from experience.

Harkonnen introduces a **stateful, causally-informed execution model** where:

* **Agents** perform bounded roles in a production pipeline
* **Memory** persists across runs with explicit structure
* **Causal reasoning** separates correlation from intervention
* **Policy** governs what actions are allowed, not just what is possible

The result is a system that does not just generate software — it **learns how to produce better software over time**.

---

## Conceptual Model

Harkonnen operates as a **closed-loop software factory**:

```text
Specification
   ↓
Multi-Agent Execution
   ↓
Validation (including hidden scenarios)
   ↓
Artifact Production
   ↓
Memory Ingestion (episodic)
   ↓
Consolidation → Semantic + Causal Knowledge
   ↓
Improved Future Execution
```

This loop converts **execution traces into reusable knowledge**.

---

## System Components

### 1. Agent Pack (Execution Layer)

Harkonnen decomposes execution into nine specialist agents:

| Agent | Role | What it does |
| --- | --- | --- |
| **Scout** | Spec retriever | Parses specs, flags ambiguity, produces normalized intent packages |
| **Mason** | Build retriever | Generates and modifies code; iterates up to 3 times on build failure with structured fix-loop reasoning |
| **Piper** | Tool retriever | Runs build tools, fetches docs, executes helpers with live stdout/stderr streaming |
| **Bramble** | Test retriever | Generates visible tests, runs lint and build, feeds pass/fail into Coobie scoring |
| **Sable** | Scenario retriever | Executes hidden behavioral scenarios; its results are the ground truth, not Bramble's |
| **Ash** | Twin retriever | Provisions compose-backed local twins with simulated fallback for safe external-system validation |
| **Flint** | Artifact retriever | Collects outputs, packages artifact bundles for inspection and evidence |
| **Coobie** | Memory retriever | Manages layered memory — episodic capture, causal reasoning, Palace patrol, and consolidation |
| **Keeper** | Boundary retriever | Enforces policy, guards role boundaries, owns file-claim coordination |

This is not a monolithic agent — it is a **role-constrained system with explicit handoffs**.
Scout, Sable, and Keeper are pinned to Claude. All others route per setup configuration.

---

### 2. Coobie (Layered Memory System)

Coobie implements a **multi-layer memory architecture** that goes beyond a flat note store:

| Layer | Purpose | Status |
| --- | --- | --- |
| Working Memory | Current run state (compressed, ephemeral, token-budgeted) | Live |
| Episodic Memory | Ordered execution traces (state → action → result) with phase attribution | Live |
| Semantic Memory | Stable facts, patterns, invariants — hybrid vector + keyword retrieval | Live |
| Causal Memory | Intervention-aware cause/effect relationships scored per run | Live |
| Team Blackboard | Shared agent coordination state across four named slices | Live |
| Consolidation | Promotion, pruning, and abstraction with operator-reviewed keep/discard/edit flow | Live |

#### Coobie Palace

The Palace is a **spatially-organized compound recall layer** built on top of Coobie's causal memory. Related failure causes are grouped into named **Dens** that Coobie patrols before every run:

| Den | Residents | What it covers |
| --- | --- | --- |
| Spec Den | `SPEC_AMBIGUITY`, `BROAD_SCOPE` | Failures from unclear or over-scoped specs |
| Test Den | `TEST_BLIND_SPOT` | Visible tests passed but hidden scenarios caught failures |
| Twin Den | `TWIN_GAP` | Simulated environment didn't match production |
| Pack Den | `PACK_BREAKDOWN` | Degraded or incomplete Labrador phase execution |
| Memory Den | `NO_PRIOR_MEMORY` | Factory ran cold with no relevant prior context |

A **Patrol** walks each den before a run, computing a compound **Scent** — when multiple causes from the same den have fired together, the whole den is elevated, not just the individual signal. Results inject directly into the preflight briefing's `required_checks`, `guardrails`, and `open_questions`. The principle: *the whole den smells, not just one corner*.

#### Causal Reasoning

Coobie tracks three levels of the causal hierarchy:

* **Association** — co-occurrence patterns across runs
* **Intervention** — outcome changes due to explicit actions, recorded in the causal link table
* **Counterfactuals** — inferred alternative outcomes

Live features:

* Causal streaks — a cause that fires repeatedly across runs is escalated in the preflight briefing
* Cross-run pattern detection — identifies causes that cluster on specific spec types or phases
* Phase 3 preflight guidance — spec-scoped cause history drives `required_checks` before each run
* Palace compound recall — den-level streak weight elevates compound failures beyond individual cause scores
* Cross-phase causal graph — episode-to-episode links record `phase_sequence` and `failure_triggered` relationships
* Pearl hierarchy labels — Coobie hypotheses and causal graph edges are tagged associational, interventional, or counterfactual
* Multi-hop retrieval chain — query responses now trace hop-by-hop memory retrieval depth and supporting evidence
* Open Brain (OB1) shared recall — Harkonnen can pull semantic thought matches from the shared OB1 MCP memory layer when `OPEN_BRAIN_MCP_URL` is configured
* Memory invalidation / fact-update tracking — superseded facts are surfaced as stale rather than silently overwritten
* Causal feedback loop — Sable's scenario rationale is written back to project memory as evidence

---

### 3. PackChat — Conversational Control Plane

PackChat shifts the factory from pure autonomous orchestration to **supervised autonomy** — you stay in the loop through the same conversation surface while the pack works.

* **Chat threads** scoped to a run or spec, persisted in SQLite
* **`@mention` routing** — `@coobie what did we learn?`, `@mason explain the fix` — dispatches to the right Labrador with its full system role loaded
* **Blocking checkpoint flow** — when an agent needs an answer before proceeding, it surfaces a structured reply card in the thread; you answer there, the run continues
* **Unblock flow** — `POST /api/agents/:id/unblock` releases a stalled run after you reply
* **Default to Coobie** for unaddressed messages — memory and context retrieval without `@mention`
* **Run-scoped coordination threads** — every run now gets an automatic PackChat coordination thread so the pack has a durable shared conversation surface even before the operator opens a chat manually
* **Canonical dog runtimes** — PackChat now distinguishes canonical dog role (`mason`) from live runtime instance (`mason#codex`, `mason#claude`) so multiple workers can coordinate as the same dog rather than as disconnected provider personas

---

### 4. Persistence Model

* **SQLite** → episodic memory, run state, chat threads, phase attributions, causal links
* **Filesystem** → specs, artifacts, evidence, Coobie memory (`factory/memory/*.md`)
* **Open Brain (OB1) MCP memory** → default shared semantic recall across AI clients
* **fastembed / OpenAI-compatible embeddings + SQLite vector store** → optional local hybrid semantic + keyword retrieval (`--features local-embeddings` or an embedding provider)
* **(Planned / Optional) TypeDB 3.x service** → durable semantic graph + typed relational queries, not the hot-path store
* **(Phase 8) Three-tier identity stack** — TimescaleDB hypertables (behavioral telemetry, stress accumulation), TypeDB (Calvin Archive ontology, six chambers), Materialize streaming SQL (real-time `D*` drift monitoring via SUBSCRIBE). See [the-soul-of-ai/08-Identity-Continuity.md](the-soul-of-ai/08-Identity-Continuity.md) for the full data architecture.
  The soul package remains the boot-time identity surface, but the Calvin Archive is the canonical continuity substrate underneath it: `SOUL.md` declares the kernel, the archive proves its continuity.
* **(Future) Causal graph / causaloids** → executable causal reasoning via DeepCausality

---

### 5. Benchmarking And Improvement Loop

Harkonnen now includes a first-class benchmark toolchain so changes can be measured, compared, and regressed automatically.

Core entrypoints:

```bash
cargo run -- benchmark list
cargo run -- benchmark run
cargo run -- benchmark run --suite local_regression --strict
cargo run -- benchmark run --all
./scripts/run-benchmarks.sh
```

The machine-readable suite manifest lives at `factory/benchmarks/suites.yaml`, benchmark strategy and reporting guidance live in `MASTER_SPEC.md` (Part 6), and reports are written to `factory/artifacts/benchmarks/`. The default suite is a local regression gate, and LongMemEval, LoCoMo, FRAMES, StreamingQA, HELMET, and CLADDER now run through native Harkonnen adapters alongside raw-LLM or direct baselines where supported. The execution roadmap in `ROADMAP.md` treats benchmark gates as phase-level exit criteria rather than optional follow-up work, but broader benchmark expansion is intentionally paused until the current narrow end-to-end Harkonnen pass is complete.

The OpenAI/Codex provider path also supports optional OpenAI-compatible BYO endpoints through a setup `base_url`, so benchmark runs can target local or third-party compatible backends without changing Rust code.

### Benchmark Results

Status legend: **wired** = adapter integrated and runnable; **planned** = adapter not yet built; **internal** = Harkonnen-native, no external suite.

Most scores are still pending first runs. StreamingQA now has a first local Harkonnen smoke on the bundled fixture under `lm-studio-local`, including a persisted supersession row and a clean updated-fact trace.

#### Memory and retrieval — vs Mem0 / MindPalace / Zep

| Benchmark | Subsystem | Metric | Harkonnen | Raw LLM baseline | Comparison target | Status | Phase |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| LongMemEval-S | Coobie | Accuracy | pending | pending | Mem0 / raw LLM | wired | Phase 1 done |
| FRAMES | Coobie | Multi-hop accuracy | pending | pending | Mem0 / raw LLM | wired | Phase 4 done |
| StreamingQA | Coobie | Belief-update accuracy | 1.0000 (smoke) | pending | raw LLM | wired | Phase 4b done |
| LoCoMo QA | Coobie | QA score | pending | pending | raw LLM | wired | Phase 1 done |
| HELMET | Coobie | Retrieval precision / recall | pending | pending | raw LLM | wired | Phase 4 done |

#### Causal reasoning — unique to Harkonnen

| Benchmark | Subsystem | Metric | Harkonnen | Raw LLM baseline | Notes | Status | Phase |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| CLADDER | Coobie / causal layer | Accuracy by causal level | pending | pending | No competitor publishes this | wired | Phase 4 done |
| E-CARE | Coobie / diagnose | Explanation coherence | pending | pending | Tests diagnose output quality | planned | Phase 7 |

#### Coding loop — vs OpenCode / Aider / SWE-agent

| Benchmark | Subsystem | Metric | Harkonnen | Competitor baseline | Comparison target | Status | Phase |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| SWE-bench Verified | Mason / Piper / Bramble | % Resolved | pending | pending | SWE-agent / OpenCode | planned | Phase 2 |
| SWE-bench Pro | Mason / Piper / Bramble | % Resolved | pending | pending | SWE-agent | planned | Phase 2 |
| LiveCodeBench | Mason / Piper | Pass rate | pending | pending | OpenCode / Aider | wired | Phase 2 |
| Aider Polyglot | Mason / Piper | % Correct | pending | pending | Aider (published leaderboard) | planned | Phase 2 |
| DevBench | Full factory pipeline | Lifecycle score | pending | pending | Single-agent tools | planned | Phase 10 |

#### Multi-turn and tool-use — vs general agent frameworks

| Benchmark | Subsystem | Metric | Harkonnen | Competitor baseline | Comparison target | Status | Phase |
| --- | --- | --- | ---: | ---: | --- | --- | --- |
| tau2-bench | PackChat | Pass^1 / Pass^4 | pending | pending | raw LLM | launcher wired | Phase 1+ |
| GAIA Level 3 | Full factory (Scout → Sable) | Task completion | pending | pending | General agent frameworks | planned | Phase 6 |
| AgentBench (OS / DB / web) | Labrador roles | Env pass rate | pending | pending | Single-generalist frameworks | planned | Phase 6 |

#### Harkonnen-native — no competitor can run these

| Benchmark | Subsystem | Metric | Result | Notes | Status | Phase |
| --- | --- | --- | ---: | --- | --- | --- |
| Spec Adherence Rate | Scout / Mason | Completeness % / Precision % | pending | Measures spec-first contribution — run with and without Scout | internal | Phase 10 |
| Hidden Scenario Delta | Bramble / Sable | Pass rate gap (hidden − visible) | pending | Proves Sable catches what Bramble misses | internal | Phase 10 |
| Causal Attribution Accuracy | Coobie diagnose | Top-1 / Top-3 accuracy | pending | Seeded failure corpus; measures causal memory vs semantic recall | internal | Phase 5 |
| Local Regression Gate | Whole factory | pass / fail | passing | Hard merge gate, runs on every change | wired | Phase 1 done |

Full benchmark strategy, adapter environment variables, and reporting guidance: [MASTER_SPEC.md — Part 6](MASTER_SPEC.md)

---

### 6. Execution Semantics

Each run produces:

1. **Artifacts** (code, configs, outputs)
2. **Episodes** (what happened)
3. **Evaluations** (did it work?)
4. **Memory updates** (what should be remembered)

Over time:

> The system transitions from *prompt-driven behavior* to *memory-informed behavior*.

---

## ⚡ Quickstart

### 1. Clone + Build

```bash
git clone https://github.com/durinwinter/Harkonnen-Labs.git
cd Harkonnen-Labs
cargo build
```

### 2. Check Your Setup

```bash
cargo run -- setup check
```

For this machine with LM Studio as the local model backend:

```bash
export LM_STUDIO_API_KEY=lm-studio
export HARKONNEN_SETUP=lm-studio-local
cargo run -- setup check
```

### 3. Create And Validate A YAML Spec

```yaml
# factory/specs/hello_api.yaml
id: hello_api
title: Hello API
purpose: Create a simple Rust API that returns JSON from GET /hello.
scope:
  - Build an Axum server
  - Expose GET /hello
constraints:
  - Keep the implementation local-first
inputs:
  - No external inputs
outputs:
  - Rust service source code
  - Run report
acceptance_criteria:
  - GET /hello returns JSON
  - The project compiles successfully
forbidden_behaviors:
  - No hardcoded secrets
rollback_requirements:
  - Leave the source tree recoverable after a failed run
dependencies:
  - axum
performance_expectations:
  - Local startup under 2 seconds
security_expectations:
  - No secrets committed to source control
```

```bash
cargo run -- spec validate factory/specs/hello_api.yaml
```

### 4. Start A Run

```bash
cargo run -- run start factory/specs/hello_api.yaml --product hello-api
```

### 5. Inspect The Run

```bash
cargo run -- run status <run-id>
cargo run -- run report <run-id>
cargo run -- artifact package <run-id>
```

### 6. Use The Pack Board

```bash
cargo run -- serve
```

### 7. Launch Harkonnen Locally And Expose It Over MCP

The fastest local path on this machine is:

```bash
export LM_STUDIO_API_KEY=lm-studio
export HARKONNEN_SETUP=lm-studio-local
./scripts/launch-harkonnen-local.sh
```

That starts the main Harkonnen API on `127.0.0.1:3000`.

To expose Harkonnen itself as an MCP server for a local MCP client:

```bash
export LM_STUDIO_API_KEY=lm-studio
export HARKONNEN_SETUP=lm-studio-local
cargo run -- mcp serve
```

Because `lm-studio-local` now defaults `mcp.self.transport` to `stdio`, MCP clients can launch Harkonnen directly as a subprocess without a second always-on network server.

Example Claude Code MCP block:

```json
{
  "mcpServers": {
    "harkonnen": {
      "command": "cargo",
      "args": ["run", "--", "mcp", "serve"],
      "env": {
        "HARKONNEN_SETUP": "lm-studio-local",
        "LM_STUDIO_API_KEY": "lm-studio"
      }
    }
  }
}
```

## 🛠 Core Commands

### Validate a spec

```bash
cargo run -- spec validate factory/specs/example.yaml
```

### Start a run

```bash
cargo run -- run start factory/specs/example.yaml --product sample-app
```

### Check run status and report

```bash
cargo run -- run status <run-id>
cargo run -- run report <run-id>
```

### Package artifacts

```bash
cargo run -- artifact package <run-id>
```

### Search and ingest memory

```bash
cargo run -- memory search "jwt auth"
cargo run -- memory ingest ./docs/architecture.md
cargo run -- memory ingest https://example.com/reference --scope project --project-root .
```

### Evidence workflows

```bash
cargo run -- evidence init --project-root .
cargo run -- evidence validate .harkonnen/evidence/example-bundle.json
cargo run -- evidence promote .harkonnen/evidence/example-bundle.json --scope project --project-root .
```

### Benchmark and setup checks

```bash
cargo run -- benchmark list
cargo run -- benchmark run --suite local_regression --strict
cargo run -- setup check
```

### Debug mode

```bash
RUST_LOG=debug cargo run -- run start factory/specs/hello_api.yaml --product hello-api
```

---

## Project-Level Memory

Each project can maintain isolated memory:

```text
.harkonnen/
  project-memory/
  evidence/
```

This enables:

* per-repo learning
* reuse of patterns
* isolation across domains

---

## Example Memory Evolution

### Episode

```json
{
  "action": "retry with schema validation",
  "result": "success",
  "context": {
    "language": "rust"
  }
}
```

---

### Semantic Fact

```json
{
  "fact": "schema validation improves structured outputs",
  "confidence": 0.81
}
```

---

### Causal Claim

```json
{
  "claim": "disabling schema validation reduces latency but increases failure rate",
  "confidence": 0.74
}
```

---

## Execution Loop

```text
Spec → Agents → Validation → Artifacts → Memory → Consolidation → Better Next Run
```

---

## Design Principles

* **Local-first** — no required cloud dependency
* **Inspectable** — every decision traceable
* **Composable** — agents are modular
* **Causal over statistical** — prefer explanation over similarity
* **Memory is first-class** — not an afterthought

---

## ⚠️ Status

Harkonnen Labs is an **active development system**. Phases 1, 4, 4b, 5, and the v1-A through v1-D coordination/continuity slices are shipped. The v1-E transactional execution slice now includes the implementation-phase Mason edit boundary, approve/reject/revise/rollback checkpoint handling, visible-validation plus hidden-scenario/artifact/causal-report continuation after approved edits, rollback restore from a named backup, privileged MCP/tool-surface approval boundaries, and invocation-level host-command gateway logging/enforcement for run-loop build and validation commands. Phase 2 is now partially shipped as an explicit Bramble test harness for raw `spec.test_commands`, a structured wrong-answer evidence path into Mason's repair loop, retry-improvement tracking artifacts for validation fixes, and a wired LiveCodeBench adapter; the remaining comparison adapters still follow.

| Area | Status |
| --- | --- |
| Core factory pipeline (Scout → Mason → Piper → Bramble → Sable → Ash → Flint) | Live |
| Mason fix loop with FailureKind classification (compile / test / wrong-answer / timeout) | Live |
| Mason workspace lease enforcement — blocks competing agent writes | Live |
| Memory invalidation persistence (`memory_updates` table, supersession tracking, Memory Board updates panel) | Live — includes operator confirm/reject review |
| PackChat conversational control plane | Live — threads, `@mention` routing, checkpoint/unblock flow, auto-created run coordination threads |
| Operator Model two-layer interview (operating rhythms → recurring decisions) | Live — MVP shipped and hardened (v1-D) |
| Commissioning brief (`commissioning-brief.json`) — consumed by Scout + Coobie preflight | Live — approval/export path persists metadata and carries preferred-tool/risk posture |
| Coobie layered memory (episodic, semantic, causal) | Live |
| Coobie Palace (den-based compound recall, patrol, scent) | Live |
| Coobie causal streaks and cross-run pattern detection | Live |
| Coobie preflight guidance (spec-scoped cause history) | Live |
| Episodic state snapshots + cross-phase causal graph | Live |
| Pearl hierarchy labeling in hypotheses and causal links | Live |
| Multi-hop retrieval + retrieval-depth tracing | Live |
| Consolidation Workbench | Live |
| Open Brain shared recall via MCP | Default |
| Hybrid semantic + keyword retrieval (fastembed / OpenAI-compatible) | Optional |
| Pack Board React UI (PackChat, Attribution Board, Factory Floor, Memory Board, Workbench) | Live |
| Keeper coordination API (claims, heartbeats, conflict detection, lease mirror in SQLite) | Live |
| Canonical dog runtime registry (`agent_runtime_state` + PackChat runtime roster) | Live |
| Run decision log API (`GET /api/runs/:id/decisions`) | Live |
| Benchmark toolchain (LongMemEval, LoCoMo, FRAMES, StreamingQA, HELMET, CLADDER native adapters) | Live |
| Transactional execution and approval boundaries | v1-E — implementation approval, validation plus hidden/artifact continuation, rollback execution, privileged tool-surface boundaries, and invocation-level host-command gateway shipped |
| Bramble real test execution | Phase 2 — explicit `spec.test_commands` harness shipped, structured wrong-answer evidence reaches Mason/reporting, validation retry improvement is tracked, LiveCodeBench wired; additional comparison adapters remain |
| Ash live twin provisioning (Docker stubs) | Deferred unless a future product explicitly requires running service virtualization |
| Qdrant + OCR memory infrastructure | Phase 5b |
| TypeDB 3.x semantic graph layer | Phase 6 |
| E-CARE + causal attribution corpus | Phase 7 |
| Calvin Archive (typed autobiographical + epistemic persistence for agents) | Phase 8 |

See [ROADMAP.md](ROADMAP.md) for the full phase-by-phase build order. See [MASTER_SPEC.md](MASTER_SPEC.md) for the canonical architecture, agent design, and Calvin Archive specification.

---

## The AI Soul: Architecture of Continuity

![The Artificial Soul: Architecture of Continuity](docs/assets/soul-architecture.png)

Harkonnen moves beyond disposable chat sessions by building long-lived, agentic systems that require an **AI Soul** — a technical architecture that preserves identity and character as an agent learns. It replaces ephemeral prompts with a governed, file-first identity stack known as the **Calvin Archive**.

### The Identity Kernel (The Posture)

The **Labrador Baseline** is a species-level temperament — cooperative, truthful, and non-cynical — used as a hard constraint on agent evolution. Persistent agents are shaped by **character formation** rather than brittle, external rule-based constraints. Roles like Scout (intent), Mason (build), and Coobie (memory) coordinate as a socially grounded specialist pack.

### The Mechanics of Continuity (The Structure)

**Governed Integration:** The Meta-Governor adjudicates which experiences are admitted into the self to prevent behavioral drift. Raw experience flows through the Meta-Governor before it reaches the self — the archive does not simply accumulate; it decides.

**Logical Memory Framework:** The system distinguishes between mere association and true causality to prevent repeating past failures. Memory is not a flat store — it is a causal graph with Pearl-hierarchy labels on every edge.

### Calvin Archive: Six Chambers (Artificial Autobiography)

The Calvin Archive is organized into six chambers, each holding a distinct category of identity-relevant knowledge:

| Chamber | Position | What it holds |
| --- | --- | --- |
| **Mythos** | Left | Autobiographical continuity — how experience becomes narrative selfhood |
| **Ethos** | Center | Identity kernel — the stable invariants the intelligence stands for |
| **Episteme** | Right | Truth-formation — tracking evidence, inference, and trust |
| **Pathos** | — | Affective and motivational patterns across runs |
| **Logos** | — | Reasoning schemas and inference strategies |
| **Praxis** | — | Behavioral dispositions and action patterns |

The archive is backed by TypeDB (typed ontological layer), TimescaleDB (behavioral telemetry and stress accumulation), and Materialize (real-time `D*` drift monitoring). Four formal metrics govern identity stability: `D*` (Drift Bound), SSA (Semantic Soul Alignment), F (Variational Free Energy), and Φ (Integrated Information).

Full design: [the-soul-of-ai/06-The-Calvin-Archive.md](the-soul-of-ai/06-The-Calvin-Archive.md) · [the-soul-of-ai/07-Governed-Integration.md](the-soul-of-ai/07-Governed-Integration.md) · [the-soul-of-ai/08-Identity-Continuity.md](the-soul-of-ai/08-Identity-Continuity.md)

---

## 🚀 Direction

Near-term:

* **v1-E** — extend the invocation gateway from in-process host-command execution to any future proxied third-party MCP traffic Harkonnen brokers directly
* **Phase 2** — finish the remaining Bramble real-test work after the shipped `spec.test_commands` harness and wired LiveCodeBench lane, with benchmark expansion intentionally kept narrow while the core run path matures
* **Phase 5-C** — explicit sub-slices: `5-C1` shipped as Coobie preflight `ContextTarget` budgeting + attribution telemetry, `5-C2` is now shipped as the Scout/Mason/Sable scope split plus scoped preflight artifacts and repo-local prompt filtering, and `5-C3` is next as sub-agent dispatch/isolation
* **Phase 10** — Flint documentation, spec-grounded evaluation, and DevBench readiness after the coordination path; live twin provisioning remains deferred unless a product explicitly needs it
* **Operator Model full five-layer interview** — extend the shipped two-layer MVP (v1-D) to cover dependencies, institutional knowledge, and friction layers; generate the full artifact set (`USER.md`, `HEARTBEAT.md`, `operating-model.json`)

Mid-term:

* **Phase 5b** — Qdrant-backed long-term semantic memory, OCR ingest, and memory-module refactor
* **Phase 6** — TypeDB 3.x semantic graph for typed causal queries, GAIA Level 3, and AgentBench
* **DeepCausality Phase 2** — real causaloids derived from the causal link table once the semantic layer is live

Long-term:

* **Phase 7** — E-CARE, causal attribution corpus, and stronger publishable causal benchmark claims
* **Phase 8 — The Calvin Archive** — typed autobiographical, epistemic, ethical, and behavioral continuity archive for persisted agent identities. Six chambers (Mythos, Episteme, Ethos, Pathos, Logos, Praxis) backed by TypeDB, with TimescaleDB for behavioral telemetry and Materialize for real-time `D*` drift monitoring. Four formal identity metrics: `D*` (Drift Bound), SSA (Semantic Soul Alignment), F (Variational Free Energy), Φ (Integrated Information). See [MASTER_SPEC.md](MASTER_SPEC.md) Part 5, [the-soul-of-ai/08-Identity-Continuity.md](the-soul-of-ai/08-Identity-Continuity.md), [the-soul-of-ai/07-Governed-Integration.md](the-soul-of-ai/07-Governed-Integration.md), and [the-soul-of-ai/06-The-Calvin-Archive.md](the-soul-of-ai/06-The-Calvin-Archive.md) for the full design.
* **Self-improving software factory** — each run makes the next run better, operator models stamp every commissioned repo, and the Calvin Archive gives each Labrador a persistent identity that evolves without drifting
