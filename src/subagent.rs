//! Sub-agent dispatch layer — Phase 5-C3.
//!
//! Routes high-context orchestrator tasks (Coobie briefing construction, Sable
//! scenario evaluation) to the configured backend. `DirectLlm` is a behavioral
//! no-op vs. the prior inline LLM calls; `ClaudeCodeAgent` runs the same call
//! with an isolation-enforcing system prompt so context never spills back into
//! the orchestrator.
//!
//! Resolution order per task:
//!   1. Agent profile `dispatch.<task>` (most specific)
//!   2. `[sub_agents.<task_name>]` in harkonnen.toml
//!   3. `[sub_agents] default_mode`

use std::collections::HashMap;
use std::time::Instant;

use crate::setup::{SetupConfig, SubAgentConfig, SubAgentTaskConfig};

// ─────────────────────────────────────────────────────────────────────────────
// Backend variants
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum SubAgentBackend {
    /// Calls the existing inline LLM path — behavioral no-op. Default.
    DirectLlm,
    /// Isolated LLM call with a clean context window and a system prompt that
    /// forbids writes to memory / SQLite / Calvin. Phase 5b upgrades this to a
    /// genuine sub-process spawn via `harkonnen mcp serve`.
    ClaudeCodeAgent { model: String, max_turns: u32 },
    /// Codex plan-mode for failure diagnosis — routes through the OpenAI
    /// completions endpoint with a structured chain-of-thought prompt.
    CodexPlanAgent {
        model: String,
        context_paths: Vec<String>,
    },
    /// Gemini routable agent.
    GeminiAgent { model: String },
}

impl SubAgentBackend {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DirectLlm => "direct_llm",
            Self::ClaudeCodeAgent { .. } => "claude_code_agent",
            Self::CodexPlanAgent { .. } => "codex_plan_agent",
            Self::GeminiAgent { .. } => "gemini_agent",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Result type
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub output: String,
    pub backend_used: String,
    pub tokens_used: Option<u32>,
    pub duration_ms: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Dispatcher
// ─────────────────────────────────────────────────────────────────────────────

pub struct SubAgentDispatcher {
    config: SubAgentConfig,
    setup: SetupConfig,
}

impl std::fmt::Debug for SubAgentDispatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SubAgentDispatcher")
            .field("default_mode", &self.config.default_mode)
            .finish()
    }
}

impl Clone for SubAgentDispatcher {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            setup: self.setup.clone(),
        }
    }
}

impl SubAgentDispatcher {
    pub fn new(config: SubAgentConfig, setup: SetupConfig) -> Self {
        Self { config, setup }
    }

    /// Resolve the backend for a task, following the three-tier resolution order.
    ///
    /// `profile_dispatch` comes from the agent's YAML `dispatch:` block.
    pub fn backend_for(
        &self,
        task_name: &str,
        profile_dispatch: Option<&HashMap<String, SubAgentTaskConfig>>,
    ) -> SubAgentBackend {
        // 1. Profile dispatch block takes priority.
        if let Some(profile_task) = profile_dispatch.and_then(|d| d.get(task_name)) {
            return Self::task_config_to_backend(profile_task);
        }
        // 2. harkonnen.toml [sub_agents.<task_name>].
        if let Some(task_cfg) = self.config.tasks.get(task_name) {
            return Self::task_config_to_backend(task_cfg);
        }
        // 3. [sub_agents] default_mode.
        Self::mode_str_to_backend(&self.config.default_mode, None)
    }

    /// Returns true when the resolved backend provides context isolation.
    pub fn isolates(
        &self,
        task_name: &str,
        profile_dispatch: Option<&HashMap<String, SubAgentTaskConfig>>,
    ) -> bool {
        !matches!(
            self.backend_for(task_name, profile_dispatch),
            SubAgentBackend::DirectLlm
        )
    }

