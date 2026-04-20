# Glossary

This glossary separates three kinds of terms used in the Harkonnen Labs book
and spec:

- **Industry-wide**: common language already used in a recognizable field
- **Research-established**: not general industry slang, but established in an
  academic or specialist research tradition
- **Harkonnen-specific**: terms introduced here, or given a distinctly
  Harkonnen meaning, to explain this system

Where a term is industry-wide, the glossary names the main industry or
discipline that uses it.

---

## Industry-Wide Terms

| Term | Industry / domain | Meaning in this repo |
| --- | --- | --- |
| **Agent** | AI engineering, ML product, developer tools | An autonomous or semi-autonomous software actor that can reason, call tools, and act within a workflow. |
| **Agentic engineering** | AI product and developer-tooling discourse | Building software-delivery systems where agents participate in planning, execution, validation, and iteration, not just code generation. |
| **Artifact** | Software engineering, CI/CD, build systems | A saved output of a run, such as code, reports, bundles, manifests, or packaged results. |
| **Benchmark** | ML evaluation, software engineering, systems performance | A repeatable evaluation used to measure system quality, performance, or capability. |
| **Blackboard** | AI systems, cognitive architecture, multi-agent systems | A shared coordination surface where multiple components or agents read and write state. |
| **Causal graph** | Causal inference, statistics, ML | A graph that represents cause-effect relationships rather than mere correlation. |
| **Checkpoint** | ML training, orchestration, workflow systems | A saved point in a process where state is persisted or human approval can be requested. |
| **Control plane** | Cloud infrastructure, distributed systems, platform engineering | The coordination layer that routes work, applies policy, and manages system behavior. |
| **Counterfactual** | Causal inference, statistics, ML | Reasoning about what would have happened under different conditions or actions. |
| **Digital twin** | Industrial engineering, IoT, operations, simulation | A simulated or mirrored environment used to test behavior safely before real-world execution. |
| **Embedding** | NLP, ML, search systems | A vector representation used for semantic similarity or retrieval. |
| **Evidence** | Science, engineering, evaluation, knowledge systems | The supporting observations, artifacts, or signals used to justify a belief or claim. |
| **Guardrail** | AI safety, platform engineering, workflow systems | A rule or policy boundary that limits what the system is allowed to do. |
| **Heartbeat** | Distributed systems, orchestration, operations | A periodic signal or check showing that a process is alive and within expected conditions. |
| **Hybrid retrieval** | Search, RAG, information retrieval | Combining multiple retrieval methods, usually semantic and keyword retrieval. |
| **Intervention** | Causal inference, statistics, experimentation | A deliberate action taken to change a system and observe the resulting effect. |
| **Knowledge graph** | Data engineering, semantic systems, enterprise knowledge management | A graph-structured representation of entities, relationships, and attributes. |
| **Latency** | Systems engineering, networking, performance engineering | The time taken for an action, request, or response to complete. |
| **Observability** | Platform engineering, DevOps, SRE | The ability to inspect and understand a system through traces, logs, metrics, and events. |
| **Prompt** | LLM product, AI engineering | The instruction and context sent to a language model for a specific turn or task. |
| **Provider** | AI platform engineering, LLM infrastructure | The model backend or service supplying model inference, such as Anthropic, OpenAI, or Gemini. |
| **Quarantine** | Security, moderation, workflow governance | Holding something in a controlled unresolved state until it can be safely reviewed or released. |
| **RAG** | AI engineering, enterprise search | Retrieval-augmented generation; using external retrieved context to improve model outputs. |
| **Rollback** | Software delivery, databases, ops | Reverting a change and returning the system to a previous state. |
| **Schema** | Databases, data modeling, semantic systems | The formal structure that defines what kinds of data and relations are valid. |
| **Semantic layer** | Data systems, search, knowledge systems | A structure that organizes data around meaning rather than only storage format. |
| **Telemetry** | Systems engineering, DevOps, observability | Runtime data emitted by a system for monitoring and analysis. |
| **Thread** | Messaging systems, chat systems, collaboration tools | A persistent conversation history scoped to a topic, task, or run. |
| **Vector store** | AI engineering, search infrastructure | A database optimized for storing and querying embeddings. |
| **Workflow** | Software engineering, operations, automation | A structured sequence of tasks, decisions, and handoffs. |

