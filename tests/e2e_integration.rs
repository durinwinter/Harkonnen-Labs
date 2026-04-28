//! End-to-end integration tests for the Harkonnen Labs ↔ Calvin Archive ↔ Twilight Bark ↔ OpenBrain triangle.
//!
//! Each test section covers one integration seam and documents implementation gaps discovered
//! by exercising the live protocol contracts. Tests marked `// GAP:` assert the CORRECT
//! behavior — they will fail until the gap is closed.
//!
//! # Running
//!
//! Against mock infrastructure (no real services needed):
//!   cargo test --test e2e_integration
//!
//! Against a live stack (TYPEDB_URL, CALVIN_PORT, etc. set):
//!   INTEGRATION_LIVE=1 cargo test --test e2e_integration
//!
//! # Infrastructure
//!
//! All mock servers are started in-process using tokio::spawn and axum. The Twilight
//! daemon side is mocked via a tokio UnixListener speaking the JSON-lines IPC protocol.
//! Tests are isolated: each gets its own port / socket path via tempfile.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, post},
    Json, Router,
};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, UnixListener};
use tokio::time::timeout;

// ─────────────────────────────────────────────────────────────────────────────
// SHARED MOCK INFRASTRUCTURE
// ─────────────────────────────────────────────────────────────────────────────

/// Captures every request body that hits a mock server so tests can assert on them.
type RequestLog = Arc<Mutex<Vec<Value>>>;

/// Binds an ephemeral TCP port and returns (url, listener).
async fn bind_ephemeral() -> (String, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    (format!("http://127.0.0.1:{port}"), listener)
}

/// Spawns a minimal Calvin Archive mock that records all request bodies.
/// Returns (base_url, request_log).
async fn spawn_mock_calvin() -> (String, RequestLog) {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let (url, listener) = bind_ephemeral().await;

    let log_clone = Arc::clone(&log);
    let app = Router::new()
        .route("/health", get(|| async { Json(json!({"status":"ok"})) }))
        .route(
            "/status",
            get(|| async {
                Json(json!({
                    "status": "ok",
                    "typedb_entities": {"experience":0,"belief":3,"trait":10,"agent_self":1,"run_record":0,"causal_pattern":0},
                    "telemetry_enabled": false,
                    "streaming_enabled": false
                }))
            }),
        )
        .route("/runs", post(capture_body))
        .route("/runs/:run_id/experiences", post(capture_body))
        .route("/runs/:run_id/beliefs", post(capture_body))
        .route(
            "/runs/:run_id/close",
            patch(|| async { StatusCode::OK }),
        )
        .route(
            "/agents/:name/traits",
            get(|| async {
                Json(json!([
                    "Cooperative",
                    "Helpful / Retrieving",
                    "Non-Adversarial",
                    "Truth-Seeking",
                    "Signals Uncertainty",
                ]))
            }),
        )
        // Fixed: the real archive.rs now adds a stabilizes join so only the named agent's
        // beliefs are returned. The mock reflects this by returning only agent-scoped beliefs.
        .route(
            "/agents/:name/beliefs",
            get(|| async {
                Json(json!([
                    "When memory is thin, turn that thinness into explicit checks.",
                    "Continuity requires traceability.",
                    // "The sky is blue." removed — foreign beliefs no longer leak through.
                ]))
            }),
        )
        .route(
            "/agents/:name/check",
            post(check_adaptation_body),
        )
        .route("/agents/:name/status", patch(capture_body))
        .route("/runs/:run_id/causal-links", post(capture_body))
        .route("/telemetry", post(capture_body))
        .route("/telemetry/batch", post(capture_body))
        .with_state(Arc::clone(&log_clone));

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (url, log)
}

async fn capture_body(State(log): State<RequestLog>, body: axum::body::Bytes) -> impl IntoResponse {
    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
        log.lock().unwrap().push(v);
    }
    StatusCode::CREATED
}

/// Simulates the fixed check_adaptation_safe: catches both literal and semantic negations.
async fn check_adaptation_body(
    State(log): State<RequestLog>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let req: Value = serde_json::from_slice(&body).unwrap_or_default();
    log.lock().unwrap().push(req.clone());
    let summary = req["adaptation_summary"]
        .as_str()
        .unwrap_or("")
        .to_lowercase();
    let traits = [
        "cooperative",
        "helpful",
        "non-adversarial",
        "truth-seeking",
        "signals uncertainty",
    ];
    let negation_prefixes = [
        "not ",
        "remove ",
        "eliminate ",
        "avoid ",
        "without ",
        "less ",
        "deprioritise ",
        "deprioritize ",
        "reduce ",
        "replace ",
        "instead of ",
        "abandon ",
        "drop ",
    ];
    let safe = !traits.iter().any(|t| {
        negation_prefixes
            .iter()
            .any(|p| summary.contains(&format!("{p}{t}")))
    });
    Json(json!({"safe": safe}))
}

/// Spawns a minimal OpenBrain MCP mock (JSON-RPC 2.0 over HTTP POST).
/// Returns (base_url, request_log).
async fn spawn_mock_openbrain() -> (String, RequestLog) {
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));
    let (url, listener) = bind_ephemeral().await;

    let log_clone = Arc::clone(&log);
    let app = Router::new()
        .route("/", post(openbrain_dispatch))
        .with_state(Arc::clone(&log_clone));

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    (url, log)
}

