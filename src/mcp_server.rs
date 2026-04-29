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
        "prompts/get" => get_prompt(state, &envelope.params).await?,
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
        "get_run_reasoning_snapshot" => {
            let run_id = required_string(&arguments, "run_id")?;
            let snapshot = crate::api::build_run_reasoning_snapshot(&state.app, &run_id)
                .await
                .map_err(internal_error)?
                .ok_or_else(|| (-32004, format!("run not found: {run_id}")))?;
            let view = arguments
                .get("view")
                .and_then(Value::as_str)
                .unwrap_or("summary");
            match view {
                "full" => json!(snapshot),
                "summary" => json!({
                    "run_id": snapshot.run_id,
                    "run_status": snapshot.run_status,
                    "current_phase": snapshot.current_phase,
                    "decision_count": snapshot.decision_count,
                    "checkpoint_answer_count": snapshot.checkpoint_answer_count,
                    "open_checkpoint_count": snapshot.open_checkpoint_count,
                    "recent_decision_count": snapshot.recent_decisions.len(),
                    "recent_checkpoint_answer_count": snapshot.recent_checkpoint_answers.len(),
                }),
                other => {
                    return Err((
                        -32602,
                        format!(
                        "unsupported reasoning snapshot view: {other} (expected summary or full)"
                    ),
                    ))
                }
            }
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
        // ══════════════════════════════════════════════════════════════════════
        // CONSOLIDATED NATIVE TOOLS — Phase 5b
        // Replaces three npx MCP servers (filesystem, memory, sqlite) with
        // Rust-native implementations backed by tokio::fs and sqlx.
        // All writes are boundary-enforced to allowed directories.
        // ══════════════════════════════════════════════════════════════════════

        // ── filesystem_read alias ─────────────────────────────────────────────
        "read_file" => {
            let path = required_string(&arguments, "path")?;
            let abs = resolve_allowed_read_path(&state.app.paths, &path)?;
            let content = tokio::fs::read_to_string(&abs)
                .await
                .map_err(|e| (-32004, format!("read_file failed for {path}: {e}")))?;
            return Ok(text_tool_result(&content));
        }

        "list_directory" => {
            let path = optional_string(&arguments, "path").unwrap_or_else(|| ".".into());
            let abs = resolve_allowed_read_path(&state.app.paths, &path)?;
            let mut entries = tokio::fs::read_dir(&abs)
                .await
                .map_err(|e| (-32004, format!("list_directory failed for {path}: {e}")))?;
            let mut names: Vec<String> = Vec::new();
            while let Some(entry) = entries.next_entry().await.map_err(internal_error)? {
                let name = entry.file_name().to_string_lossy().to_string();
                let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
                names.push(if is_dir { format!("{name}/") } else { name });
            }
            names.sort();
            return Ok(text_tool_result(&names.join("\n")));
        }

        // ── workspace_write / artifact_writer aliases ─────────────────────────
        "write_file" => {
            let path = required_string(&arguments, "path")?;
            let content = required_string(&arguments, "content")?;
            let abs = resolve_allowed_write_path(&state.app.paths, &path)?;
            if let Some(parent) = abs.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(internal_error)?;
            }
            tokio::fs::write(&abs, &content)
                .await
                .map_err(|e| (-32004, format!("write_file failed for {path}: {e}")))?;
            return Ok(text_tool_result(&format!(
                "wrote {} bytes to {path}",
                content.len()
            )));
        }

        "create_directory" => {
            let path = required_string(&arguments, "path")?;
            let abs = resolve_allowed_write_path(&state.app.paths, &path)?;
            tokio::fs::create_dir_all(&abs)
                .await
                .map_err(|e| (-32004, format!("create_directory failed for {path}: {e}")))?;
            return Ok(text_tool_result(&format!("created {path}")));
        }

        // ── memory_store / metadata_query aliases ─────────────────────────────
        "memory_store" => {
            let content = required_string(&arguments, "content")?;
            let tags = optional_string_array(&arguments, "tags").unwrap_or_default();
            let id = uuid::Uuid::new_v4().to_string();
            let tag_prefix = if tags.is_empty() {
                String::new()
            } else {
                format!("[{}] ", tags.join(", "))
            };
            let entry = format!("{tag_prefix}{content}");
            // Write to a timestamped file in the memory store so it's indexed.
            let ts = chrono::Utc::now().format("%Y%m%dT%H%M%S");
            let filename = format!("mcp-store-{ts}-{}.md", &id[..8]);
            let path = state.app.paths.memory.join(&filename);
            tokio::fs::write(
                &path,
                &format!("---\ntags: [mcp_store]\nsummary: {content}\n---\n{entry}"),
            )
            .await
            .map_err(internal_error)?;
            // Also capture to OB1 if available.
            if let Some(ob) = state.app.open_brain.as_ref() {
                let _ = ob.capture_thought(&entry, Some("mcp_store")).await;
            }
            return Ok(text_tool_result(&format!("stored memory entry: {id}")));
        }

        "memory_retrieve" => {
            let query = required_string(&arguments, "query")?;
            let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(8) as usize;
            let mut hits = state
                .app
                .memory_store
                .retrieve_context(&query)
                .await
                .unwrap_or_default();
            if let Some(ob) = state.app.open_brain.as_ref() {
                if let Ok(ob_hits) = ob.search_thoughts(&query).await {
                    for h in ob_hits {
                        if !hits.contains(&h) {
                            hits.push(h);
                        }
                    }
                }
            }
            hits.truncate(limit);
            let body = hits
                .iter()
                .enumerate()
                .map(|(i, h)| format!("{}. {}", i + 1, h.trim()))
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(text_tool_result(if body.is_empty() {
                "No memory found."
            } else {
                &body
            }));
        }

        "memory_list" => {
            let limit = arguments.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
            let entries = state
                .app
                .memory_store
                .list_entries()
                .await
                .unwrap_or_default()
                .into_iter()
                .take(limit)
                .collect::<Vec<_>>();
            let body = entries
                .iter()
                .enumerate()
                .map(|(i, e)| format!("{}. [{}] {}", i + 1, e.id, e.summary))
                .collect::<Vec<_>>()
                .join("\n");
            return Ok(text_tool_result(if body.is_empty() {
                "Memory is empty."
            } else {
                &body
            }));
        }

        // ── db_read alias ─────────────────────────────────────────────────────
        "db_list_tables" => {
            let rows =
                sqlx::query("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                    .fetch_all(&state.app.pool)
                    .await
                    .map_err(internal_error)?;
            let names: Vec<String> = rows
                .iter()
                .filter_map(|r| {
                    use sqlx::Row as _;
                    r.try_get("name").ok()
                })
                .collect();
            return Ok(text_tool_result(&names.join("\n")));
        }

        "db_query" => {
            let sql = required_string(&arguments, "sql")?;
            // Enforce read-only: reject any non-SELECT statement.
            let normalized = sql.trim().to_lowercase();
            if !normalized.starts_with("select") && !normalized.starts_with("with") {
                return Err((
                    -32602,
                    "db_query only permits SELECT / WITH queries".to_string(),
                ));
            }
            let rows = sqlx::query(&sql)
                .fetch_all(&state.app.pool)
                .await
                .map_err(|e| (-32004, format!("db_query failed: {e}")))?;
            let mut lines: Vec<String> = Vec::new();
            for row in &rows {
                use sqlx::{Column as _, Row as _, TypeInfo as _};
                let mut cols: Vec<String> = Vec::new();
                for i in 0..row.len() {
                    let col_name = row.column(i).name().to_string();
                    let val: String = row
                        .try_get::<String, _>(i)
                        .or_else(|_| row.try_get::<i64, _>(i).map(|n| n.to_string()))
                        .or_else(|_| row.try_get::<f64, _>(i).map(|n| format!("{n:.4}")))
                        .or_else(|_| row.try_get::<bool, _>(i).map(|b| b.to_string()))
                        .unwrap_or_else(|_| "NULL".to_string());
                    cols.push(format!("{col_name}={val}"));
                }
                lines.push(cols.join(" | "));
            }
            let body = if lines.is_empty() {
                "(no rows)".to_string()
            } else {
                format!("{} row(s):\n{}", lines.len(), lines.join("\n"))
            };
            return Ok(text_tool_result(&body));
        }

        // ── memory_pull: on-demand context retrieval mid-task (Phase 5b) ─────
        "memory_pull" => {
            let query = required_string(&arguments, "query")
                .map_err(|_| (-32602, "memory_pull requires query".to_string()))?;
            let scope = optional_string(&arguments, "scope").unwrap_or_else(|| "general".into());
            let max_tokens = arguments
                .get("max_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(500) as usize;

            let mut hits: Vec<String> = Vec::new();

            // File-backed memory search.
            if let Ok(h) = state.app.memory_store.retrieve_context(&query).await {
                hits.extend(h);
            }

            // OB1 semantic search.
            if let Some(ob) = state.app.open_brain.as_ref() {
                if let Ok(ob_hits) = ob.search_thoughts(&query).await {
                    for hit in ob_hits {
                        if !hits.contains(&hit) {
                            hits.push(hit);
                        }
                    }
                }
            }

            // Scope filtering: drop hits tagged with disallowed categories.
            let disallowed = scope_disallowed_tags(&scope);
            hits.retain(|h| {
                let lower = h.to_lowercase();
                !disallowed.iter().any(|tag| lower.contains(tag))
            });

            // Budget enforcement (~4 chars per token).
            let char_budget = max_tokens * 4;
            let mut total = 0usize;
            let mut output_lines: Vec<String> = Vec::new();
            for hit in &hits {
                if total + hit.len() > char_budget {
                    break;
                }
                total += hit.len();
                output_lines.push(hit.trim().to_string());
            }

            let hits_returned = output_lines.len() as u32;
            let tokens_approx = (total / 4) as u32;

            // Log the pull call for context utilization tracking.
            tracing::info!(
                query = %query,
                scope = %scope,
                hits_returned = hits_returned,
                tokens = tokens_approx,
                "memory_pull"
            );

            let body = if output_lines.is_empty() {
                format!("No memory found for query: \"{query}\" (scope: {scope})")
            } else {
                format!(
                    "# Memory pull — query: \"{query}\" | scope: {scope} | {hits_returned} hit(s)\n\n{}",
                    output_lines
                        .iter()
                        .enumerate()
                        .map(|(i, h)| format!("{}. {h}", i + 1))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };

            return Ok(text_tool_result(&body));
        }

        _ => return Err((-32601, format!("unknown tool: {name}"))),
    };

    Ok(text_tool_result_pretty(&result))
}

/// Resolve a path for read access. Allowed roots: products, workspaces,
/// artifacts, memory, specs, logs, the-soul-of-ai, factory/context.
fn resolve_allowed_read_path(
    paths: &crate::config::Paths,
    requested: &str,
) -> std::result::Result<std::path::PathBuf, (i64, String)> {
    let candidate = if std::path::Path::new(requested).is_absolute() {
        std::path::PathBuf::from(requested)
    } else {
        paths.root.join(requested)
    };
    let canonical = candidate.canonicalize().unwrap_or(candidate.clone());

    let allowed: Vec<std::path::PathBuf> = vec![
        paths.products.clone(),
        paths.workspaces.clone(),
        paths.artifacts.clone(),
        paths.memory.clone(),
        paths.specs.clone(),
        paths.logs.clone(),
        paths.root.join("factory/context"),
        paths.root.join("the-soul-of-ai"),
        paths.root.join("factory/calvin_archive"),
        paths.root.join("ROADMAP.md"),
        paths.root.join("MASTER_SPEC.md"),
        paths.root.join("AGENTS.md"),
        paths.root.join("CLAUDE.md"),
    ];

    let ok = allowed.iter().any(|root| {
        root.canonicalize()
            .map(|r| canonical.starts_with(&r) || canonical == r)
            .unwrap_or_else(|_| canonical.starts_with(root))
    });

    if ok {
        Ok(canonical)
    } else {
        Err((
            -32602,
            format!("read_file path '{requested}' is outside allowed directories"),
        ))
    }
}

/// Resolve a path for write access. Allowed roots: workspaces, artifacts only.
fn resolve_allowed_write_path(
    paths: &crate::config::Paths,
    requested: &str,
) -> std::result::Result<std::path::PathBuf, (i64, String)> {
    let candidate = if std::path::Path::new(requested).is_absolute() {
        std::path::PathBuf::from(requested)
    } else {
        paths.root.join(requested)
    };

    let allowed: Vec<&std::path::PathBuf> = vec![&paths.workspaces, &paths.artifacts];
    let ok = allowed.iter().any(|root| {
        let norm_root = root.to_string_lossy();
        let norm_cand = candidate.to_string_lossy();
        norm_cand.starts_with(norm_root.as_ref())
    });

    if ok {
        Ok(candidate)
    } else {
        Err((
            -32602,
            format!(
                "write_file path '{requested}' is outside allowed write directories \
             (factory/workspaces, factory/artifacts)"
            ),
        ))
    }
}

/// Returns tag strings that should be filtered out for a given scope name.
/// Mirrors the BriefingScope isolation rules without requiring the full enum.
fn scope_disallowed_tags(scope: &str) -> Vec<&'static str> {
    match scope {
        "sable" | "sable_preflight" | "scenario" => {
            vec![
                "implementation_notes",
                "mason_plan",
                "edit_rationale",
                "fix_patterns",
            ]
        }
        "mason" | "mason_preflight" => vec!["scenario_patterns", "hidden_scenario"],
        _ => vec![],
    }
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
    } else if let Some(run_id) = uri.strip_prefix("harkonnen://reasoning/") {
        let payload = crate::api::build_run_reasoning_snapshot(&state.app, run_id)
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

async fn get_prompt(state: &McpState, params: &Value) -> std::result::Result<Value, (i64, String)> {
    let name = required_string(params, "name")?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    let text = match name.as_str() {
        // ── Static templates (kept for backwards compatibility) ───────────────
        "briefing_for_spec" => {
            let spec_id = required_string(&arguments, "spec_id")?;
            format!(
                "Build a concise Harkonnen briefing for spec `{spec_id}`. \
                 Include likely risks, guardrails, and the next recommended operator action."
            )
        }
        "diagnose_run" => {
            let run_id = required_string(&arguments, "run_id")?;
            format!(
                "Diagnose Harkonnen run `{run_id}`. Summarize status, likely failure causes, \
                 operator-visible risks, and the most useful next debugging step."
            )
        }

        // ── Live-hydrated prompts (Phase 5b) ─────────────────────────────────

        // coobie/briefing: pulls live memory context for given keywords and
        // formats it as a Coobie preflight briefing the agent can act on.
        "coobie/briefing" => {
            let run_id = optional_string(&arguments, "run_id");
            let phase = optional_string(&arguments, "phase").unwrap_or_else(|| "preflight".into());
            let keywords_raw = optional_string(&arguments, "keywords").unwrap_or_default();
            let max_tokens = arguments
                .get("max_tokens")
                .and_then(Value::as_u64)
                .unwrap_or(500) as usize;
            let query_terms: Vec<String> = keywords_raw
                .split(',')
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(String::from)
                .collect();

            build_coobie_briefing_prompt(state, run_id.as_deref(), &phase, &query_terms, max_tokens)
                .await
        }

        // sable/eval-setup: run artifacts + scenario patterns, no mason content.
        "sable/eval-setup" => {
            let run_id = required_string(&arguments, "run_id")
                .map_err(|_| (-32602, "sable/eval-setup requires run_id".to_string()))?;
            build_sable_eval_prompt(state, &run_id).await
        }

        // scout/preflight: spec-scoped intent package + prior ambiguity patterns.
        "scout/preflight" => {
            let spec_id = optional_string(&arguments, "spec_id").unwrap_or_default();
            let run_id = optional_string(&arguments, "run_id");
            build_scout_preflight_prompt(state, &spec_id, run_id.as_deref()).await
        }

        // keeper/policy-check: relevant guardrails + prior decisions for an action.
        "keeper/policy-check" => {
            let action = required_string(&arguments, "action")
                .map_err(|_| (-32602, "keeper/policy-check requires action".to_string()))?;
            let context = optional_string(&arguments, "context").unwrap_or_default();
            build_keeper_policy_prompt(state, &action, &context).await
        }

        _ => return Err((-32601, format!("unknown prompt: {name}"))),
    };

    Ok(json!({
        "description": format!("Live-hydrated prompt: {name}"),
        "messages": [
            {
                "role": "user",
                "content": { "type": "text", "text": text }
            }
        ]
    }))
}

// ── Live prompt builders ──────────────────────────────────────────────────────

async fn build_coobie_briefing_prompt(
    state: &McpState,
    run_id: Option<&str>,
    phase: &str,
    query_terms: &[String],
    max_tokens: usize,
) -> String {
    let mut sections: Vec<String> = Vec::new();
    sections.push(format!(
        "# Coobie Briefing — phase: {phase}{}\n",
        run_id.map(|id| format!(" | run: {id}")).unwrap_or_default()
    ));

    // Pull from file-backed memory.
    let mut hits: Vec<String> = Vec::new();
    for term in query_terms.iter().take(8) {
        if let Ok(h) = state.app.memory_store.retrieve_context(term).await {
            for hit in h {
                if !hits.contains(&hit) {
                    hits.push(hit);
                }
            }
        }
    }

    // Pull from OB1.
    if !query_terms.is_empty() {
        if let Some(ob) = state.app.open_brain.as_ref() {
            let q = query_terms.join(" ");
            if let Ok(ob_hits) = ob.search_thoughts(&q).await {
                for hit in ob_hits {
                    if !hits.contains(&hit) {
                        hits.push(hit);
                    }
                }
            }
        }
    }

    // Budget enforcement (approximate: 1 token ≈ 4 chars).
    let char_budget = max_tokens * 4;
    let mut total_chars = 0usize;
    let mut budgeted_hits: Vec<&str> = Vec::new();
    for hit in &hits {
        if total_chars + hit.len() > char_budget {
            break;
        }
        total_chars += hit.len();
        budgeted_hits.push(hit);
    }

    if budgeted_hits.is_empty() {
        sections.push("**Memory:** No relevant context found for the given keywords.".into());
    } else {
        sections.push("## What the factory knows".into());
        for (i, hit) in budgeted_hits.iter().enumerate() {
            sections.push(format!("{}. {}", i + 1, hit.trim()));
        }
    }

    // Pull top prior causes for causal guardrails.
    let cause_rows = sqlx::query(
        "SELECT cause_id, description, COUNT(*) as occurrences, \
         AVG(CASE WHEN scenario_passed = 1 THEN 1.0 ELSE 0.0 END) as pass_rate \
         FROM causal_hypotheses \
         GROUP BY cause_id, description ORDER BY occurrences DESC LIMIT 5",
    )
    .fetch_all(&state.app.pool)
    .await
    .unwrap_or_default();
    if !cause_rows.is_empty() {
        sections.push("\n## Causal guardrails".into());
        for row in &cause_rows {
            use sqlx::Row as _;
            let cause_id: String = row.try_get("cause_id").unwrap_or_default();
            let desc: String = row
                .try_get("description")
                .unwrap_or_else(|_| cause_id.clone());
            let occ: i64 = row.try_get("occurrences").unwrap_or(0);
            let pct: f64 = row.try_get::<f64, _>("pass_rate").unwrap_or(1.0) * 100.0;
            sections.push(format!(
                "- **{desc}** ({occ} occurrences, {pct:.0}% pass rate)"
            ));
        }
    }

    sections.push(format!(
        "\n*Briefing built from {} memory hits (budget: {} tokens). \
         Apply guidance concretely — turn it into guardrails and explicit checks, not paraphrase.*",
        budgeted_hits.len(),
        max_tokens,
    ));
    sections.join("\n")
}

async fn build_sable_eval_prompt(state: &McpState, run_id: &str) -> String {
    let mut lines = vec![
        format!("# Sable Evaluation Setup — run: {run_id}"),
        String::new(),
        "## Isolation firewall".into(),
        "You are Sable. Do NOT use content tagged: \
         implementation_notes, mason_plan, edit_rationale, fix_patterns."
            .into(),
        String::new(),
    ];

    // List run artifacts.
    let run_dir = state.app.paths.workspaces.join(run_id).join("product");
    if let Ok(entries) = std::fs::read_dir(&run_dir) {
        let artifacts: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();
        if !artifacts.is_empty() {
            lines.push("## Available artifacts".into());
            for a in &artifacts {
                lines.push(format!("- {a}"));
            }
            lines.push(String::new());
        }
    }

    // Pull scenario-pattern memory (no implementation content).
    if let Some(ob) = state.app.open_brain.as_ref() {
        if let Ok(hits) = ob
            .search_thoughts("scenario failure acceptance criteria")
            .await
        {
            if !hits.is_empty() {
                lines.push("## Prior scenario patterns".into());
                for hit in hits.iter().take(4) {
                    lines.push(format!("- {}", hit.trim()));
                }
                lines.push(String::new());
            }
        }
    }

    lines.push(
        "Generate 2-4 hidden scenarios that verify the spec's acceptance criteria \
         were genuinely met — not just that the pipeline ran."
            .into(),
    );
    lines.join("\n")
}

async fn build_scout_preflight_prompt(
    state: &McpState,
    spec_id: &str,
    run_id: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "# Scout Preflight Package{}{}",
        if spec_id.is_empty() {
            String::new()
        } else {
            format!(" — spec: {spec_id}")
        },
        run_id.map(|id| format!(" | run: {id}")).unwrap_or_default(),
    )];

    // Query for spec-history and prior ambiguity patterns.
    let query = if spec_id.is_empty() {
        "spec ambiguity prior failure scout".to_string()
    } else {
        format!("{spec_id} spec ambiguity prior failure")
    };

    let mut hits: Vec<String> = Vec::new();
    if let Ok(h) = state.app.memory_store.retrieve_context(&query).await {
        hits.extend(h);
    }
    if let Some(ob) = state.app.open_brain.as_ref() {
        if let Ok(ob_hits) = ob.search_thoughts(&query).await {
            for hit in ob_hits {
                if !hits.contains(&hit) {
                    hits.push(hit);
                }
            }
        }
    }

    if !hits.is_empty() {
        lines.push(String::new());
        lines.push("## Relevant prior context".into());
        for hit in hits.iter().take(6) {
            lines.push(format!("- {}", hit.trim()));
        }
    }

    // Pull operator model commissioning brief if available.
    let brief_path = state
        .app
        .paths
        .root
        .join(".harkonnen/operator-model/commissioning-brief.json");
    if let Ok(raw) = std::fs::read_to_string(&brief_path) {
        if let Ok(brief) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(patterns) = brief.get("top_patterns").and_then(|p| p.as_array()) {
                lines.push(String::new());
                lines.push("## Operator model patterns".into());
                for p in patterns.iter().take(3) {
                    if let Some(s) = p.as_str() {
                        lines.push(format!("- {s}"));
                    }
                }
            }
        }
    }

    lines.push(String::new());
    lines.push(
        "Parse the spec, flag ambiguities, and produce a structured intent package. \
         Do not write implementation code."
            .into(),
    );
    lines.join("\n")
}

