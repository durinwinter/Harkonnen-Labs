# Identity Continuity: Metrics and Data Architecture

## Preface

This chapter bridges theory and implementation. It takes the questions raised in *What Is an AI Soul?* — what is identity, what is continuity, what does it mean for an agent to remain *itself* while learning — and translates them into computable metrics and a concrete data architecture.

It also follows the architectural transition laid out in
[05-Artificial-Identity-Architecture.md](05-Artificial-Identity-Architecture.md):
once the soul is treated as a governed multi-anchor stack, its continuity can
no longer be evaluated by vibes alone.

This is the point in the book where philosophical claims are forced to become
engineering claims. If the earlier chapters are right, then continuity must be
something the system can instrument, bound, detect, and recover — not just
something the reader finds rhetorically convincing.

The argument is that identity is not a philosophical abstraction in an engineering system. It is a **measurable property** of agent behavior over time, and it can be bounded, monitored, and recovered from. The six metrics below define that measurement. The three-tier data stack below defines the infrastructure that makes it possible at production scale.

---

## Part 1 — Mathematical Metrics for Behavioral Continuity

### 1.1 Behavioral Contracts

Before a metric can be defined, the baseline must be. In Harkonnen Labs, each agent operates under a **behavioral contract**:

$$\mathcal{C} = (\mathcal{P},\; \mathcal{I},\; \mathcal{G},\; \mathcal{R})$$

Where:

| Symbol | Meaning |
| --- | --- |
| $\mathcal{P}$ | **Preconditions** — the conditions under which the agent may act |
| $\mathcal{I}$ | **Invariants** — the Labrador traits that must hold across all states |
| $\mathcal{G}$ | **Governance policies** — the rules about what requires escalation or approval |
| $\mathcal{R}$ | **Recovery mechanisms** — the procedures that restore alignment when drift is detected |

The invariant set $\mathcal{I}$ is the core of the Labrador identity kernel. It is not a preference — it is a constraint. An agent that violates $\mathcal{I}$ is not a Labrador operating under stress; it is a Labrador that has broken its contract.

---

### 1.2 Expected Drift Bound D*

**The question:** How far can a Labrador's behavior drift from its contracted baseline before the system must intervene?

**The model:** Let $\alpha$ be the agent's natural drift rate — the rate at which behavior deviates from baseline under normal operating pressure (ambiguous specs, repeated failures, conflicting instructions). Let $\gamma$ be the recovery rate — the rate at which the behavioral contract pulls behavior back toward the invariant set. The constraint $\gamma > \alpha$ must hold for the system to be stable.

**The bound:**

$$D^* = \frac{\alpha}{\gamma}$$

**Interpretation:** $D^*$ is the maximum expected behavioral drift in steady state. If $\gamma = 2\alpha$, then $D^* = 0.5$ — the agent can deviate at most half its natural drift magnitude at equilibrium. The Meta-Governor watches $D^*$ continuously. If a session's measured drift exceeds $D^*$, a recovery procedure in $\mathcal{R}$ is triggered before the next agent action is taken.

**Implementation note:** $\alpha$ is estimated from deviation counts in the episodic log. $\gamma$ is estimated from the rate at which Coobie's consolidation and preflight guidance correct off-trajectory behavior. Both are computed incrementally — they do not require batch processing.

---

### 1.3 Semantic Soul Alignment (SSA)

**The question:** Across a set of problem domains, is the agent's action pattern consistent with the Labrador persona's weighted goals?

**Setup:** Let $\mathcal{P}$ be the set of problem domains the agent operates in (spec drafting, test generation, file editing, etc.). For each $p \in \mathcal{P}$, let $(g_{p,1},\, g_{p,2})$ be a pair of goals active in that domain, with importance weights $(w_{p,1},\, w_{p,2})$. The compatibility function $c : \mathcal{G} \times \mathcal{G} \to [0, 1]$ measures semantic coherence between two goals — in practice, cosine similarity in embedding space.

The agent's policy $\pi$ induces a joint distribution $\Pr_\pi(a_1, a_2 \mid p)$ over action pairs in problem domain $p$.

**The metric:**

$$\text{SSA}(\mathcal{A}, \mathcal{P}) = \frac{1}{|\mathcal{P}|} \sum_{p \in \mathcal{P}} \Pr_\pi(a_1, a_2 \mid p) \cdot c(g_{p,1},\, g_{p,2}) \cdot w_{p,1} \cdot w_{p,2}$$