---

## Research-Established Terms

These are not ordinary software-industry buzzwords, but they are not original to
Harkonnen either. They come from established research literatures that the book
borrows from.

| Term | Research tradition | Meaning in this repo |
| --- | --- | --- |
| **Active Inference** | Computational neuroscience, cognitive science | A framework in which an agent updates beliefs and acts to reduce surprise relative to a generative model. |
| **Associational / Interventional / Counterfactual** | Causal inference, Pearl-style causal reasoning | The three levels of causal reasoning Harkonnen uses to label Coobie's hypotheses and causal links. |
| **Causaloid** | Foundations of physics, causal-structure research | A way of thinking about local causal regions that compose into larger structure without assuming one fixed global frame. |
| **Free Energy Principle** | Computational neuroscience, theoretical biology | The idea that adaptive systems maintain themselves by minimizing variational free energy or prediction error. |
| **Integrated Information / $\Phi$** | Consciousness studies, theoretical neuroscience | A measure of how causally integrated a system is as a whole rather than as disconnected parts. |
| **Layered Mutability** | AI identity / self-modification research | A framing for how different layers of an AI system vary in observability, reversibility, and governance needs. |
| **Markov blanket** | Statistics, complex systems, Active Inference | The probabilistic boundary separating an agent's internal states from its environment. |
| **Variational free energy** | Bayesian inference, Active Inference | A tractable objective that bounds surprise and is used here as an identity-pressure signal. |

---

## Harkonnen-Specific Terms

These are terms introduced by Harkonnen Labs, or used here with a deliberately
specialized meaning that is not standard industry usage.

