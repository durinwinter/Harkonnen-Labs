use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::{
    coobie::CausalReport,
    models::{AgentExecution, BlackboardState, CoobieBriefing, LessonRecord, RunEvent, RunRecord},
    orchestrator::AppContext,
    pidgin::{self, PidginTranslation},
    tesseract,
};

#[derive(Debug, Serialize)]
struct RunStateResponse {
    run: RunRecord,
    events: Vec<RunEvent>,
    blackboard: Option<BlackboardState>,
    lessons: Vec<LessonRecord>,
    agent_executions: Vec<AgentExecution>,
    coobie_briefing: Option<CoobieBriefing>,
    causal_report: Option<CausalReport>,
    coobie_preflight_response: Option<String>,
    coobie_report_response: Option<String>,
    coobie_translations: Vec<PidginTranslation>,
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

#[derive(Debug, Deserialize)]
struct ClaimRequest {
    agent: String,
    task: String,
    #[serde(default)]
    files: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseRequest {
    agent: String,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    agent: String,
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
        .route("/api/runs/:id/blackboard", get(get_run_blackboard))
        .route("/api/runs/:id/blackboard/:role", get(get_run_blackboard_for_role))
        .route("/api/runs/:id/lessons", get(get_run_lessons))
        .route("/api/runs/:id/state", get(get_run_state))
        .route("/api/runs/:id/coobie-briefing", get(get_coobie_briefing))
        .route("/api/runs/:id/coobie-response", get(get_coobie_response))
        .route("/api/runs/:id/coobie-signals", get(get_coobie_signals))
        .route("/api/runs/:id/causal-report", get(get_causal_report))
        .route("/api/capacity", get(get_capacity))
        .route("/api/tesseract/scene", get(get_tesseract_scene))
        .route("/api/runs/:id/artifacts/:name", get(get_run_artifact))
        .route("/api/coordination/assignments", get(get_assignments))
        .route("/api/coordination/policy-events", get(get_coordination_policy_events))
        .route("/api/coordination/claim", post(claim_task))
        .route("/api/coordination/heartbeat", post(heartbeat_task))
        .route("/api/coordination/release", post(release_task))
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

async fn get_run_events(Path(id): Path<String>, State(app): State<AppContext>) -> impl IntoResponse {
    match app.list_run_events(&id).await {
        Ok(events) => (StatusCode::OK, Json(events)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
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
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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

async fn get_coobie_briefing(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let briefing_path = app.paths.workspaces.join(&id).join("run").join("coobie_briefing.json");
            match read_optional_json::<CoobieBriefing>(&briefing_path).await {
                Ok(Some(briefing)) => (StatusCode::OK, Json(briefing)).into_response(),
                Ok(None) => (StatusCode::NOT_FOUND, "Coobie briefing not yet generated").into_response(),
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
            let response = match read_optional_text(&run_dir.join("coobie_report_response.md")).await {
                Ok(Some(text)) => Some(text),
                Ok(None) => match read_optional_text(&run_dir.join("coobie_preflight_response.md")).await {
                    Ok(text) => text,
                    Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
                },
                Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
            };
            match response {
                Some(text) => (StatusCode::OK, text).into_response(),
                None => (StatusCode::NOT_FOUND, "Coobie response not yet generated").into_response(),
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
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_causal_report(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let report_path = app.paths.workspaces.join(&id).join("run").join("causal_report.json");
            match read_optional_json::<CausalReport>(&report_path).await {
                Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
                Ok(None) => (StatusCode::NOT_FOUND, "Causal report not yet generated").into_response(),
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
            }
        }
        Ok(None) => (StatusCode::NOT_FOUND, "Run not found").into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn build_run_state(app: &AppContext, id: &str) -> anyhow::Result<Option<RunStateResponse>> {
    let Some(run) = app.get_run(id).await? else {
        return Ok(None);
    };

    let events = app.list_run_events(id).await?;
    let run_dir = app.paths.workspaces.join(id).join("run");
    let blackboard = read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await?;
    let lessons = read_optional_json::<Vec<LessonRecord>>(&run_dir.join("lessons.json"))
        .await?
        .unwrap_or_default();
    let agent_executions = read_optional_json::<Vec<AgentExecution>>(&run_dir.join("agent_executions.json"))
        .await?
        .unwrap_or_default();
    let coobie_briefing = read_optional_json::<CoobieBriefing>(&run_dir.join("coobie_briefing.json")).await?;
    let causal_report = read_optional_json::<CausalReport>(&run_dir.join("causal_report.json")).await?;
    let coobie_preflight_response = read_optional_text(&run_dir.join("coobie_preflight_response.md")).await?;
    let coobie_report_response = read_optional_text(&run_dir.join("coobie_report_response.md")).await?;
    let coobie_translations = load_coobie_translations(&run_dir).await?;

    Ok(Some(RunStateResponse {
        run,
        events,
        blackboard,
        lessons,
        agent_executions,
        coobie_briefing,
        causal_report,
        coobie_preflight_response,
        coobie_report_response,
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
    app.paths.factory.join("coordination").join("assignments.json")
}

fn assignments_markdown_path(app: &AppContext) -> PathBuf {
    app.paths.root.join("assignments.md")
}

fn coordination_policy_events_path(app: &AppContext) -> PathBuf {
    app.paths.factory.join("coordination").join("policy_events.json")
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

async fn save_policy_events(app: &AppContext, events: &[CoordinationPolicyEvent]) -> anyhow::Result<()> {
    let path = coordination_policy_events_path(app);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(&path, serde_json::to_string_pretty(events)?).await?;
    Ok(())
}

async fn append_policy_event(app: &AppContext, event: CoordinationPolicyEvent) -> anyhow::Result<()> {
    let mut events = load_policy_events(app).await?;
    events.push(event);
    save_policy_events(app, &events).await
}

async fn save_assignments(app: &AppContext, state: &AssignmentsState) -> anyhow::Result<()> {
    let json_path = coordination_json_path(app);
    if let Some(parent) = json_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tokio::fs::write(&json_path, serde_json::to_string_pretty(state)?).await?;
    tokio::fs::write(assignments_markdown_path(app), render_assignments_markdown(state)).await?;
    Ok(())
}

fn parse_utc(raw: &str) -> Option<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn has_file_conflict(requested_files: &[String], existing_files: &[String]) -> bool {
    requested_files.iter().any(|file| existing_files.contains(file))
}

fn normalize_assignment(assignment: &mut Assignment, now: DateTime<Utc>, stale_after_seconds: i64) -> bool {
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

async fn normalize_assignments(app: &AppContext, mut state: AssignmentsState) -> anyhow::Result<AssignmentsState> {
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

async fn ensure_assignments_state(app: &AppContext) -> anyhow::Result<AssignmentsState> {
    let state = load_assignments(app).await?;
    normalize_assignments(app, state).await
}

fn render_assignments_markdown(state: &AssignmentsState) -> String {
    let mut out = String::new();
    out.push_str("# Assignments

");
    out.push_str("This is the fallback coordination document when the Harkonnen API server is not running.

");
    out.push_str(&format!(
        "Keeper manages file-claim policy for this repo.
Policy mode: {}
Heartbeat timeout: {} seconds

",
        state.policy_mode, state.stale_after_seconds
    ));
    out.push_str("Preferred live source once the server is up: `GET /api/coordination/assignments`.

");
    out.push_str("Policy event stream: `GET /api/coordination/policy-events`.

");
    out.push_str("Claim work with `POST /api/coordination/claim`, heartbeat with `POST /api/coordination/heartbeat`, and release it with `POST /api/coordination/release`.

");
    out.push_str(&format!("Last updated: {}

", state.updated_at));
    out.push_str("## Active Claims

");

    if state.active.is_empty() {
        out.push_str("No active claims.

");
    } else {
        let mut claims: Vec<_> = state.active.values().cloned().collect();
        claims.sort_by(|a, b| a.agent.cmp(&b.agent));
        for claim in claims {
            out.push_str(&format!("### {}
", claim.agent));
            out.push_str(&format!("Task: {}
", claim.task));
            out.push_str(&format!("Status: {}
", claim.status));
            out.push_str(&format!("Claimed: {}
", claim.claimed_at));
            out.push_str(&format!("Last heartbeat: {}
", claim.last_heartbeat_at));
            if claim.files.is_empty() {
                out.push_str("Files: none declared

");
            } else {
                out.push_str(&format!("Files:
- {}

", claim.files.join("
- ")));
            }
        }
    }

    out.push_str("## How To Use This Fallback

");
    out.push_str("1. Before assigning work, read the relevant active claim section.
");
    out.push_str("2. Paste only the relevant section into the AI's context.
");
    out.push_str("3. If you are actively holding files, send a heartbeat about once per minute.
");
    out.push_str("4. Keeper may reap stale conflicting claims when another agent needs the same files.
");
    out.push_str("5. Once the server is running, switch all agents to the live coordination endpoint.
");
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
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
            ).await {
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
                let message = format!("Keeper blocked claim: {} already owns {:?}", owner, conflict);
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

    state.active.insert(
        agent.clone(),
        Assignment {
            agent: agent.clone(),
            task: req.task,
            files: files.clone(),
            claimed_at: claimed_at.clone(),
            last_heartbeat_at: claimed_at,
            status: "active".to_string(),
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
    ).await;

    match save_assignments(&app, &state).await {
        Ok(()) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn heartbeat_task(
    State(app): State<AppContext>,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let mut state = match ensure_assignments_state(&app).await {
        Ok(state) => state,
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
        ).await;
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
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
    ).await;

    match save_assignments(&app, &state).await {
        Ok(()) => (StatusCode::OK, Json(state)).into_response(),
        Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
    }
}

async fn get_tesseract_scene(State(app): State<AppContext>) -> impl IntoResponse {
    let runs = match app.list_runs(30).await {
        Ok(r) => r,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let mut run_reports: Vec<(RunRecord, Option<CausalReport>)> = Vec::new();
    for run in runs {
        let report_path = app.paths.workspaces
            .join(&run.run_id)
            .join("run")
            .join("causal_report.json");
        let report = read_optional_json::<CausalReport>(&report_path).await.unwrap_or(None);
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
