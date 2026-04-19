# Harkonnen Labs — Agent Context

This file is the universal context document for all AI agents (Claude, Gemini, Codex)
working in this repository. Read it before touching any code or spec.

For structured machine-readable data, see `factory/context/`.
For memory retrieval, see `factory/memory/` and ask Coobie.

---

## What This System Is

A local-first, spec-driven AI software factory. The human commissions a run through
conversation, a pack of nine specialist agents executes with discipline, and Coobie
remembers what worked. Correctness is judged by behavioral outcomes, not code review.

**The factory is a coordinated pack.** You talk to the pack — they talk to their
providers — and you stay in the loop even while they work autonomously. Blocking
questions, spec reviews, and unblock decisions all flow through the same conversation
surface rather than stalling silently.

---

## Codebase Map

```text
src/                    Rust CLI (cargo run -- <command>)
  main.rs               Entry point and command dispatch
  cli.rs                All subcommands and handlers
  api.rs                Axum API routes and handlers
  agents.rs             Agent profile loading and prompt bundle resolution
  benchmark.rs          Benchmark manifests, execution, and report generation
  capacity.rs           Token budget and working-memory capacity management
  chat.rs               PackChat thread/message persistence and agent dispatch
  claude_pack.rs        Claude-specific pack setup and agent routing helpers
  config.rs             Path discovery + SetupConfig loading
  coobie.rs             Coobie causal reasoning, episode scoring, preflight guidance
  coobie_palace.rs      Palace: den definitions, patrol, compound scent, PatchPatrol
  db.rs                 SQLite init and schema migrations
  embeddings.rs         fastembed + OpenAI-compatible vector store, hybrid retrieval
  llm.rs                LLM request/response types and provider routing
  longmemeval.rs        LongMemEval native adapter (Harkonnen vs raw-LLM paired mode)
  memory.rs             File-backed memory store, init, reindex, retrieve
  models.rs             Shared data types (Spec, RunRecord, EpisodeRecord, etc.)
  orchestrator.rs       AppContext, run lifecycle, all Labrador phase methods
  pidgin.rs             Inter-agent message format and handoff protocol
  policy.rs             Path boundary enforcement
  reporting.rs          Run report generation
  scenarios.rs          Hidden scenario loading and evaluation (Sable)
  setup.rs              SetupConfig structs, provider resolution, routing
  spec.rs               YAML spec loader and validation
  tesseract.rs          Workspace isolation and file-write permission model
  workspace.rs          Per-run workspace creation

factory/
  agents/profiles/      Nine agent YAML profiles (one per agent)
  agents/personality/   labrador.md — shared personality for all agents
  memory/               Coobie's memory store (md files + index.json)
  mcp/                  MCP server documentation YAMLs
  context/              Machine-parseable YAML context for agent consumption
  specs/                Factory input specs (YAML)
  scenarios/            Hidden behavioral scenarios (Sable + Keeper only)
  workspaces/           Per-run isolated workspaces
  artifacts/            Packaged run outputs
  benchmarks/           Benchmark suite manifest for local and external evals
  logs/                 Run logs
  state.db              SQLite run metadata

setups/                 Named environment TOML files
BENCHMARKS.md           Benchmark strategy, benchmark map, reporting guidance
harkonnen.toml          Active/default setup config
.env.example            Environment variable template
```

---

## CLI Commands

```sh
cargo run -- spec validate <file>              # Scout: parse and validate a spec
cargo run -- run start <spec> --product <name> # Start a factory run
cargo run -- run status <run-id>               # Check run status
cargo run -- run report <run-id>               # Print run report
cargo run -- artifact package <run-id>         # Package artifacts for a run
cargo run -- memory init                       # Seed Coobie's memory, print backend setup
cargo run -- memory index                      # Rebuild index.json from md files
cargo run -- memory ingest <file-or-url>       # Extract docs/web content into core or project memory
cargo run -- evidence init --project-root <repo> # Bootstrap repo-local evidence/annotation storage
cargo run -- evidence validate <file>          # Validate a causal evidence annotation bundle
cargo run -- evidence promote <file>           # Promote reviewed evidence into project/core Coobie memory
cargo run -- benchmark list                    # List configured benchmark suites
cargo run -- benchmark run                     # Run default benchmark suites and write reports
cargo run -- benchmark report <file>           # Render a saved benchmark report as Markdown
cargo run -- setup check                       # Verify active setup (providers + MCP)
```

