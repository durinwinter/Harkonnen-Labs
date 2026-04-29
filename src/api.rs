use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::{
    calvin_archive::SoulBootstrapDocument,
    chat::{dispatch_message, ChatThread, ChatThreadKind, OpenThreadRequest, PostMessageRequest},
    coobie::CausalReport,
    llm::{self, LlmRequest},
    memory::{MemoryRetrievalHit, MemoryStore},
    models::{
        AgentExecution, AgentRuntimeState, BlackboardState, ConsolidationCandidate, CoobieBriefing,
        DecisionRecord, EvidenceAnnotation, EvidenceAnnotationBundle,
        EvidenceAnnotationHistoryEvent, EvidenceMatchReport, EvidenceSource, HiddenScenarioSummary,
        InterventionPlan, LessonRecord, MetricAttack, OperatorModelContext, OperatorModelProfile,
        OperatorModelScope, OperatorModelSession, OptimizationProgram, PhaseAttributionRecord,
        PriorCauseSignal, RunCheckpointRecord, RunEvent, RunRecord, Spec, ValidationSummary,
    },
    orchestrator::{AppContext, RunRequest},
    pidgin::{self, PidginTranslation},
    reporting,
    setup::command_available,
    tesseract,
};

#[derive(Debug, Serialize)]
struct RunStateResponse {
    run: RunRecord,
    events: Vec<RunEvent>,
    blackboard: Option<BlackboardState>,
    lessons: Vec<LessonRecord>,
    agent_executions: Vec<AgentExecution>,
    phase_attributions: Vec<PhaseAttributionRecord>,
    coobie_briefing: Option<CoobieBriefing>,
    causal_report: Option<CausalReport>,
    coobie_preflight_response: Option<String>,
    coobie_report_response: Option<String>,
    evidence_match_report: Option<EvidenceMatchReport>,
    coobie_translations: Vec<PidginTranslation>,
}

#[derive(Debug, Serialize)]
struct ConsolidateRunResponse {
    run_id: String,
    total_new_lessons: usize,
    new_lessons: Vec<LessonRecord>,
    memory_board: MemoryBoardResponse,
}

#[derive(Debug, Serialize)]
struct MemoryBoardResponse {
    run_id: String,
    current_phase: Option<String>,
    active_recalled_lessons: Vec<MemoryBoardLessonView>,
    active_reasoning_lessons: Vec<MemoryBoardLessonView>,
    phase_memory_usage: Vec<MemoryBoardPhaseUsage>,
    causal_precedents: Vec<PriorCauseSignal>,
    policy_reminders: Vec<String>,
    project_memory_root: Option<String>,
    reasoning_summary: MemoryBoardReasoningSummary,
    recent_decisions: Vec<DecisionRecord>,
    recent_checkpoint_answers: Vec<ReasoningCheckpointAnswerView>,
    stale_risk_summary: MemoryBoardRiskSummary,
    stale_memory_entries: Vec<MemoryBoardRiskView>,
    memory_updates: Vec<MemoryBoardUpdateView>,
    consolidate_available: bool,
}

