//! Coobie Causal Tesseract — scene-graph builder.
//!
//! Computes a `TesseractScene` from recent factory runs and their CausalReports.
//! The frontend (`CausalTesseract.jsx`) fetches `/api/tesseract/scene` and renders
//! it directly — no projection math needed client-side.
//!
//! Coordinate convention
//! ─────────────────────
//! observed_position_3d  → outer cube (observed state space)
//!   x = spec_clarity_score        mapped to [-1, 1]
//!   y = twin_fidelity_score        mapped to [-1, 1]
//!   z = validation_outcome         mapped to [-1, 1]
//!
//! inferred_position_3d  → inner cube (Coobie's causal model)
//!   x = cause family axis          fixed per cause type
//!   y = confidence                 mapped to [-1, 1]
//!   z = intervention_potential     mapped to [-1, 1]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    coobie::{CausalReport, EpisodeScores},
    models::{InterventionPlan, RunRecord},
};

// ── Scene graph types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TesseractScene {
    pub episode_nodes: Vec<EpisodeTesseractNode>,
    pub cause_nodes: Vec<CauseTesseractNode>,
    pub intervention_nodes: Vec<()>,
    pub edges: Vec<TesseractEdge>,
    pub clusters: Vec<TesseractCluster>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeTesseractNode {
    pub id: String,
    pub label: String,
    pub product_id: String,
    pub run_id: String,

    // Semantic quality dimensions (0.0–1.0)
    pub spec_quality: f32,
    pub implementation_quality: f32,
    pub validation_outcome: f32,
    pub intervention_potential: f32,
    pub primary_cause_type: String,
    pub confidence: f32,

    // Display
    pub status: String,
    pub timestamp: String,

    // 3D positions for the renderer
    #[serde(rename = "observedPosition3D")]
    pub observed_position_3d: [f32; 3],
    #[serde(rename = "inferredPosition3D")]
    pub inferred_position_3d: [f32; 3],

    // Raw data for the detail panel
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_scores: Option<EpisodeScores>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary_cause_text: Option<String>,
    pub contributing_causes: Vec<String>,
    pub interventions: Vec<InterventionPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CauseTesseractNode {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub cause_type: String,
    pub confidence: f32,
    pub supporting_run_ids: Vec<String>,
    #[serde(rename = "position3D")]
    pub position_3d: [f32; 3],
    pub interventions: Vec<InterventionPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TesseractEdge {
    pub id: String,
    pub source_id: String,
    pub target_id: String,
    pub kind: String,
    pub confidence: f32,
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TesseractCluster {
    pub id: String,
    pub label: String,
    pub center: [f32; 3],
    pub radius: f32,
    pub color: String,
}

// ── Projection constants ──────────────────────────────────────────────────────

/// Cause type → X-axis position in inferred (inner) cube space.
fn cause_x(cause_type: &str) -> f32 {
    match cause_type {
        "spec_ambiguity" => -0.80,
        "missing_failure_case" => -0.40,
        "low_twin_fidelity" => 0.00,
        "test_blind_spot" => 0.40,
        "policy_block" => 0.80,
        "tool_misuse" => 0.20,
        "context_gap" => -0.20,
        _ => 0.00,
    }
}

/// Fixed inner-cube 3D position for each cause type.
fn cause_position(cause_type: &str) -> [f32; 3] {
    match cause_type {
        "spec_ambiguity" => [-0.65, 0.00, -0.30],
        "missing_failure_case" => [-0.30, -0.35, 0.20],
        "low_twin_fidelity" => [0.00, 0.10, 0.40],
        "test_blind_spot" => [0.35, 0.25, -0.10],
        "policy_block" => [0.65, -0.10, 0.05],
        "tool_misuse" => [0.15, -0.40, 0.30],
        "context_gap" => [-0.15, 0.40, -0.25],
        _ => [0.00, 0.00, 0.00],
    }
}

fn cluster_color(status: &str) -> &'static str {
    match status {
        "accepted" => "#8fae7c",
        "failed" => "#c7684c",
        "rejected" => "#c7684c",
        "in_progress" => "#c4922a",
        _ => "#67727b",
    }
}

fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// Map a run status string to the display status used by the tesseract.
fn map_status(status: &str) -> &'static str {
    match status {
        "completed" => "accepted",
        "failed" => "failed",
        "running" => "in_progress",
        _ => "rejected",
    }
}

// ── Cause-type inference ──────────────────────────────────────────────────────

/// Map a Coobie CausalRule description string to a stable cause-type key.
/// Matches against both the rule ID tokens and natural-language fragments
/// present in the description text.
pub fn infer_cause_type(description: &str) -> &'static str {
    let d = description.to_lowercase();
    if d.contains("spec_ambiguity")
        || d.contains("spec ambig")
        || d.contains("ambiguous")
        || d.contains("acceptance criteria")
    {
        return "spec_ambiguity";
    }
    if d.contains("twin_gap") || d.contains("twin gap") || d.contains("twin fidelit") {
        return "low_twin_fidelity";
    }
    if d.contains("test_blind")
        || d.contains("blind spot")
        || d.contains("happy path")
        || d.contains("false-negative")
    {
        return "test_blind_spot";
    }
    if d.contains("no_prior_memory")
        || d.contains("no prior memory")
        || d.contains("prior memory")
        || d.contains("pattern-cold")
    {
        return "context_gap";
    }
    if d.contains("broad_scope")
        || d.contains("broad scope")
        || d.contains("scope reduction")
        || d.contains("narrow the spec")
    {
        return "missing_failure_case";
    }
    if d.contains("policy") || d.contains("keeper") {
        return "policy_block";
    }
    if d.contains("tool") || d.contains("misu") {
        return "tool_misuse";
    }
    "context_gap"
}

