use anyhow::{bail, Context, Result};
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, BufWriter};
use tokio_stream::{wrappers::BroadcastStream, StreamExt as _};
use tracing::info;

use crate::{
    benchmark,
    chat::{dispatch_message, ChatThreadKind, OpenThreadRequest, PostMessageRequest},
    cli::McpServeArgs,
    models::Spec,
    orchestrator::{AppContext, RunRequest},
    reporting,
};

#[derive(Clone)]
struct McpState {
    app: AppContext,
    started_at: Instant,
}

#[derive(Debug, Serialize)]
struct McpHealthResponse {
    status: &'static str,
    transport: &'static str,
    uptime_secs: u64,
    version: &'static str,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    #[serde(default)]
    id: Option<Value>,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StdioMessageFormat {
    ContentLengthFramed,
    UnframedJson,
}

pub async fn handle_mcp_serve(app: AppContext, args: McpServeArgs) -> Result<()> {
    let configured = app
        .paths
        .setup
        .mcp
        .as_ref()
        .and_then(|mcp| mcp.self_server.as_ref());

    let transport = args
        .transport
        .clone()
        .or_else(|| configured.map(|cfg| cfg.transport.clone()))
        .unwrap_or_else(|| "sse".to_string());

    match transport.as_str() {
        "sse" => {
            let host = args
                .host
                .clone()
                .or_else(|| configured.and_then(|cfg| cfg.host.clone()))
                .unwrap_or_else(|| "127.0.0.1".to_string());
            let port = args
                .port
                .or_else(|| configured.and_then(|cfg| cfg.port))
                .unwrap_or(3001);
            start_sse_server(app, &host, port).await
        }
        "stdio" => start_stdio_server(app).await,
        other => bail!("unsupported MCP transport: {other}"),
    }
}

async fn start_sse_server(app: AppContext, host: &str, port: u16) -> Result<()> {
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("invalid MCP self-server address: {host}:{port}"))?;
    let state = Arc::new(McpState {
        app,
        started_at: Instant::now(),
    });
    let router = Router::new()
        .route("/health", get(get_health))
        .route("/sse", get(get_sse))
        .route("/rpc", post(post_rpc))
        .with_state(state);

    info!(%addr, "starting Harkonnen MCP self-server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    Ok(())
}

async fn start_stdio_server(app: AppContext) -> Result<()> {
    let state = McpState {
        app,
        started_at: Instant::now(),
    };
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut writer = BufWriter::new(stdout);

    loop {
        let Some((body, format)) = read_stdio_message(&mut reader).await? else {
            break;
        };
        match handle_rpc_body(&state, body).await {
            Ok(Some(response)) => write_stdio_message(&mut writer, &response, format).await?,
            Ok(None) => {}
            Err(error) => write_stdio_message(&mut writer, &error, format).await?,
        }
    }

    writer.flush().await?;
    Ok(())
}

async fn get_health(State(state): State<Arc<McpState>>) -> impl IntoResponse {
    Json(McpHealthResponse {
        status: "ok",
        transport: "sse",
        uptime_secs: state.started_at.elapsed().as_secs(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn get_sse(State(state): State<Arc<McpState>>) -> impl IntoResponse {
    let rx = state.app.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| match item {
        Ok(event) => {
            let event = Event::default()
                .event("live-event")
                .json_data(&event)
                .unwrap_or_else(|_| Event::default().event("live-event").data("{}"));
            Some(Ok::<Event, Infallible>(event))
        }
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("keepalive"),
    )
}

async fn post_rpc(State(state): State<Arc<McpState>>, Json(body): Json<Value>) -> Response {
    match handle_rpc_body(&state, body).await {
        Ok(Some(result)) => Json(result).into_response(),
        Ok(None) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => Json(error).into_response(),
    }
}

async fn handle_rpc_body(
    state: &McpState,
    body: Value,
) -> std::result::Result<Option<Value>, Value> {
    if body.is_array() {
        return Err(rpc_error_value(
            None,
            -32600,
            "batch requests are not supported",
        ));
    }

    let envelope: RpcEnvelope = match serde_json::from_value(body) {
        Ok(value) => value,
        Err(error) => {
            return Err(rpc_error_value(
                None,
                -32600,
                &format!("invalid request: {error}"),
            ))
        }
    };

    match handle_rpc(state, &envelope).await {
        Ok(Some(result)) => Ok(Some(result)),
        Ok(None) => Ok(None),
        Err((code, message)) => Err(rpc_error_value(envelope.id.clone(), code, &message)),
    }
}

async fn handle_rpc(
    state: &McpState,
    envelope: &RpcEnvelope,
) -> std::result::Result<Option<Value>, (i64, String)> {
    let id = envelope.id.clone();
    let method = envelope.method.trim();
    if method.is_empty() {
        return Err((-32600, "missing method".to_string()));
    }

    let result = match method {
        "initialize" => json!({
            "protocolVersion": requested_protocol_version(&envelope.params),
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "listChanged": false, "subscribe": false },
                "prompts": { "listChanged": false }
            },
            "serverInfo": {
                "name": "harkonnen-labs",
                "version": env!("CARGO_PKG_VERSION")
            },
            "instructions": "Harkonnen exposes factory runs, reports, decision logs, and commissioning actions through a minimal MCP-compatible self-server."
        }),
        "notifications/initialized" => return Ok(None),
        "ping" => json!({ "ok": true }),
        "tools/list" => json!({ "tools": tool_descriptors() }),
        "resources/list" => json!({ "resources": resource_descriptors() }),
        "prompts/list" => json!({ "prompts": prompt_descriptors() }),
        "resources/read" => read_resource(state, &envelope.params).await?,
        "prompts/get" => get_prompt(&envelope.params)?,
        "tools/call" => call_tool(state, &envelope.params).await?,
        _ => return Err((-32601, format!("method not found: {method}"))),
    };

    Ok(Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })))
}

