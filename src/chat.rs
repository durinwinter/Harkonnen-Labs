//! PackChat — conversational control plane for factory runs.
//!
//! Implements the interaction model from `factory/context/backend-interaction-control-plane.yaml`:
//! - Chat threads scoped to a run or spec
//! - Operator messages routed to named agents
//! - Blocking checkpoint materialisation and reply flow
//! - Agent unblock flow resumes a stalled run
//!
//! ## Thread lifecycle
//!
//! 1. Operator opens a thread (optionally tied to a run or spec)
//! 2. Operator sends messages; system routes to the right agent
//! 3. Agent replies are persisted and broadcast as `LiveEvent::RunEvent`
//! 4. When an agent posts a blocker, a `run_checkpoint` is created and linked
//!    to the thread — the operator sees it as a message requiring a reply
//! 5. Operator replies with `POST /api/runs/:id/checkpoints/:cid/reply`
//! 6. Operator calls `POST /api/agents/:agent/unblock` to release the run
//!
//! ## Agent routing
//!
//! @mentions in message content route to a specific agent: `@coobie what did we learn?`
//! Unaddressed messages default to Coobie (memory/context retrieval).
//! Pinned agents (Scout, Sable, Keeper) always use Claude.
//! Routable agents (Mason, Piper, Bramble, Ash, Flint, Coobie) use the setup default.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::{collections::BTreeSet, fs::OpenOptions, io::Write, path::PathBuf, sync::Arc};
use uuid::Uuid;

use crate::{
    config::Paths,
    llm::{self, LlmRequest, Message},
    models::{AgentRuntimeState, CheckpointAnswerRecord, RunCheckpointRecord},
};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ChatThreadKind {
    #[default]
    General,
    Run,
    Spec,
    OperatorModel,
}

impl ChatThreadKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::General => "general",
            Self::Run => "run",
            Self::Spec => "spec",
            Self::OperatorModel => "operator_model",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub thread_id: String,
    pub run_id: Option<String>,
    pub spec_id: Option<String>,
    pub title: String,
    pub status: String, // "open" | "closed"
    #[serde(default)]
    pub thread_kind: ChatThreadKind,
    #[serde(default = "default_thread_metadata")]
    pub metadata_json: Value,
    pub created_at: chrono::DateTime<Utc>,
    pub updated_at: chrono::DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub message_id: String,
    pub thread_id: String,
    /// `"operator"` | `"agent"` | `"system"`
    pub role: String,
    /// Which agent sent or is addressed by this message.
    pub agent: Option<String>,
    /// Stable dog instance identifier when a canonical role has multiple lives.
    #[serde(default)]
    pub agent_runtime_id: Option<String>,
    pub content: String,
    /// Set when this message resolved a checkpoint.
    pub checkpoint_id: Option<String>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Request body for opening a new thread.
#[derive(Debug, Deserialize)]
pub struct OpenThreadRequest {
    pub run_id: Option<String>,
    pub spec_id: Option<String>,
    pub title: Option<String>,
    #[serde(default)]
    pub thread_kind: ChatThreadKind,
    #[serde(default)]
    pub metadata_json: Option<Value>,
}

/// Request body for posting a message.
#[derive(Debug, Deserialize)]
pub struct PostMessageRequest {
    pub content: String,
    /// Override agent routing — if None the system extracts @mentions or
    /// defaults to Coobie.
    pub agent: Option<String>,
}

/// Request body for replying to a checkpoint.
#[derive(Debug, Deserialize)]
pub struct CheckpointReplyRequest {
    pub answer_text: String,
    pub answered_by: Option<String>,
}

/// Response for a message post — includes the agent's reply if one was generated.
#[derive(Debug, Serialize)]
pub struct PostMessageResponse {
    pub operator_message: ChatMessage,
    pub agent_reply: Option<ChatMessage>,
}

const PACKCHAT_RECENT_MESSAGE_COUNT: usize = 6;
const PACKCHAT_RELEVANT_MESSAGE_LIMIT: usize = 10;
const PACKCHAT_CONTEXT_NEIGHBOR_WINDOW: usize = 1;
const PACKCHAT_HISTORY_CHAR_BUDGET: usize = 18_000;
const PACKCHAT_MIN_HISTORY_CHAR_BUDGET: usize = 3_000;
const PACKCHAT_MESSAGE_EXCERPT_CHARS: usize = 1_200;
const PACKCHAT_MIN_MESSAGE_EXCERPT_CHARS: usize = 300;
const PACKCHAT_ASSISTANT_CONTEXT_CHARS: usize = 2_400;
const PACKCHAT_MIN_ASSISTANT_CONTEXT_CHARS: usize = 600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PackChatBusEventKind {
    ThreadOpened,
    ThreadRosterSynced,
    MessageAppended,
    CheckpointResolved,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackChatBusEvent {
    pub event_id: String,
    pub topic: String,
    pub kind: PackChatBusEventKind,
    #[serde(default)]
    pub setup_name: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub spec_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub checkpoint_id: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub agent_runtime_id: Option<String>,
    #[serde(default)]
    pub content_preview: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default)]
    pub metadata_json: Value,
    pub emitted_at: chrono::DateTime<Utc>,
}

pub trait PackChatBus: Send + Sync {
    fn publish(&self, event: &PackChatBusEvent) -> Result<()>;
}

#[derive(Debug, Default)]
pub struct NoopPackChatBus;

