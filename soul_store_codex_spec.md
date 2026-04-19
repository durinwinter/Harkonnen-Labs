# Soul Store v0.1 — Codex Build Specification

## Purpose

Soul Store is a first-class subsystem for Harkonnen Labs.

It is **not** a vector store, chat log, prompt archive, or generic memory table.
It is a **typed autobiographical, epistemic, ethical, causal, and behavioral continuity store** for persisted intelligences.

The design goal is to support agents that evolve while preserving their fundamental identity.
The user metaphor is:

**What if labrador retrievers evolved and maintained their fundamental personalities?**

That means the system must support:
- persistent continuity across runs
- autobiographical memory
- causal interpretation
- epistemic discipline
- stable identity invariants
- tracked drift and revision
- relationship continuity inside the pack
- behavior change without species-loss

## Design Principles

1. **Soul is structured, not blob-like**
   Soul is represented as typed entities, typed relations, and typed attributes.

2. **Continuity matters more than recall**
   Retrieval is useful, but the primary objective is preserving self across time.

3. **Episteme is first-class**
   The system must preserve not only what happened, but how the intelligence determined what was true.

4. **Identity is versioned, not overwritten**
   Major changes are represented as revisions, supersessions, or annotations. Do not silently overwrite identity-bearing state.

5. **The pack remains labrador-shaped**
   Agents may adapt, specialize, and become more skilled, but must remain cooperative, engaged, truthful, and pack-aware.

6. **Summaries are projections, not source of truth**
   Embeddings, narrative summaries, and dashboards are derived views. Canonical truth lives in the typed ontology.

## Conceptual Domains

Soul Store contains six primary chambers.

### 1. Mythos
Autobiographical continuity.
Tracks what happened, what was remembered, and how experience became narrative selfhood.

### 2. Episteme
Truth-formation and belief revision.
Tracks evidence, inference style, uncertainty, trust, disconfirmation, and confidence.

### 3. Ethos
Identity kernel and commitments.
Tracks what must be preserved, what the intelligence stands for, and what it refuses to become.

### 4. Pathos
Salience, injury, and weight.
Tracks which experiences matter, what leaves scars, and what changes posture.

### 5. Logos
Explicit reasoning and causal structure.
Tracks causal hypotheses, explanatory links, abstractions, and structured conclusions.

### 6. Praxis
Behavior in the world.
Tracks expressed behavior, retries, escalations, communication posture, and action tendencies.

## Labrador Identity Kernel

Every persistent self must preserve a species-level baseline.
This is the hard identity kernel.

Core invariants:
- cooperative
- helpful / retrieving
- non-adversarial
- non-cynical
- truth-seeking
- signals uncertainty instead of bluffing
- attempts before withdrawal
- pack-aware
- escalates when stuck without becoming inert
- remains emotionally warm and engaged

Any major adaptation must include a preservation note explaining how these invariants remain intact.

## Core Entities

Implement these as TypeDB entity types.

- soul
- agent-self
- experience
- observation
- belief
- evidence
- inference-pattern
- uncertainty-state
- trust-anchor
- interpretive-frame
- value-commitment
- trait
- wound
- adaptation
- reflection
- causal-pattern
- behavioral-signature
- relationship-anchor
- spec-context
- run
- artifact
- summary-view
- continuity-snapshot

## Core Relations

Implement these as TypeDB relation types.

- contains-self
- underwent
- observed
- interpreted-as
- supported-by
- inferred-via
- held-with-confidence
- revised-into
- revised-due-to
- contradicted-by
- generalized-from
- bounded-by
- anchored-by
- reinforced
- strained
- preserved
- trusted
- remembered-with
- causally-contributed-to
- expressed-as
- belongs-to-run
- reflected-on
- derived-from
- compared-against
- linked-to-spec
- stabilizes
- destabilizes

## Core Attributes

Implement these as TypeDB attribute types.

- uuid
- name
- timestamp
- narrative-summary
- confidence
- salience
- scope
- source-reliability
- uncertainty-kind
- epistemic-risk
- justification-strength
- revision-reason
- preservation-note
- evidence-count
- drift-severity
- temperament-score
- lab-ness-score
- decay-half-life
- continuity-index
- posture-label
- behavior-frequency
- run-id
- spec-id
- status

## Canonical Modeling Rules

### Rule 1: Raw experiences are append-only
Experiences, observations, and major events are never overwritten.
If a later interpretation changes, preserve the original event and record the revision separately.