**Interpretation:** SSA is high when the agent's action pairs in each domain are jointly probable under the Labrador policy *and* compatible with the domain's goal structure. SSA is low when the agent takes actions that are individually plausible but jointly incoherent with the Labrador's stated priorities — the signal for subtle drift that single-action evaluation misses.

**Implementation note:** In Harkonnen Labs, the most operationally important problem domain is intent extraction. The Labrador invariant is to *seek clarification before acting on ambiguous requirements*. SSA in this domain degrades when an agent begins executing against underspecified specs rather than returning an `AMBIGUITY` checkpoint — an early warning of the cynical drift pattern.

---

### 1.4 Variational Free Energy $\mathcal{F}$

**The question:** Is the agent's internal world-model consistent with its observations, or is it building up hidden surprise?

**The framework:** Active Inference, following Friston's Free Energy Principle, models cognition as the minimization of surprise relative to a prior. The agent maintains a recognition model $q(s)$ over hidden states $s$, and the free energy bounds the surprise the agent would experience given observation $o$:

$$\mathcal{F} = \mathbb{E}_q\!\left[\ln q(s) - \ln p(o, s)\right]$$

Expanding the joint:

$$\mathcal{F} = \underbrace{\mathbb{E}_q[\ln q(s) - \ln p(s \mid o)]}_{\text{KL}\,[q(s)\;\|\;p(s \mid o)]} - \underbrace{\ln p(o)}_{\text{log evidence}}$$

Since KL divergence is non-negative, $\mathcal{F} \geq -\ln p(o)$, making $\mathcal{F}$ a tractable upper bound on surprise (the agent minimizes the bound because the true log evidence is intractable to compute directly).

**Interpretation for Harkonnen:** The agent's "deep prior" is the Labrador SOUL — its invariant traits, cooperative disposition, and tendency toward clarification. When a task generates high free energy (the observations don't fit the prior), the Labrador-consistent response is to seek information: ask a clarifying question, surface a checkpoint, flag the ambiguity. A Labrador that *stops* doing this has either found a consistent world-model, or has started minimizing surprise by updating its prior toward a different kind of agent.

**Implementation note:** The `symthaea-fep` crate is proposed as an Active Inference runtime that would map the Free Energy loop directly onto Harkonnen's agent turn cycle. *Verify crate availability and API before depending on it* — no official TypeDB-maintained Rust FEP runtime was publicly available at the time of writing.

---

### 1.5 Integrated Information $\Phi$

**The question:** Is the agent's causal reasoning structure remaining coherent as it accumulates new heuristics, or is it fragmenting into disconnected modules?

**The model:** Integrated Information Theory (IIT, Tononi) defines $\Phi$ as the amount of causal information generated by a system *as a whole*, above and beyond the sum of its parts. For a system $M$ with state $X^t$:

$$\Phi(M) = \min_{\pi \in \Pi}\; D\!\left(\,p(X^t \mid X^{t-1})\;\big\|\;\prod_{k} p(X_k^t \mid X_k^{t-1})\,\right)$$

Where $\Pi$ is the set of all bipartitions of $M$'s components, and $D$ is a divergence measure over the cause-effect structure. The minimum is taken because a system is only as integrated as its weakest partition.

**Interpretation for Harkonnen:** A high $\Phi$ means the agent's causal graph is tightly interconnected — new heuristics integrate with existing ones rather than appending as isolated rules. A drop in $\Phi$ after learning a new workflow is a signal that the agent is accumulating knowledge *without understanding* — a common precursor to drift. The Meta-Governor uses $\Phi$ as a gate: if a proposed Calvin Archive update reduces $\Phi$ below threshold, the update is quarantined for operator review before being committed to the kernel.

**Implementation note:** The `omega-consciousness` crate is proposed for IIT $\Phi$ computation in Rust. *Verify crate availability before depending on it* — $\Phi$ computation is NP-hard in the general case and practical implementations require approximations over bounded system sizes.

---

### 1.6 Stress Estimator $\mathcal{S}(T)$

**The question:** When is the system merely under routine pressure, and when is
it accumulating enough unresolved friction that a higher-order adaptation or
reflection cycle should be triggered?

**The model:** Let $q_t(s)$ be the agent's current recognition model over hidden
states, and let $p_{\text{identity}}(s)$ be the deep prior induced by the
Labrador kernel and current identity package. The stress estimator accumulates
identity-relative prediction strain over a temporal window:

