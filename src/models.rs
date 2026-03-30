use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
pub struct CausalLink {
    pub link_id: String,
    pub from_event: i64,
    pub to_event: i64,
    pub link_type: String,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
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
                view.artifact_refs
                    .retain(|artifact| artifact == "intent.json" || artifact == "memory_context.md");
            }
            "mason" => {
                view.agent_claims.retain(|agent, _| agent == "mason");
                view.artifact_refs.retain(|artifact| {
                    matches!(
                        artifact.as_str(),
                        "intent.json" | "memory_context.md" | "implementation_plan.md" | "validation.json"
                    )
                });
            }
            "bramble" => {
                view.agent_claims.retain(|agent, _| agent == "bramble");
                view.artifact_refs
                    .retain(|artifact| artifact == "validation.json" || artifact == "twin.json");
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
