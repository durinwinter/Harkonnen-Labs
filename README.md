# Harkonnen Labs

## A Local-First, Causally-Aware AI Software Factory (WIP)

Harkonnen Labs is a **multi-agent software execution system** that transforms specifications into validated software artifacts while accumulating **structured operational knowledge** across runs.

At its core, Harkonnen is designed to solve a specific failure mode in modern LLM systems:

> LLM pipelines are *stateless, similarity-driven, and non-causal* — they cannot reliably improve from experience.

Harkonnen introduces a **stateful, causally-informed execution model** where:

* **Agents** perform bounded roles in a production pipeline
* **Memory** persists across runs with explicit structure
* **Causal reasoning** separates correlation from intervention
* **Policy** governs what actions are allowed, not just what is possible

The result is a system that does not just generate software — it **learns how to produce better software over time**.

---

## Conceptual Model

Harkonnen operates as a **closed-loop software factory**:

```text
Specification
   ↓
Multi-Agent Execution
   ↓
Validation (including hidden scenarios)
   ↓
Artifact Production
   ↓
Memory Ingestion (episodic)
   ↓
Consolidation → Semantic + Causal Knowledge
   ↓
Improved Future Execution
```

This loop converts **execution traces into reusable knowledge**.

---

## System Components

### 1. Agent Pack (Execution Layer)

Harkonnen decomposes execution into specialized agents:

* **Scout** → specification parsing and ambiguity detection
* **Mason** → code generation and modification
* **Bramble** → test generation and evaluation
* **Sable** → hidden scenario execution (ground truth validation)
* **Flint** → artifact packaging and output structuring
* **Keeper** → policy enforcement and boundary control
* **Coobie** → memory, consolidation, and causal reasoning

This is not a monolithic agent — it is a **role-constrained system with explicit handoffs**.

---

### 2. Coobie (Layered Memory System)

Coobie implements a **multi-layer memory architecture**:

| Layer           | Purpose                                            |
| --------------- | -------------------------------------------------- |
| Working Memory  | Current run state (compressed, ephemeral)          |
| Episodic Memory | Ordered execution traces (state → action → result) |
| Semantic Memory | Stable facts, patterns, invariants                 |
| Causal Memory   | Intervention-aware cause/effect relationships      |
| Team Blackboard | Shared agent coordination state                    |
| Consolidation   | Promotion, pruning, and abstraction                |

---

### 3. Causal Memory (Key Differentiator)

Most systems rely on **semantic similarity**:

> “This looks like something that worked before”

Harkonnen builds toward **causal inference**:

> “When we changed X under context Y, Z occurred”

Coobie tracks:

* **Association** — co-occurrence patterns
* **Intervention** — outcome changes due to explicit actions
* **Counterfactuals** — inferred alternative outcomes

Causal knowledge is represented as:

* structured **episodes**
* promoted **semantic facts**
* **causal claims** with:

  * supporting evidence
  * contradiction tracking
  * scoped applicability
  * confidence over time

---

### 4. Persistence Model

* **SQLite** → episodic memory and run state
* **Filesystem** → specs, artifacts, evidence
* **(Optional) TypeDB** → semantic + relational knowledge
* **(Optional) Vector store** → retrieval acceleration
* **(Future) Causal graph / causaloids** → executable reasoning

---

### 5. Execution Semantics

Each run produces:

1. **Artifacts** (code, configs, outputs)
2. **Episodes** (what happened)
3. **Evaluations** (did it work?)
4. **Memory updates** (what should be remembered)

Over time:

> The system transitions from *prompt-driven behavior* to *memory-informed behavior*.

---

## ⚡ Quickstart

### 1. Clone + Build

```bash
git clone https://github.com/durinwinter/Harkonnen-Labs.git
cd Harkonnen-Labs

cargo build
```

---

### 2. Start the Factory

```bash
cargo run
```

You should see:

* agent initialization
* memory system startup
* factory ready state

---

### 3. Create a Spec

```bash
mkdir -p specs
```

```json
// specs/hello_api.json
{
  "name": "hello-api",
  "description": "Create a simple REST API",
  "language": "rust",
  "requirements": [
    "axum server",
    "GET /hello endpoint",
    "returns JSON"
  ]
}
```

---

### 4. Run the Spec

```bash
cargo run -- run specs/hello_api.json
```

---

### 5. Inspect Outputs

Artifacts:

```bash
artifacts/
```

Runs:

```bash
runs/<run_id>/
```

Memory:

```bash
factory/memory/
```

---

## 🛠 Core Commands

### Run a spec

```bash
cargo run -- run <spec.json>
```

---

### Run with memory influence

```bash
cargo run -- run <spec.json> --with-memory
```

---

### List runs

```bash
cargo run -- runs list
```

---

### Inspect a run

```bash
cargo run -- runs inspect <run_id>
```

---

### Inspect memory

```bash
cargo run -- memory list
```

```bash
cargo run -- memory inspect <memory_id>
```

---

### Ingest knowledge

```bash
cargo run -- memory ingest ./docs/
```

```bash
cargo run -- memory ingest https://example.com
```

---

### Debug mode

```bash
RUST_LOG=debug cargo run -- run specs/hello_api.json
```

---

##  Project-Level Memory

Each project can maintain isolated memory:

```text
.harkonnen/
  project-memory/
  evidence/
```

This enables:

* per-repo learning
* reuse of patterns
* isolation across domains

---

##  Example Memory Evolution

### Episode

```json
{
  "action": "retry with schema validation",
  "result": "success",
  "context": {
    "language": "rust"
  }
}
```

---

### Semantic Fact

```json
{
  "fact": "schema validation improves structured outputs",
  "confidence": 0.81
}
```

---

### Causal Claim

```json
{
  "claim": "disabling schema validation reduces latency but increases failure rate",
  "confidence": 0.74
}
```

---

##  Execution Loop

```text
Spec → Agents → Validation → Artifacts → Memory → Consolidation → Better Next Run
```

---

##  Design Principles

* **Local-first** — no required cloud dependency
* **Inspectable** — every decision traceable
* **Composable** — agents are modular
* **Causal over statistical** — prefer explanation over similarity
* **Memory is first-class** — not an afterthought

---

## ⚠️ Status

Harkonnen Labs is an **active development system**.

* Core pipeline: working
* Memory system: functional, evolving
* Causal reasoning: emerging
* APIs / CLI: stabilizing

Expect rapid iteration.

---

## 🚀 Direction

Near-term:

* stronger causal claim system
* intervention-aware execution
* policy integration

Mid-term:

* executable causal units (causaloids)
* automated causal discovery
* cross-project knowledge reuse

Long-term:

* **self-improving software factory**