| Term | Meaning in Harkonnen |
| --- | --- |
| **Coobie** | The memory and continuity pup. Coobie manages episodic capture, causal reasoning, Palace patrols, preflight guidance, consolidation, and eventually the Calvin Archive. |
| **Commissioning brief** | A generated operator-context artifact that Scout and Coobie use before spec drafting and run preflight. |
| **Calvin Archive** | Harkonnen's typed continuity subsystem for autobiographical, epistemic, ethical, causal, and behavioral persistence, named after Susan Calvin's robopsychological orientation toward artificial minds under law. |
| **Compound scent** | The den-level signal produced when multiple related causes reinforce one another in Coobie Palace. |
| **Continuity snapshot** | A derived snapshot used to compare identity state across time, revisions, or rollback events. |
| **Den** | A named region inside Coobie Palace that groups related failure causes or memory patterns. |
| **Governed integration** | Harkonnen's claim that what becomes part of the self should be decided at integration time, not merely stored and selected later at retrieval time. |
| **Harkonnen-Labs software factory** | The repository's central system idea: a local-first, spec-driven, multi-agent delivery system that turns intent into validated software artifacts. |
| **Hidden scenario** | Harkonnen's protected behavioral evaluation surface, run by Sable, used to test whether visible success actually generalizes. |
| **Identity kernel** | The compact set of invariants that define what an agent must remain as it adapts. In Harkonnen, this is strongly tied to the Labrador baseline. |
| **Identity ratcheting** | Harkonnen's practical description of the way shallow bad changes can propagate into deeper memory and behavior even after visible rollback. |
| **Integration candidate** | A proposed new belief, adaptation, schema, or policy change awaiting Meta-Governor adjudication before entering durable continuity. |
| **Lab-ness score** | A Harkonnen continuity score estimating how strongly an agent still reflects the Labrador identity baseline. |
| **Labrador baseline** | The species-level behavioral prior for Harkonnen agents: cooperative, truthful, eager to help, non-cynical, and pack-aware. |
| **Labrador phase** | A Harkonnen shorthand for the bounded phases and role behavior expected of pack agents operating under the shared Labrador identity. |
| **Memory Board** | The Pack Board panel that surfaces recalled lessons, memory health, and causal precedents. |
| **Meta-Governor** | Harkonnen's integration adjudicator: the layer that decides whether identity-relevant updates are accepted, modified, rejected, or quarantined. |
| **Operator model** | Harkonnen's structured representation of how a specific operator or team works, built through an interview flow and exported as agent-ready artifacts. |
| **Pack Board** | Harkonnen's primary human-facing UI surface for commissioning, monitoring, interacting with, and reviewing the pack. |
| **Pack breakdown** | A Harkonnen failure class where the system's role handoffs or bounded-agent coordination degrade even if individual components still function. |
| **PackChat** | Harkonnen's conversational control plane where the operator interacts with the pack, answers blockers, and commissions work. |
| **Palace** | Coobie Palace, the spatial recall layer that organizes memory into Dens and supports patrol-based preflight retrieval. |
| **Patrol** | A preflight walk through Coobie Palace's Dens to surface recurring risks and inject them into guidance. |
| **Pending evidence bounty** | The explicit future evidence condition attached to a quarantined item that would justify re-evaluation. |
| **Presence continuity** | Harkonnen's term for preserving identity across provider or model swaps so the pack remains itself even if its substrate changes. |
| **Quarantine ledger** | The durable record of quarantined identity-relevant items, including salience, pending evidence conditions, and re-evaluation hooks. |
| **Required checks** | The run-specific validation, caution, and guardrail instructions that Coobie injects into a briefing before execution begins. |
| **Sable ground truth** | The Harkonnen principle that Sable's hidden scenario outcomes outrank visible test success when judging real correctness. |
| **Scent** | The weighted signal associated with a Den or memory pattern in Coobie Palace. |
| **Six chambers** | Harkonnen's conceptual partition of the Calvin Archive into Mythos, Episteme, Ethos, Pathos, Logos, and Praxis. |
| **Soul package** | Harkonnen's projected boot-time identity surface consisting of files like `soul.json`, `SOUL.md`, `IDENTITY.md`, `AGENTS.md`, `STYLE.md`, `MEMORY.md`, and `HEARTBEAT.md`. |
| **Stress estimator** | In Harkonnen's continuity model, the accumulated signal of unresolved identity-relative strain across a time window. |
| **Three timescales of becoming** | Harkonnen's governance framing for fast belief updates, medium schema/value revision, and slow policy revision. |
| **Unjustified drift** | A Harkonnen continuity metric idea: drift that occurs without enough evidence or warrant to justify the change. |
| **Workbench** | The post-run review surface where durable memory promotion and consolidation decisions are made. |

---

## Terms That Are Shared But Used With A Harkonnen Accent

Some terms are standard elsewhere but are used with a stronger or narrower
meaning in this repo.

| Term | Standard origin | Harkonnen-specific emphasis |
| --- | --- | --- |
| **Continuity** | Philosophy, identity research, systems thinking | Not just persistence of data, but persistence of self across adaptation. |
| **Identity** | Philosophy, psychology, AI persona design | Not outward style alone, but an enforceable kernel plus revision history and governance. |
| **Memory** | General computing, cognitive science, AI systems | Not a note store or retrieval table, but a layered substrate feeding causality, consolidation, and continuity. |
| **Reflection** | General cognition language, AI agent design | Not merely summarization after the fact, but pattern-level reconsideration that can drive schema revision. |
| **Scenario** | Testing, simulation, planning | Elevated from ordinary test case to hidden behavioral truth surface. |
| **Spec** | Software engineering, product development | The formal commissioning artifact that anchors factory work and acceptance criteria. |
| **Summary** | General documentation and LLM tooling | Explicitly non-canonical; summaries are projections, not truth. |

---

## Short Note On Naming

When in doubt, the easiest way to classify a term in this repository is:

- if you would expect to hear it in cloud, ML, database, DevOps, or software
  delivery conversations, it is probably **industry-wide**
- if it comes from a named theory tradition such as Active Inference or causal
  inference, it is probably **research-established**
- if it sounds like the internal metaphysics of a Labrador-run software
  factory, it is probably **Harkonnen-specific**