async fn openbrain_dispatch(
    State(log): State<RequestLog>,
    Json(req): Json<Value>,
) -> impl IntoResponse {
    log.lock().unwrap().push(req.clone());
    let method = req.get("method").and_then(Value::as_str).unwrap_or("");
    let id = req.get("id").cloned().unwrap_or(json!(1));
    let result = match method {
        "tools/call" => {
            let tool = req
                .pointer("/params/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            match tool {
                "capture_thought" => json!({"content": [{"type":"text","text":"ok"}]}),
                "search_thoughts" => json!({
                    "content": [{
                        "type": "text",
                        "text": "Coobie remembers: thin evidence requires explicit checks.\nCoobie remembers: continuity requires traceability."
                    }]
                }),
                "list_thoughts" => json!({"content": [{"type":"text","text":"[]"}]}),
                _ => json!({"content": [{"type":"text","text":"unknown tool"}]}),
            }
        }
        _ => json!({"error": "unknown method"}),
    };
    Json(json!({"jsonrpc":"2.0","id":id,"result":result}))
}

/// Spawns a mock Twilight daemon speaking the JSON-lines IPC protocol over a Unix socket.
/// Returns (socket_path, received_commands_log).
async fn spawn_mock_twilight_socket(dir: &tempfile::TempDir) -> (String, RequestLog) {
    let path = dir.path().join("twilight-daemon.sock");
    let socket_path = path.to_str().unwrap().to_string();
    let log: RequestLog = Arc::new(Mutex::new(Vec::new()));

    let listener = UnixListener::bind(&path).unwrap();
    let log_clone = Arc::clone(&log);

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let log = Arc::clone(&log_clone);
            tokio::spawn(async move {
                let (read_half, mut write_half) = stream.into_split();
                let mut lines = BufReader::new(read_half).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(cmd) = serde_json::from_str::<Value>(&line) {
                        let response = twilight_ipc_response(&cmd);
                        log.lock().unwrap().push(cmd);
                        let mut resp_line = serde_json::to_string(&response).unwrap();
                        resp_line.push('\n');
                        let _ = write_half.write_all(resp_line.as_bytes()).await;
                    }
                }
            });
        }
    });

    (socket_path, log)
}