async fn call_tool(state: &McpState, params: &Value) -> std::result::Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| (-32602, "tools/call requires params.name".to_string()))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let result = match name {
        "list_runs" => {
            let limit = arguments
                .get("limit")
                .and_then(Value::as_i64)
                .unwrap_or(20)
                .clamp(1, 100);
            let runs = state.app.list_runs(limit).await.map_err(internal_error)?;
            json!(runs)
        }
        "get_run" => {
            let run_id = required_string(&arguments, "run_id")?;
            let run = state
                .app
                .get_run(&run_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| (-32004, format!("run not found: {run_id}")))?;
            json!(run)
        }
        "get_run_report" => {
            let run_id = required_string(&arguments, "run_id")?;
            let report = reporting::build_report(&state.app, &run_id)
                .await
                .map_err(internal_error)?;
            return Ok(text_tool_result(&report));
        }
        "list_run_decisions" => {
            let run_id = required_string(&arguments, "run_id")?;
            let decisions = state
                .app
                .list_run_decisions(&run_id)
                .await
                .map_err(internal_error)?;
            json!(decisions)
        }
        "start_run" => {
            let spec = required_string(&arguments, "spec")?;
            let product = optional_string(&arguments, "product");
            let product_path = optional_string(&arguments, "product_path");
            if product.is_none() && product_path.is_none() {
                return Err((
                    -32602,
                    "start_run requires either arguments.product or arguments.product_path"
                        .to_string(),
                ));
            }
            let run_hidden_scenarios = arguments
                .get("run_hidden_scenarios")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let spec_path = resolve_spec_reference(&state.app, &spec);
            if let Some(spec_yaml) = optional_string(&arguments, "spec_yaml") {
                persist_spec_yaml(&state.app, &spec_path, &spec_yaml)
                    .await
                    .map_err(internal_error)?;
            }
            let run = state
                .app
                .start_run(RunRequest {
                    spec_path,
                    product,
                    product_path,
                    run_hidden_scenarios,
                    failure_harness: None,
                })
                .await
                .map_err(internal_error)?;
            json!({
                "run_id": run.run_id,
                "status": run.status,
                "spec_id": run.spec_id,
                "product": run.product
            })
        }
        "queue_run" => {
            let spec = required_string(&arguments, "spec")?;
            let product = optional_string(&arguments, "product");
            let product_path = optional_string(&arguments, "product_path");
            if product.is_none() && product_path.is_none() {
                return Err((
                    -32602,
                    "queue_run requires either arguments.product or arguments.product_path"
                        .to_string(),
                ));
            }
            let run_hidden_scenarios = arguments
                .get("run_hidden_scenarios")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let spec_path = resolve_spec_reference(&state.app, &spec);
            if let Some(spec_yaml) = optional_string(&arguments, "spec_yaml") {
                persist_spec_yaml(&state.app, &spec_path, &spec_yaml)
                    .await
                    .map_err(internal_error)?;
            }
            let run = state
                .app
                .queue_run(RunRequest {
                    spec_path,
                    product,
                    product_path,
                    run_hidden_scenarios,
                    failure_harness: None,
                })
                .await
                .map_err(internal_error)?;
            json!({
                "run_id": run.run_id,
                "status": run.status,
                "spec_id": run.spec_id,
                "product": run.product
            })
        }
        "watch_run" => {
            let run_id = required_string(&arguments, "run_id")?;
            let event_limit = arguments
                .get("event_limit")
                .and_then(Value::as_i64)
                .unwrap_or(12)
                .clamp(1, 100) as usize;
            run_watch_payload(state, &run_id, event_limit)
                .await
                .map_err(internal_error)?
        }
        "get_run_board_snapshot" => {
            let run_id = required_string(&arguments, "run_id")?;
            let snapshot = crate::api::build_run_board_snapshot(&state.app, &run_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| (-32004, format!("run not found: {run_id}")))?;
            let view = optional_string(&arguments, "view").unwrap_or_else(|| "summary".to_string());
            match view.as_str() {
                "full" => snapshot,
                "summary" => summarize_board_snapshot(&snapshot),
                other => {
                    return Err((
                        -32602,
                        format!(
                            "unsupported board snapshot view: {other} (expected full or summary)"
                        ),
                    ))
                }
            }
        }
        "list_chat_threads" => {
            let run_id = optional_string(&arguments, "run_id");
            let thread_kind = optional_chat_thread_kind(&arguments, "thread_kind")?;
            let limit = arguments
                .get("limit")
                .and_then(Value::as_i64)
                .unwrap_or(20)
                .clamp(1, 100) as usize;
            let threads = state
                .app
                .chat
                .list_threads(run_id.as_deref(), thread_kind.as_ref(), limit)
                .await
                .map_err(internal_error)?;
            json!(threads)
        }
        "open_chat_thread" => {
            let thread = state
                .app
                .chat
                .open_thread(&OpenThreadRequest {
                    run_id: optional_string(&arguments, "run_id"),
                    spec_id: optional_string(&arguments, "spec_id"),
                    title: optional_string(&arguments, "title"),
                    thread_kind: optional_chat_thread_kind(&arguments, "thread_kind")?
                        .unwrap_or_default(),
                    metadata_json: arguments.get("metadata_json").cloned(),
                })
                .await
                .map_err(internal_error)?;
            json!(thread)
        }
        "get_chat_thread" => {
            let thread_id = required_string(&arguments, "thread_id")?;
            let thread = state
                .app
                .chat
                .get_thread(&thread_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| (-32004, format!("thread not found: {thread_id}")))?;
            json!(thread)
        }
        "list_chat_messages" => {
            let thread_id = required_string(&arguments, "thread_id")?;
            let messages = state
                .app
                .chat
                .list_messages(&thread_id)
                .await
                .map_err(internal_error)?;
            json!(messages)
        }
        "post_chat_message" => {
            let thread_id = required_string(&arguments, "thread_id")?;
            let content = required_string(&arguments, "content")?;
            let thread = state
                .app
                .chat
                .get_thread(&thread_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| (-32004, format!("thread not found: {thread_id}")))?;
            let response = dispatch_message(
                &state.app.chat,
                &state.app.paths,
                &thread,
                &PostMessageRequest {
                    content,
                    agent: optional_string(&arguments, "agent"),
                },
            )
            .await
            .map_err(internal_error)?;
            json!(response)
        }
        "list_run_checkpoints" => {
            let run_id = required_string(&arguments, "run_id")?;
            let checkpoints = state
                .app
                .list_run_checkpoints(&run_id)
                .await
                .map_err(internal_error)?;
            json!(checkpoints)
        }
        "reply_to_checkpoint" => {
            let run_id = required_string(&arguments, "run_id")?;
            let checkpoint_id = required_string(&arguments, "checkpoint_id")?;
            let answer_text = optional_string(&arguments, "answer_text").unwrap_or_default();
            let decision_json = arguments.get("decision_json").cloned();
            let resolve = arguments
                .get("resolve")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let answered_by = optional_string(&arguments, "answered_by")
                .unwrap_or_else(|| "operator".to_string());
            let checkpoint = state
                .app
                .reply_to_checkpoint(
                    &run_id,
                    &checkpoint_id,
                    &answered_by,
                    &answer_text,
                    decision_json,
                    resolve,
                )
                .await
                .map_err(internal_error)?;
            json!(checkpoint)
        }
        "unblock_agent" => {
            let run_id = required_string(&arguments, "run_id")?;
            let agent = required_string(&arguments, "agent")?;
            let answered_by = optional_string(&arguments, "answered_by")
                .unwrap_or_else(|| "operator".to_string());
            let checkpoints = state
                .app
                .unblock_agent_checkpoints(
                    &run_id,
                    &agent,
                    optional_string(&arguments, "checkpoint_id").as_deref(),
                    &answered_by,
                    optional_string(&arguments, "answer_text").as_deref(),
                    arguments.get("decision_json").cloned(),
                )
                .await
                .map_err(internal_error)?;
            json!({
                "run_id": run_id,
                "agent": agent,
                "resolved": checkpoints.len(),
                "checkpoints": checkpoints
            })
        }
        "list_benchmark_suites" => {
            let manifest_path = benchmark_manifest_path(state, &arguments);
            let manifest = benchmark::load_manifest(&manifest_path).map_err(internal_error)?;
            json!({
                "manifest_path": manifest_path.display().to_string(),
                "version": manifest.version,
                "suites": manifest.suites
            })
        }
        "run_benchmarks" => {
            let manifest_path = benchmark_manifest_path(state, &arguments);
            let suite_ids = optional_string_array(&arguments, "suite_ids")?;
            let run_all = arguments
                .get("all")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let output_path = optional_string(&arguments, "output_path");
            let output = benchmark::run_benchmarks(
                &state.app.paths,
                &manifest_path,
                &suite_ids,
                run_all,
                output_path.as_deref().map(std::path::Path::new),
            )
            .await
            .map_err(internal_error)?;
            let suite_statuses = output
                .report
                .suites
                .iter()
                .map(|suite| {
                    json!({
                        "id": suite.id,
                        "status": suite.status,
                        "duration_ms": suite.duration_ms,
                        "reason": suite.reason,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "json_path": output.json_path.display().to_string(),
                "markdown_path": output.markdown_path.display().to_string(),
                "generated_at": output.report.generated_at,
                "manifest_path": output.report.manifest_path,
                "selected_suites": output.report.selected_suites,
                "summary": output.report.summary,
                "suite_statuses": suite_statuses
            })
        }
        "list_benchmark_reports" => {
            let limit = arguments
                .get("limit")
                .and_then(Value::as_i64)
                .unwrap_or(10)
                .clamp(1, 100) as usize;
            let reports = benchmark::list_recent_run_reports(&state.app.paths, limit)
                .map_err(internal_error)?;
            json!(reports)
        }
        "get_benchmark_report" => {
            let report_id = optional_string(&arguments, "report_id");
            let format = arguments
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or("markdown");
            let json_path =
                benchmark::resolve_run_report_path(&state.app.paths, report_id.as_deref())
                    .map_err(internal_error)?;
            let report = benchmark::load_run_report(&json_path).map_err(internal_error)?;
            let rendered = match format {
                "markdown" => benchmark::render_report_markdown(&report),
                "json" => serde_json::to_string_pretty(&report).map_err(internal_error)?,
                "summary" => serde_json::to_string_pretty(&json!({
                    "report_id": json_path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or("unknown"),
                    "json_path": json_path.display().to_string(),
                    "markdown_path": json_path.with_extension("md").display().to_string(),
                    "generated_at": report.generated_at,
                    "selected_suites": report.selected_suites,
                    "summary": report.summary,
                }))
                .map_err(internal_error)?,
                other => {
                    return Err((
                        -32602,
                        format!(
                            "unsupported benchmark report format: {other} (expected markdown, json, or summary)"
                        ),
                    ))
                }
            };
            return Ok(text_tool_result(&rendered));
        }
        _ => return Err((-32601, format!("unknown tool: {name}"))),
    };

    Ok(text_tool_result_pretty(&result))
}

async fn read_resource(
    state: &McpState,
    params: &Value,
) -> std::result::Result<Value, (i64, String)> {
    let uri = required_string(params, "uri")?;
    let (mime_type, payload) = if uri == "harkonnen://runs" {
        let runs = state.app.list_runs(20).await.map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&runs).map_err(internal_error)?,
        )
    } else if let Some(run_id) = uri.strip_prefix("harkonnen://runs/") {
        let run = state
            .app
            .get_run(run_id)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| (-32004, format!("run not found: {run_id}")))?;
        (
            "application/json",
            serde_json::to_string_pretty(&run).map_err(internal_error)?,
        )
    } else if let Some(run_id) = uri.strip_prefix("harkonnen://reports/") {
        let report = reporting::build_report(&state.app, run_id)
            .await
            .map_err(internal_error)?;
        ("text/plain", report)
    } else if let Some(run_id) = uri.strip_prefix("harkonnen://watch/") {
        let payload = run_watch_payload(state, run_id, 12)
            .await
            .map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&payload).map_err(internal_error)?,
        )
    } else if let Some(run_id) = uri.strip_prefix("harkonnen://boards/") {
        let payload = crate::api::build_run_board_snapshot(&state.app, run_id)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| (-32004, format!("run not found: {run_id}")))?;
        (
            "application/json",
            serde_json::to_string_pretty(&payload).map_err(internal_error)?,
        )
    } else if uri == "harkonnen://chat/threads" {
        let threads = state
            .app
            .chat
            .list_threads(None, None, 50)
            .await
            .map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&threads).map_err(internal_error)?,
        )
    } else if let Some(thread_id) = uri.strip_prefix("harkonnen://chat/messages/") {
        let messages = state
            .app
            .chat
            .list_messages(thread_id)
            .await
            .map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&messages).map_err(internal_error)?,
        )
    } else if let Some(thread_id) = uri.strip_prefix("harkonnen://chat/threads/") {
        let thread = state
            .app
            .chat
            .get_thread(thread_id)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| (-32004, format!("thread not found: {thread_id}")))?;
        (
            "application/json",
            serde_json::to_string_pretty(&thread).map_err(internal_error)?,
        )
    } else if let Some(run_id) = uri.strip_prefix("harkonnen://checkpoints/") {
        let checkpoints = state
            .app
            .list_run_checkpoints(run_id)
            .await
            .map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&checkpoints).map_err(internal_error)?,
        )
    } else if uri == "harkonnen://benchmarks/suites" {
        let manifest_path = benchmark::default_manifest_path(&state.app.paths);
        let manifest = benchmark::load_manifest(&manifest_path).map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&json!({
                "manifest_path": manifest_path.display().to_string(),
                "version": manifest.version,
                "suites": manifest.suites
            }))
            .map_err(internal_error)?,
        )
    } else if uri == "harkonnen://benchmarks/reports" {
        let reports =
            benchmark::list_recent_run_reports(&state.app.paths, 20).map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&reports).map_err(internal_error)?,
        )
    } else if let Some(report_id) = uri.strip_prefix("harkonnen://benchmarks/reports/") {
        let json_path = benchmark::resolve_run_report_path(&state.app.paths, Some(report_id))
            .map_err(internal_error)?;
        let report = benchmark::load_run_report(&json_path).map_err(internal_error)?;
        (
            "application/json",
            serde_json::to_string_pretty(&report).map_err(internal_error)?,
        )
    } else {
        return Err((-32602, format!("unknown resource URI: {uri}")));
    };

    Ok(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": mime_type,
                "text": payload
            }
        ]
    }))
}

