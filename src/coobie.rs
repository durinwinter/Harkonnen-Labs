//! Coobie — Causal Memory and Reasoning Engine
//!
//! Phase 1: Heuristic causal engine backed by SQLite.
//!   - Ingests factory runs as FactoryEpisode records
//!   - Scores each episode on five diagnostic dimensions
//!   - Evaluates a set of causal heuristic rules to generate CausalHypotheses
//!   - Recommends concrete InterventionPlans per hypothesis
//!   - Simulates counterfactuals by querying prior episodes where similar
//!     interventions were applied
//!   - Emits a structured CausalReport for failed/degraded runs
//!
//! Phase 2 (partially wired):
//!   Coobie now uses `deep_causality` observations, inferences, and causaloids
//!   to activate causal signals alongside the existing heuristic rule engine.
//!   The next layer is richer contextual hypergraphs and policy-oriented
//!   propagation on top of the current signal model.
//!
//! Initial causal domain: "why do runs pass internal validation but fail
//! hidden scenarios?" — the most common and useful failure pattern.

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use deep_causality::prelude::{
    BaseCausaloid, Causable, CausalityError, Causaloid, Inferable, Inference, Observable,
    Observation,
};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::models::{
    CausalHypothesis, CausalStreak, CoobieBriefing, CoobieEvidenceCitation,
    CounterfactualEstimate, CounterfactualOutcome, FactoryEpisode, InterventionPlan, LessonRecord,
    ProjectComponent, ProjectResumeRisk, ScenarioBlueprint,
};

// ── Public reasoning trait ────────────────────────────────────────────────────

#[async_trait]
pub trait CoobieReasoner: Send + Sync {
    /// Ingest a completed run as an episode and persist its causal scores.
    async fn ingest_episode(&self, ep: &FactoryEpisode) -> Result<()>;

    /// Diagnose a run: return ranked causal hypotheses with confidence.
    async fn diagnose(&self, run_id: &str) -> Result<Vec<CausalHypothesis>>;

    /// Recommend concrete interventions based on the diagnosed hypotheses.
    async fn recommend_interventions(&self, run_id: &str) -> Result<Vec<InterventionPlan>>;

    /// Simulate "what if we applied this intervention?" against historical data.
    async fn simulate_counterfactual(
        &self,
        run_id: &str,
        intervention: &InterventionPlan,
    ) -> Result<CounterfactualOutcome>;

    /// Emit the full structured causal report for a run.
    /// `spec_id` is used to compute cross-run causal streaks for this spec.
    async fn emit_report(&self, run_id: &str, spec_id: &str) -> Result<CausalReport>;
}

// ── Report ────────────────────────────────────────────────────────────────────

/// The structured output Coobie emits for every failed or degraded run.
/// Consumed by the orchestrator, Pack Board UI, and human reviewers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalReport {
    pub run_id: String,
    pub primary_cause: Option<String>,
    pub primary_confidence: f32,
    pub contributing_causes: Vec<String>,
    pub recommended_interventions: Vec<InterventionPlan>,
    pub counterfactual_prediction: Option<CounterfactualOutcome>,
    pub episode_scores: EpisodeScores,
    #[serde(default)]
    pub deep_causality: Option<DeepCausalityAnalysis>,
    /// Causes that have fired on ≥ 2 consecutive runs of the same spec.
    /// Populated when spec_id is available at report time.
    #[serde(default)]
    pub streaks: Vec<CausalStreak>,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepCausalitySignal {
    pub cause_id: String,
    pub question: String,
    pub observation: f64,
    pub threshold: f64,
    pub effect: f64,
    pub target: f64,
    pub activated: bool,
    pub activation_strength: f32,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepCausalityAnalysis {
    pub effect_score: f64,
    pub active_signal_count: usize,
    pub active_signal_percent: f64,
    pub active_signals: Vec<DeepCausalitySignal>,
    pub inactive_signals: Vec<DeepCausalitySignal>,
}

// ── Episode scoring ───────────────────────────────────────────────────────────

/// Five normalised scores (0.0–1.0) derived from a FactoryEpisode.
/// Higher is better for all dimensions except change_scope_score (lower = safer).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeScores {
    pub run_id: String,
    /// How clear and complete the spec was (criteria, failure behaviors, outputs).
    pub spec_clarity_score: f32,
    /// Breadth of implementation scope — higher means broader/riskier changes.
    pub change_scope_score: f32,
    /// How faithfully the twin environment represented production conditions.
    pub twin_fidelity_score: f32,
    /// Fraction of visible validation checks that passed.
    pub test_coverage_score: f32,
    /// Whether Coobie retrieved relevant prior memory before the run.
    pub memory_retrieval_score: f32,
    /// Fraction of attributed Labrador phases that completed successfully.
    pub phase_success_score: f32,
    pub scenario_passed: bool,
    pub validation_passed: bool,
}

// ── Heuristic causal rule ─────────────────────────────────────────────────────

/// A single causal heuristic rule evaluated against EpisodeScores.
struct CausalRule {
    /// Short stable identifier, used as cause_id in CausalHypothesis.
    id: &'static str,
    description: &'static str,
    /// The concrete intervention this rule recommends if triggered.
    intervention_target: &'static str,
    intervention_action: &'static str,
    intervention_impact: &'static str,
    /// Returns Some(base_confidence) if this rule fires, None if it does not.
    evaluate: fn(&EpisodeScores) -> Option<f32>,
}