## Benchmarking

Benchmark strategy lives in `BENCHMARKS.md`, runnable suites live in `factory/benchmarks/suites.yaml`, and benchmark reports are written to `factory/artifacts/benchmarks/`.

The default benchmark gate is `local_regression` and runs `cargo fmt --check`, `cargo check`, and `cargo test -q`. LongMemEval and LoCoMo now run through native benchmark adapters, tau2-bench now has a PackChat launcher wrapper for external harnesses, and SWE-bench Verified/Pro remain adapter-ready so Harkonnen can publish comparable memory, PackChat, and coding-loop scores as the remaining integrations mature.

## Coordination

While the API server is running, the live coordination source is:

```sh
GET /api/coordination/assignments
POST /api/coordination/claim
POST /api/coordination/heartbeat
POST /api/coordination/release
```

Claim example:

```json
{ "agent": "claude", "task": "wire mason phase", "files": ["src/orchestrator.rs"] }
```

Release example:

```json
{ "agent": "claude" }
```

Keeper is the policy owner of file-claim coordination. While the API server is running, claim conflicts, stale claims, heartbeats, and releases should be treated as Keeper-managed policy events.

Agents holding files should send a heartbeat about once per minute with:

```json
{ "agent": "claude" }
```

Keeper marks claims stale after 600 seconds without a heartbeat and may reap stale conflicting claims when another agent needs the same files.

If the API server is not running yet, use repo-root `assignments.md` as the coordination document and paste only the relevant claim section into each AI's context.

---

## Agent Roster

Nine specialist agents, each with a bounded role, permitted tools, and a provider assignment.
Agent profiles live in `factory/agents/profiles/<name>.yaml`.

| Agent   | Role                | Profile Provider | Key Responsibility                                                                                                        |
|---------|---------------------|------------------|---------------------------------------------------------------------------------------------------------------------------|
| Scout   | Spec retriever      | claude (pinned)  | Parse specs, flag ambiguity, produce intent package                                                                       |
| Mason   | Build retriever     | default          | Generate and modify code, multi-file changes                                                                              |
| Piper   | Tool retriever      | default          | Run build tools, fetch docs, execute helpers                                                                              |
| Bramble | Test retriever      | default          | Generate tests, run lint/build/visible tests                                                                              |
| Sable   | Scenario retriever  | claude (pinned)  | Execute hidden scenarios, produce eval reports                                                                            |
| Ash     | Twin retriever      | default          | Provision digital twins, mock dependencies                                                                                |
| Flint   | Artifact retriever  | default          | Collect outputs, package artifact bundles                                                                                 |
| Coobie  | Memory retriever    | default          | Coordinate pack memory: working context, episodic capture, causal graph, consolidation, and cross-agent blackboard health |
| Keeper  | Boundary retriever  | claude (pinned)  | Enforce policy, guard boundaries, and manage file-claim coordination                                                      |

**Pinned to Claude**: Scout, Sable, Keeper — these are trust-critical roles.
**Routable**: Mason, Piper, Bramble, Ash, Flint, Coobie — provider set per setup.

### Key Invariants

- Mason **cannot** access `scenario_store` — prevents test gaming
- Sable **cannot** write implementation code
- Only Keeper has `policy_engine` access
- Keeper owns file-claim coordination and conflict policy through the coordination API
- All agents share the labrador personality: loyal, honest, persistent, never bluffs

---

## Setup System

The active setup is read from (in order):

1. `HARKONNEN_SETUP=work-windows` → `setups/work-windows.toml`
2. `HARKONNEN_SETUP=./path/to/file.toml` → that file directly
3. `harkonnen.toml` (repo root)
4. Built-in default (Claude only)

### Provider Routing

Setup files control which AI model each agent uses:

```toml
[providers]
default = "gemini"           # agents with provider: default use this

[routing.agents]
coobie = "claude"            # Coobie always uses Claude on this machine
mason  = "codex"             # Mason uses Codex for code generation
# all others inherit providers.default = gemini
```

Agent profiles declare their preferred provider (`claude`, `default`, etc.).
Setup `[routing.agents]` overrides that declaration for a specific machine.
This means **agent profiles stay stable; routing is per-environment**.

### Provider Fields