fn get_prompt(params: &Value) -> std::result::Result<Value, (i64, String)> {
    let name = required_string(params, "name")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let text = match name.as_str() {
        "briefing_for_spec" => {
            let spec_id = required_string(&arguments, "spec_id")?;
            format!(
                "Build a concise Harkonnen briefing for spec `{spec_id}`. Include likely risks, guardrails, and the next recommended operator action."
            )
        }
        "diagnose_run" => {
            let run_id = required_string(&arguments, "run_id")?;
            format!(
                "Diagnose Harkonnen run `{run_id}`. Summarize status, likely failure causes, operator-visible risks, and the most useful next debugging step."
            )
        }
        _ => return Err((-32601, format!("unknown prompt: {name}"))),
    };

    Ok(json!({
        "description": format!("Prompt template `{name}`"),
        "messages": [
            {
                "role": "user",
                "content": {
                    "type": "text",
                    "text": text
                }
            }
        ]
    }))
}

fn required_string(params: &Value, key: &str) -> std::result::Result<String, (i64, String)> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(|value| value.to_string())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| (-32602, format!("missing required argument: {key}")))
}

fn optional_string(params: &Value, key: &str) -> Option<String> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
}

fn optional_string_array(
    params: &Value,
    key: &str,
) -> std::result::Result<Vec<String>, (i64, String)> {
    let Some(value) = params.get(key) else {
        return Ok(Vec::new());
    };
    let Some(values) = value.as_array() else {
        return Err((-32602, format!("{key} must be an array of strings")));
    };
    let mut result = Vec::new();
    for value in values {
        let Some(raw) = value.as_str() else {
            return Err((-32602, format!("{key} must be an array of strings")));
        };
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            result.push(trimmed.to_string());
        }
    }
    Ok(result)
}

