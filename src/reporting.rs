use anyhow::Result;
use serde::{de::DeserializeOwned, Deserialize};
use std::path::Path;

use crate::{
    coobie::CausalReport,
    models::{
        AgentExecution, BlackboardState, CoobieBriefing, HiddenScenarioSummary, LessonRecord, ProjectComponent,
        ScenarioBlueprint, TwinEnvironment, ValidationSummary,
    },
    orchestrator::AppContext,
};

#[derive(Debug, Deserialize)]
struct TargetGitMetadataReport {
    branch: Option<String>,
    commit: Option<String>,
    remote_origin: Option<String>,
    clean: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct TargetSourceMetadataReport {
    label: String,
    source_kind: String,
    source_path: String,
    git: Option<TargetGitMetadataReport>,
}

fn render_component_lines(components: &[ProjectComponent]) -> Vec<String> {
    components
        .iter()
        .map(|component| {
            let mut parts = vec![format!("role={}", fallback_value(&component.role))];
            parts.push(format!("kind={}", fallback_value(&component.kind)));
            parts.push(format!("path={}", component.path));
            if !component.owner.trim().is_empty() {
                parts.push(format!("owner={}", component.owner.trim()));
            }
            if !component.interfaces.is_empty() {
                parts.push(format!("interfaces={}", component.interfaces.join(", ")));
            }
            if !component.notes.is_empty() {
                parts.push(format!("notes={}", component.notes.join(" | ")));
            }
            format!("- {} -> {}", component.name, parts.join("; "))
        })
        .collect()
}

fn render_blueprint_lines(blueprint: Option<&ScenarioBlueprint>) -> Vec<String> {
    let Some(blueprint) = blueprint else {
        return vec!["- No explicit scenario blueprint recorded.".to_string()];
    };

    let mut lines = Vec::new();
    if !blueprint.pattern.trim().is_empty() {
        lines.push(format!("- pattern={}", blueprint.pattern.trim()));
    }
    if !blueprint.objective.trim().is_empty() {
        lines.push(format!("- objective={}", blueprint.objective.trim()));
    }
    if !blueprint.code_under_test.is_empty() {
        lines.push(format!("- code_under_test={}", blueprint.code_under_test.join(", ")));
    }
    if !blueprint.hidden_oracles.is_empty() {
        lines.push(format!("- hidden_oracles={}", blueprint.hidden_oracles.join(", ")));
    }
    if !blueprint.datasets.is_empty() {
        lines.push(format!("- datasets={}", blueprint.datasets.join(", ")));
    }
    if !blueprint.runtime_surfaces.is_empty() {
        lines.push(format!("- runtime_surfaces={}", blueprint.runtime_surfaces.join(", ")));
    }
    if !blueprint.coobie_memory_topics.is_empty() {
        lines.push(format!("- coobie_memory_topics={}", blueprint.coobie_memory_topics.join(", ")));
    }
    if !blueprint.required_artifacts.is_empty() {
        lines.push(format!("- required_artifacts={}", blueprint.required_artifacts.join(", ")));
    }
    if lines.is_empty() {
        lines.push("- No explicit scenario blueprint recorded.".to_string());
    }
    lines
}

fn fallback_value(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        "unspecified"
    } else {
        trimmed
    }
}

