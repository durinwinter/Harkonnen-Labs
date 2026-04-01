# Harkonnen Labs

Level 5 AI Software Factory MVP.

## What this is

Harkonnen Labs is a local-first software factory that turns:

- specs
- hidden scenarios
- prior knowledge

into:

- generated software
- validation reports
- artifact bundles

Humans write specs and judge outcomes.
Agents do the implementation work.

## Docs

- [Agent Context](./AGENTS.md)
- [Architecture](./ARCHITECTURE.md)
- [Setup Guide](./SETUP.md)
- [Work Windows Quickstart](./WORK_WINDOWS_QUICKSTART.md)
- [Coobie Memory](./COOBIE.md)
- [Coobie Spec](./COOBIE_SPEC.md)

## MVP goals

- CLI-first
- Rust core
- SQLite for run metadata
- filesystem-backed specs/scenarios/artifacts
- named agent profiles
- hidden scenario isolation
- sample target product

## Quick start

```bash
cargo run -- setup check
cargo run -- memory init
cargo run -- memory ingest ./docs/ISA-18.2.pdf
cargo run -- memory ingest https://example.com/gmp-guidance --scope project --project-root ../some-other-repo
cargo run -- evidence init --project-root ../some-other-repo
cargo run -- evidence validate ../some-other-repo/.harkonnen/evidence/annotations/sample-causal-window.yaml
cargo run -- evidence promote ../some-other-repo/.harkonnen/evidence/annotations/sample-causal-window.yaml --scope project --project-root ../some-other-repo
cargo run -- spec validate factory/specs/examples/sample_feature.yaml
cargo run -- run start factory/specs/examples/sample_feature.yaml --product sample-app
cargo run -- run start factory/specs/examples/sample_feature.yaml --product-path ../some-other-repo
cargo run -- run status <run-id>
cargo run -- run report <run-id>
cargo run -- serve --port 3000
```

On a Claude-only Windows work machine, the fastest path is:

```powershell
.\scripts\factory-up-windows.ps1
.\scripts\launch-pack-board-windows.ps1 -OpenBrowser
```

To stamp a Claude-only Labrador pack into another repo, use:

```bash
cargo run -- setup claude-pack --target-path ../SPO --project-name SPO --project-type winccoa --winccoa
```

## Current highlights

- Coobie now has a split memory model: Harkonnen core memory in `factory/memory/` and repo-local project memory in `<repo>/.harkonnen/project-memory/`.
- `memory ingest` can extract and store knowledge from local files or URLs into either core or project memory, while optionally keeping the original source asset alongside the Markdown note.
- External repos now get continuity artifacts such as `project-scan`, `resume-packet`, `strategy-register`, `memory-status`, and `stale-memory-history` under `.harkonnen/`.
- Project repos can now keep structured time-series/video/log annotation bundles under `.harkonnen/evidence/` so Coobie can learn pattern examples and causal windows from labeled evidence.
- Reviewed annotation bundles can now be promoted into repo-local or core Coobie memory, making causal windows retrievable in normal preflight memory search.
- Coobie cites exploration logs, dead ends, stale-memory mitigation outcomes, and retriever-forge evidence during preflight so prior runs directly shape new runs.