fn optional_chat_thread_kind(
    params: &Value,
    key: &str,
) -> std::result::Result<Option<ChatThreadKind>, (i64, String)> {
    let Some(value) = optional_string(params, key) else {
        return Ok(None);
    };
    match value.as_str() {
        "general" => Ok(Some(ChatThreadKind::General)),
        "run" => Ok(Some(ChatThreadKind::Run)),
        "spec" => Ok(Some(ChatThreadKind::Spec)),
        "operator_model" => Ok(Some(ChatThreadKind::OperatorModel)),
        other => Err((
            -32602,
            format!(
                "unsupported thread_kind: {other} (expected general, run, spec, or operator_model)"
            ),
        )),
    }
}

fn resolve_spec_reference(app: &AppContext, spec: &str) -> String {
    let spec = spec.trim();
    if spec.ends_with(".yaml") || spec.ends_with(".yml") || spec.contains('/') {
        return spec.to_string();
    }
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

async fn persist_spec_yaml(app: &AppContext, spec_path: &str, spec_yaml: &str) -> Result<()> {
    serde_yaml::from_str::<Spec>(spec_yaml).context("draft spec yaml is invalid")?;

    let spec_path_buf = std::path::PathBuf::from(spec_path);
    let spec_path_abs = if spec_path_buf.is_absolute() {
        spec_path_buf
    } else {
        app.paths.root.join(spec_path_buf)
    };

    if let Some(parent) = spec_path_abs.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("creating spec parent dir {}", parent.display()))?;
    }

    tokio::fs::write(&spec_path_abs, spec_yaml.as_bytes())
        .await
        .with_context(|| format!("writing draft spec {}", spec_path_abs.display()))?;
    Ok(())
}