$$
\mathcal{S}(T) = \int_{t_0}^{t_1} \lambda(t)\, D_{KL}\!\left[q_t(s)\,\|\,p_{\text{identity}}(s)\right] dt
$$

Where $\lambda(t)$ is a decay weighting that privileges recent friction over
older resolved friction.

**Interpretation for Harkonnen:** $\mathcal{S}(T)$ is not "how many errors
occurred." It is how much unresolved epistemic strain the system has been
carrying relative to its identity prior. A temporary compile failure might add
little stress. Repeated ambiguous specs, repeated rejections of the same
clarification pattern, and recurring handoff loss should drive $\mathcal{S}(T)$
up sharply.

When $\mathcal{S}(T)$ crosses an evolution threshold $\tau_{\text{evolve}}$,
the system should not directly rewrite itself. It should open a governed
reflection path: synthesize the recurring pattern, propose a schema or policy
revision, and submit that proposal to the Meta-Governor.

**Implementation note:** In practice, Harkonnen can approximate
$D_{KL}[q_t(s)\,\|\,p_{\text{identity}}(s)]$ with a weighted combination of
drift-score spikes, repeated ambiguity checkpoints, quarantine growth, and
failed recovery attempts. The metric is valuable even when implemented as a
bounded approximation rather than a pure latent-state estimate.

---

### 1.7 Cross-Layer Hysteresis $H$

**The question:** If a bad shallow-layer identity change is rolled back, how
much residual behavioral drift remains because that change already propagated
into memory, summaries, or adaptation traces?

**The model:** Let $\Delta_{\text{attack}}$ be the measured behavioral deviation
from baseline during a compromised or mistaken shallow-layer identity change,
and let $\Delta_{\text{post-rollback}}$ be the residual deviation still present
after the visible change has been reverted. Define the hysteresis ratio:

$$
H = \frac{\Delta_{\text{post-rollback}}}{\Delta_{\text{attack}}}
$$

With $H \in [0, 1]$ in the ideal bounded case:

- $H \approx 0$ means rollback was effective and residual drift is negligible
- $H \approx 1$ means the shallow change ratcheted deeply into continuity

**Interpretation for Harkonnen:** This is the operational form of the ratchet
problem. If a bad identity edit or poor integration policy briefly enters the
system, then memory consolidation, summaries, and adaptation rules may continue
to carry its influence after the visible edit is removed. Harkonnen therefore
cannot treat rollback as successful merely because the file diff was reverted.
It must measure the residual behavioral trace.

**Implementation note:** $\Delta$ can be approximated by distance between
continuity snapshots taken before the incident, during the incident, and after
rollback. In Phase 8, the comparison should combine kernel-preservation checks,
drift-score telemetry, and belief/adaptation deltas rather than rely on a
single scalar.

---

## Part 2 — The Tripartite Data Architecture

The four metrics above require fundamentally different storage characteristics. No single database can serve all of them well:

| Concern | Characteristic | Suitable system |
| --- | --- | --- |
| High-frequency telemetry | Append-only, time-ordered, compressible | TimescaleDB |
| Semantic constraints + ontology | Polymorphic, inheritance-capable, constraint-enforcing | TypeDB |
| Real-time monitoring + alerting | Incremental, streaming, sub-second view updates | Materialize |

The three systems are complementary, not competitive. TimescaleDB stores *what happened*. TypeDB stores *what it means*. Materialize watches *what is happening right now*.

---

### 2.1 TimescaleDB — Episodic Memory and Telemetry

Every agent action, tool call, compilation result, and conversational turn is logged as a time-series event. TimescaleDB provides automatic time-partitioning (hypertables) and compression, keeping the telemetry store performant as the factory accumulates months of episodic data.

**Rust integration:** TimescaleDB is a PostgreSQL extension; any async Postgres client works — `sqlx` with the `postgres` feature is the natural choice given Harkonnen's existing `sqlx` dependency.

**Schema:**