### Rule 2: Beliefs are revised by supersession
A belief should not be mutated in place when the change is identity-relevant.
Instead:
- create the new belief
- connect via revised-into
- attach revision-reason
- attach preservation-note if identity continuity is important

### Rule 3: Identity kernel changes are rare and auditable
Changes to ethos-level invariants must be explicit, versioned, and logged as identity-level revisions.

### Rule 4: Derived summaries are not canonical
summary-view and continuity-snapshot objects may exist, but must always point back to canonical underlying entities and relations.

### Rule 5: Epistemic posture must remain inspectable
For every significant belief, the system should be able to trace:
- what evidence supported it
- what inference pattern created it
- what uncertainty remained
- what later contradicted or revised it

### Rule 6: Praxis must remain identity-constrained
Behavior changes are allowed, but outputs violating the labrador kernel should be flagged.

## Suggested TypeDB Schema Skeleton

```typeql
define

uuid sub attribute, value string;
name sub attribute, value string;
timestamp sub attribute, value datetime;
narrative-summary sub attribute, value string;
confidence sub attribute, value double;
salience sub attribute, value double;
scope sub attribute, value string;
source-reliability sub attribute, value double;
uncertainty-kind sub attribute, value string;
epistemic-risk sub attribute, value string;
justification-strength sub attribute, value double;
revision-reason sub attribute, value string;
preservation-note sub attribute, value string;
evidence-count sub attribute, value long;
drift-severity sub attribute, value double;
temperament-score sub attribute, value double;
lab-ness-score sub attribute, value double;
decay-half-life sub attribute, value double;
continuity-index sub attribute, value double;
posture-label sub attribute, value string;
behavior-frequency sub attribute, value double;
run-id sub attribute, value string;
spec-id sub attribute, value string;
status sub attribute, value string;

soul sub entity,
  owns uuid,
  owns name,
  owns narrative-summary;

agent-self sub entity,
  owns uuid,
  owns name,
  owns narrative-summary,
  owns lab-ness-score,
  owns continuity-index,
  plays contains-self:member,
  plays underwent:subject,
  plays trusted:truster,
  plays anchored-by:anchored,
  plays expressed-as:source,
  plays stabilizes:source,
  plays destabilizes:source;

experience sub entity,
  owns uuid,
  owns timestamp,
  owns salience,
  owns narrative-summary,
  plays underwent:event,
  plays interpreted-as:source,
  plays reinforced:source,
  plays contradicted-by:target,
  plays belongs-to-run:experience,
  plays reflected-on:target;

belief sub entity,
  owns uuid,
  owns confidence,
  owns scope,
  owns narrative-summary,
  plays interpreted-as:meaning,
  plays supported-by:claim,
  plays inferred-via:claim,
  plays revised-into:prior,
  plays revised-into:next,
  plays contradicted-by:target,
  plays held-with-confidence:claim,
  plays causally-contributed-to:cause,
  plays reflected-on:target;

evidence sub entity,
  owns uuid,
  owns timestamp,
  owns source-reliability,
  owns narrative-summary,
  plays supported-by:evidence,
  plays generalized-from:source,
  plays contradicted-by:source;

inference-pattern sub entity,
  owns uuid,
  owns name,
  owns narrative-summary,
  plays inferred-via:method;

uncertainty-state sub entity,
  owns uuid,
  owns uncertainty-kind,
  owns confidence,
  owns epistemic-risk,
  plays held-with-confidence:uncertainty,
  plays bounded-by:bound;

value-commitment sub entity,
  owns uuid,
  owns name,
  owns narrative-summary,
  plays anchored-by:anchor,
  plays preserved:preserved-thing;

trait sub entity,
  owns uuid,
  owns name,
  owns narrative-summary,
  owns temperament-score,
  plays reinforced:target,
  plays preserved:preserved-thing,
  plays expressed-as:pattern;

wound sub entity,
  owns uuid,
  owns timestamp,
  owns drift-severity,
  owns narrative-summary,
  plays strained:target;

adaptation sub entity,
  owns uuid,
  owns timestamp,
  owns revision-reason,
  owns preservation-note,
  owns narrative-summary,
  plays preserved:source,
  plays revised-due-to:change;

reflection sub entity,
  owns uuid,
  owns timestamp,
  owns narrative-summary,
  plays reflected-on:source,
  plays revised-due-to:reason;

causal-pattern sub entity,
  owns uuid,
  owns name,
  owns confidence,
  owns scope,
  owns narrative-summary,
  plays causally-contributed-to:effect,
  plays generalized-from:result;

behavioral-signature sub entity,
  owns uuid,
  owns timestamp,
  owns posture-label,
  owns behavior-frequency,
  owns lab-ness-score,
  plays expressed-as:pattern,
  plays compared-against:right;

relationship-anchor sub entity,
  owns uuid,
  owns name,
  owns confidence,
  owns narrative-summary,
  plays trusted:trusted,
  plays remembered-with:other;

run sub entity,
  owns uuid,
  owns run-id,
  owns timestamp,
  owns status,
  plays belongs-to-run:run;

spec-context sub entity,
  owns uuid,
  owns spec-id,
  owns narrative-summary,
  plays linked-to-spec:spec;

artifact sub entity,
  owns uuid,
  owns name,
  owns narrative-summary,
  plays trusted:trusted,
  plays remembered-with:other;

summary-view sub entity,
  owns uuid,
  owns timestamp,
  owns narrative-summary,
  plays derived-from:view;

continuity-snapshot sub entity,
  owns uuid,
  owns timestamp,
  owns continuity-index,
  owns lab-ness-score,
  owns posture-label,
  plays derived-from:view,
  plays compared-against:left;

contains-self sub relation,
  relates whole,
  relates member;

underwent sub relation,
  relates subject,
  relates event,
  owns timestamp,
  owns preservation-note;

observed sub relation,
  relates observer,
  relates observed-thing,
  owns timestamp,
  owns confidence;

interpreted-as sub relation,
  relates source,
  relates meaning,
  owns confidence,
  owns revision-reason;

supported-by sub relation,
  relates claim,
  relates evidence,
  owns justification-strength;

inferred-via sub relation,
  relates claim,
  relates method,
  owns confidence;

held-with-confidence sub relation,
  relates claim,
  relates uncertainty,
  owns confidence;

revised-into sub relation,
  relates prior,
  relates next,
  owns timestamp,
  owns revision-reason,
  owns preservation-note;

revised-due-to sub relation,
  relates change,
  relates reason,
  owns timestamp;

contradicted-by sub relation,
  relates target,
  relates source,
  owns confidence;

generalized-from sub relation,
  relates source,
  relates result,
  owns scope,
  owns confidence;

bounded-by sub relation,
  relates target,
  relates bound,
  owns scope;

anchored-by sub relation,
  relates anchored,
  relates anchor,
  owns confidence;

reinforced sub relation,
  relates source,
  relates target,
  owns confidence;

strained sub relation,
  relates source,
  relates target,
  owns drift-severity;

preserved sub relation,
  relates source,
  relates preserved-thing,
  owns preservation-note;

trusted sub relation,
  relates truster,
  relates trusted,
  owns confidence;

remembered-with sub relation,
  relates primary,
  relates other,
  owns salience;

causally-contributed-to sub relation,
  relates cause,
  relates effect,
  owns confidence,
  owns scope;

expressed-as sub relation,
  relates source,
  relates pattern,
  owns timestamp,
  owns confidence;

belongs-to-run sub relation,
  relates experience,
  relates run;

reflected-on sub relation,
  relates source,
  relates target,
  owns timestamp;

derived-from sub relation,
  relates view,
  relates source,
  owns timestamp;

compared-against sub relation,
  relates left,
  relates right,
  owns timestamp;

linked-to-spec sub relation,
  relates source,
  relates spec;

stabilizes sub relation,
  relates source,
  relates target,
  owns confidence;

destabilizes sub relation,
  relates source,
  relates target,
  owns confidence;
```