#[derive(Debug, Serialize)]
struct MissionBoardResponse {
    run_id: String,
    spec_id: String,
    title: String,
    purpose: String,
    product: String,
    run_status: String,
    current_phase: Option<String>,
    active_goal: Option<String>,
    scope: Vec<String>,
    constraints: Vec<String>,
    acceptance_criteria: Vec<String>,
    forbidden_behaviors: Vec<String>,
    open_blockers: Vec<String>,
    resolved_items: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ActionBoardResponse {
    run_id: String,
    current_phase: Option<String>,
    active_goal: Option<String>,
    agent_claims: HashMap<String, String>,
    agent_instances: Vec<AgentRuntimeState>,
    open_blockers: Vec<String>,
    open_checkpoints: Vec<RunCheckpointRecord>,
    recent_events: Vec<RunEvent>,
    latest_agent_executions: Vec<AgentExecution>,
}

#[derive(Debug, Serialize)]
struct EvidenceBoardResponse {
    run_id: String,
    artifact_refs: Vec<String>,
    validation: Option<ValidationSummary>,
    hidden_scenarios: Option<HiddenScenarioSummary>,
    evidence_match_report: Option<EvidenceMatchReport>,
    causal_report: Option<CausalReport>,
    recent_evidence_events: Vec<RunEvent>,
}

#[derive(Debug, Clone, Serialize)]
struct MemoryBoardLessonView {
    lesson: LessonRecord,
    used_in_phases: Vec<String>,
    used_by_agents: Vec<String>,
    outcomes: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MemoryBoardPhaseUsage {
    phase: String,
    agent_name: String,
    outcome: String,
    prompt_bundle_provider: Option<String>,
    memory_hits: Vec<String>,
    core_memory_ids: Vec<String>,
    project_memory_ids: Vec<String>,
    relevant_lesson_ids: Vec<String>,
    required_checks: Vec<String>,
    guardrails: Vec<String>,
}

#[derive(Debug, Serialize)]
struct MemoryBoardRiskSummary {
    stale_risk_count: usize,
    satisfied_count: usize,
    partially_satisfied_count: usize,
    unresolved_count: usize,
    active_risk_score: i32,
}

#[derive(Debug, Serialize)]
struct MemoryBoardRiskView {
    memory_id: String,
    summary: String,
    severity: String,
    severity_score: i32,
    reasons: Vec<String>,
    mitigation_status: Option<String>,
    mitigation_steps: Vec<String>,
    related_checks: Vec<String>,
    evidence: Vec<String>,
    previous_severity_score: Option<i32>,
    risk_reduced_from_previous: Option<bool>,
}

#[derive(Debug, Serialize)]
struct MemoryBoardUpdateView {
    relation: String,
    stale_memory_id: String,
    stale_summary: String,
    fresh_memory_id: String,
    fresh_summary: String,
}

#[derive(Debug, Serialize)]
struct MemoryBoardReasoningSummary {
    decision_count: usize,
    checkpoint_answer_count: usize,
    active_reasoning_lesson_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReasoningCheckpointAnswerView {
    checkpoint_id: String,
    phase: Option<String>,
    agent: Option<String>,
    checkpoint_type: String,
    checkpoint_status: String,
    prompt: String,
    answered_by: String,
    answer_text: String,
    decision_json: Option<serde_json::Value>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct RunReasoningSnapshotResponse {
    pub run_id: String,
    pub run_status: String,
    pub current_phase: Option<String>,
    pub decision_count: usize,
    pub checkpoint_answer_count: usize,
    pub open_checkpoint_count: usize,
    pub recent_decisions: Vec<DecisionRecord>,
    pub recent_checkpoint_answers: Vec<ReasoningCheckpointAnswerView>,
}

#[derive(Debug, Serialize)]
struct OperatorModelSessionResponse {
    profile: OperatorModelProfile,
    session: OperatorModelSession,
    thread: ChatThread,
    export_root: String,
    reused_existing_session: bool,
}

#[derive(Debug, Serialize)]
struct OperatorModelProfileResponse {
    profile: OperatorModelProfile,
    export_root: String,
    active_session: Option<OperatorModelSession>,
    active_thread: Option<ChatThread>,
    light_global_topics: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SoulKernelResponse {
    self_name: String,
    kernel: SoulBootstrapDocument,
}

#[derive(Debug, Deserialize)]
struct StartOperatorModelSessionRequest {
    project_root: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    started_by: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default = "default_resume_if_exists")]
    resume_if_exists: bool,
}

#[derive(Debug, Deserialize)]
struct ListOperatorModelProfilesQuery {
    #[serde(default)]
    scope: Option<OperatorModelScope>,
}

#[derive(Debug, Deserialize)]
struct MemoryBoardStaleStatusArtifact {
    #[serde(default)]
    entries: Vec<MemoryBoardStaleStatusEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct MemoryBoardStaleStatusEntry {
    memory_id: String,
    #[serde(default)]
    severity: String,
    #[serde(default)]
    severity_score: i32,
    #[serde(default)]
    mitigation_steps: Vec<String>,
    #[serde(default)]
    related_checks: Vec<String>,
    #[serde(default)]
    status: String,
    #[serde(default)]
    evidence: Vec<String>,
    #[serde(default)]
    previous_severity_score: Option<i32>,
    #[serde(default)]
    risk_reduced_from_previous: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MemoryBoardUpdateArtifact {
    #[serde(default)]
    entries: Vec<MemoryBoardUpdateEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct MemoryBoardUpdateEntry {
    #[serde(default)]
    relation: String,
    #[serde(default)]
    stale_memory_id: String,
    #[serde(default)]
    stale_summary: String,
    #[serde(default)]
    fresh_memory_id: String,
    #[serde(default)]
    fresh_summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Assignment {
    pub agent: String,
    pub task: String,
    #[serde(default)]
    pub files: Vec<String>,
    pub claimed_at: String,
    #[serde(default)]
    pub last_heartbeat_at: String,
    #[serde(default = "default_assignment_status")]
    pub status: String,
    // ── ActionLease fields (Phase A3) ─────────────────────────────────────────
    /// What kind of resource is claimed: "file", "workspace", "external", "agent"
    #[serde(default = "default_resource_kind")]
    pub resource_kind: String,
    /// Seconds until Keeper auto-reaps this lease (0 = use global stale_after_seconds)
    #[serde(default)]
    pub ttl_secs: i64,
    /// Constraints that must hold for any action against this resource.
    /// Agents should call POST /api/coordination/check-lease before acting.
    #[serde(default)]
    pub guardrails: Vec<String>,
    /// When this lease expires (computed from claimed_at + ttl_secs, empty if no TTL)
    #[serde(default)]
    pub expires_at: String,
}

fn default_resource_kind() -> String {
    "file".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AssignmentsState {
    #[serde(default = "default_coordination_owner")]
    pub managed_by: String,
    #[serde(default = "default_policy_mode")]
    pub policy_mode: String,
    #[serde(default = "default_stale_after_seconds")]
    pub stale_after_seconds: i64,
    pub active: HashMap<String, Assignment>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationPolicyEvent {
    pub event_id: String,
    pub managed_by: String,
    pub event_type: String,
    pub status: String,
    pub agent: Option<String>,
    pub conflicting_agent: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
struct CoordinationConflictResponse {
    managed_by: String,
    policy_mode: String,
    event_type: String,
    requested_agent: String,
    conflicting_agent: String,
    conflicting_files: Vec<String>,
    message: String,
}

#[derive(Debug, Serialize)]
struct DirectoryEntry {
    name: String,
    path: String,
}

#[derive(Debug, Serialize)]
struct DirectoryBrowseResponse {
    current_path: String,
    parent_path: Option<String>,
    directories: Vec<DirectoryEntry>,
}

#[derive(Debug, Deserialize)]
struct DirectoryBrowseQuery {
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ClaimRequest {
    agent: String,
    task: String,
    #[serde(default)]
    files: Vec<String>,
    // ActionLease fields (Phase A3)
    #[serde(default = "default_resource_kind")]
    resource_kind: String,
    /// 0 = use server-side stale_after_seconds
    #[serde(default)]
    ttl_secs: i64,
    #[serde(default)]
    guardrails: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct CheckLeaseRequest {
    /// The agent that wants to act
    agent: String,
    /// The file or resource being acted upon
    resource: String,
    /// Short description of the action (for audit trail)
    action: String,
}

#[derive(Debug, Serialize)]
struct CheckLeaseResponse {
    allowed: bool,
    owner: Option<String>,
    guardrail_violations: Vec<String>,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseRequest {
    agent: String,
}

#[derive(Debug, Serialize)]
struct SimpleOperationResponse {
    ok: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct RunReportResponse {
    report: String,
}

#[derive(Debug, Serialize)]
struct RunPackageResponse {
    path: String,
}

#[derive(Debug, Deserialize)]
struct SpecValidateRequest {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    spec_yaml: Option<String>,
}

#[derive(Debug, Serialize)]
struct SpecValidateResponse {
    valid: bool,
    spec_id: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct SetupCheckProviderStatus {
    name: String,
    enabled: bool,
    api_key_env: String,
    configured: bool,
    model: String,
}

#[derive(Debug, Serialize)]
struct SetupCheckMcpStatus {
    name: String,
    command: String,
    available: bool,
    aliases: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SetupCheckMcpSelfStatus {
    enabled: bool,
    transport: String,
    host: Option<String>,
    port: Option<u16>,
    auth_required: Option<bool>,
}

#[derive(Debug, Serialize)]
struct SetupCheckResponse {
    setup_name: String,
    platform: String,
    default_provider: String,
    providers: Vec<SetupCheckProviderStatus>,
    agent_routes: HashMap<String, String>,
    mcp_servers: Vec<SetupCheckMcpStatus>,
    mcp_self: Option<SetupCheckMcpSelfStatus>,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    agent: String,
}

#[derive(Debug, Deserialize)]
struct CheckpointReplyRequest {
    #[serde(default)]
    answered_by: Option<String>,
    #[serde(default)]
    answer_text: String,
    #[serde(default)]
    decision_json: Option<serde_json::Value>,
    #[serde(default)]
    resolve: bool,
}

#[derive(Debug, Deserialize)]
struct AgentUnblockRequest {
    run_id: String,
    #[serde(default)]
    checkpoint_id: Option<String>,
    #[serde(default)]
    answered_by: Option<String>,
    #[serde(default)]
    answer_text: Option<String>,
    #[serde(default)]
    decision_json: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AgentUnblockResponse {
    run_id: String,
    agent: String,
    resolved: usize,
    checkpoints: Vec<RunCheckpointRecord>,
}

#[derive(Debug, Deserialize)]
struct CoobieQueryRequest {
    message: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    retrieval_depth: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct AgentChatRequest {
    message: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    retrieval_depth: Option<u8>,
}

#[derive(Debug, Serialize)]
struct CoobieQueryResponse {
    agent: String,
    response: String,
    retrieval_path: Vec<String>,
    confidence: f64,
    sources: Vec<CoobieQuerySource>,
}

#[derive(Debug, Serialize)]
struct CoobieQuerySource {
    kind: String,
    label: String,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    phase: Option<String>,
    #[serde(default)]
    artifact: Option<String>,
    #[serde(default)]
    hop: Option<u8>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    score: Option<f64>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    superseded_by: Option<String>,
    #[serde(default)]
    challenged_by: Vec<String>,
    #[serde(default)]
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QueryTargetSourceMetadata {
    source_path: String,
}

#[derive(Debug, Clone)]
struct ScopedMemoryHit {
    scope: String,
    hop: u8,
    query: String,
    hit: MemoryRetrievalHit,
}

#[derive(Debug, Deserialize)]
struct EvidenceBundlesQuery {
    project_root: String,
}

#[derive(Debug, Deserialize)]
struct EvidenceBundleQuery {
    project_root: String,
}

#[derive(Debug, Deserialize)]
struct EvidenceHistoryQuery {
    project_root: String,
    bundle_name: String,
    #[serde(default)]
    annotation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EvidenceBundleSaveRequest {
    project_root: String,
    bundle_name: String,
    bundle: EvidenceAnnotationBundle,
}

#[derive(Debug, Deserialize)]
struct EvidenceAnnotationUpsertRequest {
    project_root: String,
    bundle_name: String,
    #[serde(default)]
    scenario: Option<String>,
    #[serde(default)]
    dataset: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
    #[serde(default)]
    sources: Vec<EvidenceSource>,
    annotation: EvidenceAnnotation,
}

#[derive(Debug, Deserialize)]
struct EvidenceAnnotationReviewRequest {
    project_root: String,
    bundle_name: String,
    annotation_id: String,
    status: String,
    #[serde(default)]
    reviewed_by: Option<String>,
    #[serde(default)]
    review_note: Option<String>,
    #[serde(default)]
    promote_scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryUpdateReviewRequest {
    status: String,
    #[serde(default)]
    reviewed_by: Option<String>,
    #[serde(default)]
    review_note: Option<String>,
}

#[derive(Debug, Serialize)]
struct EvidenceBundleSaveResponse {
    bundle_name: String,
    path: String,
    bundle: EvidenceAnnotationBundle,
}

#[derive(Debug, Serialize)]
struct EvidenceAnnotationReviewResponse {
    bundle_name: String,
    path: String,
    annotation_id: String,
    status: String,
    promoted_ids: Vec<String>,
    skipped_annotations: Vec<String>,
    bundle: EvidenceAnnotationBundle,
}

#[derive(Debug, Deserialize)]
struct SimilarEvidenceQuery {
    project_root: String,
    #[serde(default)]
    spec_id: Option<String>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    labels: Option<String>,
    #[serde(default)]
    claims: Option<String>,
    #[serde(default)]
    sources: Option<String>,
    #[serde(default)]
    time_span_ms: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct EvidenceMatchWindowInput {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    annotation_type: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    claims: Vec<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    start_ms: Option<i64>,
    #[serde(default)]
    end_ms: Option<i64>,
    #[serde(default)]
    time_span_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EvidenceMatchReportRequest {
    project_root: String,
    #[serde(default)]
    spec_id: Option<String>,
    #[serde(default)]
    query_terms: Vec<String>,
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    claims: Vec<String>,
    #[serde(default)]
    sources: Vec<String>,
    #[serde(default)]
    time_span_ms: Option<i64>,
    #[serde(default)]
    limit: Option<usize>,
    #[serde(default)]
    selected_window: Option<EvidenceMatchWindowInput>,
}

pub async fn start_api_server(app: AppContext, port: u16) -> anyhow::Result<()> {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let router = Router::new()
        .route("/api/runs", get(list_runs))
        .route("/api/runs/:id", get(get_run))
        .route("/api/runs/:id/events", get(get_run_events))
        .route("/api/runs/:id/events/stream", get(get_run_events_stream))
        .route("/api/runs/:id/blackboard", get(get_run_blackboard))
        .route(
            "/api/runs/:id/blackboard/:role",
            get(get_run_blackboard_for_role),
        )
        .route("/api/runs/:id/board/mission", get(get_run_mission_board))
        .route("/api/runs/:id/board/action", get(get_run_action_board))
        .route("/api/runs/:id/board/evidence", get(get_run_evidence_board))
        .route("/api/runs/:id/board/memory", get(get_run_memory_board))
        .route("/api/runs/:id/checkpoints", get(get_run_checkpoints))
        .route(
            "/api/runs/:id/checkpoints/:checkpoint_id/reply",
            post(post_run_checkpoint_reply),
        )
        .route("/api/runs/:id/lessons", get(get_run_lessons))
        .route("/api/runs/:id/state", get(get_run_state))
        .route("/api/runs/:id/consolidate", post(post_run_consolidate))
        .route(
            "/api/runs/:id/consolidation/candidates",
            get(get_consolidation_candidates).post(post_generate_candidates),
        )
        .route(
            "/api/runs/:id/consolidation/candidates/:cid/keep",
            post(post_candidate_keep),
        )
        .route(
            "/api/runs/:id/consolidation/candidates/:cid/discard",
            post(post_candidate_discard),
        )
        .route(
            "/api/runs/:id/consolidation/candidates/:cid/edit",
            post(post_candidate_edit),
        )
        .route(
            "/api/runs/:id/memory/candidates",
            get(get_memory_candidates).post(post_process_memory_candidates),
        )
        .route(
            "/api/runs/:id/memory/candidates/retry",
            post(post_process_memory_candidates),
        )
        .route(
            "/api/runs/:id/memory/candidates/:cid/approve",
            post(post_approve_memory_candidate),
        )
        .route(
            "/api/runs/:id/memory/candidates/:cid/discard",
            post(post_discard_memory_candidate),
        )
        .route(
            "/api/runs/:id/code-review-learning",
            get(get_code_review_learning_records),
        )
        .route(
            "/api/runs/:id/plan-completion-audit",
            get(get_plan_completion_audit),
        )
        .route("/api/chat", post(post_chat))
        .route("/api/coobie/query", post(post_coobie_query))
        .route("/api/agents/:id/chat", post(post_agent_chat))
        .route("/api/agents/:id/unblock", post(post_agent_unblock))
        .route(
            "/api/chat/threads",
            get(list_chat_threads).post(post_open_thread),
        )
        .route("/api/chat/threads/:id", get(get_chat_thread))
        .route(
            "/api/chat/threads/:id/messages",
            get(list_chat_messages).post(post_chat_message),
        )
        .route(
            "/api/operator-model/sessions",
            post(post_start_operator_model_session),
        )
        .route(
            "/api/operator-model/profiles",
            get(list_operator_model_profiles),
        )
        .route(
            "/api/operator-model/profiles/:id",
            get(get_operator_model_profile),
        )
        .route(
            "/api/operator-model/sessions/:id",
            get(get_operator_model_session),
        )
        .route(
            "/api/operator-model/sessions/:id/approve-layer",
            post(post_approve_operator_model_layer),
        )
        .route(
            "/api/operator-model/profiles/:id/commissioning-brief",
            get(get_operator_model_commissioning_brief),
        )
        .route("/api/soul/:id", get(get_soul_kernel))
        .route("/api/soul/:id/guide", get(get_soul_guide))
        .route("/api/runs/:id/coobie-briefing", get(get_coobie_briefing))
        .route("/api/runs/:id/coobie-response", get(get_coobie_response))
        .route("/api/runs/:id/coobie-signals", get(get_coobie_signals))
        .route("/api/runs/:id/causal-report", get(get_causal_report))
        .route("/api/runs/:id/causal-events", get(get_run_causal_events))
        .route("/api/runs/:id/cost", get(get_run_cost))
        .route("/api/runs/:id/decisions", get(get_run_decisions))
        .route("/api/runs/:id/traces", get(get_run_traces))
        .route(
            "/api/runs/:id/optimization-program",
            get(get_run_optimization_program),
        )
        .route("/api/runs/:id/metric-attacks", get(get_run_metric_attacks))
        .route(
            "/api/runs/:id/evidence-match-report",
            get(get_run_evidence_match_report),
        )
        .route("/api/evidence/bundles", get(list_evidence_bundles))
        .route("/api/evidence/bundles/:name", get(get_evidence_bundle))
        .route("/api/evidence/history", get(get_evidence_history))
        .route(
            "/api/evidence/bundles/save",
            post(post_evidence_bundle_save),
        )
        .route(
            "/api/evidence/annotations/upsert",
            post(post_evidence_annotation_upsert),
        )
        .route(
            "/api/evidence/annotations/review",
            post(post_evidence_annotation_review),
        )
        .route("/api/evidence/similar", get(get_similar_evidence_windows))
        .route(
            "/api/evidence/match-report",
            post(post_evidence_match_report),
        )
        .route("/api/fs/directories", get(get_directory_browser))
        .route("/api/capacity", get(get_capacity))
        .route("/api/tesseract/scene", get(get_tesseract_scene))
        .route("/api/setup/check", get(get_setup_check))
        .route("/api/spec/validate", post(post_spec_validate))
        .route("/api/memory/init", post(post_memory_init))
        .route("/api/memory/index", post(post_memory_index))
        .route("/api/memory/updates", get(get_memory_updates))
        .route(
            "/api/memory/updates/:id/review",
            post(post_memory_update_review),
        )
        .route("/api/runs/start", post(start_run))
        .route("/api/runs/:id/report", get(get_run_report))
        .route("/api/runs/:id/package", post(post_run_package))
        .route("/api/runs/:id/artifacts", get(list_run_artifacts))
        .route("/api/runs/:id/artifacts/:name", get(get_run_artifact))
        .route("/api/runs/:id/memory-note", post(add_memory_note))
        .route("/api/scout/draft", post(scout_draft))
        .route("/api/coordination/assignments", get(get_assignments))
        .route(
            "/api/coordination/policy-events",
            get(get_coordination_policy_events),
        )
        .route("/api/coordination/claim", post(claim_task))
        .route("/api/coordination/check-lease", post(check_lease))
        .route("/api/coordination/heartbeat", post(heartbeat_task))
        .route("/api/coordination/release", post(release_task))
        .route("/health", get(get_health))
        .route("/api/status", get(get_server_status))
        .layer(cors)
        .with_state(app);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    info!("API server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}

async fn list_runs(State(app): State<AppContext>) -> impl IntoResponse {
    match app.list_runs(50).await {
        Ok(runs) => (StatusCode::OK, Json(runs)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run(Path(id): Path<String>, State(app): State<AppContext>) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(run)) => (StatusCode::OK, Json(run)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_soul_kernel(Path(id): Path<String>) -> impl IntoResponse {
    if !crate::calvin_archive::supported_self(&id) {
        return (StatusCode::NOT_FOUND, "soul baseline not found").into_response();
    }
    let kernel = crate::calvin_archive::coobie_identity();
    (
        StatusCode::OK,
        Json(SoulKernelResponse {
            self_name: id,
            kernel,
        }),
    )
        .into_response()
}

async fn get_soul_guide(Path(id): Path<String>) -> impl IntoResponse {
    if !crate::calvin_archive::supported_self(&id) {
        return (StatusCode::NOT_FOUND, "soul guide not found").into_response();
    }
    let markdown =
        crate::calvin_archive::render_guide_markdown(&crate::calvin_archive::coobie_identity());
    (
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "text/markdown; charset=utf-8",
        )],
        markdown,
    )
        .into_response()
}

async fn get_directory_browser(
    State(app): State<AppContext>,
    Query(query): Query<DirectoryBrowseQuery>,
) -> impl IntoResponse {
    let requested = query
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let current = match requested {
        Some(path) => {
            let candidate = PathBuf::from(path);
            if candidate.is_absolute() {
                candidate
            } else {
                app.paths.root.join(candidate)
            }
        }
        None => app.paths.products.clone(),
    };

    let current = match current.canonicalize() {
        Ok(path) => path,
        Err(error) => return (StatusCode::BAD_REQUEST, error.to_string()).into_response(),
    };

    if !current.is_dir() {
        return (StatusCode::BAD_REQUEST, "directory path is not a folder").into_response();
    }

    let mut directories = Vec::new();
    let read_dir = match fs::read_dir(&current) {
        Ok(iter) => iter,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .to_str()
            .map(|value| value.to_string())
            .unwrap_or_else(|| path.display().to_string());
        directories.push(DirectoryEntry {
            name,
            path: path.display().to_string(),
        });
    }

    directories.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let response = DirectoryBrowseResponse {
        current_path: current.display().to_string(),
        parent_path: current.parent().map(|value| value.display().to_string()),
        directories,
    };

    (StatusCode::OK, Json(response)).into_response()
}

async fn get_run_events(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.list_run_events(&id).await {
        Ok(events) => (StatusCode::OK, Json(events)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// SSE endpoint — streams `LiveEvent` values as they happen for a given run.
///
/// Each SSE `data` field is a JSON-encoded `LiveEvent`.  The stream stays open
/// until the client disconnects; a 15-second keepalive comment is sent to
/// prevent proxy timeouts.
async fn get_run_events_stream(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = app.event_tx.subscribe();
    let run_id = id.clone();
    let stream = BroadcastStream::new(rx).filter_map(move |msg| {
        let run_id = run_id.clone();
        match msg {
            Ok(live_event) => {
                // Only forward events that belong to this run.
                let matches = match &live_event {
                    crate::models::LiveEvent::RunEvent(e) => e.run_id == run_id,
                    crate::models::LiveEvent::BuildOutput { run_id: rid, .. } => *rid == run_id,
                };
                if matches {
                    match serde_json::to_string(&live_event) {
                        Ok(json) => Some(Ok(Event::default().data(json))),
                        Err(_) => None,
                    }
                } else {
                    None
                }
            }
            // Lagged receiver — skip the missed entries and continue.
            Err(_) => None,
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

async fn get_run_blackboard(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let run_dir = app.paths.workspaces.join(&id).join("run");
            let blackboard_path = run_dir.join("blackboard.json");
            match read_optional_json::<BlackboardState>(&blackboard_path).await {
                Ok(Some(board)) => (StatusCode::OK, Json(board)).into_response(),
                Ok(None) => (StatusCode::NOT_FOUND, "Blackboard not found").into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_blackboard_for_role(
    Path((id, role)): Path<(String, String)>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let run_dir = app.paths.workspaces.join(&id).join("run");
            match read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await {
                Ok(Some(board)) => (StatusCode::OK, Json(board.role_view(&role))).into_response(),
                Ok(None) => (StatusCode::NOT_FOUND, "Blackboard not found").into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_lessons(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let run_dir = app.paths.workspaces.join(&id).join("run");
            match read_optional_json::<Vec<LessonRecord>>(&run_dir.join("lessons.json")).await {
                Ok(Some(lessons)) => (StatusCode::OK, Json(lessons)).into_response(),
                Ok(None) => (StatusCode::OK, Json(Vec::<LessonRecord>::new())).into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_state(Path(id): Path<String>, State(app): State<AppContext>) -> impl IntoResponse {
    match build_run_state(&app, &id).await {
        Ok(Some(state)) => (StatusCode::OK, Json(state)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_mission_board(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_mission_board(&app, &id).await {
        Ok(Some(board)) => (StatusCode::OK, Json(board)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_action_board(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_action_board(&app, &id).await {
        Ok(Some(board)) => (StatusCode::OK, Json(board)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_evidence_board(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_evidence_board(&app, &id).await {
        Ok(Some(board)) => (StatusCode::OK, Json(board)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_memory_board(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_memory_board(&app, &id).await {
        Ok(Some(board)) => (StatusCode::OK, Json(board)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_checkpoints(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.list_run_checkpoints(&id).await {
            Ok(checkpoints) => (StatusCode::OK, Json(checkpoints)).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_run_checkpoint_reply(
    Path((id, checkpoint_id)): Path<(String, String)>,
    State(app): State<AppContext>,
    Json(request): Json<CheckpointReplyRequest>,
) -> impl IntoResponse {
    let answered_by = request
        .answered_by
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("operator");
    match app.get_run(&id).await {
        Ok(Some(_)) => match app
            .reply_to_checkpoint(
                &id,
                &checkpoint_id,
                answered_by,
                &request.answer_text,
                request.decision_json,
                request.resolve,
            )
            .await
        {
            Ok(checkpoint) => (StatusCode::OK, Json(checkpoint)).into_response(),
            Err(error) => {
                let message = error.to_string();
                let status = if message.contains("not found") {
                    StatusCode::NOT_FOUND
                } else if message.contains("need answer_text or decision_json") {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                (status, message).into_response()
            }
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_chat(
    State(app): State<AppContext>,
    Json(request): Json<CoobieQueryRequest>,
) -> impl IntoResponse {
    match execute_coobie_query(
        &app,
        request.run_id.as_deref(),
        &request.message,
        normalize_retrieval_depth(request.retrieval_depth),
    )
    .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_coobie_query(
    State(app): State<AppContext>,
    Json(request): Json<CoobieQueryRequest>,
) -> impl IntoResponse {
    match execute_coobie_query(
        &app,
        request.run_id.as_deref(),
        &request.message,
        normalize_retrieval_depth(request.retrieval_depth),
    )
    .await
    {
        Ok(response) => (StatusCode::OK, Json(response)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_agent_chat(
    Path(agent): Path<String>,
    State(app): State<AppContext>,
    Json(request): Json<AgentChatRequest>,
) -> impl IntoResponse {
    if agent.eq_ignore_ascii_case("coobie") {
        match execute_coobie_query(
            &app,
            request.run_id.as_deref(),
            &request.message,
            normalize_retrieval_depth(request.retrieval_depth),
        )
        .await
        {
            Ok(response) => (StatusCode::OK, Json(response)).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        }
    } else {
        let label = agent.to_lowercase();
        let response = CoobieQueryResponse {
            agent: label.clone(),
            response: format!(
                "{} direct chat is not live yet. Coobie can still answer pack-level causal and memory questions in the meantime.",
                title_case_agent(&label)
            ),
            retrieval_path: vec!["working_memory".to_string()],
            confidence: 0.35,
            sources: Vec::new(),
        };
        (StatusCode::OK, Json(response)).into_response()
    }
}

async fn post_agent_unblock(
    Path(agent): Path<String>,
    State(app): State<AppContext>,
    Json(request): Json<AgentUnblockRequest>,
) -> impl IntoResponse {
    let answered_by = request
        .answered_by
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("operator");
    match app.get_run(&request.run_id).await {
        Ok(Some(_)) => match app
            .unblock_agent_checkpoints(
                &request.run_id,
                &agent,
                request.checkpoint_id.as_deref(),
                answered_by,
                request.answer_text.as_deref(),
                request.decision_json,
            )
            .await
        {
            Ok(checkpoints) => (
                StatusCode::OK,
                Json(AgentUnblockResponse {
                    run_id: request.run_id,
                    agent,
                    resolved: checkpoints.len(),
                    checkpoints,
                }),
            )
                .into_response(),
            Err(error) => {
                let message = error.to_string();
                let status = if message.contains("not open") {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                };
                (status, message).into_response()
            }
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_run_consolidate(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        // If candidates exist and some are kept, only promote those.
        // Otherwise fall back to the legacy auto-promote path so old clients
        // that call /consolidate directly still work.
        Ok(Some(_)) => {
            if let Err(error) = app.process_memory_candidates(Some(&id), 100).await {
                tracing::warn!(run_id = %id, error = %error, "memory candidate processing skipped before consolidation");
            }
            match app.promote_kept_candidates(&id).await {
                Ok(new_lessons) => match build_memory_board(&app, &id).await {
                    Ok(Some(memory_board)) => (
                        StatusCode::OK,
                        Json(ConsolidateRunResponse {
                            run_id: id,
                            total_new_lessons: new_lessons.len(),
                            new_lessons,
                            memory_board,
                        }),
                    )
                        .into_response(),
                    Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
                    Err(error) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                    }
                },
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

// ── Phase 5 — Consolidation Workbench ────────────────────────────────────────

#[derive(Debug, Serialize)]
struct CandidatesResponse {
    run_id: String,
    total: usize,
    pending: usize,
    kept: usize,
    discarded: usize,
    candidates: Vec<ConsolidationCandidate>,
}

#[derive(Debug, Deserialize)]
struct EditCandidateRequest {
    content: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct ProcessMemoryCandidatesRequest {
    #[serde(default)]
    limit: Option<usize>,
}

/// `GET /api/runs/:id/consolidation/candidates` — list all candidates.
async fn get_consolidation_candidates(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.list_consolidation_candidates(&id).await {
            Ok(candidates) => {
                let total = candidates.len();
                let pending = candidates.iter().filter(|c| c.status == "pending").count();
                let kept = candidates.iter().filter(|c| c.status == "kept").count();
                let discarded = candidates
                    .iter()
                    .filter(|c| c.status == "discarded")
                    .count();
                (
                    StatusCode::OK,
                    Json(CandidatesResponse {
                        run_id: id,
                        total,
                        pending,
                        kept,
                        discarded,
                        candidates,
                    }),
                )
                    .into_response()
            }
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// `POST /api/runs/:id/consolidation/candidates` — generate candidates.
async fn post_generate_candidates(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.generate_consolidation_candidates(&id).await {
            Ok(new_candidates) => match app.list_consolidation_candidates(&id).await {
                Ok(all_candidates) => {
                    let total = all_candidates.len();
                    let pending = all_candidates
                        .iter()
                        .filter(|c| c.status == "pending")
                        .count();
                    let kept = all_candidates.iter().filter(|c| c.status == "kept").count();
                    let discarded = all_candidates
                        .iter()
                        .filter(|c| c.status == "discarded")
                        .count();
                    (
                        StatusCode::OK,
                        Json(serde_json::json!({
                            "run_id": id,
                            "new_candidates": new_candidates.len(),
                            "total": total,
                            "pending": pending,
                            "kept": kept,
                            "discarded": discarded,
                            "candidates": all_candidates,
                        })),
                    )
                        .into_response()
                }
                Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
            },
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// `POST /api/runs/:id/consolidation/candidates/:cid/keep`
async fn post_candidate_keep(
    Path((id, cid)): Path<(String, String)>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.review_consolidation_candidate(&cid, "kept").await {
            Ok(_) => (
                StatusCode::OK,
                Json(serde_json::json!({"status": "kept", "candidate_id": cid})),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// `POST /api/runs/:id/consolidation/candidates/:cid/discard`
async fn post_candidate_discard(
    Path((id, cid)): Path<(String, String)>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.review_consolidation_candidate(&cid, "discarded").await {
            Ok(_) => (
                StatusCode::OK,
                Json(serde_json::json!({"status": "discarded", "candidate_id": cid})),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

/// `POST /api/runs/:id/consolidation/candidates/:cid/edit`
async fn post_candidate_edit(
    Path((id, cid)): Path<(String, String)>,
    State(app): State<AppContext>,
    Json(body): Json<EditCandidateRequest>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.edit_consolidation_candidate(&cid, body.content).await {
            Ok(_) => (
                StatusCode::OK,
                Json(serde_json::json!({"status": "kept", "candidate_id": cid})),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_memory_candidates(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.list_memory_candidates_for_run(&id).await {
            Ok(candidates) => {
                let total = candidates.len();
                let mut status_counts = HashMap::<String, usize>::new();
                let mut source_authority_counts = HashMap::<String, usize>::new();
                for candidate in &candidates {
                    *status_counts.entry(candidate.status.clone()).or_default() += 1;
                    *source_authority_counts
                        .entry(candidate.source_authority.clone())
                        .or_default() += 1;
                }
                let pending = status_counts.get("pending").copied().unwrap_or(0);
                let retry_pending = status_counts.get("retry_pending").copied().unwrap_or(0);
                let waiting_openbrain =
                    status_counts.get("waiting_openbrain").copied().unwrap_or(0);
                let held_for_review = status_counts.get("held_for_review").copied().unwrap_or(0);
                let needs_reconsolidation = status_counts
                    .get("needs_reconsolidation")
                    .copied()
                    .unwrap_or(0);
                let captured_openbrain = status_counts
                    .get("captured_openbrain")
                    .copied()
                    .unwrap_or(0);
                let promotion_pending =
                    status_counts.get("promotion_pending").copied().unwrap_or(0);
                let duplicate_openbrain = status_counts
                    .get("duplicate_openbrain")
                    .copied()
                    .unwrap_or(0);
                let missing_evidence_refs = candidates
                    .iter()
                    .filter(|candidate| {
                        candidate
                            .evidence_refs
                            .as_array()
                            .is_none_or(|refs| refs.is_empty())
                    })
                    .count();
                let actionable = retry_pending
                    + waiting_openbrain
                    + held_for_review
                    + needs_reconsolidation
                    + promotion_pending;
                let retryable = pending + retry_pending + waiting_openbrain;
                let mut memory_chain_blockers = Vec::new();
                if held_for_review > 0 {
                    memory_chain_blockers.push(format!(
                        "{held_for_review} candidate{} held for operator review",
                        plural_suffix(held_for_review)
                    ));
                }
                if retry_pending > 0 {
                    memory_chain_blockers.push(format!(
                        "{retry_pending} candidate{} waiting for retry",
                        plural_suffix(retry_pending)
                    ));
                }
                if waiting_openbrain > 0 {
                    memory_chain_blockers.push(format!(
                        "{waiting_openbrain} candidate{} waiting for OB1 configuration",
                        plural_suffix(waiting_openbrain)
                    ));
                }
                if needs_reconsolidation > 0 {
                    memory_chain_blockers.push(format!(
                        "{needs_reconsolidation} memory candidate{} need reconsolidation",
                        plural_suffix(needs_reconsolidation)
                    ));
                }
                if promotion_pending > 0 {
                    memory_chain_blockers.push(format!(
                        "{promotion_pending} Calvin promotion{} pending review",
                        plural_suffix(promotion_pending)
                    ));
                }
                if missing_evidence_refs > 0 {
                    memory_chain_blockers.push(format!(
                        "{missing_evidence_refs} candidate{} missing evidence refs",
                        plural_suffix(missing_evidence_refs)
                    ));
                }
                let memory_chain_status = if held_for_review > 0 {
                    "needs_review"
                } else if retry_pending > 0 {
                    "retry_pending"
                } else if waiting_openbrain > 0 {
                    "waiting_openbrain"
                } else if needs_reconsolidation > 0 {
                    "needs_reconsolidation"
                } else if pending > 0 {
                    "processing"
                } else if promotion_pending > 0 {
                    "calvin_review"
                } else {
                    "clear"
                };
                let backlog_total = pending
                    + retry_pending
                    + waiting_openbrain
                    + held_for_review
                    + needs_reconsolidation
                    + promotion_pending;
                let memory_chain_health_status = if retry_pending > 0
                    || waiting_openbrain > 0
                    || held_for_review > 0
                    || needs_reconsolidation > 0
                    || missing_evidence_refs > 0
                {
                    "blocked"
                } else if backlog_total > 0 || duplicate_openbrain > 0 {
                    "degraded"
                } else {
                    "clear"
                };
                let memory_chain_health = serde_json::json!({
                    "schema": "harkonnen.memory_chain_health.v1",
                    "status": memory_chain_health_status,
                    "backlog": {
                        "total": backlog_total,
                        "pending": pending,
                        "retry_pending": retry_pending,
                        "waiting_openbrain": waiting_openbrain,
                        "held_for_review": held_for_review,
                        "needs_reconsolidation": needs_reconsolidation,
                        "promotion_pending": promotion_pending,
                        "retryable": retryable,
                        "actionable": actionable,
                    },
                    "quality": {
                        "stale_claims": needs_reconsolidation,
                        "duplicate_openbrain": duplicate_openbrain,
                        "missing_evidence_refs": missing_evidence_refs,
                        "source_authority_counts": source_authority_counts.clone(),
                    },
                    "review_load": {
                        "operator_review": held_for_review,
                        "calvin_review": promotion_pending,
                        "reconsolidation_review": needs_reconsolidation,
                    },
                    "service_readiness": {
                        "twilight_bark_packchat": {
                            "configured": app.paths.setup.twilight_bark.enabled,
                            "transport": if app.paths.setup.twilight_bark.enabled { "twilight_bark" } else { "local_sqlite" },
                            "openziti_service": "twilight-bark.packchat",
                        },
                        "openbrain_mcp": {
                            "enabled": app.paths.setup.open_brain.enabled,
                            "configured": app.open_brain.is_some(),
                            "openziti_service": "openbrain.mcp",
                        },
                        "calvin_archive": {
                            "enabled": app.paths.setup.calvin_archive.enabled,
                            "configured": app.calvin.is_some(),
                            "openziti_service": "calvin.archive",
                        },
                        "harkonnen_api": {
                            "enabled": true,
                            "configured": true,
                            "openziti_service": "harkonnen.api",
                        },
                    },
                    "blockers": memory_chain_blockers.clone(),
                });
                (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "run_id": id,
                        "total": total,
                        "memory_chain_status": memory_chain_status,
                        "memory_chain_blockers": memory_chain_blockers,
                        "status_counts": status_counts,
                        "source_authority_counts": source_authority_counts,
                        "pending": pending,
                        "retry_pending": retry_pending,
                        "waiting_openbrain": waiting_openbrain,
                        "held_for_review": held_for_review,
                        "needs_reconsolidation": needs_reconsolidation,
                        "captured_openbrain": captured_openbrain,
                        "promotion_pending": promotion_pending,
                        "actionable": actionable,
                        "retryable": retryable,
                        "duplicate_openbrain": duplicate_openbrain,
                        "missing_evidence_refs": missing_evidence_refs,
                        "memory_chain_health": memory_chain_health,
                        "candidates": candidates,
                    })),
                )
                    .into_response()
            }
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_code_review_learning_records(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.list_code_review_learning_records(Some(&id)).await {
            Ok(records) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "run_id": id,
                    "total": records.len(),
                    "records": records,
                })),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_plan_completion_audit(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.load_plan_completion_audit(&id).await {
            Ok(audit) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "run_id": id,
                    "audit": audit,
                })),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

fn plural_suffix(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

async fn post_process_memory_candidates(
    Path(id): Path<String>,
    State(app): State<AppContext>,
    Json(body): Json<ProcessMemoryCandidatesRequest>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app
            .process_memory_candidates(Some(&id), body.limit.unwrap_or(50))
            .await
        {
            Ok(summary) => (StatusCode::OK, Json(summary)).into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn post_approve_memory_candidate(
    Path((id, cid)): Path<(String, String)>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app
            .chat
            .approve_memory_candidate_for_processing(&id, &cid)
            .await
        {
            Ok(true) => match app.process_memory_candidates(Some(&id), 25).await {
                Ok(summary) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "approved",
                        "candidate_id": cid,
                        "processing": summary,
                    })),
                )
                    .into_response(),
                Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
            },
            Ok(false) => (
                StatusCode::CONFLICT,
                "Candidate is not reviewable, retryable, or waiting for OB1",
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn post_discard_memory_candidate(
    Path((id, cid)): Path<(String, String)>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.chat.discard_memory_candidate(&id, &cid).await {
            Ok(true) => (
                StatusCode::OK,
                Json(serde_json::json!({"status": "discarded", "candidate_id": cid})),
            )
                .into_response(),
            Ok(false) => (
                StatusCode::CONFLICT,
                "Candidate is already captured/promoted or was not found",
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_coobie_briefing(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let briefing_path = app
                .paths
                .workspaces
                .join(&id)
                .join("run")
                .join("coobie_briefing.json");
            match read_optional_json::<CoobieBriefing>(&briefing_path).await {
                Ok(Some(briefing)) => (StatusCode::OK, Json(briefing)).into_response(),
                Ok(None) => {
                    (StatusCode::NOT_FOUND, "Coobie briefing not yet generated").into_response()
                }
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_coobie_response(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let run_dir = app.paths.workspaces.join(&id).join("run");
            let response = match read_optional_text(&run_dir.join("coobie_report_response.md"))
                .await
            {
                Ok(Some(text)) => Some(text),
                Ok(None) => {
                    match read_optional_text(&run_dir.join("coobie_preflight_response.md")).await {
                        Ok(text) => text,
                        Err(error) => {
                            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                                .into_response()
                        }
                    }
                }
                Err(error) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            };
            match response {
                Some(text) => (StatusCode::OK, text).into_response(),
                None => {
                    (StatusCode::NOT_FOUND, "Coobie response not yet generated").into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_coobie_signals(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let run_dir = app.paths.workspaces.join(&id).join("run");
            match load_coobie_translations(&run_dir).await {
                Ok(translations) => (StatusCode::OK, Json(translations)).into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn list_evidence_bundles(
    Query(query): Query<EvidenceBundlesQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.list_project_evidence_bundles(&query.project_root).await {
        Ok(bundles) => (StatusCode::OK, Json(bundles)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_evidence_bundle(
    Path(name): Path<String>,
    Query(query): Query<EvidenceBundleQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app
        .load_project_evidence_bundle(&query.project_root, &name)
        .await
    {
        Ok(Some(bundle)) => (StatusCode::OK, Json(bundle)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Evidence bundle not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_evidence_history(
    Query(query): Query<EvidenceHistoryQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app
        .load_project_evidence_history(
            &query.project_root,
            &query.bundle_name,
            query.annotation_id.as_deref(),
        )
        .await
    {
        Ok(history) => {
            let history: Vec<EvidenceAnnotationHistoryEvent> = history;
            (StatusCode::OK, Json(history)).into_response()
        }
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_evidence_bundle_save(
    State(app): State<AppContext>,
    Json(request): Json<EvidenceBundleSaveRequest>,
) -> impl IntoResponse {
    let bundle = request.bundle;
    match app
        .save_project_evidence_bundle(&request.project_root, &request.bundle_name, &bundle)
        .await
    {
        Ok(path) => (
            StatusCode::OK,
            Json(EvidenceBundleSaveResponse {
                bundle_name: request.bundle_name,
                path: path.display().to_string(),
                bundle,
            }),
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_evidence_annotation_upsert(
    State(app): State<AppContext>,
    Json(request): Json<EvidenceAnnotationUpsertRequest>,
) -> impl IntoResponse {
    match app
        .upsert_project_evidence_annotation(
            &request.project_root,
            &request.bundle_name,
            request.scenario.as_deref(),
            request.dataset.as_deref(),
            &request.notes,
            &request.sources,
            &request.annotation,
        )
        .await
    {
        Ok((path, bundle)) => (
            StatusCode::OK,
            Json(EvidenceBundleSaveResponse {
                bundle_name: request.bundle_name,
                path: path.display().to_string(),
                bundle,
            }),
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_evidence_annotation_review(
    State(app): State<AppContext>,
    Json(request): Json<EvidenceAnnotationReviewRequest>,
) -> impl IntoResponse {
    match app
        .review_project_evidence_annotation(
            &request.project_root,
            &request.bundle_name,
            &request.annotation_id,
            &request.status,
            request.reviewed_by.as_deref(),
            request.review_note.as_deref(),
            request.promote_scope.as_deref(),
        )
        .await
    {
        Ok((path, bundle, promotion)) => (
            StatusCode::OK,
            Json(EvidenceAnnotationReviewResponse {
                bundle_name: request.bundle_name,
                path: path.display().to_string(),
                annotation_id: request.annotation_id,
                status: request.status,
                promoted_ids: promotion.promoted_ids,
                skipped_annotations: promotion.skipped_annotations,
                bundle,
            }),
        )
            .into_response(),
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else if message.contains("unsupported evidence annotation status")
                || message.contains("annotation_id cannot be empty")
            {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, message).into_response()
        }
    }
}

async fn get_similar_evidence_windows(
    Query(query): Query<SimilarEvidenceQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    let query_terms = split_csv_field(query.query.as_deref());
    let labels = split_csv_field(query.labels.as_deref());
    let claims = split_csv_field(query.claims.as_deref());
    let sources = split_csv_field(query.sources.as_deref());
    match app
        .search_similar_evidence_windows(
            &query.project_root,
            query.spec_id.as_deref(),
            &query_terms,
            &labels,
            &claims,
            &sources,
            query.time_span_ms,
            query.limit.unwrap_or(5),
        )
        .await
    {
        Ok(matches) => (StatusCode::OK, Json(matches)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_causal_report(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let report_path = app
                .paths
                .workspaces
                .join(&id)
                .join("run")
                .join("causal_report.json");
            match read_optional_json::<CausalReport>(&report_path).await {
                Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
                Ok(None) => {
                    (StatusCode::NOT_FOUND, "Causal report not yet generated").into_response()
                }
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_causal_events(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.get_run_causal_graph(&id).await {
            Ok(graph) => (StatusCode::OK, Json(graph)).into_response(),
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `GET /api/runs/:id/cost` — aggregate token/latency cost for a run.
async fn get_run_cost(Path(id): Path<String>, State(app): State<AppContext>) -> impl IntoResponse {
    match app.get_run_cost_summary(&id).await {
        Ok(summary) => (StatusCode::OK, Json(summary)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `GET /api/runs/:id/decisions` — decision log for a run.
async fn get_run_decisions(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.list_run_decisions(&id).await {
        Ok(decisions) => (StatusCode::OK, Json(decisions)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `GET /api/runs/:id/traces` — agent trace spine for a run.
async fn get_run_traces(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.list_run_traces(&id).await {
        Ok(traces) => (StatusCode::OK, Json(traces)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `GET /api/runs/:id/optimization-program` — machine-readable success metric for a run.
async fn get_run_optimization_program(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    let program_path = app
        .paths
        .workspaces
        .join(&id)
        .join("run")
        .join("optimization_program.json");
    match read_optional_json::<OptimizationProgram>(&program_path).await {
        Ok(Some(program)) => (StatusCode::OK, Json(program)).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            "OptimizationProgram not yet generated",
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `GET /api/runs/:id/metric-attacks` — Sable's red-team attacks against the objective metric.
async fn get_run_metric_attacks(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    let attacks_path = app
        .paths
        .workspaces
        .join(&id)
        .join("run")
        .join("metric_attacks.json");
    match read_optional_json::<Vec<MetricAttack>>(&attacks_path).await {
        Ok(Some(attacks)) => (StatusCode::OK, Json(attacks)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Metric attacks not yet generated").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_evidence_match_report(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let report_path = app
                .paths
                .workspaces
                .join(&id)
                .join("run")
                .join("evidence_match_report.json");
            match read_optional_json::<EvidenceMatchReport>(&report_path).await {
                Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
                Ok(None) => (
                    StatusCode::NOT_FOUND,
                    "Evidence match report not yet generated",
                )
                    .into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_evidence_match_report(
    State(app): State<AppContext>,
    Json(request): Json<EvidenceMatchReportRequest>,
) -> impl IntoResponse {
    let mut query_terms = request.query_terms;
    let mut labels = request.labels;
    let mut claims = request.claims;
    let mut sources = request.sources;
    let mut time_span_ms = request.time_span_ms;
    let mut query_source = "api_query".to_string();
    let mut selected_window_summary = None;

    if let Some(window) = request.selected_window {
        query_source = "selected_window".to_string();
        if let Some(title) = window.title.as_deref() {
            push_unique_string(&mut query_terms, title);
        }
        if let Some(annotation_type) = window.annotation_type.as_deref() {
            push_unique_string(&mut query_terms, annotation_type);
        }
        if let Some(notes) = window.notes.as_deref() {
            push_unique_string(&mut query_terms, notes);
        }
        for label in &window.labels {
            push_unique_string(&mut labels, label);
        }
        for claim in &window.claims {
            push_unique_string(&mut claims, claim);
        }
        for source in &window.sources {
            push_unique_string(&mut sources, source);
        }
        if time_span_ms.is_none() {
            time_span_ms = window
                .time_span_ms
                .or_else(|| match (window.start_ms, window.end_ms) {
                    (Some(start), Some(end)) if end >= start => Some(end - start),
                    _ => None,
                });
        }
        selected_window_summary = Some(render_selected_window_summary(&window, time_span_ms));
    }

    match app
        .build_evidence_match_report_from_query(
            &request.project_root,
            request.spec_id.as_deref(),
            &query_source,
            selected_window_summary,
            &query_terms,
            &labels,
            &claims,
            &sources,
            time_span_ms,
            request.limit.unwrap_or(8),
        )
        .await
    {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn execute_coobie_query(
    app: &AppContext,
    requested_run_id: Option<&str>,
    message: &str,
    retrieval_depth: usize,
) -> anyhow::Result<CoobieQueryResponse> {
    let query = message.trim();
    if query.is_empty() {
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: "Ask me about the current run, recalled lessons, stale memory, recoveries, or interventions.".to_string(),
            retrieval_path: vec!["working_memory".to_string()],
            confidence: 0.3,
            sources: Vec::new(),
        });
    }

    let target_run = resolve_query_run(app, requested_run_id).await?;
    let normalized = query.to_ascii_lowercase();

    if normalized.contains("memory-bearing")
        || (normalized.contains("memory") && normalized.contains("event"))
    {
        return answer_memory_events_query(app, target_run.as_deref(), query).await;
    }
    if normalized.contains("intervention")
        || normalized.contains("recover")
        || normalized.contains("recovery")
    {
        return answer_recovery_query(app, target_run.as_deref(), query).await;
    }
    if normalized.contains("stale")
        || normalized.contains("lesson")
        || normalized.contains("recalled")
    {
        return answer_memory_status_query(app, target_run.as_deref(), query).await;
    }

    answer_general_coobie_query(app, target_run.as_deref(), query, retrieval_depth).await
}

async fn resolve_query_run(
    app: &AppContext,
    requested_run_id: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if let Some(run_id) = requested_run_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if app.get_run(run_id).await?.is_some() {
            return Ok(Some(run_id.to_string()));
        }
    }
    Ok(app
        .list_runs(1)
        .await?
        .into_iter()
        .next()
        .map(|run| run.run_id))
}

fn normalize_retrieval_depth(depth: Option<u8>) -> usize {
    depth.unwrap_or(1).clamp(1, 2) as usize
}

async fn load_query_memory_stores(
    app: &AppContext,
    run_id: Option<&str>,
) -> anyhow::Result<Vec<(String, MemoryStore)>> {
    let mut stores = vec![("core_memory".to_string(), app.memory_store.clone())];

    let Some(run_id) = run_id else {
        return Ok(stores);
    };

    let target_source_path = app
        .paths
        .workspaces
        .join(run_id)
        .join("run")
        .join("target_source.json");
    if !target_source_path.exists() {
        return Ok(stores);
    }

    let raw = tokio::fs::read_to_string(&target_source_path).await?;
    let target_source: QueryTargetSourceMetadata = serde_json::from_str(&raw)?;
    let project_memory_root = PathBuf::from(target_source.source_path)
        .join(".harkonnen")
        .join("project-memory");
    tokio::fs::create_dir_all(project_memory_root.join("imports")).await?;
    let project_store = MemoryStore::new(project_memory_root);
    project_store.reindex().await?;
    stores.insert(0, ("project_memory".to_string(), project_store));
    Ok(stores)
}

fn memory_source_ref(
    scope: &str,
    run_id: Option<&str>,
    hit: &ScopedMemoryHit,
) -> CoobieQuerySource {
    let note = if !hit.hit.surfaced_via.is_empty() {
        Some(format!("via {}", hit.hit.surfaced_via.join("; ")))
    } else if !hit.hit.invalidation_reasons.is_empty() {
        Some(hit.hit.invalidation_reasons.join("; "))
    } else {
        None
    };

    CoobieQuerySource {
        kind: scope.to_string(),
        label: hit.hit.summary.clone(),
        run_id: run_id.map(|value| value.to_string()),
        phase: None,
        artifact: Some(hit.hit.id.clone()),
        hop: Some(hit.hop),
        query: Some(hit.query.clone()),
        score: Some(hit.hit.score as f64),
        status: hit.hit.status.clone(),
        superseded_by: hit.hit.superseded_by.clone(),
        challenged_by: hit.hit.challenged_by.clone(),
        note,
    }
}

fn follow_up_query_from_hits(original_query: &str, hits: &[ScopedMemoryHit]) -> Option<String> {
    let top_hit = hits.first()?;
    let original_tokens = original_query
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 4)
        .map(|token| token.to_ascii_lowercase())
        .collect::<HashSet<_>>();

    let mut parts = vec![top_hit.hit.summary.clone()];
    for tag in top_hit.hit.tags.iter().take(3) {
        if !tag.trim().is_empty() {
            parts.push(tag.trim().to_string());
        }
    }

    let snippet_terms = top_hit
        .hit
        .snippet
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| token.len() >= 5)
        .map(|token| token.to_ascii_lowercase())
        .filter(|token| {
            !matches!(
                token.as_str(),
                "which"
                    | "their"
                    | "there"
                    | "would"
                    | "about"
                    | "because"
                    | "after"
                    | "before"
                    | "under"
                    | "while"
                    | "where"
                    | "using"
                    | "these"
                    | "those"
                    | "query"
                    | "memory"
            ) && !original_tokens.contains(token)
        })
        .take(6)
        .collect::<Vec<_>>();
    if !snippet_terms.is_empty() {
        parts.push(snippet_terms.join(" "));
    }

    let derived = parts.join(" ").trim().to_string();
    if derived.is_empty() || derived.eq_ignore_ascii_case(original_query) {
        None
    } else {
        Some(derived)
    }
}

async fn retrieve_multi_hop_memory_hits(
    app: &AppContext,
    run_id: Option<&str>,
    query: &str,
    retrieval_depth: usize,
) -> anyhow::Result<(Vec<String>, Vec<ScopedMemoryHit>)> {
    let stores = load_query_memory_stores(app, run_id).await?;
    let mut retrieval_path = Vec::new();
    let mut all_hits = Vec::new();
    let mut seen = HashSet::new();
    let mut active_query = query.trim().to_string();

    for hop in 1..=retrieval_depth.max(1) {
        if active_query.is_empty() {
            break;
        }

        let mut hop_hits = Vec::new();
        for (scope, store) in &stores {
            let ranked = store
                .retrieve_ranked_entries(
                    &active_query,
                    app.embedding_store.as_ref(),
                    if hop == 1 { 4 } else { 3 },
                )
                .await?;
            for hit in ranked {
                let dedupe_key = format!("{}::{}", scope, hit.id);
                if seen.insert(dedupe_key) {
                    hop_hits.push(ScopedMemoryHit {
                        scope: scope.clone(),
                        hop: hop as u8,
                        query: active_query.clone(),
                        hit,
                    });
                }
            }
        }

        hop_hits.sort_by(|left, right| {
            right
                .hit
                .score
                .partial_cmp(&left.hit.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        if hop_hits.is_empty() {
            if hop == 1 {
                retrieval_path.push("memory_chain:hop_1_empty".to_string());
            }
            break;
        }

        retrieval_path.push(format!("memory_chain:hop_{}", hop));
        for hit in &hop_hits {
            let scope_segment = format!("{}:hop_{}", hit.scope, hop);
            if !retrieval_path
                .iter()
                .any(|existing| existing == &scope_segment)
            {
                retrieval_path.push(scope_segment);
            }
        }
        all_hits.extend(hop_hits.clone());

        if hop >= retrieval_depth {
            break;
        }
        let Some(next_query) = follow_up_query_from_hits(query, &hop_hits) else {
            break;
        };
        if next_query.eq_ignore_ascii_case(&active_query) {
            break;
        }
        active_query = next_query;
    }

    Ok((retrieval_path, all_hits))
}

fn format_memory_chain_summary(hits: &[ScopedMemoryHit]) -> Option<String> {
    if hits.is_empty() {
        return None;
    }

    let hop_count = hits.iter().map(|hit| hit.hop).max().unwrap_or(1);
    let top = hits
        .iter()
        .take(4)
        .map(|hit| format!("hop {} {} [{}]", hit.hop, hit.hit.summary, hit.scope))
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "memory chain surfaced {} hit(s) across {} hop(s): {}",
        hits.len(),
        hop_count,
        top,
    ))
}

async fn answer_general_coobie_query(
    app: &AppContext,
    run_id: Option<&str>,
    query: &str,
    retrieval_depth: usize,
) -> anyhow::Result<CoobieQueryResponse> {
    let mut retrieval_path = Vec::new();
    let mut sources = Vec::new();
    let (memory_retrieval_path, memory_hits) =
        retrieve_multi_hop_memory_hits(app, run_id, query, retrieval_depth).await?;
    retrieval_path.extend(memory_retrieval_path);
    for hit in memory_hits.iter().take(6) {
        sources.push(memory_source_ref(&hit.scope, run_id, hit));
    }

    if let Some(run_id) = run_id {
        retrieval_path.push("working_memory".to_string());
        retrieval_path.push("blackboard".to_string());
        let mission = build_mission_board(app, run_id).await?;
        let action = build_action_board(app, run_id).await?;
        let evidence = build_evidence_board(app, run_id).await?;
        let memory = build_memory_board(app, run_id).await?;
        if let Some(board) = mission.as_ref() {
            sources.push(source_ref(
                "mission_board",
                &board.title,
                Some(run_id),
                board.current_phase.as_deref(),
                Some("spec.yaml"),
            ));
        }
        if let Some(board) = action.as_ref() {
            sources.push(source_ref(
                "action_board",
                board.active_goal.as_deref().unwrap_or("action-board"),
                Some(run_id),
                board.current_phase.as_deref(),
                Some("blackboard.json"),
            ));
        }
        if let Some(board) = evidence.as_ref() {
            sources.push(source_ref(
                "evidence_board",
                "evidence-board",
                Some(run_id),
                None,
                Some("validation.json"),
            ));
            if board.causal_report.is_some() {
                sources.push(source_ref(
                    "causal_report",
                    "causal-report",
                    Some(run_id),
                    Some("memory"),
                    Some("causal_report.json"),
                ));
            }
        }
        if let Some(board) = memory.as_ref() {
            sources.push(source_ref(
                "memory_board",
                "memory-board",
                Some(run_id),
                board.current_phase.as_deref(),
                Some("coobie_briefing.json"),
            ));
        }

        let mut response = format_general_query_response(
            query,
            mission.as_ref(),
            action.as_ref(),
            evidence.as_ref(),
            memory.as_ref(),
        );
        if let Some(summary) = format_memory_chain_summary(&memory_hits) {
            response.push(' ');
            response.push_str(&summary);
            response.push('.');
        }
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response,
            retrieval_path,
            confidence: if memory_hits.is_empty() { 0.72 } else { 0.82 },
            sources,
        });
    }

    if let Some(summary) = format_memory_chain_summary(&memory_hits) {
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: format!(
                "I do not have a run in working memory yet, but {}.",
                summary
            ),
            retrieval_path,
            confidence: 0.61,
            sources,
        });
    }

    Ok(CoobieQueryResponse {
        agent: "coobie".to_string(),
        response: "I do not have a run in working memory yet. Commission a run or pass a run_id and I can answer from the blackboard, lessons, causal history, and memory chain retrieval.".to_string(),
        retrieval_path: vec!["working_memory".to_string(), "memory_chain:hop_1_empty".to_string()],
        confidence: 0.42,
        sources,
    })
}

async fn answer_memory_status_query(
    app: &AppContext,
    run_id: Option<&str>,
    _query: &str,
) -> anyhow::Result<CoobieQueryResponse> {
    let Some(run_id) = run_id else {
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: "I do not have an active run to inspect for recalled lessons or stale memory. Pass a run_id or commission a run first.".to_string(),
            retrieval_path: vec!["working_memory".to_string()],
            confidence: 0.4,
            sources: Vec::new(),
        });
    };

    let board = build_memory_board(app, run_id).await?;
    let Some(board) = board else {
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: format!("Run {run_id} was not found."),
            retrieval_path: vec!["working_memory".to_string()],
            confidence: 0.2,
            sources: Vec::new(),
        });
    };

    let active_lessons = board
        .active_recalled_lessons
        .iter()
        .take(3)
        .map(|entry| {
            format!(
                "{} [{}]",
                entry.lesson.lesson_id,
                entry.used_in_phases.join(", ")
            )
        })
        .collect::<Vec<_>>();
    let stale = board
        .stale_memory_entries
        .iter()
        .take(3)
        .map(|entry| {
            format!(
                "{}:{}:{}",
                entry.memory_id,
                entry.severity,
                entry.mitigation_status.as_deref().unwrap_or("unresolved")
            )
        })
        .collect::<Vec<_>>();
    let updates = board
        .memory_updates
        .iter()
        .take(3)
        .map(|entry| {
            format!(
                "{}:{}->{}",
                entry.relation, entry.stale_memory_id, entry.fresh_memory_id
            )
        })
        .collect::<Vec<_>>();

    let mut response = format!(
        "Memory Board for run {}: {} active recalled lessons, {} stale-risk entries, active risk score {}.",
        run_id,
        board.active_recalled_lessons.len(),
        board.stale_risk_summary.stale_risk_count,
        board.stale_risk_summary.active_risk_score,
    );
    if !active_lessons.is_empty() {
        response.push_str(&format!(" Active lessons: {}.", active_lessons.join("; ")));
    }
    if !stale.is_empty() {
        response.push_str(&format!(" Top stale memory entries: {}.", stale.join("; ")));
    }
    if !updates.is_empty() {
        response.push_str(&format!(" Recorded fact updates: {}.", updates.join("; ")));
    }

    Ok(CoobieQueryResponse {
        agent: "coobie".to_string(),
        response,
        retrieval_path: vec![
            "working_memory".to_string(),
            "blackboard".to_string(),
            "typed_lessons".to_string(),
        ],
        confidence: 0.83,
        sources: vec![
            source_ref(
                "memory_board",
                "memory-board",
                Some(run_id),
                board.current_phase.as_deref(),
                Some("coobie_briefing.json"),
            ),
            source_ref(
                "memory_board",
                "stale-memory",
                Some(run_id),
                Some("memory"),
                Some("stale_memory_mitigation_status.json"),
            ),
            source_ref(
                "memory_board",
                "memory-updates",
                Some(run_id),
                Some("memory"),
                Some("memory-updates.json"),
            ),
        ],
    })
}

async fn answer_memory_events_query(
    app: &AppContext,
    run_id: Option<&str>,
    _query: &str,
) -> anyhow::Result<CoobieQueryResponse> {
    let Some(run_id) = run_id else {
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: "I need a run in scope to return memory-bearing events. Pass a run_id or open a run first.".to_string(),
            retrieval_path: vec!["working_memory".to_string()],
            confidence: 0.38,
            sources: Vec::new(),
        });
    };

    let attributions = app.list_phase_attributions_for_run(run_id).await?;
    let memory_events = attributions
        .into_iter()
        .filter(|record| {
            !record.memory_hits.is_empty()
                || !record.core_memory_ids.is_empty()
                || !record.project_memory_ids.is_empty()
                || !record.relevant_lesson_ids.is_empty()
                || record.phase == "memory"
        })
        .collect::<Vec<_>>();

    let summary = if memory_events.is_empty() {
        format!("I found no memory-bearing phase attributions for run {run_id}.")
    } else {
        let lines = memory_events
            .iter()
            .take(8)
            .map(|record| {
                format!(
                    "{}:{} outcome={} memories={} core={} project={} lessons={}",
                    record.phase,
                    record.agent_name,
                    record.outcome,
                    record.memory_hits.len(),
                    record.core_memory_ids.len(),
                    record.project_memory_ids.len(),
                    record.relevant_lesson_ids.len(),
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "I found {} memory-bearing phase events for run {}. {}",
            memory_events.len(),
            run_id,
            lines,
        )
    };

    let sources = memory_events
        .iter()
        .take(8)
        .map(|record| {
            source_ref(
                "phase_attribution",
                &record.agent_name,
                Some(run_id),
                Some(&record.phase),
                Some("phase_attributions.json"),
            )
        })
        .collect::<Vec<_>>();

    Ok(CoobieQueryResponse {
        agent: "coobie".to_string(),
        response: summary,
        retrieval_path: vec![
            "working_memory".to_string(),
            "blackboard".to_string(),
            "typed_lessons".to_string(),
        ],
        confidence: 0.87,
        sources,
    })
}

async fn answer_recovery_query(
    app: &AppContext,
    run_id: Option<&str>,
    query: &str,
) -> anyhow::Result<CoobieQueryResponse> {
    let lower = query.to_ascii_lowercase();
    let ask_mason = lower.contains("mason");
    let ask_validation = lower.contains("validation");
    let mut recovery_rows = Vec::new();
    let mut interventions = HashMap::<String, usize>::new();

    for run in app.list_runs(40).await? {
        let events = app.list_run_events(&run.run_id).await?;
        let Some(first_validation_issue) = events.iter().find(|event| {
            (!ask_validation || event.phase == "validation")
                && matches!(event.status.as_str(), "warning" | "failed" | "blocked")
        }) else {
            continue;
        };

        let later_mason = events.iter().any(|event| {
            event.event_id > first_validation_issue.event_id
                && event.agent.eq_ignore_ascii_case("mason")
                && matches!(event.status.as_str(), "running" | "complete" | "info")
        });
        if ask_mason && !later_mason {
            continue;
        }
        if !matches!(run.status.as_str(), "completed" | "completed_with_issues") {
            continue;
        }

        let run_dir = app.paths.workspaces.join(&run.run_id).join("run");
        let report =
            read_optional_json::<CausalReport>(&run_dir.join("causal_report.json")).await?;
        if let Some(report) = report.as_ref() {
            tally_interventions(&mut interventions, &report.recommended_interventions);
        }

        recovery_rows.push((
            run,
            first_validation_issue.phase.clone(),
            later_mason,
            report,
        ));
    }

    let mut response = if recovery_rows.is_empty() {
        "I did not find matching recovery runs in the last 40 runs.".to_string()
    } else {
        let rows = recovery_rows
            .iter()
            .take(6)
            .map(|(run, phase, later_mason, _)| {
                format!(
                    "{} status={} phase={} mason_recovery={}",
                    run.run_id,
                    run.status,
                    phase,
                    if *later_mason { "yes" } else { "no" }
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "I found {} recovery runs in the last 40 runs. {}",
            recovery_rows.len(),
            rows,
        )
    };

    if !interventions.is_empty() {
        let mut ranked = interventions.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        let top = ranked
            .into_iter()
            .take(3)
            .map(|(label, count)| format!("{} x{}", label, count))
            .collect::<Vec<_>>()
            .join("; ");
        response.push_str(&format!(
            " Most common recommended interventions before recovery: {}.",
            top
        ));
    }

    let mut sources = recovery_rows
        .iter()
        .take(6)
        .map(|(run, phase, _, _)| {
            source_ref(
                "run_event",
                &run.run_id,
                Some(&run.run_id),
                Some(phase),
                Some("run_events"),
            )
        })
        .collect::<Vec<_>>();
    if let Some(run_id) = run_id {
        sources.push(source_ref(
            "query_scope",
            "query-scope",
            Some(run_id),
            None,
            None,
        ));
    }

    Ok(CoobieQueryResponse {
        agent: "coobie".to_string(),
        response,
        retrieval_path: vec!["working_memory".to_string(), "causal_lookup".to_string()],
        confidence: if recovery_rows.is_empty() { 0.45 } else { 0.74 },
        sources,
    })
}

fn format_general_query_response(
    query: &str,
    mission: Option<&MissionBoardResponse>,
    action: Option<&ActionBoardResponse>,
    evidence: Option<&EvidenceBoardResponse>,
    memory: Option<&MemoryBoardResponse>,
) -> String {
    let mut parts = vec![format!("For \"{}\"", query)];
    if let Some(mission) = mission {
        parts.push(format!(
            "run {} is {} in phase {} with goal {}",
            mission.run_id,
            mission.run_status,
            mission.current_phase.as_deref().unwrap_or("unknown"),
            mission.active_goal.as_deref().unwrap_or("unspecified")
        ));
    }
    if let Some(action) = action {
        parts.push(format!(
            "{} blockers and {} open checkpoints are active",
            action.open_blockers.len(),
            action.open_checkpoints.len()
        ));
    }
    if let Some(memory) = memory {
        parts.push(format!(
            "Coobie has {} active recalled lessons and stale risk score {}",
            memory.active_recalled_lessons.len(),
            memory.stale_risk_summary.active_risk_score
        ));
    }
    if let Some(evidence) = evidence {
        parts.push(format!(
            "evidence includes validation={}, hidden_scenarios={}, causal_report={}",
            evidence
                .validation
                .as_ref()
                .map(|summary| if summary.passed { "passed" } else { "failed" })
                .unwrap_or("missing"),
            evidence
                .hidden_scenarios
                .as_ref()
                .map(|summary| if summary.passed { "passed" } else { "failed" })
                .unwrap_or("missing"),
            if evidence.causal_report.is_some() {
                "present"
            } else {
                "missing"
            }
        ));
    }
    parts.join("; ") + "."
}

fn source_ref(
    kind: &str,
    label: &str,
    run_id: Option<&str>,
    phase: Option<&str>,
    artifact: Option<&str>,
) -> CoobieQuerySource {
    CoobieQuerySource {
        kind: kind.to_string(),
        label: label.to_string(),
        run_id: run_id.map(|value| value.to_string()),
        phase: phase.map(|value| value.to_string()),
        artifact: artifact.map(|value| value.to_string()),
        hop: None,
        query: None,
        score: None,
        status: None,
        superseded_by: None,
        challenged_by: Vec::new(),
        note: None,
    }
}

fn tally_interventions(counts: &mut HashMap<String, usize>, interventions: &[InterventionPlan]) {
    for intervention in interventions {
        let label = format!("{} -> {}", intervention.target, intervention.action);
        *counts.entry(label).or_insert(0) += 1;
    }
}

fn title_case_agent(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

async fn build_mission_board(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<MissionBoardResponse>> {
    let Some(run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let spec = read_optional_spec(&run_dir.join("spec.yaml")).await?;

    let (title, purpose, scope, constraints, acceptance_criteria, forbidden_behaviors) =
        if let Some(spec) = spec {
            (
                spec.title,
                spec.purpose,
                spec.scope,
                spec.constraints,
                spec.acceptance_criteria,
                spec.forbidden_behaviors,
            )
        } else {
            (
                run.spec_id.clone(),
                String::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
        };

    Ok(Some(MissionBoardResponse {
        run_id: run.run_id,
        spec_id: run.spec_id,
        title,
        purpose,
        product: run.product,
        run_status: run.status,
        current_phase: blackboard.as_ref().map(|board| board.current_phase.clone()),
        active_goal: blackboard.as_ref().map(|board| board.active_goal.clone()),
        scope,
        constraints,
        acceptance_criteria,
        forbidden_behaviors,
        open_blockers: blackboard
            .as_ref()
            .map(|board| board.open_blockers.clone())
            .unwrap_or_default(),
        resolved_items: blackboard
            .as_ref()
            .map(|board| board.resolved_items.clone())
            .unwrap_or_default(),
    }))
}

async fn build_action_board(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<ActionBoardResponse>> {
    let Some(_run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let checkpoints = app.list_run_checkpoints(id).await?;
    let open_checkpoints = checkpoints
        .into_iter()
        .filter(|checkpoint| matches!(checkpoint.status.as_str(), "open" | "answered"))
        .collect::<Vec<_>>();
    let mut recent_events = app.list_run_events(id).await?;
    if recent_events.len() > 12 {
        recent_events = recent_events.split_off(recent_events.len() - 12);
    }
    let mut latest_agent_executions =
        read_optional_json::<Vec<AgentExecution>>(&run_dir.join("agent_executions.json"))
            .await?
            .unwrap_or_default();
    latest_agent_executions.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    if latest_agent_executions.len() > 8 {
        latest_agent_executions =
            latest_agent_executions.split_off(latest_agent_executions.len() - 8);
    }

    Ok(Some(ActionBoardResponse {
        run_id: id.to_string(),
        current_phase: blackboard.as_ref().map(|board| board.current_phase.clone()),
        active_goal: blackboard.as_ref().map(|board| board.active_goal.clone()),
        agent_claims: blackboard
            .as_ref()
            .map(|board| board.agent_claims.clone())
            .unwrap_or_default(),
        agent_instances: blackboard
            .as_ref()
            .map(|board| board.agent_instances.clone())
            .unwrap_or_default(),
        open_blockers: blackboard
            .as_ref()
            .map(|board| board.open_blockers.clone())
            .unwrap_or_default(),
        open_checkpoints,
        recent_events,
        latest_agent_executions,
    }))
}

async fn build_evidence_board(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<EvidenceBoardResponse>> {
    let Some(_run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let validation =
        read_optional_json::<ValidationSummary>(&run_dir.join("validation.json")).await?;
    let hidden_scenarios =
        read_optional_json::<HiddenScenarioSummary>(&run_dir.join("hidden_scenarios.json")).await?;
    let evidence_match_report =
        read_optional_json::<EvidenceMatchReport>(&run_dir.join("evidence_match_report.json"))
            .await?;
    let causal_report =
        read_optional_json::<CausalReport>(&run_dir.join("causal_report.json")).await?;
    let recent_evidence_events = app
        .list_run_events(id)
        .await?
        .into_iter()
        .filter(|event| {
            matches!(
                event.phase.as_str(),
                "validation" | "hidden_scenarios" | "memory" | "artifacts"
            )
        })
        .collect::<Vec<_>>();

    Ok(Some(EvidenceBoardResponse {
        run_id: id.to_string(),
        artifact_refs: blackboard
            .as_ref()
            .map(|board| board.artifact_refs.clone())
            .unwrap_or_default(),
        validation,
        hidden_scenarios,
        evidence_match_report,
        causal_report,
        recent_evidence_events,
    }))
}

fn flatten_checkpoint_answers(
    checkpoints: &[RunCheckpointRecord],
) -> Vec<ReasoningCheckpointAnswerView> {
    let mut answers = checkpoints
        .iter()
        .flat_map(|checkpoint| {
            checkpoint
                .answers
                .iter()
                .map(|answer| ReasoningCheckpointAnswerView {
                    checkpoint_id: checkpoint.checkpoint_id.clone(),
                    phase: checkpoint.phase.clone(),
                    agent: checkpoint.agent.clone(),
                    checkpoint_type: checkpoint.checkpoint_type.clone(),
                    checkpoint_status: checkpoint.status.clone(),
                    prompt: checkpoint.prompt.clone(),
                    answered_by: answer.answered_by.clone(),
                    answer_text: answer.answer_text.clone(),
                    decision_json: answer.decision_json.clone(),
                    created_at: answer.created_at.to_owned(),
                })
        })
        .collect::<Vec<_>>();
    answers.sort_by(|left, right| left.created_at.cmp(&right.created_at));
    answers
}

pub async fn build_run_reasoning_snapshot(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<RunReasoningSnapshotResponse>> {
    let Some(run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let decisions = app.list_run_decisions(id).await?;
    let checkpoints = app.list_run_checkpoints(id).await?;
    let open_checkpoint_count = checkpoints
        .iter()
        .filter(|checkpoint| matches!(checkpoint.status.as_str(), "open" | "answered"))
        .count();
    let checkpoint_answers = flatten_checkpoint_answers(&checkpoints);

    let recent_decisions = if decisions.len() > 8 {
        decisions[decisions.len() - 8..].to_vec()
    } else {
        decisions.clone()
    };
    let recent_checkpoint_answers = if checkpoint_answers.len() > 8 {
        checkpoint_answers[checkpoint_answers.len() - 8..].to_vec()
    } else {
        checkpoint_answers.clone()
    };

    Ok(Some(RunReasoningSnapshotResponse {
        run_id: id.to_string(),
        run_status: run.status,
        current_phase: blackboard.as_ref().map(|board| board.current_phase.clone()),
        decision_count: decisions.len(),
        checkpoint_answer_count: checkpoint_answers.len(),
        open_checkpoint_count,
        recent_decisions,
        recent_checkpoint_answers,
    }))
}

async fn build_memory_board(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<MemoryBoardResponse>> {
    let Some(_run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let phase_attributions =
        read_optional_json::<Vec<PhaseAttributionRecord>>(&run_dir.join("phase_attributions.json"))
            .await?
            .unwrap_or_default();
    let coobie_briefing =
        read_optional_json::<CoobieBriefing>(&run_dir.join("coobie_briefing.json")).await?;
    let stale_status = read_optional_json::<MemoryBoardStaleStatusArtifact>(
        &run_dir.join("stale_memory_mitigation_status.json"),
    )
    .await?;
    let reasoning = build_run_reasoning_snapshot(app, id).await?;

    let mut active_lessons = Vec::new();
    let mut active_reasoning_lessons = Vec::new();
    let mut policy_reminders = Vec::new();
    let mut causal_precedents = Vec::new();
    let mut project_memory_root = None;
    let mut stale_entries = Vec::new();
    let mut memory_updates = Vec::new();

    if let Some(briefing) = coobie_briefing.as_ref() {
        for reminder in &briefing.recommended_guardrails {
            push_unique_string(&mut policy_reminders, reminder);
        }
        for reminder in &briefing.required_checks {
            push_unique_string(&mut policy_reminders, reminder);
        }
        if let Some(board) = blackboard.as_ref() {
            for flag in &board.policy_flags {
                push_unique_string(&mut policy_reminders, flag);
            }
        }
        causal_precedents = briefing.prior_causes.clone();
        project_memory_root = briefing.project_memory_root.clone();
        if let Some(project_memory_root_value) = project_memory_root.as_ref() {
            if let Some(harkonnen_dir) = PathBuf::from(project_memory_root_value)
                .parent()
                .map(|path| path.to_path_buf())
            {
                if let Some(update_artifact) = read_optional_json::<MemoryBoardUpdateArtifact>(
                    &harkonnen_dir.join("memory-updates.json"),
                )
                .await?
                {
                    memory_updates = update_artifact
                        .entries
                        .into_iter()
                        .map(|entry| MemoryBoardUpdateView {
                            relation: entry.relation,
                            stale_memory_id: entry.stale_memory_id,
                            stale_summary: entry.stale_summary,
                            fresh_memory_id: entry.fresh_memory_id,
                            fresh_summary: entry.fresh_summary,
                        })
                        .collect();
                }
            }
        }

        for lesson in &briefing.relevant_lessons {
            let mut used_in_phases = Vec::new();
            let mut used_by_agents = Vec::new();
            let mut outcomes = Vec::new();

            for attribution in phase_attributions
                .iter()
                .filter(|attribution| attribution.relevant_lesson_ids.contains(&lesson.lesson_id))
            {
                push_unique_string(&mut used_in_phases, &attribution.phase);
                push_unique_string(&mut used_by_agents, &attribution.agent_name);
                push_unique_string(&mut outcomes, &attribution.outcome);
            }

            let lesson_view = MemoryBoardLessonView {
                lesson: lesson.clone(),
                used_in_phases,
                used_by_agents,
                outcomes,
            };
            if lesson.tags.iter().any(|tag| {
                matches!(
                    tag.as_str(),
                    "command-trial" | "decision-trial" | "checkpoint-trial"
                )
            }) {
                active_reasoning_lessons.push(lesson_view.clone());
            }
            active_lessons.push(lesson_view);
        }

        let mut stale_status_by_id = HashMap::new();
        if let Some(status) = stale_status.as_ref() {
            for entry in &status.entries {
                stale_status_by_id.insert(entry.memory_id.clone(), entry.clone());
            }
        }

        for risk in &briefing.resume_packet_risks {
            let status = stale_status_by_id.remove(&risk.memory_id);
            stale_entries.push(MemoryBoardRiskView {
                memory_id: risk.memory_id.clone(),
                summary: risk.summary.clone(),
                severity: if let Some(status) = status.as_ref() {
                    if status.severity.is_empty() {
                        risk.severity.clone()
                    } else {
                        status.severity.clone()
                    }
                } else {
                    risk.severity.clone()
                },
                severity_score: status
                    .as_ref()
                    .map(|entry| entry.severity_score)
                    .unwrap_or(risk.severity_score),
                reasons: risk.reasons.clone(),
                mitigation_status: status.as_ref().map(|entry| entry.status.clone()),
                mitigation_steps: status
                    .as_ref()
                    .map(|entry| entry.mitigation_steps.clone())
                    .unwrap_or_default(),
                related_checks: status
                    .as_ref()
                    .map(|entry| entry.related_checks.clone())
                    .unwrap_or_default(),
                evidence: status
                    .as_ref()
                    .map(|entry| entry.evidence.clone())
                    .unwrap_or_default(),
                previous_severity_score: status
                    .as_ref()
                    .and_then(|entry| entry.previous_severity_score),
                risk_reduced_from_previous: status
                    .as_ref()
                    .and_then(|entry| entry.risk_reduced_from_previous),
            });
        }

        for status in stale_status_by_id.into_values() {
            stale_entries.push(MemoryBoardRiskView {
                memory_id: status.memory_id,
                summary: String::new(),
                severity: status.severity,
                severity_score: status.severity_score,
                reasons: Vec::new(),
                mitigation_status: Some(status.status),
                mitigation_steps: status.mitigation_steps,
                related_checks: status.related_checks,
                evidence: status.evidence,
                previous_severity_score: status.previous_severity_score,
                risk_reduced_from_previous: status.risk_reduced_from_previous,
            });
        }
    } else if let Some(board) = blackboard.as_ref() {
        for flag in &board.policy_flags {
            push_unique_string(&mut policy_reminders, flag);
        }
    }

    stale_entries.sort_by(|left, right| {
        right
            .severity_score
            .cmp(&left.severity_score)
            .then_with(|| left.memory_id.cmp(&right.memory_id))
    });

    let phase_memory_usage = phase_attributions
        .iter()
        .map(|attribution| MemoryBoardPhaseUsage {
            phase: attribution.phase.clone(),
            agent_name: attribution.agent_name.clone(),
            outcome: attribution.outcome.clone(),
            prompt_bundle_provider: attribution.prompt_bundle_provider.clone(),
            memory_hits: attribution.memory_hits.clone(),
            core_memory_ids: attribution.core_memory_ids.clone(),
            project_memory_ids: attribution.project_memory_ids.clone(),
            relevant_lesson_ids: attribution.relevant_lesson_ids.clone(),
            required_checks: attribution.required_checks.clone(),
            guardrails: attribution.guardrails.clone(),
        })
        .collect::<Vec<_>>();

    let satisfied_count = stale_entries
        .iter()
        .filter(|entry| entry.mitigation_status.as_deref() == Some("satisfied"))
        .count();
    let partially_satisfied_count = stale_entries
        .iter()
        .filter(|entry| entry.mitigation_status.as_deref() == Some("partially_satisfied"))
        .count();
    let unresolved_count = stale_entries
        .iter()
        .filter(|entry| {
            entry.mitigation_status.as_deref() == Some("unresolved")
                || entry.mitigation_status.is_none()
        })
        .count();
    let active_risk_score = stale_entries
        .iter()
        .filter(|entry| entry.mitigation_status.as_deref() != Some("satisfied"))
        .map(|entry| entry.severity_score)
        .sum();
    let active_reasoning_lesson_count = active_reasoning_lessons.len();

    Ok(Some(MemoryBoardResponse {
        run_id: id.to_string(),
        current_phase: blackboard.as_ref().map(|board| board.current_phase.clone()),
        active_recalled_lessons: active_lessons,
        active_reasoning_lessons,
        phase_memory_usage,
        causal_precedents,
        policy_reminders,
        project_memory_root,
        reasoning_summary: MemoryBoardReasoningSummary {
            decision_count: reasoning
                .as_ref()
                .map(|snapshot| snapshot.decision_count)
                .unwrap_or(0),
            checkpoint_answer_count: reasoning
                .as_ref()
                .map(|snapshot| snapshot.checkpoint_answer_count)
                .unwrap_or(0),
            active_reasoning_lesson_count,
        },
        recent_decisions: reasoning
            .as_ref()
            .map(|snapshot| snapshot.recent_decisions.clone())
            .unwrap_or_default(),
        recent_checkpoint_answers: reasoning
            .as_ref()
            .map(|snapshot| snapshot.recent_checkpoint_answers.clone())
            .unwrap_or_default(),
        stale_risk_summary: MemoryBoardRiskSummary {
            stale_risk_count: stale_entries.len(),
            satisfied_count,
            partially_satisfied_count,
            unresolved_count,
            active_risk_score,
        },
        stale_memory_entries: stale_entries,
        memory_updates,
        consolidate_available: true,
    }))
}

pub async fn build_run_board_snapshot(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(mission) = build_mission_board(app, id).await? else {
        return Ok(None);
    };
    let action = build_action_board(app, id).await?;
    let evidence = build_evidence_board(app, id).await?;
    let memory = build_memory_board(app, id).await?;

    Ok(Some(serde_json::json!({
        "run_id": id,
        "mission": mission,
        "action": action,
        "evidence": evidence,
        "memory": memory,
    })))
}

async fn build_run_state(app: &AppContext, id: &str) -> anyhow::Result<Option<RunStateResponse>> {
    let Some(run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let events = app.list_run_events(id).await?;
    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let lessons = read_optional_json::<Vec<LessonRecord>>(&run_dir.join("lessons.json"))
        .await?
        .unwrap_or_default();
    let agent_executions =
        read_optional_json::<Vec<AgentExecution>>(&run_dir.join("agent_executions.json"))
            .await?
            .unwrap_or_default();
    let phase_attributions =
        read_optional_json::<Vec<PhaseAttributionRecord>>(&run_dir.join("phase_attributions.json"))
            .await?
            .unwrap_or_default();
    let coobie_briefing =
        read_optional_json::<CoobieBriefing>(&run_dir.join("coobie_briefing.json")).await?;
    let causal_report =
        read_optional_json::<CausalReport>(&run_dir.join("causal_report.json")).await?;
    let coobie_preflight_response =
        read_optional_text(&run_dir.join("coobie_preflight_response.md")).await?;
    let coobie_report_response =
        read_optional_text(&run_dir.join("coobie_report_response.md")).await?;
    let evidence_match_report =
        read_optional_json::<EvidenceMatchReport>(&run_dir.join("evidence_match_report.json"))
            .await?;
    let coobie_translations = load_coobie_translations(&run_dir).await?;

    Ok(Some(RunStateResponse {
        run,
        events,
        blackboard,
        lessons,
        agent_executions,
        phase_attributions,
        coobie_briefing,
        causal_report,
        coobie_preflight_response,
        coobie_report_response,
        evidence_match_report,
        coobie_translations,
    }))
}

async fn read_optional_json<T: DeserializeOwned>(path: &FsPath) -> anyhow::Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str::<T>(&raw)?))
}

async fn read_optional_spec(path: &FsPath) -> anyhow::Result<Option<Spec>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_yaml::from_str::<Spec>(&raw)?))
}

fn split_csv_field(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or_default()
        .split(',')
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .collect()
}

fn push_unique_string(values: &mut Vec<String>, candidate: &str) {
    let trimmed = candidate.trim();
    if trimmed.is_empty() {
        return;
    }
    if !values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(trimmed))
    {
        values.push(trimmed.to_string());
    }
}

fn render_selected_window_summary(
    window: &EvidenceMatchWindowInput,
    time_span_ms: Option<i64>,
) -> String {
    let title = window
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("selected-window");
    let annotation_type = window
        .annotation_type
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("annotation");
    let labels = if window.labels.is_empty() {
        "none".to_string()
    } else {
        window.labels.join(", ")
    };
    let span = time_span_ms
        .map(|value| format!("{} ms", value))
        .unwrap_or_else(|| "unspecified".to_string());
    format!(
        "{} [{}] labels={} span={}",
        title, annotation_type, labels, span
    )
}

async fn read_optional_text(path: &FsPath) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(tokio::fs::read_to_string(path).await?))
}

async fn load_coobie_translations(run_dir: &FsPath) -> anyhow::Result<Vec<PidginTranslation>> {
    let mut translations = Vec::new();

    if let Some(text) = read_optional_text(&run_dir.join("coobie_preflight_response.md")).await? {
        let translation = pidgin::translate_pidgin_text("preflight", &text);
        if !translation.signals.is_empty() || !translation.raw.trim().is_empty() {
            translations.push(translation);
        }
    }

    if let Some(text) = read_optional_text(&run_dir.join("coobie_report_response.md")).await? {
        let translation = pidgin::translate_pidgin_text("report", &text);
        if !translation.signals.is_empty() || !translation.raw.trim().is_empty() {
            translations.push(translation);
        }
    }

    Ok(translations)
}

fn default_assignment_status() -> String {
    "active".to_string()
}

fn default_coordination_owner() -> String {
    "keeper".to_string()
}

fn default_policy_mode() -> String {
    "exclusive_file_claims".to_string()
}

fn default_stale_after_seconds() -> i64 {
    600
}

fn default_assignments_state() -> AssignmentsState {
    AssignmentsState {
        managed_by: default_coordination_owner(),
        policy_mode: default_policy_mode(),
        stale_after_seconds: default_stale_after_seconds(),
        active: HashMap::new(),
        updated_at: Utc::now().to_rfc3339(),
    }
}

fn coordination_json_path(app: &AppContext) -> PathBuf {
    app.paths
        .factory
        .join("coordination")
        .join("assignments.json")
}

fn assignments_markdown_path(app: &AppContext) -> PathBuf {
    app.paths.root.join("assignments.md")
}

fn coordination_policy_events_path(app: &AppContext) -> PathBuf {
    app.paths
        .factory
        .join("coordination")
        .join("policy_events.json")
}

async fn load_assignments(app: &AppContext) -> anyhow::Result<AssignmentsState> {
    let path = coordination_json_path(app);
    if !path.exists() {
        return Ok(default_assignments_state());
    }
    let raw = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&raw)?)
}

async fn load_policy_events(app: &AppContext) -> anyhow::Result<Vec<CoordinationPolicyEvent>> {
    let path = coordination_policy_events_path(app);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&raw)?)
}

async fn save_policy_events(
    app: &AppContext,
    events: &[CoordinationPolicyEvent],
) -> anyhow::Result<()> {
    let path = coordination_policy_events_path(app);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, serde_json::to_string_pretty(events)?).await?;
    Ok(())
}

pub(crate) async fn append_policy_event(
    app: &AppContext,
    event: CoordinationPolicyEvent,
) -> anyhow::Result<()> {
    let mut events = load_policy_events(app).await?;
    events.push(event.clone());
    save_policy_events(app, &events).await?;
    crate::db::insert_coordination_policy_event(&app.pool, &event).await?;
    Ok(())
}

pub(crate) async fn save_assignments(
    app: &AppContext,
    state: &AssignmentsState,
) -> anyhow::Result<()> {
    let json_path = coordination_json_path(app);
    if let Some(parent) = json_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::write(&json_path, serde_json::to_string_pretty(state)?).await?;
    tokio::fs::write(
        assignments_markdown_path(app),
        render_assignments_markdown(state),
    )
    .await?;
    crate::db::sync_coordination_leases(&app.pool, state).await?;
    Ok(())
}

fn parse_utc(raw: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn has_file_conflict(requested_files: &[String], existing_files: &[String]) -> bool {
    requested_files
        .iter()
        .any(|file| existing_files.contains(file))
}

fn normalize_assignment(
    assignment: &mut Assignment,
    now: DateTime<Utc>,
    stale_after_seconds: i64,
) -> bool {
    let mut changed = false;
    if assignment.last_heartbeat_at.trim().is_empty() {
        assignment.last_heartbeat_at = assignment.claimed_at.clone();
        changed = true;
    }
    let heartbeat = parse_utc(&assignment.last_heartbeat_at)
        .or_else(|| parse_utc(&assignment.claimed_at))
        .unwrap_or(now);
    let next_status = if now.signed_duration_since(heartbeat).num_seconds() >= stale_after_seconds {
        "stale"
    } else {
        "active"
    };
    if assignment.status != next_status {
        assignment.status = next_status.to_string();
        changed = true;
    }
    changed
}

async fn normalize_assignments(
    app: &AppContext,
    mut state: AssignmentsState,
) -> anyhow::Result<AssignmentsState> {
    let now = Utc::now();
    let mut changed = false;
    let mut events = Vec::new();

    if state.managed_by.trim().is_empty() {
        state.managed_by = default_coordination_owner();
        changed = true;
    }
    if state.policy_mode.trim().is_empty() {
        state.policy_mode = default_policy_mode();
        changed = true;
    }
    if state.stale_after_seconds <= 0 {
        state.stale_after_seconds = default_stale_after_seconds();
        changed = true;
    }
    if state.updated_at.trim().is_empty() {
        state.updated_at = now.to_rfc3339();
        changed = true;
    }

    for assignment in state.active.values_mut() {
        let previous_status = assignment.status.clone();
        if normalize_assignment(assignment, now, state.stale_after_seconds) {
            changed = true;
            if assignment.status == "stale" && previous_status != "stale" {
                events.push(CoordinationPolicyEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    managed_by: state.managed_by.clone(),
                    event_type: "claim_stale".to_string(),
                    status: "stale".to_string(),
                    agent: Some(assignment.agent.clone()),
                    conflicting_agent: None,
                    files: assignment.files.clone(),
                    message: format!(
                        "Keeper marked claim for {} as stale after {} seconds without a heartbeat",
                        assignment.agent, state.stale_after_seconds
                    ),
                    created_at: now.to_rfc3339(),
                });
            }
        }
    }

    if changed {
        state.updated_at = now.to_rfc3339();
        save_assignments(app, &state).await?;
    }

    for event in events {
        append_policy_event(app, event).await?;
    }

    Ok(state)
}

pub(crate) async fn ensure_assignments_state(app: &AppContext) -> anyhow::Result<AssignmentsState> {
    let state = load_assignments(app).await?;
    normalize_assignments(app, state).await
}

fn render_assignments_markdown(state: &AssignmentsState) -> String {
    let mut out = String::new();
    out.push_str(
        "# Assignments

",
    );
    out.push_str(
        "This is the fallback coordination document when the Harkonnen API server is not running.

",
    );
    out.push_str(&format!(
        "Keeper manages file-claim policy for this repo.
Policy mode: {}
Heartbeat timeout: {} seconds

",
        state.policy_mode, state.stale_after_seconds
    ));
    out.push_str(
        "Preferred live source once the server is up: `GET /api/coordination/assignments`.

",
    );
    out.push_str(
        "Policy event stream: `GET /api/coordination/policy-events`.

",
    );
    out.push_str("Claim work with `POST /api/coordination/claim`, heartbeat with `POST /api/coordination/heartbeat`, and release it with `POST /api/coordination/release`.

");
    out.push_str(&format!(
        "Last updated: {}

",
        state.updated_at
    ));
    out.push_str(
        "## Active Claims

",
    );

    if state.active.is_empty() {
        out.push_str(
            "No active claims.

",
        );
    } else {
        let mut claims: Vec<_> = state.active.values().cloned().collect();
        claims.sort_by(|a, b| a.agent.cmp(&b.agent));
        for claim in claims {
            out.push_str(&format!(
                "### {}
",
                claim.agent
            ));
            out.push_str(&format!(
                "Task: {}
",
                claim.task
            ));
            out.push_str(&format!(
                "Status: {}
",
                claim.status
            ));
            out.push_str(&format!(
                "Claimed: {}
",
                claim.claimed_at
            ));
            out.push_str(&format!(
                "Last heartbeat: {}
",
                claim.last_heartbeat_at
            ));
            if claim.files.is_empty() {
                out.push_str(
                    "Files: none declared

",
                );
            } else {
                out.push_str(&format!(
                    "Files:
- {}

",
                    claim.files.join(
                        "
- "
                    )
                ));
            }
        }
    }

    out.push_str(
        "## How To Use This Fallback

",
    );
    out.push_str(
        "1. Before assigning work, read the relevant active claim section.
",
    );
    out.push_str(
        "2. Paste only the relevant section into the AI's context.
",
    );
    out.push_str(
        "3. If you are actively holding files, send a heartbeat about once per minute.
",
    );
    out.push_str(
        "4. Keeper may reap stale conflicting claims when another agent needs the same files.
",
    );
    out.push_str(
        "5. Once the server is running, switch all agents to the live coordination endpoint.
",
    );
    out
}

async fn get_coordination_policy_events(State(app): State<AppContext>) -> impl IntoResponse {
    match load_policy_events(&app).await {
        Ok(events) => (StatusCode::OK, Json(events)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_assignments(State(app): State<AppContext>) -> impl IntoResponse {
    match ensure_assignments_state(&app).await {
        Ok(state) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn claim_task(
    State(app): State<AppContext>,
    Json(req): Json<ClaimRequest>,
) -> impl IntoResponse {
    let mut state = match ensure_assignments_state(&app).await {
        Ok(state) => state,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    let now = Utc::now();
    let mut reaped = Vec::new();

    if !req.files.is_empty() {
        let stale_owners: Vec<String> = state
            .active
            .iter()
            .filter(|(owner, existing)| {
                *owner != &req.agent
                    && existing.status == "stale"
                    && has_file_conflict(&req.files, &existing.files)
            })
            .map(|(owner, _)| owner.clone())
            .collect();

        for owner in stale_owners {
            if let Some(assignment) = state.active.remove(&owner) {
                reaped.push((owner, assignment));
            }
        }

        for (owner, assignment) in &reaped {
            if let Err(error) = append_policy_event(
                &app,
                CoordinationPolicyEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    managed_by: state.managed_by.clone(),
                    event_type: "stale_claim_reaped".to_string(),
                    status: "released".to_string(),
                    agent: Some(owner.clone()),
                    conflicting_agent: Some(req.agent.clone()),
                    files: assignment.files.clone(),
                    message: format!(
                        "Keeper reaped stale claim for {} so {} could claim the files",
                        owner, req.agent
                    ),
                    created_at: now.to_rfc3339(),
                },
            )
            .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
            }
        }

        for (owner, existing) in &state.active {
            if owner == &req.agent {
                continue;
            }
            let conflict: Vec<&String> = req
                .files
                .iter()
                .filter(|file| existing.files.contains(file))
                .collect();
            if !conflict.is_empty() {
                let message = format!(
                    "Keeper blocked claim: {} already owns {:?}",
                    owner, conflict
                );
                let event = CoordinationPolicyEvent {
                    event_id: uuid::Uuid::new_v4().to_string(),
                    managed_by: state.managed_by.clone(),
                    event_type: "file_claim_conflict".to_string(),
                    status: "blocked".to_string(),
                    agent: Some(req.agent.clone()),
                    conflicting_agent: Some(owner.clone()),
                    files: conflict.iter().map(|file| (*file).clone()).collect(),
                    message: message.clone(),
                    created_at: now.to_rfc3339(),
                };
                if let Err(error) = append_policy_event(&app, event).await {
                    return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response();
                }
                let response = CoordinationConflictResponse {
                    managed_by: state.managed_by.clone(),
                    policy_mode: state.policy_mode.clone(),
                    event_type: "file_claim_conflict".to_string(),
                    requested_agent: req.agent.clone(),
                    conflicting_agent: owner.clone(),
                    conflicting_files: conflict.iter().map(|file| (*file).clone()).collect(),
                    message,
                };
                return (StatusCode::CONFLICT, Json(response)).into_response();
            }
        }
    }

    let agent = req.agent.clone();
    let files = req.files.clone();
    let claimed_at = now.to_rfc3339();

    // Compute expires_at from TTL if provided.
    let expires_at = if req.ttl_secs > 0 {
        (now + ChronoDuration::seconds(req.ttl_secs)).to_rfc3339()
    } else {
        String::new()
    };

    state.active.insert(
        agent.clone(),
        Assignment {
            agent: agent.clone(),
            task: req.task,
            files: files.clone(),
            claimed_at: claimed_at.clone(),
            last_heartbeat_at: claimed_at,
            status: "active".to_string(),
            resource_kind: req.resource_kind,
            ttl_secs: req.ttl_secs,
            guardrails: req.guardrails,
            expires_at,
        },
    );
    state.updated_at = now.to_rfc3339();

    let _ = append_policy_event(
        &app,
        CoordinationPolicyEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            managed_by: state.managed_by.clone(),
            event_type: "claim_granted".to_string(),
            status: "granted".to_string(),
            agent: Some(agent.clone()),
            conflicting_agent: None,
            files,
            message: format!("Keeper granted claim for {}", agent),
            created_at: now.to_rfc3339(),
        },
    )
    .await;

    match save_assignments(&app, &state).await {
        Ok(()) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `POST /api/coordination/check-lease` — verify an agent is allowed to act
/// on a resource and that no guardrail violations exist.
///
/// Returns 200 with `allowed: true` when safe to proceed, or `allowed: false`
/// with `guardrail_violations` describing what must be resolved first.
async fn check_lease(
    State(app): State<AppContext>,
    Json(req): Json<CheckLeaseRequest>,
) -> impl IntoResponse {
    let state = match ensure_assignments_state(&app).await {
        Ok(s) => s,
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    };
    let now = Utc::now();

    // Find the assignment that owns this resource.
    let owner_entry = state.active.iter().find(|(_, assignment)| {
        assignment
            .files
            .iter()
            .any(|f| req.resource.starts_with(f.as_str()) || f.starts_with(req.resource.as_str()))
    });

    let (owner, guardrails) = match owner_entry {
        Some((owner, assignment)) => (Some(owner.clone()), assignment.guardrails.clone()),
        None => (None, vec![]),
    };

    // Check TTL expiry: if the claim has an expires_at and it has passed, the
    // resource is effectively unclaimed — allow access.
    if let Some((_, assignment)) = owner_entry {
        if !assignment.expires_at.is_empty() {
            if let Ok(expires) = chrono::DateTime::parse_from_rfc3339(&assignment.expires_at) {
                if expires.with_timezone(&Utc) < now {
                    return (
                        StatusCode::OK,
                        Json(CheckLeaseResponse {
                            allowed: true,
                            owner: Some(assignment.agent.clone()),
                            guardrail_violations: vec![],
                            message: "Lease expired — resource is available.".to_string(),
                        }),
                    )
                        .into_response();
                }
            }
        }
    }

    // If owned by someone else and not expired, check guardrails.
    if let Some(ref owner_name) = owner {
        if owner_name != &req.agent {
            let response = CheckLeaseResponse {
                allowed: false,
                owner: Some(owner_name.clone()),
                guardrail_violations: vec![format!(
                    "{} holds an active lease on {}",
                    owner_name, req.resource
                )],
                message: format!(
                    "Cannot perform '{}' on '{}': owned by {}",
                    req.action, req.resource, owner_name
                ),
            };
            return (StatusCode::OK, Json(response)).into_response();
        }
    }

    // Resource is owned by the requesting agent (or unowned). Check guardrails.
    // A guardrail starting with "require:" demands the action description contains
    // the keyword after the colon. All others are advisory strings.
    let mut violations: Vec<String> = Vec::new();
    for g in &guardrails {
        if let Some(keyword) = g.strip_prefix("require:") {
            if !req.action.to_lowercase().contains(&keyword.to_lowercase()) {
                violations.push(format!("Action must satisfy guardrail: {g}"));
            }
        }
    }

    let allowed = violations.is_empty();
    let message = if allowed {
        format!(
            "Lease check passed for '{}' on '{}'",
            req.action, req.resource
        )
    } else {
        format!(
            "{} guardrail violation(s) for '{}' on '{}'",
            violations.len(),
            req.action,
            req.resource
        )
    };

    (
        StatusCode::OK,
        Json(CheckLeaseResponse {
            allowed,
            owner,
            guardrail_violations: violations,
            message,
        }),
    )
        .into_response()
}

async fn heartbeat_task(
    State(app): State<AppContext>,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let mut state = match ensure_assignments_state(&app).await {
        Ok(state) => state,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    let now = Utc::now().to_rfc3339();
    let Some(assignment) = state.active.get_mut(&req.agent) else {
        return (StatusCode::NOT_FOUND, "Claim not found for agent").into_response();
    };

    let was_stale = assignment.status == "stale";
    assignment.last_heartbeat_at = now.clone();
    assignment.status = "active".to_string();
    state.updated_at = now.clone();

    if was_stale {
        let _ = append_policy_event(
            &app,
            CoordinationPolicyEvent {
                event_id: uuid::Uuid::new_v4().to_string(),
                managed_by: state.managed_by.clone(),
                event_type: "claim_revived".to_string(),
                status: "revived".to_string(),
                agent: Some(req.agent.clone()),
                conflicting_agent: None,
                files: assignment.files.clone(),
                message: format!("Keeper revived claim for {} after a heartbeat", req.agent),
                created_at: now.clone(),
            },
        )
        .await;
    }

    match save_assignments(&app, &state).await {
        Ok(()) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn release_task(
    State(app): State<AppContext>,
    Json(req): Json<ReleaseRequest>,
) -> impl IntoResponse {
    let mut state = match ensure_assignments_state(&app).await {
        Ok(state) => state,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };

    state.active.remove(&req.agent);
    state.updated_at = Utc::now().to_rfc3339();

    let _ = append_policy_event(
        &app,
        CoordinationPolicyEvent {
            event_id: uuid::Uuid::new_v4().to_string(),
            managed_by: state.managed_by.clone(),
            event_type: "claim_released".to_string(),
            status: "released".to_string(),
            agent: Some(req.agent.clone()),
            conflicting_agent: None,
            files: Vec::new(),
            message: format!("Keeper recorded release for {}", req.agent),
            created_at: Utc::now().to_rfc3339(),
        },
    )
    .await;

    match save_assignments(&app, &state).await {
        Ok(()) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

// ── Scout draft ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ScoutDraftRequest {
    intent: String,
    product: String,
    #[serde(default)]
    product_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ScoutDraftResponse {
    spec_yaml: String,
    spec_path: String,
    spec_id: String,
}

/// Generate a spec YAML from natural-language intent.
/// Writes a draft to factory/specs/drafts/<id>.yaml so /api/runs/start can use it directly.
async fn scout_draft(
    State(app): State<AppContext>,
    Json(req): Json<ScoutDraftRequest>,
) -> impl IntoResponse {
    let intent = req.intent.trim().to_string();
    let product = req.product.trim().to_string();
    let product_path = req
        .product_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if intent.is_empty() || product.is_empty() {
        return (StatusCode::BAD_REQUEST, "intent and product are required").into_response();
    }

    let spec_id = format!(
        "{}-draft-{}",
        slugify(&product),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    let operator_model_context =
        match best_effort_operator_model_root(&app, product_path.as_deref()) {
            Some(root) => app
                .load_effective_operator_model_context(Some(&root))
                .await
                .unwrap_or(None),
            None => app
                .load_effective_operator_model_context(None)
                .await
                .unwrap_or(None),
        };

    let spec = maybe_llm_draft_spec(
        &app,
        &spec_id,
        &intent,
        &product,
        product_path.as_deref(),
        operator_model_context.as_ref(),
    )
    .await
    .unwrap_or_else(|| {
        fallback_scout_draft_spec(
            &spec_id,
            &intent,
            &product,
            product_path.as_deref(),
            operator_model_context.as_ref(),
        )
    });

    let spec_yaml = match serde_yaml::to_string(&spec) {
        Ok(text) => text,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let drafts_dir = app.paths.factory.join("specs").join("drafts");
    if let Err(e) = tokio::fs::create_dir_all(&drafts_dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    let spec_filename = format!("{spec_id}.yaml");
    let spec_path_abs = drafts_dir.join(&spec_filename);
    let spec_path_rel = format!("factory/specs/drafts/{spec_filename}");

    if let Err(e) = tokio::fs::write(&spec_path_abs, spec_yaml.as_bytes()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    (
        StatusCode::OK,
        Json(ScoutDraftResponse {
            spec_yaml,
            spec_path: spec_path_rel,
            spec_id,
        }),
    )
        .into_response()
}

/// Load the top-3 patterns from `commissioning-brief.json` for the given profile.
/// Returns an empty string if no brief exists or loading fails.
async fn load_commissioning_brief_patterns(app: &AppContext, profile_id: &str) -> String {
    let profile = match app.operator_models.get_profile(profile_id).await {
        Ok(Some(p)) => p,
        _ => return String::new(),
    };
    let brief_path = app
        .operator_models
        .export_root_for_profile(&app.paths, &profile)
        .join("commissioning-brief.json");

    let json = match tokio::fs::read_to_string(&brief_path).await {
        Ok(j) => j,
        Err(_) => return String::new(),
    };
    let brief: crate::models::CommissioningBrief = match serde_json::from_str(&json) {
        Ok(b) => b,
        Err(_) => return String::new(),
    };

    brief
        .top_patterns
        .iter()
        .take(3)
        .enumerate()
        .map(|(i, p)| format!("{}. {p}", i + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

fn best_effort_operator_model_root(app: &AppContext, raw: Option<&str>) -> Option<PathBuf> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = PathBuf::from(raw);
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        app.paths.root.join(candidate)
    };
    absolute.canonicalize().ok().filter(|path| path.is_dir())
}

async fn maybe_llm_draft_spec(
    app: &AppContext,
    spec_id: &str,
    intent: &str,
    product: &str,
    product_path: Option<&str>,
    operator_model_context: Option<&OperatorModelContext>,
) -> Option<Spec> {
    let provider = llm::build_provider("scout", "claude", &app.paths.setup)?;
    let operator_context_json = operator_model_context
        .map(|context| serde_json::to_string_pretty(context).unwrap_or_default())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "null".to_string());

    // Inject top-3 commissioning brief patterns if a brief exists for this profile.
    let brief_patterns_text = if let Some(ctx) = operator_model_context {
        load_commissioning_brief_patterns(app, &ctx.profile_id).await
    } else {
        String::new()
    };

    let product_path_text = product_path.unwrap_or("products/<product>");
    let brief_section = if brief_patterns_text.is_empty() {
        String::new()
    } else {
        format!("\n\nCOMMISSIONING BRIEF (operator's top patterns — treat as strong constraints):\n{brief_patterns_text}")
    };
    let request = LlmRequest::simple(
        "You are Scout, drafting a Harkonnen factory spec from operator intent. Respond with valid YAML only, no markdown fences. Use this schema exactly: id, title, purpose, scope, constraints, inputs, outputs, acceptance_criteria, forbidden_behaviors, rollback_requirements, dependencies, performance_expectations, security_expectations. Make the draft concrete, bounded, and operational. If operator-model context is present, treat its guardrails, escalation rules, dependencies, and rhythms as first-class commissioning constraints. If a commissioning brief is present, incorporate its top patterns into constraints and acceptance_criteria.",
        format!(
            "SPEC ID: {spec_id}\nPRODUCT: {product}\nPRODUCT PATH: {product_path_text}\n\nINTENT:\n{intent}\n\nOPERATOR MODEL CONTEXT:\n```json\n{operator_context_json}\n```{brief_section}\n\nReturn YAML only.",
        ),
    );
    let response = provider.complete(request).await.ok()?;
    let body = response.content.trim();
    let stripped = body
        .trim_start_matches("```yaml")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let mut parsed = serde_yaml::from_str::<Spec>(stripped).ok()?;
    parsed.id = spec_id.to_string();
    if parsed.title.trim().is_empty() {
        parsed.title = title_case(product);
    }
    if parsed.scope.is_empty() {
        parsed.scope.push(product.to_string());
    }
    Some(parsed)
}

fn fallback_scout_draft_spec(
    spec_id: &str,
    intent: &str,
    product: &str,
    product_path: Option<&str>,
    operator_model_context: Option<&OperatorModelContext>,
) -> Spec {
    let lines: Vec<&str> = intent
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let purpose = lines.first().copied().unwrap_or(intent).to_string();
    let mut acceptance_criteria = lines
        .iter()
        .skip(1)
        .map(|line| (*line).to_string())
        .collect::<Vec<_>>();
    if acceptance_criteria.is_empty() {
        acceptance_criteria.push("run completes without errors".to_string());
    }

    let mut constraints = vec![
        format!("remain within the {product} workspace boundary"),
        "do not modify files outside the target product".to_string(),
    ];
    let mut dependencies = Vec::new();
    let mut security_expectations =
        vec!["secrets must not appear in logs or artifact bundles".to_string()];

    if let Some(context) = operator_model_context {
        for item in context.guardrails.iter().take(3) {
            push_unique(
                &mut constraints,
                format!("operator model guardrail: {item}"),
            );
        }
        for item in context.dependencies.iter().take(3) {
            push_unique(&mut dependencies, format!("operator dependency: {item}"));
        }
        if !context.escalation_rules.is_empty() {
            push_unique(
                &mut acceptance_criteria,
                "approval and escalation boundaries remain explicit for any action outside the operator model".to_string(),
            );
        }
        if context
            .guardrails
            .iter()
            .any(|item| contains_security_signal(item))
        {
            push_unique(
                &mut security_expectations,
                "operator-defined approval, boundary, and credential handling rules are preserved"
                    .to_string(),
            );
        }
    }

    Spec {
        id: spec_id.to_string(),
        title: title_case(product),
        purpose,
        scope: vec![product.to_string()],
        constraints,
        inputs: vec![format!(
            "product directory: {}",
            product_path.unwrap_or(&format!("products/{product}/"))
        )],
        outputs: vec![
            "implementation artifacts in the run workspace".to_string(),
            "validation.json with pass/fail verdict".to_string(),
        ],
        acceptance_criteria,
        forbidden_behaviors: vec![
            "deleting unrelated files".to_string(),
            "reaching outside the workspace boundary".to_string(),
        ],
        rollback_requirements: vec![
            "retain prior artifacts unless explicitly cleaned up".to_string()
        ],
        dependencies,
        performance_expectations: vec!["commands should complete in a reasonable time".to_string()],
        security_expectations,
        twin_services: Vec::new(),
        project_components: Vec::new(),
        scenario_blueprint: None,
        worker_harness: None,
        test_commands: Vec::new(),
    }
}

fn push_unique(items: &mut Vec<String>, value: String) {
    if items.iter().any(|existing| existing == &value) {
        return;
    }
    items.push(value);
}

fn contains_security_signal(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "secret",
        "credential",
        "auth",
        "permission",
        "security",
        "boundary",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn slugify(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn title_case(s: &str) -> String {
    s.split(['-', '_', ' '])
        .filter(|p| !p.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ── Start run ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct StartRunRequest {
    spec: String,
    #[serde(default)]
    product: Option<String>,
    #[serde(default)]
    product_path: Option<String>,
    #[serde(default)]
    spec_yaml: Option<String>,
    #[serde(default = "default_true")]
    run_hidden_scenarios: bool,
}

fn default_true() -> bool {
    true
}

async fn get_setup_check(State(app): State<AppContext>) -> impl IntoResponse {
    let setup = &app.paths.setup;
    let providers = [
        ("claude", setup.providers.claude.as_ref()),
        ("gemini", setup.providers.gemini.as_ref()),
        ("codex", setup.providers.codex.as_ref()),
    ]
    .into_iter()
    .filter_map(|(name, provider)| {
        provider.map(|provider| SetupCheckProviderStatus {
            name: name.to_string(),
            enabled: provider.enabled,
            api_key_env: provider.api_key_env.clone(),
            configured: std::env::var(&provider.api_key_env).is_ok(),
            model: provider.model.clone(),
        })
    })
    .collect::<Vec<_>>();

    let agent_routes = [
        "scout", "keeper", "mason", "piper", "ash", "bramble", "sable", "flint", "coobie",
    ]
    .into_iter()
    .map(|agent| {
        (
            agent.to_string(),
            setup.resolve_agent_provider_name(agent, "default"),
        )
    })
    .collect::<HashMap<_, _>>();

    let mcp_servers = setup
        .mcp
        .as_ref()
        .map(|mcp| {
            mcp.servers
                .iter()
                .map(|server| SetupCheckMcpStatus {
                    name: server.name.clone(),
                    command: server.command.clone(),
                    available: command_available(&server.command),
                    aliases: server.tool_aliases.clone().unwrap_or_default(),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mcp_self = setup
        .mcp
        .as_ref()
        .and_then(|mcp| mcp.self_server.as_ref())
        .map(|self_server| SetupCheckMcpSelfStatus {
            enabled: self_server.enabled,
            transport: self_server.transport.clone(),
            host: self_server.host.clone(),
            port: self_server.port,
            auth_required: self_server.auth_required,
        });

    (
        StatusCode::OK,
        Json(SetupCheckResponse {
            setup_name: setup.setup.name.clone(),
            platform: setup.setup.platform.clone(),
            default_provider: setup.providers.default.clone(),
            providers,
            agent_routes,
            mcp_servers,
            mcp_self,
        }),
    )
        .into_response()
}

async fn post_spec_validate(
    State(app): State<AppContext>,
    Json(req): Json<SpecValidateRequest>,
) -> impl IntoResponse {
    let spec_yaml = req
        .spec_yaml
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let spec_path = req
        .path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let spec = if let Some(spec_yaml) = spec_yaml {
        match serde_yaml::from_str::<Spec>(spec_yaml) {
            Ok(spec) => spec,
            Err(error) => {
                return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
            }
        }
    } else if let Some(spec_path) = spec_path {
        let resolved = resolve_spec_path(&app, spec_path);
        match crate::spec::load_spec(&resolved) {
            Ok(spec) => spec,
            Err(error) => {
                return (StatusCode::BAD_REQUEST, error.to_string()).into_response();
            }
        }
    } else {
        return (StatusCode::BAD_REQUEST, "path or spec_yaml is required").into_response();
    };

    (
        StatusCode::OK,
        Json(SpecValidateResponse {
            valid: true,
            spec_id: spec.id,
            title: spec.title,
        }),
    )
        .into_response()
}

async fn post_memory_init(State(app): State<AppContext>) -> impl IntoResponse {
    match app.memory_store.init(&app.paths.setup).await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleOperationResponse {
                ok: true,
                message: "Memory initialized".to_string(),
            }),
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_memory_index(State(app): State<AppContext>) -> impl IntoResponse {
    match app.memory_store.reindex().await {
        Ok(()) => (
            StatusCode::OK,
            Json(SimpleOperationResponse {
                ok: true,
                message: "Memory reindexed".to_string(),
            }),
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

/// `GET /api/memory/updates` — list all persisted memory supersession records.
async fn get_memory_updates(State(app): State<AppContext>) -> impl IntoResponse {
    match app.list_memory_updates().await {
        Ok(records) => (StatusCode::OK, Json(records)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_memory_update_review(
    State(app): State<AppContext>,
    Path(id): Path<String>,
    Json(request): Json<MemoryUpdateReviewRequest>,
) -> impl IntoResponse {
    match app
        .review_memory_update(
            &id,
            &request.status,
            request.reviewed_by.as_deref(),
            request.review_note.as_deref(),
        )
        .await
    {
        Ok(record) => (StatusCode::OK, Json(record)).into_response(),
        Err(error) => {
            let message = error.to_string();
            let status = if message.contains("not found") {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::BAD_REQUEST
            };
            (status, message).into_response()
        }
    }
}

async fn get_run_report(
    State(app): State<AppContext>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match reporting::build_report(&app, &id).await {
        Ok(report) => (StatusCode::OK, Json(RunReportResponse { report })).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_run_package(
    State(app): State<AppContext>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match app.package_artifacts(&id).await {
        Ok(path) => (
            StatusCode::OK,
            Json(RunPackageResponse {
                path: path.display().to_string(),
            }),
        )
            .into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn start_run(
    State(app): State<AppContext>,
    Json(req): Json<StartRunRequest>,
) -> impl IntoResponse {
    let spec_ref = req.spec.trim();
    let product = req
        .product
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let product_path = req
        .product_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let spec_yaml = req
        .spec_yaml
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if spec_ref.is_empty() {
        return (StatusCode::BAD_REQUEST, "spec is required").into_response();
    }
    if product.is_none() && product_path.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            "product or product_path is required",
        )
            .into_response();
    }

    let spec_path = resolve_spec_path(&app, spec_ref);

    if let Some(spec_yaml) = spec_yaml {
        if let Err(e) = serde_yaml::from_str::<Spec>(&spec_yaml) {
            return (
                StatusCode::BAD_REQUEST,
                format!("draft spec yaml is invalid: {e}"),
            )
                .into_response();
        }

        let spec_path_buf = PathBuf::from(&spec_path);
        let spec_path_abs = if spec_path_buf.is_absolute() {
            spec_path_buf
        } else {
            app.paths.root.join(spec_path_buf)
        };

        if let Some(parent) = spec_path_abs.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
        }

        if let Err(e) = tokio::fs::write(&spec_path_abs, spec_yaml.as_bytes()).await {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    }

    let run_req = RunRequest {
        spec_path,
        product: if product_path.is_some() {
            None
        } else {
            product
        },
        product_path,
        run_hidden_scenarios: req.run_hidden_scenarios,
        failure_harness: None,
    };

    match app.start_run(run_req).await {
        Ok(run) => (StatusCode::OK, Json(run)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn resolve_spec_path(app: &AppContext, spec: &str) -> String {
    // If it looks like a path, use it directly
    if spec.ends_with(".yaml") || spec.ends_with(".yml") || spec.contains('/') {
        return spec.to_string();
    }
    // Otherwise treat it as a spec id: look in drafts first, then examples
    let drafts = app
        .paths
        .factory
        .join("specs")
        .join("drafts")
        .join(format!("{spec}.yaml"));
    if drafts.exists() {
        return drafts.to_string_lossy().into_owned();
    }
    let examples = app
        .paths
        .factory
        .join("specs")
        .join("examples")
        .join(format!("{spec}.yaml"));
    if examples.exists() {
        return examples.to_string_lossy().into_owned();
    }
    spec.to_string()
}

// ── Memory note ───────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MemoryNoteRequest {
    note: String,
    tags: Vec<String>,
}

async fn add_memory_note(
    Path(run_id): Path<String>,
    State(app): State<AppContext>,
    Json(req): Json<MemoryNoteRequest>,
) -> impl IntoResponse {
    if req.note.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "note is required").into_response();
    }

    // Write note as a markdown file in factory/memory/ so Coobie picks it up on next retrieval
    let note_id = format!(
        "run-note-{}-{}",
        &run_id[..8],
        &uuid::Uuid::new_v4().to_string()[..6]
    );
    let summary = req
        .note
        .lines()
        .next()
        .unwrap_or("Human run note")
        .to_string();

    let mut all_tags = req.tags.clone();
    all_tags.push("human-note".to_string());
    all_tags.push(format!("run:{}", &run_id[..8]));

    let tags_yaml = all_tags
        .iter()
        .map(|t| format!("  - {t}"))
        .collect::<Vec<_>>()
        .join("\n");

    let content = format!(
        "---\nid: {note_id}\ntags:\n{tags_yaml}\nsummary: {summary}\n---\n\n{note}\n",
        note_id = note_id,
        tags_yaml = tags_yaml,
        summary = summary,
        note = req.note.trim(),
    );

    let note_path = app
        .paths
        .factory
        .join("memory")
        .join(format!("{note_id}.md"));

    if let Err(e) = tokio::fs::create_dir_all(note_path.parent().unwrap()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    if let Err(e) = tokio::fs::write(&note_path, content.as_bytes()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // Rebuild the memory index so the note is immediately searchable
    let _ = app.memory_store.reindex().await;

    (
        StatusCode::OK,
        Json(serde_json::json!({ "id": note_id, "path": note_path })),
    )
        .into_response()
}

async fn get_tesseract_scene(State(app): State<AppContext>) -> impl IntoResponse {
    let runs = match app.list_runs(30).await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let mut run_reports: Vec<(RunRecord, Option<CausalReport>)> = Vec::new();
    for run in runs {
        let report_path = app
            .paths
            .workspaces
            .join(&run.run_id)
            .join("run")
            .join("causal_report.json");
        let report = read_optional_json::<CausalReport>(&report_path)
            .await
            .unwrap_or(None);
        run_reports.push((run, report));
    }

    let scene = tesseract::build_scene(run_reports);
    (StatusCode::OK, Json(scene)).into_response()
}

async fn get_capacity(State(app): State<AppContext>) -> impl IntoResponse {
    let path = app.paths.factory.join("state").join("capacity.json");
    match tokio::fs::read_to_string(&path).await {
        Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
            Ok(json) => (StatusCode::OK, Json(json)).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
        Err(_) => (StatusCode::NOT_FOUND, "capacity.json not found").into_response(),
    }
}

async fn list_run_artifacts(
    Path(run_id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    let run_dir = app.paths.workspaces.join(&run_id).join("run");
    match tokio::fs::read_dir(&run_dir).await {
        Ok(mut dir) => {
            let mut files: Vec<serde_json::Value> = Vec::new();
            while let Ok(Some(entry)) = dir.next_entry().await {
                let name = entry.file_name().to_string_lossy().to_string();
                let ext = std::path::Path::new(&name)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_string();
                let size = entry.metadata().await.map(|m| m.len()).unwrap_or(0);
                files.push(serde_json::json!({ "name": name, "ext": ext, "size": size }));
            }
            files.sort_by(|a, b| {
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or(""))
            });
            (StatusCode::OK, Json(files)).into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "run directory not found").into_response(),
    }
}

async fn get_run_artifact(
    Path((run_id, name)): Path<(String, String)>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    if name.contains('/') || name.contains("..") {
        return (StatusCode::BAD_REQUEST, "invalid artifact name").into_response();
    }
    let path = app.paths.workspaces.join(&run_id).join("run").join(&name);
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => {
            let content_type = if name.ends_with(".json") {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, content_type)],
                content,
            )
                .into_response()
        }
        Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
    }
}

// ── PackChat handlers ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ListThreadsQuery {
    run_id: Option<String>,
    #[serde(default)]
    thread_kind: Option<ChatThreadKind>,
    #[serde(default = "default_thread_limit")]
    limit: usize,
}

fn default_thread_limit() -> usize {
    50
}

async fn list_chat_threads(
    Query(q): Query<ListThreadsQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app
        .chat
        .list_threads(q.run_id.as_deref(), q.thread_kind.as_ref(), q.limit)
        .await
    {
        Ok(threads) => (StatusCode::OK, Json(threads)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_open_thread(
    State(app): State<AppContext>,
    Json(req): Json<OpenThreadRequest>,
) -> impl IntoResponse {
    match app.chat.open_thread(&req).await {
        Ok(thread) => (StatusCode::CREATED, Json(thread)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_chat_thread(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.chat.get_thread(&id).await {
        Ok(Some(thread)) => (StatusCode::OK, Json(thread)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "thread not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_chat_messages(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.chat.list_messages(&id).await {
        Ok(messages) => (StatusCode::OK, Json(messages)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_chat_message(
    Path(id): Path<String>,
    State(app): State<AppContext>,
    Json(req): Json<PostMessageRequest>,
) -> impl IntoResponse {
    let thread = match app.chat.get_thread(&id).await {
        Ok(Some(t)) => t,
        Ok(None) => return (StatusCode::NOT_FOUND, "thread not found").into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    match dispatch_message(&app.chat, &app.paths, &thread, &req).await {
        Ok(response) => {
            if let Some(run_id) = thread.run_id.as_deref() {
                if let Err(error) = app.process_memory_candidates(Some(run_id), 10).await {
                    tracing::warn!(run_id = %run_id, error = %error, "memory candidate processing skipped after PackChat post");
                }
            }
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn default_resume_if_exists() -> bool {
    true
}

fn normalize_project_root(app: &AppContext, raw: &str) -> Result<PathBuf, String> {
    let candidate = PathBuf::from(raw.trim());
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        app.paths.root.join(candidate)
    };
    let canonical = absolute.canonicalize().map_err(|e| e.to_string())?;
    if !canonical.is_dir() {
        return Err(format!(
            "project_root is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn default_project_profile_name(project_root: &FsPath) -> String {
    project_root
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| format!("{} operator model", value))
        .unwrap_or_else(|| "project operator model".to_string())
}

async fn operator_model_profile_response(
    app: &AppContext,
    profile: OperatorModelProfile,
) -> Result<OperatorModelProfileResponse, String> {
    let active_session = app
        .operator_models
        .find_active_session_for_profile(&profile.profile_id)
        .await
        .map_err(|e| e.to_string())?;
    let active_thread = match active_session
        .as_ref()
        .and_then(|session| session.thread_id.as_deref())
    {
        Some(thread_id) => app
            .chat
            .get_thread(thread_id)
            .await
            .map_err(|e| e.to_string())?,
        None => None,
    };
    Ok(OperatorModelProfileResponse {
        export_root: app
            .operator_models
            .export_root_for_profile(&app.paths, &profile)
            .display()
            .to_string(),
        light_global_topics: crate::operator_model::LIGHT_GLOBAL_PROFILE_TOPICS
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        profile,
        active_session,
        active_thread,
    })
}

async fn list_operator_model_profiles(
    Query(q): Query<ListOperatorModelProfilesQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    let profiles = match q.scope {
        Some(scope) => app.operator_models.list_profiles_by_scope(scope).await,
        None => app.operator_models.list_profiles().await,
    };

    match profiles {
        Ok(profiles) => {
            let mut responses = Vec::with_capacity(profiles.len());
            for profile in profiles {
                match operator_model_profile_response(&app, profile).await {
                    Ok(response) => responses.push(response),
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e).into_response();
                    }
                }
            }
            (StatusCode::OK, Json(responses)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_operator_model_profile(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.operator_models.get_profile(&id).await {
        Ok(Some(profile)) => match operator_model_profile_response(&app, profile).await {
            Ok(response) => (StatusCode::OK, Json(response)).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "operator-model profile not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_operator_model_session(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    let session = match app.operator_models.get_session(&id).await {
        Ok(Some(session)) => session,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "operator-model session not found").into_response()
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let profile = match app.operator_models.get_profile(&session.profile_id).await {
        Ok(Some(profile)) => profile,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "operator-model profile not found").into_response()
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let thread = match session.thread_id.as_deref() {
        Some(thread_id) => match app.chat.get_thread(thread_id).await {
            Ok(Some(thread)) => thread,
            Ok(None) => {
                return (StatusCode::NOT_FOUND, "operator-model thread not found").into_response()
            }
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
        None => {
            return (
                StatusCode::NOT_FOUND,
                "operator-model session has no thread",
            )
                .into_response()
        }
    };

    let response = OperatorModelSessionResponse {
        export_root: app
            .operator_models
            .export_root_for_profile(&app.paths, &profile)
            .display()
            .to_string(),
        profile,
        session,
        thread,
        reused_existing_session: true,
    };
    (StatusCode::OK, Json(response)).into_response()
}

async fn post_start_operator_model_session(
    State(app): State<AppContext>,
    Json(req): Json<StartOperatorModelSessionRequest>,
) -> impl IntoResponse {
    let project_root = match normalize_project_root(&app, &req.project_root) {
        Ok(path) => path,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };

    let display_name = req
        .display_name
        .clone()
        .unwrap_or_else(|| default_project_profile_name(&project_root));

    let profile = match app
        .operator_models
        .ensure_project_profile(&project_root, &display_name)
        .await
    {
        Ok(profile) => profile,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let existing_session = if req.resume_if_exists {
        match app
            .operator_models
            .find_active_session_for_profile(&profile.profile_id)
            .await
        {
            Ok(session) => session,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    } else {
        None
    };

    let project_root_text = match project_root.to_str() {
        Some(value) => value.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                format!(
                    "project_root is not valid UTF-8: {}",
                    project_root.display()
                ),
            )
                .into_response()
        }
    };

    let thread_title = req
        .title
        .clone()
        .unwrap_or_else(|| format!("Operator model: {}", display_name));

    let mut reused_existing_session = false;
    let session = if let Some(session) = existing_session {
        reused_existing_session = true;
        session
    } else {
        let thread = match app
            .chat
            .open_thread(&OpenThreadRequest {
                run_id: None,
                spec_id: None,
                title: Some(thread_title.clone()),
                thread_kind: ChatThreadKind::OperatorModel,
                metadata_json: Some(serde_json::json!({
                    "scope": "project",
                    "profile_id": profile.profile_id,
                    "project_root": project_root_text,
                    "pending_layer": crate::operator_model::DEFAULT_OPERATOR_MODEL_LAYER,
                })),
            })
            .await
        {
            Ok(thread) => thread,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };

        match app
            .operator_models
            .create_session(
                &profile.profile_id,
                Some(&thread.thread_id),
                Some(crate::operator_model::DEFAULT_OPERATOR_MODEL_LAYER),
                req.started_by.as_deref(),
            )
            .await
        {
            Ok(session) => session,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        }
    };

    let thread = match session.thread_id.as_deref() {
        Some(thread_id) => match app.chat.get_thread(thread_id).await {
            Ok(Some(thread)) => thread,
            Ok(None) => {
                let thread = match app
                    .chat
                    .open_thread(&OpenThreadRequest {
                        run_id: None,
                        spec_id: None,
                        title: Some(thread_title),
                        thread_kind: ChatThreadKind::OperatorModel,
                        metadata_json: Some(serde_json::json!({
                            "scope": "project",
                            "profile_id": profile.profile_id,
                            "project_root": project_root_text,
                            "session_id": session.session_id,
                            "pending_layer": session.pending_layer.clone().unwrap_or_else(|| crate::operator_model::DEFAULT_OPERATOR_MODEL_LAYER.to_string()),
                        })),
                    })
                    .await
                {
                    Ok(thread) => thread,
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                    }
                };
                if let Err(e) = app
                    .operator_models
                    .update_session_thread(&session.session_id, &thread.thread_id)
                    .await
                {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
                }
                thread
            }
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        },
        None => {
            let thread = match app
                .chat
                .open_thread(&OpenThreadRequest {
                    run_id: None,
                    spec_id: None,
                    title: Some(thread_title),
                    thread_kind: ChatThreadKind::OperatorModel,
                    metadata_json: Some(serde_json::json!({
                        "scope": "project",
                        "profile_id": profile.profile_id,
                        "project_root": project_root_text,
                        "session_id": session.session_id,
                        "pending_layer": session.pending_layer.clone().unwrap_or_else(|| crate::operator_model::DEFAULT_OPERATOR_MODEL_LAYER.to_string()),
                    })),
                })
                .await
            {
                Ok(thread) => thread,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            };
            if let Err(e) = app
                .operator_models
                .update_session_thread(&session.session_id, &thread.thread_id)
                .await
            {
                return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
            }
            thread
        }
    };

    if !reused_existing_session {
        let kickoff = format!(
            "I'm ready to build the operator model for `{}`. We'll treat this as a project-scoped profile and stamp the repo under `.harkonnen/operator-model/`. Let's start with operating rhythms: what recurring work, triggers, or timing patterns shape how work actually moves in this repo? Optional question for the initial questionnaire: do you want a personal supervisor card for the operator, and if so which reference image should travel with the markdown template at `/actioncards/user-supervisor-card-template.md`? If not, Jerry remains the default supervisor representation in the system.",
            project_root_text
        );
        if let Err(e) = app
            .chat
            .append_message(
                &thread.thread_id,
                "agent",
                Some("coobie"),
                None,
                &kickoff,
                None,
            )
            .await
        {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }
    }

    let response = OperatorModelSessionResponse {
        export_root: app
            .operator_models
            .export_root_for_profile(&app.paths, &profile)
            .display()
            .to_string(),
        profile,
        session,
        thread,
        reused_existing_session,
    };

    (StatusCode::CREATED, Json(response)).into_response()
}

#[derive(Debug, Deserialize)]
struct ApproveOperatorModelLayerRequest {
    /// The layer being approved (e.g. "operating_rhythms", "recurring_decisions").
    layer: String,
    /// The PackChat thread_id for this session (used for transcript synthesis).
    thread_id: String,
    #[serde(default)]
    approved_by: Option<String>,
}

async fn post_approve_operator_model_layer(
    State(app): State<AppContext>,
    Path(session_id): Path<String>,
    Json(req): Json<ApproveOperatorModelLayerRequest>,
) -> impl IntoResponse {
    let checkpoint = match app
        .approve_operator_model_layer(
            &session_id,
            &req.layer,
            &req.thread_id,
            req.approved_by.as_deref(),
        )
        .await
    {
        Ok(cp) => cp,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    // After approving, check if the session is now complete and generate the brief.
    let session = match app.operator_models.get_session(&session_id).await {
        Ok(Some(s)) => s,
        Ok(None) => {
            return (StatusCode::NOT_FOUND, "session not found".to_string()).into_response()
        }
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let brief = if session.status == "completed" {
        match app.generate_commissioning_brief(&session.profile_id).await {
            Ok(brief) => Some(brief),
            Err(e) => {
                tracing::warn!(
                    "failed to generate commissioning brief for {}: {e}",
                    session.profile_id
                );
                None
            }
        }
    } else {
        None
    };

    // Prompt the next layer if not complete.
    if session.status == "active" {
        if let Some(next) = session.pending_layer.as_deref() {
            let next_prompt = layer_transition_prompt(next);
            let _ = app
                .chat
                .append_message(
                    &req.thread_id,
                    "agent",
                    Some("coobie"),
                    None,
                    &next_prompt,
                    None,
                )
                .await;
        }
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "checkpoint": checkpoint,
            "session_status": session.status,
            "pending_layer": session.pending_layer,
            "commissioning_brief": brief,
        })),
    )
        .into_response()
}

async fn get_operator_model_commissioning_brief(
    State(app): State<AppContext>,
    Path(profile_id): Path<String>,
) -> impl IntoResponse {
    match app.generate_commissioning_brief(&profile_id).await {
        Ok(brief) => (StatusCode::OK, Json(brief)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn layer_transition_prompt(next_layer: &str) -> String {
    match next_layer {
        "recurring_decisions" => {
            "Layer 1 (operating rhythms) approved. Moving to Layer 2: **recurring decisions**.\n\n\
            Walk me through the decisions you find yourself making repeatedly in this repo — \
            things like: how you choose between approaches, when you escalate, what tools or \
            patterns you default to, and where your risk tolerance sits. \
            Be concrete — \"I always X when Y\" is more useful than general preferences."
                .to_string()
        }
        other => format!(
            "Layer approved. Moving to the next layer: **{other}**. \
            Please share what's relevant for this area."
        ),
    }
}

// ── Health and operational endpoints ─────────────────────────────────────────

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_secs: u64,
    db_ok: bool,
    memory_index_ok: bool,
}

#[derive(Debug, Serialize)]
struct ServerStatusResponse {
    active_runs: usize,
    agent_claim_count: usize,
    memory_entry_count: usize,
    last_benchmark_run: Option<String>,
}

async fn get_health(State(app): State<AppContext>) -> impl IntoResponse {
    let db_ok = sqlx::query("SELECT 1").fetch_one(&app.pool).await.is_ok();

    let memory_index_ok = app.paths.memory.join("index.json").exists();

    let body = HealthResponse {
        status: if db_ok { "ok" } else { "degraded" },
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: app.started_at.elapsed().as_secs(),
        db_ok,
        memory_index_ok,
    };

    let code = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (code, Json(body)).into_response()
}

async fn get_server_status(State(app): State<AppContext>) -> impl IntoResponse {
    let active_runs =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM runs WHERE status = 'running'")
            .fetch_one(&app.pool)
            .await
            .unwrap_or(0) as usize;

    let agent_claim_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM assignments WHERE status = 'active'")
            .fetch_one(&app.pool)
            .await
            .unwrap_or(0) as usize;

    let memory_entry_count = app
        .memory_store
        .list_entries()
        .await
        .map(|v| v.len())
        .unwrap_or(0);

    let last_benchmark_run: Option<String> = sqlx::query_scalar(
        "SELECT created_at FROM benchmark_runs ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(&app.pool)
    .await
    .unwrap_or(None);

    (
        StatusCode::OK,
        Json(ServerStatusResponse {
            active_runs,
            agent_claim_count,
            memory_entry_count,
            last_benchmark_run,
        }),
    )
        .into_response()
}