```toml
[providers.claude]
type         = "anthropic"
model        = "claude-sonnet-4-6"
api_key_env  = "ANTHROPIC_API_KEY"
enabled      = true
usage_rights = "standard"    # standard | high | targeted
surface      = "claude-code" # which surface/tool runs this provider

# OpenAI-compatible BYO endpoint example:
[providers.codex]
type         = "openai"
model        = "gpt-4o"
api_key_env  = "OPENAI_API_KEY"
enabled      = true
surface      = "vscode"
# base_url     = "http://localhost:11434"
```

### Setup Variants

| Setup            | Providers           | Default | Docker | AnythingLLM |
|------------------|---------------------|---------|--------|-------------|
| home-linux       | Claude+Gemini+Codex | gemini  | Yes    | Yes         |
| work-windows     | Claude only         | claude  | No     | No          |
| ci               | Claude Haiku only   | claude  | No     | No          |

---

## MCP Integration

MCP servers back the abstract tool names in agent profiles. Configured per setup in
`[[mcp.servers]]` blocks. The `tool_aliases` field connects abstract names to real servers.

### Active Servers (home-linux)

| Server       | Package                                    | Aliases                                           |
|--------------|--------------------------------------------|---------------------------------------------------|
| filesystem   | @modelcontextprotocol/server-filesystem    | filesystem_read, workspace_write, artifact_writer |
| memory       | @modelcontextprotocol/server-memory        | memory_store, metadata_query                      |
| sqlite       | @modelcontextprotocol/server-sqlite        | db_read, metadata_query                           |
| github       | @modelcontextprotocol/server-github        | fetch_docs, github_read                           |
| brave-search | @modelcontextprotocol/server-brave-search  | fetch_docs, web_search                            |

### Adding a New MCP Server

1. Add `[[mcp.servers]]` to the active setup TOML
2. Create `factory/mcp/<name>.yaml` (documentation + alias list)
3. Reference the tool alias in the relevant agent profile's `allowed_tools`
4. Run `cargo run -- setup check` to verify the command is on PATH

### Claude Code MCP Config

To wire MCP servers into Claude Code directly, add to `.claude/settings.local.json`:

```json
{
  "mcpServers": {
    "memory": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-memory"]
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem",
               "./products", "./factory/workspaces", "./factory/artifacts", "./factory/memory"]
    }
  }
}
```

---

## Pack Board — Primary Interaction Surface

The Pack Board is the primary UI for commissioning and monitoring factory runs.
It is not a read-only dashboard — it is the place where the human stays in the
loop while the pack works autonomously.

### Interaction model

- **PackChat** is the main input. Describe what you want to build in natural
  language. Scout drafts the spec inline. You refine it, then commission the pack
  with one button. The same thread surfaces blocking questions from any agent
  during a run — you answer them there, and the run continues.
- **@addressing** routes a message to a specific pup: `@keeper is this path safe?`
  or `@coobie what did we learn last time?`
- **Blocked agents** surface a reply card in the chat rather than stalling silently.
  Your answer unblocks the run without leaving the chat.

### Blackboard panels (four named slices)

The sidebar mirrors Coobie's team blackboard structure:

| Panel          | Blackboard slice | What it shows                                              |
|----------------|------------------|------------------------------------------------------------|
| Mission Board  | Mission          | Active goal, current phase, open blockers, resolved items  |
| Factory Floor  | Action           | Live agent roster — who is running, blocked, or done       |
| Evidence Board | Evidence         | Artifact refs, validation results, scenario outcomes       |
| Memory Board   | Memory           | Recalled lessons, causal precedents, memory health         |

Coobie sits at the top of the board because she watches all four slices and her
guidance shapes every phase. The Attribution Board shows per-phase attribution
records — which prompt bundle, which skills, which memory hits, and whether the
phase succeeded — so the human can see exactly what the pack used and what worked.

### Interactive autonomy

The pack runs autonomously once commissioned, but is not a black box:

- Any agent can post a blocking question to the chat at any phase boundary
- The human can address any pup directly at any time without interrupting the run
- Phase attribution is recorded continuously so the run is inspectable live
- The Workbench (post-run) lets the human review what the pack learned and decide
  what gets promoted into Coobie's durable memory

---

## Coobie — Memory Agent

Coobie manages the factory's accumulated knowledge. Two backends, one source of truth.

### Source of Truth: `factory/memory/`

All memory lives as `*.md` files with YAML frontmatter:

```markdown
---
tags: [spec, auth, jwt]
summary: JWT auth pattern used in sample-app
---
Content here...
```

