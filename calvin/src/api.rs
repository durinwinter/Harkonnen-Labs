use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    archive::{ArchiveExperience, BeliefRevision},
    metrics,
    telemetry::TelemetryEvent,
    CalvinState,
};

pub(crate) fn router(state: Arc<CalvinState>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/status", get(status))
        .route("/runs", post(open_run))
        .route("/runs/:run_id/experiences", post(record_experience))
        .route("/runs/:run_id/beliefs", post(revise_belief))
        .route("/runs/:run_id/close", patch(close_run))
        .route("/runs/:run_id/causal-links", post(record_causal_link))
        .route(
            "/runs/:run_id/predictions",
            post(post_prediction).get(get_prediction),
        )
        .route(
            "/runs/:run_id/prediction-result",
            post(post_prediction_result),
        )
        .route("/agents/:name/traits", get(get_traits))
        .route("/agents/:name/beliefs", get(get_beliefs))
        .route("/agents/:name/check", post(check_adaptation))
        .route("/agents/:name/status", patch(patch_agent_status))
        .route("/agents/:name/metrics", get(get_metrics))
        .route("/telemetry", post(write_event))
        .route("/telemetry/batch", post(write_events_batch))
        .with_state(state)
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({"status": "ok", "service": "calvin-archive"}))
}

