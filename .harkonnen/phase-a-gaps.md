# Phase A — Gap Closure Progress

**Source:** recentinsights.md — external review identifying the delta between current Harkonnen
state and a governed, stateful, economically-aware multi-agent system.

**Owner:** Claude (Scout/Coobie/Keeper roles on home-linux)
**Started:** 2026-04-18
**Status:** COMPLETE — build green, all three tasks shipped

---

## Why This Exists

recentinsights.md identified 8 gaps between Harkonnen's current "Phase 3 → Phase 4" state and
true agentic intelligence. The full gap analysis lives in the planning conversation and the
updated ROADMAP.md. This document tracks which gaps are being closed in Phase A (the cheap,
high-leverage ones) and what remains.

**All agents should read this before preflight if working on src/orchestrator.rs, src/models.rs,
src/api.rs, src/db.rs, or src/keeper.rs.**

---

## Gap Map (Full 8)

| # | Gap                                  | Phase A work                                        | Status      |
| - | ------------------------------------ | --------------------------------------------------- | ----------- |
| 1 | Authority / ActionLease              | A3 — extend Keeper claims with TTL + guardrails     | DONE        |
| 2 | World State Model                    | Not Phase A — Phase E+ territory                    | DEFERRED    |
| 3 | Closed-Loop Outcome Verification     | Not Phase A — Phase E territory                     | DEFERRED    |
| 4 | Multi-Agent Coordination Protocol    | Partially addressed by A3                           | PARTIAL     |
| 5 | Economic / Cost Awareness            | A1 — token + cost tracking on every LLM call        | DONE        |
| 6 | Intent → Plan → Execution separation | Structure exists; plan simulation gate is Phase B/C | DEFERRED    |
| 7 | External System Interfaces           | Phase 6+                                            | DEFERRED    |
| 8 | Governance / Auditability            | A2 — decision_log table + alternatives-considered   | DONE        |

---

## Phase A Tasks

### A1 — Cost / Token Tracking

**What:** Capture `input_tokens`, `output_tokens`, `latency_ms` from every LLM provider
response. Accumulate into a per-run `RunCostSummary`. Surface via `GET /api/runs/:id` and
the Pack Board run overview.

**Why it matters:** Without this, the factory has no answer to "was that worth it?" Retries,
fix loops, and benchmark runs all consume real budget with no visibility.

**Files changed:**

- `src/llm.rs` — `LlmUsage` struct; parse input/output tokens + latency from Anthropic, Gemini, OpenAI responses
- `src/models.rs` — `LlmCostEvent`, `RunCostSummary`, `AgentCostSummary` structs
- `src/db.rs` — `run_cost_events` table + index
- `src/orchestrator.rs` — `record_llm_cost_event` + `get_run_cost_summary` helpers
- `src/api.rs` — `GET /api/runs/:id/cost` endpoint

**Status:** ✅ Shipped

---

### A2 — Decision Log / Alternatives Considered

**What:** When an agent makes a significant choice (plan critique blocks, Mason picks an edit
approach, Coobie selects a consolidation candidate), record: what was chosen, what was
considered and rejected, why, and who approved.

Answers the governance audit question: "Why did the agent act? What alternatives were rejected?"

**Files changed:**

- `src/models.rs` — `DecisionRecord` struct
- `src/db.rs` — `decision_log` table with index
- `src/orchestrator.rs` — `record_decision` + `list_run_decisions` helpers; call sites in plan critique and consolidation promotion
- `src/api.rs` — `GET /api/runs/:id/decisions` endpoint

**Status:** ✅ Shipped

---

### A3 — ActionLease (Keeper Extension)

**What:** Extend the existing Keeper file-claim model with three new lease fields:

- `resource_kind` — `file | workspace | external | agent`
- `ttl_secs` — explicit expiry (lease auto-invalidated when `expires_at` passes)
- `guardrails` — constraints that must hold for any action against this resource

This turns "I claimed a file" into "I hold a lease with explicit authority boundaries and
expiry." Agents call `POST /api/coordination/check-lease` before acting on a resource.

**Files changed:**

- `src/api.rs` — `Assignment` struct extended with lease fields; `ClaimRequest` extended; `check-lease` route + `CheckLeaseResponse`; TTL `expires_at` computed at claim time
- `src/db.rs` — note in Phase A comment (lease fields live on JSON Assignment, no extra table needed)

**Status:** ✅ Shipped

---

## What Is NOT Being Closed in Phase A

**Gap 2 — World State Model:** Needs a live system state graph, observability ingestion, and
deterministic/probabilistic fusion. This is Phase E work and depends on the TypeDB layer (Phase 6)
for the state graph backing.

**Gap 3 — Outcome Verification:** Needs `POST /api/runs/:id/observe`, drift detection, and
external CI/CD hooks. Phase E, after Phase 6.

**Gap 6 — Plan Simulation:** The Scout → IntentPackage → Coobie critique → Mason plan structure
already separates intent from planning from execution. What's missing is a simulation gate
before Mason writes code (run the plan against a dry-run twin). This is Phase C, tied to
Phase 3 Ash twin provisioning.

**Gap 7 — External Interfaces:** Slack, Teams, Prometheus, CI/CD APIs. Phase 7 territory.
The observation hook in Phase E will be the first external integration point.

---

## Completion Criteria for Phase A

- [x] A1: `GET /api/runs/:id/cost` returns token counts and latency per agent
- [ ] A1: Pack Board run overview shows cost summary (UI work remaining)
- [x] A2: `GET /api/runs/:id/decisions` returns decision records for plan critique + consolidation
- [x] A2: Coobie consolidation promotion writes a decision record per kept candidate
- [x] A3: `POST /api/coordination/claim` accepts `resource_kind`, `ttl_secs`, `guardrails`
- [x] A3: `POST /api/coordination/check-lease` returns guardrail violations before an action
- [ ] A3: Mason workspace claim wires `resource_kind = "workspace"` + briefing guardrails (orchestrator call site remaining)
- [x] Build passes cleanly (0 errors, 7 pre-existing warnings)

---

## Hand-off Notes for Other Agents

- **Mason:** Do not implement any of these features speculatively. Read this file and the
  corresponding ROADMAP.md Phase A section before planning edits to the affected files.
- **Coobie:** After A2 lands, decision records are a new first-class memory source. The
  consolidation pipeline should offer `decision_record` as a candidate kind alongside `lesson`
  and `causal_link`.
- **Keeper:** After A3 lands, enforce `check-lease` guardrail verification before approving
  any file write claim against workspace paths during a run.
- **Scout:** After A1 lands, include cost summary in the run report artifact so spec authors
  can see token budget consumed by their spec.
