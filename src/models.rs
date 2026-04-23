use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkerHarnessConfig {
    #[serde(default)]
    pub adapter: String,
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub allowed_components: Vec<String>,
    #[serde(default)]
    pub denied_paths: Vec<String>,
    #[serde(default)]
    pub visible_success_conditions: Vec<String>,
    #[serde(default)]
    pub return_artifacts: Vec<String>,
    #[serde(default)]
    pub max_iterations: Option<u32>,
    #[serde(default)]
    pub continuity_file: Option<String>,
    #[serde(default)]
    pub llm_edits: bool,
    /// When true, Mason commits its edits to a new git branch in the source repo
    /// (mason/<spec-id>-<short-run-id>) so a real diff is always available.
    #[serde(default)]
    pub git_branch: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceTimeRange {
    #[serde(default)]
    pub start_ms: Option<i64>,
    #[serde(default)]
    pub end_ms: Option<i64>,
    #[serde(default)]
    pub start_iso: Option<String>,
    #[serde(default)]
    pub end_iso: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceSource {
    pub source_id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceRegion {
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceAnchor {
    pub anchor_id: String,
    pub source_id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub signal_keys: Vec<String>,
    #[serde(default)]
    pub sample_index: Option<i64>,
    #[serde(default)]
    pub frame_index: Option<i64>,
    #[serde(default)]
    pub timestamp_ms: Option<i64>,
    #[serde(default)]
    pub time_range: Option<EvidenceTimeRange>,
    #[serde(default)]
    pub region: Option<EvidenceRegion>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceCausalClaim {
    pub claim_id: String,
    #[serde(default)]
    pub relation: String,
    #[serde(default)]
    pub cause: String,
    #[serde(default)]
    pub effect: String,
    #[serde(default)]
    pub confidence: Option<f64>,
    #[serde(default)]
    pub evidence_anchor_ids: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceAnnotation {
    #[serde(default)]
    pub annotation_id: String,
    #[serde(default)]
    pub annotation_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub promote_to_memory: String,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub time_range: Option<EvidenceTimeRange>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub anchors: Vec<EvidenceAnchor>,
    #[serde(default)]
    pub claims: Vec<EvidenceCausalClaim>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub reviewed_by: String,
    #[serde(default)]
    pub reviewed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceAnnotationBundle {
    #[serde(default = "default_evidence_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub scenario: String,
    #[serde(default)]
    pub dataset: String,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub sources: Vec<EvidenceSource>,
    #[serde(default)]
    pub annotations: Vec<EvidenceAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EvidenceAnnotationHistoryEvent {
    pub event_id: String,
    #[serde(default)]
    pub bundle_name: String,
    #[serde(default)]
    pub annotation_id: String,
    #[serde(default)]
    pub annotation_title: String,
    #[serde(default)]
    pub event_type: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub previous_status: String,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub promoted_ids: Vec<String>,
    #[serde(default)]
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceWindowMatch {
    pub score: i32,
    pub project: String,
    pub scenario: String,
    pub dataset: String,
    pub bundle_path: String,
    pub annotation_id: String,
    pub annotation_type: String,
    pub title: String,
    #[serde(default)]
    pub time_summary: String,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub matched_labels: Vec<String>,
    #[serde(default)]
    pub matched_claims: Vec<String>,
    #[serde(default)]
    pub matched_sources: Vec<String>,
    #[serde(default)]
    pub time_span_delta_ms: Option<i64>,
    pub citation: CoobieEvidenceCitation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceMatchAssessment {
    pub rank: usize,
    pub match_type: String,
    pub score: i32,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default)]
    pub rationale: Vec<String>,
    pub window: EvidenceWindowMatch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceMatchReport {
    pub spec_id: String,
    pub product: String,
    #[serde(default)]
    pub query_source: String,
    #[serde(default)]
    pub selected_window_summary: Option<String>,
    #[serde(default)]
    pub query_terms: Vec<String>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub time_span_ms: Option<i64>,
    #[serde(default)]
    pub summary: Vec<String>,
    #[serde(default)]
    pub assessments: Vec<EvidenceMatchAssessment>,
    pub generated_at: DateTime<Utc>,
}

fn default_evidence_schema_version() -> u32 {
    1
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
    pub twin_services: Vec<TwinServiceSpec>,
    #[serde(default)]
    pub project_components: Vec<ProjectComponent>,
    #[serde(default)]
    pub scenario_blueprint: Option<ScenarioBlueprint>,
    #[serde(default)]
    pub worker_harness: Option<WorkerHarnessConfig>,
    #[serde(default)]
    pub test_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TwinFailureMode {
    AuthExpiry,
    RateLimit,
    ConnectionRefusal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TwinServiceSpec {
    pub name: String,
    pub image: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_mode: Option<TwinFailureMode>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunPhaseTiming {
    pub phase: String,
    #[serde(default)]
    pub episode_count: usize,
    #[serde(default)]
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunTimingReport {
    pub run_id: String,
    #[serde(default)]
    pub total_duration_ms: u64,
    #[serde(default)]
    pub memory_duration_ms: u64,
    #[serde(default)]
    pub intake_duration_ms: u64,
    #[serde(default)]
    pub implementation_duration_ms: u64,
    #[serde(default)]
    pub validation_duration_ms: u64,
    #[serde(default)]
    pub other_duration_ms: u64,
    #[serde(default)]
    pub phase_durations: Vec<RunPhaseTiming>,
    pub generated_at: DateTime<Utc>,
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

/// Broadcast over the in-process event channel so the SSE endpoint and any
/// other subscriber can observe factory activity in real time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LiveEvent {
    /// A normal run lifecycle event (phase transitions, agent status).
    RunEvent(RunEvent),
    /// A single line of stdout or stderr from a Piper build subprocess.
    BuildOutput {
        run_id: String,
        phase: String,
        agent: String,
        line: String,
        /// `"stdout"` or `"stderr"`
        stream: String,
        created_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointAnswerRecord {
    pub answer_id: String,
    pub checkpoint_id: String,
    pub answered_by: String,
    pub answer_text: String,
    #[serde(default)]
    pub decision_json: Option<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCheckpointRecord {
    pub checkpoint_id: String,
    pub run_id: String,
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    pub checkpoint_type: String,
    pub status: String,
    pub prompt: String,
    pub context_json: serde_json::Value,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub resolved_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub answers: Vec<CheckpointAnswerRecord>,
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
    #[serde(default)]
    pub state_before: Option<String>,
    #[serde(default)]
    pub state_after: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseAttributionRecord {
    pub attribution_id: String,
    pub run_id: String,
    pub episode_id: String,
    pub phase: String,
    pub agent_name: String,
    pub outcome: String,
    pub confidence: Option<f64>,
    #[serde(default)]
    pub prompt_bundle_fingerprint: Option<String>,
    #[serde(default)]
    pub prompt_bundle_provider: Option<String>,
    #[serde(default)]
    pub prompt_bundle_artifact: Option<String>,
    #[serde(default)]
    pub pinned_skill_ids: Vec<String>,
    #[serde(default)]
    pub memory_hits: Vec<String>,
    #[serde(default)]
    pub core_memory_ids: Vec<String>,
    #[serde(default)]
    pub project_memory_ids: Vec<String>,
    #[serde(default)]
    pub relevant_lesson_ids: Vec<String>,
    #[serde(default)]
    pub required_checks: Vec<String>,
    #[serde(default)]
    pub guardrails: Vec<String>,
    #[serde(default)]
    pub query_terms: Vec<String>,
    #[serde(default)]
    pub stakeholder_alignment: Option<StakeholderAlignmentSummary>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StakeholderAlignmentSummary {
    #[serde(default)]
    pub stamped_context_present: bool,
    #[serde(default)]
    pub repo_purpose: String,
    #[serde(default)]
    pub operator_intent: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub vertical: String,
    #[serde(default)]
    pub attitudes_recorded: usize,
    #[serde(default)]
    pub constraints_recorded: usize,
    #[serde(default)]
    pub skill_sources_recorded: usize,
    #[serde(default)]
    pub mcp_servers_recorded: usize,
    #[serde(default)]
    pub alignment_guardrails: Vec<String>,
    #[serde(default)]
    pub alignment_checks: Vec<String>,
    #[serde(default)]
    pub alignment_open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchmarkStakeholderAlignmentSnapshot {
    #[serde(default)]
    pub run_ids: Vec<String>,
    #[serde(default)]
    pub phases_considered: usize,
    #[serde(default)]
    pub phases_with_stamped_context: usize,
    #[serde(default)]
    pub phases_with_alignment_guardrails: usize,
    #[serde(default)]
    pub phases_with_alignment_checks: usize,
    #[serde(default)]
    pub phases_with_alignment_questions: usize,
    #[serde(default)]
    pub phases_with_attitude_signals: usize,
    #[serde(default)]
    pub phases_with_constraint_signals: usize,
    #[serde(default)]
    pub phases_with_mcp_signals: usize,
    #[serde(default)]
    pub latest_repo_purpose: String,
    #[serde(default)]
    pub latest_operator_intent: String,
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
pub struct CoobieEvidenceCitation {
    pub citation_id: String,
    pub source_type: String,
    pub run_id: String,
    #[serde(default)]
    pub episode_id: Option<String>,
    pub phase: String,
    pub agent: String,
    pub summary: String,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectResumeRisk {
    pub memory_id: String,
    pub summary: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub severity_score: i32,
    #[serde(default)]
    pub reasons: Vec<String>,
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
    #[serde(default)]
    pub core_memory_hits: Vec<String>,
    #[serde(default)]
    pub project_memory_hits: Vec<String>,
    #[serde(default)]
    pub resume_packet_summary: Vec<String>,
    #[serde(default)]
    pub resume_packet_risks: Vec<ProjectResumeRisk>,
    #[serde(default)]
    pub stale_memory_mitigation_plan: Vec<String>,
    #[serde(default)]
    pub exploration_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub strategy_register_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub mitigation_history_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub evidence_pattern_exemplar_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub evidence_causal_exemplar_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub nearest_evidence_window_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub pattern_matching_focus: Vec<String>,
    #[serde(default)]
    pub causal_chain_focus: Vec<String>,
    #[serde(default)]
    pub forge_evidence_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub preferred_forge_outcome_citations: Vec<CoobieEvidenceCitation>,
    #[serde(default)]
    pub preferred_forge_commands: Vec<String>,
    pub relevant_lessons: Vec<LessonRecord>,
    pub prior_causes: Vec<PriorCauseSignal>,
    pub project_components: Vec<ProjectComponent>,
    pub scenario_blueprint: Option<ScenarioBlueprint>,
    #[serde(default)]
    pub project_memory_root: Option<String>,
    pub application_risks: Vec<String>,
    pub environment_risks: Vec<String>,
    pub regulatory_considerations: Vec<String>,
    #[serde(default)]
    pub operator_model_context: Option<OperatorModelContext>,
    #[serde(default)]
    pub project_interview_context: Option<ProjectInterviewContext>,
    #[serde(default)]
    pub soul_identity_context: Option<SoulIdentityContext>,
    pub recommended_guardrails: Vec<String>,
    pub required_checks: Vec<String>,
    pub open_questions: Vec<String>,
    pub coobie_response: String,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProjectInterviewContext {
    #[serde(default)]
    pub repo_name: String,
    #[serde(default)]
    pub repo_purpose: String,
    #[serde(default)]
    pub operator_intent: String,
    #[serde(default)]
    pub environment: String,
    #[serde(default)]
    pub vertical: String,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub attitudes: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub skill_sources: Vec<String>,
    #[serde(default)]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub interview_context_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SoulIdentityContext {
    pub self_name: String,
    pub identity_thesis: String,
    #[serde(default)]
    pub preserved_invariants: Vec<String>,
    #[serde(default)]
    pub baseline_beliefs: Vec<String>,
    #[serde(default)]
    pub allowed_adaptations: Vec<String>,
    #[serde(default)]
    pub forbidden_drift: Vec<String>,
    pub adaptation_law: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRuntimeState {
    pub runtime_id: String,
    pub canonical_role: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub ownership: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub last_heartbeat_at: String,
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
    #[serde(default)]
    pub packchat_thread_id: Option<String>,
    pub agent_claims: HashMap<String, String>,
    #[serde(default)]
    pub agent_instances: Vec<AgentRuntimeState>,
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
                view.agent_instances.clear();
            }
            "sable" => {
                view.agent_claims.clear();
                view.agent_instances.clear();
            }
            "scout" => {
                view.agent_claims.retain(|agent, _| agent == "scout");
                view.agent_instances
                    .retain(|instance| instance.canonical_role == "scout");
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
                view.agent_instances
                    .retain(|instance| instance.canonical_role == "mason");
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
                view.agent_instances
                    .retain(|instance| instance.canonical_role == "bramble");
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
                view.agent_instances
                    .retain(|instance| instance.canonical_role == "ash");
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
                view.agent_instances
                    .retain(|instance| instance.canonical_role == "flint");
            }
            _ => {
                view.agent_claims.retain(|agent, _| agent == &role);
                view.agent_instances
                    .retain(|instance| instance.canonical_role == role);
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
    #[serde(default)]
    pub phase: Option<String>,
    #[serde(default)]
    pub episode_id: Option<String>,
    #[serde(default)]
    pub prompt_bundle_fingerprint: Option<String>,
    #[serde(default)]
    pub prompt_bundle_artifact: Option<String>,
    #[serde(default)]
    pub prompt_bundle_provider: Option<String>,
    #[serde(default)]
    pub pinned_skill_ids: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    CompileError,
    TestFailure,
    WrongAnswer,
    Timeout,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationSummary {
    pub passed: bool,
    #[serde(default)]
    pub scored_checks: usize,
    #[serde(default)]
    pub passed_scored_checks: usize,
    pub results: Vec<ScenarioResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<FailureKind>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose_project: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PearlHierarchyLevel {
    #[default]
    Associational,
    Interventional,
    Counterfactual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalHypothesisEvidence {
    pub kind: String,
    pub ref_id: String,
    pub relation: String,
    #[serde(default)]
    pub hierarchy_level: PearlHierarchyLevel,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FactoryEpisode {
    pub run_id: String,
    pub product: String,
    pub spec_id: String,
    pub features: Vec<String>,
    pub agent_events: Vec<RunEvent>,
    pub tool_events: Vec<String>, // Placeholder for tool-specific events
    #[serde(default)]
    pub phase_attributions: Vec<PhaseAttributionRecord>,
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
    #[serde(default)]
    pub hierarchy_level: PearlHierarchyLevel,
    pub supporting_runs: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<CausalHypothesisEvidence>,
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

/// A candidate item surfaced by Coobie at end-of-run for operator review.
/// Nothing is written to durable memory until the operator approves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsolidationCandidate {
    pub candidate_id: String,
    pub run_id: String,
    /// `"lesson"` | `"causal_link"` | `"pattern"`
    pub kind: String,
    /// `"pending"` | `"kept"` | `"discarded"`
    pub status: String,
    /// The raw proposed content (lesson record, causal link, etc.)
    pub content_json: serde_json::Value,
    /// If the operator edited the content, this holds the edited version.
    #[serde(default)]
    pub edited_json: Option<serde_json::Value>,
    pub confidence: f64,
    /// Human-readable one-liner shown in the Workbench card.
    pub label: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub reviewed_at: Option<DateTime<Utc>>,
}

/// A causal pattern that has fired on consecutive runs of the same spec.
/// Streak length ≥ 3 triggers escalation — the standard intervention alone
/// is not breaking the cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalStreak {
    pub cause_id: String,
    pub streak_len: usize,
    pub description: String,
    /// Confidence boost applied to the hypothesis because of the streak.
    pub confidence_boost: f32,
    /// True when streak_len >= 3 — escalation intervention is recommended.
    pub escalate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeStateDiff {
    pub summary: String,
    pub added_files: Vec<String>,
    pub modified_files: Vec<String>,
    pub removed_files: Vec<String>,
    pub unchanged_files: usize,
    pub bytes_before: u64,
    pub bytes_after: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpisodeCausalState {
    pub episode: EpisodeRecord,
    #[serde(default)]
    pub state_diff: Option<EpisodeStateDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEventNode {
    pub event_id: i64,
    pub run_id: String,
    #[serde(default)]
    pub episode_id: Option<String>,
    pub phase: String,
    pub agent: String,
    pub status: String,
    pub message: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalEventEdge {
    pub link_id: String,
    pub from_event: i64,
    pub to_event: i64,
    pub link_type: String,
    pub confidence: f64,
    #[serde(default)]
    pub hierarchy_level: PearlHierarchyLevel,
    #[serde(default)]
    pub from_episode_id: Option<String>,
    #[serde(default)]
    pub to_episode_id: Option<String>,
    pub from_phase: String,
    pub to_phase: String,
    pub from_agent: String,
    pub to_agent: String,
    pub from_status: String,
    pub to_status: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunCausalGraph {
    pub run_id: String,
    pub generated_at: DateTime<Utc>,
    pub episodes: Vec<EpisodeCausalState>,
    pub events: Vec<CausalEventNode>,
    pub links: Vec<CausalEventEdge>,
    #[serde(default)]
    pub hypotheses: Vec<CausalHypothesis>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OperatorModelScope {
    #[default]
    Project,
    Global,
}

impl OperatorModelScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Global => "global",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorModelProfile {
    pub profile_id: String,
    pub scope: OperatorModelScope,
    #[serde(default)]
    pub project_root: Option<String>,
    pub display_name: String,
    pub status: String,
    pub current_version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorModelSession {
    pub session_id: String,
    pub profile_id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    pub status: String,
    #[serde(default)]
    pub pending_layer: Option<String>,
    #[serde(default)]
    pub started_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorModelLayerCheckpoint {
    pub checkpoint_id: String,
    pub session_id: String,
    pub profile_id: String,
    pub version: i64,
    pub layer: String,
    pub status: String,
    pub summary_md: String,
    pub raw_notes_json: serde_json::Value,
    #[serde(default)]
    pub approved_by: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub approved_at: Option<DateTime<Utc>>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorModelEntry {
    pub entry_id: String,
    pub profile_id: String,
    pub version: i64,
    pub layer: String,
    pub entry_type: String,
    pub title: String,
    pub content: String,
    pub details_json: serde_json::Value,
    pub source_checkpoint_id: String,
    pub status: String,
    #[serde(default)]
    pub superseded_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUpdateRecord {
    pub update_id: String,
    pub old_memory_id: String,
    pub new_memory_id: String,
    #[serde(default)]
    pub memory_root: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub review_status: String,
    #[serde(default)]
    pub reviewed_by: Option<String>,
    #[serde(default)]
    pub review_note: Option<String>,
    #[serde(default)]
    pub reviewed_at: Option<String>,
    pub created_at: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorModelExport {
    pub export_id: String,
    pub profile_id: String,
    pub version: i64,
    pub artifact_name: String,
    pub content: String,
    pub content_type: String,
    pub created_at: DateTime<Utc>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperatorModelUpdateCandidate {
    pub candidate_id: String,
    pub profile_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub entry_id: Option<String>,
    pub proposal_kind: String,
    pub summary: String,
    pub proposal_json: serde_json::Value,
    pub status: String,
    pub confidence: f64,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub reviewed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OperatorModelContext {
    pub profile_id: String,
    pub scope: OperatorModelScope,
    pub display_name: String,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub source_thread_id: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub operating_rhythms: Vec<String>,
    #[serde(default)]
    pub guardrails: Vec<String>,
    #[serde(default)]
    pub escalation_rules: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub preferred_tools: Vec<String>,
    #[serde(default)]
    pub risk_tolerances: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
    #[serde(default)]
    pub transcript_excerpt: Vec<String>,
}

/// Artifact generated after both MVP interview layers are approved.
/// Written to `<export_root>/commissioning-brief.json` and read by Scout/Coobie.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CommissioningBrief {
    pub profile_id: String,
    pub generated_at: String,
    /// Operating rhythm facts extracted from layer 1.
    #[serde(default)]
    pub operating_rhythms: Vec<String>,
    /// Recurring decision patterns extracted from layer 2.
    #[serde(default)]
    pub recurring_decisions: Vec<String>,
    /// Top-3 patterns for Scout intent prompt injection.
    #[serde(default)]
    pub top_patterns: Vec<String>,
    #[serde(default)]
    pub preferred_tools: Vec<String>,
    #[serde(default)]
    pub risk_tolerances: Vec<String>,
}

// ── Cost / usage tracking ─────────────────────────────────────────────────────

/// Per-call LLM usage captured from provider response metadata.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmCostEvent {
    pub event_id: String,
    pub run_id: String,
    pub agent: String,
    pub phase: String,
    pub provider: String,
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
    pub created_at: DateTime<Utc>,
}

/// Aggregate cost summary for a run, returned by GET /api/runs/:id/cost.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunCostSummary {
    pub run_id: String,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
    pub total_tokens: u32,
    pub total_latency_ms: u64,
    pub call_count: u32,
    pub by_agent: Vec<AgentCostSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentCostSummary {
    pub agent: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub call_count: u32,
    pub latency_ms: u64,
}

// ── Decision log ──────────────────────────────────────────────────────────────

/// A record of a significant agent decision: what was chosen, what was
/// considered and rejected, and why. Answers governance question "why did
/// the agent act and what alternatives were rejected?"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub decision_id: String,
    pub run_id: String,
    pub agent: String,
    pub phase: String,
    /// e.g. "plan_critique", "mason_edit", "consolidation_promotion"
    pub decision_kind: String,
    pub chose: String,
    pub alternatives: Vec<String>,
    pub justification: String,
    #[serde(default)]
    pub approved_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Phase B: Agent Trace Spine ────────────────────────────────────────────────

/// A structured trace of one significant agent LLM call — what went in,
/// the reasoning steps, what actions were taken, and the outcome.
///
/// Reasoning steps are extracted from a leading `<reasoning>...</reasoning>` block
/// when the agent prompt requests it; otherwise the full response is stored as a
/// single step. This is the foundation for Coobie's self-improvement loop.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentTrace {
    pub trace_id: String,
    pub run_id: String,
    pub agent: String,
    pub phase: String,
    /// Short summary of what was passed to the agent (not the full prompt).
    pub input_summary: String,
    /// Extracted reasoning steps (from `<reasoning>` block or split by newlines).
    pub reasoning_steps: Vec<String>,
    /// Actions the agent decided to take (edit files, run command, emit JSON, etc.)
    pub actions_taken: Vec<String>,
    /// One-line outcome: "success", "partial", "failed", or a description.
    pub outcome: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u64,
    pub created_at: DateTime<Utc>,
}

// ── Phase C: OptimizationProgram ──────────────────────────────────────────────

/// Machine-readable success metric generated by Scout during spec intake.
///
/// Gives Coobie, Bramble, and Mason a shared, explicit definition of success
/// so the plan critique can check "does this plan actually optimise the stated
/// objective?" rather than only flagging guardrail violations.
///
/// Written to `optimization_program.json` in the run workspace and surfaced via
/// `GET /api/runs/:id/optimization-program`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OptimizationProgram {
    pub run_id: String,
    pub spec_id: String,
    /// Short identifier for the primary success metric ("latency_p95_ms", "test_pass_rate", etc.)
    pub objective_metric: String,
    /// Human-readable description of what the metric means and how it will be measured.
    pub objective_description: String,
    /// Files / modules / interfaces the implementation is allowed to change.
    #[serde(default)]
    pub editable_surface: Vec<String>,
    /// Hard constraints that must hold regardless of metric value.
    #[serde(default)]
    pub constraints: Vec<String>,
    /// Ordered steps to evaluate whether the objective was met after a run.
    #[serde(default)]
    pub evaluation_plan: Vec<String>,
    /// Optional wall-clock budget in seconds. None = unbounded.
    #[serde(default)]
    pub time_budget_secs: Option<u64>,
    pub generated_at: DateTime<Utc>,
}

// ── Phase D: MetricAttack ──────────────────────────────────────────────────────

/// A red-team attack generated by Sable against the run's stated objective metric.
///
/// Each attack describes a concrete way an implementation could game the metric
/// (shallow tests, output caching, hardcoded returns, etc.) and whether a probe
/// scenario detected that pattern in this run.
///
/// Written to `metric_attacks.json` in the run workspace and surfaced via
/// `GET /api/runs/:id/metric-attacks`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetricAttack {
    pub attack_id: String,
    pub run_id: String,
    /// Short description of how an implementation could game the objective metric.
    pub exploit_description: String,
    /// Observable signals in run artifacts or behaviour that would reveal this exploit.
    #[serde(default)]
    pub detection_signals: Vec<String>,
    /// Guardrail or evaluation-plan additions that would prevent this exploit.
    #[serde(default)]
    pub mitigations: Vec<String>,
    /// "detected" | "not_detected" | "inconclusive"
    pub status: String,
    pub generated_at: DateTime<Utc>,
}