`harkonnen memory index` scans these and builds `factory/memory/index.json`.

### Backend 1: File Index (all setups)

Keyword search over `index.json`. Built into the Rust layer. Always available.

### Backend 2: MCP Memory Server (all setups)

`@modelcontextprotocol/server-memory` — fast key-value entity store.
Coobie pushes key facts here for rapid retrieval during runs.
Set `MEMORY_FILE_PATH=./factory/memory/store.json` for persistence across restarts.

### Backend 3: AnythingLLM (home-linux only)

Docker-based RAG over the `factory/memory/` directory.
Provides semantic search on large document collections.
Seed with: `./scripts/coobie-seed-anythingllm.sh`

### Initializing Coobie

```sh
cargo run -- memory init          # write seed docs + build index
./scripts/coobie-seed-mcp.sh      # print MCP config + seeding instructions
./scripts/coobie-seed-anythingllm.sh  # home-linux only: upload to AnythingLLM
```

### Adding Memory

To store a new fact in Coobie's memory, either:

- Write a `.md` file to `factory/memory/` and run `harkonnen memory index`
- Use `cargo run -- memory ingest <file-or-url>` to extract text into core memory
- Use `cargo run -- memory ingest <file-or-url> --scope project --project-root <path>` to write into repo-local project memory
- Use `cargo run -- evidence init --project-root <path>` to create `.harkonnen/evidence/` for causal annotation bundles
- Use `cargo run -- evidence promote <file> --scope project --project-root <path>` to promote reviewed bundles into durable Coobie memory
- Ask Coobie directly during a run: "Coobie, store this pattern for future runs"

---

## Spec Format

All factory runs start with a YAML spec in `factory/specs/`. Required fields:

```yaml
id: snake_case_identifier
title: Human-readable name
purpose: One-sentence intent
scope: [list of in-scope items]
constraints: [list of things that must not happen]
inputs: [what the factory receives]
outputs: [what the factory produces]
acceptance_criteria: [visible pass/fail conditions]
forbidden_behaviors: [must-never-occur items]
rollback_requirements: [what survives a run failure]
dependencies: [external packages or services]
performance_expectations: [timing or throughput targets]
security_expectations: [auth, secrets, isolation]
```

---

## Development Conventions

- **Read first**: understand existing code before suggesting changes
- **Specs before code**: if there is no spec, write one first
- **Boundary discipline**: never let factory code reach into `factory/scenarios/` (Sable only)
- **Memory discipline**: after any run that produces a reusable pattern, store it via Coobie
- **Setup portability**: never hardcode paths or provider names in Rust source — use `SetupConfig`
- **Frontend on this workstation**: when you need `node` or `npm`, run them via `flatpak-spawn --host ...` rather than assuming the sandbox has a native Node toolchain
- **TypeDB direction**: when Phase 6 opens, target the Rust-based TypeDB 3.x line in a container-first deployment; do not design around or install the legacy Java distribution
- **MCP first**: prefer registering a new capability as an MCP server over adding Rust code

---

## Active Build Roadmap

**[ROADMAP.md](ROADMAP.md) is the canonical phase-by-phase build order.**
All agents and contributors must check it before starting new work.
Phases 1, 4, 4b, and 5 are already shipped.
The active numbered build phases are Phase 2 (Bramble real test execution) and
Phase 3 (Ash live twin provisioning), with Operator Model Activation running as a
parallel product/control-plane track. Long-term roadmap work remains Phase 5b
(memory infrastructure), Phase 6 (TypeDB semantic layer), and Phase 7
(causal attribution corpus + E-CARE).

---

## What Is and Is Not Implemented

### Implemented