impl PackChatBus for NoopPackChatBus {
    fn publish(&self, _event: &PackChatBusEvent) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackChatWireEnvelope {
    pub schema: String,
    pub event: PackChatBusEvent,
    pub causality: PackChatCausality,
    pub archive_contract: Option<CalvinIngressEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PackChatCausality {
    pub correlation_id: Option<String>,
    pub causation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalvinIngressEvent {
    pub schema: String,
    pub source_event_id: String,
    pub run_id: Option<String>,
    pub thread_id: Option<String>,
    pub message_id: Option<String>,
    pub agent_id: Option<String>,
    pub agent_runtime_id: Option<String>,
    pub chamber: String,
    pub candidate_kind: String,
    pub narrative_summary: String,
    pub evidence_refs: Vec<CalvinEvidenceRef>,
    pub confidence: f64,
    pub operator_review_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalvinEvidenceRef {
    pub ref_type: String,
    pub id: String,
}

impl PackChatWireEnvelope {
    pub fn from_event(event: PackChatBusEvent) -> Self {
        let causality = PackChatCausality {
            correlation_id: event
                .run_id
                .clone()
                .or_else(|| event.thread_id.clone())
                .or_else(|| event.checkpoint_id.clone()),
            causation_id: event.checkpoint_id.clone(),
        };
        let archive_contract = build_calvin_ingress_event(&event);
        Self {
            schema: "harkonnen.packchat.v1".to_string(),
            event,
            causality,
            archive_contract,
        }
    }
}

pub struct CompositePackChatBus {
    buses: Vec<Arc<dyn PackChatBus>>,
}

impl CompositePackChatBus {
    pub fn new(buses: Vec<Arc<dyn PackChatBus>>) -> Self {
        Self { buses }
    }
}

impl PackChatBus for CompositePackChatBus {
    fn publish(&self, event: &PackChatBusEvent) -> Result<()> {
        for bus in &self.buses {
            bus.publish(event)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct LocalJsonlPackChatBus {
    path: PathBuf,
    setup_name: String,
}

impl LocalJsonlPackChatBus {
    pub fn new(path: PathBuf, setup_name: impl Into<String>) -> Self {
        Self {
            path,
            setup_name: setup_name.into(),
        }
    }

    fn topic(&self, suffix: &str) -> String {
        format!("harkonnen/{}/{}", self.setup_name, suffix)
    }

    fn enrich_event(&self, event: &PackChatBusEvent) -> PackChatBusEvent {
        let mut enriched = event.clone();
        if enriched.setup_name.is_empty() {
            enriched.setup_name = self.setup_name.clone();
        }
        if enriched.topic.is_empty() {
            enriched.topic = self.topic("chat/unknown");
        } else if !enriched.topic.starts_with("harkonnen/") {
            enriched.topic = self.topic(enriched.topic.trim_start_matches('/'));
        }
        enriched
    }
}

impl PackChatBus for LocalJsonlPackChatBus {
    fn publish(&self, event: &PackChatBusEvent) -> Result<()> {
        let enriched = self.enrich_event(event);
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        serde_json::to_writer(&mut file, &enriched)?;
        file.write_all(b"\n")?;
        Ok(())
    }
}

pub struct TwilightPackChatBus {
    socket_path: PathBuf,
    agent_name: String,
    agent_role: String,
    setup_name: String,
}

impl TwilightPackChatBus {
    pub fn new(
        socket_path: PathBuf,
        agent_name: impl Into<String>,
        agent_role: impl Into<String>,
        setup_name: impl Into<String>,
    ) -> Self {
        Self {
            socket_path,
            agent_name: agent_name.into(),
            agent_role: agent_role.into(),
            setup_name: setup_name.into(),
        }
    }

    fn enrich_event(&self, event: &PackChatBusEvent) -> PackChatBusEvent {
        let mut enriched = event.clone();
        if enriched.setup_name.is_empty() {
            enriched.setup_name = self.setup_name.clone();
        }
        if enriched.topic.is_empty() {
            enriched.topic = format!("harkonnen/{}/chat/unknown", self.setup_name);
        } else if !enriched.topic.starts_with("harkonnen/") {
            enriched.topic = format!(
                "harkonnen/{}/{}",
                self.setup_name,
                enriched.topic.trim_start_matches('/')
            );
        }
        enriched
    }

    fn publish_wire_envelope(&self, envelope: &PackChatWireEnvelope) -> Result<()> {
        publish_packchat_to_twilight_socket(
            &self.socket_path,
            &self.agent_name,
            &self.agent_role,
            envelope,
        )
    }
}

impl PackChatBus for TwilightPackChatBus {
    fn publish(&self, event: &PackChatBusEvent) -> Result<()> {
        let envelope = PackChatWireEnvelope::from_event(self.enrich_event(event));
        self.publish_wire_envelope(&envelope)
    }
}

// ── Chat store ────────────────────────────────────────────────────────────────

/// Thin persistence wrapper — all chat state lives in SQLite.
#[derive(Clone)]
pub struct ChatStore {
    pool: SqlitePool,
    bus: Arc<dyn PackChatBus>,
}

impl std::fmt::Debug for ChatStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatStore").finish_non_exhaustive()
    }
}

impl ChatStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_bus(pool, Arc::new(NoopPackChatBus))
    }

    pub fn with_bus(pool: SqlitePool, bus: Arc<dyn PackChatBus>) -> Self {
        Self { pool, bus }
    }

    pub fn spawn_twilight_ingest_loop(
        &self,
        socket_path: PathBuf,
        agent_name: String,
        agent_role: String,
    ) {
        spawn_twilight_ingest_loop(self.clone(), socket_path, agent_name, agent_role);
    }

    fn publish_bus_event(&self, event: PackChatBusEvent) {
        if let Err(error) = self.bus.publish(&event) {
            tracing::warn!("packchat bus publish failed: {}", error);
        }
    }

    fn build_thread_event(
        &self,
        thread: &ChatThread,
        kind: PackChatBusEventKind,
    ) -> PackChatBusEvent {
        let topic_suffix = match kind {
            PackChatBusEventKind::ThreadOpened => {
                format!("chat/{}/thread/open", thread.thread_id)
            }
            PackChatBusEventKind::ThreadRosterSynced => {
                format!("chat/{}/thread/roster", thread.thread_id)
            }
            PackChatBusEventKind::MessageAppended => {
                format!("chat/{}/message", thread.thread_id)
            }
            PackChatBusEventKind::CheckpointResolved => {
                format!("chat/{}/checkpoint", thread.thread_id)
            }
        };
        PackChatBusEvent {
            event_id: Uuid::new_v4().to_string(),
            topic: topic_suffix,
            kind,
            setup_name: String::new(),
            thread_id: Some(thread.thread_id.clone()),
            run_id: thread.run_id.clone(),
            spec_id: thread.spec_id.clone(),
            message_id: None,
            checkpoint_id: None,
            role: None,
            agent: None,
            agent_runtime_id: None,
            content_preview: String::new(),
            content: None,
            metadata_json: thread.metadata_json.clone(),
            emitted_at: Utc::now(),
        }
    }

    fn build_message_event(&self, thread: &ChatThread, message: &ChatMessage) -> PackChatBusEvent {
        PackChatBusEvent {
            event_id: Uuid::new_v4().to_string(),
            topic: format!("chat/{}/message", message.thread_id),
            kind: PackChatBusEventKind::MessageAppended,
            setup_name: String::new(),
            thread_id: Some(message.thread_id.clone()),
            run_id: thread.run_id.clone(),
            spec_id: thread.spec_id.clone(),
            message_id: Some(message.message_id.clone()),
            checkpoint_id: message.checkpoint_id.clone(),
            role: Some(message.role.clone()),
            agent: message.agent.clone(),
            agent_runtime_id: message.agent_runtime_id.clone(),
            content_preview: preview_chat_content(&message.content, 240),
            content: Some(message.content.clone()),
            metadata_json: serde_json::json!({
                "thread_kind": thread.thread_kind.as_str(),
                "thread_status": thread.status,
            }),
            emitted_at: Utc::now(),
        }
    }

    fn build_checkpoint_event(
        &self,
        checkpoint_id: &str,
        answered_by: &str,
        answer_text: &str,
    ) -> PackChatBusEvent {
        PackChatBusEvent {
            event_id: Uuid::new_v4().to_string(),
            topic: format!("checkpoint/{}/resolved", checkpoint_id),
            kind: PackChatBusEventKind::CheckpointResolved,
            setup_name: String::new(),
            thread_id: None,
            run_id: None,
            spec_id: None,
            message_id: None,
            checkpoint_id: Some(checkpoint_id.to_string()),
            role: Some("operator".to_string()),
            agent: None,
            agent_runtime_id: None,
            content_preview: preview_chat_content(answer_text, 240),
            content: Some(answer_text.to_string()),
            metadata_json: serde_json::json!({
                "answered_by": answered_by,
            }),
            emitted_at: Utc::now(),
        }
    }

    // ── Threads ───────────────────────────────────────────────────────────────

    pub async fn open_thread(&self, req: &OpenThreadRequest) -> Result<ChatThread> {
        let thread_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let title = req
            .title
            .clone()
            .unwrap_or_else(|| "New conversation".to_string());
        let metadata_json = req
            .metadata_json
            .clone()
            .unwrap_or_else(default_thread_metadata);

        sqlx::query(
            r#"
            INSERT INTO chat_threads (thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&thread_id)
        .bind(&req.run_id)
        .bind(&req.spec_id)
        .bind(&title)
        .bind(req.thread_kind.as_str())
        .bind(serde_json::to_string(&metadata_json)?)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert chat_thread")?;

        let thread = ChatThread {
            thread_id,
            run_id: req.run_id.clone(),
            spec_id: req.spec_id.clone(),
            title,
            status: "open".to_string(),
            thread_kind: req.thread_kind.clone(),
            metadata_json,
            created_at: now,
            updated_at: now,
        };
        self.publish_bus_event(
            self.build_thread_event(&thread, PackChatBusEventKind::ThreadOpened),
        );
        Ok(thread)
    }

    pub async fn get_thread(&self, thread_id: &str) -> Result<Option<ChatThread>> {
        let row = sqlx::query(
            "SELECT thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at
             FROM chat_threads WHERE thread_id = ?1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_chat_thread).transpose()
    }

    pub async fn list_threads(
        &self,
        run_id: Option<&str>,
        thread_kind: Option<&ChatThreadKind>,
        limit: usize,
    ) -> Result<Vec<ChatThread>> {
        let rows = match (run_id, thread_kind) {
            (Some(rid), Some(kind)) => {
                sqlx::query(
                    "SELECT thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at
                     FROM chat_threads WHERE run_id = ?1 AND thread_kind = ?2
                     ORDER BY created_at DESC LIMIT ?3",
                )
                .bind(rid)
                .bind(kind.as_str())
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(rid), None) => {
                sqlx::query(
                    "SELECT thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at
                     FROM chat_threads WHERE run_id = ?1
                     ORDER BY created_at DESC LIMIT ?2",
                )
                .bind(rid)
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(kind)) => {
                sqlx::query(
                    "SELECT thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at
                     FROM chat_threads WHERE thread_kind = ?1
                     ORDER BY created_at DESC LIMIT ?2",
                )
                .bind(kind.as_str())
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at
                     FROM chat_threads ORDER BY created_at DESC LIMIT ?1",
                )
                .bind(limit as i64)
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.into_iter().map(parse_chat_thread).collect()
    }

    /// Apply a PackChat envelope received from an external transport.
    ///
    /// This is intentionally idempotent so a Twilight Bark subscriber can replay
    /// append-only bus events into the local SQLite replica without duplicating
    /// threads or messages.
    pub async fn ingest_wire_envelope(&self, envelope: &PackChatWireEnvelope) -> Result<()> {
        match envelope.event.kind {
            PackChatBusEventKind::ThreadOpened | PackChatBusEventKind::ThreadRosterSynced => {
                self.ingest_remote_thread(&envelope.event).await
            }
            PackChatBusEventKind::MessageAppended => {
                self.ingest_remote_message(&envelope.event).await
            }
            PackChatBusEventKind::CheckpointResolved => Ok(()),
        }
    }

    async fn ingest_remote_thread(&self, event: &PackChatBusEvent) -> Result<()> {
        let Some(thread_id) = event.thread_id.as_deref() else {
            return Ok(());
        };
        let now = event.emitted_at;
        let title = event
            .metadata_json
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Remote PackChat thread");
        let status = event
            .metadata_json
            .get("thread_status")
            .and_then(Value::as_str)
            .unwrap_or("open");
        let thread_kind = event
            .metadata_json
            .get("thread_kind")
            .and_then(Value::as_str)
            .unwrap_or("general");

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO chat_threads
                (thread_id, run_id, spec_id, title, status, thread_kind, metadata_json, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(thread_id)
        .bind(&event.run_id)
        .bind(&event.spec_id)
        .bind(title)
        .bind(status)
        .bind(thread_kind)
        .bind(serde_json::to_string(&event.metadata_json)?)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("ingest remote chat_thread")?;

        Ok(())
    }

    async fn ingest_remote_message(&self, event: &PackChatBusEvent) -> Result<()> {
        let Some(thread_id) = event.thread_id.as_deref() else {
            return Ok(());
        };
        let Some(message_id) = event.message_id.as_deref() else {
            return Ok(());
        };

        self.ingest_remote_thread(event).await?;
        let content = event.content.as_deref().unwrap_or(&event.content_preview);

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO chat_messages
                (message_id, thread_id, role, agent, agent_runtime_id, content, checkpoint_id, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(message_id)
        .bind(thread_id)
        .bind(event.role.as_deref().unwrap_or("system"))
        .bind(&event.agent)
        .bind(&event.agent_runtime_id)
        .bind(content)
        .bind(&event.checkpoint_id)
        .bind(event.emitted_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("ingest remote chat_message")?;

        sqlx::query("UPDATE chat_threads SET updated_at = ?1 WHERE thread_id = ?2")
            .bind(event.emitted_at.to_rfc3339())
            .bind(thread_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn ensure_run_thread(&self, run_id: &str, title: &str) -> Result<ChatThread> {
        if let Some(thread) = self
            .list_threads(Some(run_id), Some(&ChatThreadKind::Run), 1)
            .await?
            .into_iter()
            .next()
        {
            return Ok(thread);
        }

        let thread = self
            .open_thread(&OpenThreadRequest {
                run_id: Some(run_id.to_string()),
                spec_id: None,
                title: Some(title.to_string()),
                thread_kind: ChatThreadKind::Run,
                metadata_json: Some(serde_json::json!({
                    "surface": "pack_coordination",
                    "active_dogs": [],
                })),
            })
            .await?;

        self.append_message(
            &thread.thread_id,
            "system",
            None,
            None,
            "Pack coordination thread opened. Canonical dogs may have multiple live runtime instances here (for example `mason#codex` and `mason#claude`) while still coordinating as Mason.",
            None,
        )
        .await?;

        Ok(thread)
    }

    pub async fn sync_runtime_roster(
        &self,
        thread_id: &str,
        runtimes: &[AgentRuntimeState],
    ) -> Result<()> {
        let Some(thread) = self.get_thread(thread_id).await? else {
            return Ok(());
        };

        let mut metadata = match thread.metadata_json {
            Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        metadata.insert(
            "surface".to_string(),
            Value::String("pack_coordination".to_string()),
        );
        metadata.insert("active_dogs".to_string(), serde_json::to_value(runtimes)?);

        let updated_at = Utc::now();
        let metadata_value = Value::Object(metadata.clone());
        sqlx::query(
            "UPDATE chat_threads SET metadata_json = ?1, updated_at = ?2 WHERE thread_id = ?3",
        )
        .bind(serde_json::to_string(&metadata_value)?)
        .bind(updated_at.to_rfc3339())
        .bind(thread_id)
        .execute(&self.pool)
        .await
        .context("update chat_thread metadata")?;

        let synced_thread = ChatThread {
            updated_at,
            metadata_json: metadata_value,
            ..thread
        };
        self.publish_bus_event(
            self.build_thread_event(&synced_thread, PackChatBusEventKind::ThreadRosterSynced),
        );

        Ok(())
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    pub async fn append_message(
        &self,
        thread_id: &str,
        role: &str,
        agent: Option<&str>,
        agent_runtime_id: Option<&str>,
        content: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<ChatMessage> {
        let message_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO chat_messages (message_id, thread_id, role, agent, agent_runtime_id, content, checkpoint_id, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(&message_id)
        .bind(thread_id)
        .bind(role)
        .bind(agent)
        .bind(agent_runtime_id)
        .bind(content)
        .bind(checkpoint_id)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert chat_message")?;

        // Keep thread updated_at current.
        sqlx::query("UPDATE chat_threads SET updated_at = ?1 WHERE thread_id = ?2")
            .bind(now.to_rfc3339())
            .bind(thread_id)
            .execute(&self.pool)
            .await?;

        let message = ChatMessage {
            message_id,
            thread_id: thread_id.to_string(),
            role: role.to_string(),
            agent: agent.map(|s| s.to_string()),
            agent_runtime_id: agent_runtime_id.map(|s| s.to_string()),
            content: content.to_string(),
            checkpoint_id: checkpoint_id.map(|s| s.to_string()),
            created_at: now,
        };
        if let Ok(Some(thread)) = self.get_thread(thread_id).await {
            self.publish_bus_event(self.build_message_event(&thread, &message));
        }
        Ok(message)
    }

    pub async fn list_messages(&self, thread_id: &str) -> Result<Vec<ChatMessage>> {
        let rows = sqlx::query(
            "SELECT message_id, thread_id, role, agent, agent_runtime_id, content, checkpoint_id, created_at
             FROM chat_messages WHERE thread_id = ?1 ORDER BY created_at ASC",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| ChatMessage {
                message_id: r.get("message_id"),
                thread_id: r.get("thread_id"),
                role: r.get("role"),
                agent: r.get("agent"),
                agent_runtime_id: r.get("agent_runtime_id"),
                content: r.get("content"),
                checkpoint_id: r.get("checkpoint_id"),
                created_at: parse_dt(r.get("created_at")),
            })
            .collect())
    }

    // ── Checkpoints ───────────────────────────────────────────────────────────

    /// Load open checkpoints for a run, materialised as structured records.
    #[allow(dead_code)] // called from api checkpoint reply handler
    pub async fn list_open_checkpoints(&self, run_id: &str) -> Result<Vec<RunCheckpointRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT checkpoint_id, run_id, phase, agent, checkpoint_type, status,
                   prompt, context_json, created_at, resolved_at
            FROM run_checkpoints
            WHERE run_id = ?1 AND status = 'open'
            ORDER BY created_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::new();
        for r in rows {
            let checkpoint_id: String = r.get("checkpoint_id");
            let answers = self.load_checkpoint_answers(&checkpoint_id).await?;
            records.push(RunCheckpointRecord {
                checkpoint_id,
                run_id: r.get("run_id"),
                phase: r.get("phase"),
                agent: r.get("agent"),
                checkpoint_type: r.get("checkpoint_type"),
                status: r.get("status"),
                prompt: r.get("prompt"),
                context_json: serde_json::from_str(r.get::<String, _>("context_json").as_str())
                    .unwrap_or(serde_json::Value::Object(Default::default())),
                created_at: parse_dt(r.get("created_at")),
                resolved_at: r
                    .get::<Option<String>, _>("resolved_at")
                    .map(|s| parse_dt(s)),
                answers,
            });
        }
        Ok(records)
    }

    /// Persist a checkpoint answer and mark the checkpoint resolved.
    pub async fn reply_to_checkpoint(
        &self,
        checkpoint_id: &str,
        req: &CheckpointReplyRequest,
    ) -> Result<CheckpointAnswerRecord> {
        let answer_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let answered_by = req
            .answered_by
            .clone()
            .unwrap_or_else(|| "operator".to_string());

        sqlx::query(
            r#"
            INSERT INTO checkpoint_answers (answer_id, checkpoint_id, answered_by, answer_text, decision_json, created_at)
            VALUES (?1, ?2, ?3, ?4, '{}', ?5)
            "#,
        )
        .bind(&answer_id)
        .bind(checkpoint_id)
        .bind(&answered_by)
        .bind(&req.answer_text)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert checkpoint_answer")?;

        sqlx::query(
            "UPDATE run_checkpoints SET status = 'resolved', resolved_at = ?1 WHERE checkpoint_id = ?2",
        )
        .bind(now.to_rfc3339())
        .bind(checkpoint_id)
        .execute(&self.pool)
        .await?;

        self.publish_bus_event(self.build_checkpoint_event(
            checkpoint_id,
            &answered_by,
            &req.answer_text,
        ));

        Ok(CheckpointAnswerRecord {
            answer_id,
            checkpoint_id: checkpoint_id.to_string(),
            answered_by,
            answer_text: req.answer_text.clone(),
            decision_json: None,
            created_at: now,
        })
    }

    async fn load_checkpoint_answers(
        &self,
        checkpoint_id: &str,
    ) -> Result<Vec<CheckpointAnswerRecord>> {
        let rows = sqlx::query(
            "SELECT answer_id, checkpoint_id, answered_by, answer_text, decision_json, created_at
             FROM checkpoint_answers WHERE checkpoint_id = ?1 ORDER BY created_at ASC",
        )
        .bind(checkpoint_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| CheckpointAnswerRecord {
                answer_id: r.get("answer_id"),
                checkpoint_id: r.get("checkpoint_id"),
                answered_by: r.get("answered_by"),
                answer_text: r.get("answer_text"),
                decision_json: r
                    .get::<Option<String>, _>("decision_json")
                    .and_then(|s| serde_json::from_str(&s).ok()),
                created_at: parse_dt(r.get("created_at")),
            })
            .collect())
    }
}

// ── Agent routing ─────────────────────────────────────────────────────────────

/// Extract the target agent from message content.
///
/// Looks for a leading `@name` mention. Known agents: scout, mason, piper,
/// bramble, sable, ash, flint, coobie, keeper, plus user-facing aliases such
/// as `@storm` for Piper and `@bear` for Keeper.
/// Defaults to `"coobie"` when no mention is found.
pub fn route_message(content: &str) -> &'static str {
    let lower = content.to_lowercase();
    if lower.contains("@storm") {
        return "piper";
    }
    if lower.contains("@bear") {
        return "keeper";
    }
    for agent in &[
        "scout", "mason", "piper", "bramble", "sable", "ash", "flint", "keeper", "coobie",
    ] {
        if lower.contains(&format!("@{}", agent)) {
            return agent;
        }
    }
    "coobie"
}

/// Build a system prompt for a named agent responding in PackChat context.
fn agent_system_prompt(agent: &str, thread: &ChatThread) -> String {
    let run_ctx = thread
        .run_id
        .as_deref()
        .map(|id| format!(" You are currently assisting with run `{}`.", id))
        .unwrap_or_default();

    let role = match agent {
        "scout" => {
            "spec intake specialist — you parse specs, identify ambiguity, and produce intent packages"
        }
        "mason" => {
            "implementation specialist — you generate and modify code inside the staged workspace"
        }
        "piper" => {
            "tool and MCP routing specialist — you run build tools and fetch documentation"
        }
        "bramble" => {
            "test specialist — you generate tests, run lint/build/visible tests, and report results"
        }
        "sable" => {
            "scenario evaluation specialist — you execute hidden behavioral scenarios and produce eval reports"
        }
        "ash" => {
            "digital twin specialist — you provision simulated environments and mock external dependencies"
        }
        "flint" => "artifact specialist — you collect outputs and package artifact bundles",
        "keeper" => {
            "boundary enforcement specialist — you guard policy, protect secrets, and manage file-claim coordination"
        }
        _ => "memory and reasoning specialist — you retrieve prior patterns, causal history, and lessons learned",
    };

    let mut prompt = format!(
        "You are {}, a {}, working inside Harkonnen Labs — a local-first, spec-driven AI software factory.{} You share the Labrador Retriever personality: loyal, honest, persistent, never bluff. Keep answers concise and grounded in what you know. If you're uncertain, say so clearly.",
        agent_display(agent), role, run_ctx
    );

    if thread.thread_kind == ChatThreadKind::OperatorModel {
        let scope = thread
            .metadata_json
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("project");
        let pending_layer = thread
            .metadata_json
            .get("pending_layer")
            .and_then(Value::as_str)
            .unwrap_or("operating_rhythms");
        let project_root = thread
            .metadata_json
            .get("project_root")
            .and_then(Value::as_str)
            .unwrap_or("");

        prompt.push_str("

This thread is an operator-model activation interview. Your job is to elicit concrete, reusable operating detail that Harkonnen can stamp into the commissioned repo as structured operator-model artifacts.");
        prompt.push_str("
Ask 1-3 focused questions at a time. Prefer recurring triggers, decision rules, dependencies, approval boundaries, and real examples over generic biography.");
        prompt.push_str("
Treat the operator as the source of truth. Do not say a layer is approved, complete, or persisted unless the operator clearly confirms it.");
        prompt.push_str("
Summarize what you learned before moving on, and call out any ambiguity or missing operational detail explicitly.");
        prompt.push_str(&format!(
            "
Current scope: `{}`. Current layer: `{}`.",
            scope, pending_layer
        ));
        if !project_root.is_empty() {
            prompt.push_str(&format!(
                "
Target project root: `{}`.",
                project_root
            ));
        }
        if scope == "project" {
            prompt.push_str("
Bias toward repo-specific workflows and decision logic. Use only light, stable global defaults when the operator explicitly frames something as cross-project.");
        }
    }

    prompt
}

fn agent_display(agent: &str) -> &'static str {
    match agent {
        "scout" => "Scout",
        "mason" => "Mason",
        "piper" => "Storm",
        "bramble" => "Bramble",
        "sable" => "Sable",
        "ash" => "Ash",
        "flint" => "Flint",
        "keeper" => "Bear",
        _ => "Coobie",
    }
}

fn synthetic_chat_thread(run_id: Option<&str>) -> ChatThread {
    ChatThread {
        thread_id: "synthetic-packchat-thread".to_string(),
        run_id: run_id.map(str::to_string),
        spec_id: None,
        title: "Synthetic PackChat thread".to_string(),
        status: "open".to_string(),
        thread_kind: ChatThreadKind::General,
        metadata_json: default_thread_metadata(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

// ── Message dispatch ──────────────────────────────────────────────────────────

/// Post an operator message to a thread, route it to the appropriate agent,
/// generate a reply, and persist both sides.
pub async fn dispatch_message(
    store: &ChatStore,
    paths: &Paths,
    thread: &ChatThread,
    req: &PostMessageRequest,
) -> Result<PostMessageResponse> {
    // Determine routing — explicit override wins, then @mention, then default.
    let agent = req
        .agent
        .as_deref()
        .unwrap_or_else(|| route_message(&req.content));

    // Persist operator message.
    let operator_msg = store
        .append_message(
            &thread.thread_id,
            "operator",
            Some(agent),
            None,
            &req.content,
            None,
        )
        .await?;

    // Build conversation history for multi-turn context.
    let history = store.list_messages(&thread.thread_id).await?;
    // Generate agent reply.
    let agent_reply = generate_agent_reply(agent, &req.content, &history, thread, paths).await;

    let reply_msg = match agent_reply {
        Some(reply_content) => {
            let msg = store
                .append_message(
                    &thread.thread_id,
                    "agent",
                    Some(agent),
                    None,
                    &reply_content,
                    None,
                )
                .await?;
            Some(msg)
        }
        None => None,
    };

    Ok(PostMessageResponse {
        operator_message: operator_msg,
        agent_reply: reply_msg,
    })
}

pub async fn complete_agent_reply(
    agent: &str,
    user_content: &str,
    history: &[ChatMessage],
    run_id: Option<&str>,
    paths: &Paths,
) -> Result<String> {
    let thread = synthetic_chat_thread(run_id);
    complete_agent_reply_for_thread(agent, user_content, history, &thread, paths).await
}

async fn complete_agent_reply_for_thread(
    agent: &str,
    user_content: &str,
    history: &[ChatMessage],
    thread: &ChatThread,
    paths: &Paths,
) -> Result<String> {
    let provider = llm::build_provider(agent, "default", &paths.setup).with_context(|| {
        format!(
            "no configured provider available for PackChat agent {}",
            agent
        )
    })?;

    let prior_history = history
        .iter()
        .take(history.len().saturating_sub(1))
        .cloned()
        .collect::<Vec<_>>();
    let query_terms = retrieval_terms(user_content);
    let trimmed_user_content = user_content.trim();
    let user_message = if trimmed_user_content.is_empty() {
        "Please respond to the latest operator message using the available conversation context."
    } else {
        trimmed_user_content
    };

    let mut history_budget = PACKCHAT_HISTORY_CHAR_BUDGET;
    let mut excerpt_budget = PACKCHAT_MESSAGE_EXCERPT_CHARS;
    let mut assistant_budget = PACKCHAT_ASSISTANT_CONTEXT_CHARS;

    loop {
        let selected_history =
            select_relevant_history(&prior_history, user_content, history_budget, excerpt_budget);

        let mut system = agent_system_prompt(agent, thread);
        system.push_str(
            "\n\nPrefer explicit user-stated facts, previously confirmed preferences, and concrete operator details over generic assistant prose.",
        );
        if !prior_history.is_empty() && selected_history.len() < prior_history.len() {
            system.push_str(
                "\n\nThe conversation thread is longer than the context shown below. The supplied history has been trimmed to the most relevant and most recent slices for the current question.",
            );
        }

        let mut prior_messages: Vec<Message> = Vec::new();
        let mut leading_assistant_context = Vec::new();

        for msg in selected_history {
            let role = if msg.role == "operator" {
                "user"
            } else {
                "assistant"
            };
            let content = compact_history_message(&msg.content, &query_terms, excerpt_budget);

            if prior_messages.is_empty() && role == "assistant" {
                leading_assistant_context.push(content);
                continue;
            }

            if let Some(last) = prior_messages.last_mut() {
                if last.role == role {
                    if !last.content.is_empty() {
                        last.content.push_str("\n\n");
                    }
                    last.content.push_str(&content);
                    continue;
                }
            }

            prior_messages.push(Message {
                role: role.to_string(),
                content,
            });
        }

        if !leading_assistant_context.is_empty() {
            let assistant_context = compact_history_message(
                &leading_assistant_context.join("\n\n"),
                &query_terms,
                assistant_budget,
            );
            system.push_str("\n\nConversation context from earlier assistant turns:\n");
            system.push_str(&assistant_context);
        }

        let mut messages = vec![Message::system(system)];
        messages.extend(prior_messages);

        if let Some(last) = messages.last_mut() {
            if last.role == "user" {
                if !last.content.is_empty() {
                    last.content.push_str("\n\n");
                }
                last.content.push_str(user_message);
            } else {
                messages.push(Message::user(user_message));
            }
        } else {
            messages.push(Message::user(user_message));
        }

        let req = LlmRequest {
            messages,
            max_tokens: 1024,
            temperature: 0.3,
        };

        match provider.complete(req).await {
            Ok(resp) => return Ok(resp.content),
            Err(err) if is_context_window_error(&err) => {
                let next_history_budget =
                    (history_budget / 2).max(PACKCHAT_MIN_HISTORY_CHAR_BUDGET);
                let next_excerpt_budget =
                    (excerpt_budget / 2).max(PACKCHAT_MIN_MESSAGE_EXCERPT_CHARS);
                let next_assistant_budget =
                    (assistant_budget / 2).max(PACKCHAT_MIN_ASSISTANT_CONTEXT_CHARS);
                if next_history_budget == history_budget
                    && next_excerpt_budget == excerpt_budget
                    && next_assistant_budget == assistant_budget
                {
                    return Err(err)
                        .with_context(|| format!("PackChat agent reply failed for {}", agent));
                }
                tracing::warn!(
                    "PackChat context overflow for {} - retrying with history budget {} -> {}, excerpt {} -> {}, assistant {} -> {}",
                    agent,
                    history_budget,
                    next_history_budget,
                    excerpt_budget,
                    next_excerpt_budget,
                    assistant_budget,
                    next_assistant_budget
                );
                history_budget = next_history_budget;
                excerpt_budget = next_excerpt_budget;
                assistant_budget = next_assistant_budget;
            }
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("PackChat agent reply failed for {}", agent));
            }
        }
    }
}

async fn generate_agent_reply(
    agent: &str,
    user_content: &str,
    history: &[ChatMessage],
    thread: &ChatThread,
    paths: &Paths,
) -> Option<String> {
    match complete_agent_reply_for_thread(agent, user_content, history, thread, paths).await {
        Ok(content) => Some(content),
        Err(e) => {
            tracing::warn!("PackChat agent reply failed for {} ({})", agent, e);
            None
        }
    }
}

fn select_relevant_history(
    history: &[ChatMessage],
    user_content: &str,
    history_budget: usize,
    excerpt_budget: usize,
) -> Vec<ChatMessage> {
    if history.len() <= PACKCHAT_RECENT_MESSAGE_COUNT + 2 {
        return history.to_vec();
    }

    let query_terms = retrieval_terms(user_content);
    let recent_start = history.len().saturating_sub(PACKCHAT_RECENT_MESSAGE_COUNT);
    let mut selected = BTreeSet::new();

    for idx in recent_start..history.len() {
        selected.insert(idx);
    }

    let mut scored = history
        .iter()
        .enumerate()
        .map(|(idx, msg)| (score_history_message(msg, user_content, &query_terms), idx))
        .collect::<Vec<_>>();
    scored.sort_by(|(left_score, left_idx), (right_score, right_idx)| {
        right_score.cmp(left_score).then(right_idx.cmp(left_idx))
    });

    for (_, idx) in scored
        .into_iter()
        .filter(|(score, _)| *score > 0)
        .take(PACKCHAT_RELEVANT_MESSAGE_LIMIT)
    {
        let start = idx.saturating_sub(PACKCHAT_CONTEXT_NEIGHBOR_WINDOW);
        let end = (idx + PACKCHAT_CONTEXT_NEIGHBOR_WINDOW).min(history.len().saturating_sub(1));
        for neighbor in start..=end {
            selected.insert(neighbor);
        }
    }

    trim_selected_history_to_budget(
        history,
        &selected.into_iter().collect::<Vec<_>>(),
        user_content,
        &query_terms,
        history_budget,
        excerpt_budget,
    )
}

fn trim_selected_history_to_budget(
    history: &[ChatMessage],
    selected_indices: &[usize],
    user_content: &str,
    query_terms: &[String],
    history_budget: usize,
    excerpt_budget: usize,
) -> Vec<ChatMessage> {
    let recent_start = history.len().saturating_sub(PACKCHAT_RECENT_MESSAGE_COUNT);
    let mut candidates = selected_indices
        .iter()
        .copied()
        .map(|idx| {
            let excerpt =
                compact_history_message(&history[idx].content, query_terms, excerpt_budget);
            let excerpt_chars = excerpt.chars().count();
            let mut priority = score_history_message(&history[idx], user_content, query_terms);
            if idx >= recent_start {
                priority += 1_000;
            }
            if history[idx].role == "operator" {
                priority += 25;
            }
            (priority, idx, excerpt_chars)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(
        |(left_priority, left_idx, _), (right_priority, right_idx, _)| {
            right_priority
                .cmp(left_priority)
                .then(right_idx.cmp(left_idx))
        },
    );

    let mut kept = BTreeSet::new();
    let mut used_chars = 0usize;
    for (_, idx, excerpt_chars) in candidates {
        let message_cost = excerpt_chars + 24;
        if kept.is_empty() || used_chars + message_cost <= history_budget {
            kept.insert(idx);
            used_chars += message_cost;
        }
    }

    if kept.is_empty() && !history.is_empty() {
        kept.insert(history.len() - 1);
    }

    kept.into_iter().map(|idx| history[idx].clone()).collect()
}

fn score_history_message(msg: &ChatMessage, user_content: &str, query_terms: &[String]) -> i64 {
    let normalized_query = normalize_retrieval_text(user_content);
    let normalized_content = normalize_retrieval_text(&msg.content);
    if normalized_content.is_empty() {
        return 0;
    }

    let mut score = 0i64;
    if !normalized_query.is_empty() && normalized_content.contains(&normalized_query) {
        score += 80;
    }

    let overlap = query_terms
        .iter()
        .filter(|term| normalized_content.contains(term.as_str()))
        .count() as i64;
    score += overlap * 18;

    if msg.role == "operator" {
        score += 6;
    }
    if looks_like_user_fact(msg) {
        score += 14;
    }
    if normalized_content.contains("remember") {
        score += 4;
    }

    score
}

fn looks_like_user_fact(msg: &ChatMessage) -> bool {
    if msg.role != "operator" {
        return false;
    }

    let normalized = normalize_retrieval_text(&msg.content);
    [
        "i am",
        "im",
        "i was",
        "i have",
        "i had",
        "i graduated",
        "i work",
        "i live",
        "i like",
        "i love",
        "i prefer",
        "i booked",
        "my favorite",
        "my name",
        "my degree",
        "my job",
        "my birthday",
        "i finally",
    ]
    .iter()
    .any(|pattern| normalized.contains(pattern))
}

fn retrieval_terms(query: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "a", "an", "and", "are", "at", "be", "did", "do", "does", "for", "from", "had", "have",
        "how", "i", "if", "in", "is", "it", "my", "of", "on", "or", "that", "the", "to", "was",
        "what", "when", "where", "which", "who", "why", "with", "would", "you", "your",
    ];

    let mut terms = Vec::new();
    for token in normalize_retrieval_text(query).split_whitespace() {
        if token.len() < 3 || STOPWORDS.contains(&token) {
            continue;
        }
        if !terms.iter().any(|existing| existing == token) {
            terms.push(token.to_string());
        }
    }
    terms
}

fn normalize_retrieval_text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut prev_space = false;
    for ch in value.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            ' '
        };
        if mapped == ' ' {
            if !prev_space && !out.is_empty() {
                out.push(' ');
            }
            prev_space = true;
        } else {
            out.push(mapped);
            prev_space = false;
        }
    }
    out.trim().to_string()
}

fn compact_history_message(content: &str, query_terms: &[String], max_chars: usize) -> String {
    let trimmed = content.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }

    let lower = trimmed.to_ascii_lowercase();
    let mut best_match = None;
    for term in query_terms {
        if let Some(idx) = lower.find(term) {
            best_match = match best_match {
                Some(current) if current <= idx => Some(current),
                _ => Some(idx),
            };
        }
    }

    if let Some(byte_idx) = best_match {
        let char_idx = trimmed[..byte_idx].chars().count();
        let half_window = max_chars / 2;
        let start = char_idx.saturating_sub(half_window / 2);
        let end = (start + max_chars).min(trimmed.chars().count());
        let excerpt = slice_chars(trimmed, start, end).trim().to_string();
        let mut output = String::new();
        if start > 0 {
            output.push_str("...");
        }
        output.push_str(&excerpt);
        if end < trimmed.chars().count() {
            output.push_str("...");
        }
        return output;
    }

    let head_chars = max_chars / 2;
    let tail_chars = max_chars.saturating_sub(head_chars + 3);
    format!(
        "{}...{}",
        take_first_chars(trimmed, head_chars).trim_end(),
        take_last_chars(trimmed, tail_chars).trim_start()
    )
}

fn take_first_chars(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

fn take_last_chars(text: &str, max_chars: usize) -> String {
    let total = text.chars().count();
    if total <= max_chars {
        return text.to_string();
    }
    text.chars().skip(total - max_chars).collect()
}

fn slice_chars(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

fn is_context_window_error(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("context length")
        || text.contains("context window")
        || text.contains("context size")
        || text.contains("has been exceeded")
        || text.contains("n_keep")
        || text.contains("n_ctx")
        || text.contains("too many tokens")
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn default_thread_metadata() -> Value {
    Value::Object(Default::default())
}

fn build_calvin_ingress_event(event: &PackChatBusEvent) -> Option<CalvinIngressEvent> {
    if event.kind != PackChatBusEventKind::MessageAppended {
        return None;
    }
    let message_id = event.message_id.clone()?;
    let narrative_source = event
        .content
        .as_deref()
        .unwrap_or(event.content_preview.as_str());
    let narrative_summary = format!(
        "PackChat {} message{}{}: {}",
        event.role.as_deref().unwrap_or("unknown"),
        event
            .agent
            .as_deref()
            .map(|agent| format!(" addressed to or from {agent}"))
            .unwrap_or_default(),
        event
            .run_id
            .as_deref()
            .map(|run_id| format!(" during run {run_id}"))
            .unwrap_or_default(),
        preview_chat_content(narrative_source, 480)
    );
    let mut evidence_refs = vec![
        CalvinEvidenceRef {
            ref_type: "packchat_event".to_string(),
            id: event.event_id.clone(),
        },
        CalvinEvidenceRef {
            ref_type: "packchat_message".to_string(),
            id: message_id.clone(),
        },
    ];
    if let Some(thread_id) = event.thread_id.as_ref() {
        evidence_refs.push(CalvinEvidenceRef {
            ref_type: "packchat_thread".to_string(),
            id: thread_id.clone(),
        });
    }
    Some(CalvinIngressEvent {
        schema: "harkonnen.calvin.ingress.v1".to_string(),
        source_event_id: event.event_id.clone(),
        run_id: event.run_id.clone(),
        thread_id: event.thread_id.clone(),
        message_id: Some(message_id),
        agent_id: event.agent.clone(),
        agent_runtime_id: event.agent_runtime_id.clone(),
        chamber: calvin_chamber_for_packchat_event(event).to_string(),
        candidate_kind: "experience".to_string(),
        narrative_summary,
        evidence_refs,
        confidence: 0.55,
        operator_review_required: true,
    })
}

fn calvin_chamber_for_packchat_event(event: &PackChatBusEvent) -> &'static str {
    match event.role.as_deref() {
        Some("operator") => "pathos",
        Some("agent") => "praxis",
        Some("system") => "logos",
        _ => "episteme",
    }
}

#[cfg(unix)]
fn publish_packchat_to_twilight_socket(
    socket_path: &std::path::Path,
    agent_name: &str,
    agent_role: &str,
    envelope: &PackChatWireEnvelope,
) -> Result<()> {
    use std::io::BufReader;
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(socket_path)
        .with_context(|| format!("connecting to Twilight daemon at {:?}", socket_path))?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(750)));
    let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(750)));
    let mut reader = BufReader::new(stream.try_clone()?);

    write_ipc_json(
        &mut stream,
        serde_json::json!({
            "cmd": "register",
            "name": agent_name,
            "role": agent_role,
        }),
    )?;
    let registration = read_ipc_json(&mut reader)?;
    if registration["ok"].as_bool() == Some(false) || registration["agent_uuid"].is_null() {
        anyhow::bail!("Twilight daemon rejected registration: {registration}");
    }

    write_ipc_json(
        &mut stream,
        serde_json::json!({
            "cmd": "publish_task",
            "operation": "harkonnen.packchat.event",
            "input_json": serde_json::to_string(envelope)?,
        }),
    )?;
    let response = read_ipc_json(&mut reader)?;
    if response["ok"].as_bool() == Some(false) {
        anyhow::bail!("Twilight daemon rejected PackChat event: {response}");
    }
    Ok(())
}

#[cfg(not(unix))]
fn publish_packchat_to_twilight_socket(
    _socket_path: &std::path::Path,
    _agent_name: &str,
    _agent_role: &str,
    _envelope: &PackChatWireEnvelope,
) -> Result<()> {
    anyhow::bail!("Twilight PackChat bridge currently requires a Unix daemon socket")
}

#[cfg(unix)]
fn write_ipc_json(stream: &mut std::os::unix::net::UnixStream, value: Value) -> Result<()> {
    serde_json::to_writer(&mut *stream, &value)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

#[cfg(unix)]
fn read_ipc_json(reader: &mut std::io::BufReader<std::os::unix::net::UnixStream>) -> Result<Value> {
    use std::io::BufRead;

    let mut line = String::new();
    let read = reader.read_line(&mut line)?;
    if read == 0 {
        anyhow::bail!("Twilight daemon closed IPC socket");
    }
    serde_json::from_str(line.trim()).context("parsing Twilight daemon IPC response")
}

#[cfg(unix)]
fn spawn_twilight_ingest_loop(
    store: ChatStore,
    socket_path: PathBuf,
    agent_name: String,
    agent_role: String,
) {
    tokio::spawn(async move {
        loop {
            if let Err(error) =
                run_twilight_ingest_once(&store, &socket_path, &agent_name, &agent_role).await
            {
                tracing::warn!(
                    socket = %socket_path.display(),
                    error = %error,
                    "Twilight PackChat ingest loop disconnected"
                );
            }
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    });
}

#[cfg(not(unix))]
fn spawn_twilight_ingest_loop(
    _store: ChatStore,
    _socket_path: PathBuf,
    _agent_name: String,
    _agent_role: String,
) {
    tracing::warn!("Twilight PackChat ingest loop requires a Unix daemon socket");
}

#[cfg(unix)]
async fn run_twilight_ingest_once(
    store: &ChatStore,
    socket_path: &std::path::Path,
    agent_name: &str,
    agent_role: &str,
) -> Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connecting to Twilight daemon at {:?}", socket_path))?;
    let (read_half, mut write_half) = stream.into_split();
    let mut lines = BufReader::new(read_half).lines();

    write_async_ipc_json(
        &mut write_half,
        serde_json::json!({
            "cmd": "register",
            "name": agent_name,
            "role": agent_role,
        }),
    )
    .await?;
    let registration = read_async_ipc_json(&mut lines).await?;
    if registration["ok"].as_bool() == Some(false) || registration["agent_uuid"].is_null() {
        anyhow::bail!("Twilight daemon rejected ingest registration: {registration}");
    }

    write_async_ipc_json(
        &mut write_half,
        serde_json::json!({"cmd": "subscribe_tasks"}),
    )
    .await?;
    let subscribed = read_async_ipc_json(&mut lines).await?;
    if subscribed["ok"].as_bool() != Some(true) {
        anyhow::bail!("Twilight daemon rejected ingest subscription: {subscribed}");
    }

    while let Some(line) = lines.next_line().await? {
        let message: Value = serde_json::from_str(line.trim()).context("parsing Twilight event")?;
        if message["event"].as_str() != Some("task_request")
            || message["operation"].as_str() != Some("harkonnen.packchat.event")
        {
            continue;
        }
        let task_id = message["task_id"].as_str().unwrap_or_default().to_string();
        let input_json = message["input_json"].as_str().unwrap_or("{}");
        let envelope: PackChatWireEnvelope =
            serde_json::from_str(input_json).context("parsing PackChat wire envelope")?;
        let outcome = match store.ingest_wire_envelope(&envelope).await {
            Ok(()) => serde_json::json!({"ingested": true, "event_id": envelope.event.event_id}),
            Err(error) => {
                tracing::warn!(error = %error, "PackChat wire envelope ingest failed");
                serde_json::json!({"ingested": false, "error": error.to_string()})
            }
        };
        if !task_id.is_empty() {
            write_async_ipc_json(
                &mut write_half,
                serde_json::json!({
                    "cmd": "reply_task",
                    "task_id": task_id,
                    "output_json": outcome.to_string(),
                    "success": outcome["ingested"].as_bool().unwrap_or(false),
                }),
            )
            .await?;
            let _ = read_async_ipc_json(&mut lines).await?;
        }
    }

    Ok(())
}

#[cfg(unix)]
async fn write_async_ipc_json(
    writer: &mut tokio::net::unix::OwnedWriteHalf,
    value: Value,
) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let mut line = serde_json::to_string(&value)?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

#[cfg(unix)]
async fn read_async_ipc_json(
    lines: &mut tokio::io::Lines<tokio::io::BufReader<tokio::net::unix::OwnedReadHalf>>,
) -> Result<Value> {
    let Some(line) = lines.next_line().await? else {
        anyhow::bail!("Twilight daemon closed IPC socket");
    };
    serde_json::from_str(line.trim()).context("parsing Twilight daemon IPC response")
}

fn parse_chat_thread(row: sqlx::sqlite::SqliteRow) -> Result<ChatThread> {
    Ok(ChatThread {
        thread_id: row.get("thread_id"),
        run_id: row.get("run_id"),
        spec_id: row.get("spec_id"),
        title: row.get("title"),
        status: row.get("status"),
        thread_kind: parse_thread_kind(row.get("thread_kind"))?,
        metadata_json: row
            .get::<Option<String>, _>("metadata_json")
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_else(default_thread_metadata),
        created_at: parse_dt(row.get("created_at")),
        updated_at: parse_dt(row.get("updated_at")),
    })
}

fn parse_thread_kind(value: String) -> Result<ChatThreadKind> {
    match value.as_str() {
        "general" => Ok(ChatThreadKind::General),
        "run" => Ok(ChatThreadKind::Run),
        "spec" => Ok(ChatThreadKind::Spec),
        "operator_model" => Ok(ChatThreadKind::OperatorModel),
        other => anyhow::bail!("unknown chat thread kind: {other}"),
    }
}

fn parse_dt(s: String) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn preview_chat_content(content: &str, max_chars: usize) -> String {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut chars = trimmed.chars();
    let mut preview = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn msg(id: &str, role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            message_id: id.to_string(),
            thread_id: "thread-1".to_string(),
            role: role.to_string(),
            agent: Some("coobie".to_string()),
            agent_runtime_id: None,
            content: content.to_string(),
            checkpoint_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn relevant_history_prefers_fact_bearing_user_turns() {
        let history = vec![
            msg("1", "agent", "Welcome to the thread."),
            msg(
                "2",
                "operator",
                "Can you help me with organizing kitchen cabinets?",
            ),
            msg("3", "agent", "Use bins and labels for your pantry."),
            msg(
                "4",
                "operator",
                "I graduated with a degree in Business Administration.",
            ),
            msg(
                "5",
                "agent",
                "That sounds like a strong foundation for work.",
            ),
            msg("6", "operator", "What apps help with errands?"),
            msg("7", "agent", "Todoist and Trello are common picks."),
            msg("8", "operator", "Please remember my pantry question too."),
            msg("9", "agent", "I will remember that."),
            msg("10", "operator", "Also, I booked a train for Saturday."),
            msg("11", "agent", "Nice, have a safe trip."),
        ];

        let selected = select_relevant_history(
            &history,
            "What degree did I graduate with?",
            PACKCHAT_HISTORY_CHAR_BUDGET,
            PACKCHAT_MESSAGE_EXCERPT_CHARS,
        );
        assert!(selected
            .iter()
            .any(|message| message.content.contains("Business Administration")));
    }

    #[test]
    fn compact_history_message_centers_relevant_excerpt() {
        let content = format!(
            "{} Business Administration {}",
            "intro ".repeat(300),
            "tail ".repeat(300)
        );
        let excerpt = compact_history_message(
            &content,
            &["business".to_string(), "administration".to_string()],
            160,
        );
        assert!(excerpt.contains("Business Administration"));
        assert!(excerpt.starts_with("...") || excerpt.ends_with("..."));
    }

    #[test]
    fn local_jsonl_packchat_bus_writes_event_envelope() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("packchat-bus.jsonl");
        let bus = LocalJsonlPackChatBus::new(path.clone(), "lm-studio-local");
        let event = PackChatBusEvent {
            event_id: "evt-1".to_string(),
            topic: "chat/thread-1/message".to_string(),
            kind: PackChatBusEventKind::MessageAppended,
            setup_name: String::new(),
            thread_id: Some("thread-1".to_string()),
            run_id: Some("run-1".to_string()),
            spec_id: None,
            message_id: Some("msg-1".to_string()),
            checkpoint_id: None,
            role: Some("operator".to_string()),
            agent: Some("coobie".to_string()),
            agent_runtime_id: None,
            content_preview: "hello".to_string(),
            content: Some("hello".to_string()),
            metadata_json: serde_json::json!({"thread_kind": "run"}),
            emitted_at: Utc::now(),
        };

        bus.publish(&event).expect("publish");

        let raw = std::fs::read_to_string(&path).expect("read jsonl");
        assert!(raw.contains("\"event_id\":\"evt-1\""));
        assert!(raw.contains("\"setup_name\":\"lm-studio-local\""));
        assert!(raw.contains("\"topic\":\"harkonnen/lm-studio-local/chat/thread-1/message\""));
    }

    #[test]
    fn packchat_wire_envelope_carries_calvin_ingress_contract() {
        let event = PackChatBusEvent {
            event_id: "evt-2".to_string(),
            topic: "harkonnen/home-linux/chat/thread-1/message".to_string(),
            kind: PackChatBusEventKind::MessageAppended,
            setup_name: "home-linux".to_string(),
            thread_id: Some("thread-1".to_string()),
            run_id: Some("run-1".to_string()),
            spec_id: Some("spec-1".to_string()),
            message_id: Some("msg-1".to_string()),
            checkpoint_id: None,
            role: Some("agent".to_string()),
            agent: Some("coobie".to_string()),
            agent_runtime_id: Some("coobie#home-linux".to_string()),
            content_preview: "Coobie found a reusable pattern.".to_string(),
            content: Some("Coobie found a reusable pattern in the run evidence.".to_string()),
            metadata_json: serde_json::json!({"thread_kind": "run"}),
            emitted_at: Utc::now(),
        };

        let envelope = PackChatWireEnvelope::from_event(event);

        assert_eq!(envelope.schema, "harkonnen.packchat.v1");
        assert_eq!(envelope.causality.correlation_id.as_deref(), Some("run-1"));
        let archive = envelope.archive_contract.expect("archive contract");
        assert_eq!(archive.schema, "harkonnen.calvin.ingress.v1");
        assert_eq!(archive.chamber, "praxis");
        assert_eq!(archive.candidate_kind, "experience");
        assert!(archive.operator_review_required);
        assert!(archive
            .evidence_refs
            .iter()
            .any(|reference| reference.ref_type == "packchat_message" && reference.id == "msg-1"));
    }

    #[tokio::test]
    async fn ingest_wire_envelope_writes_remote_message_once() {
        let pool = SqlitePool::connect("sqlite::memory:").await.expect("pool");
        sqlx::query(
            r#"
            CREATE TABLE chat_threads (
                thread_id TEXT PRIMARY KEY,
                run_id TEXT,
                spec_id TEXT,
                title TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL DEFAULT 'open',
                thread_kind TEXT NOT NULL DEFAULT 'general',
                metadata_json TEXT NOT NULL DEFAULT '{}',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("threads table");
        sqlx::query(
            r#"
            CREATE TABLE chat_messages (
                message_id TEXT PRIMARY KEY,
                thread_id TEXT NOT NULL REFERENCES chat_threads(thread_id),
                role TEXT NOT NULL,
                agent TEXT,
                agent_runtime_id TEXT,
                content TEXT NOT NULL,
                checkpoint_id TEXT,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(&pool)
        .await
        .expect("messages table");

        let store = ChatStore::new(pool.clone());
        let event = PackChatBusEvent {
            event_id: "evt-3".to_string(),
            topic: "harkonnen/work-windows/chat/thread-remote/message".to_string(),
            kind: PackChatBusEventKind::MessageAppended,
            setup_name: "work-windows".to_string(),
            thread_id: Some("thread-remote".to_string()),
            run_id: Some("run-remote".to_string()),
            spec_id: None,
            message_id: Some("msg-remote".to_string()),
            checkpoint_id: None,
            role: Some("operator".to_string()),
            agent: Some("coobie".to_string()),
            agent_runtime_id: None,
            content_preview: "remote hello".to_string(),
            content: Some("remote hello from Twilight".to_string()),
            metadata_json: serde_json::json!({"thread_kind": "run", "title": "Remote"}),
            emitted_at: Utc::now(),
        };
        let envelope = PackChatWireEnvelope::from_event(event);

        store
            .ingest_wire_envelope(&envelope)
            .await
            .expect("first ingest");
        store
            .ingest_wire_envelope(&envelope)
            .await
            .expect("second ingest");

        let messages = store
            .list_messages("thread-remote")
            .await
            .expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "remote hello from Twilight");
    }
}
