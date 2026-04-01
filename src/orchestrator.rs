use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    agents::{self, AgentProfile},
    coobie::CoobieReasoner,
    config::Paths,
    db,
    llm::{self, LlmRequest},
    memory::MemoryStore,
    models::{
        AgentExecution, BlackboardState, CoobieBriefing, EpisodeRecord,
        HiddenScenarioCheckResult, HiddenScenarioEvaluation, HiddenScenarioSummary,
        IntentPackage, LessonRecord, PriorCauseSignal, RunEvent, RunRecord, ScenarioResult,
        Spec, TwinEnvironment, TwinService, ValidationSummary,
    },
    pidgin,
    policy,
    scenarios,
    setup::command_available,
    spec, workspace,
};

#[derive(Debug, Clone)]
pub struct AppContext {
    pub paths: Paths,
    pub pool: SqlitePool,
    pub memory_store: MemoryStore,
    pub blackboard: Arc<RwLock<BlackboardState>>,
    pub coobie: crate::coobie::SqliteCoobie,
}

#[derive(Debug, Clone)]
pub struct FailureHarness {
    pub phase: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct RunRequest {
    pub spec_path: String,
    pub product: Option<String>,
    pub product_path: Option<String>,
    pub failure_harness: Option<FailureHarness>,
}

#[derive(Debug, Clone, Serialize)]
struct TargetGitMetadata {
    branch: Option<String>,
    commit: Option<String>,
    remote_origin: Option<String>,
    clean: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct TargetSourceMetadata {
    label: String,
    source_kind: String,
    source_path: String,
    git: Option<TargetGitMetadata>,
}

impl RunRequest {
    fn harness_message(&self, phase: &str) -> Option<&str> {
        self.failure_harness
            .as_ref()
            .filter(|harness| harness.phase == phase)
            .map(|harness| harness.message.as_str())
    }
}

#[derive(Debug, Clone)]
struct ExecutionOutput {
    validation: ValidationSummary,
    hidden_scenarios: HiddenScenarioSummary,
    run_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct CommandOutcome {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl AppContext {
    pub async fn bootstrap() -> Result<Self> {
        let paths = Paths::discover()?;
        tokio::fs::create_dir_all(&paths.factory).await?;
        tokio::fs::create_dir_all(&paths.logs).await?;
        tokio::fs::create_dir_all(&paths.artifacts).await?;
        tokio::fs::create_dir_all(&paths.workspaces).await?;
        tokio::fs::create_dir_all(&paths.memory).await?;
        tokio::fs::create_dir_all(&paths.specs).await?;
        tokio::fs::create_dir_all(&paths.scenarios).await?;
        tokio::fs::create_dir_all(&paths.products).await?;

        let pool = db::init_db(&paths).await?;
        let memory_store = MemoryStore::new(paths.memory.clone());
        let coobie = crate::coobie::SqliteCoobie::new(pool.clone());
        Ok(Self {
            paths,
            pool,
            memory_store,
            blackboard: Arc::new(RwLock::new(BlackboardState::default())),
            coobie,
        })
    }

    pub async fn start_run(&self, req: RunRequest) -> Result<RunRecord> {
        let spec_obj = spec::load_spec(&req.spec_path)?;
        let target_source = self.resolve_target_source(&req).await?;

        let run_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let log_path = self.run_log_path(&run_id);

        self.insert_run(&run_id, &spec_obj.id, &target_source.label, "queued", now)
            .await?;
        let _ = self
            .record_event(
                &run_id,
                None,
                "queued",
                "orchestrator",
                "queued",
                &format!(
                    "Created run for spec {} against target {} ({})",
                    spec_obj.id, target_source.label, target_source.source_path
                ),
                &log_path,
            )
            .await?;

        match self
            .execute_run(&run_id, &req, &spec_obj, &target_source, &log_path)
            .await
        {
            Ok(output) => {
                let final_status = if output.validation.passed && output.hidden_scenarios.passed {
                    "completed"
                } else {
                    "completed_with_issues"
                };
                self.update_run_status(&run_id, final_status).await?;

                let lessons = match self.consolidate_run(&run_id).await {
                    Ok(lessons) => lessons,
                    Err(error) => {
                        let _ = self
                            .record_event(
                                &run_id,
                                None,
                                "memory",
                                "coobie",
                                "warning",
                                &format!("Consolidation skipped: {error}"),
                                &log_path,
                            )
                            .await;
                        Vec::new()
                    }
                };
                self.attach_lessons_to_blackboard(&output.run_dir, &lessons)
                    .await?;

                let _ = self
                    .record_event(
                        &run_id,
                        None,
                        "complete",
                        "orchestrator",
                        if final_status == "completed" {
                            "complete"
                        } else {
                            "warning"
                        },
                        &format!("Run finished with status {}", final_status),
                        &log_path,
                    )
                    .await?;
                self.finalize_blackboard(final_status, &output.run_dir).await?;
                self.package_artifacts(&run_id).await?;
            }
            Err(error) => {
                let message = error.to_string();
                self.update_run_status(&run_id, "failed").await?;
                let _ = self
                    .record_event(
                        &run_id,
                        None,
                        "complete",
                        "orchestrator",
                        "failed",
                        &message,
                        &log_path,
                    )
                    .await?;
                let run_dir = self.run_dir(&run_id);
                self.mark_blackboard_failed(&message, &run_dir).await?;
                let lessons = match self.consolidate_run(&run_id).await {
                    Ok(lessons) => lessons,
                    Err(consolidation_error) => {
                        let _ = self
                            .record_event(
                                &run_id,
                                None,
                                "memory",
                                "coobie",
                                "warning",
                                &format!("Consolidation skipped: {consolidation_error}"),
                                &log_path,
                            )
                            .await;
                        Vec::new()
                    }
                };
                if run_dir.exists() {
                    self.attach_lessons_to_blackboard(&run_dir, &lessons).await?;
                }
                let _ = self.package_artifacts(&run_id).await;
            }
        }

        self.get_run(&run_id)
            .await?
            .with_context(|| format!("run not found after execution: {run_id}"))
    }

    async fn execute_run(
        &self,
        run_id: &str,
        req: &RunRequest,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        log_path: &Path,
    ) -> Result<ExecutionOutput> {
        let profiles = agents::load_profiles(&self.paths.factory.join("agents").join("profiles"))?;
        let workspace_root = workspace::create_run_workspace(&self.paths.workspaces, run_id).await?;
        let run_dir = workspace_root.join("run");
        let agents_dir = run_dir.join("agents");
        tokio::fs::create_dir_all(&agents_dir).await?;
        tokio::fs::copy(&req.spec_path, run_dir.join("spec.yaml"))
            .await
            .with_context(|| format!("copying spec snapshot for run {run_id}"))?;
        self.write_json_file(&run_dir.join("target_source.json"), target_source)
            .await?;

        let mut blackboard = BlackboardState {
            run_id: run_id.to_string(),
            current_phase: "queued".to_string(),
            active_goal: spec_obj.title.clone(),
            ..Default::default()
        };
        push_unique(&mut blackboard.artifact_refs, "target_source.json");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let mut agent_executions = Vec::new();
        let query_terms = build_coobie_query_terms(spec_obj, target_source);
        let domain_signals = infer_domain_signals(spec_obj, target_source, &query_terms);

        let memory_episode = self
            .start_episode(run_id, "memory", &format!("Coobie preflight for {}", spec_obj.id))
            .await?;
        blackboard.current_phase = "memory".to_string();
        blackboard.active_goal = format!("Coobie preflight for {}", spec_obj.title);
        claim_agent(&mut blackboard, "coobie", "retrieve prior context and emit causal briefing");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "memory").await?;
        let memory_start = self
            .record_event(
                run_id,
                Some(&memory_episode),
                "memory",
                "coobie",
                "running",
                "Retrieving prior context and preparing Coobie briefing",
                log_path,
            )
            .await?;
        let memory_hits = self.retrieve_coobie_memory_context(&query_terms).await?;
        let briefing = self
            .build_coobie_briefing(
                spec_obj,
                target_source,
                &query_terms,
                &domain_signals,
                &memory_hits,
            )
            .await?;
        tokio::fs::write(
            run_dir.join("memory_context.md"),
            format_memory_context(&memory_hits),
        )
        .await?;
        self.write_json_file(&run_dir.join("coobie_briefing.json"), &briefing)
            .await?;
        tokio::fs::write(
            run_dir.join("coobie_preflight_response.md"),
            &briefing.coobie_response,
        )
        .await?;
        push_unique(&mut blackboard.artifact_refs, "memory_context.md");
        push_unique(&mut blackboard.artifact_refs, "coobie_briefing.json");
        push_unique(&mut blackboard.artifact_refs, "coobie_preflight_response.md");
        self.write_agent_execution(
            &profiles,
            "coobie",
            &format!(
                "Retrieve prior context, infer domain risks, and produce a preflight causal briefing for spec '{}'.",
                spec_obj.title
            ),
            &format!(
                "Prepared Coobie preflight with {} memory hit(s), {} guardrail(s), and {} required check(s).",
                briefing.memory_hits.len(),
                briefing.recommended_guardrails.len(),
                briefing.required_checks.len()
            ),
            &briefing.coobie_response,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let memory_end = self
            .record_event(
                run_id,
                Some(&memory_episode),
                "memory",
                "coobie",
                "complete",
                &format!(
                    "Prepared Coobie briefing with {} memory hit(s) and {} required check(s)",
                    briefing.memory_hits.len(),
                    briefing.required_checks.len()
                ),
                log_path,
            )
            .await?;
        self.finish_episode(&memory_episode, "success", Some(1.0)).await?;
        self.link_events(memory_start.event_id, memory_end.event_id, "contributed_to", 1.0)
            .await?;
        release_agent(&mut blackboard, "coobie");
        push_unique(&mut blackboard.resolved_items, "memory");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let intake_episode = self
            .start_episode(run_id, "intake", &format!("Interpret spec {}", spec_obj.id))
            .await?;
        blackboard.current_phase = "intake".to_string();
        blackboard.active_goal = format!("Interpret spec {}", spec_obj.title);
        claim_agent(&mut blackboard, "scout", "interpret spec and normalize intent with Coobie context");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "intake").await?;
        let intake_start = self
            .record_event(
                run_id,
                Some(&intake_episode),
                "intake",
                "scout",
                "running",
                &format!("Loading spec '{}' with Coobie briefing", spec_obj.title),
                log_path,
            )
            .await?;
        let intent = self.scout_intake(spec_obj, &briefing).await?;
        self.write_json_file(&run_dir.join("intent.json"), &intent).await?;
        push_unique(&mut blackboard.artifact_refs, "intent.json");
        self.write_agent_execution(
            &profiles,
            "scout",
            &format!(
                "Interpret spec {} and prepare a normalized intent package from Coobie's preflight.",
                spec_obj.id
            ),
            "Parsed the spec and produced an implementation intent package anchored to Coobie's briefing.",
            &serde_json::to_string_pretty(&intent)?,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let intake_end = self
            .record_event(
                run_id,
                Some(&intake_episode),
                "intake",
                "scout",
                "complete",
                &format!(
                    "Intent package ready with {} recommended steps",
                    intent.recommended_steps.len()
                ),
                log_path,
            )
            .await?;
        self.finish_episode(&intake_episode, "success", Some(1.0)).await?;
        self.link_events(intake_start.event_id, intake_end.event_id, "contributed_to", 1.0)
            .await?;
        release_agent(&mut blackboard, "scout");
        push_unique(&mut blackboard.resolved_items, "intake");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let workspace_episode = self
            .start_episode(run_id, "workspace", "Verify and stage isolated workspace")
            .await?;
        blackboard.current_phase = "workspace".to_string();
        blackboard.active_goal = "Stage isolated product workspace".to_string();
        claim_agent(&mut blackboard, "keeper", "verify workspace boundaries");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "workspace").await?;
        let workspace_start = self
            .record_event(
                run_id,
                Some(&workspace_episode),
                "workspace",
                "keeper",
                "running",
                "Verifying workspace boundaries",
                log_path,
            )
            .await?;
        let staged_product = workspace::stage_target_workspace(
            Path::new(&target_source.source_path),
            &workspace_root,
        )
        .await?;
        policy::ensure_path_within(&workspace_root, &staged_product)?;
        self.write_agent_execution(
            &profiles,
            "keeper",
            "Verify that all product writes stay inside the staged run workspace.",
            "Workspace boundaries verified.",
            &format!(
                "workspace_root={}\nstaged_product={}\npolicy=within_workspace",
                workspace_root.display(),
                staged_product.display()
            ),
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let workspace_end = self
            .record_event(
                run_id,
                Some(&workspace_episode),
                "workspace",
                "keeper",
                "complete",
                "Workspace boundaries verified",
                log_path,
            )
            .await?;
        self.finish_episode(&workspace_episode, "success", Some(1.0)).await?;
        self.link_events(
            workspace_start.event_id,
            workspace_end.event_id,
            "contributed_to",
            1.0,
        )
        .await?;
        release_agent(&mut blackboard, "keeper");
        claim_agent(&mut blackboard, "mason", "owns staged product workspace");
        push_unique(&mut blackboard.resolved_items, "workspace");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let implementation_episode = self
            .start_episode(run_id, "implementation", &format!("Plan work for {}", target_source.label))
            .await?;
        blackboard.current_phase = "implementation".to_string();
        blackboard.active_goal = format!("Prepare implementation plan for {}", target_source.label);
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "implementation").await?;
        let implementation_start = self
            .record_event(
                run_id,
                Some(&implementation_episode),
                "implementation",
                "mason",
                "running",
                "Drafting implementation plan for the staged product",
                log_path,
            )
            .await?;
        let implementation_plan = self
            .mason_implementation_plan(spec_obj, &intent, &briefing, &staged_product, target_source)
            .await;
        tokio::fs::write(run_dir.join("implementation_plan.md"), &implementation_plan).await?;
        push_unique(&mut blackboard.artifact_refs, "implementation_plan.md");
        self.write_agent_execution(
            &profiles,
            "mason",
            &format!(
                "Prepare an implementation plan for target '{}' using the staged workspace.",
                target_source.label
            ),
            "Prepared a local implementation plan for the staged product copy.",
            &implementation_plan,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let implementation_end = self
            .record_event(
                run_id,
                Some(&implementation_episode),
                "implementation",
                "mason",
                "complete",
                &format!("Staged product copy at {}", staged_product.display()),
                log_path,
            )
            .await?;
        self.finish_episode(&implementation_episode, "success", Some(1.0))
            .await?;
        self.link_events(
            implementation_start.event_id,
            implementation_end.event_id,
            "contributed_to",
            1.0,
        )
        .await?;
        release_agent(&mut blackboard, "mason");
        push_unique(&mut blackboard.resolved_items, "implementation");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let tools_episode = self
            .start_episode(run_id, "tools", "Review tool and MCP availability")
            .await?;
        blackboard.current_phase = "tools".to_string();
        blackboard.active_goal = "Summarize tools and MCP surface".to_string();
        claim_agent(&mut blackboard, "piper", "review tools and MCP availability");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "tools").await?;
        let tools_start = self
            .record_event(
                run_id,
                Some(&tools_episode),
                "tools",
                "piper",
                "running",
                "Reviewing tool and MCP availability",
                log_path,
            )
            .await?;
        let tool_plan = self.piper_tool_plan(spec_obj, &briefing).await;
        tokio::fs::write(run_dir.join("tool_plan.md"), &tool_plan).await?;
        push_unique(&mut blackboard.artifact_refs, "tool_plan.md");
        self.write_agent_execution(
            &profiles,
            "piper",
            "Summarize the configured provider and MCP tool surface for this run.",
            "Captured current tool and MCP availability for the run.",
            &tool_plan,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let tools_end = self
            .record_event(
                run_id,
                Some(&tools_episode),
                "tools",
                "piper",
                "complete",
                "Tool and MCP plan captured",
                log_path,
            )
            .await?;
        self.finish_episode(&tools_episode, "success", Some(1.0)).await?;
        self.link_events(tools_start.event_id, tools_end.event_id, "contributed_to", 0.9)
            .await?;
        release_agent(&mut blackboard, "piper");
        push_unique(&mut blackboard.resolved_items, "tools");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let twin_episode = self
            .start_episode(run_id, "twin", "Provision local twin environment")
            .await?;
        blackboard.current_phase = "twin".to_string();
        blackboard.active_goal = "Provision local twin environment".to_string();
        claim_agent(&mut blackboard, "ash", "prepare twin environment");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "twin").await?;
        let twin_start = self
            .record_event(
                run_id,
                Some(&twin_episode),
                "twin",
                "ash",
                "running",
                "Provisioning local twin environment",
                log_path,
            )
            .await?;
        let twin = self.build_twin_environment(run_id, spec_obj);
        self.write_json_file(&run_dir.join("twin.json"), &twin).await?;
        if let Some(narrative) = self.ash_twin_narrative(spec_obj, &twin, &briefing).await {
            let _ = tokio::fs::write(run_dir.join("twin_narrative.md"), &narrative).await;
            push_unique(&mut blackboard.artifact_refs, "twin_narrative.md");
        }
        push_unique(&mut blackboard.artifact_refs, "twin.json");
        self.write_agent_execution(
            &profiles,
            "ash",
            "Provision a safe local twin environment for validation and hidden scenario work.",
            &format!("Provisioned {} twin service(s).", twin.services.len()),
            &serde_json::to_string_pretty(&twin)?,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let twin_end = self
            .record_event(
                run_id,
                Some(&twin_episode),
                "twin",
                "ash",
                "complete",
                &format!("Provisioned {} twin service(s)", twin.services.len()),
                log_path,
            )
            .await?;
        self.finish_episode(&twin_episode, "success", Some(1.0)).await?;
        self.link_events(twin_start.event_id, twin_end.event_id, "contributed_to", 1.0)
            .await?;
        release_agent(&mut blackboard, "ash");
        push_unique(&mut blackboard.resolved_items, "twin");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let validation_episode = self
            .start_episode(run_id, "validation", "Run visible validation")
            .await?;
        blackboard.current_phase = "validation".to_string();
        blackboard.active_goal = "Run visible validation".to_string();
        claim_agent(&mut blackboard, "bramble", "run visible validation");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "validation").await?;
        let validation_start = self
            .record_event(
                run_id,
                Some(&validation_episode),
                "validation",
                "bramble",
                "running",
                "Running visible validation",
                log_path,
            )
            .await?;
        let mut validation = self.run_visible_validation(&workspace_root, &staged_product, spec_obj).await?;
        if let Some(message) = req.harness_message("validation") {
            validation.passed = false;
            validation.results.push(ScenarioResult {
                scenario_id: "failure_harness".to_string(),
                passed: false,
                details: message.to_string(),
            });
        }
        self.write_json_file(&run_dir.join("validation.json"), &validation)
            .await?;
        push_unique(&mut blackboard.artifact_refs, "validation.json");
        if workspace_root.join("run").join("corpus_results.json").exists() {
            push_unique(&mut blackboard.artifact_refs, "corpus_results.json");
        }
        self.write_agent_execution(
            &profiles,
            "bramble",
            "Run the visible validation loop over the staged product copy.",
            &format!(
                "Visible validation finished with {} check(s).",
                validation.results.len()
            ),
            &serde_json::to_string_pretty(&validation)?,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let validation_outcome = if validation.passed { "success" } else { "failure" };
        let validation_message = if let Some(message) = req.harness_message("validation") {
            format!("Failure harness forced validation failure: {message}")
        } else {
            format!(
                "Visible validation finished: {} checks, {} passed",
                validation.results.len(),
                validation.results.iter().filter(|result| result.passed).count()
            )
        };
        let validation_end = self
            .record_event(
                run_id,
                Some(&validation_episode),
                "validation",
                "bramble",
                if validation.passed { "complete" } else { "warning" },
                &validation_message,
                log_path,
            )
            .await?;
        self.finish_episode(
            &validation_episode,
            validation_outcome,
            Some(if validation.passed { 1.0 } else { 0.5 }),
        )
        .await?;
        self.link_events(
            validation_start.event_id,
            validation_end.event_id,
            "contributed_to",
            if validation.passed { 1.0 } else { 0.5 },
        )
        .await?;
        release_agent(&mut blackboard, "bramble");
        if validation.passed {
            push_unique(&mut blackboard.resolved_items, "validation");
            remove_blocker(&mut blackboard, "visible_validation_failed");
        } else {
            push_unique(&mut blackboard.open_blockers, "visible_validation_failed");
        }
        // Bramble LLM interpretation — best-effort, non-blocking
        if let Some(analysis) = self
            .bramble_interpret_validation(spec_obj, &validation, &briefing)
            .await
        {
            let _ = tokio::fs::write(run_dir.join("validation_analysis.md"), &analysis).await;
            push_unique(&mut blackboard.artifact_refs, "validation_analysis.md");
        }
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let hidden_episode = self
            .start_episode(run_id, "hidden_scenarios", "Evaluate hidden scenarios")
            .await?;
        blackboard.current_phase = "hidden_scenarios".to_string();
        blackboard.active_goal = "Evaluate hidden scenarios".to_string();
        claim_agent(&mut blackboard, "sable", "evaluate hidden scenarios");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "hidden_scenarios").await?;
        let hidden_start = self
            .record_event(
                run_id,
                Some(&hidden_episode),
                "hidden_scenarios",
                "sable",
                "running",
                "Evaluating hidden scenarios",
                log_path,
            )
            .await?;
        let predicted_final_status = if validation.passed {
            "completed"
        } else {
            "completed_with_issues"
        };
        let events_so_far = self.list_run_events(run_id).await?;
        let hidden_definitions = scenarios::load_hidden_scenarios(&self.paths.scenarios, &spec_obj.id)?;
        let mut hidden_scenarios = scenarios::evaluate_hidden_scenarios(
            &hidden_definitions,
            predicted_final_status,
            &events_so_far,
            &validation,
            &twin,
            &agent_executions,
            &run_dir,
        );
        if let Some(message) = req.harness_message("hidden_scenarios") {
            hidden_scenarios.passed = false;
            hidden_scenarios.results.push(HiddenScenarioEvaluation {
                scenario_id: "failure-harness".to_string(),
                title: "Failure Harness".to_string(),
                passed: false,
                details: message.to_string(),
                checks: vec![HiddenScenarioCheckResult {
                    kind: "failure_harness".to_string(),
                    passed: false,
                    details: message.to_string(),
                }],
            });
        }
        self.write_json_file(&run_dir.join("hidden_scenarios.json"), &hidden_scenarios)
            .await?;
        push_unique(&mut blackboard.artifact_refs, "hidden_scenarios.json");
        self.write_agent_execution(
            &profiles,
            "sable",
            "Execute hidden scenarios from the protected scenario store and compare the run against them.",
            &format!("Hidden scenarios passed: {}", hidden_scenarios.passed),
            &serde_json::to_string_pretty(&hidden_scenarios)?,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let hidden_outcome = if hidden_scenarios.passed { "success" } else { "failure" };
        let hidden_message = if let Some(message) = req.harness_message("hidden_scenarios") {
            format!("Failure harness forced hidden scenario failure: {message}")
        } else {
            format!(
                "Hidden scenario evaluation finished: {} scenario(s)",
                hidden_scenarios.results.len()
            )
        };
        let hidden_end = self
            .record_event(
                run_id,
                Some(&hidden_episode),
                "hidden_scenarios",
                "sable",
                if hidden_scenarios.passed { "complete" } else { "warning" },
                &hidden_message,
                log_path,
            )
            .await?;
        self.finish_episode(
            &hidden_episode,
            hidden_outcome,
            Some(if hidden_scenarios.passed { 1.0 } else { 0.5 }),
        )
        .await?;
        self.link_events(
            hidden_start.event_id,
            hidden_end.event_id,
            "contributed_to",
            if hidden_scenarios.passed { 1.0 } else { 0.5 },
        )
        .await?;
        release_agent(&mut blackboard, "sable");
        if hidden_scenarios.passed {
            push_unique(&mut blackboard.resolved_items, "hidden_scenarios");
            remove_blocker(&mut blackboard, "hidden_scenarios_failed");
        } else {
            push_unique(&mut blackboard.open_blockers, "hidden_scenarios_failed");
        }
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let memory_store_episode = self
            .start_episode(run_id, "memory", "Store run summary back into long-term memory")
            .await?;
        claim_agent(&mut blackboard, "coobie", "store run summary and prepare future recall");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        let memory_store_start = self
            .record_event(
                run_id,
                Some(&memory_store_episode),
                "memory",
                "coobie",
                "running",
                "Storing run summary back into local memory",
                log_path,
            )
            .await?;
        self.memory_store
            .store(
                &format!("run-{}", run_id),
                vec![
                    "run".to_string(),
                    spec_obj.id.clone(),
                    target_source.label.clone(),
                    if validation.passed && hidden_scenarios.passed {
                        "completed".to_string()
                    } else {
                        "completed-with-issues".to_string()
                    },
                ],
                &format!("Run {} for {}", run_id, spec_obj.title),
                &format!(
                    "Spec: {}\nProduct: {}\nVisible validation passed: {}\nHidden scenarios passed: {}\nRecommended steps: {}\n\nTop memory hits:\n{}",
                    spec_obj.id,
                    target_source.label,
                    validation.passed,
                    hidden_scenarios.passed,
                    intent.recommended_steps.join(", "),
                    memory_hits.join("\n\n")
                ),
            )
            .await?;
        let memory_store_end = self
            .record_event(
                run_id,
                Some(&memory_store_episode),
                "memory",
                "coobie",
                "complete",
                "Stored run summary back into local memory",
                log_path,
            )
            .await?;
        self.finish_episode(&memory_store_episode, "success", Some(1.0)).await?;
        self.link_events(
            memory_store_start.event_id,
            memory_store_end.event_id,
            "corrected",
            0.8,
        )
        .await?;
        release_agent(&mut blackboard, "coobie");
        push_unique(&mut blackboard.resolved_items, "memory_store");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let artifacts_episode = self
            .start_episode(run_id, "artifacts", "Package run artifacts")
            .await?;
        blackboard.current_phase = "artifacts".to_string();
        blackboard.active_goal = "Refresh artifact bundle".to_string();
        claim_agent(&mut blackboard, "flint", "prepare artifact bundle");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        self.update_run_status(run_id, "artifacts").await?;
        let artifacts_start = self
            .record_event(
                run_id,
                Some(&artifacts_episode),
                "artifacts",
                "flint",
                "running",
                "Packaging run artifacts",
                log_path,
            )
            .await?;
        self.write_agent_execution(
            &profiles,
            "flint",
            "Collect outputs, logs, and evaluation evidence into a portable artifact bundle.",
            "Prepared bundle contents for packaging.",
            &list_run_directory(&run_dir)?.join("\n"),
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        self.package_artifacts(run_id).await?;
        let artifacts_end = self
            .record_event(
                run_id,
                Some(&artifacts_episode),
                "artifacts",
                "flint",
                "complete",
                "Artifact bundle refreshed",
                log_path,
            )
            .await?;
        self.finish_episode(&artifacts_episode, "success", Some(1.0)).await?;
        self.link_events(
            artifacts_start.event_id,
            artifacts_end.event_id,
            "contributed_to",
            1.0,
        )
        .await?;
        release_agent(&mut blackboard, "flint");
        push_unique(&mut blackboard.resolved_items, "artifacts");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        // ── Coobie: full causal ingest + report ───────────────────────────────
        let all_events = self.list_run_events(run_id).await.unwrap_or_default();
        let factory_episode = crate::models::FactoryEpisode {
            run_id: run_id.to_string(),
            product: target_source.label.clone(),
            spec_id: spec_obj.id.clone(),
            features: spec_obj.acceptance_criteria.clone(),
            agent_events: all_events,
            tool_events: vec![],
            twin_env: Some(twin.clone()),
            validation: Some(validation.clone()),
            scenarios: Some(hidden_scenarios.clone()),
            decision: None,
            created_at: Utc::now(),
        };
        if let Err(err) = self.coobie.ingest_episode(&factory_episode).await {
            tracing::warn!("Coobie ingest failed: {err}");
        } else {
            match self.coobie.emit_report(run_id).await {
                Ok(report) => {
                    let report_response = crate::coobie::render_coobie_report_response(&report);
                    let _ = self
                        .write_json_file(&run_dir.join("causal_report.json"), &report)
                        .await;
                    let _ = tokio::fs::write(
                        run_dir.join("coobie_report_response.md"),
                        &report_response,
                    )
                    .await;
                    let _ = tokio::fs::write(
                        run_dir.join("causal_summary.md"),
                        &report_response,
                    )
                    .await;
                    push_unique(&mut blackboard.artifact_refs, "causal_report.json");
                    push_unique(&mut blackboard.artifact_refs, "coobie_report_response.md");
                    push_unique(&mut blackboard.artifact_refs, "causal_summary.md");
                    let _ = self.sync_blackboard(&blackboard, Some(&run_dir)).await;
                }
                Err(err) => tracing::warn!("Coobie emit_report failed: {err}"),
            }
        }

        Ok(ExecutionOutput {
            validation,
            hidden_scenarios,
            run_dir,
        })
    }

    async fn retrieve_coobie_memory_context(&self, query_terms: &[String]) -> Result<Vec<String>> {
        let mut hits = Vec::new();
        let mut seen = HashSet::new();

        for term in query_terms {
            if term.trim().is_empty() {
                continue;
            }
            for hit in self.memory_store.retrieve_context(term).await? {
                if hit.contains("No memories found") || hit.contains("Memory not initialized") {
                    continue;
                }
                if seen.insert(hit.clone()) {
                    hits.push(hit);
                }
            }
        }

        if hits.is_empty() {
            Ok(vec![format!(
                "No memories found for Coobie preflight queries: {}",
                query_terms.join(", ")
            )])
        } else {
            hits.truncate(10);
            Ok(hits)
        }
    }

    async fn find_relevant_lessons(
        &self,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<LessonRecord>> {
        let lessons = self.list_lessons().await?;
        let mut scored = lessons
            .into_iter()
            .map(|lesson| {
                let haystack = format!(
                    "{} {} {}",
                    lesson.pattern.to_lowercase(),
                    lesson.tags.join(" ").to_lowercase(),
                    lesson.intervention.clone().unwrap_or_default().to_lowercase(),
                );
                let mut score = 0_i32;
                for term in query_terms {
                    let needle = term.to_lowercase();
                    if needle.len() >= 3 && haystack.contains(&needle) {
                        score += 3;
                    }
                }
                for signal in domain_signals {
                    if haystack.contains(&signal.to_lowercase()) {
                        score += 2;
                    }
                }
                if lesson.tags.iter().any(|tag| tag == "causal") {
                    score += 1;
                }
                (score, lesson)
            })
            .collect::<Vec<_>>();

        scored.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.created_at.cmp(&left.1.created_at))
        });

        let mut relevant = scored
            .iter()
            .filter(|(score, _)| *score > 0)
            .map(|(_, lesson)| lesson.clone())
            .take(5)
            .collect::<Vec<_>>();

        if relevant.is_empty() {
            relevant = scored
                .iter()
                .filter(|(_, lesson)| lesson.tags.iter().any(|tag| tag == "causal" || tag == "lesson"))
                .map(|(_, lesson)| lesson.clone())
                .take(3)
                .collect::<Vec<_>>();
        }

        Ok(relevant)
    }

    async fn summarize_prior_causes(&self, limit: usize) -> Result<Vec<PriorCauseSignal>> {
        #[derive(Debug)]
        struct CauseAggregate {
            description: String,
            occurrences: i64,
            scenario_successes: i64,
            last_seen_run_id: Option<String>,
            last_seen_at: Option<chrono::DateTime<Utc>>,
        }

        let rows = sqlx::query(
            r#"
            SELECT h.run_id, h.cause_id, h.description, h.created_at, s.scenario_passed
            FROM causal_hypotheses h
            LEFT JOIN coobie_episode_scores s ON s.run_id = h.run_id
            ORDER BY h.created_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut aggregates: HashMap<String, CauseAggregate> = HashMap::new();
        for row in rows {
            let cause_id = row.get::<String, _>("cause_id");
            let description = row.get::<String, _>("description");
            let run_id = row.get::<String, _>("run_id");
            let created_at = chrono::DateTime::parse_from_rfc3339(
                row.get::<String, _>("created_at").as_str(),
            )?
            .with_timezone(&Utc);
            let scenario_passed = row.get::<Option<i64>, _>("scenario_passed").unwrap_or(0) != 0;

            let entry = aggregates.entry(cause_id).or_insert_with(|| CauseAggregate {
                description,
                occurrences: 0,
                scenario_successes: 0,
                last_seen_run_id: Some(run_id.clone()),
                last_seen_at: Some(created_at),
            });
            entry.occurrences += 1;
            if scenario_passed {
                entry.scenario_successes += 1;
            }
            if entry.last_seen_at.is_none() {
                entry.last_seen_at = Some(created_at);
                entry.last_seen_run_id = Some(run_id);
            }
        }

        let mut signals = aggregates
            .into_iter()
            .map(|(cause_id, aggregate)| PriorCauseSignal {
                cause_id,
                description: aggregate.description,
                occurrences: aggregate.occurrences,
                scenario_pass_rate: if aggregate.occurrences > 0 {
                    aggregate.scenario_successes as f32 / aggregate.occurrences as f32
                } else {
                    0.0
                },
                last_seen_run_id: aggregate.last_seen_run_id,
                last_seen_at: aggregate.last_seen_at,
            })
            .collect::<Vec<_>>();

        signals.sort_by(|left, right| {
            right
                .occurrences
                .cmp(&left.occurrences)
                .then_with(|| right.last_seen_at.cmp(&left.last_seen_at))
        });
        signals.truncate(limit);
        Ok(signals)
    }

    async fn build_coobie_briefing(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
        memory_hits: &[String],
    ) -> Result<CoobieBriefing> {
        let relevant_lessons = self.find_relevant_lessons(query_terms, domain_signals).await?;
        let prior_causes = self.summarize_prior_causes(5).await?;
        let prior_report_count = sqlx::query(
            "SELECT COUNT(DISTINCT run_id) AS cnt FROM causal_hypotheses",
        )
        .fetch_one(&self.pool)
        .await?
        .get::<i64, _>("cnt") as usize;
        let application_risks = build_application_risks(spec_obj, domain_signals, memory_hits, &prior_causes);
        let environment_risks = build_environment_risks(spec_obj, domain_signals);
        let regulatory_considerations = build_regulatory_considerations(spec_obj, domain_signals);
        let recommended_guardrails = build_recommended_guardrails(spec_obj, domain_signals, memory_hits, &prior_causes);
        let required_checks = build_required_checks(spec_obj, domain_signals, &regulatory_considerations);
        let open_questions = build_coobie_open_questions(spec_obj, domain_signals, &regulatory_considerations);

        let mut briefing = CoobieBriefing {
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            query_terms: query_terms.to_vec(),
            domain_signals: domain_signals.to_vec(),
            prior_report_count,
            memory_hits: memory_hits.to_vec(),
            relevant_lessons,
            prior_causes,
            project_components: spec_obj.project_components.clone(),
            scenario_blueprint: spec_obj.scenario_blueprint.clone(),
            application_risks,
            environment_risks,
            regulatory_considerations,
            recommended_guardrails,
            required_checks,
            open_questions,
            coobie_response: String::new(),
            generated_at: Utc::now(),
        };
        briefing.coobie_response = crate::coobie::render_coobie_briefing_response(&briefing);
        Ok(briefing)
    }

    async fn scout_intake(&self, spec_obj: &Spec, briefing: &CoobieBriefing) -> Result<IntentPackage> {
        if let Some(provider) = llm::build_provider("scout", "claude", &self.paths.setup) {
            let memory_section = format_memory_context(&briefing.memory_hits);
            let briefing_json = serde_json::to_string_pretty(briefing).unwrap_or_default();
            let spec_yaml = serde_yaml::to_string(spec_obj).unwrap_or_else(|_| format!("{:?}", spec_obj));

            let req = LlmRequest::simple(
                "You are Scout, a spec-intake specialist for a software factory.                  Your job is to read a YAML spec and prior memory context, then produce a                  concise implementation intent package as JSON with these fields:                  spec_id (string), summary (one sentence), ambiguity_notes (array of strings —                  things that are unclear or missing), recommended_steps (ordered array of strings).                  Respond with valid JSON only — no markdown, no explanation.",
                format!(
                    "SPEC:
```yaml
{spec_yaml}
```

PRIOR MEMORY:
{memory_section}

COOBIE BRIEFING:
```json
{briefing_json}
```

COOBIE RESPONSE:
{response}

                     Produce the intent package JSON and incorporate Coobie guardrails, required checks, and open questions.",
                    response = briefing.coobie_response,
                ),
            );

            match provider.complete(req).await {
                Ok(resp) => {
                    if let Ok(parsed) = serde_json::from_str::<IntentPackage>(&resp.content.trim()) {
                        return Ok(parsed);
                    }
                    let stripped = resp.content
                        .trim()
                        .trim_start_matches("```json")
                        .trim_start_matches("```")
                        .trim_end_matches("```")
                        .trim();
                    if let Ok(parsed) = serde_json::from_str::<IntentPackage>(stripped) {
                        return Ok(parsed);
                    }
                    tracing::warn!("Scout LLM response was not valid IntentPackage JSON — falling back to stub");
                }
                Err(e) => {
                    tracing::warn!("Scout LLM call failed ({}), using stub", e);
                }
            }
        }

        let mut ambiguity_notes = Vec::new();
        if spec_obj.outputs.is_empty() {
            ambiguity_notes.push("Spec does not describe concrete outputs yet".to_string());
        }
        if spec_obj.acceptance_criteria.is_empty() {
            ambiguity_notes.push("Spec is missing acceptance criteria".to_string());
        }
        if briefing
            .memory_hits
            .iter()
            .any(|hit| hit.contains("No memories found") || hit.contains("Memory not initialized"))
        {
            ambiguity_notes.push("No strong prior memory matched this spec".to_string());
        }
        ambiguity_notes.extend(briefing.open_questions.iter().cloned());

        Ok(IntentPackage {
            spec_id: spec_obj.id.clone(),
            summary: format!("Implement {}", spec_obj.title),
            ambiguity_notes,
            recommended_steps: vec![
                "Read Coobie's preflight briefing and required guardrails".into(),
                "Retrieve prior patterns with Coobie".into(),
                "Stage an isolated product workspace".into(),
                "Run visible validation before scenario work".into(),
                "Evaluate hidden scenarios with Sable".into(),
                "Package evidence for human review".into(),
            ],
        })
    }

    /// Mason: build an implementation plan, using an LLM when available.
    async fn mason_implementation_plan(
        &self,
        spec_obj: &Spec,
        intent: &IntentPackage,
        briefing: &CoobieBriefing,
        staged_product: &Path,
        target_source: &TargetSourceMetadata,
    ) -> String {
        let stub = build_implementation_plan(spec_obj, intent, briefing, staged_product, target_source);

        if let Some(provider) = llm::build_provider("mason", "default", &self.paths.setup) {
            let memory_section = format_memory_context(&briefing.memory_hits);
            let spec_yaml = serde_yaml::to_string(spec_obj).unwrap_or_else(|_| format!("{:?}", spec_obj));
            let intent_json = serde_json::to_string_pretty(intent).unwrap_or_default();
            let briefing_json = serde_json::to_string_pretty(briefing).unwrap_or_default();

            let req = LlmRequest::simple(
                "You are Mason, an implementation planning specialist for a software factory.                  You receive a YAML spec, a Scout intent package, and prior memory context.                  Produce a clear, actionable implementation plan in Markdown.                  Structure: ## Target, ## Intent Summary, ## Scope, ## Acceptance Criteria,                  ## Recommended Steps (ordered, numbered), ## Risks, ## Prior Context.                  Be specific. No filler. No preamble.",
                format!(
                    "SPEC:
```yaml
{spec_yaml}
```

INTENT:
```json
{intent_json}
```

                     TARGET: {} ({})

PRIOR MEMORY:
{memory_section}

COOBIE BRIEFING:
```json
{briefing_json}
```

COOBIE RESPONSE:
{response}

                     Produce the implementation plan markdown and treat Coobie guardrails and required checks as constraints.",
                    target_source.label,
                    target_source.source_path,
                    response = briefing.coobie_response,
                ),
            );

            match provider.complete(req).await {
                Ok(resp) => return resp.content,
                Err(e) => tracing::warn!("Mason LLM call failed ({}), using stub", e),
            }
        }

        stub
    }

    /// Piper: build a tool and MCP surface plan, using an LLM when available.
    async fn piper_tool_plan(&self, spec_obj: &Spec, briefing: &CoobieBriefing) -> String {
        let stub = self.build_tool_plan(briefing);

        if let Some(provider) = llm::build_provider("piper", "default", &self.paths.setup) {
            let req = LlmRequest::simple(
                "You are Piper, a tool and MCP routing specialist for a software factory.                  You receive the current tool surface and a spec summary.                  Produce a brief Markdown report: which tools are available, which are relevant                  to this spec, and any gaps or warnings. No filler.",
                format!(
                    "SPEC: {} — {}
DOMAIN SIGNALS: {}
REGULATORY: {}
REQUIRED CHECKS: {}

TOOL SURFACE:
{stub}

                     Produce the tool plan analysis and explicitly call out tools or MCP gaps that block Coobie's required checks.",
                    spec_obj.id,
                    spec_obj.title,
                    if briefing.domain_signals.is_empty() { "none".to_string() } else { briefing.domain_signals.join(", ") },
                    if briefing.regulatory_considerations.is_empty() { "none".to_string() } else { briefing.regulatory_considerations.join(" | ") },
                    if briefing.required_checks.is_empty() { "none".to_string() } else { briefing.required_checks.join(" | ") },
                ),
            );

            match provider.complete(req).await {
                Ok(resp) => return resp.content,
                Err(e) => tracing::warn!("Piper LLM call failed ({}), using stub", e),
            }
        }

        stub
    }

    async fn run_visible_validation(
        &self,
        workspace_root: &Path,
        staged_product: &Path,
        spec_obj: &Spec,
    ) -> Result<ValidationSummary> {
        let mut results = Vec::new();
        let run_dir = workspace_root.join("run");
        let workspace_ok = staged_product.exists() && run_dir.exists();
        results.push(ScenarioResult {
            scenario_id: "workspace_layout".to_string(),
            passed: workspace_ok,
            details: format!(
                "workspace={} run_dir={}",
                staged_product.display(),
                run_dir.display()
            ),
        });

        let validation_log_path = run_dir.join("validation_output.log");
        let mut output_chunks = Vec::new();

        let cargo_manifest = staged_product.join("Cargo.toml");
        let package_json = staged_product.join("package.json");
        let pyproject_toml = staged_product.join("pyproject.toml");
        let requirements_txt = staged_product.join("requirements.txt");
        let go_mod = staged_product.join("go.mod");

        if cargo_manifest.exists() {
            let outcome = self
                .run_command_capture("cargo", &["check", "--quiet"], staged_product)
                .await?;
            output_chunks.push(format_command_output("cargo check --quiet", &outcome));
            results.push(ScenarioResult {
                scenario_id: "cargo_check".to_string(),
                passed: outcome.success,
                details: command_detail("cargo check --quiet", &outcome),
            });
        } else if package_json.exists() {
            if let Some((program, args, label)) = detect_node_bootstrap(staged_product) {
                let arg_refs: Vec<&str> = args.iter().map(|arg| arg.as_str()).collect();
                let outcome = self
                    .run_command_capture(program.as_str(), &arg_refs, staged_product)
                    .await?;
                output_chunks.push(format_command_output(&label, &outcome));
                results.push(ScenarioResult {
                    scenario_id: "node_bootstrap".to_string(),
                    passed: outcome.success,
                    details: command_detail(&label, &outcome),
                });
                if !outcome.success {
                    if !output_chunks.is_empty() {
                        tokio::fs::write(&validation_log_path, output_chunks.join("\n\n"))
                            .await
                            .with_context(|| format!("writing validation log {}", validation_log_path.display()))?;
                    }
                    return Ok(ValidationSummary {
                        passed: false,
                        results,
                    });
                }
            }

            let scripts = detect_package_scripts(&package_json)?;
            let build_command = if command_available("npm") {
                Some(("npm", vec!["run", "build"], "npm run build", "npm_build"))
            } else if staged_product.join("pnpm-lock.yaml").exists() && command_available("pnpm") {
                Some(("pnpm", vec!["build"], "pnpm build", "pnpm_build"))
            } else if staged_product.join("yarn.lock").exists() && command_available("yarn") {
                Some(("yarn", vec!["build"], "yarn build", "yarn_build"))
            } else {
                None
            };
            let test_command = if command_available("npm") {
                Some(("npm", vec!["run", "test"], "npm run test", "npm_test"))
            } else if staged_product.join("pnpm-lock.yaml").exists() && command_available("pnpm") {
                Some(("pnpm", vec!["test"], "pnpm test", "pnpm_test"))
            } else if staged_product.join("yarn.lock").exists() && command_available("yarn") {
                Some(("yarn", vec!["test"], "yarn test", "yarn_test"))
            } else {
                None
            };

            if scripts.contains(&"build".to_string()) {
                if let Some((program, args, label, scenario_id)) = build_command {
                    let outcome = self
                        .run_command_capture(program, &args, staged_product)
                        .await?;
                    output_chunks.push(format_command_output(label, &outcome));
                    results.push(ScenarioResult {
                        scenario_id: scenario_id.to_string(),
                        passed: outcome.success,
                        details: command_detail(label, &outcome),
                    });
                } else {
                    results.push(ScenarioResult {
                        scenario_id: "node_runtime".to_string(),
                        passed: false,
                        details: "package.json found but no supported Node package manager is available".to_string(),
                    });
                }
            } else if scripts.contains(&"test".to_string()) {
                if let Some((program, args, label, scenario_id)) = test_command {
                    let outcome = self
                        .run_command_capture(program, &args, staged_product)
                        .await?;
                    output_chunks.push(format_command_output(label, &outcome));
                    results.push(ScenarioResult {
                        scenario_id: scenario_id.to_string(),
                        passed: outcome.success,
                        details: command_detail(label, &outcome),
                    });
                } else {
                    results.push(ScenarioResult {
                        scenario_id: "node_runtime".to_string(),
                        passed: false,
                        details: "package.json found but no supported Node package manager is available".to_string(),
                    });
                }
            } else {
                results.push(ScenarioResult {
                    scenario_id: "build_manifest".to_string(),
                    passed: true,
                    details: "package.json found but no build/test script is defined".to_string(),
                });
            }
        } else if go_mod.exists() {
            if command_available("go") {
                let outcome = self
                    .run_command_capture("go", &["test", "./..."], staged_product)
                    .await?;
                output_chunks.push(format_command_output("go test ./...", &outcome));
                results.push(ScenarioResult {
                    scenario_id: "go_test".to_string(),
                    passed: outcome.success,
                    details: command_detail("go test ./...", &outcome),
                });
            } else {
                results.push(ScenarioResult {
                    scenario_id: "go_test".to_string(),
                    passed: false,
                    details: "go.mod found but the go toolchain is not available".to_string(),
                });
            }
        } else if pyproject_toml.exists() || requirements_txt.exists() {
            if let Some(python_command) = detect_python_command() {
                let run_pytest = staged_product.join("tests").exists()
                    || pyproject_mentions_pytest(&pyproject_toml)?;
                if run_pytest {
                    let outcome = if command_available("pytest") {
                        self.run_command_capture("pytest", &["-q"], staged_product).await?
                    } else {
                        self.run_command_capture(python_command, &["-m", "pytest", "-q"], staged_product)
                            .await?
                    };
                    let command_label = if command_available("pytest") {
                        "pytest -q"
                    } else {
                        "python -m pytest -q"
                    };
                    output_chunks.push(format_command_output(command_label, &outcome));
                    results.push(ScenarioResult {
                        scenario_id: "python_tests".to_string(),
                        passed: outcome.success,
                        details: command_detail(command_label, &outcome),
                    });
                } else {
                    let outcome = self
                        .run_command_capture(python_command, &["-m", "compileall", "."], staged_product)
                        .await?;
                    output_chunks.push(format_command_output("python -m compileall .", &outcome));
                    results.push(ScenarioResult {
                        scenario_id: "python_compile".to_string(),
                        passed: outcome.success,
                        details: command_detail("python -m compileall .", &outcome),
                    });
                }
            } else {
                results.push(ScenarioResult {
                    scenario_id: "python_runtime".to_string(),
                    passed: false,
                    details: "Python project detected but neither python3 nor python is available"
                        .to_string(),
                });
            }
        } else {
            results.push(ScenarioResult {
                scenario_id: "build_manifest".to_string(),
                passed: true,
                details: "No Cargo.toml, package.json, go.mod, or Python project manifest found; visible validation skipped"
                    .to_string(),
            });
        }

        // Run spec-driven test commands (e.g. corpus tests) and write corpus_results.json.
        if !spec_obj.test_commands.is_empty() {
            let mut command_results = Vec::new();
            let mut all_passed = true;
            for raw_cmd in &spec_obj.test_commands {
                let parts: Vec<&str> = raw_cmd.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let (program, args) = (parts[0], &parts[1..]);
                let outcome = self
                    .run_command_capture(program, args, staged_product)
                    .await?;
                if !outcome.success {
                    all_passed = false;
                }
                output_chunks.push(format_command_output(raw_cmd, &outcome));
                results.push(ScenarioResult {
                    scenario_id: format!("test_command_{}", command_results.len() + 1),
                    passed: outcome.success,
                    details: command_detail(raw_cmd, &outcome),
                });
                command_results.push(serde_json::json!({
                    "label": raw_cmd,
                    "exit_code": outcome.code.unwrap_or(-1),
                    "passed": outcome.success,
                }));
            }
            let corpus_results = serde_json::json!({
                "commands": command_results,
                "all_passed": all_passed,
            });
            let corpus_results_path = workspace_root.join("run").join("corpus_results.json");
            if let Ok(json_str) = serde_json::to_string_pretty(&corpus_results) {
                let _ = tokio::fs::write(&corpus_results_path, json_str).await;
            }
        }

        if !output_chunks.is_empty() {
            tokio::fs::write(&validation_log_path, output_chunks.join("\n\n"))
                .await
                .with_context(|| format!("writing validation log {}", validation_log_path.display()))?;
        }

        Ok(ValidationSummary {
            passed: results.iter().all(|result| result.passed),
            results,
        })
    }

    async fn run_command_capture(
        &self,
        program: &str,
        args: &[&str],
        cwd: &Path,
    ) -> Result<CommandOutcome> {
        let output = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .await
            .with_context(|| format!("running {} in {}", program, cwd.display()))?;

        Ok(CommandOutcome {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }

    async fn write_agent_execution(
        &self,
        profiles: &HashMap<String, AgentProfile>,
        agent_name: &str,
        prompt: &str,
        summary: &str,
        output: &str,
        run_dir: &Path,
        agent_executions: &mut Vec<AgentExecution>,
    ) -> Result<()> {
        let profile = profiles
            .get(agent_name)
            .with_context(|| format!("agent profile not found: {agent_name}"))?;
        let execution = agents::build_execution(profile, &self.paths.setup, prompt, summary, output);
        let agents_dir = run_dir.join("agents");
        tokio::fs::create_dir_all(&agents_dir).await?;
        self.write_json_file(&agents_dir.join(format!("{agent_name}.json")), &execution)
            .await?;

        agent_executions.retain(|existing| existing.agent_name != agent_name);
        agent_executions.push(execution);
        agent_executions.sort_by(|left, right| left.agent_name.cmp(&right.agent_name));
        self.write_json_file(&agents_dir.join("index.json"), agent_executions)
            .await?;
        self.write_json_file(&run_dir.join("agent_executions.json"), agent_executions)
            .await?;
        Ok(())
    }

    /// Bramble: interpret validation output with an LLM when available.
    /// Returns `None` if no LLM is configured (caller uses command output alone).
    async fn bramble_interpret_validation(
        &self,
        spec_obj: &Spec,
        validation: &ValidationSummary,
        briefing: &CoobieBriefing,
    ) -> Option<String> {
        let provider = llm::build_provider("bramble", "default", &self.paths.setup)?;

        let results_text = validation
            .results
            .iter()
            .map(|r| format!("- [{}] {}: {}", if r.passed { "PASS" } else { "FAIL" }, r.scenario_id, r.details))
            .collect::<Vec<_>>()
            .join("
");

        let req = LlmRequest::simple(
            "You are Bramble, a validation analyst for a software factory.              You receive a spec summary and the results of visible validation checks.              Produce a brief Markdown analysis: what passed, what failed, likely root causes              for any failures, and what a developer should look at first. No filler.",
            format!(
                "SPEC: {} — {}
COOBIE REQUIRED CHECKS: {}
COOBIE GUARDRAILS: {}

VALIDATION RESULTS (passed={}):
{results_text}

                 Produce the validation analysis and note any checks Coobie asked for that are still unproven.",
                spec_obj.id,
                spec_obj.title,
                if briefing.required_checks.is_empty() { "none".to_string() } else { briefing.required_checks.join(" | ") },
                if briefing.recommended_guardrails.is_empty() { "none".to_string() } else { briefing.recommended_guardrails.join(" | ") },
                validation.passed,
            ),
        );

        match provider.complete(req).await {
            Ok(resp) => Some(resp.content),
            Err(e) => {
                tracing::warn!("Bramble LLM call failed ({}), skipping analysis", e);
                None
            }
        }
    }

    /// Ash: write a narrative describing the provisioned twin environment.
    /// Returns `None` when no LLM is available — caller continues without it.
    async fn ash_twin_narrative(
        &self,
        spec_obj: &Spec,
        twin: &TwinEnvironment,
        briefing: &CoobieBriefing,
    ) -> Option<String> {
        let provider = llm::build_provider("ash", "default", &self.paths.setup)?;
        let ash_addendum = std::fs::read_to_string(
            self.paths.factory.join("agents").join("personality").join("ash.md"),
        )
        .unwrap_or_default();

        let services = twin.services.iter()
            .map(|s| format!("- {} [{}] status={} — {}", s.name, s.kind, s.status, s.details))
            .collect::<Vec<_>>()
            .join("
");

        let req = LlmRequest::simple(
            "You are Ash, a digital twin specialist for a software factory.              You have just provisioned a local twin environment for a run.              Produce a brief Markdown narrative: what was provisioned, what each service              provides to this run, and any gaps or warnings relevant to the spec.              Two to four short paragraphs. No filler.",
            format!(
                "ASH ADDENDUM:
{}

SPEC: {} — {}
DEPENDENCIES: {}
COOBIE ENVIRONMENT RISKS: {}
COOBIE REQUIRED CHECKS: {}

TWIN SERVICES:
{services}

                 Write the twin environment narrative and identify any simulation gaps against Coobie's environment risks. Be explicit about which twin facts came from Harkonnen versus any product runtime assumptions.",
                if ash_addendum.trim().is_empty() { "none" } else { ash_addendum.trim() },
                spec_obj.id,
                spec_obj.title,
                if spec_obj.dependencies.is_empty() { "none".to_string() } else { spec_obj.dependencies.join(", ") },
                if briefing.environment_risks.is_empty() { "none".to_string() } else { briefing.environment_risks.join(" | ") },
                if briefing.required_checks.is_empty() { "none".to_string() } else { briefing.required_checks.join(" | ") },
            ),
        );

        match provider.complete(req).await {
            Ok(resp) => Some(resp.content),
            Err(e) => {
                tracing::warn!("Ash LLM call failed ({}), skipping narrative", e);
                None
            }
        }
    }

    fn build_tool_plan(&self, briefing: &CoobieBriefing) -> String {
        let mut lines = vec![
            "Tool Plan".to_string(),
            "=========".to_string(),
            format!("Setup: {}", self.paths.setup.setup.name),
            format!("Default provider: {}", self.paths.setup.providers.default),
        ];

        if let Some(mcp) = &self.paths.setup.mcp {
            lines.push(String::new());
            lines.push("MCP Servers".to_string());
            lines.push("-----------".to_string());
            for server in &mcp.servers {
                lines.push(format!(
                    "- {} via {} {} (available={})",
                    server.name,
                    server.command,
                    server.args.join(" "),
                    command_available(&server.command)
                ));
            }
        }

        lines.push(String::new());
        lines.push("Coobie Requirements".to_string());
        lines.push("-------------------".to_string());
        lines.push(format!(
            "- Domain signals: {}",
            if briefing.domain_signals.is_empty() {
                "none".to_string()
            } else {
                briefing.domain_signals.join(", ")
            }
        ));
        lines.push(format!(
            "- Regulatory considerations: {}",
            if briefing.regulatory_considerations.is_empty() {
                "none".to_string()
            } else {
                briefing.regulatory_considerations.join(" | ")
            }
        ));
        lines.push(format!(
            "- Required checks: {}",
            if briefing.required_checks.is_empty() {
                "none".to_string()
            } else {
                briefing.required_checks.join(" | ")
            }
        ));

        lines.push(String::new());
        lines.push("Host Commands".to_string());
        lines.push("-------------".to_string());
        for command in ["cargo", "node", "npm", "docker", "podman", "openclaw"] {
            lines.push(format!("- {} available={}", command, command_available(command)));
        }

        lines.join("\n") + "\n"
    }

    fn build_twin_environment(&self, run_id: &str, spec_obj: &Spec) -> TwinEnvironment {
        let mut services = vec![
            TwinService {
                name: "workspace_fs".to_string(),
                kind: "filesystem".to_string(),
                status: "ready".to_string(),
                details: self.paths.workspaces.display().to_string(),
            },
            TwinService {
                name: "state_db".to_string(),
                kind: "sqlite".to_string(),
                status: "ready".to_string(),
                details: self.paths.db_file.display().to_string(),
            },
            TwinService {
                name: "memory_store".to_string(),
                kind: "memory".to_string(),
                status: "ready".to_string(),
                details: self.paths.memory.display().to_string(),
            },
        ];

        if self.paths.setup.setup.anythingllm.unwrap_or(false) {
            services.push(TwinService {
                name: "anythingllm".to_string(),
                kind: "rag".to_string(),
                status: "configured".to_string(),
                details: "AnythingLLM is enabled for this setup.".to_string(),
            });
        }
        if self.paths.setup.setup.openclaw.unwrap_or(false) {
            services.push(TwinService {
                name: "openclaw".to_string(),
                kind: "orchestrator".to_string(),
                status: "configured".to_string(),
                details: "OpenClaw is enabled for this setup.".to_string(),
            });
        }

        let fingerprint = self
            .paths
            .setup
            .machine
            .as_ref()
            .and_then(|machine| machine.fingerprint.as_ref());
        if fingerprint
            .map(|fingerprint| fingerprint.docker || fingerprint.podman)
            .unwrap_or(false)
        {
            services.push(TwinService {
                name: "container_runtime".to_string(),
                kind: "container".to_string(),
                status: "ready".to_string(),
                details: "Container runtime available for twin workloads.".to_string(),
            });
        }

        for dependency in &spec_obj.dependencies {
            let name = dependency
                .to_lowercase()
                .chars()
                .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                .collect::<String>();
            if services.iter().any(|service| service.name == name) {
                continue;
            }
            services.push(TwinService {
                name,
                kind: "dependency".to_string(),
                status: "simulated".to_string(),
                details: format!("Synthetic twin stub for dependency {}", dependency),
            });
        }

        TwinEnvironment {
            name: format!("run-{run_id}-twin"),
            status: "ready".to_string(),
            services,
            created_at: Utc::now(),
        }
    }

    async fn resolve_target_source(&self, req: &RunRequest) -> Result<TargetSourceMetadata> {
        let (source_kind, source_path, label) = if let Some(product) = &req.product {
            (
                "catalog".to_string(),
                self.paths.products.join(product),
                product.clone(),
            )
        } else if let Some(product_path) = &req.product_path {
            let candidate = PathBuf::from(product_path);
            let resolved = if candidate.is_absolute() {
                candidate
            } else {
                self.paths.root.join(candidate)
            };
            let label = resolved
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_string())
                .unwrap_or_else(|| resolved.display().to_string());
            ("path".to_string(), resolved, label)
        } else {
            bail!("provide either a catalog product or a target path");
        };

        if !source_path.exists() {
            bail!("target source not found: {}", source_path.display());
        }
        if !source_path.is_dir() {
            bail!("target source is not a directory: {}", source_path.display());
        }

        let canonical = source_path.canonicalize()?;
        let git = self.capture_git_metadata(&canonical).await?;
        Ok(TargetSourceMetadata {
            label,
            source_kind,
            source_path: canonical.display().to_string(),
            git,
        })
    }

    async fn capture_git_metadata(&self, source: &Path) -> Result<Option<TargetGitMetadata>> {
        if !command_available("git") {
            return Ok(None);
        }

        let branch = self
            .run_command_capture("git", &["rev-parse", "--abbrev-ref", "HEAD"], source)
            .await
            .ok()
            .filter(|outcome| outcome.success)
            .map(|outcome| outcome.stdout.trim().to_string())
            .filter(|value| !value.is_empty());
        if branch.is_none() {
            return Ok(None);
        }

        let commit = self
            .run_command_capture("git", &["rev-parse", "HEAD"], source)
            .await
            .ok()
            .filter(|outcome| outcome.success)
            .map(|outcome| outcome.stdout.trim().to_string())
            .filter(|value| !value.is_empty());
        let remote_origin = self
            .run_command_capture("git", &["config", "--get", "remote.origin.url"], source)
            .await
            .ok()
            .filter(|outcome| outcome.success)
            .map(|outcome| outcome.stdout.trim().to_string())
            .filter(|value| !value.is_empty());
        let clean = self
            .run_command_capture("git", &["status", "--porcelain"], source)
            .await
            .ok()
            .filter(|outcome| outcome.success)
            .map(|outcome| outcome.stdout.trim().is_empty());

        Ok(Some(TargetGitMetadata {
            branch,
            commit,
            remote_origin,
            clean,
        }))
    }

    async fn insert_run(
        &self,
        run_id: &str,
        spec_id: &str,
        product: &str,
        status: &str,
        now: chrono::DateTime<Utc>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO runs (run_id, spec_id, product, status, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(run_id)
        .bind(spec_id)
        .bind(product)
        .bind(status)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_run_status(&self, run_id: &str, status: &str) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE runs
            SET status = ?2, updated_at = ?3
            WHERE run_id = ?1
            "#,
        )
        .bind(run_id)
        .bind(status)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn start_episode(&self, run_id: &str, phase: &str, goal: &str) -> Result<String> {
        let episode_id = format!("{}-{}", phase, Uuid::new_v4());
        sqlx::query(
            r#"
            INSERT INTO episodes (episode_id, run_id, phase, goal, started_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
        )
        .bind(&episode_id)
        .bind(run_id)
        .bind(phase)
        .bind(goal)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(episode_id)
    }

    async fn finish_episode(
        &self,
        episode_id: &str,
        outcome: &str,
        confidence: Option<f64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE episodes
            SET outcome = ?2, confidence = ?3, ended_at = ?4
            WHERE episode_id = ?1
            "#,
        )
        .bind(episode_id)
        .bind(outcome)
        .bind(confidence)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn link_events(
        &self,
        from_event: i64,
        to_event: i64,
        link_type: &str,
        confidence: f64,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO causal_links (link_id, from_event, to_event, link_type, confidence, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(from_event)
        .bind(to_event)
        .bind(link_type)
        .bind(confidence)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn record_event(
        &self,
        run_id: &str,
        episode_id: Option<&str>,
        phase: &str,
        agent: &str,
        status: &str,
        message: &str,
        log_path: &Path,
    ) -> Result<RunEvent> {
        let created_at = Utc::now();
        let event_preview = RunEvent {
            event_id: 0,
            run_id: run_id.to_string(),
            episode_id: episode_id.map(|value| value.to_string()),
            phase: phase.to_string(),
            agent: agent.to_string(),
            status: status.to_string(),
            message: message.to_string(),
            created_at,
        };
        let pidgin_summary = pidgin::pidgin_summary(&event_preview);
        let message = pidgin::prepend_pidgin(&pidgin_summary, message);
        let result = sqlx::query(
            r#"
            INSERT INTO run_events (run_id, phase, episode_id, agent, status, message, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
        )
        .bind(run_id)
        .bind(phase)
        .bind(episode_id)
        .bind(agent)
        .bind(status)
        .bind(&message)
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .await
            .with_context(|| format!("opening run log {}", log_path.display()))?;
        let line = format!(
            "{} [{}] {} {}: {}\n",
            created_at.to_rfc3339(),
            phase,
            agent,
            status,
            message
        );
        file.write_all(line.as_bytes()).await?;

        Ok(RunEvent {
            event_id: result.last_insert_rowid(),
            run_id: run_id.to_string(),
            episode_id: episode_id.map(|value| value.to_string()),
            phase: phase.to_string(),
            agent: agent.to_string(),
            status: status.to_string(),
            message: message.to_string(),
            created_at,
        })
    }

    async fn write_json_file<T: Serialize>(&self, path: &Path, value: &T) -> Result<()> {
        let content = serde_json::to_string_pretty(value)?;
        tokio::fs::write(path, content)
            .await
            .with_context(|| format!("writing json file {}", path.display()))?;
        Ok(())
    }

    async fn sync_blackboard(&self, board: &BlackboardState, run_dir: Option<&Path>) -> Result<()> {
        {
            let mut guard = self.blackboard.write().await;
            *guard = board.clone();
        }
        if let Some(run_dir) = run_dir {
            self.write_json_file(&run_dir.join("blackboard.json"), board).await?;
        }
        Ok(())
    }

    async fn finalize_blackboard(&self, final_status: &str, run_dir: &Path) -> Result<()> {
        let mut board = self.blackboard.write().await;
        board.current_phase = "complete".to_string();
        board.active_goal = format!("Run finished with status {final_status}");
        board.agent_claims.clear();
        let snapshot = board.clone();
        drop(board);
        self.write_json_file(&run_dir.join("blackboard.json"), &snapshot)
            .await?;
        Ok(())
    }

    async fn mark_blackboard_failed(&self, message: &str, run_dir: &Path) -> Result<()> {
        let mut board = self.blackboard.write().await;
        board.current_phase = "failed".to_string();
        board.active_goal = "Run failed".to_string();
        push_unique(&mut board.open_blockers, message);
        board.agent_claims.clear();
        let snapshot = board.clone();
        drop(board);
        if run_dir.exists() {
            self.write_json_file(&run_dir.join("blackboard.json"), &snapshot)
                .await?;
        }
        Ok(())
    }

    async fn attach_lessons_to_blackboard(&self, run_dir: &Path, lessons: &[LessonRecord]) -> Result<()> {
        if lessons.is_empty() {
            return Ok(());
        }
        let mut board = self.blackboard.write().await;
        for lesson in lessons {
            push_unique(&mut board.lesson_refs, &lesson.lesson_id);
        }
        let snapshot = board.clone();
        drop(board);
        self.write_json_file(&run_dir.join("blackboard.json"), &snapshot)
            .await?;
        self.write_json_file(&run_dir.join("lessons.json"), &lessons).await?;
        Ok(())
    }

    fn run_log_path(&self, run_id: &str) -> PathBuf {
        self.paths.logs.join(format!("{run_id}.log"))
    }

    fn workspace_root(&self, run_id: &str) -> PathBuf {
        self.paths.workspaces.join(run_id)
    }

    fn run_dir(&self, run_id: &str) -> PathBuf {
        self.workspace_root(run_id).join("run")
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>> {
        let row = sqlx::query(
            "SELECT run_id, spec_id, product, status, created_at, updated_at FROM runs WHERE run_id = ?",
        )
        .bind(run_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let run = RunRecord {
            run_id: row.get::<String, _>("run_id"),
            spec_id: row.get::<String, _>("spec_id"),
            product: row.get::<String, _>("product"),
            status: row.get::<String, _>("status"),
            created_at: chrono::DateTime::parse_from_rfc3339(
                row.get::<String, _>("created_at").as_str(),
            )?
            .with_timezone(&Utc),
            updated_at: chrono::DateTime::parse_from_rfc3339(
                row.get::<String, _>("updated_at").as_str(),
            )?
            .with_timezone(&Utc),
        };

        Ok(Some(run))
    }

    pub async fn list_runs(&self, limit: i64) -> Result<Vec<RunRecord>> {
        let rows = sqlx::query(
            "SELECT run_id, spec_id, product, status, created_at, updated_at FROM runs ORDER BY created_at DESC LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut runs = Vec::new();
        for row in rows {
            runs.push(RunRecord {
                run_id: row.get::<String, _>("run_id"),
                spec_id: row.get::<String, _>("spec_id"),
                product: row.get::<String, _>("product"),
                status: row.get::<String, _>("status"),
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("updated_at").as_str(),
                )?
                .with_timezone(&Utc),
            });
        }
        Ok(runs)
    }

    pub async fn list_run_events(&self, run_id: &str) -> Result<Vec<RunEvent>> {
        let rows = sqlx::query(
            "SELECT event_id, run_id, episode_id, phase, agent, status, message, created_at FROM run_events WHERE run_id = ? ORDER BY event_id ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            events.push(RunEvent {
                event_id: row.get::<i64, _>("event_id"),
                run_id: row.get::<String, _>("run_id"),
                episode_id: row.get::<Option<String>, _>("episode_id"),
                phase: row.get::<String, _>("phase"),
                agent: row.get::<String, _>("agent"),
                status: row.get::<String, _>("status"),
                message: row.get::<String, _>("message"),
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
            });
        }
        Ok(events)
    }

    pub async fn list_run_episodes(&self, run_id: &str) -> Result<Vec<EpisodeRecord>> {
        let rows = sqlx::query(
            "SELECT episode_id, run_id, phase, goal, outcome, confidence, started_at, ended_at FROM episodes WHERE run_id = ? ORDER BY started_at ASC",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        let mut episodes = Vec::new();
        for row in rows {
            episodes.push(EpisodeRecord {
                episode_id: row.get::<String, _>("episode_id"),
                run_id: row.get::<String, _>("run_id"),
                phase: row.get::<String, _>("phase"),
                goal: row.get::<String, _>("goal"),
                outcome: row.get::<Option<String>, _>("outcome"),
                confidence: row.get::<Option<f64>, _>("confidence"),
                started_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("started_at").as_str(),
                )?
                .with_timezone(&Utc),
                ended_at: row
                    .get::<Option<String>, _>("ended_at")
                    .map(|value| chrono::DateTime::parse_from_rfc3339(&value))
                    .transpose()?
                    .map(|value| value.with_timezone(&Utc)),
            });
        }
        Ok(episodes)
    }

    pub async fn list_events_for_episode(&self, episode_id: &str) -> Result<Vec<RunEvent>> {
        let rows = sqlx::query(
            "SELECT event_id, run_id, episode_id, phase, agent, status, message, created_at FROM run_events WHERE episode_id = ? ORDER BY event_id ASC",
        )
        .bind(episode_id)
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();
        for row in rows {
            events.push(RunEvent {
                event_id: row.get::<i64, _>("event_id"),
                run_id: row.get::<String, _>("run_id"),
                episode_id: row.get::<Option<String>, _>("episode_id"),
                phase: row.get::<String, _>("phase"),
                agent: row.get::<String, _>("agent"),
                status: row.get::<String, _>("status"),
                message: row.get::<String, _>("message"),
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
            });
        }
        Ok(events)
    }

    pub async fn list_lessons(&self) -> Result<Vec<LessonRecord>> {
        let rows = sqlx::query(
            "SELECT lesson_id, source_episode, pattern, intervention, tags, strength, recall_count, last_recalled, created_at FROM lessons ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut lessons = Vec::new();
        for row in rows {
            lessons.push(LessonRecord {
                lesson_id: row.get::<String, _>("lesson_id"),
                source_episode: row.get::<Option<String>, _>("source_episode"),
                pattern: row.get::<String, _>("pattern"),
                intervention: row.get::<Option<String>, _>("intervention"),
                tags: row
                    .get::<String, _>("tags")
                    .split(',')
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .collect(),
                strength: row.get::<f64, _>("strength"),
                recall_count: row.get::<i64, _>("recall_count"),
                last_recalled: row
                    .get::<Option<String>, _>("last_recalled")
                    .map(|value| chrono::DateTime::parse_from_rfc3339(&value))
                    .transpose()?
                    .map(|value| value.with_timezone(&Utc)),
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
            });
        }
        Ok(lessons)
    }

    async fn consolidate_run(&self, run_id: &str) -> Result<Vec<LessonRecord>> {
        let episodes = self.list_run_episodes(run_id).await?;
        let prior_lessons = self.list_lessons().await?;
        let mut new_lessons = Vec::new();

        for episode in episodes {
            let outcome = episode.outcome.as_deref().unwrap_or("unknown");
            if outcome != "failure" && outcome != "blocked" {
                continue;
            }
            let events = self.list_events_for_episode(&episode.episode_id).await?;
            if events.is_empty() {
                continue;
            }
            let pattern = build_episode_pattern(&episode.phase, &events);
            let prior_count = self
                .count_prior_matching_failed_episodes(run_id, &episode.phase, &pattern)
                .await?;
            if prior_count < 3 {
                continue;
            }

            let lesson_id = format!("lesson-{}", episode.episode_id);
            if prior_lessons.iter().any(|lesson| lesson.lesson_id == lesson_id) {
                continue;
            }

            let lesson = LessonRecord {
                lesson_id: lesson_id.clone(),
                source_episode: Some(episode.episode_id.clone()),
                pattern: format!(
                    "Repeated failure pattern in {}: {}",
                    episode.phase, pattern
                ),
                intervention: infer_intervention(&events),
                tags: vec![
                    "lesson".to_string(),
                    "causal".to_string(),
                    episode.phase.clone(),
                    events
                        .last()
                        .map(|event| event.agent.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                ],
                strength: 1.0,
                recall_count: 0,
                last_recalled: None,
                created_at: Utc::now(),
            };
            self.insert_lesson(&lesson).await?;
            self.memory_store
                .store(
                    &lesson.lesson_id,
                    lesson.tags.clone(),
                    &lesson.pattern,
                    &format!(
                        "Source episode: {}\nPhase: {}\nIntervention: {}\nObserved pattern: {}",
                        episode.episode_id,
                        episode.phase,
                        lesson
                            .intervention
                            .clone()
                            .unwrap_or_else(|| "No intervention recorded yet".to_string()),
                        pattern
                    ),
                )
                .await?;
            new_lessons.push(lesson);

            // Ingest into Coobie causal engine (partial — full ingest happens post-execution)
            let intake_episode = crate::models::FactoryEpisode {
                run_id: run_id.to_string(),
                product: String::new(),
                spec_id: String::new(),
                features: vec![],
                agent_events: events.clone(),
                tool_events: vec![],
                twin_env: None,
                validation: None,
                scenarios: None,
                decision: None,
                created_at: Utc::now(),
            };
            let _ = self.coobie.ingest_episode(&intake_episode).await;
        }

        Ok(new_lessons)
    }

    async fn count_prior_matching_failed_episodes(
        &self,
        current_run_id: &str,
        phase: &str,
        pattern: &str,
    ) -> Result<usize> {
        let rows = sqlx::query(
            "SELECT episode_id FROM episodes WHERE run_id != ? AND phase = ? AND outcome IN ('failure', 'blocked')",
        )
        .bind(current_run_id)
        .bind(phase)
        .fetch_all(&self.pool)
        .await?;

        let mut count = 0;
        for row in rows {
            let episode_id = row.get::<String, _>("episode_id");
            let events = self.list_events_for_episode(&episode_id).await?;
            if events.is_empty() {
                continue;
            }
            let candidate = build_episode_pattern(phase, &events);
            if candidate == pattern {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn insert_lesson(&self, lesson: &LessonRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO lessons (
                lesson_id,
                source_episode,
                pattern,
                intervention,
                tags,
                strength,
                recall_count,
                last_recalled,
                created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
        )
        .bind(&lesson.lesson_id)
        .bind(&lesson.source_episode)
        .bind(&lesson.pattern)
        .bind(&lesson.intervention)
        .bind(lesson.tags.join(","))
        .bind(lesson.strength)
        .bind(lesson.recall_count)
        .bind(lesson.last_recalled.map(|value| value.to_rfc3339()))
        .bind(lesson.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn package_artifacts(&self, run_id: &str) -> Result<PathBuf> {
        let run = self
            .get_run(run_id)
            .await?
            .with_context(|| format!("run not found: {run_id}"))?;
        let events = self.list_run_events(run_id).await?;
        let bundle_dir = self.paths.artifacts.join(run_id);
        tokio::fs::create_dir_all(&bundle_dir).await?;

        let summary = render_bundle_summary(&run, &events);
        tokio::fs::write(bundle_dir.join("SUMMARY.txt"), summary).await?;
        self.write_json_file(&bundle_dir.join("run.json"), &run).await?;
        self.write_json_file(&bundle_dir.join("events.json"), &events).await?;

        let run_dir = self.run_dir(run_id);
        if run_dir.exists() {
            copy_tree_contents(&run_dir, &run_dir, &bundle_dir)?;
        }

        let log_path = self.run_log_path(run_id);
        if log_path.exists() {
            std::fs::copy(&log_path, bundle_dir.join("run.log"))
                .with_context(|| format!("copying run log {}", log_path.display()))?;
        }

        let staged_product = self.workspace_root(run_id).join("product");
        if staged_product.exists() {
            let manifest = list_relative_files(&staged_product, &staged_product)?;
            tokio::fs::write(bundle_dir.join("workspace_manifest.txt"), manifest.join("\n"))
                .await?;
        }

        Ok(bundle_dir)
    }
}

fn build_implementation_plan(
    spec_obj: &Spec,
    intent: &IntentPackage,
    briefing: &CoobieBriefing,
    staged_product: &Path,
    target_source: &TargetSourceMetadata,
) -> String {
    let memory_summary = format_memory_context(&briefing.memory_hits);
    let scope = render_list(&spec_obj.scope, "Scope not specified in the spec yet.");
    let acceptance = render_list(
        &spec_obj.acceptance_criteria,
        "Acceptance criteria not specified yet.",
    );
    let recommended_steps = render_list(
        &intent.recommended_steps,
        "No recommended steps were generated.",
    );
    let domain_signals = render_list(&briefing.domain_signals, "No domain signals inferred yet.");
    let project_components = render_list(
        &render_project_component_lines(spec_obj),
        "No project components were declared in the spec.",
    );
    let scenario_blueprint = render_list(
        &render_scenario_blueprint_lines(spec_obj),
        "No explicit scenario blueprint was declared in the spec.",
    );
    let application_risks = render_list(
        &briefing.application_risks,
        "No application-level risks inferred yet.",
    );
    let environment_risks = render_list(
        &briefing.environment_risks,
        "No environment-level risks inferred yet.",
    );
    let regulatory = render_list(
        &briefing.regulatory_considerations,
        "No regulatory considerations inferred yet.",
    );
    let guardrails = render_list(
        &briefing.recommended_guardrails,
        "No additional Coobie guardrails yet.",
    );
    let required_checks = render_list(
        &briefing.required_checks,
        "No additional Coobie required checks yet.",
    );
    let open_questions = render_list(
        &briefing.open_questions,
        "No open questions captured by Coobie.",
    );
    let git_summary = match &target_source.git {
        Some(git) => format!(
            "branch={} commit={} remote={} clean={}",
            git.branch.as_deref().unwrap_or("unknown"),
            git.commit.as_deref().unwrap_or("unknown"),
            git.remote_origin.as_deref().unwrap_or("unknown"),
            git.clean
                .map(|value| if value { "true" } else { "false" })
                .unwrap_or("unknown")
        ),
        None => "not a git repository or git metadata unavailable".to_string(),
    };
    format!(
        "# Mason Implementation Plan

## Target
- Label: {}
- Source kind: {}
- Source path: {}
- Staged workspace: {}
- Git: {}

## Intent
{}

## Scope
{}

## Acceptance Criteria
{}

## Recommended Steps
{}

## Coobie Domain Signals
{}

## Project Components
{}

## Scenario Blueprint
{}

## Application Risks
{}

## Environment Risks
{}

## Regulatory Considerations
{}

## Guardrails
{}

## Required Checks
{}

## Open Questions
{}

## Prior Context
{}

## Coobie Response
{}
",
        target_source.label,
        target_source.source_kind,
        target_source.source_path,
        staged_product.display(),
        git_summary,
        intent.summary,
        scope,
        acceptance,
        recommended_steps,
        domain_signals,
        project_components,
        scenario_blueprint,
        application_risks,
        environment_risks,
        regulatory,
        guardrails,
        required_checks,
        open_questions,
        memory_summary,
        briefing.coobie_response,
    )
}

fn build_coobie_query_terms(spec_obj: &Spec, target_source: &TargetSourceMetadata) -> Vec<String> {
    let mut terms = vec![
        spec_obj.id.clone(),
        spec_obj.title.clone(),
        spec_obj.purpose.clone(),
        target_source.label.clone(),
        target_source.source_kind.clone(),
    ];

    for value in spec_obj
        .scope
        .iter()
        .chain(spec_obj.constraints.iter())
        .chain(spec_obj.inputs.iter())
        .chain(spec_obj.outputs.iter())
        .chain(spec_obj.acceptance_criteria.iter())
        .chain(spec_obj.dependencies.iter())
        .chain(spec_obj.performance_expectations.iter())
        .chain(spec_obj.security_expectations.iter())
    {
        terms.push(value.clone());
    }

    for component in &spec_obj.project_components {
        terms.push(component.name.clone());
        terms.push(component.role.clone());
        terms.push(component.kind.clone());
        terms.push(component.path.clone());
        if !component.owner.is_empty() {
            terms.push(component.owner.clone());
        }
        terms.extend(component.notes.iter().cloned());
        terms.extend(component.interfaces.iter().cloned());
    }

    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        terms.push(blueprint.pattern.clone());
        terms.push(blueprint.objective.clone());
        terms.extend(blueprint.code_under_test.iter().cloned());
        terms.extend(blueprint.hidden_oracles.iter().cloned());
        terms.extend(blueprint.datasets.iter().cloned());
        terms.extend(blueprint.runtime_surfaces.iter().cloned());
        terms.extend(blueprint.coobie_memory_topics.iter().cloned());
        terms.extend(blueprint.required_artifacts.iter().cloned());
    }

    let mut unique = Vec::new();
    let mut seen = HashSet::new();
    for term in terms {
        let normalized = term.trim();
        if normalized.len() < 3 {
            continue;
        }
        let key = normalized.to_lowercase();
        if seen.insert(key) {
            unique.push(normalized.to_string());
        }
    }
    unique
}

fn infer_domain_signals(
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    query_terms: &[String],
) -> Vec<String> {
    let mut signals = Vec::new();
    let corpus = format!(
        "{}
{}
{}
{}
{}
{}
{}
{}
{}
{}
{}
{}
{}
{}",
        spec_obj.id,
        spec_obj.title,
        spec_obj.purpose,
        target_source.label,
        target_source.source_kind,
        spec_obj.scope.join("
"),
        spec_obj.constraints.join("
"),
        spec_obj.inputs.join("
"),
        spec_obj.outputs.join("
"),
        spec_obj.acceptance_criteria.join("
"),
        spec_obj.dependencies.join("
"),
        spec_obj.performance_expectations.join("
"),
        spec_obj.security_expectations.join("
"),
        query_terms.join("
"),
    )
    .to_lowercase();

    let signal_map = [
        (["sensor", "telemetry", "sampling", "daq"].as_slice(), "high_speed_sensing"),
        (["plc", "opc ua", "modbus", "ethernet/ip", "fieldbus"].as_slice(), "plc_control"),
        (["histori", "time series", "pi system"].as_slice(), "historian_integration"),
        (["scada", "hmi", "alarm", "operator"].as_slice(), "scada_operations"),
        (["simulator", "digital twin", "emulator", "hardware in the loop"].as_slice(), "simulation"),
        (["analytics", "model", "inference", "prediction"].as_slice(), "analytics"),
        (["latency", "throughput", "real-time", "jitter", "cycle time"].as_slice(), "timing_sensitive"),
        (["fail-safe", "interlock", "shutdown", "degraded mode", "safety"].as_slice(), "safety_critical"),
        (["gmp", "gxp", "21 cfr part 11", "audit trail", "validation"].as_slice(), "regulated_environment"),
        (["batch", "recipe", "traceability", "electronic record"].as_slice(), "manufacturing_execution"),
    ];

    for (needles, signal) in signal_map {
        if needles.iter().any(|needle| corpus.contains(needle)) {
            signals.push(signal.to_string());
        }
    }

    if signals.is_empty() {
        signals.push("general_software_factory".to_string());
    }

    signals
}

fn build_application_risks(
    spec_obj: &Spec,
    domain_signals: &[String],
    memory_hits: &[String],
    prior_causes: &[PriorCauseSignal],
) -> Vec<String> {
    let mut risks = Vec::new();

    if domain_signals.iter().any(|signal| signal == "timing_sensitive") {
        risks.push("Throughput, latency, or jitter budgets may be violated without explicit buffering and backpressure handling.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "analytics") {
        risks.push("Analytics outputs may look plausible while operating on stale, replayed, or low-quality plant data.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "manufacturing_execution") {
        risks.push("Batch, recipe, and traceability state can drift if workflow transitions are not modeled as explicit state machines.".to_string());
    }
    if has_project_component_role(spec_obj, "reference_oracle") {
        risks.push("Parity can drift when the code under test and external oracle use different preprocessing, reference anchors, or anomaly semantics.".to_string());
    }
    if has_project_component_role(spec_obj, "dataset") {
        risks.push("Dataset-driven validation is strong for deterministic logic, but it can still miss hardware acquisition quirks and floor timing behavior if the workflow treats replay as production.".to_string());
    }
    if spec_obj.security_expectations.is_empty() {
        risks.push("Security expectations are underspecified, which leaves device trust, credential handling, and operator access ambiguous.".to_string());
    }
    if memory_hits.iter().any(|hit| hit.contains("No memories found")) {
        risks.push("Coobie found little directly reusable prior context, so assumptions need stronger explicit checks and telemetry.".to_string());
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        if !blueprint.coobie_memory_topics.is_empty()
            && memory_hits.iter().any(|hit| hit.contains("No memories found"))
        {
            risks.push(format!(
                "The run declares project memory topics ({}) but Coobie did not retrieve strong context yet, so the pack is at risk of relearning known behavior the hard way.",
                blueprint.coobie_memory_topics.join(", ")
            ));
        }
    }
    for cause in prior_causes.iter().take(2) {
        risks.push(format!(
            "Historical cause signal '{}' has appeared {} time(s); plan mitigations instead of rediscovering it during validation.",
            cause.description,
            cause.occurrences
        ));
    }

    risks.dedup();
    risks
}

fn build_environment_risks(spec_obj: &Spec, domain_signals: &[String]) -> Vec<String> {
    let mut risks = Vec::new();
    let dependency_text = spec_obj.dependencies.join(" ").to_lowercase();

    if domain_signals.iter().any(|signal| signal == "high_speed_sensing") {
        risks.push("High-rate sensor ingest can drop samples or reorder packets unless the twin exercises burst conditions and queue saturation.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "plc_control") {
        risks.push("PLC handshakes, state transitions, and command acknowledgement timing can diverge from nominal flows on the shop floor.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "historian_integration") {
        risks.push("Historian lag, replay, and tag-quality changes can create false confidence if only happy-path reads are simulated.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "scada_operations") {
        risks.push("Alarm acknowledgement, operator overrides, and stale HMI tag quality need environment-level checks, not just unit tests.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "simulation") {
        risks.push("Simulator fidelity gaps can hide timing or protocol defects unless the twin declares what is simulated versus merely stubbed.".to_string());
    }
    if has_project_component_role(spec_obj, "runtime_api") {
        risks.push("Twin assumptions can drift from the product runtime API unless request/response behavior, health surfaces, and degraded states are checked against the product-owned endpoints.".to_string());
    }
    if has_project_component_role(spec_obj, "dataset") {
        risks.push("Replay datasets preserve evidence for comparison, but they do not automatically prove live transport timing, USB capture stability, or runtime service readiness.".to_string());
    }
    if dependency_text.contains("docker") || dependency_text.contains("container") {
        risks.push("Containerized support services may start cleanly while still masking floor-network timing and service-discovery behavior.".to_string());
    }

    risks.dedup();
    risks
}

fn build_regulatory_considerations(spec_obj: &Spec, domain_signals: &[String]) -> Vec<String> {
    let mut items = Vec::new();
    let corpus = format!(
        "{}
{}
{}
{}
{}",
        spec_obj.constraints.join("
"),
        spec_obj.acceptance_criteria.join("
"),
        spec_obj.security_expectations.join("
"),
        spec_obj.outputs.join("
"),
        spec_obj.purpose,
    )
    .to_lowercase();

    if domain_signals.iter().any(|signal| signal == "regulated_environment")
        || contains_any(&corpus, &["gmp", "gxp", "21 cfr part 11", "audit trail"])
    {
        items.push("Treat the run as potentially regulated: preserve audit trails, user attribution, and evidence needed for validation packages.".to_string());
    }
    if contains_any(&corpus, &["electronic signature", "sign-off", "approval"]) {
        items.push("Approval and sign-off flows need tamper-evident records and clear separation between draft, review, and release states.".to_string());
    }
    if contains_any(&corpus, &["traceability", "batch", "lot", "genealogy"]) {
        items.push("Traceability requirements imply immutable linkage between source events, derived analytics, and operator-visible decisions.".to_string());
    }

    items.dedup();
    items
}

fn build_recommended_guardrails(
    spec_obj: &Spec,
    domain_signals: &[String],
    memory_hits: &[String],
    prior_causes: &[PriorCauseSignal],
) -> Vec<String> {
    let mut guardrails = vec![
        "Require every planning agent to consume Coobie's briefing before finalizing its output.".to_string(),
        "Prefer explicit state machines and quality-coded data flows over implicit happy-path assumptions.".to_string(),
        "Capture evidence artifacts for each critical validation claim instead of relying on narrative confidence.".to_string(),
    ];

    if domain_signals.iter().any(|signal| signal == "timing_sensitive") {
        guardrails.push("Model latency budgets, queue limits, retry windows, and timeout behavior as first-class constraints.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "plc_control") {
        guardrails.push("Do not assume PLC writes succeeded until acknowledgement, state echo, and timeout handling are explicitly checked.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "regulated_environment") {
        guardrails.push("Preserve auditability: configuration changes, operator actions, and derived records should all produce reviewable evidence.".to_string());
    }
    if has_project_component_role(spec_obj, "reference_oracle") {
        guardrails.push("Treat the external oracle as read-only evidence: tune the code under test to match observed behavior, not the oracle to match a preferred outcome.".to_string());
    }
    if has_project_component_role(spec_obj, "dataset") {
        guardrails.push("Preserve datasets as immutable evidence inputs and record which dataset bundle was used for each comparison run.".to_string());
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        if !blueprint.coobie_memory_topics.is_empty() {
            guardrails.push(format!(
                "Retrieve and cite project memory topics before planning: {}.",
                blueprint.coobie_memory_topics.join(", ")
            ));
        }
        if !blueprint.runtime_surfaces.is_empty() {
            guardrails.push("Keep Harkonnen coordination state separate from product runtime APIs; Ash may read product surfaces, but the pack still coordinates through the Harkonnen backend.".to_string());
        }
    }
    if memory_hits.iter().any(|hit| hit.contains("No memories found")) {
        guardrails.push("When prior memory is weak, convert assumptions into explicit open questions and required checks before implementation proceeds.".to_string());
    }
    if let Some(cause) = prior_causes.first() {
        guardrails.push(format!(
            "Account for recurring cause '{}' early instead of waiting for validation to rediscover it.",
            cause.description
        ));
    }

    guardrails.dedup();
    guardrails
}

fn build_required_checks(
    spec_obj: &Spec,
    domain_signals: &[String],
    regulatory_considerations: &[String],
) -> Vec<String> {
    let mut checks = vec![
        "Visible validation must prove the main project still builds or executes in the staged workspace.".to_string(),
        "The twin narrative must state which external systems are simulated, stubbed, or still missing.".to_string(),
    ];

    if domain_signals.iter().any(|signal| signal == "high_speed_sensing") {
        checks.push("Exercise burst input conditions and verify sample loss, ordering, and backpressure behavior.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "plc_control") {
        checks.push("Verify PLC command/acknowledgement, heartbeat loss, and safe timeout behavior.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "historian_integration") {
        checks.push("Test historian lag, stale-tag quality, and replay behavior before trusting analytics outputs.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "scada_operations") {
        checks.push("Validate alarm semantics, acknowledgement flow, and operator-visible degraded modes.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "simulation") {
        checks.push("Compare simulator assumptions against at least one declared real-world timing or protocol constraint.".to_string());
    }
    if has_project_component_role(spec_obj, "reference_oracle") {
        checks.push("Run the preserved dataset through both the code under test and the external oracle, then emit a comparison artifact that records mismatches instead of summarizing them away.".to_string());
    }
    if has_project_component_role(spec_obj, "dataset") {
        checks.push("Record dataset identity, frame counts, and replay provenance in the run artifacts so Sable can judge the scenario against preserved evidence.".to_string());
    }
    if has_project_component_role(spec_obj, "runtime_api")
        || spec_obj
            .scenario_blueprint
            .as_ref()
            .map(|b| !b.runtime_surfaces.is_empty())
            .unwrap_or(false)
    {
        checks.push("State clearly which facts came from the Harkonnen API versus product-owned runtime APIs before Ash or Sable treats them as ground truth.".to_string());
    }
    if !regulatory_considerations.is_empty() {
        checks.push("Emit evidence artifacts that support audit trails, traceability, and validation review.".to_string());
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        for artifact in &blueprint.required_artifacts {
            checks.push(format!("Produce required evidence artifact: {}", artifact));
        }
    }
    if spec_obj.rollback_requirements.is_empty() {
        checks.push("Clarify rollback and degraded-mode expectations before relying on destructive or stateful flows.".to_string());
    }

    checks.dedup();
    checks
}

fn build_coobie_open_questions(
    spec_obj: &Spec,
    domain_signals: &[String],
    regulatory_considerations: &[String],
) -> Vec<String> {
    let mut questions = Vec::new();

    if domain_signals.iter().any(|signal| signal == "plc_control") {
        questions.push("Which PLC protocols, command acknowledgement semantics, and timeout budgets are expected on the floor?".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "high_speed_sensing") {
        questions.push("What sampling rates, burst sizes, and loss tolerances define acceptable behavior for incoming sensor data?".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "historian_integration") {
        questions.push("What historian freshness, replay, and tag-quality guarantees must the application respect?".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "simulation") {
        questions.push("Which simulator behaviors are trusted representations of the plant, and which are convenience stubs only?".to_string());
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        if blueprint.pattern.eq_ignore_ascii_case("reference_oracle_regression") {
            if blueprint.hidden_oracles.is_empty() {
                questions.push("Which external oracle or known-good reference implementation defines the hidden acceptance behavior for this run?".to_string());
            }
            if blueprint.datasets.is_empty() {
                questions.push("Which preserved dataset bundle should drive the reference-oracle comparison run?".to_string());
            }
        }
        if !blueprint.runtime_surfaces.is_empty() {
            questions.push("Which product-owned runtime surfaces may Ash observe directly, and which should remain inferred only through Harkonnen artifacts?".to_string());
        }
        if !blueprint.coobie_memory_topics.is_empty() {
            questions.push(format!(
                "Which prior lessons about {} should be treated as strong prior evidence versus hypotheses to retest?",
                blueprint.coobie_memory_topics.join(", ")
            ));
        }
    }
    if !regulatory_considerations.is_empty() {
        questions.push("Which regulatory evidence expectations, such as GMP validation artifacts or audit trails, must this run preserve?".to_string());
    }
    if spec_obj.performance_expectations.is_empty() {
        questions.push("What performance envelope should the system honor under realistic plant load?".to_string());
    }

    questions.dedup();
    questions
}

fn has_project_component_role(spec_obj: &Spec, role: &str) -> bool {
    spec_obj
        .project_components
        .iter()
        .any(|component| component.role.eq_ignore_ascii_case(role))
}

fn render_project_component_lines(spec_obj: &Spec) -> Vec<String> {
    spec_obj
        .project_components
        .iter()
        .map(|component| {
            let mut details = vec![format!("role={}", fallback_component_value(&component.role))];
            details.push(format!("kind={}", fallback_component_value(&component.kind)));
            details.push(format!("path={}", component.path));
            if !component.owner.trim().is_empty() {
                details.push(format!("owner={}", component.owner.trim()));
            }
            if !component.interfaces.is_empty() {
                details.push(format!("interfaces={}", component.interfaces.join(", ")));
            }
            if !component.notes.is_empty() {
                details.push(format!("notes={}", component.notes.join(" | ")));
            }
            format!("{} -> {}", component.name, details.join("; "))
        })
        .collect()
}

fn render_scenario_blueprint_lines(spec_obj: &Spec) -> Vec<String> {
    let Some(blueprint) = &spec_obj.scenario_blueprint else {
        return Vec::new();
    };

    let mut lines = Vec::new();
    if !blueprint.pattern.trim().is_empty() {
        lines.push(format!("pattern={}", blueprint.pattern.trim()));
    }
    if !blueprint.objective.trim().is_empty() {
        lines.push(format!("objective={}", blueprint.objective.trim()));
    }
    if !blueprint.code_under_test.is_empty() {
        lines.push(format!("code_under_test={}", blueprint.code_under_test.join(", ")));
    }
    if !blueprint.hidden_oracles.is_empty() {
        lines.push(format!("hidden_oracles={}", blueprint.hidden_oracles.join(", ")));
    }
    if !blueprint.datasets.is_empty() {
        lines.push(format!("datasets={}", blueprint.datasets.join(", ")));
    }
    if !blueprint.runtime_surfaces.is_empty() {
        lines.push(format!("runtime_surfaces={}", blueprint.runtime_surfaces.join(", ")));
    }
    if !blueprint.coobie_memory_topics.is_empty() {
        lines.push(format!("coobie_memory_topics={}", blueprint.coobie_memory_topics.join(", ")));
    }
    if !blueprint.required_artifacts.is_empty() {
        lines.push(format!("required_artifacts={}", blueprint.required_artifacts.join(", ")));
    }
    lines
}

fn fallback_component_value(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unspecified"
    } else {
        trimmed
    }
}

fn render_list(items: &[String], empty_message: &str) -> String {
    if items.is_empty() {
        format!("- {}", empty_message)
    } else {
        format!("- {}", items.join("
- "))
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(&needle.to_lowercase()))
}

fn format_memory_context(memory_hits: &[String]) -> String {
    if memory_hits.is_empty() {
        "No memory hits collected for this run.".to_string()
    } else {
        memory_hits.join("\n\n---\n\n")
    }
}

fn detect_python_command() -> Option<&'static str> {
    if command_available("python3") {
        Some("python3")
    } else if command_available("python") {
        Some("python")
    } else {
        None
    }
}

fn pyproject_mentions_pytest(pyproject: &Path) -> Result<bool> {
    if !pyproject.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(pyproject)
        .with_context(|| format!("reading {}", pyproject.display()))?;
    Ok(content.contains("pytest") || content.contains("[tool.pytest"))
}

fn detect_node_bootstrap(staged_product: &Path) -> Option<(String, Vec<String>, String)> {
    if staged_product.join("node_modules").exists() {
        return None;
    }

    if staged_product.join("pnpm-lock.yaml").exists() {
        if command_available("pnpm") {
            return Some((
                "pnpm".to_string(),
                vec!["install".to_string(), "--frozen-lockfile".to_string()],
                "pnpm install --frozen-lockfile".to_string(),
            ));
        }
        if command_available("npm") {
            return Some((
                "npm".to_string(),
                vec!["install".to_string(), "--no-fund".to_string(), "--no-audit".to_string()],
                "npm install --no-fund --no-audit".to_string(),
            ));
        }
    }

    if staged_product.join("yarn.lock").exists() {
        if command_available("yarn") {
            return Some((
                "yarn".to_string(),
                vec!["install".to_string(), "--frozen-lockfile".to_string()],
                "yarn install --frozen-lockfile".to_string(),
            ));
        }
        if command_available("npm") {
            return Some((
                "npm".to_string(),
                vec!["install".to_string(), "--no-fund".to_string(), "--no-audit".to_string()],
                "npm install --no-fund --no-audit".to_string(),
            ));
        }
    }

    if staged_product.join("package-lock.json").exists() && command_available("npm") {
        return Some((
            "npm".to_string(),
            vec!["ci".to_string(), "--no-fund".to_string(), "--no-audit".to_string()],
            "npm ci --no-fund --no-audit".to_string(),
        ));
    }

    if command_available("npm") {
        return Some((
            "npm".to_string(),
            vec!["install".to_string(), "--no-fund".to_string(), "--no-audit".to_string()],
            "npm install --no-fund --no-audit".to_string(),
        ));
    }

    None
}

fn detect_package_scripts(package_json: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(package_json)
        .with_context(|| format!("reading {}", package_json.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", package_json.display()))?;
    let scripts = parsed
        .get("scripts")
        .and_then(|value| value.as_object())
        .map(|map| map.keys().cloned().collect())
        .unwrap_or_else(Vec::new);
    Ok(scripts)
}

fn command_detail(command: &str, outcome: &CommandOutcome) -> String {
    let output = if !outcome.stderr.is_empty() {
        outcome.stderr.as_str()
    } else {
        outcome.stdout.as_str()
    };
    let excerpt = truncate_text(output, 220);
    format!(
        "{} -> success={} code={:?} {}",
        command, outcome.success, outcome.code, excerpt
    )
}

fn format_command_output(command: &str, outcome: &CommandOutcome) -> String {
    let mut sections = vec![format!("$ {}", command)];
    if !outcome.stdout.is_empty() {
        sections.push(format!("stdout:\n{}", outcome.stdout));
    }
    if !outcome.stderr.is_empty() {
        sections.push(format!("stderr:\n{}", outcome.stderr));
    }
    sections.push(format!("exit_code: {:?}", outcome.code));
    sections.join("\n\n")
}

fn truncate_text(text: &str, max_len: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "(no output)".to_string();
    }
    if trimmed.chars().count() <= max_len {
        trimmed.to_string()
    } else {
        let mut out = trimmed.chars().take(max_len).collect::<String>();
        out.push_str("...");
        out
    }
}

fn list_relative_files(root: &Path, current: &Path) -> Result<Vec<String>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            files.extend(list_relative_files(root, &path)?);
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .with_context(|| format!("stripping prefix {}", root.display()))?;
            files.push(relative.display().to_string());
        }
    }
    files.sort();
    Ok(files)
}

fn list_run_directory(run_dir: &Path) -> Result<Vec<String>> {
    let mut files = list_relative_files(run_dir, run_dir)?;
    files.insert(0, format!("run_dir={}", run_dir.display()));
    Ok(files)
}

fn copy_tree_contents(source_root: &Path, current: &Path, destination_root: &Path) -> Result<()> {
    for entry in std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let relative = path
            .strip_prefix(source_root)
            .with_context(|| format!("stripping prefix {}", source_root.display()))?;
        let destination = destination_root.join(relative);
        if file_type.is_dir() {
            std::fs::create_dir_all(&destination)
                .with_context(|| format!("creating {}", destination.display()))?;
            copy_tree_contents(source_root, &path, destination_root)?;
        } else if file_type.is_file() {
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
            std::fs::copy(&path, &destination)
                .with_context(|| format!("copying {} -> {}", path.display(), destination.display()))?;
        }
    }
    Ok(())
}

fn render_bundle_summary(run: &RunRecord, events: &[RunEvent]) -> String {
    let mut lines = vec![
        "Run Bundle".to_string(),
        "==========".to_string(),
        format!("Run ID: {}", run.run_id),
        format!("Spec ID: {}", run.spec_id),
        format!("Product: {}", run.product),
        format!("Status: {}", run.status),
        format!("Created: {}", run.created_at),
        format!("Updated: {}", run.updated_at),
        String::new(),
        "Timeline".to_string(),
        "--------".to_string(),
    ];

    if events.is_empty() {
        lines.push("No events recorded".to_string());
    } else {
        for event in events {
            lines.push(format!(
                "{} [{}] {} {} - {}",
                event.created_at, event.phase, event.agent, event.status, event.message
            ));
        }
    }

    lines.join("\n") + "\n"
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if !list.iter().any(|existing| existing == value) {
        list.push(value.to_string());
    }
}

fn remove_blocker(board: &mut BlackboardState, blocker: &str) {
    board.open_blockers.retain(|existing| existing != blocker);
}

fn claim_agent(board: &mut BlackboardState, agent: &str, ownership: &str) {
    board
        .agent_claims
        .insert(agent.to_string(), ownership.to_string());
}

fn release_agent(board: &mut BlackboardState, agent: &str) {
    board.agent_claims.remove(agent);
}

fn normalize_message_pattern(message: &str) -> String {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for ch in message.chars() {
        if ch.is_ascii_alphabetic() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            tokens.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens.into_iter().take(10).collect::<Vec<_>>().join(" ")
}

fn build_episode_pattern(phase: &str, events: &[RunEvent]) -> String {
    let last_meaningful = events
        .iter()
        .rev()
        .find(|event| event.status != "running")
        .unwrap_or_else(|| events.last().expect("events non-empty"));
    format!(
        "{}:{}:{}",
        phase,
        last_meaningful.agent,
        normalize_message_pattern(&last_meaningful.message)
    )
}

fn infer_intervention(events: &[RunEvent]) -> Option<String> {
    events
        .iter()
        .rev()
        .find(|event| event.status == "complete")
        .map(|event| format!("{} completed: {}", event.agent, event.message))
}
