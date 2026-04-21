---
name: harkonnen
description: "Operate Harkonnen through the repo-local MCP server and CLI. TRIGGER: user asks about run history, PackChat threads or messages, checkpoints, board snapshots, benchmark suites or reports, wants to diagnose a failure, wants a run report, or wants to start, queue, or watch a run. Preferred over raw shell commands for Harkonnen interactions."
user-invocable: true
argument-hint: "[recent-runs | report <run-id> | diagnose <run-id> | queue <spec> <product-or-path> | watch <run-id> | boards <run-id> | start <spec> <product-or-path> | thread-open <title> | thread-list | say <thread-id> <message> | checkpoints <run-id> | answer-checkpoint <run-id> <checkpoint-id> | unblock <run-id> <agent> | benchmark-suites | benchmark-recent | benchmark-report <id-or-latest> | benchmark-smoke <suite-id...>]"
allowed-tools:
  - Read
  - Bash(claude mcp list)
  - Bash(cargo run -- setup check)
  - mcp__harkonnen__*
  - mcp__sqlite__*
  - mcp__filesystem__*
---

# /harkonnen - Run, Board, And Benchmark Operations

Arguments passed: `$ARGUMENTS`

Use the repo-local `harkonnen` MCP server as the first-choice surface for run,
PackChat, checkpoint, and benchmark ops. If MCP is unavailable, diagnose and
fix the MCP path instead of silently dropping to normal CLI operations.

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

### `queue <spec> <product-or-path>`
Use `mcp__harkonnen__queue_run` for normal commissioning so the command returns
immediately with a queued run id. If the second argument looks like a path
(contains `/` or starts with `.`), use `product_path`; otherwise use `product`.
Quote paths with spaces. Default `run_hidden_scenarios` to `true` unless the
user asks for a lighter run. Echo: spec, product/path, hidden scenarios
enabled, returned run-id, and tell the user `watch <run-id>` is the next step.
If the user provides draft spec YAML, pass it through `queue_run` instead of
asking them to save a file first.

### `watch <run-id>`
Call `mcp__harkonnen__watch_run`. Summarize the current status, latest phase
activity, active checkpoints, and run timing when present. This is the default
follow-up after `queue`.

### `boards <run-id>`
Call `mcp__harkonnen__get_run_board_snapshot`. Present the run using the same
four-board mental model as the web UI: Mission, Action, Evidence, and Memory.
Prefer this when the user wants a holistic “what’s going on right now?” view
instead of a narrow event stream. Use the summary view by default for quick
answers, and only ask for the full board payload when the user explicitly wants
deep detail.

### `start <spec> <product-or-path>`
If the second argument looks like a path (contains `/` or starts with `.`),
use `product_path`. Otherwise use `product`. Quote paths with spaces.
This is the blocking variant and should be used only when the user explicitly
wants to wait for the lifecycle result in one call. Otherwise prefer `queue`.
If used, default `run_hidden_scenarios` to `true` unless user asks for a
lighter run. Echo: spec, product/path, hidden scenarios enabled, returned
run-id. Tell the user: `report <run-id>` or `diagnose <run-id>` is the next
step. If the user provides draft spec YAML, pass it through the MCP `start_run`
operation instead of asking them to save a file first.

### `thread-list`
Call `mcp__harkonnen__list_chat_threads`. Return thread id, title, kind, run
binding, and updated time. Use small limits unless the user asks for a full list.

### `thread-open <title>`
Call `mcp__harkonnen__open_chat_thread`. Default to `thread_kind: "general"`
unless the user clearly wants a run, spec, or operator-model thread.

### `say <thread-id> <message>`
Call `mcp__harkonnen__post_chat_message`. Summarize both the operator message
and the generated agent reply. If the user addresses a specific pup, pass the
agent explicitly when helpful; otherwise let normal PackChat routing handle it.

### `checkpoints <run-id>`
Call `mcp__harkonnen__list_run_checkpoints`. Focus on checkpoints still open or
recently answered, and surface agent, phase, prompt, and next action.

### `answer-checkpoint <run-id> <checkpoint-id>`
Call `mcp__harkonnen__reply_to_checkpoint`. Use `answer_text` for ordinary
operator replies and `decision_json` only when the user has clearly provided
structured decisions.

### `unblock <run-id> <agent>`
Call `mcp__harkonnen__unblock_agent` after confirming the target agent and run.
Use this when the user wants the run to continue after answering a blocker or
when they explicitly ask to release a stalled pup.

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
- Prefer `queue` plus `watch` over the blocking `start` path for everyday
  commissioning through Claude or Codex
- Prefer `boards` when the user wants the UI-style Mission / Action / Evidence /
  Memory overview instead of raw event or status output
- Prefer PackChat thread open/list/message and checkpoint reply/unblock through
  MCP instead of telling the user to use the web UI or raw API routes
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