async fn build_keeper_policy_prompt(state: &McpState, action: &str, context: &str) -> String {
    let mut lines = vec![
        format!("# Keeper Policy Check — action: {action}"),
        String::new(),
        format!("**Proposed action:** {action}"),
    ];
    if !context.is_empty() {
        lines.push(format!("\n**Context:** {context}"));
    }

    // Pull relevant guardrails from memory.
    let query = format!("policy boundary guardrail {action}");
    let mut hits: Vec<String> = Vec::new();
    if let Ok(h) = state.app.memory_store.retrieve_context(&query).await {
        hits.extend(h);
    }
    if let Some(ob) = state.app.open_brain.as_ref() {
        if let Ok(ob_hits) = ob.search_thoughts(&query).await {
            for hit in ob_hits {
                if !hits.contains(&hit) {
                    hits.push(hit);
                }
            }
        }
    }

    if !hits.is_empty() {
        lines.push(String::new());
        lines.push("## Relevant policy context".into());
        for hit in hits.iter().take(4) {
            lines.push(format!("- {}", hit.trim()));
        }
    }

    lines.push(String::new());
    lines.push(
        "Issue a clear policy decision: in-bounds, out-of-bounds, or conditional with \
         stated requirements. Update the claim record if coordination is needed."
            .into(),
    );
    lines.join("\n")
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
    let reasoning = crate::api::build_run_reasoning_snapshot(&state.app, run_id).await?;
    Ok(json!({
        "run": run,
        "recent_events": events,
        "active_checkpoints": active_checkpoints,
        "reasoning": reasoning,
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
        "active_reasoning_lesson_count": memory
            .and_then(|board| board.get("active_reasoning_lessons"))
            .and_then(Value::as_array)
            .map(|items| items.len())
            .unwrap_or(0),
        "decision_count": memory
            .and_then(|board| board.get("reasoning_summary"))
            .and_then(Value::as_object)
            .and_then(|summary| summary.get("decision_count"))
            .cloned()
            .unwrap_or(Value::from(0)),
        "checkpoint_answer_count": memory
            .and_then(|board| board.get("reasoning_summary"))
            .and_then(Value::as_object)
            .and_then(|summary| summary.get("checkpoint_answer_count"))
            .cloned()
            .unwrap_or(Value::from(0)),
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
            "name": "get_run_reasoning_snapshot",
            "description": "Return live reasoning trails for a run, including recent decisions and checkpoint answers.",
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
        // ── Consolidated native tools (replaces filesystem / memory / sqlite npx servers) ──
        json!({
            "name": "read_file",
            "description": "Read a file from factory workspaces, artifacts, memory, specs, or logs.",
            "inputSchema": {
                "type": "object", "required": ["path"],
                "properties": { "path": { "type": "string", "description": "Relative or absolute path" } }
            }
        }),
        json!({
            "name": "list_directory",
            "description": "List files in a factory directory (workspaces, artifacts, memory, specs, logs).",
            "inputSchema": {
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Directory path (default: .)" } }
            }
        }),
        json!({
            "name": "write_file",
            "description": "Write content to a file inside factory/workspaces or factory/artifacts only.",
            "inputSchema": {
                "type": "object", "required": ["path", "content"],
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "create_directory",
            "description": "Create a directory inside factory/workspaces or factory/artifacts.",
            "inputSchema": {
                "type": "object", "required": ["path"],
                "properties": { "path": { "type": "string" } }
            }
        }),
        json!({
            "name": "memory_store",
            "description": "Store a new entry in Harkonnen memory (file store + OB1 capture). Use for lessons, patterns, operator facts.",
            "inputSchema": {
                "type": "object", "required": ["content"],
                "properties": {
                    "content": { "type": "string", "description": "The memory content to store" },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional classification tags" }
                }
            }
        }),
        json!({
            "name": "memory_retrieve",
            "description": "Retrieve memory hits from file store + OB1 for a query. Returns ranked results.",
            "inputSchema": {
                "type": "object", "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "default": 8, "minimum": 1, "maximum": 20 }
                }
            }
        }),
        json!({
            "name": "memory_list",
            "description": "List recent memory entries in the file store.",
            "inputSchema": {
                "type": "object",
                "properties": { "limit": { "type": "integer", "default": 20, "minimum": 1, "maximum": 100 } }
            }
        }),
        json!({
            "name": "db_list_tables",
            "description": "List tables in the Harkonnen SQLite state database.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "db_query",
            "description": "Execute a read-only SELECT query against the Harkonnen SQLite state database.",
            "inputSchema": {
                "type": "object", "required": ["sql"],
                "properties": { "sql": { "type": "string", "description": "A SELECT or WITH query" } }
            }
        }),
        json!({
            "name": "memory_pull",
            "description": "On-demand context retrieval mid-task. Searches file-backed memory and OB1 for the query, applies scope isolation, and returns the top hits within the token budget.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string", "description": "The retrieval query" },
                    "scope": {
                        "type": "string",
                        "description": "Isolation scope: general | sable | mason | scout | keeper",
                        "default": "general"
                    },
                    "max_tokens": {
                        "type": "integer",
                        "description": "Token budget for returned hits",
                        "default": 500,
                        "minimum": 50,
                        "maximum": 4000
                    }
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
            "description": "Return a compact progress snapshot for a run, including recent events, active checkpoints, and live reasoning counts.",
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
            "description": "Compact run progress snapshot with recent events, active checkpoints, and live reasoning trails.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://boards/{run_id}",
            "name": "Run Boards",
            "description": "Mission, Action, Evidence, and Memory board snapshot for a run.",
            "mimeType": "application/json"
        }),
        json!({
            "uriTemplate": "harkonnen://reasoning/{run_id}",
            "name": "Run Reasoning",
            "description": "Live reasoning trails for a run, including decisions and checkpoint answers.",
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
        // ── Static templates (backwards compat) ──────────────────────────────
        json!({
            "name": "briefing_for_spec",
            "description": "Static prompt template for a Coobie-style briefing."
        }),
        json!({
            "name": "diagnose_run",
            "description": "Static prompt template for diagnosing a completed or failed run."
        }),
        // ── Live-hydrated prompts (Phase 5b) ─────────────────────────────────
        json!({
            "name": "coobie/briefing",
            "description": "Live Coobie preflight briefing — pulls real memory hits, OB1 recall, and prior causes for the given keywords and phase.",
            "arguments": [
                { "name": "keywords", "description": "Comma-separated search keywords", "required": false },
                { "name": "phase", "description": "Factory phase (preflight, scout, mason, sable, ...)", "required": false },
                { "name": "run_id", "description": "Optional run ID for run-scoped context", "required": false },
                { "name": "max_tokens", "description": "Token budget for memory hits (default: 500)", "required": false }
            ]
        }),
        json!({
            "name": "sable/eval-setup",
            "description": "Live Sable evaluation context — run artifacts, scenario patterns, isolation firewall enforced.",
            "arguments": [
                { "name": "run_id", "description": "Run ID to evaluate", "required": true }
            ]
        }),
        json!({
            "name": "scout/preflight",
            "description": "Live Scout preflight package — spec history, prior ambiguities, operator model posture.",
            "arguments": [
                { "name": "spec_id", "description": "Spec identifier", "required": false },
                { "name": "run_id", "description": "Optional run ID for run-scoped context", "required": false }
            ]
        }),
        json!({
            "name": "keeper/policy-check",
            "description": "Live Keeper policy context — relevant guardrails and prior decisions for a proposed action.",
            "arguments": [
                { "name": "action", "description": "The action under review", "required": true },
                { "name": "context", "description": "Additional context for the policy check", "required": false }
            ]
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

// ─────────────────────────────────────────────────────────────────────────────
// MCP SERVER END-TO-END TESTS
// Tests exercise handle_rpc_body directly using an in-process McpState backed
// by a temporary directory and an isolated SQLite database.
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod mcp_e2e {
    use super::*;
    use serde_json::json;
    use std::{sync::Arc, time::Instant};
    use tokio::sync::RwLock;

    // ── Test helpers ──────────────────────────────────────────────────────────

    /// Build a minimal McpState backed by a temp directory.
    /// Returns `(state, _dir)` — keep `_dir` alive for the test duration.
    async fn build_test_state() -> (McpState, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path().to_path_buf();
        let factory = root.join("factory");
        for sub in [
            "specs",
            "scenarios",
            "artifacts",
            "logs",
            "workspaces",
            "memory",
        ] {
            tokio::fs::create_dir_all(factory.join(sub)).await.unwrap();
        }
        tokio::fs::create_dir_all(root.join("products"))
            .await
            .unwrap();

        let setup = crate::setup::SetupConfig::discover(&root).unwrap();
        let paths = crate::config::Paths {
            root: root.clone(),
            factory: factory.clone(),
            specs: factory.join("specs"),
            scenarios: factory.join("scenarios"),
            artifacts: factory.join("artifacts"),
            logs: factory.join("logs"),
            workspaces: factory.join("workspaces"),
            memory: factory.join("memory"),
            db_file: factory.join("state.db"),
            products: root.join("products"),
            setup: setup.clone(),
        };

        let pool = crate::db::init_db(&paths).await.unwrap();
        let memory_store = crate::memory::MemoryStore::new(paths.memory.clone());
        let coobie = crate::coobie::SqliteCoobie::new(pool.clone());
        let (event_tx, _) = tokio::sync::broadcast::channel(512);
        let bus = Arc::new(crate::chat::LocalJsonlPackChatBus::new(
            paths.logs.join("packchat-bus.jsonl"),
            "test",
        ));
        let chat = crate::chat::ChatStore::with_bus(pool.clone(), bus);
        let operator_models = crate::operator_model::OperatorModelStore::new(pool.clone());
        let dispatcher =
            crate::subagent::SubAgentDispatcher::new(Default::default(), setup.clone());

        let app = crate::orchestrator::AppContext {
            paths,
            pool,
            memory_store,
            blackboard: Arc::new(RwLock::new(crate::models::BlackboardState::default())),
            coobie,
            embedding_store: None,
            event_tx,
            chat,
            operator_models,
            started_at: Instant::now(),
            calvin: None,
            open_brain: None,
            dispatcher,
        };

        let state = McpState {
            app,
            started_at: Instant::now(),
        };
        (state, tmp)
    }

    /// Call `handle_rpc_body` and return the inner `result` field.
    /// Panics if the RPC response carries an error.
    async fn rpc(state: &McpState, body: serde_json::Value) -> serde_json::Value {
        match handle_rpc_body(state, body)
            .await
            .expect("handle_rpc_body returned Err")
        {
            Some(resp) => {
                assert!(
                    resp.get("error").is_none(),
                    "unexpected RPC error: {}",
                    resp
                );
                resp["result"].clone()
            }
            None => serde_json::Value::Null,
        }
    }

    /// Call `handle_rpc_body` and expect an error.  Returns `(code, message)`.
    async fn rpc_err(state: &McpState, body: serde_json::Value) -> (i64, String) {
        match handle_rpc_body(state, body).await {
            Err(err) => {
                let code = err["error"]["code"].as_i64().unwrap_or(0);
                let message = err["error"]["message"].as_str().unwrap_or("").to_string();
                (code, message)
            }
            Ok(Some(resp)) if resp.get("error").is_some() => {
                let code = resp["error"]["code"].as_i64().unwrap_or(0);
                let message = resp["error"]["message"].as_str().unwrap_or("").to_string();
                (code, message)
            }
            Ok(other) => panic!("expected error, got: {other:?}"),
        }
    }

    /// Build a tools/call envelope.
    fn call(id: u64, tool: &str, args: serde_json::Value) -> serde_json::Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": tool, "arguments": args }
        })
    }

    /// Parse the text content of a tool result as JSON.
    fn parse_tool_json(result: &serde_json::Value) -> serde_json::Value {
        let text = result["content"][0]["text"]
            .as_str()
            .expect("expected text content");
        serde_json::from_str(text).expect("expected JSON in tool result text")
    }

    /// Get the plain-text content of a tool result.
    fn tool_text(result: &serde_json::Value) -> &str {
        result["content"][0]["text"]
            .as_str()
            .expect("expected text content")
    }

    /// Write a minimal valid spec YAML and return the absolute path.
    async fn write_test_spec(paths: &crate::config::Paths, name: &str) -> String {
        let spec_path = paths.specs.join(format!("{name}.yaml"));
        let yaml = format!(
            "id: {name}\ntitle: {name} Test Spec\npurpose: MCP E2E test\n\
             scope: [test]\nconstraints: []\ninputs: []\noutputs: []\n\
             acceptance_criteria: [test passes]\nforbidden_behaviors: []\n\
             rollback_requirements: []\ndependencies: []\n\
             performance_expectations: []\nsecurity_expectations: []\n"
        );
        tokio::fs::write(&spec_path, yaml).await.unwrap();
        spec_path.to_string_lossy().into_owned()
    }

    /// Create a product directory and return the absolute path.
    async fn make_product_dir(paths: &crate::config::Paths, name: &str) -> String {
        let product_dir = paths.products.join(name);
        tokio::fs::create_dir_all(&product_dir).await.unwrap();
        product_dir.to_string_lossy().into_owned()
    }

    // ── Protocol ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "initialize",
                "params": { "protocolVersion": "2025-11-25" }
            }),
        )
        .await;
        assert_eq!(result["serverInfo"]["name"], "harkonnen-labs");
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["protocolVersion"], "2025-11-25");
    }

    #[tokio::test]
    async fn ping_returns_ok() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "ping", "params": {} }),
        )
        .await;
        assert_eq!(result["ok"], true);
    }

    #[tokio::test]
    async fn notifications_initialized_returns_no_content() {
        let (state, _dir) = build_test_state().await;
        let result = handle_rpc_body(
            &state,
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized", "params": {} }),
        )
        .await
        .unwrap();
        assert!(
            result.is_none(),
            "notifications/initialized must return None"
        );
    }

    #[tokio::test]
    async fn tools_list_includes_all_expected_tools() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/list", "params": {} }),
        )
        .await;
        let names: Vec<String> = result["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();

        for expected in [
            "list_runs",
            "get_run",
            "get_run_report",
            "start_run",
            "queue_run",
            "watch_run",
            "list_run_decisions",
            "get_run_reasoning_snapshot",
            "get_run_board_snapshot",
            "list_chat_threads",
            "open_chat_thread",
            "get_chat_thread",
            "list_chat_messages",
            "post_chat_message",
            "list_run_checkpoints",
            "reply_to_checkpoint",
            "unblock_agent",
            "read_file",
            "write_file",
            "list_directory",
            "create_directory",
            "memory_store",
            "memory_retrieve",
            "memory_list",
            "memory_pull",
            "db_list_tables",
            "db_query",
            "list_benchmark_suites",
            "run_benchmarks",
            "list_benchmark_reports",
            "get_benchmark_report",
        ] {
            assert!(
                names.contains(&expected.to_string()),
                "missing tool: {expected}"
            );
        }
    }

    #[tokio::test]
    async fn resources_list_includes_core_uris() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({ "jsonrpc": "2.0", "id": 4, "method": "resources/list", "params": {} }),
        )
        .await;
        let resources = result["resources"].as_array().unwrap();
        let uris: Vec<&str> = resources
            .iter()
            .filter_map(|r| r["uri"].as_str().or_else(|| r["uriTemplate"].as_str()))
            .collect();
        assert!(
            uris.contains(&"harkonnen://runs"),
            "missing harkonnen://runs"
        );
        assert!(
            uris.contains(&"harkonnen://chat/threads"),
            "missing harkonnen://chat/threads"
        );
        assert!(
            uris.contains(&"harkonnen://benchmarks/suites"),
            "missing harkonnen://benchmarks/suites"
        );
    }

    #[tokio::test]
    async fn prompts_list_includes_live_hydrated_prompts() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({ "jsonrpc": "2.0", "id": 5, "method": "prompts/list", "params": {} }),
        )
        .await;
        let names: Vec<&str> = result["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        for expected in [
            "coobie/briefing",
            "sable/eval-setup",
            "scout/preflight",
            "keeper/policy-check",
        ] {
            assert!(names.contains(&expected), "missing prompt: {expected}");
        }
    }

    #[tokio::test]
    async fn batch_request_is_rejected() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(
            &state,
            json!([{ "jsonrpc": "2.0", "id": 1, "method": "ping", "params": {} }]),
        )
        .await;
        assert_eq!(code, -32600);
        assert!(
            msg.contains("batch"),
            "expected 'batch' in error message, got: {msg}"
        );
    }

    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let (state, _dir) = build_test_state().await;
        let (code, _msg) = rpc_err(
            &state,
            json!({ "jsonrpc": "2.0", "id": 6, "method": "no_such_method", "params": {} }),
        )
        .await;
        assert_eq!(code, -32601);
    }

    // ── DB tools ──────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn db_list_tables_returns_core_schema_tables() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(&state, call(10, "db_list_tables", json!({}))).await;
        let text = tool_text(&result);
        assert!(text.contains("runs"), "'runs' table not found: {text}");
        assert!(
            text.contains("run_events"),
            "'run_events' table not found: {text}"
        );
        assert!(
            text.contains("chat_threads"),
            "'chat_threads' table not found: {text}"
        );
    }

    #[tokio::test]
    async fn db_query_select_on_empty_runs_returns_no_rows() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            call(
                11,
                "db_query",
                json!({ "sql": "SELECT run_id, status FROM runs LIMIT 10" }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(
            text.contains("(no rows)"),
            "expected '(no rows)' on empty DB, got: {text}"
        );
    }

    #[tokio::test]
    async fn db_query_rejects_non_select_statements() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(
            &state,
            call(
                12,
                "db_query",
                json!({ "sql": "INSERT INTO runs VALUES ('x','y','z','q','n','n')" }),
            ),
        )
        .await;
        assert_eq!(code, -32602);
        assert!(
            msg.to_lowercase().contains("select"),
            "expected SELECT-only message, got: {msg}"
        );
    }

    #[tokio::test]
    async fn db_query_with_prefix_returns_rows() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            call(
                13,
                "db_query",
                json!({ "sql": "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name LIMIT 5" }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(
            text.contains("row(s)"),
            "expected 'row(s)' in output, got: {text}"
        );
    }

    // ── Memory tools ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn memory_store_confirms_write() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            call(
                20,
                "memory_store",
                json!({
                    "content": "Thin evidence requires explicit preflight checks.",
                    "tags": ["coobie", "preflight"]
                }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(
            text.contains("stored"),
            "expected store confirmation, got: {text}"
        );
    }

    #[tokio::test]
    async fn memory_list_shows_entry_after_store() {
        let (state, _dir) = build_test_state().await;
        rpc(
            &state,
            call(
                21,
                "memory_store",
                json!({ "content": "Mason succeeded on auth module." }),
            ),
        )
        .await;

        let result = rpc(&state, call(22, "memory_list", json!({ "limit": 10 }))).await;
        let text = tool_text(&result);
        assert!(
            !text.contains("Memory is empty."),
            "memory_list should show stored entry, got: {text}"
        );
    }

    #[tokio::test]
    async fn memory_retrieve_returns_non_empty_text() {
        let (state, _dir) = build_test_state().await;
        rpc(
            &state,
            call(
                23,
                "memory_store",
                json!({ "content": "Scout flagged ambiguity in the auth scope definition." }),
            ),
        )
        .await;

        let result = rpc(
            &state,
            call(
                24,
                "memory_retrieve",
                json!({ "query": "scout auth scope ambiguity", "limit": 5 }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(
            !text.is_empty(),
            "memory_retrieve must return non-empty text"
        );
    }

    #[tokio::test]
    async fn memory_pull_returns_structured_header() {
        let (state, _dir) = build_test_state().await;
        rpc(
            &state,
            call(
                25,
                "memory_store",
                json!({ "content": "Coobie preflight: check for stale runs before starting." }),
            ),
        )
        .await;

        let result = rpc(
            &state,
            call(
                26,
                "memory_pull",
                json!({ "query": "coobie preflight stale runs", "scope": "general", "max_tokens": 300 }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(!text.is_empty(), "memory_pull must return non-empty text");
    }

    #[tokio::test]
    async fn memory_pull_sable_scope_filters_implementation_notes() {
        let (state, _dir) = build_test_state().await;
        // Store content that contains the disallowed tag text
        rpc(
            &state,
            call(
                27,
                "memory_store",
                json!({ "content": "[implementation_notes] Mason used trait objects for the auth abstraction." }),
            ),
        )
        .await;

        let result = rpc(
            &state,
            call(
                28,
                "memory_pull",
                json!({
                    "query": "implementation notes auth mason",
                    "scope": "sable",
                    "max_tokens": 400
                }),
            ),
        )
        .await;
        let text = tool_text(&result);
        // Under sable scope, hits containing "implementation_notes" are filtered out
        assert!(
            !text.contains("Mason used trait objects"),
            "sable scope must suppress implementation_notes content"
        );
    }

    // ── File tools ────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn write_and_read_file_round_trip() {
        let (state, _dir) = build_test_state().await;
        let ws_run = state.app.paths.workspaces.join("rw-test");
        tokio::fs::create_dir_all(&ws_run).await.unwrap();
        let ws_path = "factory/workspaces/rw-test/output.txt";

        let write_result = rpc(
            &state,
            call(
                30,
                "write_file",
                json!({ "path": ws_path, "content": "Hello from write_file." }),
            ),
        )
        .await;
        assert!(
            tool_text(&write_result).contains("wrote"),
            "expected write confirmation"
        );

        let read_result = rpc(&state, call(31, "read_file", json!({ "path": ws_path }))).await;
        assert_eq!(tool_text(&read_result), "Hello from write_file.");
    }

    #[tokio::test]
    async fn read_file_outside_allowed_paths_rejected() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(
            &state,
            call(32, "read_file", json!({ "path": "/etc/passwd" })),
        )
        .await;
        assert_eq!(code, -32602);
        assert!(
            msg.contains("outside"),
            "expected 'outside allowed' in error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn write_file_outside_workspaces_rejected() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(
            &state,
            call(
                33,
                "write_file",
                json!({ "path": "factory/memory/sneaky.md", "content": "should not write" }),
            ),
        )
        .await;
        assert_eq!(code, -32602);
        assert!(
            msg.contains("outside"),
            "expected 'outside allowed write' in error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn list_directory_shows_written_file() {
        let (state, _dir) = build_test_state().await;
        let ws_dir = state.app.paths.workspaces.join("dir-test");
        tokio::fs::create_dir_all(&ws_dir).await.unwrap();
        tokio::fs::write(ws_dir.join("result.json"), b"{}")
            .await
            .unwrap();

        let result = rpc(
            &state,
            call(
                34,
                "list_directory",
                json!({ "path": "factory/workspaces/dir-test" }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(
            text.contains("result.json"),
            "expected result.json in listing, got: {text}"
        );
    }

    #[tokio::test]
    async fn create_directory_in_workspace() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            call(
                35,
                "create_directory",
                json!({ "path": "factory/workspaces/new-dir/sub" }),
            ),
        )
        .await;
        let text = tool_text(&result);
        assert!(
            text.contains("created"),
            "expected 'created' confirmation, got: {text}"
        );
        assert!(
            state.app.paths.workspaces.join("new-dir/sub").exists(),
            "directory must exist after create_directory"
        );
    }

    // ── Run lifecycle via MCP ─────────────────────────────────────────────────

    #[tokio::test]
    async fn list_runs_empty_on_fresh_database() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(&state, call(40, "list_runs", json!({ "limit": 20 }))).await;
        let runs = parse_tool_json(&result);
        assert!(
            runs.as_array().map(|a| a.is_empty()).unwrap_or(false),
            "expected empty run list, got: {runs}"
        );
    }

    #[tokio::test]
    async fn queue_run_creates_queued_record() {
        let (state, _dir) = build_test_state().await;
        let spec = write_test_spec(&state.app.paths, "queue-run-spec").await;
        let product = make_product_dir(&state.app.paths, "queue-run-product").await;

        let result = rpc(
            &state,
            call(
                41,
                "queue_run",
                json!({ "spec": spec, "product_path": product, "run_hidden_scenarios": false }),
            ),
        )
        .await;
        let info = parse_tool_json(&result);
        let run_id = info["run_id"]
            .as_str()
            .expect("queue_run must return run_id");
        // Status is initially "queued" but the background task may complete (and fail
        // without a real LLM) before queue_run returns its record in test contexts.
        let status = info["status"].as_str().unwrap_or("");
        assert!(
            ["queued", "running", "failed", "completed"].contains(&status),
            "run must have a valid status, got: {status}"
        );
        assert!(info["spec_id"].as_str().is_some(), "run must carry spec_id");
        assert!(!run_id.is_empty(), "run_id must not be empty");
    }

    #[tokio::test]
    async fn get_run_returns_matching_queued_record() {
        let (state, _dir) = build_test_state().await;
        let spec = write_test_spec(&state.app.paths, "get-run-spec").await;
        let product = make_product_dir(&state.app.paths, "get-run-product").await;

        let queue_result = rpc(
            &state,
            call(
                42,
                "queue_run",
                json!({ "spec": spec, "product_path": product, "run_hidden_scenarios": false }),
            ),
        )
        .await;
        let run_id = parse_tool_json(&queue_result)["run_id"]
            .as_str()
            .unwrap()
            .to_string();

        let get_result = rpc(&state, call(43, "get_run", json!({ "run_id": &run_id }))).await;
        let run = parse_tool_json(&get_result);
        assert_eq!(run["run_id"], run_id);
        // Status may have transitioned from "queued" to "failed" by the time we
        // call get_run (background execution fails fast without a real LLM).
        let status = run["status"].as_str().unwrap_or("");
        assert!(
            ["queued", "running", "failed", "completed"].contains(&status),
            "run status must be a valid value, got: {status}"
        );
    }

    #[tokio::test]
    async fn list_runs_shows_queued_run() {
        let (state, _dir) = build_test_state().await;
        let spec = write_test_spec(&state.app.paths, "list-runs-spec").await;
        let product = make_product_dir(&state.app.paths, "list-runs-product").await;

        let queue_result = rpc(
            &state,
            call(
                44,
                "queue_run",
                json!({ "spec": spec, "product_path": product, "run_hidden_scenarios": false }),
            ),
        )
        .await;
        let run_id = parse_tool_json(&queue_result)["run_id"]
            .as_str()
            .unwrap()
            .to_string();

        let list_result = rpc(&state, call(45, "list_runs", json!({ "limit": 10 }))).await;
        let runs = parse_tool_json(&list_result);
        let arr = runs.as_array().unwrap();
        assert!(!arr.is_empty(), "list_runs must show the queued run");
        assert!(
            arr.iter().any(|r| r["run_id"] == run_id),
            "queued run_id must appear in list"
        );
    }

    #[tokio::test]
    async fn watch_run_snapshot_contains_required_fields() {
        let (state, _dir) = build_test_state().await;
        let spec = write_test_spec(&state.app.paths, "watch-run-spec").await;
        let product = make_product_dir(&state.app.paths, "watch-run-product").await;

        let queue_result = rpc(
            &state,
            call(
                46,
                "queue_run",
                json!({ "spec": spec, "product_path": product, "run_hidden_scenarios": false }),
            ),
        )
        .await;
        let run_id = parse_tool_json(&queue_result)["run_id"]
            .as_str()
            .unwrap()
            .to_string();

        let watch_result = rpc(
            &state,
            call(
                47,
                "watch_run",
                json!({ "run_id": &run_id, "event_limit": 5 }),
            ),
        )
        .await;
        let payload = parse_tool_json(&watch_result);
        assert_eq!(
            payload["run"]["run_id"], run_id,
            "watch_run must include run record"
        );
        assert!(
            payload["recent_events"].is_array(),
            "watch_run must include recent_events"
        );
        assert!(
            payload["active_checkpoints"].is_array(),
            "watch_run must include active_checkpoints"
        );
    }

    #[tokio::test]
    async fn list_run_checkpoints_returns_current_array_for_queued_run() {
        let (state, _dir) = build_test_state().await;
        let spec = write_test_spec(&state.app.paths, "checkpoint-spec").await;
        let product = make_product_dir(&state.app.paths, "checkpoint-product").await;

        let queue_result = rpc(
            &state,
            call(
                48,
                "queue_run",
                json!({ "spec": spec, "product_path": product, "run_hidden_scenarios": false }),
            ),
        )
        .await;
        let run_id = parse_tool_json(&queue_result)["run_id"]
            .as_str()
            .unwrap()
            .to_string();

        let cp_result = rpc(
            &state,
            call(49, "list_run_checkpoints", json!({ "run_id": &run_id })),
        )
        .await;
        let checkpoints = parse_tool_json(&cp_result);
        let checkpoints = checkpoints
            .as_array()
            .expect("list_run_checkpoints must return an array");
        assert!(
            checkpoints
                .iter()
                .all(|checkpoint| checkpoint["run_id"] == run_id),
            "list_run_checkpoints must only return checkpoints for the requested run"
        );
    }

    #[tokio::test]
    async fn get_run_not_found_returns_error() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(
            &state,
            call(50, "get_run", json!({ "run_id": "nonexistent-xyz" })),
        )
        .await;
        assert_eq!(code, -32004, "missing run must return -32004");
        assert!(
            msg.contains("nonexistent-xyz"),
            "error must name the missing run_id"
        );
    }

    // ── Chat / PackChat tools ─────────────────────────────────────────────────

    #[tokio::test]
    async fn open_and_get_chat_thread_round_trip() {
        let (state, _dir) = build_test_state().await;

        let open_result = rpc(
            &state,
            call(
                60,
                "open_chat_thread",
                json!({ "title": "E2E Thread", "thread_kind": "general" }),
            ),
        )
        .await;
        let thread = parse_tool_json(&open_result);
        let thread_id = thread["thread_id"]
            .as_str()
            .expect("open_chat_thread must return thread_id")
            .to_string();

        let get_result = rpc(
            &state,
            call(61, "get_chat_thread", json!({ "thread_id": &thread_id })),
        )
        .await;
        let fetched = parse_tool_json(&get_result);
        assert_eq!(
            fetched["thread_id"], thread_id,
            "get_chat_thread must return matching thread_id"
        );
    }

    #[tokio::test]
    async fn list_chat_threads_shows_opened_thread() {
        let (state, _dir) = build_test_state().await;

        rpc(
            &state,
            call(
                62,
                "open_chat_thread",
                json!({ "title": "List Test Thread", "thread_kind": "general" }),
            ),
        )
        .await;

        let list_result = rpc(
            &state,
            call(
                63,
                "list_chat_threads",
                json!({ "thread_kind": "general", "limit": 10 }),
            ),
        )
        .await;
        let threads = parse_tool_json(&list_result);
        assert!(
            threads.as_array().map(|a| !a.is_empty()).unwrap_or(false),
            "list_chat_threads must show the opened thread"
        );
    }

    #[tokio::test]
    async fn list_chat_messages_empty_on_new_thread() {
        let (state, _dir) = build_test_state().await;

        let open_result = rpc(
            &state,
            call(
                64,
                "open_chat_thread",
                json!({ "title": "Empty Thread", "thread_kind": "general" }),
            ),
        )
        .await;
        let thread_id = parse_tool_json(&open_result)["thread_id"]
            .as_str()
            .unwrap()
            .to_string();

        let msg_result = rpc(
            &state,
            call(65, "list_chat_messages", json!({ "thread_id": &thread_id })),
        )
        .await;
        let messages = parse_tool_json(&msg_result);
        assert!(
            messages.as_array().map(|a| a.is_empty()).unwrap_or(true),
            "new thread must have no messages"
        );
    }

    #[tokio::test]
    async fn get_chat_thread_not_found_returns_error() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(
            &state,
            call(
                66,
                "get_chat_thread",
                json!({ "thread_id": "nonexistent-thread-abc" }),
            ),
        )
        .await;
        assert_eq!(code, -32004);
        assert!(
            msg.contains("nonexistent-thread-abc"),
            "error must name the missing thread_id"
        );
    }

    // ── Error handling ────────────────────────────────────────────────────────

    #[tokio::test]
    async fn missing_required_param_returns_invalid_params() {
        let (state, _dir) = build_test_state().await;
        // get_run requires run_id
        let (code, msg) = rpc_err(&state, call(70, "get_run", json!({}))).await;
        assert_eq!(code, -32602, "missing required param must return -32602");
        assert!(
            msg.contains("run_id"),
            "error must name the missing parameter"
        );
    }

    #[tokio::test]
    async fn unknown_tool_name_returns_method_not_found() {
        let (state, _dir) = build_test_state().await;
        let (code, msg) = rpc_err(&state, call(71, "no_such_tool_ever_xyz", json!({}))).await;
        assert_eq!(code, -32601);
        assert!(
            msg.contains("no_such_tool_ever_xyz"),
            "error must name the unknown tool"
        );
    }

    // ── Live-hydrated prompts ─────────────────────────────────────────────────

    #[tokio::test]
    async fn coobie_briefing_prompt_returns_messages() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 80,
                "method": "prompts/get",
                "params": {
                    "name": "coobie/briefing",
                    "arguments": {
                        "keywords": "scout,spec,ambiguity",
                        "phase": "preflight"
                    }
                }
            }),
        )
        .await;
        let messages = result["messages"]
            .as_array()
            .expect("must have messages array");
        assert!(!messages.is_empty(), "coobie/briefing must return messages");
        let text = messages[0]["content"]["text"].as_str().unwrap_or("");
        assert!(
            text.contains("Coobie"),
            "coobie/briefing text must reference Coobie, got: {text}"
        );
    }

    #[tokio::test]
    async fn keeper_policy_check_prompt_echoes_action() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 81,
                "method": "prompts/get",
                "params": {
                    "name": "keeper/policy-check",
                    "arguments": {
                        "action": "write to factory/scenarios",
                        "context": "Sable wants to add a hidden scenario file"
                    }
                }
            }),
        )
        .await;
        let messages = result["messages"].as_array().unwrap();
        assert!(!messages.is_empty());
        let text = messages[0]["content"]["text"].as_str().unwrap_or("");
        assert!(
            text.contains("write to factory/scenarios"),
            "keeper prompt must echo the action, got: {text}"
        );
    }

    #[tokio::test]
    async fn scout_preflight_prompt_returns_messages() {
        let (state, _dir) = build_test_state().await;
        let result = rpc(
            &state,
            json!({
                "jsonrpc": "2.0", "id": 82,
                "method": "prompts/get",
                "params": {
                    "name": "scout/preflight",
                    "arguments": { "spec_id": "auth-spec-001" }
                }
            }),
        )
        .await;
        let messages = result["messages"].as_array().unwrap();
        assert!(!messages.is_empty(), "scout/preflight must return messages");
    }
}
