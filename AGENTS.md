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
  config.rs             Path discovery + SetupConfig loading
  setup.rs              SetupConfig structs, provider resolution, routing
  orchestrator.rs       AppContext, run lifecycle
  memory.rs             File-backed memory store, init, reindex, retrieve
  spec.rs               YAML spec loader
  models.rs             Shared data types (Spec, RunRecord, IntentPackage)
  policy.rs             Path boundary enforcement
  workspace.rs          Per-run workspace creation
  db.rs                 SQLite init
  reporting.rs          Run report generation

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
  logs/                 Run logs
  state.db              SQLite run metadata

setups/                 Named environment TOML files
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
cargo run -- setup check                       # Verify active setup (providers + MCP)
```

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
- **MCP first**: prefer registering a new capability as an MCP server over adding Rust code

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
- Opt-in Mason LLM-authored file writes inside the staged workspace
- Hidden scenario evaluation through protected scenario files (Sable)
- Digital twin manifests with dependency stubs and optional narrative (Ash)
- Coobie causal reasoning phase 1 with causal report output
- Keeper coordination API with claims, heartbeats, conflict detection, and release flow
- Pack Board web UI with PackChat conversation surface, Attribution Board, Factory Floor, and Memory Board
- Bootstrap scripts for home-linux and work-windows

### Planned (next build layer)

- `/api/chat` and `/api/agents/{id}/chat` Rust endpoints to back PackChat live agent routing
- `/api/agents/{id}/unblock` endpoint for in-chat run unblocking
- Coobie reading phase attribution records during causal scoring and lesson promotion
- Post-run consolidation surface in the Workbench — review phase attributions, promote or discard lessons, trigger consolidation pass
- Richer black-box hidden scenarios beyond event/artifact evaluation
- Richer digital twins for external-system simulation
- Richer memory indexing (Qdrant semantic, not just keyword)
- TypeDB semantic graph layer (COOBIE_SPEC.md Layer C)
- DeepCausality phase 2 integration
