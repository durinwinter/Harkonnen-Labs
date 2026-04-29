# Harkonnen Labs

## The AI Software Factory That Gets Smarter Every Time It Runs

Harkonnen Labs is a local-first, multi-agent software factory for turning specifications into validated software. It does not treat each run as a disposable chat session. Every run leaves behind structured evidence: what was requested, what was built, what failed, what passed, what surprised the agents, and what should change next time.

That is the core promise:

> Harkonnen Labs improves through memory, evidence, causal feedback, and governed consolidation, not through vague prompt accumulation.

Most AI coding systems rely on the model's current context window. When the window clears, the system forgets. Harkonnen is built around the opposite assumption: the useful part of an AI system is not only the model response, but the durable operating record around it.

## What Harkonnen Does

Harkonnen coordinates a specialist pack of agents across the full software delivery loop:

- It reads and normalizes specifications.
- It routes work to bounded specialist agents.
- It builds and edits code.
- It runs visible tests and hidden scenarios.
- It records failures, fixes, and review outcomes.
- It turns repeated lessons into reusable memory.
- It routes identity-relevant learning into a governed archive instead of letting it silently mutate the system.

The result is a system that can become better at your codebase, your standards, your failure modes, and your operating preferences over time.

## Why It Gets Smarter Without Training A New Model

Harkonnen does not need to fine-tune an LLM to improve. It improves by changing the information architecture around the LLM.

### 1. It Captures Experience As Data

Every run produces structured records:

- specifications and acceptance criteria
- agent decisions and handoffs
- build output and tool logs
- visible test results
- hidden scenario results
- causal links between actions and outcomes
- operator decisions
- code-review findings
- memory candidates from PackChat conversations

This gives Harkonnen something most chat-based systems do not have: an audit trail it can query before the next decision.

### 2. It Separates Raw Events From Lasting Memory

Not every message deserves to become memory. Harkonnen uses a memory candidate queue so raw conversations do not immediately become permanent truth.

The chain looks like this:

```text
PackChat / Twilight Bark conversation
  -> memory candidate queue
  -> Coobie distillation
  -> Open Brain shared recall
  -> Calvin Archive governed promotion
```

Coobie decides what is useful, what is temporary, what belongs in shared recall, and what is important enough to propose for Calvin Archive promotion.

### 3. It Uses Recall Instead Of Hope

Open Brain gives Harkonnen shared semantic recall across AI clients. When a future run begins, Coobie can retrieve relevant lessons, prior decisions, known risks, and operator preferences.

This is not "the LLM remembered." It is a real retrieval path:

- store a distilled thought
- attach provenance
- retrieve it later by meaning
- include it in a targeted briefing
- show why it was relevant

That makes memory inspectable. The operator can see what the system is using and decide whether it still applies.

### 4. It Learns From Failures Causally

Harkonnen does not only ask, "what text is similar to this issue?" It asks, "what caused this class of failure before?"

Coobie tracks recurring failure patterns such as:

- unclear specs
- broad scope
- missing hidden tests
- environment twin gaps
- agent handoff breakdowns
- stale or missing memory

Before the next run, those patterns are promoted into required checks and guardrails. A prior failure becomes a preflight warning, not a buried log.

### 5. It Consolidates Knowledge Instead Of Hoarding Notes

Many memory systems accumulate facts until retrieval becomes noisy. Harkonnen treats memory as something that must be distilled, deduped, invalidated, and reviewed.

Useful memories move through states:

- candidate
- distilled
- captured in Open Brain
- retrieved in a briefing
- promoted to Calvin when identity-, policy-, or causally significant
- marked `needs_reconsolidation` when newer evidence changes the claim

This is how Harkonnen can get smarter without becoming cluttered.

### 6. It Preserves Identity Separately From Recall

Open Brain answers: what should the system remember semantically?

The Calvin Archive answers: what should become part of the system's governed continuity?

That distinction matters. A useful reminder is not the same thing as a belief, policy, identity trait, or behavioral commitment. Calvin promotion contracts carry compiled claims, evidence timelines, source authority, and review state so important changes are governed instead of accidental.

## The Feature Set In Plain English

### Pack Of Specialist Agents