async fn status(State(state): State<Arc<CalvinState>>) -> impl IntoResponse {
    match state.archive.entity_counts().await {
        Ok(counts) => Json(serde_json::json!({
            "status": "ok",
            "typedb_entities": counts,
            "telemetry_enabled": state.telemetry.is_some(),
            "streaming_enabled": false
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct OpenRunRequest {
    run_id: String,
    spec_id: String,
    provider: String,
    model: String,
}

async fn open_run(
    State(state): State<Arc<CalvinState>>,
    Json(req): Json<OpenRunRequest>,
) -> impl IntoResponse {
    match state
        .archive
        .open_run(&req.run_id, &req.spec_id, &req.provider, &req.model)
        .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn record_experience(
    State(state): State<Arc<CalvinState>>,
    Path(_run_id): Path<String>,
    Json(exp): Json<ArchiveExperience>,
) -> impl IntoResponse {
    match state.archive.record_experience(&exp).await {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn revise_belief(
    State(state): State<Arc<CalvinState>>,
    Path(_run_id): Path<String>,
    Json(rev): Json<BeliefRevision>,
) -> impl IntoResponse {
    match state.archive.revise_belief(&rev).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct CloseRunRequest {
    outcome: String,
}

async fn close_run(
    State(state): State<Arc<CalvinState>>,
    Path(run_id): Path<String>,
    Json(req): Json<CloseRunRequest>,
) -> impl IntoResponse {
    match state.archive.close_run(&run_id, &req.outcome).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_traits(
    State(state): State<Arc<CalvinState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.archive.get_kernel_traits(&name).await {
        Ok(traits) => Json(traits).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_beliefs(
    State(state): State<Arc<CalvinState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    match state.archive.get_active_beliefs(&name).await {
        Ok(beliefs) => Json(beliefs).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct CheckAdaptationRequest {
    adaptation_summary: String,
}

#[derive(Debug, Serialize)]
struct CheckAdaptationResponse {
    safe: bool,
}

async fn check_adaptation(
    State(state): State<Arc<CalvinState>>,
    Path(name): Path<String>,
    Json(req): Json<CheckAdaptationRequest>,
) -> impl IntoResponse {
    match state
        .archive
        .check_adaptation_safe(&req.adaptation_summary, &name)
        .await
    {
        Ok(safe) => Json(CheckAdaptationResponse { safe }).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct RecordCausalLinkRequest {
    cause_episode_id: String,
    effect_episode_id: String,
    pearl_level: String,
    confidence: f64,
}

async fn record_causal_link(
    State(state): State<Arc<CalvinState>>,
    Path(run_id): Path<String>,
    Json(req): Json<RecordCausalLinkRequest>,
) -> impl IntoResponse {
    match state
        .archive
        .record_causal_link(
            &run_id,
            &req.cause_episode_id,
            &req.effect_episode_id,
            &req.pearl_level,
            req.confidence,
        )
        .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PatchAgentStatusRequest {
    status: String,
}

#[derive(Debug, Deserialize)]
struct PostPredictionRequest {
    prediction_id: String,
    predicted_outcome: String,
    risk_score: f64,
    confidence: f64,
    #[serde(default)]
    failure_phase: Option<String>,
    #[serde(default)]
    failure_kind: Option<String>,
    #[serde(default)]
    source_cause_ids: String,
    narrative_summary: String,
}

async fn post_prediction(
    State(state): State<Arc<CalvinState>>,
    Path(run_id): Path<String>,
    Json(req): Json<PostPredictionRequest>,
) -> impl IntoResponse {
    match state
        .archive
        .record_prediction(
            &run_id,
            &req.prediction_id,
            &req.predicted_outcome,
            req.risk_score,
            req.confidence,
            req.failure_phase.as_deref(),
            req.failure_kind.as_deref(),
            &req.source_cause_ids,
            &req.narrative_summary,
        )
        .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_prediction(
    State(state): State<Arc<CalvinState>>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    match state.archive.get_prediction(&run_id).await {
        Ok(Some(pred)) => Json(pred).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct PostPredictionResultRequest {
    prediction_id: String,
    result_id: String,
    actual_outcome: String,
    #[serde(default)]
    actual_failure_phase: Option<String>,
    #[serde(default)]
    actual_failure_kind: Option<String>,
    prediction_error: f64,
    narrative_summary: String,
}

async fn post_prediction_result(
    State(state): State<Arc<CalvinState>>,
    Path(_run_id): Path<String>,
    Json(req): Json<PostPredictionResultRequest>,
) -> impl IntoResponse {
    match state
        .archive
        .record_prediction_result(
            &req.prediction_id,
            &req.result_id,
            &req.actual_outcome,
            req.actual_failure_phase.as_deref(),
            req.actual_failure_kind.as_deref(),
            req.prediction_error,
            &req.narrative_summary,
        )
        .await
    {
        Ok(()) => StatusCode::CREATED.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn patch_agent_status(
    State(state): State<Arc<CalvinState>>,
    Path(name): Path<String>,
    Json(req): Json<PatchAgentStatusRequest>,
) -> impl IntoResponse {
    match state.archive.update_agent_status(&name, &req.status).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn get_metrics(
    State(state): State<Arc<CalvinState>>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let Some(telemetry) = state.telemetry.as_ref() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                serde_json::json!({"error": "telemetry is disabled in minimal archive-first mode"}),
            ),
        )
            .into_response();
    };
    match metrics::full_snapshot(telemetry.pool(), &state.archive, &name).await {
        Ok(snap) => Json(snap).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

async fn write_event(
    State(state): State<Arc<CalvinState>>,
    Json(evt): Json<TelemetryEvent>,
) -> impl IntoResponse {
    match state.telemetry.as_ref() {
        Some(telemetry) => match telemetry.write_event(&evt).await {
            Ok(()) => StatusCode::CREATED.into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        },
        None => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "ignored", "reason": "telemetry disabled"})),
        )
            .into_response(),
    }
}

async fn write_events_batch(
    State(state): State<Arc<CalvinState>>,
    Json(evts): Json<Vec<TelemetryEvent>>,
) -> impl IntoResponse {
    match state.telemetry.as_ref() {
        Some(telemetry) => match telemetry.write_events_batch(&evts).await {
            Ok(()) => StatusCode::CREATED.into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response(),
        },
        None => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({
                "status": "ignored",
                "reason": "telemetry disabled",
                "count": evts.len()
            })),
        )
            .into_response(),
    }
}
