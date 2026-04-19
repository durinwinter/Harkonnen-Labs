# The Soul of AI

This folder is a book in progress. It collects the theoretical and philosophical writing behind Harkonnen Labs — the questions the code can't answer on its own.

The engineering lives in [MASTER_SPEC.md](../MASTER_SPEC.md). This is what the engineering is *for*.

---

## Chapters

| File | What it covers |
| --- | --- |
| [01-Agentic-Engineering.md](01-Agentic-Engineering.md) | What agentic software engineering is, why it is a control-plane problem rather than a code-generation problem, and the principles under which Harkonnen was built. |
| [02-What-Is-An-AI-Soul.md](02-What-Is-An-AI-Soul.md) | The foundational question: what is a soul, computationally speaking? Definitions, structure, the six chambers, and why persistence of identity matters for AI systems. |
| [03-Artificial-Identity-Architecture.md](03-Artificial-Identity-Architecture.md) | Why persistent agents need more than a static `SOUL.md`: the move from session-bound models to file-first, multi-anchor identity architecture, and how Soul Store fits underneath it. |
| [04-Governed-Integration.md](04-Governed-Integration.md) | Why selfhood requires integration-time adjudication, what quarantine is for, how the Meta-Governor should work, and why multi-timescale revision matters. |
| [05-Identity-Continuity.md](05-Identity-Continuity.md) | The mathematics of identity: drift bounds, semantic soul alignment, variational free energy, integrated information, stress accumulation, hysteresis, and the three-tier data architecture (TimescaleDB, TypeDB, Materialize) that enforces them at production scale. |
| [06-SOUL.md](06-SOUL.md) | The identity of Harkonnen Labs specifically — what it believes, why it exists, and how to make trade-offs when the answer isn't obvious. |

---

More chapters will be added here as the thinking develops. Likely candidates:

- What does it mean for an agent to *learn*?
- The labrador identity kernel — why cooperative and non-adversarial are hard constraints, not soft preferences
- Memory as autobiography — how the Soul Store chambers map to the way humans construct identity over time
- The ethics of persistent intelligences — what obligations arise when an agent accumulates a self