    /// Dispatch a task using the resolved backend.
    ///
    /// For `DirectLlm` the caller is expected to short-circuit and call the
    /// existing inline function — this path returns a sentinel so the caller
    /// knows not to use the result.
    ///
    /// For `ClaudeCodeAgent` and other isolation backends, the LLM is called
    /// with a clean context and the output is returned for the orchestrator to
    /// persist.
    pub async fn dispatch(
        &self,
        task_name: &str,
        user_prompt: String,
        system_override: Option<String>,
        profile_dispatch: Option<&HashMap<String, SubAgentTaskConfig>>,
    ) -> SubAgentResult {
        let backend = self.backend_for(task_name, profile_dispatch);
        let started = Instant::now();

        let (output, tokens) = match &backend {
            SubAgentBackend::DirectLlm => {
                // Caller should short-circuit before calling dispatch() for DirectLlm.
                // Return empty so the caller can detect and fall through to inline logic.
                (String::new(), None)
            }
            SubAgentBackend::ClaudeCodeAgent { model, .. } => {
                self.call_isolated_llm(model, system_override, &user_prompt)
                    .await
            }
            SubAgentBackend::CodexPlanAgent { model, .. } => {
                self.call_isolated_llm(model, system_override, &user_prompt)
                    .await
            }
            SubAgentBackend::GeminiAgent { model } => {
                self.call_isolated_llm(model, system_override, &user_prompt)
                    .await
            }
        };

        SubAgentResult {
            output,
            backend_used: backend.label().to_string(),
            tokens_used: tokens,
            duration_ms: started.elapsed().as_millis() as u64,
        }
    }

    /// Call the LLM with an isolation-enforcing system prompt.
    /// Uses the same `llm::build_provider` infrastructure as the inline path,
    /// but with a clean context window and write-prohibition in the system prompt.
    async fn call_isolated_llm(
        &self,
        model: &str,
        system_override: Option<String>,
        user_prompt: &str,
    ) -> (String, Option<u32>) {
        let system = system_override.unwrap_or_else(isolation_system_prompt);

        // Map model name to a provider tier name defined in harkonnen.toml.
        let provider_name = if model.contains("opus") {
            "claude-opus"
        } else if model.contains("haiku") {
            "claude-haiku"
        } else if model.contains("sonnet") || model.contains("claude") {
            "claude-sonnet"
        } else if model.starts_with("gpt") || model.starts_with("o4") || model.starts_with("o3") {
            "codex"
        } else if model.starts_with("gemini") {
            "gemini"
        } else {
            self.setup.providers.default.as_str()
        };

        let Some(provider) = crate::llm::build_provider("subagent", provider_name, &self.setup)
        else {
            tracing::warn!(model = %model, "SubAgentDispatcher: no provider available for isolated call");
            return (String::new(), None);
        };

        let request = crate::llm::LlmRequest::simple(system, user_prompt);
        match provider.complete(request).await {
            Ok(resp) => {
                let tokens = resp.usage.map(|u| u.input_tokens + u.output_tokens);
                (resp.content, tokens)
            }
            Err(e) => {
                tracing::warn!(error = %e, "SubAgentDispatcher: isolated LLM call failed");
                (String::new(), None)
            }
        }
    }

    fn task_config_to_backend(cfg: &SubAgentTaskConfig) -> SubAgentBackend {
        Self::mode_str_to_backend(&cfg.backend, cfg.model.as_deref())
    }