- Spec loading and validation (Scout layer)
- Run creation, status, reporting, and persistence in SQLite
- Per-run workspace isolation and artifact packaging
- File-backed memory store with keyword retrieval, raw asset import, and extracted document/URL ingest into core or project memory (Coobie)
- Repo-local evidence bootstrap and annotation bundle validation under `.harkonnen/evidence/`
- `setup check`, `setup init`, and `setup claude-pack`
- Agent profile loading and provider routing display
- Provider-aware prompt bundle resolution per agent — filters pinned skills to the resolved provider, fingerprints the bundle, writes `agents/<agent>_prompt_bundle.json`
- Phase-level attribution recording — captures prompt bundle, pinned skills, memory hits, lessons, required checks, and outcome per Labrador phase into SQLite and `phase_attributions.json`
- Provider-aware LLM routing for Claude, Gemini, and OpenAI/Codex
- Scout, Mason, Piper, Bramble, and Ash LLM calls with rule-based or procedural fallback
- **Mason fix loop** — up to 3 iterations on build failure; each iteration feeds structured failure context back to Mason for targeted correction before giving up
- Opt-in Mason LLM-authored file writes inside the staged workspace
- Piper real build execution with live stdout/stderr streaming on the `LiveEvent` broadcast channel
- **PackChat conversational control plane** — chat threads scoped to run or spec; `@mention` routing to named agents; blocking checkpoint materialization and reply flow; agent unblock route; multi-turn conversation history
- Checkpoint reply and agent unblock routes wired into the conversational control plane
- Hidden scenario evaluation through protected scenario files (Sable)
- Digital twin manifests with dependency stubs and optional narrative (Ash)
- **Coobie causal reasoning** — heuristic rules, episode scoring per run, causal report output
- **Coobie causal streaks** — causes that fire repeatedly across runs are escalated in briefings
- **Coobie cross-run pattern detection** — identifies causes that cluster on specific spec types or phases
- **Coobie Phase 3 preflight guidance** — spec-scoped cause history drives `required_checks` before each run
- **Coobie Palace** (`src/coobie_palace.rs`) — den-based compound recall layer; patrol walks five dens (Spec, Test, Twin, Pack, Memory) before each run; compound scent elevates den-level streaks over individual cause scores; feeds directly into preflight briefing
- **Causal feedback loop** — Sable scenario rationale written back to project memory as evidence after each run
- **Semantic memory** — fastembed or OpenAI-compatible embeddings + SQLite vector store; hybrid vector + keyword retrieval
- **Episodic state snapshots** — `state_before` / `state_after` workspace snapshots recorded for implementation and build episodes
- **Cross-phase causal graph** — phase-sequence and failure-triggered links populate `causal_links`, surfaced through the causal events API
- **Pearl hierarchy labeling** — Coobie hypotheses and causal graph edges carry associational / interventional / counterfactual levels
- **Coobie multi-hop retrieval** — configurable retrieval depth with hop-by-hop source tracing in query responses
- **Memory invalidation / fact-update tracking** — stale facts are marked superseded or challenged rather than silently overwritten
- **Consolidation Workbench** — operator-reviewed keep/discard/edit flow before durable lesson promotion
- **Native LongMemEval adapter** — paired Harkonnen vs raw-LLM comparison mode; `longmemeval_s_cleaned.json` support
- **Native LoCoMo adapter** — paired Harkonnen vs raw-LLM comparison mode
- **Native FRAMES, StreamingQA, HELMET, and CLADDER adapters** — benchmarked retrieval and causal-reasoning surfaces wired into the Rust runner
- **First-class benchmark toolchain** — `benchmark list/run/report`; manifest-driven suites in `factory/benchmarks/suites.yaml`; CI workflow; LM Studio local routing
- Keeper coordination API with claims, heartbeats, conflict detection, and release flow
- Pack Board web UI with PackChat conversation surface, Attribution Board, Factory Floor, Memory Board, and Consolidation Workbench
- Bootstrap scripts for home-linux and work-windows

### Planned (next build layer)

- **Phase 2** — Bramble real test execution from spec-driven `test_commands`; Mason online-judge feedback loop (`FailureKind::WrongAnswer`); LiveCodeBench and Aider Polyglot adapters
- **Phase 3** — Ash live twin provisioning against running Docker stubs; Flint documentation phase; spec adherence rate and hidden scenario delta internal benchmarks; DevBench adapter
- **Parallel Product Track** — Operator Model Activation: PackChat-based elicitation workflow, approved operator-model storage, artifact export/import, and Scout/Coobie/Keeper integration
- **Phase 5b** — Memory infrastructure: Qdrant semantic layer, OCR ingest, and `src/memory.rs` split into the COOBIE_SPEC module tree
- **Phase 6** — TypeDB 3.x semantic graph layer (COOBIE_SPEC.md Layer C); GAIA Level 3 and AgentBench adapters
- **Phase 7** — causal attribution corpus, E-CARE adapter, and publishable causal benchmark baselines
- **DeepCausality Phase 2** — real causaloids from the causal link table after the TypeDB layer is live