/// All Phase 1 heuristic rules.
/// Narrow initial domain: internal-pass / scenario-fail splits.
const CAUSAL_RULES: &[CausalRule] = &[
    CausalRule {
        id: "SPEC_AMBIGUITY",
        description: "Ambiguous or incomplete spec likely caused scenario failure — \
                       acceptance criteria, failure behaviors, or explicit outputs were missing.",
        intervention_target: "spec",
        intervention_action: "Add explicit failure behaviors, edge-case acceptance criteria, \
                              and concrete output examples to the spec before next run.",
        intervention_impact: "Estimated +0.25 scenario pass probability based on similar runs.",
        evaluate: |s| {
            if s.spec_clarity_score < 0.4 && !s.scenario_passed {
                Some(0.7 * (1.0 - s.spec_clarity_score))
            } else if s.spec_clarity_score < 0.55 && !s.scenario_passed {
                Some(0.4 * (1.0 - s.spec_clarity_score))
            } else {
                None
            }
        },
    },
    CausalRule {
        id: "TWIN_GAP",
        description: "Low twin fidelity may have caused false-negative hidden scenario failure — \
                       the simulated environment did not cover production conditions.",
        intervention_target: "twin",
        intervention_action: "Increase twin service coverage: add auth token expiry simulation, \
                              third-party dependency stubs, and error-injection paths.",
        intervention_impact: "Estimated +0.20 scenario pass probability for auth/API features.",
        evaluate: |s| {
            if s.twin_fidelity_score < 0.4 && !s.scenario_passed {
                Some(0.65 * (1.0 - s.twin_fidelity_score))
            } else {
                None
            }
        },
    },
    CausalRule {
        id: "TEST_BLIND_SPOT",
        description: "Visible tests all passed but hidden scenarios failed — tests were \
                       too aligned with the happy path and did not cover failure modes.",
        intervention_target: "validation",
        intervention_action: "Before scenario evaluation, generate at least one failure-path \
                              test (expired token, invalid input, permission boundary).",
        intervention_impact: "Estimated +0.30 scenario pass probability; reduces false pass rate.",
        evaluate: |s| {
            if s.test_coverage_score >= 0.9 && !s.scenario_passed {
                Some(0.80)
            } else if s.test_coverage_score >= 0.75 && !s.scenario_passed {
                Some(0.55)
            } else {
                None
            }
        },
    },
    CausalRule {
        id: "NO_PRIOR_MEMORY",
        description: "No relevant prior memory was retrieved before the run — \
                       the factory approached this pattern without prior context.",
        intervention_target: "memory",
        intervention_action: "Seed Coobie with prior similar runs or domain notes before \
                              retrying. Run: harkonnen memory import <prior-context-file>.",
        intervention_impact: "Pattern-cold runs have lower first-pass acceptance rates.",
        evaluate: |s| {
            if s.memory_retrieval_score < 0.1 {
                Some(0.55)
            } else {
                None
            }
        },
    },
    CausalRule {
        id: "BROAD_SCOPE",
        description: "Broad implementation scope (many agent phases active, wide file changes) \
                       increases the probability of hidden scenario failures.",
        intervention_target: "spec",
        intervention_action: "Narrow the spec scope: break into smaller deliverables with \
                              tighter acceptance criteria per run.",
        intervention_impact: "Estimated -0.15 failure probability per scope reduction step.",
        evaluate: |s| {
            if s.change_scope_score > 0.75 {
                Some(0.45 * s.change_scope_score)
            } else {
                None
            }
        },
    },
    CausalRule {
        id: "PACK_BREAKDOWN",
        description: "One or more Labrador phases failed or degraded before the run stabilized — \
                       the pack execution chain likely broke down before the product was sound.",
        intervention_target: "pack",
        intervention_action: "Inspect the failed phase attribution records, prompt bundles, provider route, \
                              and pinned skills; rerun the weakest Labrador phase before attempting a broad retry.",
        intervention_impact: "Improves retries by targeting the weakest phase instead of repeating the full run blindly.",
        evaluate: |s| {
            if (!s.validation_passed || !s.scenario_passed) && s.phase_success_score < 0.70 {
                Some((0.55 * (1.0 - s.phase_success_score)).max(0.20))
            } else if !s.scenario_passed && s.phase_success_score < 0.85 {
                Some((0.35 * (1.0 - s.phase_success_score)).max(0.10))
            } else {
                None
            }
        },
    },
];

struct DeepSignalSpec {
    cause_id: &'static str,
    question: &'static str,
    description: &'static str,
    threshold: f64,
    observe: fn(&EpisodeScores) -> f64,
    verify: fn(f64) -> Result<bool, CausalityError>,
}

const DEEP_SIGNAL_SPECS: &[DeepSignalSpec] = &[
    DeepSignalSpec {
        cause_id: "SPEC_AMBIGUITY",
        question: "Is spec ambiguity the most plausible driver of this run's outcome?",
        description: "Deep Causality signal for spec ambiguity.",
        threshold: 0.45,
        observe: spec_ambiguity_observation,
        verify: spec_ambiguity_causality,
    },
    DeepSignalSpec {
        cause_id: "TWIN_GAP",
        question: "Did the twin environment under-represent production behavior?",
        description: "Deep Causality signal for twin fidelity gaps.",
        threshold: 0.40,
        observe: twin_gap_observation,
        verify: twin_gap_causality,
    },
    DeepSignalSpec {
        cause_id: "TEST_BLIND_SPOT",
        question: "Did visible validation overfit the happy path?",
        description: "Deep Causality signal for visible-test blind spots.",
        threshold: 0.75,
        observe: test_blind_spot_observation,
        verify: test_blind_spot_causality,
    },
    DeepSignalSpec {
        cause_id: "NO_PRIOR_MEMORY",
        question: "Did the run proceed without enough prior memory context?",
        description: "Deep Causality signal for missing prior memory.",
        threshold: 0.90,
        observe: no_prior_memory_observation,
        verify: no_prior_memory_causality,
    },
    DeepSignalSpec {
        cause_id: "BROAD_SCOPE",
        question: "Was the implementation scope broad enough to raise hidden risk?",
        description: "Deep Causality signal for overly broad scope.",
        threshold: 0.75,
        observe: broad_scope_observation,
        verify: broad_scope_causality,
    },
    DeepSignalSpec {
        cause_id: "PACK_BREAKDOWN",
        question:
            "Did Labrador phase failures indicate the pack broke down before the run stabilized?",
        description: "Deep Causality signal for Labrador phase breakdown.",
        threshold: 0.35,
        observe: phase_breakdown_observation,
        verify: phase_breakdown_causality,
    },
];

fn threshold_causality(obs: f64, threshold: f64) -> Result<bool, CausalityError> {
    if obs.is_nan() {
        return Err(CausalityError("Observation is NULL/NAN".into()));
    }
    Ok(obs >= threshold)
}

fn spec_ambiguity_causality(obs: f64) -> Result<bool, CausalityError> {
    threshold_causality(obs, 0.45)
}

fn twin_gap_causality(obs: f64) -> Result<bool, CausalityError> {
    threshold_causality(obs, 0.40)
}

fn test_blind_spot_causality(obs: f64) -> Result<bool, CausalityError> {
    threshold_causality(obs, 0.75)
}

fn no_prior_memory_causality(obs: f64) -> Result<bool, CausalityError> {
    threshold_causality(obs, 0.90)
}

fn broad_scope_causality(obs: f64) -> Result<bool, CausalityError> {
    threshold_causality(obs, 0.75)
}

fn phase_breakdown_causality(obs: f64) -> Result<bool, CausalityError> {
    threshold_causality(obs, 0.35)
}

fn spec_ambiguity_observation(scores: &EpisodeScores) -> f64 {
    (1.0 - scores.spec_clarity_score as f64).clamp(0.0, 1.0)
}

fn twin_gap_observation(scores: &EpisodeScores) -> f64 {
    (1.0 - scores.twin_fidelity_score as f64).clamp(0.0, 1.0)
}

