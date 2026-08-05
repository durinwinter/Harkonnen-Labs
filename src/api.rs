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
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::{
    calvin_archive::SoulBootstrapDocument,
    causal_graph::{CausalGraphStatus, CausalGraphStatusResponse},
    chat::{dispatch_message, ChatThread, ChatThreadKind, OpenThreadRequest, PostMessageRequest},
    coobie::CausalReport,
    db,
    llm::{self, LlmRequest},
    memory::{MemoryRetrievalHit, MemoryStore},
    models::{
        AgentExecution, AgentRuntimeState, BlackboardState, BriefingBlock, ConsolidationCandidate,
        CoobieBriefing, DecisionRecord, EvidenceAnnotation, EvidenceAnnotationBundle,
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

#[derive(Debug, Deserialize)]
struct BriefingQuery {
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Serialize)]
struct RunBriefingResponse {
    run_id: String,
    scope: String,
    artifact: String,
    block_count: usize,
    total_block_tokens: u32,
    blocks: Vec<BriefingBlock>,
    briefing: CoobieBriefing,
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

#[derive(Debug, Serialize)]
struct CausalFailureHistoryMaterializeResponse {
    run_id: String,
    json_artifact: String,
    markdown_artifact: String,
    replay_query_count: usize,
}

#[derive(Debug, Serialize)]
struct E2eReadinessMaterializeResponse {
    run_id: String,
    json_artifact: String,
    markdown_artifact: String,
    status: String,
    missing_artifact_count: usize,
}

#[derive(Debug, Serialize)]
struct E2eEvidenceManifestMaterializeResponse {
    run_id: String,
    json_artifact: String,
    markdown_artifact: String,
    status: String,
    present_artifact_count: usize,
    required_artifact_count: usize,
}

#[derive(Debug, Serialize)]
struct E2eEvidenceBundleMaterializeResponse {
    run_id: String,
    json_artifact: String,
    markdown_artifact: String,
    status: String,
    artifact_score: f64,
    bundle_artifacts: Vec<String>,
    bundle_artifact_urls: Vec<String>,
}

#[derive(Debug, Serialize)]
struct E2eReadinessIndexResponse {
    schema: &'static str,
    generated_at: DateTime<Utc>,
    run_count: usize,
    ready_count: usize,
    best_run_id: Option<String>,
    entries: Vec<E2eReadinessIndexEntry>,
}

#[derive(Debug, Serialize)]
struct E2eReadinessIndexEntry {
    run_id: String,
    spec_id: String,
    product: String,
    run_status: String,
    status: String,
    health_status: String,
    artifact_score: f64,
    ready_artifact_count: usize,
    required_artifact_count: usize,
    missing_artifact_count: usize,
    next_action: Option<String>,
    updated_at: DateTime<Utc>,
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
        .route("/api/e2e-readiness", get(get_e2e_readiness_index))
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
        .route("/api/runs/:id/health", get(get_run_health))
        .route("/api/runs/:id/e2e-readiness", get(get_run_e2e_readiness))
        .route(
            "/api/runs/:id/e2e-evidence-bundle",
            get(get_run_e2e_evidence_bundle),
        )
        .route(
            "/api/runs/:id/e2e-readiness/materialize",
            post(post_materialize_e2e_readiness),
        )
        .route(
            "/api/runs/:id/e2e-evidence-manifest/materialize",
            post(post_materialize_e2e_evidence_manifest),
        )
        .route(
            "/api/runs/:id/e2e-evidence-bundle/materialize",
            post(post_materialize_e2e_evidence_bundle),
        )
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
        .route(
            "/api/runs/:id/behavioral-change",
            get(get_behavioral_change_report),
        )
        .route(
            "/api/runs/:id/prediction-reinforcements",
            get(get_prediction_success_reinforcements),
        )
        .route(
            "/api/runs/:id/memory-influence-exclusions",
            get(get_memory_influence_exclusions),
        )
        .route(
            "/api/runs/:id/context-utilization",
            get(get_context_utilization),
        )
        .route(
            "/api/context-utilization/baseline",
            get(get_context_utilization_baseline),
        )
        .route("/api/chat", post(post_chat))
        .route("/api/coobie/query", post(post_coobie_query))
        .route("/api/agent-state", get(list_agent_state))
        .route("/api/agent-state/:name", get(get_agent_state))
        .route("/api/causal-graph/status", get(get_causal_graph_status))
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
        .route("/api/runs/:id/briefing", get(get_run_briefing))
        .route("/api/runs/:id/coobie-briefing", get(get_coobie_briefing))
        .route("/api/runs/:id/coobie-response", get(get_coobie_response))
        .route("/api/runs/:id/coobie-signals", get(get_coobie_signals))
        .route("/api/runs/:id/causal-report", get(get_causal_report))
        .route("/api/runs/:id/causal-events", get(get_run_causal_events))
        .route(
            "/api/runs/:id/causal-graph-projection",
            get(get_run_causal_graph_projection),
        )
        .route(
            "/api/runs/:id/causal-failure-history",
            get(get_run_causal_failure_history),
        )
        .route(
            "/api/runs/:id/causal-failure-history/export",
            get(get_run_causal_failure_history_export),
        )
        .route(
            "/api/runs/:id/causal-failure-history/export/materialize",
            post(post_materialize_causal_failure_history_export),
        )
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

async fn get_e2e_readiness_index(State(app): State<AppContext>) -> impl IntoResponse {
    match build_e2e_readiness_index(&app, 20).await {
        Ok(index) => (StatusCode::OK, Json(index)).into_response(),
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

async fn get_run_health(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_run_health(&app, &id).await {
        Ok(Some(health)) => (StatusCode::OK, Json(health)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_e2e_readiness(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_run_e2e_readiness(&app, &id).await {
        Ok(Some(readiness)) => (StatusCode::OK, Json(readiness)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_e2e_evidence_bundle(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match read_e2e_evidence_bundle(&app, &id).await {
        Ok(Some(bundle)) => (StatusCode::OK, Json(bundle)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "E2E evidence bundle not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_materialize_e2e_readiness(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match materialize_e2e_readiness(&app, &id).await {
        Ok(Some(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_materialize_e2e_evidence_manifest(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match materialize_e2e_evidence_manifest(&app, &id).await {
        Ok(Some(response)) => (StatusCode::OK, Json(response)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_materialize_e2e_evidence_bundle(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match materialize_e2e_evidence_bundle(&app, &id).await {
        Ok(Some(response)) => (StatusCode::OK, Json(response)).into_response(),
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

async fn get_prediction_success_reinforcements(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.list_prediction_success_reinforcements(Some(&id)).await {
            Ok(reinforcements) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "run_id": id,
                    "total": reinforcements.len(),
                    "reinforcements": reinforcements,
                })),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_memory_influence_exclusions(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.list_memory_influence_exclusions(Some(&id)).await {
            Ok(exclusions) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "run_id": id,
                    "total": exclusions.len(),
                    "exclusions": exclusions,
                })),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_behavioral_change_report(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match app.load_behavioral_change_report(&id).await {
            Ok(report) => (
                StatusCode::OK,
                Json(serde_json::json!({
                    "run_id": id,
                    "report": report,
                })),
            )
                .into_response(),
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_context_utilization(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_context_utilization_report(&app, &id).await {
        Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn get_context_utilization_baseline(State(app): State<AppContext>) -> impl IntoResponse {
    match build_context_utilization_baseline(&app, 10).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn build_context_utilization_report(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    if app.get_run(id).await?.is_none() {
        return Ok(None);
    }
    let phase_attributions = app.list_phase_attributions_for_run(id).await?;
    let pull_records = app.list_context_pull_records(id).await?;
    let briefing_hits_provided: usize = phase_attributions
        .iter()
        .map(|record| record.briefing_hits_provided)
        .sum();
    let briefing_tokens_used: u32 = phase_attributions
        .iter()
        .map(|record| record.briefing_tokens_used)
        .sum();
    let pull_tokens_returned: u32 = pull_records
        .iter()
        .map(|record| record.tokens_returned)
        .sum();
    let triggered_pulls = pull_records
        .iter()
        .filter(|record| record.trigger.is_some())
        .count();
    let unexpected_discovery_pulls = pull_records
        .iter()
        .filter(|record| record.trigger.as_deref() == Some("unexpected_discovery"))
        .count();
    let utilized_briefing_hits = phase_attributions
        .iter()
        .filter(|record| {
            record
                .memory_hits
                .iter()
                .any(|hit| context_hit_referenced_by_pull(hit, &pull_records))
        })
        .count();
    let utilization_rate = if phase_attributions.is_empty() {
        0.0
    } else {
        utilized_briefing_hits as f64 / phase_attributions.len() as f64
    };
    let utilization_status = if phase_attributions.is_empty() {
        "no_briefing"
    } else if utilization_rate < 0.2 {
        "low"
    } else {
        "healthy"
    };

    Ok(Some(serde_json::json!({
        "run_id": id,
        "summary": {
            "phase_attribution_count": phase_attributions.len(),
            "briefing_hits_provided": briefing_hits_provided,
            "briefing_tokens_used": briefing_tokens_used,
            "mid_task_pull_count": pull_records.len(),
            "mid_task_pull_tokens": pull_tokens_returned,
            "triggered_pull_count": triggered_pulls,
            "unexpected_discovery_pull_count": unexpected_discovery_pulls,
            "utilized_briefing_hits": utilized_briefing_hits,
            "utilization_rate": utilization_rate,
            "utilization_status": utilization_status,
        },
        "phase_attributions": phase_attributions,
        "pull_records": pull_records,
    })))
}

async fn build_context_utilization_baseline(
    app: &AppContext,
    target_runs: usize,
) -> anyhow::Result<serde_json::Value> {
    let runs = app.list_runs(target_runs as i64).await?;
    let mut entries = Vec::new();
    let mut rates = Vec::new();
    for run in runs {
        let Some(report) = build_context_utilization_report(app, &run.run_id).await? else {
            continue;
        };
        let summary = report
            .get("summary")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        if let Some(rate) = summary
            .get("utilization_rate")
            .and_then(serde_json::Value::as_f64)
        {
            rates.push(rate);
        }
        entries.push(serde_json::json!({
            "run_id": run.run_id,
            "spec_id": run.spec_id,
            "status": summary.get("utilization_status").cloned().unwrap_or_else(|| serde_json::json!("unknown")),
            "utilization_rate": summary.get("utilization_rate").cloned().unwrap_or_else(|| serde_json::json!(0.0)),
            "phase_attribution_count": summary.get("phase_attribution_count").cloned().unwrap_or_else(|| serde_json::json!(0)),
            "mid_task_pull_count": summary.get("mid_task_pull_count").cloned().unwrap_or_else(|| serde_json::json!(0)),
            "updated_at": run.updated_at,
        }));
    }
    let sample_count = entries.len();
    let average_utilization_rate = if rates.is_empty() {
        0.0
    } else {
        rates.iter().sum::<f64>() / rates.len() as f64
    };
    let minimum_utilization_rate = rates.iter().copied().reduce(f64::min).unwrap_or_default();

    Ok(serde_json::json!({
        "schema": "harkonnen.context_utilization_baseline.v1",
        "target_run_count": target_runs,
        "sample_count": sample_count,
        "baseline_complete": sample_count >= target_runs,
        "average_utilization_rate": average_utilization_rate,
        "minimum_utilization_rate": minimum_utilization_rate,
        "entries": entries,
    }))
}

fn context_hit_referenced_by_pull(hit: &str, pulls: &[crate::models::ContextPullRecord]) -> bool {
    let terms = hit
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_ascii_lowercase)
        .filter(|term| term.len() >= 5)
        .take(12)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return false;
    }
    pulls.iter().any(|pull| {
        let haystack = format!(
            "{} {}",
            pull.query.to_ascii_lowercase(),
            pull.hit_previews
                .iter()
                .map(|preview| preview.to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        );
        terms.iter().any(|term| haystack.contains(term))
    })
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

async fn get_run_briefing(
    Path(id): Path<String>,
    Query(query): Query<BriefingQuery>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let Some((scope, artifact)) = briefing_scope_artifact(query.scope.as_deref()) else {
                return (
                    StatusCode::BAD_REQUEST,
                    "scope must be one of coobie_preflight, scout_preflight, mason_preflight, sable_preflight",
                )
                    .into_response();
            };
            let briefing_path = app.paths.workspaces.join(&id).join("run").join(&artifact);
            match read_optional_json::<CoobieBriefing>(&briefing_path).await {
                Ok(Some(briefing)) => {
                    let blocks = briefing.briefing_blocks.clone();
                    let total_block_tokens =
                        blocks.iter().map(|block| block.token_count).sum::<u32>();
                    let response = RunBriefingResponse {
                        run_id: id,
                        scope,
                        artifact,
                        block_count: blocks.len(),
                        total_block_tokens,
                        blocks,
                        briefing,
                    };
                    (StatusCode::OK, Json(response)).into_response()
                }
                Ok(None) => (StatusCode::NOT_FOUND, "Briefing not yet generated").into_response(),
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

fn briefing_scope_artifact(scope: Option<&str>) -> Option<(String, String)> {
    let normalized = scope
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("coobie_preflight")
        .to_ascii_lowercase()
        .replace('-', "_");
    match normalized.as_str() {
        "coobie" | "coobie_preflight" => Some((
            "coobie_preflight".to_string(),
            "coobie_briefing.json".to_string(),
        )),
        "scout" | "scout_preflight" => Some((
            "scout_preflight".to_string(),
            "scout_briefing.json".to_string(),
        )),
        "mason" | "mason_preflight" => Some((
            "mason_preflight".to_string(),
            "mason_briefing.json".to_string(),
        )),
        "sable" | "sable_preflight" => Some((
            "sable_preflight".to_string(),
            "sable_briefing.json".to_string(),
        )),
        _ => None,
    }
}

async fn list_agent_state(State(app): State<AppContext>) -> impl IntoResponse {
    match crate::db::list_agent_state(&app.pool).await {
        Ok(state) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_agent_state(
    Path(name): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match crate::db::get_agent_state(&app.pool, &name).await {
        Ok(Some(state)) => (StatusCode::OK, Json(state)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Agent state not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_causal_graph_status(State(app): State<AppContext>) -> impl IntoResponse {
    let config = app.causal_graph.config();
    let projection_count = match db::count_causal_graph_projections(&app.pool).await {
        Ok(count) => count,
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };
    let latest_projection = match db::list_causal_graph_projection_summaries(&app.pool, 1).await {
        Ok(mut records) => records.pop(),
        Err(error) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
        }
    };
    // Probe the live store's actual current reachability instead of
    // trusting static config or a startup-time snapshot. `ping()` performs a
    // real, bounded round-trip against the backing store (for
    // `TypeDbCausalGraphStore`, a fresh read transaction + cheap query,
    // capped at `PING_TIMEOUT` — see `src/causal_graph.rs`), so this
    // reflects whether TypeDB is reachable *right now*, not just at
    // `AppContext` construction. It never panics and never returns an
    // `Err`, so there's no fallible branch to handle here.
    let status = app.causal_graph.ping().await;

    let note = match status {
        CausalGraphStatus::Ready => Some(
            "TypeDB 3.x live adapter is connected and serving typed causal graph queries; the SQLite projection ledger continues to mirror runs for replay and inspection.".to_string()
        ),
        CausalGraphStatus::Unavailable => Some(
            "TypeDB 3.x is configured but currently unreachable; falling back to the SQLite projection ledger and memory retrieval.".to_string()
        ),
        CausalGraphStatus::Disabled => Some(
            "TypeDB is disabled; run graphs are mirrored into the SQLite projection ledger for inspectability and replay.".to_string()
        ),
    };

    (
        StatusCode::OK,
        Json(CausalGraphStatusResponse {
            status,
            backend: config.backend.clone(),
            enabled: config.enabled,
            url: config.url.clone(),
            database: config.database.clone(),
            schema_path: config.schema_path.clone(),
            reasoning_mode: config.reasoning_mode.clone(),
            projection_count,
            latest_projection,
            note,
        }),
    )
        .into_response()
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

async fn get_run_causal_graph_projection(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => match db::get_causal_graph_projection(&app.pool, &id).await {
            Ok(Some(record)) => {
                (StatusCode::OK, Json(inspect_projection_record(record))).into_response()
            }
            Ok(None) => match app.get_run_causal_graph(&id).await {
                Ok(_) => match db::get_causal_graph_projection(&app.pool, &id).await {
                    Ok(Some(record)) => {
                        (StatusCode::OK, Json(inspect_projection_record(record))).into_response()
                    }
                    Ok(None) => (
                        StatusCode::NOT_FOUND,
                        "Causal graph projection not generated",
                    )
                        .into_response(),
                    Err(error) => {
                        (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                    }
                },
                Err(error) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response()
                }
            },
            Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
        },
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_causal_failure_history(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_causal_failure_history(&app, &id, 8).await {
        Ok(Some(history)) => (StatusCode::OK, Json(history)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_run_causal_failure_history_export(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match build_causal_failure_history_export(&app, &id, 8).await {
        Ok(Some(export)) => (StatusCode::OK, Json(export)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn post_materialize_causal_failure_history_export(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match materialize_causal_failure_history_export(&app, &id).await {
        Ok(Some(response)) => (StatusCode::OK, Json(response)).into_response(),
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
    let causal_graph_result = if looks_like_causal_graph_query(query) {
        let result = app
            .causal_graph
            .query(crate::causal_graph::CausalGraphQuery {
                question: query.to_string(),
                run_id: run_id.map(str::to_string),
                spec_id: None,
                limit: 6,
            })
            .await?;
        retrieval_path.push(format!("typed_causal_graph:{:?}", result.status));
        sources.push(CoobieQuerySource {
            kind: "typed_causal_graph".to_string(),
            label: format!("{} ({:?})", result.database, result.status),
            run_id: run_id.map(str::to_string),
            phase: None,
            artifact: Some(app.causal_graph.config().schema_path.clone()),
            hop: None,
            query: Some(query.to_string()),
            score: None,
            status: Some(format!("{:?}", result.status).to_ascii_lowercase()),
            superseded_by: None,
            challenged_by: Vec::new(),
            note: result.note.clone(),
        });
        Some(result)
    } else {
        None
    };
    let projection_hits = if looks_like_causal_graph_query(query) {
        search_causal_graph_projection_ledger(app, run_id, query, 6).await?
    } else {
        Vec::new()
    };
    let failure_history = if let Some(run_id) = run_id {
        if looks_like_causal_graph_query(query) && should_search_spec_projection_history(query) {
            build_causal_failure_history(app, run_id, 8).await?
        } else {
            None
        }
    } else {
        None
    };
    if !projection_hits.is_empty() {
        retrieval_path.push("causal_graph_projection_ledger".to_string());
        for hit in &projection_hits {
            sources.push(CoobieQuerySource {
                kind: "causal_graph_projection".to_string(),
                label: hit.label.clone(),
                run_id: hit.evidence_refs.first().and_then(|evidence| {
                    evidence.strip_prefix("run:").map(|value| value.to_string())
                }),
                phase: None,
                artifact: Some("causal_graph_projections".to_string()),
                hop: None,
                query: Some(query.to_string()),
                score: Some(hit.confidence),
                status: Some("sqlite_projection".to_string()),
                superseded_by: None,
                challenged_by: Vec::new(),
                note: Some(hit.summary.clone()),
            });
        }
    }
    if let Some(history) = failure_history.as_ref() {
        retrieval_path.push("same_spec_causal_failure_history".to_string());
        for cause in history.repeated_causes.iter().take(4) {
            sources.push(CoobieQuerySource {
                kind: "causal_failure_history".to_string(),
                label: cause.cause_id.clone(),
                run_id: Some(history.anchor_run_id.clone()),
                phase: None,
                artifact: Some("causal_graph_projections".to_string()),
                hop: None,
                query: Some(query.to_string()),
                score: Some(cause.average_confidence),
                status: Some("same_spec_history".to_string()),
                superseded_by: None,
                challenged_by: Vec::new(),
                note: Some(format!(
                    "{} occurrence(s) across {} same-spec run(s): {}",
                    cause.count,
                    cause.run_ids.len(),
                    cause.run_ids.join(", ")
                )),
            });
        }
    }
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
        if let Some(graph) = causal_graph_result
            .as_ref()
            .and_then(format_causal_graph_note)
        {
            response.push(' ');
            response.push_str(&graph);
        }
        if let Some(summary) = format_projection_ledger_summary(&projection_hits) {
            response.push(' ');
            response.push_str(&summary);
        }
        if let Some(summary) = failure_history
            .as_ref()
            .and_then(format_failure_history_summary)
        {
            response.push(' ');
            response.push_str(&summary);
        }
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response,
            retrieval_path,
            confidence: if memory_hits.is_empty()
                && projection_hits.is_empty()
                && failure_history
                    .as_ref()
                    .is_none_or(|history| history.repeated_causes.is_empty())
            {
                0.72
            } else {
                0.82
            },
            sources,
        });
    }

    if let Some(summary) = format_projection_ledger_summary(&projection_hits) {
        let memory_summary = format_memory_chain_summary(&memory_hits)
            .map(|value| format!(" {value}."))
            .unwrap_or_default();
        let graph_note = causal_graph_result
            .as_ref()
            .and_then(format_causal_graph_note)
            .map(|note| format!(" {note}"))
            .unwrap_or_default();
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: format!(
                "I do not have a run in working memory yet, but {summary}{memory_summary}{graph_note}"
            ),
            retrieval_path,
            confidence: 0.68,
            sources,
        });
    }

    if let Some(summary) = format_memory_chain_summary(&memory_hits) {
        let graph_note = causal_graph_result
            .as_ref()
            .and_then(format_causal_graph_note)
            .map(|note| format!(" {note}"))
            .unwrap_or_default();
        return Ok(CoobieQueryResponse {
            agent: "coobie".to_string(),
            response: format!(
                "I do not have a run in working memory yet, but {}.{}",
                summary, graph_note
            ),
            retrieval_path,
            confidence: 0.61,
            sources,
        });
    }

    retrieval_path.push("working_memory".to_string());
    retrieval_path.push("memory_chain:hop_1_empty".to_string());
    let graph_note = causal_graph_result
        .as_ref()
        .and_then(format_causal_graph_note)
        .map(|note| format!(" {note}"))
        .unwrap_or_default();

    Ok(CoobieQueryResponse {
        agent: "coobie".to_string(),
        response: format!("I do not have a run in working memory yet. Commission a run or pass a run_id and I can answer from the blackboard, lessons, causal history, and memory chain retrieval.{graph_note}"),
        retrieval_path,
        confidence: 0.42,
        sources,
    })
}

fn looks_like_causal_graph_query(query: &str) -> bool {
    let normalized = query.to_ascii_lowercase();
    [
        "cause",
        "caused",
        "causal",
        "failure",
        "failures",
        "intervention",
        "counterfactual",
    ]
    .iter()
    .any(|term| normalized.contains(term))
}

fn format_causal_graph_note(
    result: &crate::causal_graph::CausalGraphQueryResult,
) -> Option<String> {
    match result.status {
        crate::causal_graph::CausalGraphStatus::Ready if !result.hits.is_empty() => Some(format!(
            "Typed causal graph returned {} graph hit(s).",
            result.hits.len()
        )),
        crate::causal_graph::CausalGraphStatus::Unavailable
        | crate::causal_graph::CausalGraphStatus::Disabled => result.note.clone(),
        _ => None,
    }
}

fn inspect_projection_record(
    record: crate::causal_graph::CausalGraphProjectionRecord,
) -> crate::causal_graph::CausalGraphProjectionInspection {
    let highlights =
        serde_json::from_value::<crate::models::RunCausalGraph>(record.graph_json.clone())
            .map(|graph| {
                let mut hits = Vec::new();
                collect_projection_hits(&graph, &[], 8, &mut hits);
                hits
            })
            .unwrap_or_default();

    crate::causal_graph::CausalGraphProjectionInspection { record, highlights }
}

async fn build_causal_failure_history(
    app: &AppContext,
    run_id: &str,
    limit: usize,
) -> anyhow::Result<Option<crate::causal_graph::CausalSpecFailureHistory>> {
    let Some(run) = app.get_run(run_id).await? else {
        return Ok(None);
    };
    let records =
        db::list_causal_graph_projections_for_spec(&app.pool, &run.spec_id, limit as i64).await?;
    let projection_count = records.len() as u64;
    let mut runs = Vec::new();
    let mut cause_totals: BTreeMap<String, (u64, f64, Vec<String>)> = BTreeMap::new();

    for record in records {
        let graph =
            serde_json::from_value::<crate::models::RunCausalGraph>(record.graph_json.clone())?;
        let failed_episode_count = graph
            .episodes
            .iter()
            .filter(|episode| {
                matches!(
                    episode.episode.outcome.as_deref(),
                    Some("failure") | Some("blocked")
                )
            })
            .count() as u64;
        let top_causes = projection_hypothesis_hits(&graph, 3);
        if failed_episode_count == 0 && top_causes.is_empty() {
            continue;
        }
        for cause in &top_causes {
            let cause_id = cause
                .label
                .strip_prefix("hypothesis:")
                .unwrap_or(&cause.label)
                .to_string();
            let entry = cause_totals
                .entry(cause_id)
                .or_insert_with(|| (0, 0.0, Vec::new()));
            entry.0 += 1;
            entry.1 += cause.confidence;
            if !entry.2.iter().any(|existing| existing == &graph.run_id) {
                entry.2.push(graph.run_id.clone());
            }
        }
        runs.push(crate::causal_graph::CausalFailureRunSummary {
            run_id: graph.run_id,
            projected_at: record.projected_at,
            failed_episode_count,
            hypothesis_count: graph.hypotheses.len() as u64,
            top_causes,
        });
    }

    let mut repeated_causes = cause_totals
        .into_iter()
        .map(|(cause_id, (count, confidence_sum, run_ids))| {
            crate::causal_graph::CausalRepeatedCause {
                cause_id,
                count,
                average_confidence: if count == 0 {
                    0.0
                } else {
                    confidence_sum / count as f64
                },
                run_ids,
            }
        })
        .collect::<Vec<_>>();
    repeated_causes.sort_by(|left, right| {
        right.count.cmp(&left.count).then_with(|| {
            right
                .average_confidence
                .partial_cmp(&left.average_confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    Ok(Some(crate::causal_graph::CausalSpecFailureHistory {
        anchor_run_id: run_id.to_string(),
        spec_id: run.spec_id,
        projection_count,
        failure_run_count: runs.len() as u64,
        repeated_causes,
        runs,
    }))
}

async fn build_causal_failure_history_export(
    app: &AppContext,
    run_id: &str,
    limit: usize,
) -> anyhow::Result<Option<crate::causal_graph::CausalFailureHistoryReplayExport>> {
    let Some(history) = build_causal_failure_history(app, run_id, limit).await? else {
        return Ok(None);
    };
    let schema_path = app.causal_graph.config().schema_path.clone();
    let replay_queries = causal_failure_history_replay_queries(&history);

    Ok(Some(
        crate::causal_graph::CausalFailureHistoryReplayExport {
            schema: "harkonnen.causal_failure_history_replay.v1".to_string(),
            generated_at: Utc::now(),
            anchor_run_id: history.anchor_run_id.clone(),
            spec_id: history.spec_id.clone(),
            projection_source: "sqlite.causal_graph_projections".to_string(),
            typedb_schema_path: schema_path,
            history,
            typedb_targets: vec![
                "agent".to_string(),
                "goal".to_string(),
                "episode".to_string(),
                "outcome".to_string(),
                "failure-mode".to_string(),
                "causal-link".to_string(),
                "causally-connects".to_string(),
            ],
            replay_queries,
        },
    ))
}

async fn materialize_causal_failure_history_export(
    app: &AppContext,
    run_id: &str,
) -> anyhow::Result<Option<CausalFailureHistoryMaterializeResponse>> {
    let Some(export) = build_causal_failure_history_export(app, run_id, 8).await? else {
        return Ok(None);
    };
    let run_dir = app.paths.workspaces.join(run_id).join("run");
    tokio::fs::create_dir_all(&run_dir).await?;
    let json_artifact = "causal_failure_history_replay.json".to_string();
    let markdown_artifact = "causal_failure_history_replay.md".to_string();
    tokio::fs::write(
        run_dir.join(&json_artifact),
        serde_json::to_string_pretty(&export)?,
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&markdown_artifact),
        render_causal_failure_history_replay_markdown(&export),
    )
    .await?;

    if let Some(mut board) =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?
    {
        push_unique(&mut board.artifact_refs, json_artifact.clone());
        push_unique(&mut board.artifact_refs, markdown_artifact.clone());
        tokio::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board)?,
        )
        .await?;
        let mut live_board = app.blackboard.write().await;
        if live_board.run_id == board.run_id {
            *live_board = board;
        }
    }

    Ok(Some(CausalFailureHistoryMaterializeResponse {
        run_id: run_id.to_string(),
        json_artifact,
        markdown_artifact,
        replay_query_count: export.replay_queries.len(),
    }))
}

fn render_causal_failure_history_replay_markdown(
    export: &crate::causal_graph::CausalFailureHistoryReplayExport,
) -> String {
    let mut lines = vec![
        "# Causal Failure History Replay".to_string(),
        String::new(),
        format!("- Schema: {}", export.schema),
        format!("- Anchor run: {}", export.anchor_run_id),
        format!("- Spec: {}", export.spec_id),
        format!("- Projection source: {}", export.projection_source),
        format!("- TypeDB schema: {}", export.typedb_schema_path),
        format!("- Failure runs: {}", export.history.failure_run_count),
        format!("- Projection rows: {}", export.history.projection_count),
        String::new(),
        "## Repeated Causes".to_string(),
    ];
    if export.history.repeated_causes.is_empty() {
        lines.push("- No repeated causes recorded.".to_string());
    } else {
        for cause in &export.history.repeated_causes {
            lines.push(format!(
                "- `{}`: {} run(s), average confidence {:.2}; supporting runs: {}",
                cause.cause_id,
                cause.count,
                cause.average_confidence,
                cause.run_ids.join(", ")
            ));
        }
    }
    lines.push(String::new());
    lines.push("## Replay Queries".to_string());
    for query in &export.replay_queries {
        lines.push(format!("- `{}`: {}", query.label, query.purpose));
        lines.push("```typeql".to_string());
        lines.push(query.typeql.clone());
        lines.push("```".to_string());
    }
    lines.push(String::new());
    lines.join("\n")
}

fn causal_failure_history_replay_queries(
    history: &crate::causal_graph::CausalSpecFailureHistory,
) -> Vec<crate::causal_graph::CausalReplayQuery> {
    let escaped_spec = typeql_string_literal(&history.spec_id);
    let mut queries = vec![
        crate::causal_graph::CausalReplayQuery {
            label: "same_spec_failed_episodes".to_string(),
            purpose: "Find failed or blocked episodes for this spec family once projections are replayed into TypeDB.".to_string(),
            typeql: format!(
                "match $g isa goal, has spec-id \"{escaped_spec}\"; (goal: $g, episode: $e) isa episode-goal; $e isa episode; $o isa outcome, has status $status; (episode: $e, outcome: $o) isa produced-outcome; {{ $status == \"failure\"; }} or {{ $status == \"blocked\"; }}; get $e, $o, $status;"
            ),
        },
        crate::causal_graph::CausalReplayQuery {
            label: "same_spec_repeated_failure_modes".to_string(),
            purpose: "Group repeated failure-mode labels for the same spec after TypeDB replay.".to_string(),
            typeql: format!(
                "match $g isa goal, has spec-id \"{escaped_spec}\"; (goal: $g, episode: $e) isa episode-goal; $f isa failure-mode, has label $label; (episode: $e, failure: $f) isa classifies-failure; get $label;"
            ),
        },
    ];

    for cause in history.repeated_causes.iter().take(4) {
        let escaped_cause = typeql_string_literal(&cause.cause_id);
        queries.push(crate::causal_graph::CausalReplayQuery {
            label: format!("cause_{}", cause.cause_id),
            purpose: format!(
                "Trace supporting causal links for repeated cause {} across same-spec runs.",
                cause.cause_id
            ),
            typeql: format!(
                "match $f isa failure-mode, has label \"{escaped_cause}\"; $c isa causal-link; (cause: $f, link: $c) isa causally-connects; get $f, $c;"
            ),
        });
    }

    queries
}

fn typeql_string_literal(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect()
}

fn projection_hypothesis_hits(
    graph: &crate::models::RunCausalGraph,
    limit: usize,
) -> Vec<crate::causal_graph::CausalGraphHit> {
    graph
        .hypotheses
        .iter()
        .take(limit)
        .map(|hypothesis| crate::causal_graph::CausalGraphHit {
            label: format!("hypothesis:{}", hypothesis.cause_id),
            summary: hypothesis.description.clone(),
            evidence_refs: vec![format!("run:{}", graph.run_id)],
            confidence: hypothesis.confidence as f64,
        })
        .collect()
}

async fn search_causal_graph_projection_ledger(
    app: &AppContext,
    run_id: Option<&str>,
    query: &str,
    limit: usize,
) -> anyhow::Result<Vec<crate::causal_graph::CausalGraphHit>> {
    let terms = causal_projection_query_terms(query);
    let records = if let Some(run_id) = run_id {
        match db::get_causal_graph_projection(&app.pool, run_id).await? {
            Some(record) if should_search_spec_projection_history(query) => {
                let mut records = spec_projection_history(app, run_id, limit.max(6)).await?;
                if !records.iter().any(|entry| entry.run_id == record.run_id) {
                    records.insert(0, record);
                }
                records
            }
            Some(record) => vec![record],
            None => {
                let _ = app.get_run_causal_graph(run_id).await;
                if should_search_spec_projection_history(query) {
                    spec_projection_history(app, run_id, limit.max(6)).await?
                } else {
                    db::get_causal_graph_projection(&app.pool, run_id)
                        .await?
                        .into_iter()
                        .collect()
                }
            }
        }
    } else {
        db::list_recent_causal_graph_projections(&app.pool, 12).await?
    };

    let mut hits = Vec::new();
    for record in records {
        let graph = serde_json::from_value::<crate::models::RunCausalGraph>(record.graph_json)?;
        collect_projection_hits(&graph, &terms, limit, &mut hits);
        if hits.len() >= limit {
            break;
        }
    }
    hits.truncate(limit);
    Ok(hits)
}

async fn spec_projection_history(
    app: &AppContext,
    run_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<crate::causal_graph::CausalGraphProjectionRecord>> {
    let Some(run) = app.get_run(run_id).await? else {
        return Ok(Vec::new());
    };
    db::list_causal_graph_projections_for_spec(&app.pool, &run.spec_id, limit as i64).await
}

fn should_search_spec_projection_history(query: &str) -> bool {
    let normalized = query.to_ascii_lowercase();
    [
        "last",
        "recent",
        "previous",
        "spec",
        "same spec",
        "failures",
    ]
    .iter()
    .any(|term| normalized.contains(term))
}

fn causal_projection_query_terms(query: &str) -> Vec<String> {
    query
        .to_ascii_lowercase()
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .filter(|term| term.len() >= 4)
        .filter(|term| {
            !matches!(
                *term,
                "what"
                    | "when"
                    | "where"
                    | "which"
                    | "from"
                    | "that"
                    | "with"
                    | "have"
                    | "were"
                    | "been"
                    | "this"
                    | "there"
                    | "into"
                    | "about"
            )
        })
        .map(str::to_string)
        .collect()
}

fn collect_projection_hits(
    graph: &crate::models::RunCausalGraph,
    terms: &[String],
    limit: usize,
    hits: &mut Vec<crate::causal_graph::CausalGraphHit>,
) {
    for hypothesis in &graph.hypotheses {
        if hits.len() >= limit {
            return;
        }
        let haystack = format!(
            "{} {} {}",
            hypothesis.cause_id,
            hypothesis.description,
            hypothesis
                .evidence
                .iter()
                .map(|evidence| evidence.summary.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        );
        if projection_text_matches(&haystack, terms) {
            hits.push(crate::causal_graph::CausalGraphHit {
                label: format!("hypothesis:{}", hypothesis.cause_id),
                summary: hypothesis.description.clone(),
                evidence_refs: vec![format!("run:{}", graph.run_id)],
                confidence: hypothesis.confidence as f64,
            });
        }
    }

    for link in &graph.links {
        if hits.len() >= limit {
            return;
        }
        if projection_text_matches(&link.summary, terms)
            || projection_text_matches(&link.link_type, terms)
        {
            hits.push(crate::causal_graph::CausalGraphHit {
                label: format!("causal-link:{}", link.link_id),
                summary: link.summary.clone(),
                evidence_refs: vec![
                    format!("run:{}", graph.run_id),
                    format!("from_event:{}", link.from_event),
                    format!("to_event:{}", link.to_event),
                ],
                confidence: link.confidence,
            });
        }
    }

    for episode in &graph.episodes {
        if hits.len() >= limit {
            return;
        }
        let outcome = episode.episode.outcome.as_deref().unwrap_or("unknown");
        let haystack = format!(
            "{} {} {}",
            episode.episode.phase, episode.episode.goal, outcome
        );
        if matches!(outcome, "failure" | "blocked") || projection_text_matches(&haystack, terms) {
            hits.push(crate::causal_graph::CausalGraphHit {
                label: format!("episode:{}", episode.episode.episode_id),
                summary: format!(
                    "{} episode ended as {}: {}",
                    episode.episode.phase, outcome, episode.episode.goal
                ),
                evidence_refs: vec![
                    format!("run:{}", graph.run_id),
                    format!("episode:{}", episode.episode.episode_id),
                ],
                confidence: episode.episode.confidence.unwrap_or(0.55),
            });
        }
    }

    for event in &graph.events {
        if hits.len() >= limit {
            return;
        }
        let haystack = format!(
            "{} {} {} {}",
            event.phase, event.agent, event.status, event.message
        );
        if projection_text_matches(&haystack, terms) {
            hits.push(crate::causal_graph::CausalGraphHit {
                label: format!("event:{}", event.event_id),
                summary: format!(
                    "{}:{} by {} - {}",
                    event.phase, event.status, event.agent, event.message
                ),
                evidence_refs: vec![
                    format!("run:{}", graph.run_id),
                    format!("event:{}", event.event_id),
                ],
                confidence: if event.status.eq_ignore_ascii_case("failed") {
                    0.8
                } else {
                    0.62
                },
            });
        }
    }
}

fn projection_text_matches(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let normalized = text.to_ascii_lowercase();
    terms.iter().any(|term| normalized.contains(term))
}

fn format_projection_ledger_summary(
    hits: &[crate::causal_graph::CausalGraphHit],
) -> Option<String> {
    if hits.is_empty() {
        return None;
    }
    let top = hits
        .iter()
        .take(3)
        .map(|hit| format!("{} ({:.2})", hit.summary, hit.confidence))
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "SQLite causal graph projection ledger surfaced {} hit(s): {}.",
        hits.len(),
        top
    ))
}

fn format_failure_history_summary(
    history: &crate::causal_graph::CausalSpecFailureHistory,
) -> Option<String> {
    if history.failure_run_count == 0 {
        return None;
    }
    if history.repeated_causes.is_empty() {
        return Some(format!(
            "Same-spec causal history found {} failure run(s) across {} projection(s), but no repeated cause has enough hypothesis evidence yet.",
            history.failure_run_count, history.projection_count
        ));
    }
    let top = history
        .repeated_causes
        .iter()
        .take(3)
        .map(|cause| {
            format!(
                "{} appeared in {} run(s), avg confidence {:.2}",
                cause.cause_id, cause.count, cause.average_confidence
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "Same-spec causal history found {} failure run(s) across {} projection(s): {}.",
        history.failure_run_count, history.projection_count, top
    ))
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

async fn build_run_health(app: &AppContext, id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let validation =
        read_optional_json::<ValidationSummary>(&run_dir.join("validation.json")).await?;
    let hidden_scenarios =
        read_optional_json::<HiddenScenarioSummary>(&run_dir.join("hidden_scenarios.json")).await?;
    let audit = app.load_plan_completion_audit(id).await?;
    let phase_attributions = app.list_phase_attributions_for_run(id).await?;
    let pull_records = app.list_context_pull_records(id).await?;
    let candidates = app.list_memory_candidates_for_run(id).await?;

    let open_blockers = blackboard
        .as_ref()
        .map(|board| board.open_blockers.clone())
        .unwrap_or_default();
    let mut memory_status_counts = HashMap::<String, usize>::new();
    for candidate in &candidates {
        *memory_status_counts
            .entry(candidate.status.clone())
            .or_default() += 1;
    }
    let memory_blocked = [
        "retry_pending",
        "waiting_openbrain",
        "held_for_review",
        "needs_reconsolidation",
    ]
    .iter()
    .any(|status| memory_status_counts.get(*status).copied().unwrap_or(0) > 0);
    let memory_review = memory_status_counts
        .get("promotion_pending")
        .copied()
        .unwrap_or(0)
        > 0;
    let validation_failed = validation.as_ref().is_some_and(|summary| !summary.passed);
    let hidden_failed = hidden_scenarios
        .as_ref()
        .is_some_and(|summary| !summary.passed);
    let audit_unresolved = audit
        .as_ref()
        .and_then(|value| value.get("unresolved_count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let utilized_briefing_hits = phase_attributions
        .iter()
        .filter(|record| {
            record
                .memory_hits
                .iter()
                .any(|hit| context_hit_referenced_by_pull(hit, &pull_records))
        })
        .count();
    let unexpected_discovery_pulls = pull_records
        .iter()
        .filter(|record| record.trigger.as_deref() == Some("unexpected_discovery"))
        .count();
    let utilization_rate = if phase_attributions.is_empty() {
        0.0
    } else {
        utilized_briefing_hits as f64 / phase_attributions.len() as f64
    };
    let context_status = if phase_attributions.is_empty() {
        "no_briefing"
    } else if utilization_rate < 0.2 {
        "low"
    } else {
        "healthy"
    };

    let mut blockers = Vec::new();
    blockers.extend(open_blockers.iter().cloned());
    if validation_failed {
        blockers.push("visible_validation_failed".to_string());
    }
    if hidden_failed {
        blockers.push("hidden_scenarios_failed".to_string());
    }
    if memory_blocked {
        blockers.push("memory_chain_blocked".to_string());
    }

    let mut review_items = Vec::new();
    if audit_unresolved > 0 {
        review_items.push(format!("{audit_unresolved} plan audit item(s) unresolved"));
    }
    if memory_review {
        review_items.push("Calvin promotion review pending".to_string());
    }
    if context_status == "low" {
        review_items.push("context utilization is low".to_string());
    }
    if unexpected_discovery_pulls > 0 {
        review_items.push(format!(
            "{unexpected_discovery_pulls} unexpected-discovery rebrief pull{} recorded",
            plural_suffix(unexpected_discovery_pulls)
        ));
    }

    let status = if !blockers.is_empty() {
        "blocked"
    } else if !review_items.is_empty() {
        "needs_review"
    } else if run.status == "completed" {
        "ready"
    } else {
        "running"
    };

    Ok(Some(serde_json::json!({
        "schema": "harkonnen.run_health.v1",
        "run_id": id,
        "status": status,
        "run_status": run.status,
        "current_phase": blackboard.as_ref().map(|board| board.current_phase.clone()),
        "blockers": blockers,
        "review_items": review_items,
        "checks": {
            "validation": {
                "present": validation.is_some(),
                "passed": validation.as_ref().map(|summary| summary.passed),
            },
            "hidden_scenarios": {
                "present": hidden_scenarios.is_some(),
                "passed": hidden_scenarios.as_ref().map(|summary| summary.passed),
            },
            "plan_audit": {
                "present": audit.is_some(),
                "unresolved_count": audit_unresolved,
            },
            "memory_chain": {
                "status": if memory_blocked { "blocked" } else if memory_review { "needs_review" } else { "clear" },
                "total_candidates": candidates.len(),
                "status_counts": memory_status_counts,
            },
            "context_utilization": {
                "status": context_status,
                "rate": utilization_rate,
                "phase_attribution_count": phase_attributions.len(),
                "mid_task_pull_count": pull_records.len(),
                "unexpected_discovery_pull_count": unexpected_discovery_pulls,
            },
        },
    })))
}

async fn build_run_e2e_readiness(
    app: &AppContext,
    id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let Some(run) = app.get_run(id).await? else {
        return Ok(None);
    };
    let run_dir = app.paths.workspaces.join(id).join("run");
    let health = build_run_health(app, id)
        .await?
        .unwrap_or_else(|| serde_json::json!({}));
    let required_artifacts = [
        ("blackboard", "blackboard.json"),
        ("coobie_briefing", "coobie_briefing.json"),
        ("phase_attributions", "phase_attributions.json"),
        ("validation", "validation.json"),
        ("hidden_scenarios", "hidden_scenarios.json"),
        ("causal_report", "causal_report.json"),
        ("plan_completion_audit", "plan_completion_audit.json"),
        ("behavioral_change_report", "behavioral_change_report.json"),
        (
            "causal_failure_replay",
            "causal_failure_history_replay.json",
        ),
    ];
    let mut checks = Vec::new();
    let mut missing = Vec::new();
    for (name, artifact) in required_artifacts {
        let present = run_dir.join(artifact).exists();
        if !present {
            missing.push(artifact.to_string());
        }
        checks.push(serde_json::json!({
            "name": name,
            "artifact": artifact,
            "present": present,
        }));
    }

    let health_status = health
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let health_blockers = health
        .get("blockers")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let ready_artifacts = checks
        .iter()
        .filter(|check| check.get("present").and_then(serde_json::Value::as_bool) == Some(true))
        .count();
    let artifact_score = if checks.is_empty() {
        0.0
    } else {
        ready_artifacts as f64 / checks.len() as f64
    };
    let status = if !health_blockers.is_empty() {
        "blocked"
    } else if missing.is_empty() && health_status == "ready" {
        "ready"
    } else {
        "needs_evidence"
    };
    let next_actions = e2e_readiness_next_actions(&missing, health_status);

    Ok(Some(serde_json::json!({
        "schema": "harkonnen.e2e_readiness.v1",
        "run_id": id,
        "spec_id": run.spec_id,
        "status": status,
        "run_status": run.status,
        "health_status": health_status,
        "artifact_score": artifact_score,
        "ready_artifact_count": ready_artifacts,
        "required_artifact_count": checks.len(),
        "missing_artifacts": missing,
        "checks": checks,
        "health_blockers": health_blockers,
        "next_actions": next_actions,
        "month_end_goal": "full_end_to_end_functionality",
    })))
}

fn e2e_readiness_next_actions(missing: &[String], health_status: &str) -> Vec<String> {
    let mut actions = Vec::new();
    if missing.iter().any(|artifact| artifact == "validation.json") {
        actions.push("run visible validation so Bramble emits validation.json".to_string());
    }
    if missing
        .iter()
        .any(|artifact| artifact == "hidden_scenarios.json")
    {
        actions.push("run hidden scenarios so Sable emits hidden_scenarios.json".to_string());
    }
    if missing
        .iter()
        .any(|artifact| artifact == "causal_report.json")
    {
        actions
            .push("complete Coobie causal ingest so causal_report.json is available".to_string());
    }
    if missing
        .iter()
        .any(|artifact| artifact == "causal_failure_history_replay.json")
    {
        actions.push(
            "materialize same-spec causal failure replay from the Causal Graph panel".to_string(),
        );
    }
    if health_status != "ready" {
        actions.push(format!("resolve run health status `{health_status}`"));
    }
    if actions.is_empty() {
        actions.push("run is ready for end-to-end evidence review".to_string());
    }
    actions
}

async fn build_e2e_readiness_index(
    app: &AppContext,
    limit: i64,
) -> anyhow::Result<E2eReadinessIndexResponse> {
    let runs = app.list_runs(limit).await?;
    let mut entries = Vec::new();
    for run in runs {
        let Some(readiness) = build_run_e2e_readiness(app, &run.run_id).await? else {
            continue;
        };
        entries.push(e2e_readiness_index_entry(&run, &readiness));
    }

    entries.sort_by(|left, right| {
        readiness_status_rank(&left.status)
            .cmp(&readiness_status_rank(&right.status))
            .then_with(|| {
                right
                    .artifact_score
                    .partial_cmp(&left.artifact_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at.cmp(&left.updated_at))
    });
    let ready_count = entries
        .iter()
        .filter(|entry| entry.status == "ready")
        .count();
    let best_run_id = entries.first().map(|entry| entry.run_id.clone());

    Ok(E2eReadinessIndexResponse {
        schema: "harkonnen.e2e_readiness_index.v1",
        generated_at: Utc::now(),
        run_count: entries.len(),
        ready_count,
        best_run_id,
        entries,
    })
}

fn e2e_readiness_index_entry(
    run: &RunRecord,
    readiness: &serde_json::Value,
) -> E2eReadinessIndexEntry {
    E2eReadinessIndexEntry {
        run_id: run.run_id.clone(),
        spec_id: run.spec_id.clone(),
        product: run.product.clone(),
        run_status: run.status.clone(),
        status: readiness
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        health_status: readiness
            .get("health_status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        artifact_score: readiness
            .get("artifact_score")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_default(),
        ready_artifact_count: readiness
            .get("ready_artifact_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize,
        required_artifact_count: readiness
            .get("required_artifact_count")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_default() as usize,
        missing_artifact_count: readiness
            .get("missing_artifacts")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        next_action: readiness
            .get("next_actions")
            .and_then(serde_json::Value::as_array)
            .and_then(|actions| actions.first())
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string),
        updated_at: run.updated_at,
    }
}

fn readiness_status_rank(status: &str) -> u8 {
    match status {
        "ready" => 0,
        "needs_evidence" => 1,
        "blocked" => 2,
        _ => 3,
    }
}

async fn materialize_e2e_readiness(
    app: &AppContext,
    run_id: &str,
) -> anyhow::Result<Option<E2eReadinessMaterializeResponse>> {
    let Some(readiness) = build_run_e2e_readiness(app, run_id).await? else {
        return Ok(None);
    };
    let run_dir = app.paths.workspaces.join(run_id).join("run");
    tokio::fs::create_dir_all(&run_dir).await?;
    let json_artifact = "e2e_readiness.json".to_string();
    let markdown_artifact = "e2e_readiness.md".to_string();
    tokio::fs::write(
        run_dir.join(&json_artifact),
        serde_json::to_string_pretty(&readiness)?,
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&markdown_artifact),
        render_e2e_readiness_markdown(&readiness),
    )
    .await?;

    if let Some(mut board) =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?
    {
        push_unique(&mut board.artifact_refs, json_artifact.clone());
        push_unique(&mut board.artifact_refs, markdown_artifact.clone());
        tokio::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board)?,
        )
        .await?;
        let mut live_board = app.blackboard.write().await;
        if live_board.run_id == board.run_id {
            *live_board = board;
        }
    }

    let status = readiness
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let missing_artifact_count = readiness
        .get("missing_artifacts")
        .and_then(serde_json::Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();

    Ok(Some(E2eReadinessMaterializeResponse {
        run_id: run_id.to_string(),
        json_artifact,
        markdown_artifact,
        status,
        missing_artifact_count,
    }))
}

fn render_e2e_readiness_markdown(readiness: &serde_json::Value) -> String {
    let status = readiness
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let run_id = readiness
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let spec_id = readiness
        .get("spec_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let health_status = readiness
        .get("health_status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let ready_artifacts = readiness
        .get("ready_artifact_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let required_artifacts = readiness
        .get("required_artifact_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let artifact_score = readiness
        .get("artifact_score")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default();

    let mut lines = vec![
        "# E2E Readiness".to_string(),
        String::new(),
        format!(
            "- Schema: {}",
            readiness
                .get("schema")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
        ),
        format!("- Run: {run_id}"),
        format!("- Spec: {spec_id}"),
        format!("- Status: {status}"),
        format!("- Health: {health_status}"),
        format!(
            "- Artifacts: {ready_artifacts}/{required_artifacts} ({:.0}%)",
            artifact_score * 100.0
        ),
        String::new(),
        "## Missing Artifacts".to_string(),
    ];

    let missing = readiness
        .get("missing_artifacts")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if missing.is_empty() {
        lines.push("- none".to_string());
    } else {
        for artifact in missing {
            if let Some(artifact) = artifact.as_str() {
                lines.push(format!("- `{artifact}`"));
            }
        }
    }

    lines.push(String::new());
    lines.push("## Next Actions".to_string());
    let actions = readiness
        .get("next_actions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if actions.is_empty() {
        lines.push("- none".to_string());
    } else {
        for action in actions {
            if let Some(action) = action.as_str() {
                lines.push(format!("- {action}"));
            }
        }
    }

    lines.push(String::new());
    lines.push("## Checks".to_string());
    for check in readiness
        .get("checks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let name = check
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("artifact");
        let artifact = check
            .get("artifact")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let marker = if check.get("present").and_then(serde_json::Value::as_bool) == Some(true) {
            "x"
        } else {
            " "
        };
        lines.push(format!("- [{marker}] {name}: `{artifact}`"));
    }

    lines.join("\n")
}

async fn materialize_e2e_evidence_manifest(
    app: &AppContext,
    run_id: &str,
) -> anyhow::Result<Option<E2eEvidenceManifestMaterializeResponse>> {
    let Some(readiness) = build_run_e2e_readiness(app, run_id).await? else {
        return Ok(None);
    };
    let run_dir = app.paths.workspaces.join(run_id).join("run");
    tokio::fs::create_dir_all(&run_dir).await?;
    let manifest = build_e2e_evidence_manifest(run_id, &run_dir, &readiness);
    let json_artifact = "e2e_evidence_manifest.json".to_string();
    let markdown_artifact = "e2e_evidence_manifest.md".to_string();
    tokio::fs::write(
        run_dir.join(&json_artifact),
        serde_json::to_string_pretty(&manifest)?,
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&markdown_artifact),
        render_e2e_evidence_manifest_markdown(&manifest),
    )
    .await?;

    if let Some(mut board) =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?
    {
        push_unique(&mut board.artifact_refs, json_artifact.clone());
        push_unique(&mut board.artifact_refs, markdown_artifact.clone());
        tokio::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board)?,
        )
        .await?;
        let mut live_board = app.blackboard.write().await;
        if live_board.run_id == board.run_id {
            *live_board = board;
        }
    }

    let status = manifest
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let present_artifact_count = manifest
        .get("present_artifact_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default() as usize;
    let required_artifact_count = manifest
        .get("required_artifact_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default() as usize;

    Ok(Some(E2eEvidenceManifestMaterializeResponse {
        run_id: run_id.to_string(),
        json_artifact,
        markdown_artifact,
        status,
        present_artifact_count,
        required_artifact_count,
    }))
}

fn build_e2e_evidence_manifest(
    run_id: &str,
    run_dir: &FsPath,
    readiness: &serde_json::Value,
) -> serde_json::Value {
    let entries = readiness
        .get("checks")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|check| {
            let name = check
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("artifact");
            let artifact = check
                .get("artifact")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let present = run_dir.join(artifact).exists();
            let size_bytes = std::fs::metadata(run_dir.join(artifact))
                .map(|metadata| metadata.len())
                .ok();
            serde_json::json!({
                "name": name,
                "artifact": artifact,
                "present": present,
                "size_bytes": size_bytes,
                "download_url": if present {
                    Some(format!("/api/runs/{run_id}/artifacts/{artifact}"))
                } else {
                    None
                },
            })
        })
        .collect::<Vec<_>>();
    let present_artifact_count = entries
        .iter()
        .filter(|entry| entry.get("present").and_then(serde_json::Value::as_bool) == Some(true))
        .count();

    serde_json::json!({
        "schema": "harkonnen.e2e_evidence_manifest.v1",
        "run_id": run_id,
        "generated_at": Utc::now(),
        "status": readiness.get("status").and_then(serde_json::Value::as_str).unwrap_or("unknown"),
        "health_status": readiness.get("health_status").and_then(serde_json::Value::as_str).unwrap_or("unknown"),
        "artifact_score": readiness.get("artifact_score").and_then(serde_json::Value::as_f64).unwrap_or_default(),
        "present_artifact_count": present_artifact_count,
        "required_artifact_count": entries.len(),
        "missing_artifacts": readiness.get("missing_artifacts").cloned().unwrap_or_else(|| serde_json::json!([])),
        "next_actions": readiness.get("next_actions").cloned().unwrap_or_else(|| serde_json::json!([])),
        "entries": entries,
    })
}

fn render_e2e_evidence_manifest_markdown(manifest: &serde_json::Value) -> String {
    let run_id = manifest
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let status = manifest
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let present_artifact_count = manifest
        .get("present_artifact_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let required_artifact_count = manifest
        .get("required_artifact_count")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default();
    let mut lines = vec![
        "# E2E Evidence Manifest".to_string(),
        String::new(),
        format!("- Run: {run_id}"),
        format!("- Status: {status}"),
        format!("- Artifacts: {present_artifact_count}/{required_artifact_count}"),
        String::new(),
        "## Artifact Checklist".to_string(),
    ];

    for entry in manifest
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let name = entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("artifact");
        let artifact = entry
            .get("artifact")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let marker = if entry.get("present").and_then(serde_json::Value::as_bool) == Some(true) {
            "x"
        } else {
            " "
        };
        let download = entry
            .get("download_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if download.is_empty() {
            lines.push(format!("- [{marker}] {name}: `{artifact}`"));
        } else {
            lines.push(format!("- [{marker}] {name}: `{artifact}` ({download})"));
        }
    }

    lines.push(String::new());
    lines.push("## Next Actions".to_string());
    let actions = manifest
        .get("next_actions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if actions.is_empty() {
        lines.push("- none".to_string());
    } else {
        for action in actions {
            if let Some(action) = action.as_str() {
                lines.push(format!("- {action}"));
            }
        }
    }

    lines.join("\n")
}

async fn materialize_e2e_evidence_bundle(
    app: &AppContext,
    run_id: &str,
) -> anyhow::Result<Option<E2eEvidenceBundleMaterializeResponse>> {
    let Some(readiness) = build_run_e2e_readiness(app, run_id).await? else {
        return Ok(None);
    };
    let run_dir = app.paths.workspaces.join(run_id).join("run");
    tokio::fs::create_dir_all(&run_dir).await?;

    let manifest = build_e2e_evidence_manifest(run_id, &run_dir, &readiness);
    let readiness_json = "e2e_readiness.json".to_string();
    let readiness_markdown = "e2e_readiness.md".to_string();
    let manifest_json = "e2e_evidence_manifest.json".to_string();
    let manifest_markdown = "e2e_evidence_manifest.md".to_string();
    let bundle_json = "e2e_evidence_bundle.json".to_string();
    let bundle_markdown = "e2e_evidence_bundle.md".to_string();

    tokio::fs::write(
        run_dir.join(&readiness_json),
        serde_json::to_string_pretty(&readiness)?,
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&readiness_markdown),
        render_e2e_readiness_markdown(&readiness),
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&manifest_json),
        serde_json::to_string_pretty(&manifest)?,
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&manifest_markdown),
        render_e2e_evidence_manifest_markdown(&manifest),
    )
    .await?;

    let bundle_artifacts = vec![
        readiness_json.clone(),
        readiness_markdown.clone(),
        manifest_json.clone(),
        manifest_markdown.clone(),
        bundle_json.clone(),
        bundle_markdown.clone(),
    ];
    let bundle_entries = bundle_artifacts
        .iter()
        .map(|artifact| {
            let path = run_dir.join(artifact);
            serde_json::json!({
                "artifact": artifact,
                "download_url": format!("/api/runs/{run_id}/artifacts/{artifact}"),
                "size_bytes": std::fs::metadata(path).map(|metadata| metadata.len()).ok(),
            })
        })
        .collect::<Vec<_>>();
    let bundle = serde_json::json!({
        "schema": "harkonnen.e2e_evidence_bundle.v1",
        "run_id": run_id,
        "generated_at": Utc::now(),
        "status": readiness.get("status").and_then(serde_json::Value::as_str).unwrap_or("unknown"),
        "artifact_score": readiness.get("artifact_score").and_then(serde_json::Value::as_f64).unwrap_or_default(),
        "readiness_artifact": readiness_json.clone(),
        "readiness_markdown": readiness_markdown.clone(),
        "manifest_artifact": manifest_json.clone(),
        "manifest_markdown": manifest_markdown.clone(),
        "bundle_artifacts": bundle_artifacts.clone(),
        "bundle_entries": bundle_entries,
        "missing_artifacts": readiness.get("missing_artifacts").cloned().unwrap_or_else(|| serde_json::json!([])),
        "next_actions": readiness.get("next_actions").cloned().unwrap_or_else(|| serde_json::json!([])),
    });
    tokio::fs::write(
        run_dir.join(&bundle_json),
        serde_json::to_string_pretty(&bundle)?,
    )
    .await?;
    tokio::fs::write(
        run_dir.join(&bundle_markdown),
        render_e2e_evidence_bundle_markdown(&bundle),
    )
    .await?;

    if let Some(mut board) =
        read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?
    {
        for artifact in bundle_artifacts_from_bundle(&bundle) {
            push_unique(&mut board.artifact_refs, artifact);
        }
        tokio::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board)?,
        )
        .await?;
        let mut live_board = app.blackboard.write().await;
        if live_board.run_id == board.run_id {
            *live_board = board;
        }
    }

    Ok(Some(E2eEvidenceBundleMaterializeResponse {
        run_id: run_id.to_string(),
        json_artifact: bundle_json,
        markdown_artifact: bundle_markdown,
        status: bundle
            .get("status")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        artifact_score: bundle
            .get("artifact_score")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_default(),
        bundle_artifacts: bundle_artifacts_from_bundle(&bundle),
        bundle_artifact_urls: bundle_artifact_urls_from_bundle(&bundle),
    }))
}

fn bundle_artifacts_from_bundle(bundle: &serde_json::Value) -> Vec<String> {
    bundle
        .get("bundle_artifacts")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|artifact| artifact.as_str().map(ToString::to_string))
        .collect()
}

fn bundle_artifact_urls_from_bundle(bundle: &serde_json::Value) -> Vec<String> {
    bundle
        .get("bundle_entries")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| {
            entry
                .get("download_url")
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

fn render_e2e_evidence_bundle_markdown(bundle: &serde_json::Value) -> String {
    let run_id = bundle
        .get("run_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let status = bundle
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let artifact_score = bundle
        .get("artifact_score")
        .and_then(serde_json::Value::as_f64)
        .unwrap_or_default();
    let mut lines = vec![
        "# E2E Evidence Bundle".to_string(),
        String::new(),
        format!("- Run: {run_id}"),
        format!("- Status: {status}"),
        format!("- Artifact score: {:.0}%", artifact_score * 100.0),
        String::new(),
        "## Bundle Artifacts".to_string(),
    ];

    for entry in bundle
        .get("bundle_entries")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
    {
        let artifact = entry
            .get("artifact")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let download = entry
            .get("download_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        lines.push(format!("- `{artifact}` ({download})"));
    }

    lines.push(String::new());
    lines.push("## Next Actions".to_string());
    let actions = bundle
        .get("next_actions")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    if actions.is_empty() {
        lines.push("- none".to_string());
    } else {
        for action in actions {
            if let Some(action) = action.as_str() {
                lines.push(format!("- {action}"));
            }
        }
    }

    lines.join("\n")
}

async fn read_e2e_evidence_bundle(
    app: &AppContext,
    run_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    if app.get_run(run_id).await?.is_none() {
        return Ok(None);
    }
    let bundle_path = app
        .paths
        .workspaces
        .join(run_id)
        .join("run")
        .join("e2e_evidence_bundle.json");
    let Some(mut bundle) = read_optional_json::<serde_json::Value>(&bundle_path).await? else {
        return Ok(None);
    };
    ensure_e2e_bundle_download_metadata(run_id, &mut bundle);
    Ok(Some(bundle))
}

fn ensure_e2e_bundle_download_metadata(run_id: &str, bundle: &mut serde_json::Value) {
    let artifacts = bundle_artifacts_from_bundle(bundle);
    if !bundle
        .get("bundle_entries")
        .and_then(serde_json::Value::as_array)
        .is_some_and(|entries| entries.len() == artifacts.len())
    {
        bundle["bundle_entries"] = serde_json::Value::Array(
            artifacts
                .iter()
                .map(|artifact| {
                    serde_json::json!({
                        "artifact": artifact,
                        "download_url": format!("/api/runs/{run_id}/artifacts/{artifact}"),
                    })
                })
                .collect(),
        );
    }
    bundle["bundle_artifact_urls"] = serde_json::Value::Array(
        bundle_artifact_urls_from_bundle(bundle)
            .into_iter()
            .map(serde_json::Value::String)
            .collect(),
    );
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

#[cfg(test)]
mod tests {
    use super::{
        briefing_scope_artifact, execute_coobie_query, get_agent_state, get_causal_graph_status,
        get_context_utilization_baseline, get_e2e_readiness_index, get_run_causal_failure_history,
        get_run_causal_failure_history_export, get_run_causal_graph_projection,
        get_run_e2e_evidence_bundle, get_run_e2e_readiness, list_agent_state,
        post_materialize_causal_failure_history_export, post_materialize_e2e_evidence_bundle,
        post_materialize_e2e_evidence_manifest, post_materialize_e2e_readiness,
    };
    use crate::{
        config::Paths,
        db,
        memory::MemoryStore,
        models::BlackboardState,
        orchestrator::AppContext,
        setup::{
            CalvinConfig, OpenBrainConfig, ProviderConfig, ProvidersConfig, SetupConfig, SetupMeta,
            SubAgentConfig, TwilightBarkConfig,
        },
    };
    use axum::{
        body::to_bytes,
        extract::{Path, State},
        http::StatusCode,
        response::IntoResponse,
    };
    use chrono::Utc;
    use std::{collections::HashMap, sync::Arc};
    use tokio::sync::RwLock;

    #[test]
    fn briefing_scope_artifact_maps_named_scopes_to_artifacts() {
        assert_eq!(
            briefing_scope_artifact(None),
            Some((
                "coobie_preflight".to_string(),
                "coobie_briefing.json".to_string()
            ))
        );
        assert_eq!(
            briefing_scope_artifact(Some("mason-preflight")),
            Some((
                "mason_preflight".to_string(),
                "mason_briefing.json".to_string()
            ))
        );
        assert_eq!(briefing_scope_artifact(Some("unknown")), None);
    }

    fn test_paths(root: &std::path::Path) -> Paths {
        let factory = root.join("factory");
        Paths {
            root: root.to_path_buf(),
            factory: factory.clone(),
            specs: factory.join("specs"),
            scenarios: factory.join("scenarios"),
            artifacts: factory.join("artifacts"),
            logs: factory.join("logs"),
            workspaces: factory.join("workspaces"),
            memory: factory.join("memory"),
            db_file: factory.join("state.db"),
            products: root.join("products"),
            setup: SetupConfig {
                setup: SetupMeta {
                    name: "test".to_string(),
                    template: None,
                    role: None,
                    organization: None,
                    platform: "test".to_string(),
                    anythingllm: Some(false),
                    openclaw: Some(false),
                },
                machine: None,
                providers: ProvidersConfig {
                    default: "claude".to_string(),
                    claude: Some(ProviderConfig {
                        provider_type: "anthropic".to_string(),
                        model: "claude-test-model".to_string(),
                        api_key_env: "ANTHROPIC_API_KEY".to_string(),
                        enabled: true,
                        credential_kind: None,
                        usage_rights: None,
                        surface: None,
                        base_url: None,
                    }),
                    gemini: None,
                    codex: None,
                    extras: HashMap::new(),
                },
                routing: None,
                mcp: None,
                calvin_archive: CalvinConfig::default(),
                twilight_bark: TwilightBarkConfig::default(),
                open_brain: OpenBrainConfig {
                    enabled: false,
                    ..Default::default()
                },
                sub_agents: SubAgentConfig::default(),
                typedb: Default::default(),
            },
        }
    }

    async fn test_app() -> (tempfile::TempDir, AppContext) {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let paths = test_paths(dir.path());
        for path in [
            &paths.factory,
            &paths.specs,
            &paths.scenarios,
            &paths.artifacts,
            &paths.logs,
            &paths.workspaces,
            &paths.memory,
            &paths.products,
            &paths.factory.join("agents").join("profiles"),
            &paths.factory.join("agents").join("contracts"),
        ] {
            std::fs::create_dir_all(path).expect("create test dir");
        }
        std::fs::write(
            paths
                .factory
                .join("agents")
                .join("profiles")
                .join("mason.yaml"),
            r#"
name: mason
display_name: Mason
role: build_retriever
provider: claude
model_override: ~
personality_file: ../personality/labrador.md
"#,
        )
        .expect("write profile");
        std::fs::write(
            paths
                .factory
                .join("agents")
                .join("contracts")
                .join("mason.yaml"),
            "invariants:\n  - stays grounded\n",
        )
        .expect("write contract");

        let pool = db::init_db(&paths).await.expect("init db");
        db::update_agent_memory_block_ids(
            &pool,
            "mason",
            [(
                "recalled_lessons".to_string(),
                "runs/run-api/mason_briefing.json#block=recalled_lessons".to_string(),
            )]
            .into_iter()
            .collect(),
        )
        .await
        .expect("block ids");
        let memory_store = MemoryStore::new(paths.memory.clone());
        let coobie = crate::coobie::SqliteCoobie::new(pool.clone());
        let (event_tx, _) = tokio::sync::broadcast::channel(16);
        let chat = crate::chat::ChatStore::new(pool.clone());
        let operator_models = crate::operator_model::OperatorModelStore::new(pool.clone());
        let dispatcher = crate::subagent::SubAgentDispatcher::new(
            paths.setup.sub_agents.clone(),
            paths.setup.clone(),
        );
        let causal_graph = crate::causal_graph::build_store(
            crate::causal_graph::CausalGraphConfig::from(&paths.setup.typedb),
        )
        .await;
        let app = AppContext {
            paths,
            pool,
            memory_store,
            blackboard: Arc::new(RwLock::new(BlackboardState::default())),
            coobie,
            embedding_store: None,
            event_tx,
            chat,
            operator_models,
            started_at: std::time::Instant::now(),
            calvin: None,
            open_brain: None,
            semantic_memory: Arc::new(crate::memory::NoopSemanticMemory),
            causal_graph,
            dispatcher,
        };
        (dir, app)
    }

    async fn response_json(response: axum::response::Response) -> serde_json::Value {
        let bytes = to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn agent_state_routes_return_canonical_rows() {
        let (_dir, app) = test_app().await;

        let list_response = list_agent_state(State(app.clone())).await.into_response();
        assert_eq!(list_response.status(), StatusCode::OK);
        let list_body = response_json(list_response).await;
        let rows = list_body.as_array().expect("array response");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["agent_name"], "mason");
        assert_eq!(rows[0]["llm_model"], "claude-test-model");

        let get_response = get_agent_state(Path("mason".to_string()), State(app))
            .await
            .into_response();
        assert_eq!(get_response.status(), StatusCode::OK);
        let get_body = response_json(get_response).await;
        assert_eq!(get_body["agent_name"], "mason");
        assert_eq!(
            get_body["memory_block_ids"]["recalled_lessons"],
            "runs/run-api/mason_briefing.json#block=recalled_lessons"
        );
    }

    #[tokio::test]
    async fn agent_state_route_returns_404_for_unknown_agent() {
        let (_dir, app) = test_app().await;

        let response = get_agent_state(Path("unknown".to_string()), State(app))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn causal_questions_include_typed_graph_boundary_when_disabled() {
        let (_dir, app) = test_app().await;

        let response = execute_coobie_query(&app, None, "What caused recent failures?", 1)
            .await
            .expect("coobie query");

        assert!(response
            .retrieval_path
            .iter()
            .any(|entry| entry.starts_with("typed_causal_graph:")));
        assert!(response
            .sources
            .iter()
            .any(|source| source.kind == "typed_causal_graph"
                && source.status.as_deref() == Some("disabled")));
        assert!(response
            .response
            .contains("TypeDB semantic graph is disabled"));
    }

    #[tokio::test]
    async fn causal_graph_status_reports_sqlite_projection_ledger() {
        let (_dir, app) = test_app().await;

        let status_response = get_causal_graph_status(State(app.clone()))
            .await
            .into_response();
        assert_eq!(status_response.status(), StatusCode::OK);
        let status_body = response_json(status_response).await;
        assert_eq!(status_body["status"], "disabled");
        assert_eq!(status_body["projection_count"], 0);

        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-causal-projection")
        .bind("spec-causal")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        sqlx::query(
            "INSERT INTO episodes (episode_id, run_id, phase, goal, outcome, confidence, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind("episode-causal-projection")
        .bind("run-causal-projection")
        .bind("validation")
        .bind("Run validation")
        .bind("success")
        .bind(1.0)
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert episode");

        let projection_response = get_run_causal_graph_projection(
            Path("run-causal-projection".to_string()),
            State(app.clone()),
        )
        .await
        .into_response();
        assert_eq!(projection_response.status(), StatusCode::OK);
        let projection_body = response_json(projection_response).await;
        assert_eq!(projection_body["run_id"], "run-causal-projection");
        assert_eq!(projection_body["status"], "sqlite_projection");
        assert_eq!(projection_body["episode_count"], 1);
        assert!(projection_body["highlights"].as_array().is_some());

        let updated_status = get_causal_graph_status(State(app)).await.into_response();
        let updated_body = response_json(updated_status).await;
        assert_eq!(updated_body["projection_count"], 1);
        assert_eq!(
            updated_body["latest_projection"]["run_id"],
            "run-causal-projection"
        );
    }

    /// Fake store used only to prove `get_causal_graph_status` reflects the
    /// live store's reported status rather than deriving it purely from
    /// static config. Its `config().enabled` is deliberately `false` while
    /// `ping()` reports `Ready` — the opposite of what config alone would
    /// imply — so this test fails under the old hardcoded
    /// `config.enabled`-only branching and passes only once the handler
    /// actually asks the store (via `ping()`, the real liveness check).
    #[derive(Debug)]
    struct FakeReadyCausalGraphStore {
        config: crate::causal_graph::CausalGraphConfig,
    }

    #[async_trait::async_trait]
    impl crate::causal_graph::CausalGraphStore for FakeReadyCausalGraphStore {
        fn config(&self) -> &crate::causal_graph::CausalGraphConfig {
            &self.config
        }

        async fn query(
            &self,
            query: crate::causal_graph::CausalGraphQuery,
        ) -> anyhow::Result<crate::causal_graph::CausalGraphQueryResult> {
            Ok(crate::causal_graph::CausalGraphQueryResult {
                status: crate::causal_graph::CausalGraphStatus::Ready,
                backend: self.config.backend.clone(),
                database: self.config.database.clone(),
                query: query.question,
                hits: Vec::new(),
                note: Some("fake store is live".to_string()),
            })
        }

        async fn ping(&self) -> crate::causal_graph::CausalGraphStatus {
            crate::causal_graph::CausalGraphStatus::Ready
        }
    }

    #[tokio::test]
    async fn causal_graph_status_reflects_live_store_not_just_config() {
        let (_dir, mut app) = test_app().await;
        // Config says disabled, but the store behind it reports Ready via
        // ping(). A status endpoint that only looked at `config.enabled`
        // would report "disabled" here; the correct behavior is to trust
        // the store's live liveness check.
        let fake_config = crate::causal_graph::CausalGraphConfig {
            backend: crate::causal_graph::CausalGraphBackend::TypeDb3,
            enabled: false,
            url: "localhost:1729".to_string(),
            database: "harkonnen_semantic".to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };
        app.causal_graph = std::sync::Arc::new(FakeReadyCausalGraphStore {
            config: fake_config,
        });

        let status_response = get_causal_graph_status(State(app)).await.into_response();
        assert_eq!(status_response.status(), StatusCode::OK);
        let status_body = response_json(status_response).await;
        assert_eq!(status_body["status"], "ready");
        assert_eq!(
            status_body["note"],
            "TypeDB 3.x live adapter is connected and serving typed causal graph queries; the SQLite projection ledger continues to mirror runs for replay and inspection."
        );
    }

    /// Regression test for the ping-path fix itself (Task 7 follow-up): a
    /// store whose `config().enabled` is `true` but whose `ping()` reports
    /// `Unavailable` must make the status endpoint say "unavailable", not
    /// "ready". This is the scenario the old `query(run_id: None)` probe got
    /// wrong — it only reflected `build_store()`'s *startup*-time store
    /// selection, so a TypeDB death mid-session (after a healthy boot) could
    /// never be observed by this endpoint. `FakeDeadCausalGraphStore` models
    /// exactly that: config still says "enabled", but the live backend is
    /// unreachable right now.
    #[derive(Debug)]
    struct FakeDeadCausalGraphStore {
        config: crate::causal_graph::CausalGraphConfig,
    }

    #[async_trait::async_trait]
    impl crate::causal_graph::CausalGraphStore for FakeDeadCausalGraphStore {
        fn config(&self) -> &crate::causal_graph::CausalGraphConfig {
            &self.config
        }

        async fn query(
            &self,
            _query: crate::causal_graph::CausalGraphQuery,
        ) -> anyhow::Result<crate::causal_graph::CausalGraphQueryResult> {
            anyhow::bail!("fake store: query should not be called by the status endpoint")
        }

        async fn ping(&self) -> crate::causal_graph::CausalGraphStatus {
            crate::causal_graph::CausalGraphStatus::Unavailable
        }
    }

    #[tokio::test]
    async fn causal_graph_status_reports_unavailable_when_ping_fails_despite_enabled_config() {
        let (_dir, mut app) = test_app().await;
        let fake_config = crate::causal_graph::CausalGraphConfig {
            backend: crate::causal_graph::CausalGraphBackend::TypeDb3,
            enabled: true,
            url: "localhost:1729".to_string(),
            database: "harkonnen_semantic".to_string(),
            schema_path: "factory/coobie_semantic/typedb/schema.tql".to_string(),
            reasoning_mode: "function_backed".to_string(),
        };
        app.causal_graph = std::sync::Arc::new(FakeDeadCausalGraphStore {
            config: fake_config,
        });

        let status_response = get_causal_graph_status(State(app)).await.into_response();
        assert_eq!(status_response.status(), StatusCode::OK);
        let status_body = response_json(status_response).await;
        assert_eq!(status_body["status"], "unavailable");
    }

    #[tokio::test]
    async fn causal_questions_use_projection_ledger_hits_without_typedb() {
        let (_dir, app) = test_app().await;
        let graph = crate::models::RunCausalGraph {
            run_id: "run-ledger-query".to_string(),
            generated_at: Utc::now(),
            episodes: vec![crate::models::EpisodeCausalState {
                episode: crate::models::EpisodeRecord {
                    episode_id: "episode-validation-failure".to_string(),
                    run_id: "run-ledger-query".to_string(),
                    phase: "validation".to_string(),
                    goal: "Run visible validation".to_string(),
                    outcome: Some("failure".to_string()),
                    confidence: Some(0.77),
                    started_at: Utc::now(),
                    ended_at: None,
                    state_before: None,
                    state_after: None,
                },
                state_diff: None,
            }],
            events: vec![crate::models::CausalEventNode {
                event_id: 42,
                run_id: "run-ledger-query".to_string(),
                episode_id: Some("episode-validation-failure".to_string()),
                phase: "validation".to_string(),
                agent: "bramble".to_string(),
                status: "failed".to_string(),
                message: "validation failed after dependency mismatch".to_string(),
                created_at: Utc::now(),
            }],
            links: Vec::new(),
            hypotheses: vec![crate::models::CausalHypothesis {
                cause_id: "cause-dependency-mismatch".to_string(),
                description: "Dependency mismatch caused validation failure".to_string(),
                confidence: 0.84,
                hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                supporting_runs: vec!["run-ledger-query".to_string()],
                evidence: Vec::new(),
                counterfactuals: Vec::new(),
            }],
        };
        db::upsert_causal_graph_projection(&app.pool, &graph, app.causal_graph.config())
            .await
            .expect("projection");

        let response = execute_coobie_query(&app, None, "What caused validation failures?", 1)
            .await
            .expect("coobie query");

        assert!(response
            .retrieval_path
            .iter()
            .any(|entry| entry == "causal_graph_projection_ledger"));
        assert!(response
            .sources
            .iter()
            .any(|source| source.kind == "causal_graph_projection"
                && source.label.contains("cause-dependency-mismatch")));
        assert!(response
            .response
            .contains("SQLite causal graph projection ledger surfaced"));
    }

    #[tokio::test]
    async fn causal_graph_projection_endpoint_returns_inspection_highlights() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-projection-inspect")
        .bind("spec-inspect")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let graph = crate::models::RunCausalGraph {
            run_id: "run-projection-inspect".to_string(),
            generated_at: Utc::now(),
            episodes: Vec::new(),
            events: Vec::new(),
            links: Vec::new(),
            hypotheses: vec![crate::models::CausalHypothesis {
                cause_id: "cause-plan-drift".to_string(),
                description: "Plan drift caused hidden scenario instability".to_string(),
                confidence: 0.73,
                hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                supporting_runs: vec!["run-projection-inspect".to_string()],
                evidence: Vec::new(),
                counterfactuals: Vec::new(),
            }],
        };
        db::upsert_causal_graph_projection(&app.pool, &graph, app.causal_graph.config())
            .await
            .expect("projection");

        let response =
            get_run_causal_graph_projection(Path("run-projection-inspect".to_string()), State(app))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["run_id"], "run-projection-inspect");
        assert_eq!(
            body["highlights"][0]["label"],
            "hypothesis:cause-plan-drift"
        );
    }

    #[tokio::test]
    async fn causal_questions_with_run_id_search_same_spec_projection_history() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        for run_id in ["run-spec-history-1", "run-spec-history-2"] {
            sqlx::query(
                "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )
            .bind(run_id)
            .bind("spec-shared-history")
            .bind("product")
            .bind("completed")
            .bind(&now)
            .execute(&app.pool)
            .await
            .expect("insert run");
        }
        let graph_one = crate::models::RunCausalGraph {
            run_id: "run-spec-history-1".to_string(),
            generated_at: Utc::now(),
            episodes: Vec::new(),
            events: Vec::new(),
            links: Vec::new(),
            hypotheses: vec![crate::models::CausalHypothesis {
                cause_id: "cause-auth-timeout".to_string(),
                description: "Auth timeout caused validation failure".to_string(),
                confidence: 0.81,
                hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                supporting_runs: vec!["run-spec-history-1".to_string()],
                evidence: Vec::new(),
                counterfactuals: Vec::new(),
            }],
        };
        let graph_two = crate::models::RunCausalGraph {
            run_id: "run-spec-history-2".to_string(),
            generated_at: Utc::now(),
            episodes: Vec::new(),
            events: Vec::new(),
            links: Vec::new(),
            hypotheses: vec![crate::models::CausalHypothesis {
                cause_id: "cause-auth-schema".to_string(),
                description: "Auth schema mismatch caused hidden scenario failure".to_string(),
                confidence: 0.79,
                hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                supporting_runs: vec!["run-spec-history-2".to_string()],
                evidence: Vec::new(),
                counterfactuals: Vec::new(),
            }],
        };
        db::upsert_causal_graph_projection(&app.pool, &graph_one, app.causal_graph.config())
            .await
            .expect("projection one");
        db::upsert_causal_graph_projection(&app.pool, &graph_two, app.causal_graph.config())
            .await
            .expect("projection two");

        let response = execute_coobie_query(
            &app,
            Some("run-spec-history-2"),
            "What caused recent failures on this spec?",
            1,
        )
        .await
        .expect("coobie query");

        let labels = response
            .sources
            .iter()
            .filter(|source| source.kind == "causal_graph_projection")
            .map(|source| source.label.as_str())
            .collect::<Vec<_>>();
        assert!(labels
            .iter()
            .any(|label| label.contains("cause-auth-timeout")));
        assert!(labels
            .iter()
            .any(|label| label.contains("cause-auth-schema")));
        assert!(response
            .retrieval_path
            .iter()
            .any(|entry| entry == "same_spec_causal_failure_history"));
        assert!(response
            .sources
            .iter()
            .any(|source| source.kind == "causal_failure_history"
                && source.label == "cause-auth-timeout"));
        assert!(response.response.contains("Same-spec causal history found"));
    }

    #[tokio::test]
    async fn causal_failure_history_summarizes_same_spec_repeated_causes() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        for run_id in ["run-history-a", "run-history-b", "run-history-c"] {
            sqlx::query(
                "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )
            .bind(run_id)
            .bind("spec-history-summary")
            .bind("product")
            .bind("completed")
            .bind(&now)
            .execute(&app.pool)
            .await
            .expect("insert run");
        }
        for run_id in ["run-history-a", "run-history-b"] {
            let graph = crate::models::RunCausalGraph {
                run_id: run_id.to_string(),
                generated_at: Utc::now(),
                episodes: vec![crate::models::EpisodeCausalState {
                    episode: crate::models::EpisodeRecord {
                        episode_id: format!("episode-{run_id}"),
                        run_id: run_id.to_string(),
                        phase: "validation".to_string(),
                        goal: "Run visible validation".to_string(),
                        outcome: Some("failure".to_string()),
                        confidence: Some(0.7),
                        started_at: Utc::now(),
                        ended_at: None,
                        state_before: None,
                        state_after: None,
                    },
                    state_diff: None,
                }],
                events: Vec::new(),
                links: Vec::new(),
                hypotheses: vec![crate::models::CausalHypothesis {
                    cause_id: "cause-shared-timeout".to_string(),
                    description: "Shared timeout caused validation failure".to_string(),
                    confidence: 0.8,
                    hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                    supporting_runs: vec![run_id.to_string()],
                    evidence: Vec::new(),
                    counterfactuals: Vec::new(),
                }],
            };
            db::upsert_causal_graph_projection(&app.pool, &graph, app.causal_graph.config())
                .await
                .expect("projection");
        }

        let response =
            get_run_causal_failure_history(Path("run-history-b".to_string()), State(app))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["spec_id"], "spec-history-summary");
        assert_eq!(body["failure_run_count"], 2);
        assert_eq!(
            body["repeated_causes"][0]["cause_id"],
            "cause-shared-timeout"
        );
        assert_eq!(body["repeated_causes"][0]["count"], 2);
    }

    #[tokio::test]
    async fn causal_failure_history_export_returns_typedb_replay_contract() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-export-history")
        .bind("spec-export-history")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let graph = crate::models::RunCausalGraph {
            run_id: "run-export-history".to_string(),
            generated_at: Utc::now(),
            episodes: vec![crate::models::EpisodeCausalState {
                episode: crate::models::EpisodeRecord {
                    episode_id: "episode-export-history".to_string(),
                    run_id: "run-export-history".to_string(),
                    phase: "validation".to_string(),
                    goal: "Run visible validation".to_string(),
                    outcome: Some("failure".to_string()),
                    confidence: Some(0.7),
                    started_at: Utc::now(),
                    ended_at: None,
                    state_before: None,
                    state_after: None,
                },
                state_diff: None,
            }],
            events: Vec::new(),
            links: Vec::new(),
            hypotheses: vec![crate::models::CausalHypothesis {
                cause_id: "cause-export-timeout".to_string(),
                description: "Export timeout caused validation failure".to_string(),
                confidence: 0.82,
                hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                supporting_runs: vec!["run-export-history".to_string()],
                evidence: Vec::new(),
                counterfactuals: Vec::new(),
            }],
        };
        db::upsert_causal_graph_projection(&app.pool, &graph, app.causal_graph.config())
            .await
            .expect("projection");

        let response = get_run_causal_failure_history_export(
            Path("run-export-history".to_string()),
            State(app),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["schema"], "harkonnen.causal_failure_history_replay.v1");
        assert_eq!(body["history"]["failure_run_count"], 1);
        assert!(body["typedb_targets"]
            .as_array()
            .expect("typedb targets")
            .iter()
            .any(|target| target == "causal-link"));
        assert!(body["replay_queries"]
            .as_array()
            .expect("replay queries")
            .iter()
            .any(|query| query["typeql"]
                .as_str()
                .unwrap_or_default()
                .contains("spec-export-history")));
    }

    #[tokio::test]
    async fn materialize_causal_failure_history_export_writes_artifacts_and_blackboard_refs() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-materialize-history")
        .bind("spec-materialize-history")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let run_dir = app
            .paths
            .workspaces
            .join("run-materialize-history")
            .join("run");
        std::fs::create_dir_all(&run_dir).expect("run dir");
        let mut board = BlackboardState::default();
        board.run_id = "run-materialize-history".to_string();
        std::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board).expect("board json"),
        )
        .expect("write board");
        let graph = crate::models::RunCausalGraph {
            run_id: "run-materialize-history".to_string(),
            generated_at: Utc::now(),
            episodes: vec![crate::models::EpisodeCausalState {
                episode: crate::models::EpisodeRecord {
                    episode_id: "episode-materialize-history".to_string(),
                    run_id: "run-materialize-history".to_string(),
                    phase: "validation".to_string(),
                    goal: "Run visible validation".to_string(),
                    outcome: Some("failure".to_string()),
                    confidence: Some(0.7),
                    started_at: Utc::now(),
                    ended_at: None,
                    state_before: None,
                    state_after: None,
                },
                state_diff: None,
            }],
            events: Vec::new(),
            links: Vec::new(),
            hypotheses: vec![crate::models::CausalHypothesis {
                cause_id: "cause-materialize-timeout".to_string(),
                description: "Materialize timeout caused validation failure".to_string(),
                confidence: 0.82,
                hierarchy_level: crate::models::PearlHierarchyLevel::Associational,
                supporting_runs: vec!["run-materialize-history".to_string()],
                evidence: Vec::new(),
                counterfactuals: Vec::new(),
            }],
        };
        db::upsert_causal_graph_projection(&app.pool, &graph, app.causal_graph.config())
            .await
            .expect("projection");

        let response = post_materialize_causal_failure_history_export(
            Path("run-materialize-history".to_string()),
            State(app.clone()),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["json_artifact"], "causal_failure_history_replay.json");
        assert!(run_dir.join("causal_failure_history_replay.json").exists());
        let markdown = std::fs::read_to_string(run_dir.join("causal_failure_history_replay.md"))
            .expect("markdown");
        assert!(markdown.contains("Causal Failure History Replay"));
        let updated_board: BlackboardState = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("blackboard.json")).expect("board"),
        )
        .expect("updated board");
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "causal_failure_history_replay.json"));
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "causal_failure_history_replay.md"));
    }

    #[tokio::test]
    async fn e2e_readiness_reports_missing_month_end_artifacts() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-e2e-readiness-missing")
        .bind("spec-e2e-readiness")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let run_dir = app
            .paths
            .workspaces
            .join("run-e2e-readiness-missing")
            .join("run");
        std::fs::create_dir_all(&run_dir).expect("run dir");
        let mut board = BlackboardState::default();
        board.run_id = "run-e2e-readiness-missing".to_string();
        std::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board).expect("board json"),
        )
        .expect("blackboard");

        let response =
            get_run_e2e_readiness(Path("run-e2e-readiness-missing".to_string()), State(app))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["schema"], "harkonnen.e2e_readiness.v1");
        assert_eq!(body["status"], "needs_evidence");
        assert!(body["missing_artifacts"]
            .as_array()
            .expect("missing artifacts")
            .iter()
            .any(|artifact| artifact == "validation.json"));
        assert!(body["next_actions"]
            .as_array()
            .expect("next actions")
            .iter()
            .any(|action| action
                .as_str()
                .unwrap_or_default()
                .contains("visible validation")));
    }

    #[tokio::test]
    async fn materialize_e2e_readiness_writes_artifacts_and_blackboard_refs() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-e2e-readiness-materialize")
        .bind("spec-e2e-readiness")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let run_dir = app
            .paths
            .workspaces
            .join("run-e2e-readiness-materialize")
            .join("run");
        std::fs::create_dir_all(&run_dir).expect("run dir");
        let mut board = BlackboardState::default();
        board.run_id = "run-e2e-readiness-materialize".to_string();
        std::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board).expect("board json"),
        )
        .expect("blackboard");

        let response = post_materialize_e2e_readiness(
            Path("run-e2e-readiness-materialize".to_string()),
            State(app),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["json_artifact"], "e2e_readiness.json");
        assert_eq!(body["markdown_artifact"], "e2e_readiness.md");
        assert_eq!(body["status"], "needs_evidence");
        assert!(run_dir.join("e2e_readiness.json").exists());
        let markdown = std::fs::read_to_string(run_dir.join("e2e_readiness.md")).expect("markdown");
        assert!(markdown.contains("E2E Readiness"));
        assert!(markdown.contains("validation.json"));
        let updated_board: BlackboardState = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("blackboard.json")).expect("board"),
        )
        .expect("updated board");
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "e2e_readiness.json"));
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "e2e_readiness.md"));
    }

    #[tokio::test]
    async fn e2e_readiness_index_ranks_recent_candidate_runs() {
        let (_dir, app) = test_app().await;
        let now = Utc::now();
        for (run_id, offset) in [("run-e2e-index-low", 0), ("run-e2e-index-high", 1)] {
            let timestamp = (now + chrono::Duration::seconds(offset)).to_rfc3339();
            sqlx::query(
                "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            )
            .bind(run_id)
            .bind("spec-e2e-index")
            .bind("product")
            .bind("completed")
            .bind(&timestamp)
            .execute(&app.pool)
            .await
            .expect("insert run");
            let run_dir = app.paths.workspaces.join(run_id).join("run");
            std::fs::create_dir_all(&run_dir).expect("run dir");
            let mut board = BlackboardState::default();
            board.run_id = run_id.to_string();
            std::fs::write(
                run_dir.join("blackboard.json"),
                serde_json::to_string_pretty(&board).expect("board json"),
            )
            .expect("blackboard");
            if run_id == "run-e2e-index-high" {
                for artifact in [
                    "coobie_briefing.json",
                    "phase_attributions.json",
                    "causal_report.json",
                    "causal_failure_history_replay.json",
                ] {
                    std::fs::write(run_dir.join(artifact), "{}").expect("artifact");
                }
            }
        }

        let response = get_e2e_readiness_index(State(app)).await.into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["schema"], "harkonnen.e2e_readiness_index.v1");
        assert_eq!(body["run_count"], 2);
        assert_eq!(body["best_run_id"], "run-e2e-index-high");
        assert_eq!(body["entries"][0]["run_id"], "run-e2e-index-high");
        assert!(
            body["entries"][0]["artifact_score"]
                .as_f64()
                .unwrap_or_default()
                > body["entries"][1]["artifact_score"]
                    .as_f64()
                    .unwrap_or_default()
        );
    }

    #[tokio::test]
    async fn materialize_e2e_evidence_manifest_writes_downloadable_checklist() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-e2e-manifest")
        .bind("spec-e2e-manifest")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let run_dir = app.paths.workspaces.join("run-e2e-manifest").join("run");
        std::fs::create_dir_all(&run_dir).expect("run dir");
        let mut board = BlackboardState::default();
        board.run_id = "run-e2e-manifest".to_string();
        std::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board).expect("board json"),
        )
        .expect("blackboard");
        std::fs::write(run_dir.join("coobie_briefing.json"), "{}").expect("artifact");

        let response = post_materialize_e2e_evidence_manifest(
            Path("run-e2e-manifest".to_string()),
            State(app),
        )
        .await
        .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["json_artifact"], "e2e_evidence_manifest.json");
        assert_eq!(body["markdown_artifact"], "e2e_evidence_manifest.md");
        assert!(run_dir.join("e2e_evidence_manifest.json").exists());
        let manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("e2e_evidence_manifest.json")).expect("manifest"),
        )
        .expect("manifest json");
        assert_eq!(manifest["schema"], "harkonnen.e2e_evidence_manifest.v1");
        assert!(manifest["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|entry| entry["artifact"] == "coobie_briefing.json"
                && entry["download_url"]
                    == "/api/runs/run-e2e-manifest/artifacts/coobie_briefing.json"));
        let markdown =
            std::fs::read_to_string(run_dir.join("e2e_evidence_manifest.md")).expect("markdown");
        assert!(markdown.contains("E2E Evidence Manifest"));
        let updated_board: BlackboardState = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("blackboard.json")).expect("board"),
        )
        .expect("updated board");
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "e2e_evidence_manifest.json"));
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "e2e_evidence_manifest.md"));
    }

    #[tokio::test]
    async fn materialize_e2e_evidence_bundle_writes_full_handoff_set() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-e2e-bundle")
        .bind("spec-e2e-bundle")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let run_dir = app.paths.workspaces.join("run-e2e-bundle").join("run");
        std::fs::create_dir_all(&run_dir).expect("run dir");
        let mut board = BlackboardState::default();
        board.run_id = "run-e2e-bundle".to_string();
        std::fs::write(
            run_dir.join("blackboard.json"),
            serde_json::to_string_pretty(&board).expect("board json"),
        )
        .expect("blackboard");
        std::fs::write(run_dir.join("coobie_briefing.json"), "{}").expect("artifact");

        let response =
            post_materialize_e2e_evidence_bundle(Path("run-e2e-bundle".to_string()), State(app))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["json_artifact"], "e2e_evidence_bundle.json");
        assert_eq!(body["markdown_artifact"], "e2e_evidence_bundle.md");
        for artifact in [
            "e2e_readiness.json",
            "e2e_readiness.md",
            "e2e_evidence_manifest.json",
            "e2e_evidence_manifest.md",
            "e2e_evidence_bundle.json",
            "e2e_evidence_bundle.md",
        ] {
            assert!(run_dir.join(artifact).exists(), "missing {artifact}");
        }
        let bundle: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("e2e_evidence_bundle.json")).expect("bundle"),
        )
        .expect("bundle json");
        assert_eq!(bundle["schema"], "harkonnen.e2e_evidence_bundle.v1");
        assert!(bundle["bundle_artifacts"]
            .as_array()
            .expect("bundle artifacts")
            .iter()
            .any(|artifact| artifact == "e2e_evidence_manifest.json"));
        assert!(bundle["bundle_entries"]
            .as_array()
            .expect("bundle entries")
            .iter()
            .any(|entry| entry["artifact"] == "e2e_evidence_bundle.json"
                && entry["download_url"]
                    == "/api/runs/run-e2e-bundle/artifacts/e2e_evidence_bundle.json"));
        assert!(body["bundle_artifact_urls"]
            .as_array()
            .expect("bundle artifact urls")
            .iter()
            .any(|url| url == "/api/runs/run-e2e-bundle/artifacts/e2e_evidence_bundle.md"));
        let markdown =
            std::fs::read_to_string(run_dir.join("e2e_evidence_bundle.md")).expect("markdown");
        assert!(markdown.contains("E2E Evidence Bundle"));
        assert!(markdown.contains("/api/runs/run-e2e-bundle/artifacts/e2e_evidence_bundle.json"));
        let updated_board: BlackboardState = serde_json::from_str(
            &std::fs::read_to_string(run_dir.join("blackboard.json")).expect("board"),
        )
        .expect("updated board");
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "e2e_evidence_bundle.json"));
        assert!(updated_board
            .artifact_refs
            .iter()
            .any(|artifact| artifact == "e2e_evidence_manifest.json"));
    }

    #[tokio::test]
    async fn get_e2e_evidence_bundle_reads_existing_handoff_artifact() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-e2e-bundle-read")
        .bind("spec-e2e-bundle-read")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");
        let run_dir = app.paths.workspaces.join("run-e2e-bundle-read").join("run");
        std::fs::create_dir_all(&run_dir).expect("run dir");
        std::fs::write(
            run_dir.join("e2e_evidence_bundle.json"),
            serde_json::json!({
                "schema": "harkonnen.e2e_evidence_bundle.v1",
                "run_id": "run-e2e-bundle-read",
                "bundle_artifacts": ["e2e_evidence_bundle.json", "e2e_evidence_bundle.md"],
            })
            .to_string(),
        )
        .expect("bundle");

        let response =
            get_run_e2e_evidence_bundle(Path("run-e2e-bundle-read".to_string()), State(app))
                .await
                .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["schema"], "harkonnen.e2e_evidence_bundle.v1");
        assert!(body["bundle_entries"]
            .as_array()
            .expect("bundle entries")
            .iter()
            .any(|entry| entry["download_url"]
                == "/api/runs/run-e2e-bundle-read/artifacts/e2e_evidence_bundle.json"));
        assert!(body["bundle_artifact_urls"]
            .as_array()
            .expect("bundle artifact urls")
            .iter()
            .any(|url| url == "/api/runs/run-e2e-bundle-read/artifacts/e2e_evidence_bundle.md"));
    }

    #[tokio::test]
    async fn context_utilization_baseline_reports_sample_completeness() {
        let (_dir, app) = test_app().await;
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        )
        .bind("run-context-baseline")
        .bind("spec-context-baseline")
        .bind("product")
        .bind("completed")
        .bind(&now)
        .execute(&app.pool)
        .await
        .expect("insert run");

        let response = get_context_utilization_baseline(State(app))
            .await
            .into_response();
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["schema"], "harkonnen.context_utilization_baseline.v1");
        assert_eq!(body["target_run_count"], 10);
        assert_eq!(body["sample_count"], 1);
        assert_eq!(body["baseline_complete"], false);
        assert_eq!(body["entries"][0]["run_id"], "run-context-baseline");
    }
}
