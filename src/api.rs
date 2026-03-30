use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use serde::{de::DeserializeOwned, Serialize};
use std::net::SocketAddr;
use std::path::Path as FsPath;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use crate::{
    models::{AgentExecution, BlackboardState, LessonRecord, RunEvent, RunRecord},
    orchestrator::AppContext,
};

#[derive(Debug, Serialize)]
struct RunStateResponse {
    run: RunRecord,
    events: Vec<RunEvent>,
    blackboard: Option<BlackboardState>,
    lessons: Vec<LessonRecord>,
    agent_executions: Vec<AgentExecution>,
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
        .route("/api/runs/:id/lessons", get(get_run_lessons))
        .route("/api/runs/:id/state", get(get_run_state))
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
            match read_optional_json::<BlackboardState>(&run_dir.join("blackboard.json")).await {
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
    let agent_executions =
        read_optional_json::<Vec<AgentExecution>>(&run_dir.join("agent_executions.json"))
            .await?
            .unwrap_or_default();

    Ok(Some(RunStateResponse {
        run,
        events,
        blackboard,
        lessons,
        agent_executions,
    }))
}

async fn read_optional_json<T: DeserializeOwned>(path: &FsPath) -> anyhow::Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str::<T>(&raw)?))
}