async fn run_watch_payload(state: &McpState, run_id: &str, event_limit: usize) -> Result<Value> {
    let run = state
        .app
        .get_run(run_id)
        .await?
        .with_context(|| format!("run not found: {run_id}"))?;
    let mut events = state.app.list_run_events(run_id).await?;
    if events.len() > event_limit {
        let split_at = events.len() - event_limit;
        events = events.split_off(split_at);
    }
    let active_checkpoints = state
        .app
        .list_run_checkpoints(run_id)
        .await?
        .into_iter()
        .filter(|checkpoint| matches!(checkpoint.status.as_str(), "open" | "answered"))
        .collect::<Vec<_>>();
    let run_dir = state.app.paths.workspaces.join(run_id).join("run");
    let run_timing = match tokio::fs::read_to_string(run_dir.join("run_timing.json")).await {
        Ok(raw) => serde_json::from_str::<Value>(&raw).ok(),
        Err(_) => None,
    };
    Ok(json!({
        "run": run,
        "recent_events": events,
        "active_checkpoints": active_checkpoints,
        "run_timing": run_timing,
    }))
}

fn summarize_board_snapshot(snapshot: &Value) -> Value {
    let mission = snapshot.get("mission").and_then(Value::as_object);
    let action = snapshot.get("action").and_then(Value::as_object);
    let evidence = snapshot.get("evidence").and_then(Value::as_object);
    let memory = snapshot.get("memory").and_then(Value::as_object);

    json!({
        "run_id": snapshot.get("run_id").cloned().unwrap_or(Value::Null),
        "current_phase": mission
            .and_then(|board| board.get("current_phase"))
            .cloned()
            .or_else(|| action.and_then(|board| board.get("current_phase")).cloned())
            .unwrap_or(Value::Null),
        "run_status": mission
            .and_then(|board| board.get("run_status"))
            .cloned()
            .unwrap_or(Value::Null),
        "active_goal": mission
            .and_then(|board| board.get("active_goal"))
            .cloned()
            .or_else(|| action.and_then(|board| board.get("active_goal")).cloned())
            .unwrap_or(Value::Null),
        "open_blocker_count": mission
            .and_then(|board| board.get("open_blockers"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "open_checkpoint_count": action
            .and_then(|board| board.get("open_checkpoints"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "artifact_ref_count": evidence
            .and_then(|board| board.get("artifact_refs"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "recent_evidence_event_count": evidence
            .and_then(|board| board.get("recent_evidence_events"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "active_lesson_count": memory
            .and_then(|board| board.get("active_recalled_lessons"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "policy_reminder_count": memory
            .and_then(|board| board.get("policy_reminders"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "stale_risk_count": memory
            .and_then(|board| board.get("stale_risk_summary"))
            .and_then(Value::as_object)
            .and_then(|summary| summary.get("stale_risk_count"))
            .cloned()
            .unwrap_or(Value::from(0)),
        "active_risk_score": memory
            .and_then(|board| board.get("stale_risk_summary"))
            .and_then(Value::as_object)
            .and_then(|summary| summary.get("active_risk_score"))
            .cloned()
            .unwrap_or(Value::from(0)),
    })
}

fn benchmark_manifest_path(state: &McpState, arguments: &Value) -> std::path::PathBuf {
    optional_string(arguments, "manifest_path")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| benchmark::default_manifest_path(&state.app.paths))
}

fn requested_protocol_version(params: &Value) -> String {
    params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("2025-11-25")
        .to_string()
}

fn internal_error(error: impl ToString) -> (i64, String) {
    (-32000, error.to_string())
}

fn rpc_error_value(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

fn text_tool_result(text: &str) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": text
            }
        ]
    })
}

fn text_tool_result_pretty(value: &Value) -> Value {
    let rendered = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    text_tool_result(&rendered)
}

fn tool_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "list_runs",
            "description": "List recent Harkonnen runs.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        }),
        json!({
            "name": "get_run",
            "description": "Fetch a specific run record.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id"],
                "properties": {
                    "run_id": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "get_run_report",
            "description": "Render the full text run report for a specific run.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id"],
                "properties": {
                    "run_id": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "list_run_decisions",
            "description": "List decision log records for a run.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id"],
                "properties": {
                    "run_id": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "start_run",
            "description": "Start a new Harkonnen run from a spec path and product target.",
            "inputSchema": {
                "type": "object",
                "required": ["spec"],
                "properties": {
                    "spec": { "type": "string" },
                    "product": { "type": "string" },
                    "product_path": { "type": "string" },
                    "spec_yaml": { "type": "string" },
                    "run_hidden_scenarios": { "type": "boolean" }
                }
            }
        }),
        json!({
            "name": "queue_run",
            "description": "Queue a new Harkonnen run and return immediately with the queued run record.",
            "inputSchema": {
                "type": "object",
                "required": ["spec"],
                "properties": {
                    "spec": { "type": "string" },
                    "product": { "type": "string" },
                    "product_path": { "type": "string" },
                    "spec_yaml": { "type": "string" },
                    "run_hidden_scenarios": { "type": "boolean" }
                }
            }
        }),
        json!({
            "name": "watch_run",
            "description": "Return a compact progress snapshot for a run, including recent events and active checkpoints.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id"],
                "properties": {
                    "run_id": { "type": "string" },
                    "event_limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        }),
        json!({
            "name": "get_run_board_snapshot",
            "description": "Return the Mission, Action, Evidence, and Memory board snapshot for a run.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id"],
                "properties": {
                    "run_id": { "type": "string" },
                    "view": {
                        "type": "string",
                        "enum": ["summary", "full"]
                    }
                }
            }
        }),
        json!({
            "name": "list_chat_threads",
            "description": "List PackChat threads, optionally filtered by run_id or thread kind.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "run_id": { "type": "string" },
                    "thread_kind": {
                        "type": "string",
                        "enum": ["general", "run", "spec", "operator_model"]
                    },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        }),
        json!({
            "name": "open_chat_thread",
            "description": "Open a new PackChat thread for general, run, spec, or operator-model discussion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "run_id": { "type": "string" },
                    "spec_id": { "type": "string" },
                    "title": { "type": "string" },
                    "thread_kind": {
                        "type": "string",
                        "enum": ["general", "run", "spec", "operator_model"]
                    },
                    "metadata_json": { "type": "object" }
                }
            }
        }),
        json!({
            "name": "get_chat_thread",
            "description": "Fetch a PackChat thread by id.",
            "inputSchema": {
                "type": "object",
                "required": ["thread_id"],
                "properties": {
                    "thread_id": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "list_chat_messages",
            "description": "List all messages in a PackChat thread.",
            "inputSchema": {
                "type": "object",
                "required": ["thread_id"],
                "properties": {
                    "thread_id": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "post_chat_message",
            "description": "Post an operator message into a PackChat thread and persist the agent reply.",
            "inputSchema": {
                "type": "object",
                "required": ["thread_id", "content"],
                "properties": {
                    "thread_id": { "type": "string" },
                    "content": { "type": "string" },
                    "agent": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "list_run_checkpoints",
            "description": "List current run checkpoints for a Harkonnen run.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id"],
                "properties": {
                    "run_id": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "reply_to_checkpoint",
            "description": "Reply to a run checkpoint with operator text or decision JSON.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id", "checkpoint_id"],
                "properties": {
                    "run_id": { "type": "string" },
                    "checkpoint_id": { "type": "string" },
                    "answered_by": { "type": "string" },
                    "answer_text": { "type": "string" },
                    "decision_json": { "type": "object" },
                    "resolve": { "type": "boolean" }
                }
            }
        }),
        json!({
            "name": "unblock_agent",
            "description": "Resolve open checkpoints for a named agent on a run and unblock progress.",
            "inputSchema": {
                "type": "object",
                "required": ["run_id", "agent"],
                "properties": {
                    "run_id": { "type": "string" },
                    "agent": { "type": "string" },
                    "checkpoint_id": { "type": "string" },
                    "answered_by": { "type": "string" },
                    "answer_text": { "type": "string" },
                    "decision_json": { "type": "object" }
                }
            }
        }),
        json!({
            "name": "list_benchmark_suites",
            "description": "List benchmark suites from the benchmark manifest.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "manifest_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "run_benchmarks",
            "description": "Run selected benchmark suites and write report artifacts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "manifest_path": { "type": "string" },
                    "suite_ids": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "all": { "type": "boolean" },
                    "output_path": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "list_benchmark_reports",
            "description": "List recent benchmark report artifacts written under factory/artifacts/benchmarks.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
                }
            }
        }),
        json!({
            "name": "get_benchmark_report",
            "description": "Render a benchmark report artifact by id, defaulting to the latest report.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "report_id": { "type": "string" },
                    "format": {
                        "type": "string",
                        "enum": ["markdown", "json", "summary"]
                    }
                }
            }
        }),
    ]
}

fn resource_descriptors() -> Vec<Value> {
    vec![
        json!({
            "uri": "harkonnen://runs",
            "name": "Recent Runs",
            "description": "Recent Harkonnen run records.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://runs/{run_id}",
            "name": "Run Detail",
            "description": "A single Harkonnen run record.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://reports/{run_id}",
            "name": "Run Report",
            "description": "Rendered text report for a Harkonnen run.",
            "mimeType": "text/plain"
        }),
        json!({
            "uriTemplate": "harkonnen://watch/{run_id}",
            "name": "Run Watch",
            "description": "Compact run progress snapshot with recent events and active checkpoints.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://boards/{run_id}",
            "name": "Run Boards",
            "description": "Mission, Action, Evidence, and Memory board snapshot for a run.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "harkonnen://chat/threads",
            "name": "PackChat Threads",
            "description": "Recent PackChat thread records.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://chat/threads/{thread_id}",
            "name": "PackChat Thread",
            "description": "A single PackChat thread record.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://chat/messages/{thread_id}",
            "name": "PackChat Messages",
            "description": "All messages in a PackChat thread.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://checkpoints/{run_id}",
            "name": "Run Checkpoints",
            "description": "Current checkpoints for a run.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "harkonnen://benchmarks/suites",
            "name": "Benchmark Suites",
            "description": "Benchmark suites from the default Harkonnen benchmark manifest.",
            "mimeType": "application/json"
        }),
        json!({
            "uri": "harkonnen://benchmarks/reports",
            "name": "Benchmark Reports",
            "description": "Recent benchmark report artifacts.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://benchmarks/reports/{report_id}",
            "name": "Benchmark Report Detail",
            "description": "A specific benchmark report artifact.",
            "mimeType": "application/json"
        }),
    ]
}

fn prompt_descriptors() -> Vec<Value> {
    vec![
        json!({
            "name": "briefing_for_spec",
            "description": "Prompt template for building a Coobie-style briefing for a spec."
        }),
        json!({
            "name": "diagnose_run",
            "description": "Prompt template for diagnosing a completed or failed run."
        }),
    ]
}

async fn read_stdio_message<R>(reader: &mut R) -> Result<Option<(Value, StdioMessageFormat)>>
where
    R: AsyncBufRead + Unpin,
{
    let initial = reader.fill_buf().await?;
    if initial.is_empty() {
        return Ok(None);
    }
    if let Some(first_non_ws) = initial
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
    {
        if matches!(first_non_ws, b'{' | b'[') {
            return read_unframed_json_message(reader)
                .await
                .map(|value| Some((value, StdioMessageFormat::UnframedJson)));
        }
    }

    let mut content_length = None;
    let mut saw_header = false;

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            if !saw_header {
                return Ok(None);
            }
            bail!("unexpected EOF while reading MCP stdio headers");
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            if !saw_header {
                continue;
            }
            break;
        }
        saw_header = true;

        if let Some((name, value)) = trimmed.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let parsed = value.trim().parse::<usize>().with_context(|| {
                    format!("invalid MCP Content-Length header: {}", value.trim())
                })?;
                content_length = Some(parsed);
            }
        }
    }

    let content_length = content_length.context("missing MCP Content-Length header")?;
    let mut payload = vec![0u8; content_length];
    tokio::io::AsyncReadExt::read_exact(reader, &mut payload).await?;
    let body = serde_json::from_slice(&payload).context("parsing MCP stdio JSON payload")?;
    Ok(Some((body, StdioMessageFormat::ContentLengthFramed)))
}