// ── Projection ────────────────────────────────────────────────────────────────

/// Compute the observed (outer cube) and inferred (inner cube) 3D positions
/// for an episode, given its quality dimensions.
fn project_episode(
    spec_quality: f32,
    implementation_quality: f32,
    validation_outcome: f32,
    confidence: f32,
    intervention_potential: f32,
    cause_type: &str,
) -> ([f32; 3], [f32; 3]) {
    let observed = [
        (spec_quality - 0.5) * 1.7,
        (implementation_quality - 0.5) * 1.7,
        (validation_outcome - 0.5) * 1.7,
    ];

    let inferred = [
        cause_x(cause_type) * 0.75,
        (confidence - 0.5) * 1.2,
        (intervention_potential - 0.5) * 1.2,
    ];

    (observed, inferred)
}

// ── Scene builder ─────────────────────────────────────────────────────────────

/// Build the full `TesseractScene` from an array of (RunRecord, Option<CausalReport>) pairs.
pub fn build_scene(run_reports: Vec<(RunRecord, Option<CausalReport>)>) -> TesseractScene {
    let mut episode_nodes: Vec<EpisodeTesseractNode> = Vec::new();
    let mut cause_map: HashMap<String, CauseTesseractNode> = HashMap::new();
    let mut edges: Vec<TesseractEdge> = Vec::new();

    for (run, report) in run_reports {
        let status = map_status(&run.status);
        let s = report.as_ref().and_then(|r| Some(&r.episode_scores));

        // ── Quality dimensions ────────────────────────────────────────────
        let spec_quality = s
            .map(|sc| sc.spec_clarity_score)
            .unwrap_or_else(|| match status {
                "accepted" => 0.72,
                "in_progress" => 0.55,
                _ => 0.38,
            });

        let implementation_quality =
            s.map(|sc| sc.twin_fidelity_score)
                .unwrap_or_else(|| match status {
                    "accepted" => 0.78,
                    _ => 0.34,
                });

        let validation_outcome = s
            .map(|sc| {
                clamp01(
                    sc.test_coverage_score * 0.50
                        + if sc.scenario_passed { 0.30 } else { 0.0 }
                        + if sc.validation_passed { 0.20 } else { 0.0 },
                )
            })
            .unwrap_or_else(|| match status {
                "accepted" => 0.82,
                _ => 0.18,
            });

        let confidence = report.as_ref().map(|r| r.primary_confidence).unwrap_or(0.5);
        let intervention_potential = if report.is_some() {
            clamp01(confidence * 0.8 + 0.15)
        } else {
            0.40
        };

        // ── Cause type ────────────────────────────────────────────────────
        let primary_cause_text = report.as_ref().and_then(|r| r.primary_cause.clone());
        let primary_cause_type = primary_cause_text
            .as_deref()
            .map(infer_cause_type)
            .unwrap_or("context_gap")
            .to_string();

        // ── Positions ─────────────────────────────────────────────────────
        let (observed, inferred) = project_episode(
            spec_quality,
            implementation_quality,
            validation_outcome,
            confidence,
            intervention_potential,
            &primary_cause_type,
        );

        let interventions: Vec<InterventionPlan> = report
            .as_ref()
            .map(|r| r.recommended_interventions.clone())
            .unwrap_or_default();

        let contributing_causes: Vec<String> = report
            .as_ref()
            .map(|r| r.contributing_causes.clone())
            .unwrap_or_default();

        let episode = EpisodeTesseractNode {
            id: run.run_id.clone(),
            label: format!("{} · {}", run.product, run.spec_id),
            product_id: run.product.clone(),
            run_id: run.run_id.clone(),
            spec_quality,
            implementation_quality,
            validation_outcome,
            intervention_potential,
            primary_cause_type: primary_cause_type.clone(),
            confidence,
            status: status.to_string(),
            timestamp: run.created_at.to_rfc3339(),
            observed_position_3d: observed,
            inferred_position_3d: inferred,
            raw_scores: report.as_ref().map(|r| r.episode_scores.clone()),
            primary_cause_text: primary_cause_text.clone(),
            contributing_causes: contributing_causes.clone(),
            interventions: interventions.clone(),
        };

        episode_nodes.push(episode);

        // ── Primary cause node ────────────────────────────────────────────
        if let Some(ref cause_text) = primary_cause_text {
            let entry = cause_map
                .entry(primary_cause_type.clone())
                .or_insert_with(|| CauseTesseractNode {
                    id: primary_cause_type.clone(),
                    label: cause_text.clone(),
                    cause_type: primary_cause_type.clone(),
                    confidence,
                    supporting_run_ids: Vec::new(),
                    position_3d: cause_position(&primary_cause_type),
                    interventions: Vec::new(),
                });

            entry.supporting_run_ids.push(run.run_id.clone());
            if confidence > entry.confidence {
                entry.confidence = confidence;
                entry.label = cause_text.clone();
                entry.interventions = interventions.clone();
            }

            edges.push(TesseractEdge {
                id: format!("{}->{}", run.run_id, primary_cause_type),
                source_id: run.run_id.clone(),
                target_id: primary_cause_type.clone(),
                kind: "observed_to_inferred".to_string(),
                confidence,
                weight: confidence,
            });
        }

        // ── Contributing cause nodes (secondary edges) ────────────────────
        for contrib in &contributing_causes {
            let contrib_type = infer_cause_type(contrib).to_string();
            if contrib_type == primary_cause_type {
                continue;
            }

            let sec_conf = confidence * 0.55;
            let entry =
                cause_map
                    .entry(contrib_type.clone())
                    .or_insert_with(|| CauseTesseractNode {
                        id: contrib_type.clone(),
                        label: contrib.clone(),
                        cause_type: contrib_type.clone(),
                        confidence: sec_conf,
                        supporting_run_ids: Vec::new(),
                        position_3d: cause_position(&contrib_type),
                        interventions: Vec::new(),
                    });
            entry.supporting_run_ids.push(run.run_id.clone());

            edges.push(TesseractEdge {
                id: format!("{}->{}-contrib", run.run_id, contrib_type),
                source_id: run.run_id.clone(),
                target_id: contrib_type.clone(),
                kind: "cause_to_effect".to_string(),
                confidence: sec_conf,
                weight: sec_conf,
            });
        }
    }

    let clusters = cluster_episodes(&episode_nodes);

    TesseractScene {
        episode_nodes,
        cause_nodes: cause_map.into_values().collect(),
        intervention_nodes: Vec::new(),
        edges,
        clusters,
    }
}

