---
name: harkonnen
description: "Operate Harkonnen through the repo-local MCP server and CLI. TRIGGER: user asks about run history, benchmark suites or reports, wants to diagnose a failure, wants a run report, or wants to start a benchmark-smoke run. Preferred over raw shell commands for Harkonnen and benchmark operations."
user-invocable: true
argument-hint: "[recent-runs | report <run-id> | diagnose <run-id> | start <spec> <product-or-path> | benchmark-suites | benchmark-recent | benchmark-report <id-or-latest> | benchmark-smoke <suite-id...>]"
allowed-tools:
  - Read
  - Bash(claude mcp list)
  - Bash(cargo run -- setup check)
  - mcp__harkonnen__*
  - mcp__sqlite__*
  - mcp__filesystem__*
---

# /harkonnen - Run And Benchmark Operations

Arguments passed: `$ARGUMENTS`

Use the repo-local `harkonnen` MCP server as the first-choice surface for run
and benchmark ops. If MCP is unavailable, diagnose and fix the MCP path instead
of silently dropping to normal CLI operations.

---

## Common Tasks

### `recent-runs`
Call `mcp__harkonnen__list_runs` with limit 5–10. Return a compact table:
run-id, spec, status, outcome, timestamp.

### `report <run-id>`
Call `mcp__harkonnen__get_run_report`. Extract: outcome, failure point, timings,
recommended next step. Don't dump the full report unless the user asks.

### `diagnose <run-id>`
Combine `mcp__harkonnen__get_run`, `mcp__harkonnen__get_run_report`, and
`mcp__harkonnen__list_run_decisions`. Identify whether the blocker is:
spec quality / implementation / validation / setup or MCP / hidden scenarios.
End with the highest-leverage next move.

### `start <spec> <product-or-path>`
If the second argument looks like a path (contains `/` or starts with `.`),
use `product_path`. Otherwise use `product`. Quote paths with spaces.
Default `run_hidden_scenarios` to `true` unless user asks for a lighter run.
Echo: spec, product/path, hidden scenarios enabled, returned run-id.
Tell the user: `report <run-id>` or `diagnose <run-id>` is the next step.

### `benchmark-suites`
Call `mcp__harkonnen__list_benchmark_suites`. Return the suite ids, titles,
default-selected status, and any obvious required env that affects a smoke run.

### `benchmark-recent`
Call `mcp__harkonnen__list_benchmark_reports`. Summarize the newest benchmark
artifacts with report id, generated time, selected suites, and pass/fail/skipped
counts.

### `benchmark-report <id-or-latest>`
Call `mcp__harkonnen__get_benchmark_report`. Default to `latest` when the user
does not provide an id. Prefer `format: "markdown"` for operator-facing output
and `format: "summary"` for compact machine-readable summaries.

### `benchmark-smoke <suite-id...>`
Call `mcp__harkonnen__run_benchmarks`. If the user does not name a suite,
default to the repo's fast smoke path, `local_regression`. Before running
obviously external or long-haul suites such as LongMemEval, LoCoMo, DevBench,
SWE-bench, or tau2-bench, pause and confirm because they may need extra setup
or much more time.

---

## Benchmark Smoke

- Prefer structured specs, especially DevBench-generated specs
- For Harkonnen self-tests: `product_path` is usually `.`
- Start the run first; inspect report + decision log before dropping to shell-level
  benchmark commands
- Prefer benchmark suite listing, report listing, report retrieval, and smoke
  execution through MCP before dropping to shell-level benchmark commands
- For comparative benchmarking beyond one run: use MCP run data first, then recommend
  suite-level shell commands

---

## MCP Recovery

If `harkonnen` tools are unavailable:
1. Read `.mcp.json` and `.claude/settings.local.json`
2. Confirm `harkonnen` is present and enabled
3. Run `claude mcp list` or `cargo run -- setup check`
4. Tell the user the concrete fix — don't only report that MCP failed

---

## Boundaries

- Do not use shell commands for run inspection when the MCP tool already exists
- Do not invent run IDs, statuses, reports, or decision details
- If a start request is missing both product name and product path, ask one
  concise follow-up question