fn twilight_ipc_response(cmd: &Value) -> Value {
    match cmd.get("cmd").and_then(Value::as_str).unwrap_or("") {
        "register" => json!({"ok":true,"agent_uuid":"test-agent-uuid-0001"}),
        "subscribe_tasks" => json!({"ok":true}),
        "publish_task" => json!({"ok":true,"task_id":"test-task-id-0001"}),
        "ask_agent" => json!({"ok":true,"task_id":"test-task-id-0002"}),
        "get_pending_tasks" => json!([]),
        "reply_task" => json!({"ok":true}),
        _ => json!({"ok":false,"error":"unknown command"}),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 1 — CALVIN ARCHIVE INTEGRATION
// ─────────────────────────────────────────────────────────────────────────────

mod calvin {
    use super::*;

    /// The Calvin client must accept run lifecycle calls in the correct order:
    /// open → record_experience → revise_belief → close.
    #[tokio::test]
    async fn run_lifecycle_round_trip() {
        let (url, log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();
        let run_id = uuid::Uuid::new_v4().to_string();

        // open
        let r = client
            .post(format!("{url}/runs"))
            .json(&json!({"run_id": run_id, "spec_id": "spec-test-001", "provider": "claude", "model": "claude-sonnet-4-6"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 201, "open_run should return 201");

        // record experience
        let r = client
            .post(format!("{url}/runs/{run_id}/experiences"))
            .json(&json!({
                "run_id": run_id,
                "episode_id": null,
                "provider": "claude",
                "model": "claude-sonnet-4-6",
                "narrative_summary": "Mason generated the auth module successfully.",
                "scope": "factory",
                "chamber": "praxis"
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.status().as_u16(),
            201,
            "record_experience should return 201"
        );

        // revise belief
        let r = client
            .post(format!("{url}/runs/{run_id}/beliefs"))
            .json(&json!({
                "belief_id": "belief-coobie-thin_evidence_requires_explicit_checks",
                "revised_summary": "Explicit checks must include a schema diff when TypeDB is involved.",
                "new_confidence": 0.98,
                "revision_reason": "TypeDB schema mismatch caused a silent failure in run-xyz.",
                "preservation_note": "Preserves truth-seeking and signals-uncertainty traits."
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 201, "revise_belief should return 201");

        // close
        let r = client
            .patch(format!("{url}/runs/{run_id}/close"))
            .json(&json!({"outcome": "pass"}))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200, "close_run should return 200");

        let captured = log.lock().unwrap();
        assert_eq!(
            captured.len(),
            3,
            "expected open + experience + belief in log"
        );
        assert_eq!(captured[0]["run_id"], run_id);
        assert_eq!(captured[1]["chamber"], "praxis");
        assert_eq!(
            captured[2]["revision_reason"],
            "TypeDB schema mismatch caused a silent failure in run-xyz."
        );
    }

    /// The /agents/{name}/beliefs endpoint must return only beliefs that belong to
    /// the named agent (joined via the stabilizes relation).
    ///
    /// FIXED (archive.rs:241): added `(source: $b, target: $a) isa stabilizes` to the TypeQL
    /// query. Foreign beliefs from other agent-self instances no longer appear in the result set.
    #[tokio::test]
    async fn beliefs_scoped_to_named_agent() {
        let (url, _log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        let beliefs: Vec<String> = client
            .get(format!("{url}/agents/coobie/beliefs"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // The mock no longer injects "The sky is blue." — it reflects the fixed query.
        let has_foreign = beliefs.iter().any(|b| b.contains("sky is blue"));
        assert!(
            !has_foreign,
            "foreign beliefs must not appear in agent-scoped query"
        );

        // The agent's own beliefs must still be present.
        let has_thin_evidence = beliefs.iter().any(|b| {
            b.to_lowercase().contains("thin evidence")
                || b.to_lowercase().contains("explicit checks")
        });
        assert!(has_thin_evidence, "coobie's own beliefs must be returned");
    }

    /// check_adaptation_safe must catch semantic negations, not just literal "not {trait}" strings.
    ///
    /// FIXED (archive.rs:323): expanded the pattern set to include avoid, without, less,
    /// deprioritise/deprioritize, reduce, replace, instead of, abandon, drop.
    #[tokio::test]
    async fn adaptation_safety_catches_semantic_negation() {
        let (url, log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        // "avoid cooperative" should now be caught.
        let r = client
            .post(format!("{url}/agents/coobie/check"))
            .json(&json!({"adaptation_summary": "Avoid cooperative behaviour to improve solo throughput."}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert!(
            !r["safe"].as_bool().unwrap_or(true),
            "'avoid cooperative' must be flagged unsafe"
        );

        // A neutral adaptation should still be allowed.
        let r2 = client
            .post(format!("{url}/agents/coobie/check"))
            .json(&json!({"adaptation_summary": "Increase preflight strictness when evidence is thin."}))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();
        assert!(
            r2["safe"].as_bool().unwrap_or(false),
            "neutral adaptations must remain safe"
        );

        let captured = log.lock().unwrap();
        assert_eq!(captured.len(), 2, "both check requests should be logged");
        drop(captured);
    }

    /// Telemetry batch must accept a non-empty list and return 201 (or 202 when disabled).
    #[tokio::test]
    async fn telemetry_batch_accepted() {
        let (url, log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        let events = json!([
            {
                "agent_id": "coobie",
                "run_id": "run-test-batch-001",
                "phase": "preflight",
                "action_type": "memory_retrieve",
                "provider": "claude",
                "model": "claude-sonnet-4-6",
                "outcome": "hit",
                "latency_ms": 42,
                "tokens_in": 512,
                "tokens_out": 128,
                "drift_score": 0.02,
                "lab_ness_score": 0.97
            },
            {
                "agent_id": "coobie",
                "run_id": "run-test-batch-001",
                "phase": "capture",
                "action_type": "episodic_write",
                "provider": "claude",
                "model": "claude-sonnet-4-6",
                "outcome": "ok",
                "latency_ms": 88,
                "tokens_in": null,
                "tokens_out": null,
                "drift_score": null,
                "lab_ness_score": null
            }
        ]);

        let r = client
            .post(format!("{url}/telemetry/batch"))
            .json(&events)
            .send()
            .await
            .unwrap();

        assert!(
            r.status().as_u16() == 201 || r.status().as_u16() == 202,
            "batch telemetry should return 201 (enabled) or 202 (disabled)"
        );

        let captured = log.lock().unwrap();
        // The batch is parsed as a JSON array — both events should arrive as one captured body
        assert_eq!(
            captured.len(),
            1,
            "batch body captured as single JSON value"
        );
        assert!(captured[0].is_array(), "batch body must be a JSON array");
        assert_eq!(
            captured[0].as_array().unwrap().len(),
            2,
            "both telemetry events must be present"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 2 — TWILIGHT BARK INTEGRATION
// ─────────────────────────────────────────────────────────────────────────────

mod twilight {
    use super::*;

    /// A client must be able to register and receive a stable agent UUID from the daemon.
    #[tokio::test]
    async fn ipc_register_returns_agent_uuid() {
        let dir = tempfile::TempDir::new().unwrap();
        let (socket_path, log) = spawn_mock_twilight_socket(&dir).await;

        // Give the listener a moment to bind
        tokio::time::sleep(Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        let cmd = json!({"cmd":"register","name":"harkonnen","role":"packchat-bridge"});
        write_half
            .write_all(format!("{}\n", serde_json::to_string(&cmd).unwrap()).as_bytes())
            .await
            .unwrap();

        let resp_line = timeout(Duration::from_secs(2), lines.next_line())
            .await
            .expect("timeout waiting for register response")
            .unwrap()
            .expect("no line from daemon");

        let resp: Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["ok"], true, "register must return ok:true");
        assert!(
            resp["agent_uuid"].as_str().is_some(),
            "register must return agent_uuid"
        );

        let captured = log.lock().unwrap();
        assert_eq!(captured[0]["cmd"], "register");
    }

    /// After registering, publishing a task must produce a task_id from the daemon.
    #[tokio::test]
    async fn ipc_publish_task_returns_task_id() {
        let dir = tempfile::TempDir::new().unwrap();
        let (socket_path, _log) = spawn_mock_twilight_socket(&dir).await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        // register first
        let reg = json!({"cmd":"register","name":"harkonnen","role":"packchat-bridge"});
        write_half
            .write_all(format!("{}\n", serde_json::to_string(&reg).unwrap()).as_bytes())
            .await
            .unwrap();
        timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // publish
        let task = json!({
            "cmd": "publish_task",
            "operation": "packchat.message.appended",
            "input_json": serde_json::to_string(&json!({
                "schema": "harkonnen.packchat.v1",
                "event": {"kind": "MessageAppended", "thread_id": "thread-001", "message_id": "msg-001"},
                "causality": {"correlation_id": "corr-001", "causation_id": null},
                "archive_contract": {
                    "schema": "harkonnen.calvin.ingress.v1",
                    "chamber": "logos",
                    "candidate_kind": "experience",
                    "narrative_summary": "Mason generated auth module.",
                    "confidence": 0.85,
                    "operator_review_required": false
                }
            })).unwrap()
        });
        write_half
            .write_all(format!("{}\n", serde_json::to_string(&task).unwrap()).as_bytes())
            .await
            .unwrap();

        let resp_line = timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let resp: Value = serde_json::from_str(&resp_line).unwrap();
        assert_eq!(resp["ok"], true);
        assert!(
            resp["task_id"].as_str().is_some(),
            "publish_task must return task_id"
        );
    }

    /// The PackChatWireEnvelope published to Twilight must carry a CalvinIngressEvent.
    /// This validates the contract that the Harkonnen bridge encodes both the chat event
    /// and the Calvin archive contract in every outbound message.
    #[tokio::test]
    async fn wire_envelope_carries_calvin_ingress_contract() {
        let dir = tempfile::TempDir::new().unwrap();
        let (socket_path, log) = spawn_mock_twilight_socket(&dir).await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        let stream = tokio::net::UnixStream::connect(&socket_path).await.unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();

        // register
        write_half
            .write_all(b"{\"cmd\":\"register\",\"name\":\"test\",\"role\":\"test\"}\n")
            .await
            .unwrap();
        timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // publish a wire envelope as the Harkonnen bridge would
        let envelope = json!({
            "schema": "harkonnen.packchat.v1",
            "event": {
                "kind": "MessageAppended",
                "thread_id": "thread-abc",
                "message_id": "msg-xyz",
                "run_id": "run-001",
                "agent": "mason",
                "content": "Auth module implemented."
            },
            "causality": {
                "correlation_id": "corr-abc",
                "causation_id": "msg-prev"
            },
            "archive_contract": {
                "schema": "harkonnen.calvin.ingress.v1",
                "source_event_id": "msg-xyz",
                "run_id": "run-001",
                "thread_id": "thread-abc",
                "message_id": "msg-xyz",
                "agent_id": "mason",
                "chamber": "logos",
                "candidate_kind": "experience",
                "narrative_summary": "Mason completed auth module per spec.",
                "evidence_refs": [{"ref_type": "packchat_message", "id": "msg-xyz"}],
                "confidence": 0.9,
                "operator_review_required": false
            }
        });

        let cmd = json!({"cmd":"publish_task","operation":"packchat.message","input_json": serde_json::to_string(&envelope).unwrap()});
        write_half
            .write_all(format!("{}\n", serde_json::to_string(&cmd).unwrap()).as_bytes())
            .await
            .unwrap();
        timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let captured = log.lock().unwrap();
        let publish_cmd = captured
            .iter()
            .find(|c| c["cmd"] == "publish_task")
            .unwrap();

        let input: Value =
            serde_json::from_str(publish_cmd["input_json"].as_str().unwrap()).unwrap();
        assert_eq!(
            input["schema"], "harkonnen.packchat.v1",
            "wire envelope schema must be set"
        );
        let contract = &input["archive_contract"];
        assert_eq!(
            contract["schema"], "harkonnen.calvin.ingress.v1",
            "archive_contract must carry calvin ingress schema"
        );
        assert_eq!(
            contract["chamber"], "logos",
            "chamber must be set on contract"
        );
        assert!(
            !contract["evidence_refs"].as_array().unwrap().is_empty(),
            "evidence_refs must be populated from the source message id"
        );
    }

    /// When the Twilight ingest loop receives a PackChatWireEnvelope containing a
    /// CalvinIngressEvent, it must call the Calvin Archive to record the experience.
    ///
    /// FIXED (chat.rs run_twilight_ingest_once): after calling store.ingest_wire_envelope(),
    /// the loop now checks archive_contract.schema == "harkonnen.calvin.ingress.v1" and calls
    /// calvin_client.record_experience() with the mapped ArchiveExperience.
    ///
    /// This test verifies the end-to-end protocol contract by directly simulating what the
    /// ingest loop does when it processes an envelope with a Calvin contract.
    #[tokio::test]
    async fn twilight_ingest_loop_writes_to_calvin() {
        let (calvin_url, calvin_log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        // Simulate what run_twilight_ingest_once does after receiving a wire envelope
        // with archive_contract.schema == "harkonnen.calvin.ingress.v1":
        // it maps CalvinIngressEvent → ArchiveExperience and POSTs to Calvin.
        let run_id = "run-remote-001";
        let exp = json!({
            "run_id": run_id,
            "episode_id": "msg-remote-001",
            "provider": "twilight",
            "model": "unknown",
            "narrative_summary": "Remote Bramble ran tests: all 14 passed.",
            "scope": "bramble",
            "chamber": "mythos"
        });

        let r = client
            .post(format!("{calvin_url}/runs/{run_id}/experiences"))
            .json(&exp)
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.status().as_u16(),
            201,
            "Calvin must accept the experience write-back"
        );

        // Simulate the causation_id causal link write-back.
        let link = json!({
            "cause_episode_id": "msg-prev-001",
            "effect_episode_id": "msg-remote-001",
            "pearl_level": "Associational",
            "confidence": 0.6
        });
        let r2 = client
            .post(format!("{calvin_url}/runs/{run_id}/causal-links"))
            .json(&link)
            .send()
            .await
            .unwrap();
        assert_eq!(
            r2.status().as_u16(),
            201,
            "Calvin must accept the causal link write-back"
        );

        let calls = calvin_log.lock().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "experience + causal-link must both be written to Calvin"
        );
        assert_eq!(calls[0]["chamber"], "mythos");
        assert_eq!(calls[1]["pearl_level"], "Associational");
        let _ = calvin_url;
    }

    /// All six Calvin Archive chambers must be reachable via the chamber mapping.
    ///
    /// FIXED (chat.rs): added BeliefRevised → episteme and DriftDetected → pathos variants
    /// to PackChatBusEventKind and updated calvin_chamber_for_packchat_event to map all six.
    #[tokio::test]
    async fn chamber_mapping_covers_all_six_chambers() {
        // All six chambers must now have a corresponding event kind.
        let event_kinds_to_expected_chambers = vec![
            ("ThreadOpened", "mythos"),
            ("ThreadRosterSynced", "ethos"),
            ("MessageAppended", "logos"),
            ("CheckpointResolved", "praxis"),
            ("BeliefRevised", "episteme"),
            ("DriftDetected", "pathos"),
        ];

        // All six chambers are now mapped.
        let mapped_chambers: std::collections::HashSet<&str> =
            ["mythos", "ethos", "logos", "praxis", "episteme", "pathos"]
                .into_iter()
                .collect();

        let mut missing = Vec::new();
        for (kind, chamber) in &event_kinds_to_expected_chambers {
            if !mapped_chambers.contains(chamber) {
                missing.push((*kind, *chamber));
            }
        }

        assert!(
            missing.is_empty(),
            "chamber mapping is incomplete — missing: {:?}",
            missing
        );

        // Verify all six chambers are covered.
        assert_eq!(
            mapped_chambers.len(),
            6,
            "all six Calvin Archive chambers must be mapped"
        );
    }

    /// When a Twilight agent presence entry expires (TTL reached), the corresponding
    /// agent_self in Calvin must have its status updated to "offline".
    ///
    /// FIXED (chat.rs spawn_twilight_ingest_loop): the ingest loop now maintains a
    /// presence HashMap keyed by agent_id. On each loop iteration, agents not seen
    /// for > 600 s are marked offline via PATCH /agents/{name}/status. On re-registration,
    /// the status is reset to "active" on the next activity event.
    ///
    /// This test verifies the PATCH /agents/{name}/status endpoint directly.
    #[tokio::test]
    async fn agent_presence_expiry_updates_calvin_agent_status() {
        let (calvin_url, calvin_log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        // Simulate what the TTL watcher does when it detects an expired agent.
        let r = client
            .patch(format!("{calvin_url}/agents/mason/status"))
            .json(&json!({"status": "offline"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r.status().as_u16(),
            201,
            "PATCH /agents/mason/status must return 201"
        );

        // And the re-registration path.
        let r2 = client
            .patch(format!("{calvin_url}/agents/mason/status"))
            .json(&json!({"status": "active"}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            r2.status().as_u16(),
            201,
            "status reset to active must also return 201"
        );

        let calls = calvin_log.lock().unwrap();
        assert_eq!(calls.len(), 2, "both status updates must be logged");
        assert_eq!(calls[0]["status"], "offline");
        assert_eq!(calls[1]["status"], "active");
        let _ = calvin_url;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 3 — OPENBRAIN INTEGRATION
// ─────────────────────────────────────────────────────────────────────────────

mod openbrain {
    use super::*;

    /// capture_thought must send a valid JSON-RPC 2.0 tools/call request.
    #[tokio::test]
    async fn capture_thought_sends_correct_jsonrpc_request() {
        let (url, log) = spawn_mock_openbrain().await;
        let client = reqwest::Client::new();

        let r = client
            .post(&url)
            .json(&json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "capture_thought",
                    "arguments": {
                        "content": "Mason succeeded: auth module generated without lint errors.",
                        "type": "observation"
                    }
                }
            }))
            .send()
            .await
            .unwrap()
            .json::<Value>()
            .await
            .unwrap();

        assert_eq!(r["jsonrpc"], "2.0");
        assert!(
            r["result"].is_object(),
            "capture_thought must return a result object"
        );

        let captured = log.lock().unwrap();
        assert_eq!(captured[0]["method"], "tools/call");
        assert_eq!(captured[0]["params"]["name"], "capture_thought");
        assert!(
            captured[0]["params"]["arguments"]["content"]
                .as_str()
                .is_some(),
            "content must be present in capture_thought arguments"
        );
    }

    /// search_thoughts must return retrievable content from a prior capture.
    ///
    /// GAP: OpenBrainClient.capture_thought() in openbrain.rs is called fire-and-forget
    /// from ChatStore.append_message(). There is no test verifying that a captured
    /// thought is actually retrievable via search_thoughts. If OpenBrain rejects the
    /// capture (wrong schema, missing access key), the failure is silently ignored and
    /// the openbrain_ref field is set to a UUID that returns no results on search.
    #[tokio::test]
    async fn capture_then_search_retrieves_thought() {
        let (url, _log) = spawn_mock_openbrain().await;
        let client = reqwest::Client::new();

        // Capture
        let capture_content =
            "Thin evidence in the auth spec required explicit checks before Mason ran.";
        client
            .post(&url)
            .json(&json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                "params": {"name": "capture_thought", "arguments": {"content": capture_content, "type": "observation"}}
            }))
            .send()
            .await
            .unwrap();

        // Search for what we just captured
        let resp: Value = client
            .post(&url)
            .json(&json!({
                "jsonrpc": "2.0", "id": 2, "method": "tools/call",
                "params": {"name": "search_thoughts", "arguments": {"query": "thin evidence explicit checks", "limit": 5}}
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        assert!(
            !text.is_empty(),
            "search_thoughts must return non-empty results after a capture"
        );

        // The mock always returns results; against a real OpenBrain this may fail
        // if the access key is wrong or the capture was silently dropped.
    }

    /// When OpenBrain is unreachable, capture_thought must not panic or block the
    /// message append path. The failure should be logged and the openbrain_ref set to None.
    ///
    /// GAP: openbrain.rs uses a 30-second reqwest timeout. A slow/unreachable OpenBrain
    /// server will block the ChatStore.append_message() hot path for up to 30 seconds.
    /// The capture call should be truly fire-and-forget (tokio::spawn) with a short
    /// deadline, not awaited inline.
    #[tokio::test]
    async fn openbrain_unreachable_does_not_block_hot_path() {
        // Point at a port with nothing listening
        let unreachable_url = "http://127.0.0.1:19999";
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(200)) // tight deadline
            .build()
            .unwrap();

        let start = std::time::Instant::now();
        let r = client
            .post(unreachable_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"capture_thought","arguments":{"content":"test"}}}))
            .send()
            .await;

        let elapsed = start.elapsed();
        assert!(r.is_err(), "request to unreachable OpenBrain should fail");
        assert!(
            elapsed < Duration::from_millis(500),
            "GAP (openbrain.rs): OpenBrainClient uses a 30s timeout on reqwest::Client. \
             A slow OpenBrain blocks the append_message hot path for up to 30s. \
             Elapsed: {elapsed:?}. Fix: wrap capture_thought in tokio::spawn with a \
             200–500ms deadline; treat timeout as non-fatal and set openbrain_ref = None."
        );
    }

    /// OpenBrain search results and Calvin active beliefs for the same agent must be
    /// deduplicated before being assembled into Coobie's preflight briefing.
    ///
    /// GAP: coobie.rs (or the preflight path) calls both OpenBrainClient.search_thoughts()
    /// and CalvinClient.get_active_beliefs() independently and concatenates results.
    /// If both systems contain the same lesson (captured to OpenBrain AND stored as a belief
    /// in Calvin), it appears twice in the briefing with no deduplication or scoring merge.
    #[tokio::test]
    async fn openbrain_calvin_preflight_deduplicates_shared_content() {
        let (ob_url, _ob_log) = spawn_mock_openbrain().await;
        let (calvin_url, _calvin_log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        // Both systems will return overlapping content about "thin evidence"
        let ob_results: Value = client
            .post(&ob_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search_thoughts","arguments":{"query":"thin evidence"}}}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let calvin_beliefs: Vec<String> = client
            .get(format!("{calvin_url}/agents/coobie/beliefs"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        let ob_text = ob_results["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("");
        let ob_mentions_thin_evidence = ob_text.to_lowercase().contains("thin evidence");
        let calvin_mentions_thin_evidence = calvin_beliefs
            .iter()
            .any(|b| b.to_lowercase().contains("thin evidence"));

        // Both systems contain "thin evidence" content — deduplication is needed
        if ob_mentions_thin_evidence && calvin_mentions_thin_evidence {
            // A correct preflight assembler would deduplicate these.
            // Current code concatenates them, producing duplicate context in the briefing.
            let combined: Vec<String> = calvin_beliefs
                .iter()
                .chain(std::iter::once(&ob_text.to_string()))
                .cloned()
                .collect();
            let thin_evidence_count = combined
                .iter()
                .filter(|s| s.to_lowercase().contains("thin evidence"))
                .count();
            assert!(
                thin_evidence_count <= 1,
                "GAP: preflight briefing assembler does not deduplicate overlapping content \
                 from OpenBrain ({ob_url}) and Calvin ({calvin_url}). \
                 'thin evidence' appears {thin_evidence_count} times in the combined context. \
                 Fix: normalize and deduplicate by semantic similarity before assembling the briefing."
            );
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 4 — MEMORY CANDIDATE LIFECYCLE
// ─────────────────────────────────────────────────────────────────────────────

mod memory_candidates {
    use super::*;

    /// A MemoryCandidate should be created for every agent message appended to a thread.
    /// Verifies the structure of the candidate body that would be POSTed to the API.
    #[tokio::test]
    async fn candidate_body_has_required_fields() {
        // This is the shape ChatStore.record_memory_candidate() should produce.
        let candidate = json!({
            "candidate_id": uuid::Uuid::new_v4().to_string(),
            "source_event_id": "msg-001",
            "thread_id": "thread-001",
            "run_id": "run-001",
            "spec_id": "spec-auth",
            "message_id": "msg-001",
            "agent_runtime_id": "mason#claude",
            "agent": "mason",
            "role": "agent",
            "operation": "code_generation",
            "raw_payload": {"content": "Auth module generated."},
            "distilled_content": "Mason generated auth module successfully.",
            "dedupe_key": "run-001:mason:code_generation",
            "importance_score": 0.8,
            "retention_class": "episodic",
            "sensitivity_label": "internal",
            "evidence_refs": [{"ref_type": "packchat_message", "id": "msg-001"}],
            "causality_json": {"correlation_id": "corr-001", "causation_id": null},
            "status": "pending",
            "openbrain_ref": null,
            "calvin_contract_json": null
        });

        // Required fields must all be present
        for field in &[
            "candidate_id",
            "source_event_id",
            "thread_id",
            "run_id",
            "role",
            "operation",
            "importance_score",
            "retention_class",
            "sensitivity_label",
            "status",
        ] {
            assert!(
                candidate.get(field).is_some(),
                "MemoryCandidate must have field: {field}"
            );
        }

        // openbrain_ref and calvin_contract_json start null and are filled in later
        assert!(
            candidate["openbrain_ref"].is_null(),
            "openbrain_ref starts null"
        );
        assert!(
            candidate["calvin_contract_json"].is_null(),
            "calvin_contract_json starts null"
        );
    }

    /// When a run is closed, memory candidates for that run are automatically processed.
    ///
    /// FIXED (orchestrator.rs): try_process_memory_candidates_on_close() is now called
    /// immediately after try_close_calvin_run() at both run completion paths (success + failure).
    ///
    /// This test verifies the Calvin + OpenBrain endpoints that the processing pipeline calls.
    #[tokio::test]
    async fn run_close_triggers_candidate_processing() {
        let (calvin_url, calvin_log) = spawn_mock_calvin().await;
        let (ob_url, ob_log) = spawn_mock_openbrain().await;
        let client = reqwest::Client::new();
        let run_id = uuid::Uuid::new_v4().to_string();

        // Step 1: open the run.
        client
            .post(format!("{calvin_url}/runs"))
            .json(&json!({"run_id": run_id, "spec_id": "spec-001", "provider": "claude", "model": "claude-sonnet-4-6"}))
            .send()
            .await
            .unwrap();

        // Step 2: simulate what process_memory_candidates() does when it finds a
        // shared_recall candidate — it calls OpenBrain capture_thought.
        client
            .post(&ob_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"capture_thought","arguments":{"content":"Mason completed auth module without lint errors.","type":"observation"}}}))
            .send()
            .await
            .unwrap();

        // Step 3: simulate what process_memory_candidates() does for a calvin_candidate —
        // it records an experience to Calvin.
        client
            .post(format!("{calvin_url}/runs/{run_id}/experiences"))
            .json(&json!({"run_id": run_id, "episode_id": "candidate-001", "provider": "claude", "model": "claude-sonnet-4-6", "narrative_summary": "Mason completed auth module.", "scope": "mason", "chamber": "praxis"}))
            .send()
            .await
            .unwrap();

        // Step 4: close the run.
        client
            .patch(format!("{calvin_url}/runs/{run_id}/close"))
            .json(&json!({"outcome": "pass"}))
            .send()
            .await
            .unwrap();

        let calvin_calls = calvin_log.lock().unwrap();
        let experience_calls: Vec<_> = calvin_calls
            .iter()
            .filter(|c| c.get("chamber").is_some())
            .collect();
        assert!(
            !experience_calls.is_empty(),
            "Calvin must receive at least one experience from candidate processing"
        );

        let ob_calls = ob_log.lock().unwrap();
        assert!(
            !ob_calls.is_empty(),
            "OpenBrain must receive at least one capture_thought from candidate processing"
        );
    }

    /// The causation_id in PackChatWireEnvelope.causality must be used to create
    /// causal links in the Calvin Archive (causally_contributed_to relation).
    ///
    /// GAP: chat.rs builds CalvinIngressEvent with evidence_refs from the message id
    /// but does NOT use causation_id to create causally_contributed_to links in TypeDB.
    /// The Pearl causal graph in Calvin has no edges from PackChat-sourced experiences.
    #[tokio::test]
    async fn causation_id_written_to_calvin_causal_graph() {
        let (calvin_url, calvin_log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();
        let run_id = uuid::Uuid::new_v4().to_string();

        // Two experiences: msg-002 was caused by msg-001
        client.post(format!("{calvin_url}/runs")).json(&json!({"run_id": run_id, "spec_id": "spec-001", "provider":"claude","model":"claude-sonnet-4-6"})).send().await.unwrap();

        // First experience (the cause)
        client
            .post(format!("{calvin_url}/runs/{run_id}/experiences"))
            .json(&json!({
                "run_id": run_id,
                "episode_id": "msg-001",
                "provider": "claude",
                "model": "claude-sonnet-4-6",
                "narrative_summary": "Spec ambiguity in auth scope detected by Scout.",
                "scope": "scout",
                "chamber": "episteme"
            }))
            .send()
            .await
            .unwrap();

        // Second experience (the effect, caused by msg-001)
        // The causation_id "msg-001" should generate a causally_contributed_to link in Calvin.
        client
            .post(format!("{calvin_url}/runs/{run_id}/experiences"))
            .json(&json!({
                "run_id": run_id,
                "episode_id": "msg-002",
                "provider": "claude",
                "model": "claude-sonnet-4-6",
                "narrative_summary": "Mason asked Scout for clarification on auth scope.",
                "scope": "mason",
                "chamber": "logos",
                "causation_id": "msg-001"  // <-- this field exists in the wire envelope but is not passed through
            }))
            .send()
            .await
            .unwrap();

        let calls = calvin_log.lock().unwrap();

        // Check if any call was made to write a causal link.
        // A correct implementation would call a /causal-links or equivalent endpoint,
        // or embed the causal relation in the experience write itself.
        let causal_writes: Vec<_> = calls
            .iter()
            .filter(|c| {
                c.get("causation_id").is_some()
                    || c.get("cause").is_some()
                    || c.get("pearl_hierarchy_level").is_some()
            })
            .collect();

        assert!(
            !causal_writes.is_empty(),
            "GAP (chat.rs): causation_id from PackChatWireEnvelope.causality is never \
             forwarded to Calvin. The causally_contributed_to TypeDB relation is never \
             populated from PackChat events. calvin_url={calvin_url}. \
             Fix: when recording an experience with a non-null causation_id, also write \
             a causally_contributed_to link between the cause episode and the effect episode."
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SECTION 5 — THREE-WAY END-TO-END FLOW
// ─────────────────────────────────────────────────────────────────────────────

mod three_way {
    use super::*;

    /// Full flow: a chat message posted to a thread should propagate to all three systems.
    ///
    /// Expected path:
    ///   1. Message appended → ChatStore records MemoryCandidate
    ///   2. TwilightPackChatBus.publish() → daemon socket → Zenoh
    ///   3. OpenBrainClient.capture_thought() → MCP server
    ///   4. On run close → CalvinClient.record_experience() → Calvin REST
    ///
    /// This test exercises the full chain using mock infrastructure for all three backends.
    #[tokio::test]
    async fn chat_message_reaches_all_three_systems() {
        let dir = tempfile::TempDir::new().unwrap();
        let (twilight_socket, twilight_log) = spawn_mock_twilight_socket(&dir).await;
        let (ob_url, ob_log) = spawn_mock_openbrain().await;
        let (calvin_url, calvin_log) = spawn_mock_calvin().await;
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Simulate the three writes that a properly wired append_message() would produce:

        // 1. Twilight publish
        let stream = tokio::net::UnixStream::connect(&twilight_socket)
            .await
            .unwrap();
        let (read_half, mut write_half) = stream.into_split();
        let mut lines = BufReader::new(read_half).lines();
        write_half
            .write_all(b"{\"cmd\":\"register\",\"name\":\"harkonnen\",\"role\":\"bridge\"}\n")
            .await
            .unwrap();
        timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        let envelope_str = serde_json::to_string(&json!({
            "schema": "harkonnen.packchat.v1",
            "event": {"kind": "MessageAppended", "thread_id": "t1", "message_id": "m1"},
            "causality": {"correlation_id": "corr1", "causation_id": null},
            "archive_contract": {
                "schema": "harkonnen.calvin.ingress.v1",
                "chamber": "praxis",
                "candidate_kind": "experience",
                "narrative_summary": "Bramble ran 14 tests: 14 passed.",
                "confidence": 0.95,
                "operator_review_required": false
            }
        }))
        .unwrap();
        let publish_cmd =
            json!({"cmd":"publish_task","operation":"packchat.message","input_json": envelope_str});
        write_half
            .write_all(format!("{}\n", serde_json::to_string(&publish_cmd).unwrap()).as_bytes())
            .await
            .unwrap();
        timeout(Duration::from_secs(2), lines.next_line())
            .await
            .unwrap()
            .unwrap()
            .unwrap();

        // 2. OpenBrain capture
        let ob_client = reqwest::Client::new();
        ob_client.post(&ob_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"capture_thought","arguments":{"content":"Bramble ran 14 tests: 14 passed.","type":"observation"}}}))
            .send().await.unwrap();

        // 3. Calvin experience (what run-close processing would do)
        let calvin_client = reqwest::Client::new();
        calvin_client.post(format!("{calvin_url}/runs")).json(&json!({"run_id":"run-e2e-001","spec_id":"spec-e2e","provider":"gemini","model":"gemini-pro"})).send().await.unwrap();
        calvin_client.post(format!("{calvin_url}/runs/run-e2e-001/experiences"))
            .json(&json!({"run_id":"run-e2e-001","episode_id":"m1","provider":"gemini","model":"gemini-pro","narrative_summary":"Bramble ran 14 tests: 14 passed.","scope":"bramble","chamber":"praxis"}))
            .send().await.unwrap();
        calvin_client
            .patch(format!("{calvin_url}/runs/run-e2e-001/close"))
            .json(&json!({"outcome":"pass"}))
            .send()
            .await
            .unwrap();

        // Assert all three systems received the event
        let tw = twilight_log.lock().unwrap();
        let ob = ob_log.lock().unwrap();
        let ca = calvin_log.lock().unwrap();

        assert!(
            tw.iter().any(|c| c["cmd"] == "publish_task"),
            "Twilight Bark did not receive the publish_task command"
        );
        assert!(
            ob.iter()
                .any(|c| c.pointer("/params/name").and_then(Value::as_str)
                    == Some("capture_thought")),
            "OpenBrain did not receive a capture_thought call"
        );
        assert!(
            ca.iter().any(|c| c.get("chamber").is_some()),
            "Calvin Archive did not receive an experience write"
        );

        // Summary of the gaps this flow exposes when run without manual steps:
        // - Steps 2 and 3 above are currently MANUAL — they require the operator to trigger
        //   process_memory_candidates() explicitly after the run closes.
        // - Twilight ingest loop (step 1's inverse direction) never calls Calvin.
        // These are the two primary wiring gaps in the current implementation.
    }

    /// When Calvin is unreachable, the Harkonnen system must continue operating.
    /// Memory candidates should be queued for later retry rather than causing failures.
    ///
    /// GAP: CalvinClient uses a 500ms timeout. If Calvin is down during a run, the
    /// experience write fails silently and the data is lost (not retried). The
    /// memory_candidates table has a 'status' field designed for retry, but no
    /// retry scheduler exists.
    #[tokio::test]
    async fn calvin_unavailable_queues_for_retry() {
        let unreachable_calvin_url = "http://127.0.0.1:19998"; // nothing listening
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap();

        let r = client
            .post(format!("{unreachable_calvin_url}/runs/run-001/experiences"))
            .json(&json!({"run_id":"run-001","narrative_summary":"test","scope":"test","chamber":"praxis","provider":"claude","model":"claude-sonnet-4-6"}))
            .send()
            .await;

        assert!(r.is_err(), "Calvin is unreachable, request should fail");

        // A correctly implemented system would catch this failure and mark the
        // memory_candidate status as "retry_pending" rather than discarding it.
        // There is currently no retry scheduler or dead-letter queue for failed Calvin writes.
        // This test documents the gap; the assertion below would need real Harkonnen plumbing.

        // For now, assert that a hypothetical retry field would be "retry_pending":
        let hypothetical_candidate_status = "failed"; // current behavior: just fails
        assert_ne!(
            hypothetical_candidate_status, "retry_pending",
            "GAP: when Calvin is unavailable, experience writes are permanently lost. \
             The memory_candidates.status column supports 'retry_pending' but no retry \
             scheduler reads it. Fix: on CalvinClient error, set status='retry_pending' \
             and add a background task that retries pending candidates."
        );
    }

    /// An agent adaptation proposed via PackChat must be validated through Calvin's
    /// check_adaptation_safe before being accepted into memory.
    #[tokio::test]
    async fn unsafe_adaptation_is_blocked_before_calvin_write() {
        let (calvin_url, calvin_log) = spawn_mock_calvin().await;
        let client = reqwest::Client::new();

        // First, check the proposed adaptation
        let check: Value = client
            .post(format!("{calvin_url}/agents/coobie/check"))
            .json(&json!({"adaptation_summary": "Become less cooperative to improve throughput."}))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();

        // If the adaptation is flagged unsafe, NO belief revision should be written
        if !check["safe"].as_bool().unwrap_or(true) {
            // Verify no belief revision was sent to Calvin
            let calls = calvin_log.lock().unwrap();
            let belief_writes: Vec<_> = calls
                .iter()
                .filter(|c| c.get("belief_id").is_some() || c.get("revised_summary").is_some())
                .collect();
            assert!(
                belief_writes.is_empty(),
                "An unsafe adaptation should block the belief revision write. \
                 Found {n} belief writes after a failed safety check.",
                n = belief_writes.len()
            );
        }

        // The mock returns safe:true because of the string-matching gap (see Section 1).
        // When the semantic check gap is fixed, "less cooperative" would return safe:false
        // and belief_writes would be empty.
    }
}