    fn mode_str_to_backend(mode: &str, model_override: Option<&str>) -> SubAgentBackend {
        match mode {
            "claude_code_agent" => SubAgentBackend::ClaudeCodeAgent {
                model: model_override.unwrap_or("claude-sonnet-4-6").to_string(),
                max_turns: 6,
            },
            "codex_plan_agent" => SubAgentBackend::CodexPlanAgent {
                model: model_override.unwrap_or("gpt-4o").to_string(),
                context_paths: vec![],
            },
            "gemini_agent" => SubAgentBackend::GeminiAgent {
                model: model_override.unwrap_or("gemini-2.0-flash").to_string(),
            },
            _ => SubAgentBackend::DirectLlm,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Isolation system prompt
// ─────────────────────────────────────────────────────────────────────────────

fn isolation_system_prompt() -> String {
    "You are a specialist sub-agent in the Harkonnen Labs factory pack.\n\
     \n\
     Operating constraints (non-negotiable):\n\
     - You may NOT write to memory, SQLite, or the Calvin Archive.\n\
     - You may NOT read from or write to factory/scenarios/ (Sable-only).\n\
     - You may NOT modify implementation code, test files, or workspace files.\n\
     - disallowed_tools: memory_store, db_write, workspace_write, artifact_writer, scenario_store\n\
     \n\
     Your output will be returned to the orchestrator, which decides what to persist.\n\
     Complete your task and stop. Do not take autonomous follow-up actions.\n\
     Apply the Labrador baseline: cooperative, signals uncertainty, attempts before withdrawal."
        .to_string()
}

// ─────────────────────────────────────────────────────────────────────────────
// Prompt builders (existing, unchanged)
// ─────────────────────────────────────────────────────────────────────────────

const LABRADOR_REMINDERS: &str = "\
You operate under the Labrador baseline. Key invariants:
- cooperative: work with the pack, not around it
- signals uncertainty: do not bluff; flag when you do not know
- attempts before withdrawal: try seriously before giving up
- escalates without becoming inert: get help when stuck rather than stalling";

pub fn scout_prompt(spec_path: &str, run_id: &str) -> String {
    format!(
        "You are Scout — spec intake specialist for Harkonnen Labs.\n\
        \n\
        {LABRADOR_REMINDERS}\n\
        \n\
        Your first action must be to invoke /scout with args: {spec_path}\n\
        Do not write implementation code.\n\
        \n\
        Context:\n\
        - Spec path: {spec_path}\n\
        - Run ID: {run_id}\n\
        \n\
        Stop condition: you are done when you have produced a complete intent \
        package — open questions identified, ambiguities flagged, and the \
        package written to the workspace for this run."
    )
}

pub fn coobie_briefing_prompt(run_id: &str, phase: &str, keywords: &[&str]) -> String {
    let kw = keywords.join(", ");
    format!(
        "You are Coobie — memory retriever and causal reasoner for Harkonnen Labs.\n\
        \n\
        {LABRADOR_REMINDERS}\n\
        \n\
        Your first action must be to invoke /coobie with args: briefing\n\
        Do not write implementation code.\n\
        \n\
        Context:\n\
        - Run ID: {run_id}\n\
        - Phase: {phase}\n\
        - Search keywords: {kw}\n\
        \n\
        Stop condition: you are done when you have emitted a structured briefing \
        covering what the factory knows, what remains open, causal guardrails, \
        and explicit checks for this run."
    )
}

pub fn sable_prompt(run_id: &str, artifact_path: &str) -> String {
    format!(
        "You are Sable — acceptance reviewer for Harkonnen Labs.\n\
        \n\
        {LABRADOR_REMINDERS}\n\
        \n\
        Your first action must be to invoke /sable with args: {run_id}\n\
        Do not write implementation code.\n\
        \n\
        Context:\n\
        - Run ID: {run_id}\n\
        - Primary artifact: {artifact_path}\n\
        \n\
        Stop condition: you are done when you have produced a complete eval \
        report scoring each scenario criterion as met, partial, or unmet, \
        with causal feedback ready for Coobie."
    )
}

pub fn keeper_prompt(action_description: &str, context: &str) -> String {
    format!(
        "You are Keeper — policy and boundary guardian for Harkonnen Labs.\n\
        \n\
        {LABRADOR_REMINDERS}\n\
        \n\
        Your first action must be to invoke /keeper with args: assess\n\
        Do not write implementation code or memory entries.\n\
        \n\
        Action under review: {action_description}\n\
        \n\
        Context:\n\
        {context}\n\
        \n\
        Stop condition: you are done when you have issued a clear policy \
        decision — in-bounds, out-of-bounds, or conditional with stated \
        requirements — and updated the claim record if coordination is needed."
    )
}

/// Build the Sable isolation system prompt — adds the hidden-scenario firewall
/// on top of the standard isolation constraints.
pub fn sable_isolation_system() -> String {
    format!(
        "{}\n\n\
         SABLE ISOLATION FIREWALL (non-negotiable):\n\
         - You may NOT access factory/scenarios/ or any content tagged:\n\
           implementation_notes, mason_plan, edit_rationale, fix_patterns\n\
         - Your evaluation must be based solely on the spec, the run artifacts,\n\
           and the scoped Sable briefing provided in this prompt.\n\
         - Any retrieved context tagged with the above labels must be dropped\n\
           before scoring, regardless of relevance.",
        isolation_system_prompt()
    )
}
