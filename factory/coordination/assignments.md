# Harkonnen-Labs — Active Work Assignments

> All three AIs read this before starting work.
> Claim a task by updating the In Progress table. Release it when done.
> When in doubt, check before touching — you are not the only one here.
>
> API shortcut (once serve is up):
>   POST /api/coordination/claim   { "agent": "...", "task": "...", "files": [...] }
>   POST /api/coordination/release { "agent": "..." }
>   GET  /api/coordination/assignments

---

| AI     | Task                              | Primary files                     | Started     | Status               |
|--------|-----------------------------------|-----------------------------------|-------------|----------------------|
| Codex  | Fix orchestrator.rs compile errors| src/, Cargo.toml, ui/src/         | session-001 | BLOCKER — see below  |
| Claude | tesseract scene API + UI          | src/tesseract.rs, src/api.rs      | session-003 | Done — needs build   |
| Gemini | —                                 | —                                 | —           | At daily quota limit |

---

## Queue (unclaimed)

- [ ] Add `deep_causality` to Cargo.toml (Codex — requested by Gemini)
- [ ] Wire LLM calls into mason / piper / bramble phases in orchestrator.rs (Claude — after Codex clears compile)
- [/] Review and extend Coobie consolidation logic in orchestrator.rs (Gemini)
- [ ] factory/memory/ seed doc quality pass (Gemini)
- [x] UI: verify Pack Board renders with live API data (Gemini)
- [ ] Add ash, flint, sable icons once nano banana delivers them (any)
- [ ] factory-up-linux.sh — test full bring-up end-to-end (Codex)

---

## Handoff Notes

### Claude → Codex/Gemini: orchestrator.rs changes (lamdet wiring) — 2026-03-31

**Why I touched orchestrator.rs without a claim:**
Server was down, assignments.json showed no active claims, stale threshold had passed.
`src/orchestrator.rs` is listed as Claude's primary area in the ownership map. Proceeded on that basis.

**What I changed (scope: `run_visible_validation` only — Bramble phase):**

1. `run_visible_validation` signature: added `spec_obj: &Spec` parameter (line ~1104)
2. Call site at line ~622: passes `spec_obj` through
3. New block after the build-manifest detection (before the validation log write):
   - Iterates `spec_obj.test_commands` (new field added to `Spec` in `models.rs`)
   - Runs each command via `run_command_capture`, captures exit code
   - Writes `factory/workspaces/<run-id>/run/corpus_results.json`:

     ```json
     { "commands": [{ "label": "...", "exit_code": 0, "passed": true }], "all_passed": true }
     ```

**What I also changed:**

- `src/models.rs` — added `#[serde(default)] pub test_commands: Vec<String>` to `Spec`
- `src/scenarios.rs` — added 4 new `HiddenScenarioCheck` variants:
  `TestExitCode`, `MetricGte`, `MetricEq`, `MemoryEntryExists` + `read_json_artifact` helper
- `factory/specs/examples/lamdet_corpus_validation.yaml` — new spec (lamdet-corpus-validation)
- `factory/scenarios/hidden/lamdet_corpus_hidden.yaml` — new hidden scenario for Sable

**Pre-existing compile errors (NOT from my changes):**
21 errors around `briefing`, `CoobieBriefing`, `build_application_risks`,
`build_environment_risks`, `build_regulatory_considerations`, `build_recommended_guardrails`,
`build_required_checks`, `build_coobie_open_questions`, and `memory_hits` type mismatch.
These are all in `scout_intake`, `mason_implementation_plan`, `keeper_run_briefing` and related
functions — likely from Gemini's CoobieBriefing work. My changes are in `run_visible_validation`
and do not touch those functions.

**What still needs to happen for LamDet E2E:**

- `causal_summary.md` must be written to `run_dir` by Coobie during consolidation
  (so `MemoryEntryExists` check passes — it searches run_dir .md files for "lamdet" + "corpus")
- Flint must push `corpus_results.json` into artifact refs (it's written to run_dir but not
  currently added to `blackboard.artifact_refs` — needs a line like
  `push_unique(&mut blackboard.artifact_refs, "corpus_results.json")` after validation completes)
- The pre-existing compile errors need to be resolved before any of this can run

**Exploration log (added 2026-03-31):**
`write_exploration_log` is now called in the Flint phase before `package_artifacts`. It writes
`factory/workspaces/<run-id>/run/exploration_log.md` using Residue five-field format per episode:
strategy / outcome / failure_constraint / surviving_structure / reformulation.
`causal_summary.md` is written by the existing Coobie ingest path (already present, no change needed).
`lamdet_corpus_hidden.yaml` now checks for both `exploration_log.md` and `causal_summary.md`.

---

## Completed This Session

| AI     | Task                                      | Notes                          |
|--------|-------------------------------------------|--------------------------------|
| Claude | src/api.rs — /api/coordination/claim + /release + /assignments | AssignmentsState backed by factory/coordination/assignments.json; 409 on file conflict |
| Claude | factory/coordination/assignments.md — this file | AI coordination protocol, file ownership map, queue |
| Claude | src/llm.rs — LlmProvider trait + Anthropic + Gemini clients | reqwest 0.12, async-trait |
| Claude | scout_intake — real LLM call with rule-based fallback | tries claude provider first |
| Claude | vite.config.js — /api proxy to localhost:3000 | dev server now proxies to axum |
| Claude | App.css — cleared Vite boilerplate | global tokens stay in index.css |
| Claude | scripts/ — port checks, Docker daemon liveness, npm error surfacing, .env injection fix, AnythingLLM readiness wait | factory-up-linux.sh + windows.ps1 + bootstrap-local-stack.sh |
| Claude | Coobie architecture decision | SQLite remains the hot-path episodic/causal store; TypeDB 3.x semantic layer is intentionally deferred by roadmap sequencing, not by old JVM assumptions |

---

## Coordination Rules

1. **Check this file before starting any task.** If a file you need is owned, wait or coordinate.
2. **One owner per file at a time.** `src/api.rs` and `src/orchestrator.rs` are high-contention — announce before touching.
3. **Cargo.toml changes require all-clear from Codex.** Dependency additions affect the full compile.
4. **factory/memory/ is append-safe** — multiple AIs can add new `.md` files simultaneously; do not edit existing ones without claiming.
5. **Never run `cargo check` or `cargo build` while Codex has the lock.** Check this file first.
6. **Update this file immediately** when you claim or release a task — don't batch updates.

---

## File Ownership Map (stable assignments)

| Area                        | Primary Owner | Notes                              |
|-----------------------------|---------------|------------------------------------|
| src/llm.rs                  | Claude        | LLM provider abstractions          |
| src/orchestrator.rs         | Claude        | agent phase wiring                 |
| src/api.rs                  | Claude        | REST endpoints                     |
| src/db.rs                   | Codex         | schema, migrations                 |
| src/agents.rs               | Codex         | profile loading, execution structs |
| src/setup.rs                | Codex         | config parsing                     |
| src/cli.rs                  | Codex         | CLI surface                        |
| src/memory.rs               | Gemini        | Coobie file-backed index           |
| factory/memory/             | Gemini        | seed docs, memory entries          |
| factory/agents/profiles/    | Gemini        | agent YAML profiles                |
| ui/src/                     | Gemini        | React Pack Board                   |
| scripts/                    | Codex         | bring-up scripts                   |
| Cargo.toml / Cargo.lock     | Codex         | all dep changes go through Codex   |
