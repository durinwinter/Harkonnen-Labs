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
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::{
    config::Paths,
    llm::{self, LlmRequest, Message},
    models::{CheckpointAnswerRecord, RunCheckpointRecord},
};

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThread {
    pub thread_id: String,
    pub run_id: Option<String>,
    pub spec_id: Option<String>,
    pub title: String,
    pub status: String, // "open" | "closed"
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

// ── Chat store ────────────────────────────────────────────────────────────────

/// Thin persistence wrapper — all chat state lives in SQLite.
#[derive(Debug, Clone)]
pub struct ChatStore {
    pool: SqlitePool,
}

impl ChatStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    // ── Threads ───────────────────────────────────────────────────────────────

    pub async fn open_thread(&self, req: &OpenThreadRequest) -> Result<ChatThread> {
        let thread_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let title = req
            .title
            .clone()
            .unwrap_or_else(|| "New conversation".to_string());

        sqlx::query(
            r#"
            INSERT INTO chat_threads (thread_id, run_id, spec_id, title, status, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?6)
            "#,
        )
        .bind(&thread_id)
        .bind(&req.run_id)
        .bind(&req.spec_id)
        .bind(&title)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert chat_thread")?;

        Ok(ChatThread {
            thread_id,
            run_id: req.run_id.clone(),
            spec_id: req.spec_id.clone(),
            title,
            status: "open".to_string(),
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_thread(&self, thread_id: &str) -> Result<Option<ChatThread>> {
        let row = sqlx::query(
            "SELECT thread_id, run_id, spec_id, title, status, created_at, updated_at
             FROM chat_threads WHERE thread_id = ?1",
        )
        .bind(thread_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| ChatThread {
            thread_id: r.get("thread_id"),
            run_id: r.get("run_id"),
            spec_id: r.get("spec_id"),
            title: r.get("title"),
            status: r.get("status"),
            created_at: parse_dt(r.get("created_at")),
            updated_at: parse_dt(r.get("updated_at")),
        }))
    }

    pub async fn list_threads(
        &self,
        run_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ChatThread>> {
        let rows = if let Some(rid) = run_id {
            sqlx::query(
                "SELECT thread_id, run_id, spec_id, title, status, created_at, updated_at
                 FROM chat_threads WHERE run_id = ?1
                 ORDER BY created_at DESC LIMIT ?2",
            )
            .bind(rid)
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT thread_id, run_id, spec_id, title, status, created_at, updated_at
                 FROM chat_threads ORDER BY created_at DESC LIMIT ?1",
            )
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await?
        };

        Ok(rows
            .into_iter()
            .map(|r| ChatThread {
                thread_id: r.get("thread_id"),
                run_id: r.get("run_id"),
                spec_id: r.get("spec_id"),
                title: r.get("title"),
                status: r.get("status"),
                created_at: parse_dt(r.get("created_at")),
                updated_at: parse_dt(r.get("updated_at")),
            })
            .collect())
    }

    // ── Messages ──────────────────────────────────────────────────────────────

    pub async fn append_message(
        &self,
        thread_id: &str,
        role: &str,
        agent: Option<&str>,
        content: &str,
        checkpoint_id: Option<&str>,
    ) -> Result<ChatMessage> {
        let message_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO chat_messages (message_id, thread_id, role, agent, content, checkpoint_id, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(&message_id)
        .bind(thread_id)
        .bind(role)
        .bind(agent)
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

        Ok(ChatMessage {
            message_id,
            thread_id: thread_id.to_string(),
            role: role.to_string(),
            agent: agent.map(|s| s.to_string()),
            content: content.to_string(),
            checkpoint_id: checkpoint_id.map(|s| s.to_string()),
            created_at: now,
        })
    }

    pub async fn list_messages(&self, thread_id: &str) -> Result<Vec<ChatMessage>> {
        let rows = sqlx::query(
            "SELECT message_id, thread_id, role, agent, content, checkpoint_id, created_at
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
/// Looks for a leading `@name` mention.  Known agents: scout, mason, piper,
/// bramble, sable, ash, flint, coobie, keeper.
/// Defaults to `"coobie"` when no mention is found.
pub fn route_message(content: &str) -> &'static str {
    let lower = content.to_lowercase();
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
fn agent_system_prompt(agent: &str, run_id: Option<&str>) -> String {
    let run_ctx = run_id
        .map(|id| format!(" You are currently assisting with run `{}`.", id))
        .unwrap_or_default();

    let role = match agent {
        "scout"  => "spec intake specialist — you parse specs, identify ambiguity, and produce intent packages",
        "mason"  => "implementation specialist — you generate and modify code inside the staged workspace",
        "piper"  => "tool and MCP routing specialist — you run build tools and fetch documentation",
        "bramble"=> "test specialist — you generate tests, run lint/build/visible tests, and report results",
        "sable"  => "scenario evaluation specialist — you execute hidden behavioral scenarios and produce eval reports",
        "ash"    => "digital twin specialist — you provision simulated environments and mock external dependencies",
        "flint"  => "artifact specialist — you collect outputs and package artifact bundles",
        "keeper" => "boundary enforcement specialist — you guard policy, protect secrets, and manage file-claim coordination",
        _        => "memory and reasoning specialist — you retrieve prior patterns, causal history, and lessons learned",
    };

    format!(
        "You are {}, a {}, working inside Harkonnen Labs — a local-first, spec-driven AI software factory.{} \
         You share the Labrador Retriever personality: loyal, honest, persistent, never bluff. \
         Keep answers concise and grounded in what you know. If you're uncertain, say so clearly.",
        agent_display(agent), role, run_ctx
    )
}

fn agent_display(agent: &str) -> &'static str {
    match agent {
        "scout" => "Scout",
        "mason" => "Mason",
        "piper" => "Piper",
        "bramble" => "Bramble",
        "sable" => "Sable",
        "ash" => "Ash",
        "flint" => "Flint",
        "keeper" => "Keeper",
        _ => "Coobie",
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
            &req.content,
            None,
        )
        .await?;

    // Build conversation history for multi-turn context.
    let history = store.list_messages(&thread.thread_id).await?;
    let run_id = thread.run_id.as_deref();

    // Generate agent reply.
    let agent_reply = generate_agent_reply(agent, &req.content, &history, run_id, paths).await;

    let reply_msg = match agent_reply {
        Some(reply_content) => {
            let msg = store
                .append_message(
                    &thread.thread_id,
                    "agent",
                    Some(agent),
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
    let provider = llm::build_provider(agent, "default", &paths.setup).with_context(|| {
        format!(
            "no configured provider available for PackChat agent {}",
            agent
        )
    })?;

    let system = agent_system_prompt(agent, run_id);
    let mut messages = vec![Message::system(system)];

    for msg in history.iter().take(history.len().saturating_sub(1)) {
        let role = if msg.role == "operator" {
            "user"
        } else {
            "assistant"
        };
        messages.push(Message {
            role: role.to_string(),
            content: msg.content.clone(),
        });
    }
    messages.push(Message::user(user_content));

    let req = LlmRequest {
        messages,
        max_tokens: 1024,
        temperature: 0.3,
    };

    provider
        .complete(req)
        .await
        .map(|resp| resp.content)
        .with_context(|| format!("PackChat agent reply failed for {}", agent))
}

async fn generate_agent_reply(
    agent: &str,
    user_content: &str,
    history: &[ChatMessage],
    run_id: Option<&str>,
    paths: &Paths,
) -> Option<String> {
    match complete_agent_reply(agent, user_content, history, run_id, paths).await {
        Ok(content) => Some(content),
        Err(e) => {
            tracing::warn!("PackChat agent reply failed for {} ({})", agent, e);
            None
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn parse_dt(s: String) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}