Harkonnen is not one giant assistant. It is a coordinated pack with distinct responsibilities:

- Scout clarifies and retrieves spec intent.
- Mason writes and repairs code.
- Piper runs tools and streams command output.
- Bramble handles visible tests.
- Sable runs hidden behavioral scenarios.
- Ash handles local twin environments.
- Flint packages artifacts and documentation.
- Coobie manages memory and causal reasoning.
- Keeper enforces boundaries and policy.

Specialization makes the system easier to inspect, easier to test, and harder to confuse.

### PackChat Control Plane

PackChat gives the agents and operator a shared conversation surface. It supports run-scoped threads, agent mentions, checkpoint questions, operator replies, and durable memory candidates.

In distributed mode, Twilight Bark carries PackChat events across runtimes while remaining independent of Harkonnen-specific concepts. Harkonnen depends on Twilight Bark as transport; Twilight Bark does not depend on Harkonnen.

### Coobie Memory System

Coobie is the memory and learning layer. She manages:

- working memory for the current run
- episodic memory for what happened
- semantic recall through Open Brain
- causal memory for why outcomes happened
- memory distillation and dedupe
- stale-memory detection
- preflight warnings based on prior failures

Coobie is how Harkonnen turns experience into better future behavior.

### Open Brain Shared Recall

Open Brain is the default semantic memory substrate. It lets Harkonnen and other AI clients share durable thoughts through an MCP server.

Harkonnen uses Open Brain for things like:

- operator preferences
- lessons learned
- repo-specific patterns
- repeated failure modes
- useful project context
- PackChat-derived distilled memories

It replaces local vector storage as the default recall path. Local vectors can still exist as optional accelerators, but they are not the center of the system.

### Calvin Archive

The Calvin Archive is the governed continuity layer. It is for high-value memories that affect identity, policy, belief, adaptation, or long-term behavior.

Calvin does not accept loose chat logs. It accepts structured promotion contracts with:

- compiled claim
- evidence timeline
- source authority
- confidence
- chamber targets
- preservation notes
- review state
- recommended outcome

This keeps learning powerful without making it ungoverned.

### Hidden Scenarios And Evidence-Based Validation

Passing visible tests is not enough. Harkonnen separates ordinary validation from hidden behavioral scenarios so the system can learn when the implementation technically passes but still misses the real intent.

Those misses become future guardrails.

### Code-Review Learning Records

Review findings become structured records rather than one-off comments. Harkonnen can remember:

- what was found
- where it appeared
- whether it was fixed or skipped
- what lesson should carry forward
- when the lesson becomes stale because the file changed

That makes review compound over time.

### Plan Completion Audit

Before a run closes, Harkonnen can compare the promised roadmap or spec checklist against actual evidence:

- changed files
- tests run
- artifacts produced
- acceptance criteria satisfied
- unresolved gaps

This prevents "looks done" from replacing "proved done."

### OpenZiti-Ready Trust Boundaries

Harkonnen is designed for local-first work and distributed agents. OpenZiti gives the memory chain a zero-trust service boundary:

- PackChat transport
- Open Brain MCP recall
- Calvin Archive write path
- Harkonnen API

Each service can have separate dial and bind policies so memory write authority stays narrow.

## What Makes Harkonnen Different

Most AI coding products optimize the current answer. Harkonnen optimizes the learning loop around the answer.

| Common AI coding workflow | Harkonnen Labs |
| --- | --- |
| One assistant handles everything | Specialist agents with explicit roles |
| Context disappears after the session | Runs produce durable memory and evidence |
| Similarity search retrieves old notes | Coobie combines recall, causality, and provenance |
| Failures are fixed once | Failures become future preflight checks |
| Review comments disappear | Review findings become learning records |
| Memory is an ungoverned pile | Memory is distilled, deduped, invalidated, and promoted |
| "The model learned" is vague | Learning is visible through records, contracts, and tests |

## The Short Version

Harkonnen Labs gets smarter because every run teaches the system in a structured way.

It remembers what happened. It records why it happened. It checks whether the lesson still applies. It retrieves the lesson when it matters. It promotes only the important parts into governed continuity.

That is the difference between a chat session and a software factory with a memory.
