use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectComponent {
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub kind: String,
    pub path: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub interfaces: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ScenarioBlueprint {
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub code_under_test: Vec<String>,
    #[serde(default)]
    pub hidden_oracles: Vec<String>,
    #[serde(default)]
    pub datasets: Vec<String>,
    #[serde(default)]
    pub runtime_surfaces: Vec<String>,
    #[serde(default)]
    pub coobie_memory_topics: Vec<String>,
    #[serde(default)]
    pub required_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Spec {
    pub id: String,
    pub title: String,
    pub purpose: String,
    pub scope: Vec<String>,
    pub constraints: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub forbidden_behaviors: Vec<String>,
    pub rollback_requirements: Vec<String>,
    pub dependencies: Vec<String>,
    pub performance_expectations: Vec<String>,
    pub security_expectations: Vec<String>,
    #[serde(default)]
    pub project_components: Vec<ProjectComponent>,
    #[serde(default)]
    pub scenario_blueprint: Option<ScenarioBlueprint>,
    #[serde(default)]
    pub test_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub run_id: String,
    pub spec_id: String,
    pub product: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub event_id: i64,
    pub run_id: String,
    pub episode_id: Option<String>,
    pub phase: String,
    pub agent: String,
    pub status: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

pub type FactoryEvent = RunEvent;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeRecord {
    pub episode_id: String,
    pub run_id: String,
    pub phase: String,
    pub goal: String,
    pub outcome: Option<String>,
    pub confidence: Option<f64>,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LessonRecord {
    pub lesson_id: String,
    pub source_episode: Option<String>,
    pub pattern: String,
    pub intervention: Option<String>,
    pub tags: Vec<String>,
    pub strength: f64,
    pub recall_count: i64,
    pub last_recalled: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriorCauseSignal {
    pub cause_id: String,
    pub description: String,
    pub occurrences: i64,
    pub scenario_pass_rate: f32,
    pub last_seen_run_id: Option<String>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoobieBriefing {
    pub spec_id: String,
    pub product: String,
    pub query_terms: Vec<String>,
    pub domain_signals: Vec<String>,
    pub prior_report_count: usize,
    pub memory_hits: Vec<String>,
    pub relevant_lessons: Vec<LessonRecord>,
    pub prior_causes: Vec<PriorCauseSignal>,
    pub project_components: Vec<ProjectComponent>,
    pub scenario_blueprint: Option<ScenarioBlueprint>,
    pub application_risks: Vec<String>,
    pub environment_risks: Vec<String>,
    pub regulatory_considerations: Vec<String>,
    pub recommended_guardrails: Vec<String>,
    pub required_checks: Vec<String>,
    pub open_questions: Vec<String>,
    pub coobie_response: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlackboardState {
    pub run_id: String,
    pub current_phase: String,
    pub active_goal: String,
    pub open_blockers: Vec<String>,
    pub resolved_items: Vec<String>,
    pub artifact_refs: Vec<String>,
    pub lesson_refs: Vec<String>,
    pub policy_flags: Vec<String>,
    pub agent_claims: HashMap<String, String>,
}

impl BlackboardState {
    pub fn role_view(&self, role: &str) -> Self {
        let mut view = self.clone();
        let role = role.to_ascii_lowercase();

        if role == "keeper" {
            return view;
        }

        view.policy_flags.clear();

        match role.as_str() {
            "coobie" => {
                view.agent_claims.clear();
            }
            "sable" => {
                view.agent_claims.clear();
            }
            "scout" => {
                view.agent_claims.retain(|agent, _| agent == "scout");
                view.artifact_refs.retain(|artifact| {
                    matches!(
                        artifact.as_str(),
                        "intent.json"
                            | "memory_context.md"
                            | "coobie_briefing.json"
                            | "coobie_preflight_response.md"
                    )
                });
            }
            "mason" => {
                view.agent_claims.retain(|agent, _| agent == "mason");
                view.artifact_refs.retain(|artifact| {
                    matches!(
                        artifact.as_str(),
                        "intent.json"
                            | "memory_context.md"
                            | "coobie_briefing.json"
                            | "coobie_preflight_response.md"
                            | "implementation_plan.md"
                            | "validation.json"
                    )
                });
            }
            "bramble" => {
                view.agent_claims.retain(|agent, _| agent == "bramble");
                view.artifact_refs.retain(|artifact| {
                    matches!(
                        artifact.as_str(),
                        "validation.json"
                            | "twin.json"
                            | "coobie_briefing.json"
                            | "coobie_preflight_response.md"
                    )
                });
            }
            "ash" => {
                view.agent_claims.retain(|agent, _| agent == "ash");
                view.artifact_refs.retain(|artifact| {
                    matches!(
                        artifact.as_str(),
                        "twin.json"
                            | "twin_narrative.md"
                            | "coobie_briefing.json"
                            | "coobie_preflight_response.md"
                    )
                });
            }
            "flint" => {
                view.agent_claims.retain(|agent, _| agent == "flint");
            }
            _ => {
                view.agent_claims.retain(|agent, _| agent == &role);
            }
        }

        view
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecution {
    pub agent_name: String,
    pub display_name: String,
    pub role: String,
    pub provider: String,
    pub model: String,
    pub surface: Option<String>,
    pub usage_rights: Option<String>,
    pub mode: String,
    pub prompt: String,
    #[serde(default)]
    pub pidgin_summary: String,
    pub summary: String,
    pub output: String,
    pub allowed_tools: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentPackage {
    pub spec_id: String,
    pub summary: String,
    pub ambiguity_notes: Vec<String>,
    pub recommended_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub scenario_id: String,
    pub passed: bool,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub passed: bool,
    pub results: Vec<ScenarioResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinService {
    pub name: String,
    pub kind: String,
    pub status: String,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TwinEnvironment {
    pub name: String,
    pub status: String,
    pub services: Vec<TwinService>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenScenarioCheckResult {
    pub kind: String,
    pub passed: bool,
    pub details: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenScenarioEvaluation {
    pub scenario_id: String,
    pub title: String,
    pub passed: bool,
    pub details: String,
    pub checks: Vec<HiddenScenarioCheckResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HiddenScenarioSummary {
    pub passed: bool,
    pub results: Vec<HiddenScenarioEvaluation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryEpisode {
    pub run_id: String,
    pub product: String,
    pub spec_id: String,
    pub features: Vec<String>,
    pub agent_events: Vec<RunEvent>,
    pub tool_events: Vec<String>, // Placeholder for tool-specific events
    pub twin_env: Option<TwinEnvironment>,
    pub validation: Option<ValidationSummary>,
    pub scenarios: Option<HiddenScenarioSummary>,
    pub decision: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterfactualEstimate {
    pub intervention: String,
    pub predicted_outcome: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalHypothesis {
    pub cause_id: String,
    pub description: String,
    pub confidence: f32,
    pub supporting_runs: Vec<String>,
    pub counterfactuals: Vec<CounterfactualEstimate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterventionPlan {
    pub target: String,
    pub action: String,
    pub expected_impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CounterfactualOutcome {
    pub prediction: String,
    pub confidence_gain: f32,
}
