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
use std::collections::BTreeSet;
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

const PACKCHAT_RECENT_MESSAGE_COUNT: usize = 6;
const PACKCHAT_RELEVANT_MESSAGE_LIMIT: usize = 10;
const PACKCHAT_CONTEXT_NEIGHBOR_WINDOW: usize = 1;
const PACKCHAT_HISTORY_CHAR_BUDGET: usize = 18_000;
const PACKCHAT_MIN_HISTORY_CHAR_BUDGET: usize = 3_000;
const PACKCHAT_MESSAGE_EXCERPT_CHARS: usize = 1_200;
const PACKCHAT_MIN_MESSAGE_EXCERPT_CHARS: usize = 300;
const PACKCHAT_ASSISTANT_CONTEXT_CHARS: usize = 2_400;
const PACKCHAT_MIN_ASSISTANT_CONTEXT_CHARS: usize = 600;

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
        let selected_history = select_relevant_history(
            &prior_history,
            user_content,
            history_budget,
            excerpt_budget,
        );

        let mut system = agent_system_prompt(agent, run_id);
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
                let next_history_budget = (history_budget / 2).max(PACKCHAT_MIN_HISTORY_CHAR_BUDGET);
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
            let excerpt = compact_history_message(
                &history[idx].content,
                query_terms,
                excerpt_budget,
            );
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
    candidates.sort_by(|(left_priority, left_idx, _), (right_priority, right_idx, _)| {
        right_priority
            .cmp(left_priority)
            .then(right_idx.cmp(left_idx))
    });

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
        "a", "an", "and", "are", "at", "be", "did", "do", "does", "for", "from",
        "had", "have", "how", "i", "if", "in", "is", "it", "my", "of", "on",
        "or", "that", "the", "to", "was", "what", "when", "where", "which", "who",
        "why", "with", "would", "you", "your",
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

fn parse_dt(s: String) -> chrono::DateTime<Utc> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            message_id: id.to_string(),
            thread_id: "thread-1".to_string(),
            role: role.to_string(),
            agent: Some("coobie".to_string()),
            content: content.to_string(),
            checkpoint_id: None,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn relevant_history_prefers_fact_bearing_user_turns() {
        let history = vec![
            msg("1", "agent", "Welcome to the thread."),
            msg("2", "operator", "Can you help me with organizing kitchen cabinets?"),
            msg("3", "agent", "Use bins and labels for your pantry."),
            msg("4", "operator", "I graduated with a degree in Business Administration."),
            msg("5", "agent", "That sounds like a strong foundation for work."),
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
}