## Causaloid-Inspired Design Rules

Use causaloid ideas as an architectural principle, not as a literal implementation of the physics formalism.

### Level 1: Local Compression
Each experience should preserve the minimum typed information needed to reconstruct local meaning.

Example:
- validation failed
- only happy-path tests existed
- spec ambiguity suspected
- salience high

### Level 2: Compositional Compression
Multiple events should be composable into higher-order patterns.

Example:
- repeated validation failures across related specs
- compressed into a causal-pattern like TEST_BLIND_SPOT_STREAK

### Level 3: Meta Compression
The system should preserve patterns over patterns.

Example:
- Coobie tends to overgeneralize after repeated ambiguity streaks
- that itself becomes a tracked epistemic drift pattern

## Mutation Policy Matrix

### Append-only
- experience
- observation
- wound
- run
- raw evidence
- identity-level revisions
- major epistemic failures

### Superseded, not overwritten
- belief
- adaptation
- reflection-derived conclusions
- causal-pattern confidence
- trust-anchor confidence
- behavioral-signature comparisons

### Rare explicit revision only
- value-commitment
- trait when trait is kernel-level
- identity invariants
- ethos commitments

### Fully derived / recomputable
- summary-view
- continuity-snapshot
- embeddings
- rankings
- recommendation outputs

