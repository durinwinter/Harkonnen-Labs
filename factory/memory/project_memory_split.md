---
tags: [coobie, memory, project-memory, core-memory, workflow]
summary: Separate project-local learnings from Harkonnen core memory; promote only strong cross-project patterns into the core store.
---
Coobie should maintain a layered durable memory model.

- Project memory lives with the active repo in `.harkonnen/project-memory/` and should hold repo-specific truths, runtime facts, oracle semantics, local failure modes, and validated mitigation outcomes.
- Core memory in `factory/memory/` should retain only strong cross-project lessons, universal factory patterns, and durable guardrails.
- Worker-harness session memory or resumable state should stay separate from both layers; it is execution convenience, not durable causal truth.
- Runs should retrieve both durable layers, but new repo-specific run summaries and lessons should be written back to project memory by default.
- Promote a lesson into core memory only after repeated confirmation across scenarios or products, not merely because a single external repo used it successfully.