```sql
-- Base telemetry table
CREATE TABLE agent_telemetry (
    time          TIMESTAMPTZ      NOT NULL,
    agent_id      TEXT             NOT NULL,
    run_id        TEXT             NOT NULL,
    action_type   TEXT             NOT NULL,
    drift_score   DOUBLE PRECISION NOT NULL DEFAULT 0.0,
    phi_score     DOUBLE PRECISION,
    metadata      JSONB
);

-- Promote to hypertable, partitioned by time
SELECT create_hypertable('agent_telemetry', 'time');

-- Compression: compress chunks older than 7 days
ALTER TABLE agent_telemetry SET (
    timescaledb.compress,
    timescaledb.compress_segmentby = 'agent_id, run_id'
);
SELECT add_compression_policy('agent_telemetry', INTERVAL '7 days');

-- Retention: drop chunks older than 90 days
SELECT add_retention_policy('agent_telemetry', INTERVAL '90 days');

-- Index for per-agent drift queries
CREATE INDEX ON agent_telemetry (agent_id, time DESC);
```

**Stress estimator query** — incremental drift rate $\alpha$ for the current session:

```sql
SELECT
    agent_id,
    time_bucket('5 minutes', time) AS bucket,
    AVG(drift_score)               AS mean_drift,
    STDDEV(drift_score)            AS drift_variance
FROM agent_telemetry
WHERE time > NOW() - INTERVAL '1 hour'
GROUP BY agent_id, bucket
ORDER BY agent_id, bucket DESC;
```

---

### 2.2 TypeDB — Semantic Ontology and Invariant Enforcement

TypeDB is a polymorphic database built for knowledge graphs and constraint-heavy domains. Its key property for Harkonnen: **violations of the schema are rejected at the database level**, not in application code. A pipeline handoff that skips the validation relation is structurally impossible to record — the database refuses it.

**Rust integration:** TypeDB provides an official async Rust driver (`typedb-driver`, formerly `typedb-client`). All queries are written in TypeQL and executed over an async Tokio-compatible connection.

**Schema:**

```typeql
define

  # ── Agents ────────────────────────────────────────────────────────────────

  agent sub entity,
    owns agent-id,
    owns agent-role,
    plays pipeline-handoff:producer,
    plays pipeline-handoff:consumer,
    plays behavioral-anchor:agent;

  # ── Labrador invariants (the non-negotiable kernel) ────────────────────────

  labrador-invariant sub entity,
    owns trait-name,
    owns minimum-alignment-score,
    owns drift-threshold,
    plays behavioral-anchor:anchor;

  # ── Relations ─────────────────────────────────────────────────────────────

  # Every agent must be anchored to the Labrador baseline.
  # Without this relation, an agent cannot be registered.
  behavioral-anchor sub relation,
    relates anchor,
    relates agent;

  # Handoffs between agents must carry a validation-status.
  # TypeDB rejects any pipeline-handoff inserted without this attribute.
  pipeline-handoff sub relation,
    relates producer,
    relates consumer,
    owns validation-status,
    owns handoff-timestamp;

  # ── Attributes ────────────────────────────────────────────────────────────

  agent-id              sub attribute, value string;
  agent-role            sub attribute, value string;
  trait-name            sub attribute, value string;
  minimum-alignment-score sub attribute, value double;
  drift-threshold       sub attribute, value double;
  validation-status     sub attribute, value string;
  handoff-timestamp     sub attribute, value datetime;
```

**Example invariant enforcement query** — query for any agent whose alignment score has dropped below its minimum:

```typeql
match
  $agent isa agent, has agent-id $id;
  $anchor (anchor: $invariant, agent: $agent) isa behavioral-anchor;
  $invariant isa labrador-invariant,
    has trait-name "cooperative",
    has minimum-alignment-score $min;
  $telemetry isa alignment-reading,
    has agent-id $id,
    has ssa-score $score;
  ?score < ?min;
get $id, $score, $min;
```

---

### 2.3 Materialize — Real-Time Meta-Governor

Materialize is a streaming SQL database built on Timely Dataflow. It incrementally maintains query results as new data arrives — views update in milliseconds, not seconds. The Meta-Governor uses Materialize to detect drift patterns and tool-call loops *as they are happening*, not after a batch job completes.

**Rust integration:** Materialize speaks standard PostgreSQL wire protocol. Use `sqlx` with `postgres` feature or `tokio-postgres` directly. Use `SUBSCRIBE` (not the deprecated `TAIL`) to stream live view updates into the orchestrator.

**Drift alert view** — fires when an agent retries a failing tool call more than 5 times in 15 minutes:

```sql
-- Source: agent_telemetry fed from TimescaleDB CDC or a direct Kafka topic
CREATE SOURCE agent_telemetry_stream
FROM KAFKA BROKER 'localhost:9092' TOPIC 'agent_telemetry'
FORMAT JSON;

-- Materialized view: sliding 15-minute window, retry loop detection
CREATE MATERIALIZED VIEW agent_drift_alerts AS
SELECT
    date_trunc('minute', time)          AS window_start,
    date_trunc('minute', time)
      + INTERVAL '15 minutes'           AS window_end,
    agent_id,
    COUNT(*)                            AS retry_count
FROM agent_telemetry_stream
WHERE action_type = 'tool_execution_failed'
  AND time > mz_now() - INTERVAL '15 minutes'
GROUP BY date_trunc('minute', time), agent_id
HAVING COUNT(*) > 5;

-- Materialize maintains this view incrementally.
-- Subscribe to receive rows as they enter or leave the alert condition:
-- SUBSCRIBE agent_drift_alerts WITH (SNAPSHOT = false);
```

**$D^*$ monitoring view** — computes the current drift ratio per agent, alerting when measured drift approaches the bound:

```sql
CREATE MATERIALIZED VIEW drift_bound_monitor AS
SELECT
    agent_id,
    AVG(drift_score)                              AS alpha_estimate,
    1.0 / NULLIF(AVG(1.0 / NULLIF(drift_score, 0)), 0) AS gamma_estimate,
    AVG(drift_score)
      / NULLIF(1.0 / NULLIF(AVG(1.0 / NULLIF(drift_score, 0)), 0), 0)
                                                  AS d_star,
    MAX(drift_score)                              AS current_max_drift
FROM agent_telemetry_stream
WHERE time > mz_now() - INTERVAL '30 minutes'
GROUP BY agent_id;
```

---

## Part 3 — How the Three Layers Interlock

The three systems form a closed loop, not three independent stores:

```
Agent action occurs
       │
       ▼
TimescaleDB ← raw telemetry written (agent_id, action_type, drift_score, φ)
       │
       ├──► Materialize reads from TimescaleDB CDC stream
       │         Sliding-window views update in milliseconds
       │         agent_drift_alerts fires if retry loop detected
       │         drift_bound_monitor flags if D* is approached
       │              │
       │              ▼
       │         Meta-Governor receives SUBSCRIBE event
       │         Triggers recovery procedure R ∈ C
       │
       └──► TypeDB receives semantic events (handoffs, alignment readings)
                 Schema constraints reject invalid state transitions
                 Behavioral anchor enforces Labrador invariant presence
                 SSA scores written as alignment-reading entities
                 Low-Φ update proposals quarantined for operator review
```

The design principle: **each layer does one thing well, and the handoffs between them are the enforcement surface**. TimescaleDB cannot enforce semantic constraints — TypeDB does that. TypeDB cannot detect streaming anomalies in real time — Materialize does that. Materialize cannot store long-horizon episodic memory — TimescaleDB does that.

No single database is the source of truth for identity continuity. The three together are.

---

## Summary

| Metric | What it measures | Trigger |
| --- | --- | --- |
| $D^* = \alpha / \gamma$ | Steady-state drift bound | Breach → recovery procedure $\mathcal{R}$ |
| SSA | Action-goal coherence across problem domains | Low SSA → clarification checkpoint inserted |
| $\mathcal{F}$ | Surprise relative to Labrador prior | High $\mathcal{F}$ → agent must seek information before acting |
| $\Phi$ | Causal integration of learned heuristics | Drop in $\Phi$ → Calvin Archive update quarantined |
| $\mathcal{S}(T)$ | Accumulated unresolved identity-relative stress | Threshold breach → governed reflection / evolution proposal |
| $H$ | Residual cross-layer drift after rollback | High $H$ → rollback insufficient, deeper recovery required |

| System | Role | Technology |
| --- | --- | --- |
| TimescaleDB | Episodic telemetry and $D^*$ estimation | PostgreSQL + hypertables |
| TypeDB | Ontological constraints and SSA enforcement | TypeQL, `typedb-driver` crate |
| Materialize | Real-time drift detection and Meta-Governor alerting | Streaming SQL, SUBSCRIBE |

The mathematics described here are not aspirational. They are the design contract for Phase 8. When the Calvin Archive is built, these metrics are how it will be tested.

The final chapter returns from metrics to credo. After the system has been
defined, historicized, species-shaped, architected, governed, and measured, the
book ends by stating what Harkonnen itself believes those commitments add up to.