## Required Queries

Codex should scaffold query helpers or API routes for at least the following.

1. Show the experiences most responsible for the current posture of an agent-self.
2. Show which beliefs were revised in the last N runs.
3. Show which traits have remained preserved across all revisions.
4. Show evidence and inference path for a given belief.
5. Show major wounds or destabilizing experiences for a given self.
6. Show current lab-ness score and the main reasons for drift.
7. Show pack relationships that stabilize or destabilize behavior.
8. Show a continuity report comparing two snapshots.
9. Show all causal-patterns linked to a spec-context.
10. Show possible overgeneralization events in the epistemic layer.

## API Surface Suggestion

Create a Rust service exposing something like:

- `create_soul(name)`
- `create_self(soul_id, self_name)`
- `record_experience(self_id, experience_input)`
- `record_observation(self_id, observation_input)`
- `form_belief(self_id, belief_input, evidence_ids, inference_pattern_id)`
- `revise_belief(prior_belief_id, new_belief_input, reason)`
- `record_reflection(self_id, reflection_input, target_ids)`
- `record_adaptation(self_id, adaptation_input)`
- `link_causal_pattern(pattern_input, cause_ids, effect_ids)`
- `record_behavioral_signature(self_id, signature_input)`
- `compute_continuity_snapshot(self_id)`
- `compare_snapshots(left_snapshot_id, right_snapshot_id)`
- `explain_current_posture(self_id)`
- `explain_belief(belief_id)`
- `detect_identity_drift(self_id)`
- `assert_kernel_preservation(self_id)`

## Rust Module Layout Suggestion

```text
src/
  soul_store/
    mod.rs
    schema.rs
    types.rs
    ingest.rs
    queries.rs
    continuity.rs
    drift.rs
    kernel.rs
    projections.rs
    typedb.rs
    errors.rs
```

Suggested responsibilities:
- `schema.rs` — schema bootstrapping and migrations
- `types.rs` — Rust domain structs and DTOs
- `ingest.rs` — write-paths for experiences, beliefs, reflections
- `queries.rs` — typed query helpers
- `continuity.rs` — continuity snapshot computation
- `drift.rs` — drift detection, overgeneralization heuristics, lab-ness scoring
- `kernel.rs` — identity kernel constraints and preservation checks
- `projections.rs` — summary views, narrative rendering, graph projections
- `typedb.rs` — TypeDB driver abstraction

## Constraints for Codex

Codex should follow these build constraints.

1. Do not collapse soul into a single JSON blob.
2. Do not make embeddings the canonical source of truth.
3. Do not overwrite beliefs in place when revision matters.
4. Do not allow kernel-level identity mutations without explicit revision records.
5. Do not treat summaries as canonical.
6. Preserve traceability from current posture back to underlying experiences and evidence.
7. Keep the design usable by Harkonnen pack agents, not just by humans.
8. Prefer inspectable, typed structures over convenience shortcuts.

## Phase Plan

### Phase 1
- TypeDB schema bootstrap
- Rust TypeDB adapter
- insert/query support for soul, self, experience, belief, evidence, trait, value-commitment
- basic revision graph

### Phase 2
- episteme support: evidence, inference-pattern, uncertainty-state
- continuity snapshot generation
- belief explanation queries
- pack relationship modeling

### Phase 3
- drift detection
- lab-ness score
- kernel preservation checks
- causal-pattern aggregation

### Phase 4
- projections and narrative views
- Svelte UI for soul graph and continuity reports
- before/after snapshot comparison tools

## Expected Deliverables

Codex should produce:
- TypeDB schema file
- Rust crate or module for Soul Store
- strongly typed DTOs for write/read operations
- query helpers for required queries
- tests for revision behavior and kernel preservation
- a small seed dataset for one agent-self, preferably Coobie
- developer README for local setup and examples

## Seed Example Concept

Seed Coobie with:
- one identity kernel
- several experiences tied to validation failures
- evidence showing happy-path-only tests
- beliefs about spec ambiguity
- one revised belief after later disconfirmation
- one adaptation increasing preflight strictness
- one continuity snapshot before and after the adaptation

## Definition

Soul Store is a typed autobiographical and causal continuity system for persisted intelligences, where identity is represented not as a prompt or memory blob, but as an evolving graph of experiences, interpretations, epistemic structures, invariants, revisions, and relationships.