async fn read_unframed_json_message<R>(reader: &mut R) -> Result<Value>
where
    R: AsyncBufRead + Unpin,
{
    let mut payload = Vec::new();

    if let Ok(result) = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        reader.read_until(b'\n', &mut payload),
    )
    .await
    {
        let bytes = result?;
        if bytes == 0 {
            bail!("unexpected EOF while reading unframed MCP stdio JSON payload");
        }
        if let Some((value, _consumed)) = parse_first_unframed_json_value(&payload)? {
            return Ok(value);
        }
    }
    if let Some((value, _consumed)) = parse_first_unframed_json_value(&payload)? {
        return Ok(value);
    }

    let buffer = reader.fill_buf().await?;
    if buffer.is_empty() {
        bail!("unexpected EOF while reading unframed MCP stdio JSON payload");
    }
    let Some((value, consumed)) = parse_first_unframed_json_value(buffer)? else {
        bail!("parsing unframed MCP stdio JSON payload");
    };
    reader.consume(consumed);
    Ok(value)
}

fn parse_first_unframed_json_value(payload: &[u8]) -> Result<Option<(Value, usize)>> {
    let mut stream = serde_json::Deserializer::from_slice(payload).into_iter::<Value>();
    match stream.next() {
        Some(Ok(value)) => {
            let mut consumed = stream.byte_offset();
            consumed += payload[consumed..]
                .iter()
                .take_while(|byte| byte.is_ascii_whitespace())
                .count();
            Ok(Some((value, consumed)))
        }
        Some(Err(error)) if error.is_eof() => Ok(None),
        Some(Err(error)) => Err(error).context("parsing unframed MCP stdio JSON payload"),
        None => Ok(None),
    }
}

