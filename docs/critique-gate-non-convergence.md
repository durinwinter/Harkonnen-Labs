# The Implementation Gate Does Not Converge

**Status:** open architectural problem. An interim control shipped (`HARKONNEN_CRITIQUE_ADVISORY`); the real fix is roadmap item v1-A.
**Written:** 2026-08-05, after 20 live runs of one spec against a real browser-JS product repo.
**Audience:** anyone trying to make the factory author code end-to-end.

---

## 1. The problem in one paragraph

Coobie's plan critique gates the implementation boundary. If the critique returns any
`blocking_concerns`, the run pauses **before Mason writes a line**, and there is no
resume-into-implementation path — the run must be started from scratch. The critique is an LLM
judging a plan against a large, auto-generated guardrail set. It does not converge: given the
same spec and a plan that addresses the previous round's objections, it returns a *different*
set of objections. "Satisfy the critic" is therefore an unbounded loop, not a defect you can
fix once. The factory can be blocked indefinitely by its own governance while every component
is working as designed.

## 2. Why the gate is absolute

Two mechanics combine, and each is reasonable alone.

**The blocker set is a straight pass-through.** Any non-empty list flips the boundary to
`operator_review_required`:

```rust
let approval_state = if approval_blockers.is_empty() {
    "auto_approved"
} else {
    "operator_review_required"
};
```

`approval_blockers` is `critique.blocking_concerns` verbatim. One sentence from the model is
enough to stop the run. There is no severity, no threshold, no quorum.

**Approval does not resume implementation.** The only continuation after a checkpoint reply is
`resume_hidden_artifacts_after_tool_approval`, and it is gated on `run_dir/validation.json`
already existing. That file is written *after* validation, which is downstream of
implementation. So approving a run paused at `paused_before_mutation` resumes nothing — it
only resumes the tail of a run that already reached validation. There is no resume endpoint
either; `/api/runs/start` is the only run-creating POST.

Consequence: **a blocked run is unrecoverable.** Every attempt costs a fresh run, and on the
Gemini free tier a run costs 6–8 of a 20-request daily allowance per model.

## 3. Evidence that it does not converge

Same spec. Each plan written with knowledge of the previous round's objections. Each run a
fresh set of blockers.

| Run | Blockers | What the critic demanded |
|---|---|---|
| `7db36804` | 2 | Plan mutated an existing module the spec forbade touching (**a genuinely good catch**); missing evidence-change note |
| `108d08b7` | 3 | Two Severity-100 stale lessons marked "Unresolved"; no post-injection runtime validation |
| `c3f7fe58` | 3 | Failed to challenge a named optimization program; no evidence artifacts per validation claim; a *different* stale lesson left unresolved |

Between `108d08b7` and `c3f7fe58` the spec gained hard evidence resolving both Severity-100
questions (verified against source: the target objects are never frozen, they are read live at
call time rather than snapshotted at load, and load order is guaranteed by script-tag
placement) **and** a
real runtime validation command. The critic accepted none of it as closing the point and raised
three new objections instead.

This is the signature of an unbounded critic, not a bad plan. The guardrail set is derived from
prior run history — the briefing for this spec carried 342 lesson-ID mentions and 25 guardrails
— so there is always another guardrail the plan did not visibly address.

## 4. Four real bugs found underneath it (all fixed)

The non-convergence hid genuine defects. These are worth separating out because each one was
independently misdiagnosable, and three of them presented as "the model is bad at its job."

**4.1 The critic saw only the first 3000 characters of the plan.**
`plan.chars().take(3000)` at the critique call site. A Mason plan runs 4–7 KB, so the critic
judged completeness from the first half of the document and correctly reported the missing
tail. On run `3b74a3e9` the window ended mid-sentence at `"3. **Choice**: "`, and the plan's
Twin Narrative (char 3378) and Risks (char 6431) sections were never shown.

> **This was misread as hallucination during the session.** It was not. Every blocker the model
> returned was an accurate description of truncated text. Worth remembering: when a model
> reports something missing, check what it was actually handed before blaming it.

**4.2 The Gemini client silently returned starved responses.**
Reasoning tokens bill against `maxOutputTokens`. Measured on `gemini-3.5-flash` at a 300-token
budget: `thoughtsTokenCount` 284, `candidatesTokenCount` 12, `finishReason: MAX_TOKENS`. The
client checked no finish reason and returned `Ok("")`. Downstream that surfaced as an empty
`implementation_plan.md` (read as "Mason produced nothing") and as unparseable Mason edits (read
as a formatting bug). Now sends `thinkingConfig` and fails loudly.

**4.3 The plan prompt and the critique enforced different contracts.**
The prompt asked for five sections. The critique demanded a planning-choices log, stale-memory
revalidation, an evidence-change note and a twin narrative — none of which the planner was ever
asked to write. A plan could satisfy every word of its instructions and still be blocked. Both
prompt sites now share one `MASON_PLAN_TASK_CONTRACT`.

**4.4 The spec could not write the files its own criteria required.**
Mason may only write paths declared `code_under_test`. The spec declared one source directory
while its acceptance criteria also required edits to two files outside it. Run `31d334b1` died
`edit_outside_scope` proposing exactly the edit the spec demanded.

Note that 4.1 and 4.3 are the *same shape of bug*: a producer and a consumer disagreeing about
a contract, with the failure surfacing as "the model did something wrong."

## 5. A structural aggravator: the fallback plan can never pass