// ── Clustering ────────────────────────────────────────────────────────────────

fn cluster_episodes(nodes: &[EpisodeTesseractNode]) -> Vec<TesseractCluster> {
    let mut groups: HashMap<String, Vec<&EpisodeTesseractNode>> = HashMap::new();
    for node in nodes {
        groups.entry(node.status.clone()).or_default().push(node);
    }

    groups
        .into_iter()
        .map(|(status, eps)| {
            let n = eps.len() as f32;
            let center = eps.iter().fold([0f32; 3], |acc, ep| {
                [
                    acc[0] + ep.observed_position_3d[0] / n,
                    acc[1] + ep.observed_position_3d[1] / n,
                    acc[2] + ep.observed_position_3d[2] / n,
                ]
            });

            let radius = eps
                .iter()
                .map(|ep| {
                    let dx = ep.observed_position_3d[0] - center[0];
                    let dy = ep.observed_position_3d[1] - center[1];
                    let dz = ep.observed_position_3d[2] - center[2];
                    (dx * dx + dy * dy + dz * dz).sqrt()
                })
                .fold(0.18f32, f32::max)
                + 0.18;

            TesseractCluster {
                label: status.clone(),
                color: cluster_color(&status).to_string(),
                center,
                radius,
                id: status,
            }
        })
        .collect()
}