async fn write_stdio_message<W>(
    writer: &mut W,
    value: &Value,
    format: StdioMessageFormat,
) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(value).context("serializing MCP stdio response")?;
    if format == StdioMessageFormat::ContentLengthFramed {
        let header = format!(
            "Content-Length: {}\r\nContent-Type: application/json\r\n\r\n",
            payload.len()
        );
        writer.write_all(header.as_bytes()).await?;
    }
    writer.write_all(&payload).await?;
    if format == StdioMessageFormat::UnframedJson {
        writer.write_all(b"\n").await?;
    }
    writer.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{read_stdio_message, write_stdio_message, StdioMessageFormat};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn reads_content_length_framed_stdio_message() {
        let payload = br#"{"jsonrpc":"2.0","method":"ping"}"#;
        let frame = format!("Content-Length: {}\r\n\r\n", payload.len());
        let (mut client, server) = tokio::io::duplex(512);

        let reader_task = tokio::spawn(async move {
            let mut reader = BufReader::new(server);
            read_stdio_message(&mut reader).await.unwrap().unwrap()
        });

        client.write_all(frame.as_bytes()).await.unwrap();
        client.write_all(payload).await.unwrap();
        drop(client);

        let message = reader_task.await.unwrap();
        assert_eq!(message.0["method"], "ping");
        assert_eq!(message.1, StdioMessageFormat::ContentLengthFramed);
    }

    #[tokio::test]
    async fn writes_content_length_framed_stdio_message() {
        let message = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});
        let (client, mut server) = tokio::io::duplex(512);

        let writer_task = tokio::spawn(async move {
            let mut writer = tokio::io::BufWriter::new(client);
            write_stdio_message(
                &mut writer,
                &message,
                StdioMessageFormat::ContentLengthFramed,
            )
            .await
            .unwrap();
        });

        let mut buf = Vec::new();
        server.read_to_end(&mut buf).await.unwrap();
        writer_task.await.unwrap();

        let text = String::from_utf8(buf).unwrap();
        assert!(text.contains("Content-Length:"));
        assert!(text.contains("\"ok\":true"));
    }

    #[tokio::test]
    async fn reads_newline_delimited_stdio_message() {
        let payload =
            br#"{"jsonrpc":"2.0","method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#;
        let (mut client, server) = tokio::io::duplex(512);

        let reader_task = tokio::spawn(async move {
            let mut reader = BufReader::new(server);
            read_stdio_message(&mut reader).await.unwrap().unwrap()
        });

        client.write_all(payload).await.unwrap();
        client.write_all(b"\n").await.unwrap();
        drop(client);

        let message = reader_task.await.unwrap();
        assert_eq!(message.0["method"], "initialize");
        assert_eq!(message.1, StdioMessageFormat::UnframedJson);
    }

    #[tokio::test]
    async fn reads_unframed_stdio_message_without_trailing_newline() {
        let payload = br#"{"jsonrpc":"2.0","method":"initialize","id":0}"#;
        let (mut client, server) = tokio::io::duplex(512);

        let reader_task = tokio::spawn(async move {
            let mut reader = BufReader::new(server);
            read_stdio_message(&mut reader).await.unwrap().unwrap()
        });

        client.write_all(payload).await.unwrap();

        let message = tokio::time::timeout(std::time::Duration::from_secs(1), reader_task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(message.0["method"], "initialize");
        assert_eq!(message.1, StdioMessageFormat::UnframedJson);
    }

    #[tokio::test]
    async fn writes_unframed_stdio_message_with_trailing_newline() {
        let message = json!({"jsonrpc":"2.0","id":0,"result":{"ok":true}});
        let (client, mut server) = tokio::io::duplex(512);

        let writer_task = tokio::spawn(async move {
            let mut writer = tokio::io::BufWriter::new(client);
            write_stdio_message(&mut writer, &message, StdioMessageFormat::UnframedJson)
                .await
                .unwrap();
        });

        let mut buf = Vec::new();
        server.read_to_end(&mut buf).await.unwrap();
        writer_task.await.unwrap();

        let text = String::from_utf8(buf).unwrap();
        assert!(!text.contains("Content-Length:"));
        assert!(text.ends_with('\n'));
        assert!(text.contains("\"ok\":true"));
    }
}
