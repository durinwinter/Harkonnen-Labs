# Work Windows Quickstart

This is the shortest path to a usable Claude-only Harkonnen setup on a work laptop.

## What you talk to

There are three useful surfaces, but only one is the primary conversation interface.

### 1. Claude Code in the target repo
This is where you actually talk to the system.

Open the repo you want worked on in Claude Code after stamping it with the Labrador pack:

```powershell
cargo run -- setup claude-pack --target-path ..\YourRepo --project-name YourRepo --project-type generic
```

Then open `..\YourRepo` in Claude Code. That is the main operator surface.

Use it for:
- asking Scout/Mason/Coobie to work
- writing specs
- discussing outcomes
- reviewing Coobie memory and evidence behavior

### 2. Pack Board UI
This is the control-room view.

Use it for:
- run status
- blackboard state
- coordination
- Coobie briefings and reports
- evidence matching and annotation workflows

Default URL when launched locally:
- `http://127.0.0.1:4173`

### 3. Local API
This is the machine-facing surface used by the UI and automation.

Default URL:
- `http://127.0.0.1:3057/api`

Use it for:
- evidence bundle CRUD
- evidence match reports
- annotation review history
- run state polling
- coordination endpoints

## Prerequisites

Install these first:
- Git
- Rust / Cargo
- Node.js 18+
- Claude Code
- Anthropic API access

## First-time bring-up

From the Harkonnen repo root in PowerShell:

```powershell
$env:HARKONNEN_SETUP = "work-windows"
$env:ANTHROPIC_API_KEY = "sk-ant-..."
$env:MEMORY_FILE_PATH = ".\factory\memory\store.json"
.\scripts\factory-up-windows.ps1
```

That gets you:
- the Claude-only setup
- MCP packages
- `memory init`
- `setup check`
- `.claude/settings.local.json`

## Launch the operator surfaces

Start the factory API and Pack Board UI:

```powershell
.\scripts\launch-pack-board-windows.ps1 -OpenBrowser
```

Default ports:
- API: `3057`
- Pack Board: `4173`

## Attach Harkonnen to a work repo

Stamp the target repo with a Claude Labrador pack:

```powershell
cargo run -- setup claude-pack --target-path ..\YourRepo --project-name YourRepo --project-type generic
```

For a WinCC OA / OT-style repo, use the richer export:

```powershell
cargo run -- setup claude-pack --target-path ..\SPO --project-name SPO --project-type winccoa --domain "Siemens WinCC OA / industrial automation" --summary "SPO is operated through a Claude-only Labrador pack." --winccoa
```

## Seed Coobie with project knowledge

Project-local memory:

```powershell
cargo run -- memory ingest C:\docs\ISA-18.2.pdf --scope project --project-root ..\YourRepo
```

Project-local evidence / annotation store:

```powershell
cargo run -- evidence init --project-root ..\YourRepo
```

## Most useful day-one commands

```powershell
cargo run -- setup check
cargo run -- memory index
cargo run -- spec validate factory\specs\examples\sample_feature.yaml
cargo run -- run start factory\specs\examples\sample_feature.yaml --product-path ..\YourRepo
cargo run -- run status <run-id>
cargo run -- run report <run-id>
```

## If you want the simplest mental model

- Talk to Claude Code inside the target repo.
- Watch the Pack Board for factory state.
- Let the UI call the API for evidence workflows.
- Use the CLI for setup, ingestion, and scripted runs.