fn test_blind_spot_observation(scores: &EpisodeScores) -> f64 {
    if scores.validation_passed && !scores.scenario_passed {
        (scores.test_coverage_score as f64).clamp(0.0, 1.0)
    } else if !scores.scenario_passed {
        ((scores.test_coverage_score as f64) * 0.85).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn no_prior_memory_observation(scores: &EpisodeScores) -> f64 {
    (1.0 - scores.memory_retrieval_score as f64).clamp(0.0, 1.0)
}

fn broad_scope_observation(scores: &EpisodeScores) -> f64 {
    (scores.change_scope_score as f64).clamp(0.0, 1.0)
}

fn phase_breakdown_observation(scores: &EpisodeScores) -> f64 {
    (1.0 - scores.phase_success_score as f64).clamp(0.0, 1.0)
}

fn run_effect_score(scores: &EpisodeScores) -> f64 {
    match (scores.validation_passed, scores.scenario_passed) {
        (true, false) => 1.0,
        (false, false) => 0.9,
        (false, true) => 0.45,
        (true, true) => 0.1,
    }
}

fn deep_signal_id(cause_id: &str) -> u64 {
    cause_id.bytes().fold(17_u64, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u64 + 1)
    })
}

fn deep_confidence(signal: &DeepCausalitySignal) -> f32 {
    if signal.activated && signal.effect >= 0.45 {
        (0.35 + signal.activation_strength * 0.45 + (signal.effect as f32 * 0.10)).min(0.90)
    } else {
        0.0
    }
}

fn sort_signals(signals: &mut [DeepCausalitySignal]) {
    signals.sort_by(|left, right| {
        right
            .activation_strength
            .partial_cmp(&left.activation_strength)
            .unwrap_or(Ordering::Equal)
            .then_with(|| {
                right
                    .observation
                    .partial_cmp(&left.observation)
                    .unwrap_or(Ordering::Equal)
            })
    });
}

fn build_deep_signal(spec: &DeepSignalSpec, scores: &EpisodeScores) -> DeepCausalitySignal {
    let effect_score = run_effect_score(scores);
    let cause_id = deep_signal_id(spec.cause_id);
    let observation_value = (spec.observe)(scores);
    let observation = Observation::new(cause_id, observation_value, effect_score);
    let inference = Inference::new(
        cause_id,
        spec.question.to_string(),
        observation.observation(),
        spec.threshold,
        observation.observed_effect(),
        1.0,
    );
    let causaloid: BaseCausaloid<'static> = Causaloid::new(cause_id, spec.verify, spec.description);
    let activated = causaloid
        .verify_single_cause(&inference.observation())
        .unwrap_or(false);
    let activation_strength = if activated {
        ((inference.observation() - inference.threshold())
            / (inference.target() - inference.threshold()).max(0.0001))
        .clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    let explanation = causaloid.explain().unwrap_or_else(|_| {
        format!(
            "Causaloid {} remained inactive at observation {:.2} against threshold {:.2}",
            spec.cause_id,
            inference.observation(),
            inference.threshold()
        )
    });

    DeepCausalitySignal {
        cause_id: spec.cause_id.to_string(),
        question: inference.question(),
        observation: inference.observation(),
        threshold: inference.threshold(),
        effect: inference.effect(),
        target: inference.target(),
        activated,
        activation_strength,
        explanation,
    }
}

fn build_deep_causality_analysis(scores: &EpisodeScores) -> DeepCausalityAnalysis {
    let mut active_signals = Vec::new();
    let mut inactive_signals = Vec::new();

    for spec in DEEP_SIGNAL_SPECS {
        let signal = build_deep_signal(spec, scores);
        if signal.activated {
            active_signals.push(signal);
        } else {
            inactive_signals.push(signal);
        }
    }

    sort_signals(&mut active_signals);
    sort_signals(&mut inactive_signals);

    let active_signal_count = active_signals.len();
    let total_signals = active_signal_count + inactive_signals.len();
    let active_signal_percent = if total_signals == 0 {
        0.0
    } else {
        (active_signal_count as f64 / total_signals as f64) * 100.0
    };

    DeepCausalityAnalysis {
        effect_score: run_effect_score(scores),
        active_signal_count,
        active_signal_percent,
        active_signals,
        inactive_signals,
    }
}

fn render_bullet_lines(items: &[String], empty_line: &str) -> String {
    if items.is_empty() {
        format!("- {}", empty_line)
    } else {
        items
            .iter()
            .map(|item| format!("- {}", item))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    }
}

fn render_component_lines(components: &[ProjectComponent]) -> String {
    let lines = components
        .iter()
        .map(|component| {
            let mut details = vec![format!("role={}", fallback_value(&component.role))];
            details.push(format!("kind={}", fallback_value(&component.kind)));
            details.push(format!("path={}", component.path));
            if !component.owner.trim().is_empty() {
                details.push(format!("owner={}", component.owner.trim()));
            }
            if !component.interfaces.is_empty() {
                details.push(format!("interfaces={}", component.interfaces.join(", ")));
            }
            if !component.notes.is_empty() {
                details.push(format!("notes={}", component.notes.join(" | ")));
            }
            format!("{} -> {}", component.name, details.join("; "))
        })
        .collect::<Vec<_>>();
    render_bullet_lines(&lines, "No project components were declared yet.")
}

fn render_blueprint_lines(blueprint: Option<&ScenarioBlueprint>) -> String {
    let Some(blueprint) = blueprint else {
        return "- No explicit scenario blueprint was declared yet.".to_string();
    };

    let mut lines = Vec::new();
    if !blueprint.pattern.trim().is_empty() {
        lines.push(format!("pattern={}", blueprint.pattern.trim()));
    }
    if !blueprint.objective.trim().is_empty() {
        lines.push(format!("objective={}", blueprint.objective.trim()));
    }
    if !blueprint.code_under_test.is_empty() {
        lines.push(format!(
            "code_under_test={}",
            blueprint.code_under_test.join(", ")
        ));
    }
    if !blueprint.hidden_oracles.is_empty() {
        lines.push(format!(
            "hidden_oracles={}",
            blueprint.hidden_oracles.join(", ")
        ));
    }
    if !blueprint.datasets.is_empty() {
        lines.push(format!("datasets={}", blueprint.datasets.join(", ")));
    }
    if !blueprint.runtime_surfaces.is_empty() {
        lines.push(format!(
            "runtime_surfaces={}",
            blueprint.runtime_surfaces.join(", ")
        ));
    }
    if !blueprint.coobie_memory_topics.is_empty() {
        lines.push(format!(
            "coobie_memory_topics={}",
            blueprint.coobie_memory_topics.join(", ")
        ));
    }
    if !blueprint.required_artifacts.is_empty() {
        lines.push(format!(
            "required_artifacts={}",
            blueprint.required_artifacts.join(", ")
        ));
    }
    render_bullet_lines(&lines, "No explicit scenario blueprint was declared yet.")
}

fn fallback_value(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unspecified"
    } else {
        trimmed
    }
}

