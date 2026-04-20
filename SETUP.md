# Setup Guide

Harkonnen Labs can now generate a machine-specific setup file by fingerprinting the host, asking which model providers are available, and writing a reusable config under `setups/machines/`.

Generated machine profiles are clone-local operational files. They are useful to
keep around on your machine, but they normally should not be committed to the
public repository.

## First run

Start the guided setup interview:

```bash
cargo run -- setup init
```

This flow now starts with identity first:

- machine name, for example `builder-laptop-01`
- setup role, for example `home`, `work`, or `lab`
- optional organization or team, for example `research-lab`

Then it will:

- fingerprint the host platform and toolchain
- suggest a base template such as `home-linux` or `work-windows`
- ask which providers the user can actually access on that machine
- ask what kind of credential each provider uses, which env var it should read from, and whether it goes through a custom gateway or base URL
- choose a default engine
- reroute Labrador agents away from unavailable providers
- ask which MCP capabilities should come along for the ride
- write a machine-specific TOML file

## MCP interview coverage

The setup interviewer now asks about common MCP capabilities including:

- filesystem access
- memory retrieval
- SQLite inspection
- GitHub access
- web search
- winccOA or similar OT/SCADA integrations
- additional custom MCP servers your team needs internally

That makes it much easier to transfer Harkonnen Labs into another environment without baking one machine's assumptions into every generated config.

## Non-interactive examples

Preview a generated home setup without writing a file:

```bash
cargo run -- setup init --non-interactive --machine-name builder-laptop-01 --role home
```

Preview a work setup for the same Linux machine:

```bash
cargo run -- setup init --non-interactive --machine-name builder-laptop-01 --role work
```

Preview a transfer-ready setup for another organization:

```bash
cargo run -- setup init \
  --non-interactive \
  --machine-name builder-laptop-01 \
  --role work \
  --organization research-lab \
  --template home-linux
```

Write a machine config directly:

```bash
cargo run -- setup init \
  --non-interactive \
  --machine-name builder-laptop-01 \
  --role work \
  --organization research-lab \
  --template home-linux \
  --write setups/machines/research-lab-builder-laptop-01-work.toml
```

Generate a Windows-oriented work setup on a Windows host:

```powershell
cargo run -- setup init --machine-name amazon-builder-01 --role work --template work-windows
```

## Fastest work-windows bring-up

If you already know the machine should be Claude-only, the shortest path is:

```powershell
$env:HARKONNEN_SETUP = "work-windows"
$env:ANTHROPIC_API_KEY = "sk-ant-..."
$env:MEMORY_FILE_PATH = ".\factory\memory\store.json"
.\scripts\factory-up-windows.ps1
.\scripts\launch-pack-board-windows.ps1 -OpenBrowser
```

That gives you two operator surfaces immediately:
- Claude Code in the stamped target repo, which is the main place you talk to the system
- Pack Board in the browser, which is the control-room view

For the full operator walkthrough, see [WORK_WINDOWS_QUICKSTART.md](./WORK_WINDOWS_QUICKSTART.md).

## Activating a generated setup

Linux or macOS:

```bash
export HARKONNEN_SETUP=setups/machines/builder-laptop-01-home.toml
cargo run -- setup check
```

PowerShell:

```powershell
$env:HARKONNEN_SETUP = "setups/machines/amazon-builder-01-work.toml"
cargo run -- setup check
```

## What gets fingerprinted

The generated machine file records:

- platform
- CPU architecture
- hostname and username when available
- presence of `git`, `cargo`, `node`, `npm`, `docker`, `podman`, and `openclaw`

That fingerprint is there to make setup files inspectable and easier to support across different environments.

## Exporting a Claude Labrador pack to another project

Once your setup is working, you can stamp a separate repo with a Claude-only Harkonnen pack:

```bash
cargo run -- setup claude-pack \
  --target-path ../PlantOps \
  --project-name PlantOps \
  --project-type winccoa \
  --domain "OT / industrial automation" \
  --summary "PlantOps is an industrial-control project operated through a Claude-only Labrador pack." \
  --winccoa
```

This writes:

- project-level Claude subagents under `.claude/agents/`
- a local `.harkonnen/` context and memory bundle
- a merged `.claude/settings.local.json` MCP block
- a root `CLAUDE.md` block that tells Claude how to use the pack

That gives the work laptop a project-local Claude surface for Scout, Mason, Piper, Bramble, Sable, Ash, Flint, Coobie, and Keeper without depending on Gemini or Codex.

For a generic codebase, drop `--winccoa` and leave `--project-type` as `generic`.


## Optional local memory services

For a fully local Coobie stack, treat these as optional accelerators on top of the filesystem-backed source of truth:

- Qdrant for local long-term semantic memory
- Redis for hot shared memory and team coordination
- AnythingLLM for local document retrieval and local-model RAG

The repo includes local scaffolding for Qdrant and Redis in:

```bash
./scripts/bootstrap-coobie-memory-stack.sh
```

That script writes Docker Compose stacks and launch wrappers under your local Harkonnen directory without changing the canonical memory source inside `factory/memory/`.