When the plan-generation LLM call fails (a transient 503 on run `c908bb70`), a deterministic
template plan is written instead — 75 KB, 63 sections, all boilerplate (`Detected Files`,
`Coobie Domain Signals`). It contains none of the sections the critique requires, so **a
transient network error deterministically produces a blocked run.** The fallback exists to keep
the pipeline moving but guarantees a gate failure.

## 6. Solutions

Ordered by how fundamental the fix is, not by effort.

### Option A — Operator override (shipped, interim)

`HARKONNEN_CRITIQUE_ADVISORY=1` demotes blocking concerns to advisory. The concerns are still
written to `coobie_critique.json`, still logged, still flag the blackboard; they simply stop
halting the run.

- **Pros:** unblocks immediately; loses no information; safe because Mason's edits land in a
  staged workspace and commit to a `mason/*` branch, never the working tree.
- **Cons:** a blunt on/off switch. Turned on permanently, the critique stops being a control at
  all — including for the one genuinely good catch it made (`7db36804`).
- **Verdict:** correct as an escape hatch, wrong as the permanent answer.

### Option B — Severity and a threshold

Have the critique return a severity per concern and block only above a bar (e.g. only concerns
tied to a `forbidden_behaviors` entry or a Severity-100 lesson).

- **Pros:** keeps the gate meaningful; kills the long tail of process objections
  ("did not log planning choices") that have nothing to do with code safety.
- **Cons:** the model assigns the severity, so it can inflate it. Needs the severity to be
  derived from the guardrail's own recorded severity rather than the model's opinion.
- **Verdict:** the best value-for-effort improvement.

### Option C — Bounded revision loop instead of a hard stop

On blocking concerns, feed them back to the planner and regenerate, up to N times, then proceed
with the concerns recorded. This is exactly the pattern the Mason edit lane already uses for
malformed responses (retry with the parse error fed back).

- **Pros:** consistent with existing design; converts a terminal state into a self-correcting
  one; most objections are things the planner *can* fix.
- **Cons:** costs N extra calls per run — material on a 20-request/day tier.
- **Verdict:** the right shape. Pair with B so the loop only runs for concerns worth fixing.

### Option D — Resume into implementation (roadmap v1-A)

Make the checkpoint reply actually continue the run: persist the blackboard and staged
workspace, and add a resume path that re-enters implementation rather than only the validation
tail.

- **Pros:** fixes the real asymmetry — a human *can* review and approve, but their approval
  currently buys nothing. Makes every other option cheaper by removing the restart cost.
- **Cons:** the largest change; needs durable run state and a new endpoint.
- **Verdict:** the actual fix. Everything else is mitigation.

### Option E — Shrink the guardrail set fed to the critique

342 lesson-ID mentions and 25 guardrails guarantee something is unaddressed. Rank by relevance
to the current spec and pass the top N.

- **Pros:** directly attacks non-convergence; also cuts token cost, which matters given prompts
  of 60k–116k tokens.
- **Cons:** relevance ranking is its own problem; risks dropping the guardrail that mattered.
- **Verdict:** worth doing alongside B.

### Also fix regardless

- **The fallback plan should not be judged by the critique.** Either mark template-generated
  plans as such and skip the gate, or retry the plan call before falling back. A transient 503
  should not deterministically block a run.
- **Retry transient 5xx.** Several runs died to 503s that a single retry would have absorbed.

## 7. Recommendation

1. **Now:** keep `HARKONNEN_CRITIQUE_ADVISORY` as the escape hatch, off by default.
2. **Next:** Option B (severity threshold, severity taken from the guardrail record) plus the
   fallback-plan fix. Small, and together they remove most spurious blocks.
3. **Then:** Option C, bounded revision, reusing the Mason retry pattern.
4. **Properly:** Option D — v1-A. Until a paused run can resume, every gate in the system is a
   restart, and that is what makes the non-convergence expensive rather than merely annoying.

## 8. Reproducing

```bash
cd <harkonnen-worktree>

# Blocked by the gate (default behavior):
./target/debug/harkonnen-labs run start \
  factory/specs/drafts/<spec>.yaml \
  --product-path <path-to-product-repo>

# Proceeds, concerns recorded as advisory:
HARKONNEN_CRITIQUE_ADVISORY=1 ./target/debug/harkonnen-labs run start \
  factory/specs/drafts/<spec>.yaml \
  --product-path <path-to-product-repo>

# Inspect what the critic said:
python3 -c "import json;d=json.load(open('factory/workspaces/<run-id>/run/coobie_critique.json'));[print('-',x) for x in d['blocking_concerns']]"
```

Note the binary is `target/debug/harkonnen-labs`, not `harkonnen`.

## 9. Environment notes learned the hard way

- **Gemini free tier is 20 requests/day *per model*** (`GenerateRequestsPerDayPerProjectPerModel`).
  An exhausted model is unblocked by switching to a sibling, not by waiting. There is also a
  per-minute limit that a burst of probes will trip independently.
- **`gemini-3.1-pro` reports `limit: 0`** — no free quota at all. Routing an agent there yields
  429 on every call and a silent fall back to templated output.
- **`gemini-3.6-flash` rejects `thinkingBudget: 0`** with a bare `400 INVALID_ARGUMENT`, while
  accepting 128/512/1024. `gemini-3.5-flash` accepts 0. Hence the default is a small positive
  budget, not zero.
- **Local inference cannot run this factory on 8 GB of VRAM.** `gemma-4-e4b` loads at a 32768
  context ceiling, but Scout's prompt measured **116,497 tokens** and Coobie's 59,400. That size
  comes from the project scan and retriever bundle, *not* from `code_under_test`, so narrowing
  the spec's file surface does not shrink it. Making local viable requires real context
  reduction in the agent prompts — which is Option E by another route.