fn render_citations(citations: &[CoobieEvidenceCitation], empty_line: &str) -> String {
    if citations.is_empty() {
        return format!("- {}", empty_line);
    }

    citations
        .iter()
        .map(|citation| {
            format!(
                "- [{}] {}\n  run: {}\n  phase/agent: {}/{}\n  evidence: {}",
                citation.citation_id,
                citation.summary,
                citation.run_id,
                citation.phase,
                citation.agent,
                citation.evidence
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_resume_risks(risks: &[ProjectResumeRisk]) -> String {
    if risks.is_empty() {
        return "- No project-memory entries are currently flagged as stale or contradicted."
            .to_string();
    }

    risks
        .iter()
        .map(|risk| {
            format!(
                "- {} [{} | severity={} score={}]\n  reasons: {}",
                risk.memory_id,
                risk.status.clone().unwrap_or_else(|| "review".to_string()),
                risk.severity,
                risk.severity_score,
                if risk.reasons.is_empty() {
                    "no explicit reasons recorded".to_string()
                } else {
                    risk.reasons.join(" | ")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_relevant_lessons(lessons: &[LessonRecord]) -> String {
    if lessons.is_empty() {
        return "- No distilled lessons were elevated for this run yet.".to_string();
    }

    lessons
        .iter()
        .map(|lesson| {
            let intervention = lesson
                .intervention
                .as_deref()
                .unwrap_or("No intervention has been recorded yet.");
            format!(
                "- {}\n  intervention: {}\n  tags: {}",
                lesson.pattern,
                intervention,
                lesson.tags.join(", ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn render_coobie_briefing_response(briefing: &CoobieBriefing) -> String {
    let cause_lines = if briefing.prior_causes.is_empty() {
        "- No prior causal reports matched strongly enough to summarize yet.".to_string()
    } else {
        briefing
            .prior_causes
            .iter()
            .map(|cause| {
                format!(
                    "- {}: {} occurrence(s), {:.0}% scenario pass rate in prior runs",
                    cause.cause_id,
                    cause.occurrences,
                    cause.scenario_pass_rate * 100.0,
                )
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };

    let pidgin = crate::pidgin::coobie_briefing_pidgin(briefing);
    let project_memory_root = briefing
        .project_memory_root
        .clone()
        .unwrap_or_else(|| "not recorded".to_string());

    format!(
        "# Coobie Preflight Response

{}

I reviewed prior memory and causal history for `{}` targeting `{}`.

## Memory Sources
- Project memory root: {}
- Project memory hits: {}
- Core memory hits: {}

## Resume Packet Summary
{}

## Project Memory At Risk
{}

## Stale Memory Mitigation Plan
{}

## Project Memory Context
{}

## Core Memory Context
{}

## Domain Signals
{}

## Project Components
{}

## Scenario Blueprint
{}

## Application Risks
{}

## Environment Risks
{}

## Regulatory Considerations
{}

## Prior Causes Worth Respecting
{}

## Distilled Lessons To Apply
{}

## Exploration Evidence Coobie Is Citing
{}

## Strategy Register Evidence Coobie Is Citing
{}

## Prior Mitigation Outcomes Coobie Is Citing
{}

## Pattern Exemplars Coobie Is Citing
{}

## Causal Exemplars Coobie Is Citing
{}

## Nearest Reviewed Evidence Windows
{}

## Pattern Matching Focus
{}

## Causal Chains Coobie Wants Probed
{}

## Retriever Forge Evidence Coobie Is Citing
{}

## Preferred Forge Outcomes Coobie Is Citing
{}

## Preferred Retriever Forge Commands
{}

## Guardrails I Want The Pack To Follow
{}

## Required Checks
{}

## Open Questions
{}
",
        pidgin,
        briefing.spec_id,
        briefing.product,
        project_memory_root,
        briefing.project_memory_hits.len(),
        briefing.core_memory_hits.len(),
        render_bullet_lines(
            &briefing.resume_packet_summary,
            "No resume packet summary was generated yet.",
        ),
        render_resume_risks(&briefing.resume_packet_risks),
        render_bullet_lines(
            &briefing.stale_memory_mitigation_plan,
            "No stale-memory mitigation steps were generated.",
        ),
        render_bullet_lines(
            &briefing.project_memory_hits,
            "No project-local memory hits were retrieved yet.",
        ),
        render_bullet_lines(
            &briefing.core_memory_hits,
            "No Harkonnen core memory hits were retrieved yet.",
        ),
        render_bullet_lines(
            &briefing.domain_signals,
            "No domain signals were detected yet."
        ),
        render_component_lines(&briefing.project_components),
        render_blueprint_lines(briefing.scenario_blueprint.as_ref()),
        render_bullet_lines(
            &briefing.application_risks,
            "No application risks were highlighted yet."
        ),
        render_bullet_lines(
            &briefing.environment_risks,
            "No environment risks were highlighted yet."
        ),
        render_bullet_lines(
            &briefing.regulatory_considerations,
            "No explicit regulatory considerations were recorded yet.",
        ),
        cause_lines,
        render_relevant_lessons(&briefing.relevant_lessons),
        render_citations(
            &briefing.exploration_citations,
            "No prior exploration-log entries were elevated for this run yet.",
        ),
        render_citations(
            &briefing.strategy_register_citations,
            "No strategy-register entries were elevated for this run yet.",
        ),
        render_citations(
            &briefing.mitigation_history_citations,
            "No prior stale-memory mitigation outcomes were elevated for this run yet.",
        ),
        render_citations(
            &briefing.evidence_pattern_exemplar_citations,
            "No pattern exemplars from promoted evidence memory were elevated for this run yet.",
        ),
        render_citations(
            &briefing.evidence_causal_exemplar_citations,
            "No causal exemplars from promoted evidence memory were elevated for this run yet.",
        ),
        render_citations(
            &briefing.nearest_evidence_window_citations,
            "No reviewed evidence windows were retrieved from project annotation bundles yet.",
        ),
        render_bullet_lines(
            &briefing.pattern_matching_focus,
            "No pattern-matching focus was derived from promoted evidence exemplars yet.",
        ),
        render_bullet_lines(
            &briefing.causal_chain_focus,
            "No causal-chain focus was derived from promoted evidence exemplars yet.",
        ),
        render_citations(
            &briefing.forge_evidence_citations,
            "No prior retriever-forge evidence was elevated for this run yet.",
        ),
        render_citations(
            &briefing.preferred_forge_outcome_citations,
            "No prior preferred-command outcomes were elevated for this run yet.",
        ),
        render_bullet_lines(
            &briefing.preferred_forge_commands,
            "No preferred retriever-forge commands were recovered from prior successful runs yet.",
        ),
        render_bullet_lines(
            &briefing.recommended_guardrails,
            "No extra guardrails were generated yet.",
        ),
        render_bullet_lines(
            &briefing.required_checks,
            "No required checks were generated yet."
        ),
        render_bullet_lines(&briefing.open_questions, "No open questions were raised."),
    )
}

pub fn render_coobie_report_response(report: &CausalReport) -> String {
    let primary = report
        .primary_cause
        .clone()
        .unwrap_or_else(|| "No dominant cause was identified.".to_string());
    let contributing = if report.contributing_causes.is_empty() {
        "- No secondary causes were elevated.".to_string()
    } else {
        report
            .contributing_causes
            .iter()
            .map(|cause| format!("- {}", cause))
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    let interventions = if report.recommended_interventions.is_empty() {
        "- No concrete interventions were recommended.".to_string()
    } else {
        report
            .recommended_interventions
            .iter()
            .map(|plan| {
                format!(
                    "- [{}] {} -> {}",
                    plan.target, plan.action, plan.expected_impact
                )
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            )
    };
    let deep_signals = report
        .deep_causality
        .as_ref()
        .map(|deep| {
            if deep.active_signals.is_empty() {
                "- No DeepCausality signals activated for this run.".to_string()
            } else {
                deep.active_signals
                    .iter()
                    .take(5)
                    .map(|signal| {
                        format!(
                            "- {} activated at {:.0}% strength (obs {:.2} vs threshold {:.2})",
                            signal.cause_id,
                            signal.activation_strength * 100.0,
                            signal.observation,
                            signal.threshold,
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(
                        "
",
                    )
            }
        })
        .unwrap_or_else(|| "- DeepCausality analysis was not available.".to_string());
    let counterfactual = report
        .counterfactual_prediction
        .as_ref()
        .map(|prediction| {
            format!(
                "{} (confidence gain {:.0}%)",
                prediction.prediction,
                prediction.confidence_gain * 100.0,
            )
        })
        .unwrap_or_else(|| "No counterfactual prediction was available.".to_string());

    let streaks_section = if report.streaks.is_empty() {
        "- No recurring cause streaks detected.".to_string()
    } else {
        report
            .streaks
            .iter()
            .map(|s| {
                if s.escalate {
                    format!(
                        "- ⚠ {} has fired {} consecutive times — ESCALATE to Scout for spec rework",
                        s.cause_id, s.streak_len
                    )
                } else {
                    format!(
                        "- {} has fired {} consecutive times (+{:.0}% confidence boost)",
                        s.cause_id,
                        s.streak_len,
                        s.confidence_boost * 100.0,
                    )
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let pidgin = crate::pidgin::coobie_report_pidgin(report);

    format!(
        "# Coobie Run Response

{}

I completed a causal review for run `{}`.

## Primary Cause
- {}
- Confidence: {:.0}%

## Contributing Causes
{}

## Recurring Streaks
{}

## Recommended Interventions
{}

## Deep Signals
{}

## Counterfactual
- {}
",
        pidgin,
        report.run_id,
        primary,
        report.primary_confidence * 100.0,
        contributing,
        streaks_section,
        interventions,
        deep_signals,
        counterfactual,
    )
}

// ── SQLite-backed engine ──────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SqliteCoobie {
    pool: SqlitePool,
}

impl SqliteCoobie {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ── Scoring ───────────────────────────────────────────────────────────────

    fn score_episode(ep: &FactoryEpisode) -> EpisodeScores {
        let max_required_checks = ep
            .phase_attributions
            .iter()
            .map(|record| record.required_checks.len())
            .max()
            .unwrap_or(0);

        // spec_clarity_score: presence of key spec fields plus whether Coobie surfaced checks.
        let spec_clarity = {
            let mut score: f32 = 0.0;
            let n = ep.features.len();
            score += (n as f32 * 0.12).min(0.6);
            if let Some(v) = &ep.validation {
                if !v.results.is_empty() {
                    score += 0.2;
                }
            }
            if ep.decision.is_some() {
                score += 0.2;
            }
            if max_required_checks > 0 {
                score += ((max_required_checks.min(4) as f32) * 0.05).min(0.2);
            }
            score.min(1.0)
        };

        let change_scope = {
            let event_breadth = (ep.agent_events.len() as f32 / 10.0).min(1.0);
            let attributed_phase_breadth = if ep.phase_attributions.is_empty() {
                0.0
            } else {
                let phase_count = ep
                    .phase_attributions
                    .iter()
                    .map(|record| record.phase.as_str())
                    .collect::<HashSet<_>>()
                    .len() as f32;
                (phase_count / 8.0).min(1.0)
            };
            if ep.phase_attributions.is_empty() {
                event_breadth
            } else {
                ((attributed_phase_breadth * 0.7) + (event_breadth * 0.3)).min(1.0)
            }
        };

        let twin_fidelity = match &ep.twin_env {
            None => 0.1,
            Some(twin) => {
                let total = twin.services.len() as f32;
                if total == 0.0 {
                    return EpisodeScores {
                        run_id: ep.run_id.clone(),
                        spec_clarity_score: spec_clarity,
                        change_scope_score: change_scope,
                        twin_fidelity_score: 0.1,
                        test_coverage_score: 0.0,
                        memory_retrieval_score: 0.0,
                        phase_success_score: if ep.phase_attributions.is_empty() {
                            0.0
                        } else {
                            ep.phase_attributions
                                .iter()
                                .filter(|record| record.outcome == "success")
                                .count() as f32
                                / ep.phase_attributions.len() as f32
                        },
                        scenario_passed: ep.scenarios.as_ref().map(|s| s.passed).unwrap_or(false),
                        validation_passed: ep
                            .validation
                            .as_ref()
                            .map(|v| v.passed)
                            .unwrap_or(false),
                    };
                }
                let ready = twin
                    .services
                    .iter()
                    .filter(|s| s.status == "ready" || s.status == "running")
                    .count() as f32;
                (ready / total).min(1.0)
            }
        };

        let test_coverage = match &ep.validation {
            None => 0.0,
            Some(v) => {
                if v.results.is_empty() {
                    0.0
                } else {
                    let passed = v.results.iter().filter(|r| r.passed).count() as f32;
                    passed / v.results.len() as f32
                }
            }
        };

        let memory_score = if ep.phase_attributions.is_empty() {
            let has_memory_phase = ep
                .agent_events
                .iter()
                .any(|e| e.phase == "memory" && e.status == "complete");
            if has_memory_phase {
                1.0
            } else {
                0.0
            }
        } else {
            let mut supporting_ids = HashSet::new();
            let mut has_query_context = false;
            for record in &ep.phase_attributions {
                for id in record
                    .project_memory_ids
                    .iter()
                    .chain(record.core_memory_ids.iter())
                    .chain(record.relevant_lesson_ids.iter())
                {
                    supporting_ids.insert(id.clone());
                }
                if !record.query_terms.is_empty() {
                    has_query_context = true;
                }
            }
            if !supporting_ids.is_empty() {
                (0.35 + supporting_ids.len() as f32 * 0.1).min(1.0)
            } else if has_query_context {
                0.6
            } else {
                0.0
            }
        };

        let phase_success_score = if ep.phase_attributions.is_empty() {
            match (
                ep.validation.as_ref().map(|v| v.passed).unwrap_or(false),
                ep.scenarios.as_ref().map(|s| s.passed).unwrap_or(false),
            ) {
                (true, true) => 1.0,
                (false, false) if ep.agent_events.is_empty() => 0.0,
                (true, false) => 0.7,
                (false, true) => 0.75,
                (false, false) => 0.4,
            }
        } else {
            ep.phase_attributions
                .iter()
                .filter(|record| record.outcome == "success")
                .count() as f32
                / ep.phase_attributions.len() as f32
        };

        EpisodeScores {
            run_id: ep.run_id.clone(),
            spec_clarity_score: spec_clarity,
            change_scope_score: change_scope,
            twin_fidelity_score: twin_fidelity,
            test_coverage_score: test_coverage,
            memory_retrieval_score: memory_score,
            phase_success_score,
            scenario_passed: ep.scenarios.as_ref().map(|s| s.passed).unwrap_or(false),
            validation_passed: ep.validation.as_ref().map(|v| v.passed).unwrap_or(false),
        }
    }

    // ── Persistence ───────────────────────────────────────────────────────────

    async fn upsert_scores(&self, scores: &EpisodeScores) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO coobie_episode_scores
                (run_id, spec_clarity_score, change_scope_score, twin_fidelity_score,
                 test_coverage_score, memory_retrieval_score, phase_success_score,
                 scenario_passed, validation_passed, human_accepted, scored_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, ?10)
            ON CONFLICT(run_id) DO UPDATE SET
                spec_clarity_score      = excluded.spec_clarity_score,
                change_scope_score      = excluded.change_scope_score,
                twin_fidelity_score     = excluded.twin_fidelity_score,
                test_coverage_score     = excluded.test_coverage_score,
                memory_retrieval_score  = excluded.memory_retrieval_score,
                phase_success_score     = excluded.phase_success_score,
                scenario_passed         = excluded.scenario_passed,
                validation_passed       = excluded.validation_passed,
                scored_at               = excluded.scored_at
            "#,
        )
        .bind(&scores.run_id)
        .bind(scores.spec_clarity_score as f64)
        .bind(scores.change_scope_score as f64)
        .bind(scores.twin_fidelity_score as f64)
        .bind(scores.test_coverage_score as f64)
        .bind(scores.memory_retrieval_score as f64)
        .bind(scores.phase_success_score as f64)
        .bind(scores.scenario_passed as i64)
        .bind(scores.validation_passed as i64)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await
        .context("upserting episode scores")?;
        Ok(())
    }

    async fn persist_hypotheses(
        &self,
        run_id: &str,
        hypotheses: &[CausalHypothesis],
    ) -> Result<()> {
        for h in hypotheses {
            let supporting = serde_json::to_string(&h.supporting_runs)?;
            let counterfactuals = serde_json::to_string(&h.counterfactuals)?;
            sqlx::query(
                r#"
                INSERT OR REPLACE INTO causal_hypotheses
                    (hypothesis_id, run_id, cause_id, description, confidence,
                     supporting_runs, counterfactuals, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            )
            .bind(Uuid::new_v4().to_string())
            .bind(run_id)
            .bind(&h.cause_id)
            .bind(&h.description)
            .bind(h.confidence as f64)
            .bind(&supporting)
            .bind(&counterfactuals)
            .bind(Utc::now().to_rfc3339())
            .execute(&self.pool)
            .await
            .context("persisting hypothesis")?;
        }
        Ok(())
    }

    async fn load_scores(&self, run_id: &str) -> Result<Option<EpisodeScores>> {
        let row = sqlx::query(
            r#"
            SELECT run_id, spec_clarity_score, change_scope_score, twin_fidelity_score,
                   test_coverage_score, memory_retrieval_score, phase_success_score,
                   scenario_passed, validation_passed
            FROM coobie_episode_scores WHERE run_id = ?1
            "#,
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await
        .context("loading episode scores")?;

        Ok(row.map(|r| {
            use sqlx::Row;
            EpisodeScores {
                run_id: r.get("run_id"),
                spec_clarity_score: r.get::<f64, _>("spec_clarity_score") as f32,
                change_scope_score: r.get::<f64, _>("change_scope_score") as f32,
                twin_fidelity_score: r.get::<f64, _>("twin_fidelity_score") as f32,
                test_coverage_score: r.get::<f64, _>("test_coverage_score") as f32,
                memory_retrieval_score: r.get::<f64, _>("memory_retrieval_score") as f32,
                phase_success_score: r.get::<f64, _>("phase_success_score") as f32,
                scenario_passed: r.get::<i64, _>("scenario_passed") != 0,
                validation_passed: r.get::<i64, _>("validation_passed") != 0,
            }
        }))
    }

    /// Find prior episodes where the same rule fired AND the scenario later passed.
    /// Used for counterfactual confidence estimation.
    async fn find_supporting_runs(&self, cause_id: &str, limit: i64) -> Result<Vec<String>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT h.run_id
            FROM causal_hypotheses h
            JOIN coobie_episode_scores s ON s.run_id = h.run_id
            WHERE h.cause_id = ?1 AND s.scenario_passed = 1
            ORDER BY h.created_at DESC
            LIMIT ?2
            "#,
        )
        .bind(cause_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .context("finding supporting runs")?;

        use sqlx::Row;
        Ok(rows.iter().map(|r| r.get::<String, _>("run_id")).collect())
    }

    /// Count how many consecutive recent runs of `spec_id` had `cause_id` fire,
    /// starting from the most recent run. Returns 0 if the cause has never fired.
    ///
    /// "Consecutive" means looking at the last `window` runs for this spec
    /// (ordered newest-first) and counting from the front while the cause fires.
    /// The streak resets as soon as a run is found where the cause did NOT fire.
    async fn detect_cause_streak(&self, spec_id: &str, cause_id: &str, window: i64) -> Result<usize> {
        // Ordered list of recent run_ids for this spec, newest first.
        let run_rows = sqlx::query(
            r#"
            SELECT r.run_id
            FROM runs r
            WHERE r.spec_id = ?1
            ORDER BY r.created_at DESC
            LIMIT ?2
            "#,
        )
        .bind(spec_id)
        .bind(window)
        .fetch_all(&self.pool)
        .await?;

        use sqlx::Row;
        let mut streak = 0usize;
        for row in &run_rows {
            let run_id: String = row.get("run_id");
            // Did this cause fire in this run?
            let fired: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM causal_hypotheses WHERE run_id = ?1 AND cause_id = ?2",
            )
            .bind(&run_id)
            .bind(cause_id)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0);

            if fired > 0 {
                streak += 1;
            } else {
                break; // streak is broken
            }
        }
        Ok(streak)
    }

    /// Compute streaks for all active causes in `hypotheses` for this `spec_id`.
    async fn compute_streaks(
        &self,
        spec_id: &str,
        hypotheses: &[CausalHypothesis],
    ) -> Vec<CausalStreak> {
        let mut streaks = Vec::new();
        for h in hypotheses {
            let streak_len = self
                .detect_cause_streak(spec_id, &h.cause_id, 10)
                .await
                .unwrap_or(0);
            if streak_len < 2 {
                continue;
            }
            // Confidence boost: +0.05 per extra run beyond 1, capped at +0.20.
            let confidence_boost = ((streak_len - 1) as f32 * 0.05).min(0.20);
            let escalate = streak_len >= 3;
            streaks.push(CausalStreak {
                cause_id: h.cause_id.clone(),
                streak_len,
                description: format!(
                    "'{}' has fired on {} consecutive runs of this spec{}.",
                    h.cause_id,
                    streak_len,
                    if escalate { " — standard interventions are not breaking the cycle" } else { "" },
                ),
                confidence_boost,
                escalate,
            });
        }
        streaks.sort_by(|a, b| b.streak_len.cmp(&a.streak_len));
        streaks
    }

    /// Estimate counterfactual outcome: what fraction of runs that had this
    /// cause_id diagnosed AND the intervention applied later passed scenarios?
    async fn counterfactual_estimate(
        &self,
        cause_id: &str,
        intervention: &InterventionPlan,
    ) -> Result<CounterfactualEstimate> {
        // Count runs where this cause was diagnosed
        let total: i64 = {
            let row =
                sqlx::query("SELECT COUNT(*) as cnt FROM causal_hypotheses WHERE cause_id = ?1")
                    .bind(cause_id)
                    .fetch_one(&self.pool)
                    .await?;
            use sqlx::Row;
            row.get("cnt")
        };

        // Count runs where this cause was diagnosed AND scenario passed (proxy for "intervention worked")
        let improved: i64 = {
            let row = sqlx::query(
                r#"
                SELECT COUNT(*) as cnt
                FROM causal_hypotheses h
                JOIN coobie_episode_scores s ON s.run_id = h.run_id
                WHERE h.cause_id = ?1 AND s.scenario_passed = 1
                "#,
            )
            .bind(cause_id)
            .fetch_one(&self.pool)
            .await?;
            use sqlx::Row;
            row.get("cnt")
        };

        let baseline_pass_rate = if total > 0 {
            improved as f32 / total as f32
        } else {
            0.0
        };
        // Assume intervention raises pass rate toward the model's built-in estimate
        let predicted_rate = (baseline_pass_rate + 0.35).min(0.95);

        Ok(CounterfactualEstimate {
            intervention: intervention.action.clone(),
            predicted_outcome: format!(
                "Scenario pass probability increases from {:.0}% to {:.0}% (based on {} prior episodes with this pattern)",
                baseline_pass_rate * 100.0,
                predicted_rate * 100.0,
                total,
            ),
            confidence: if total >= 3 { 0.70 } else if total >= 1 { 0.45 } else { 0.25 },
        })
    }
}

#[async_trait]
impl CoobieReasoner for SqliteCoobie {
    async fn ingest_episode(&self, ep: &FactoryEpisode) -> Result<()> {
        let scores = Self::score_episode(ep);
        self.upsert_scores(&scores).await?;

        // Immediately diagnose and persist hypotheses so they're queryable
        let hypotheses = self.diagnose(&ep.run_id).await?;
        self.persist_hypotheses(&ep.run_id, &hypotheses).await?;

        Ok(())
    }

    async fn diagnose(&self, run_id: &str) -> Result<Vec<CausalHypothesis>> {
        let scores = match self.load_scores(run_id).await? {
            Some(s) => s,
            None => return Ok(Vec::new()),
        };

        let deep_analysis = build_deep_causality_analysis(&scores);
        let deep_signals: HashMap<&str, &DeepCausalitySignal> = deep_analysis
            .active_signals
            .iter()
            .chain(deep_analysis.inactive_signals.iter())
            .map(|signal| (signal.cause_id.as_str(), signal))
            .collect();

        let mut hypotheses: Vec<CausalHypothesis> = Vec::new();

        for rule in CAUSAL_RULES {
            let heuristic_confidence = (rule.evaluate)(&scores).unwrap_or(0.0);
            let deep_signal = deep_signals.get(rule.id).copied();
            let deep_confidence_score = deep_signal.map(deep_confidence).unwrap_or(0.0);
            let base_confidence = heuristic_confidence.max(deep_confidence_score);
            if base_confidence <= 0.0 {
                continue;
            }

            let supporting = self
                .find_supporting_runs(rule.id, 10)
                .await
                .unwrap_or_default();
            let support_boost = (supporting.len() as f32 * 0.03).min(0.15);
            let final_confidence = (base_confidence + support_boost).min(0.95);
            let description = if heuristic_confidence <= 0.0 {
                if let Some(signal) = deep_signal {
                    format!(
                        "{} DeepCausality activated {} at {:.0}% strength from observation {:.2} against threshold {:.2}.",
                        rule.description,
                        signal.cause_id,
                        signal.activation_strength * 100.0,
                        signal.observation,
                        signal.threshold,
                    )
                } else {
                    rule.description.to_string()
                }
            } else {
                rule.description.to_string()
            };

            hypotheses.push(CausalHypothesis {
                cause_id: rule.id.to_string(),
                description,
                confidence: final_confidence,
                supporting_runs: supporting,
                counterfactuals: Vec::new(),
            });
        }

        hypotheses.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(Ordering::Equal)
        });

        Ok(hypotheses)
    }

    async fn recommend_interventions(&self, run_id: &str) -> Result<Vec<InterventionPlan>> {
        let hypotheses = self.diagnose(run_id).await?;

        // Map each hypothesis to its rule's intervention
        let plans: Vec<InterventionPlan> = CAUSAL_RULES
            .iter()
            .filter(|rule| {
                hypotheses
                    .iter()
                    .any(|h| h.cause_id == rule.id && h.confidence >= 0.4)
            })
            .map(|rule| InterventionPlan {
                target: rule.intervention_target.to_string(),
                action: rule.intervention_action.to_string(),
                expected_impact: rule.intervention_impact.to_string(),
            })
            .collect();

        Ok(plans)
    }

    async fn simulate_counterfactual(
        &self,
        run_id: &str,
        intervention: &InterventionPlan,
    ) -> Result<CounterfactualOutcome> {
        // Find which rule maps to this intervention target
        let cause_id = CAUSAL_RULES
            .iter()
            .find(|r| r.intervention_target == intervention.target)
            .map(|r| r.id)
            .unwrap_or("UNKNOWN");

        let estimate = self.counterfactual_estimate(cause_id, intervention).await?;

        let _ = run_id; // run_id available for future per-run baseline calculation
        Ok(CounterfactualOutcome {
            prediction: estimate.predicted_outcome,
            confidence_gain: estimate.confidence,
        })
    }

    async fn emit_report(&self, run_id: &str, spec_id: &str) -> Result<CausalReport> {
        let mut hypotheses = self.diagnose(run_id).await?;

        // ── Streak detection ──────────────────────────────────────────────────
        // Compute cross-run streaks before we build counterfactuals, so streak
        // boosts are reflected in the final confidence and intervention list.
        let streaks = self.compute_streaks(spec_id, &hypotheses).await;
        let streak_map: HashMap<&str, &CausalStreak> =
            streaks.iter().map(|s| (s.cause_id.as_str(), s)).collect();

        // Apply streak confidence boost and, for escalated causes, prepend a
        // Scout escalation description to the hypothesis.
        for h in &mut hypotheses {
            if let Some(streak) = streak_map.get(h.cause_id.as_str()) {
                h.confidence = (h.confidence + streak.confidence_boost).min(0.95);
                if streak.escalate {
                    h.description = format!(
                        "[ESCALATE — {} consecutive runs] {}",
                        streak.streak_len, h.description
                    );
                }
            }
        }

        // Re-sort after boosts (confidence may have shifted ranks).
        hypotheses.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut interventions = self.recommend_interventions(run_id).await?;

        // For any cause that has escalated, prepend a Scout escalation
        // intervention before the standard one.
        let mut escalation_plans: Vec<InterventionPlan> = streaks
            .iter()
            .filter(|s| s.escalate)
            .map(|s| InterventionPlan {
                target: "scout".to_string(),
                action: format!(
                    "Escalate '{}' to Scout for spec-level rework — this cause has fired on {} \
                     consecutive runs and standard interventions have not broken the cycle. \
                     Scout should re-examine acceptance criteria, scope, and hidden oracle \
                     assumptions from scratch.",
                    s.cause_id, s.streak_len
                ),
                expected_impact: format!(
                    "Breaking a {}-run streak requires structural spec changes, not incremental \
                     fixes. Scout rework estimated to reset failure cycle.",
                    s.streak_len
                ),
            })
            .collect();
        if !escalation_plans.is_empty() {
            escalation_plans.append(&mut interventions);
            interventions = escalation_plans;
        }

        for h in &mut hypotheses {
            if let Some(rule) = CAUSAL_RULES.iter().find(|r| r.id == h.cause_id) {
                let plan = InterventionPlan {
                    target: rule.intervention_target.to_string(),
                    action: rule.intervention_action.to_string(),
                    expected_impact: rule.intervention_impact.to_string(),
                };
                if let Ok(est) = self
                    .counterfactual_estimate(h.cause_id.as_str(), &plan)
                    .await
                {
                    h.counterfactuals = vec![est];
                }
            }
        }

        let primary = hypotheses.first();
        let primary_cause = primary.map(|h| h.description.clone());
        let primary_confidence = primary.map(|h| h.confidence).unwrap_or(0.0);

        let contributing: Vec<String> = hypotheses
            .iter()
            .skip(1)
            .take(3)
            .map(|h| h.description.clone())
            .collect();

        let best_intervention = interventions.first().cloned();
        let counterfactual = match &best_intervention {
            Some(plan) => self.simulate_counterfactual(run_id, plan).await.ok(),
            None => None,
        };

        let scores = self
            .load_scores(run_id)
            .await?
            .unwrap_or_else(|| EpisodeScores {
                run_id: run_id.to_string(),
                spec_clarity_score: 0.0,
                change_scope_score: 0.0,
                twin_fidelity_score: 0.0,
                test_coverage_score: 0.0,
                memory_retrieval_score: 0.0,
                phase_success_score: 0.0,
                scenario_passed: false,
                validation_passed: false,
            });
        let deep_causality = build_deep_causality_analysis(&scores);

        Ok(CausalReport {
            run_id: run_id.to_string(),
            primary_cause,
            primary_confidence,
            contributing_causes: contributing,
            recommended_interventions: interventions,
            counterfactual_prediction: counterfactual,
            episode_scores: scores,
            deep_causality: Some(deep_causality),
            streaks,
            generated_at: Utc::now().to_rfc3339(),
        })
    }
}

// ── Phase 2 stub ──────────────────────────────────────────────────────────────
//
// The next layer for Coobie is a contextual Deep Causality model: attach these
// signals to a real context hypergraph so WinCC OA and product-specific domain
// facts can influence causal reasoning without replacing the current heuristics.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deep_causality_activates_failure_signals() {
        let scores = EpisodeScores {
            run_id: "run-failure".to_string(),
            spec_clarity_score: 0.20,
            change_scope_score: 0.30,
            twin_fidelity_score: 0.80,
            test_coverage_score: 0.95,
            memory_retrieval_score: 0.0,
            phase_success_score: 0.40,
            scenario_passed: false,
            validation_passed: true,
        };

        let analysis = build_deep_causality_analysis(&scores);
        let active: Vec<&str> = analysis
            .active_signals
            .iter()
            .map(|signal| signal.cause_id.as_str())
            .collect();

        assert_eq!(analysis.effect_score, 1.0);
        assert!(active.contains(&"SPEC_AMBIGUITY"));
        assert!(active.contains(&"TEST_BLIND_SPOT"));
        assert!(active.contains(&"NO_PRIOR_MEMORY"));
        assert!(!active.contains(&"BROAD_SCOPE"));
    }

    #[test]
    fn deep_causality_stays_quiet_for_healthy_runs() {
        let scores = EpisodeScores {
            run_id: "run-healthy".to_string(),
            spec_clarity_score: 0.90,
            change_scope_score: 0.20,
            twin_fidelity_score: 0.95,
            test_coverage_score: 0.80,
            memory_retrieval_score: 1.0,
            phase_success_score: 1.0,
            scenario_passed: true,
            validation_passed: true,
        };

        let analysis = build_deep_causality_analysis(&scores);

        assert_eq!(analysis.effect_score, 0.1);
        assert_eq!(analysis.active_signal_count, 0);
        assert!(analysis.active_signals.is_empty());
    }
}