pub async fn build_report(app: &AppContext, run_id: &str) -> Result<String> {
    let run = app.get_run(run_id).await?;
    let Some(run) = run else {
        return Ok(format!("Run not found: {run_id}"));
    };

    let events = app.list_run_events(run_id).await?;
    let run_dir = app.paths.workspaces.join(run_id).join("run");
    let target_source: Option<TargetSourceMetadataReport> =
        read_optional_json(&run_dir.join("target_source.json")).await?;
    let validation: Option<ValidationSummary> =
        read_optional_json(&run_dir.join("validation.json")).await?;
    let twin: Option<TwinEnvironment> = read_optional_json(&run_dir.join("twin.json")).await?;
    let hidden: Option<HiddenScenarioSummary> =
        read_optional_json(&run_dir.join("hidden_scenarios.json")).await?;
    let blackboard: Option<BlackboardState> =
        read_optional_json(&run_dir.join("blackboard.json")).await?;
    let lessons: Option<Vec<LessonRecord>> =
        read_optional_json(&run_dir.join("lessons.json")).await?;
    let agent_executions: Option<Vec<AgentExecution>> =
        read_optional_json(&run_dir.join("agent_executions.json")).await?;
    let coobie_briefing: Option<CoobieBriefing> =
        read_optional_json(&run_dir.join("coobie_briefing.json")).await?;
    let causal_report: Option<CausalReport> =
        read_optional_json(&run_dir.join("causal_report.json")).await?;
    let coobie_preflight_response = read_optional_text(&run_dir.join("coobie_preflight_response.md")).await?;
    let coobie_report_response = read_optional_text(&run_dir.join("coobie_report_response.md")).await?;

    let mut report = format!(
        "Run Report\n==========\nRun ID: {}\nSpec ID: {}\nProduct: {}\nStatus: {}\nCreated: {}\nUpdated: {}\nWorkspace: {}\nArtifacts: {}\n",
        run.run_id,
        run.spec_id,
        run.product,
        run.status,
        run.created_at,
        run.updated_at,
        app.paths.workspaces.join(run_id).display(),
        app.paths.artifacts.join(run_id).display(),
    );

    report.push_str("\nTarget Source\n-------------\n");
    if let Some(target_source) = target_source {
        report.push_str(&format!("Label: {}\n", target_source.label));
        report.push_str(&format!("Kind: {}\n", target_source.source_kind));
        report.push_str(&format!("Path: {}\n", target_source.source_path));
        if let Some(git) = target_source.git {
            report.push_str(&format!(
                "Git: branch={} commit={} clean={}\n",
                git.branch.unwrap_or_else(|| "unknown".to_string()),
                git.commit.unwrap_or_else(|| "unknown".to_string()),
                git.clean
                    .map(|value| if value { "true" } else { "false" }.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ));
            report.push_str(&format!(
                "Remote: {}\n",
                git.remote_origin.unwrap_or_else(|| "unknown".to_string())
            ));
        }
    } else {
        report.push_str("No target source metadata written yet.\n");
    }

    report.push_str("\nTimeline\n--------\n");
    if events.is_empty() {
        report.push_str("No run events recorded.\n");
    } else {
        for event in &events {
            report.push_str(&format!(
                "{} [{}] {} {} - {}\n",
                event.created_at, event.phase, event.agent, event.status, event.message
            ));
        }
    }

    report.push_str("\nAgents\n------\n");
    if let Some(agent_executions) = agent_executions {
        for execution in agent_executions {
            report.push_str(&format!(
                "- {} -> {} / {} / mode={}\n  {}\n",
                execution.agent_name,
                execution.provider,
                execution.model,
                execution.mode,
                execution.summary
            ));
        }
    } else {
        report.push_str("No agent execution transcripts written yet.\n");
    }

    report.push_str("\nTwin Environment\n----------------\n");
    if let Some(twin) = twin {
        report.push_str(&format!("Status: {}\n", twin.status));
        for service in twin.services {
            report.push_str(&format!(
                "- {} [{}] {}\n  {}\n",
                service.name, service.kind, service.status, service.details
            ));
        }
    } else {
        report.push_str("No twin environment manifest written yet.\n");
    }

    report.push_str("\nVisible Validation\n------------------\n");
    if let Some(validation) = validation {
        report.push_str(&format!("Passed: {}\n", validation.passed));
        for result in validation.results {
            report.push_str(&format!(
                "- {}: {}\n  {}\n",
                result.scenario_id,
                if result.passed { "pass" } else { "fail" },
                result.details
            ));
        }
    } else {
        report.push_str("No validation summary written yet.\n");
    }

    report.push_str("\nHidden Scenarios\n----------------\n");
    if let Some(hidden) = hidden {
        report.push_str(&format!("Passed: {}\n", hidden.passed));
        for result in hidden.results {
            report.push_str(&format!(
                "- {}: {}\n  {}\n",
                result.scenario_id,
                if result.passed { "pass" } else { "fail" },
                result.details
            ));
            for check in result.checks {
                report.push_str(&format!(
                    "    * {} -> {} ({})\n",
                    check.kind,
                    if check.passed { "pass" } else { "fail" },
                    check.details
                ));
            }
        }
    } else {
        report.push_str("No hidden scenario summary written yet.\n");
    }

    report.push_str("\nBlackboard\n----------\n");
    if let Some(blackboard) = blackboard {
        report.push_str(&format!("Phase: {}\n", blackboard.current_phase));
        report.push_str(&format!("Active goal: {}\n", blackboard.active_goal));
        report.push_str(&format!(
            "Open blockers: {}\n",
            if blackboard.open_blockers.is_empty() {
                "none".to_string()
            } else {
                blackboard.open_blockers.join(", ")
            }
        ));
        report.push_str(&format!(
            "Resolved items: {}\n",
            if blackboard.resolved_items.is_empty() {
                "none".to_string()
            } else {
                blackboard.resolved_items.join(", ")
            }
        ));
        report.push_str(&format!(
            "Artifacts tracked: {}\n",
            if blackboard.artifact_refs.is_empty() {
                "none".to_string()
            } else {
                blackboard.artifact_refs.join(", ")
            }
        ));
        report.push_str(&format!(
            "Lesson refs: {}\n",
            if blackboard.lesson_refs.is_empty() {
                "none".to_string()
            } else {
                blackboard.lesson_refs.join(", ")
            }
        ));
    } else {
        report.push_str("No blackboard snapshot written yet.\n");
    }

    report.push_str("\nConsolidated Lessons\n--------------------\n");
    if let Some(lessons) = lessons {
        if lessons.is_empty() {
            report.push_str("No lessons were promoted for this run.\n");
        } else {
            for lesson in lessons {
                report.push_str(&format!(
                    "- {} (strength {:.1})\n  tags={}\n  intervention={}\n",
                    lesson.pattern,
                    lesson.strength,
                    lesson.tags.join(","),
                    lesson.intervention.unwrap_or_else(|| "none".to_string())
                ));
            }
        }
    } else {
        report.push_str("No lessons were promoted for this run.\n");
    }

    report.push_str("
Coobie Preflight
-----------------
");
    if let Some(briefing) = coobie_briefing {
        report.push_str(&format!("Generated: {}
", briefing.generated_at));
        report.push_str(&format!(
            "Domain signals: {}
",
            if briefing.domain_signals.is_empty() {
                "none".to_string()
            } else {
                briefing.domain_signals.join(", ")
            }
        ));
        report.push_str(&format!(
            "Required checks: {}
",
            if briefing.required_checks.is_empty() {
                "none".to_string()
            } else {
                briefing.required_checks.join(" | ")
            }
        ));
        report.push_str(&format!(
            "Regulatory considerations: {}
",
            if briefing.regulatory_considerations.is_empty() {
                "none".to_string()
            } else {
                briefing.regulatory_considerations.join(" | ")
            }
        ));
        report.push_str("Project components:
");
        for line in render_component_lines(&briefing.project_components) {
            report.push_str(&format!("{}
", line));
        }
        report.push_str("Scenario blueprint:
");
        for line in render_blueprint_lines(briefing.scenario_blueprint.as_ref()) {
            report.push_str(&format!("{}
", line));
        }
    } else {
        report.push_str("No Coobie preflight briefing written yet.
");
    }

    report.push_str("\nCoobie Responses\n-----------------\n");
    if let Some(response) = coobie_preflight_response {
        report.push_str("Preflight response:\n");
        report.push_str(&response);
        report.push('\n');
    } else {
        report.push_str("No Coobie preflight response written yet.\n");
    }
    if let Some(response) = coobie_report_response {
        report.push_str("\nReport response:\n");
        report.push_str(&response);
        report.push('\n');
    } else {
        report.push_str("Report response: not yet generated.\n");
    }


    report.push_str("\nCoobie Causal Analysis\n----------------------\n");
    if let Some(causal) = causal_report {
        report.push_str(&format!("Generated: {}\n", causal.generated_at));
        if let Some(cause) = &causal.primary_cause {
            report.push_str(&format!(
                "Primary cause: {} (confidence {:.0}%)\n",
                cause,
                causal.primary_confidence * 100.0
            ));
        } else {
            report.push_str("Primary cause: none identified\n");
        }
        if !causal.contributing_causes.is_empty() {
            report.push_str(&format!(
                "Contributing: {}\n",
                causal.contributing_causes.join(", ")
            ));
        }
        let scores = &causal.episode_scores;
        report.push_str(&format!(
            "Scores: spec_clarity={:.2} change_scope={:.2} twin_fidelity={:.2} test_coverage={:.2} memory_retrieval={:.2}\n",
            scores.spec_clarity_score,
            scores.change_scope_score,
            scores.twin_fidelity_score,
            scores.test_coverage_score,
            scores.memory_retrieval_score,
        ));
        if let Some(deep) = &causal.deep_causality {
            report.push_str(&format!(
                "DeepCausality: effect={:.2} active_signals={}/{} ({:.0}%)\n",
                deep.effect_score,
                deep.active_signal_count,
                deep.active_signal_count + deep.inactive_signals.len(),
                deep.active_signal_percent,
            ));
            for signal in deep.active_signals.iter().take(3) {
                report.push_str(&format!(
                    "  - {} obs={:.2} threshold={:.2} strength={:.0}%\n",
                    signal.cause_id,
                    signal.observation,
                    signal.threshold,
                    signal.activation_strength * 100.0,
                ));
            }
        }
        if causal.recommended_interventions.is_empty() {
            report.push_str("Interventions: none recommended\n");
        } else {
            for intervention in &causal.recommended_interventions {
                report.push_str(&format!(
                    "- [{}] {} -> {}\n",
                    intervention.target, intervention.action, intervention.expected_impact
                ));
            }
        }
        if let Some(cf) = &causal.counterfactual_prediction {
            report.push_str(&format!(
                "Counterfactual: {} (confidence gain {:.0}%)\n",
                cf.prediction,
                cf.confidence_gain * 100.0
            ));
        }
    } else {
        report.push_str("No causal analysis generated for this run.\n");
    }

    Ok(report)
}

async fn read_optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path).await?;
    Ok(Some(serde_json::from_str::<T>(&raw)?))
}

async fn read_optional_text(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(tokio::fs::read_to_string(path).await?))
}
