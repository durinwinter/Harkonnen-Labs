use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::{
    coobie::CausalReport,
    models::{AgentExecution, BlackboardState, CoobieBriefing, EvidenceAnnotation, EvidenceAnnotationBundle, EvidenceAnnotationHistoryEvent, EvidenceMatchReport, EvidenceSource, LessonRecord, RunEvent, RunRecord, Spec},
    orchestrator::{AppContext, RunRequest},
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
    evidence_match_report: Option<EvidenceMatchReport>,
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
}

#[derive(Debug, Deserialize)]
struct ReleaseRequest {
    agent: String,
}

#[derive(Debug, Deserialize)]
struct HeartbeatRequest {
    agent: String,
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
        .route("/api/runs/:id/blackboard", get(get_run_blackboard))
        .route("/api/runs/:id/blackboard/:role", get(get_run_blackboard_for_role))
        .route("/api/runs/:id/lessons", get(get_run_lessons))
        .route("/api/runs/:id/state", get(get_run_state))
        .route("/api/runs/:id/coobie-briefing", get(get_coobie_briefing))
        .route("/api/runs/:id/coobie-response", get(get_coobie_response))
        .route("/api/runs/:id/coobie-signals", get(get_coobie_signals))
        .route("/api/runs/:id/causal-report", get(get_causal_report))
        .route("/api/runs/:id/evidence-match-report", get(get_run_evidence_match_report))
        .route("/api/evidence/bundles", get(list_evidence_bundles))
        .route("/api/evidence/bundles/:name", get(get_evidence_bundle))
        .route("/api/evidence/history", get(get_evidence_history))
        .route("/api/evidence/bundles/save", post(post_evidence_bundle_save))
        .route("/api/evidence/annotations/upsert", post(post_evidence_annotation_upsert))
        .route("/api/evidence/annotations/review", post(post_evidence_annotation_review))
        .route("/api/evidence/similar", get(get_similar_evidence_windows))
        .route("/api/evidence/match-report", post(post_evidence_match_report))
        .route("/api/fs/directories", get(get_directory_browser))
        .route("/api/capacity", get(get_capacity))
        .route("/api/tesseract/scene", get(get_tesseract_scene))
        .route("/api/runs/start", post(start_run))
        .route("/api/runs/:id/artifacts", get(list_run_artifacts))
        .route("/api/runs/:id/artifacts/:name", get(get_run_artifact))
        .route("/api/runs/:id/memory-note", post(add_memory_note))
        .route("/api/scout/draft", post(scout_draft))
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

async fn get_directory_browser(
    State(app): State<AppContext>,
    Query(query): Query<DirectoryBrowseQuery>,
) -> impl IntoResponse {
    let requested = query.path.as_deref().map(str::trim).filter(|value| !value.is_empty());
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
        Err(error) => return (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
    match app.load_project_evidence_bundle(&query.project_root, &name).await {
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

async fn get_run_evidence_match_report(
    Path(id): Path<String>,
    State(app): State<AppContext>,
) -> impl IntoResponse {
    match app.get_run(&id).await {
        Ok(Some(_)) => {
            let report_path = app.paths.workspaces.join(&id).join("run").join("evidence_match_report.json");
            match read_optional_json::<EvidenceMatchReport>(&report_path).await {
                Ok(Some(report)) => (StatusCode::OK, Json(report)).into_response(),
                Ok(None) => (StatusCode::NOT_FOUND, "Evidence match report not yet generated").into_response(),
                Err(error) => (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()).into_response(),
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
            time_span_ms = window.time_span_ms.or_else(|| match (window.start_ms, window.end_ms) {
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
    let evidence_match_report = read_optional_json::<EvidenceMatchReport>(&run_dir.join("evidence_match_report.json")).await?;
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
    if !values.iter().any(|existing| existing.eq_ignore_ascii_case(trimmed)) {
        values.push(trimmed.to_string());
    }
}

fn render_selected_window_summary(window: &EvidenceMatchWindowInput, time_span_ms: Option<i64>) -> String {
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
    format!("{} [{}] labels={} span={}", title, annotation_type, labels, span)
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

    let spec_id = format!("{}-draft-{}", slugify(&product), &uuid::Uuid::new_v4().to_string()[..8]);

    // Build a structured spec from the intent text.
    // Lines starting with verbs become acceptance_criteria; everything else is purpose.
    let lines: Vec<&str> = intent.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let purpose = lines.first().copied().unwrap_or(&intent);

    let criteria: Vec<String> = lines.iter()
        .skip(1)
        .map(|l| format!("  - {l}"))
        .collect();

    let criteria_block = if criteria.is_empty() {
        "  - run completes without errors".to_string()
    } else {
        criteria.join("\n")
    };

    let product_input = product_path
        .as_ref()
        .map(|path| format!("  - \"product directory: {path}\""))
        .unwrap_or_else(|| format!("  - \"product directory: products/{product}/\""));

    let spec_yaml = format!(
r#"id: {spec_id}
title: {title}
purpose: >
  {purpose}
scope:
  - {product}
constraints:
  - remain within the {product} workspace boundary
  - do not modify files outside the target product
inputs:
{product_input}
outputs:
  - implementation artifacts in the run workspace
  - validation.json with pass/fail verdict
acceptance_criteria:
{criteria_block}
forbidden_behaviors:
  - deleting unrelated files
  - reaching outside the workspace boundary
rollback_requirements:
  - retain prior artifacts unless explicitly cleaned up
dependencies: []
performance_expectations:
  - commands should complete in a reasonable time
security_expectations:
  - secrets must not appear in logs or artifact bundles
"#,
        spec_id = spec_id,
        title = title_case(&product),
        purpose = purpose,
        product = product,
        product_input = product_input,
        criteria_block = criteria_block,
    );

    // Save to factory/specs/drafts/
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

    (StatusCode::OK, Json(ScoutDraftResponse {
        spec_yaml,
        spec_path: spec_path_rel,
        spec_id,
    })).into_response()
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
                None    => String::new(),
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
        return (StatusCode::BAD_REQUEST, "product or product_path is required").into_response();
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
        product: if product_path.is_some() { None } else { product },
        product_path,
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
    let drafts = app.paths.factory.join("specs").join("drafts").join(format!("{spec}.yaml"));
    if drafts.exists() {
        return drafts.to_string_lossy().into_owned();
    }
    let examples = app.paths.factory.join("specs").join("examples").join(format!("{spec}.yaml"));
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
    let note_id = format!("run-note-{}-{}", &run_id[..8], &uuid::Uuid::new_v4().to_string()[..6]);
    let summary = req.note.lines().next().unwrap_or("Human run note").to_string();

    let mut all_tags = req.tags.clone();
    all_tags.push("human-note".to_string());
    all_tags.push(format!("run:{}", &run_id[..8]));

    let tags_yaml = all_tags.iter().map(|t| format!("  - {t}")).collect::<Vec<_>>().join("\n");

    let content = format!(
        "---\nid: {note_id}\ntags:\n{tags_yaml}\nsummary: {summary}\n---\n\n{note}\n",
        note_id = note_id,
        tags_yaml = tags_yaml,
        summary = summary,
        note = req.note.trim(),
    );

    let note_path = app.paths.factory.join("memory").join(format!("{note_id}.md"));

    if let Err(e) = tokio::fs::create_dir_all(note_path.parent().unwrap()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    if let Err(e) = tokio::fs::write(&note_path, content.as_bytes()).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
    }

    // Rebuild the memory index so the note is immediately searchable
    let _ = app.memory_store.reindex().await;

    (StatusCode::OK, Json(serde_json::json!({ "id": note_id, "path": note_path }))).into_response()
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
                a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
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
