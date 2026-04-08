use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::path::Path;

use crate::llm::{self, LlmRequest, Message};
use crate::models::Spec;
use crate::models::{
    AgentExecution, HiddenScenarioCheckResult, HiddenScenarioEvaluation, HiddenScenarioSummary,
    RunEvent, TwinEnvironment, ValidationSummary,
};
use crate::setup::SetupConfig;

#[derive(Debug, Clone, Deserialize)]
pub struct AttemptThreshold {
    pub attempt: usize,
    pub value: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HiddenScenarioFile {
    pub id: String,
    pub spec_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub checks: Vec<HiddenScenarioCheck>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HiddenScenarioCheck {
    RunStatus {
        equals: String,
    },
    EventPresent {
        phase: String,
        agent: String,
    },
    ArtifactExists {
        path: String,
    },
    ValidationPassed,
    AgentExecuted {
        agent: String,
    },
    TwinServicePresent {
        service: String,
    },
    /// Check the exit_code field in corpus_results.json for the named command.
    TestExitCode {
        equals: i32,
        label: String,
    },
    /// Check that a numeric field in a JSON artifact is >= value.
    MetricGte {
        artifact: String,
        field: String,
        value: f64,
    },
    /// Check that a numeric field in a JSON artifact is >= the threshold selected for this run attempt.
    MetricGteByAttempt {
        artifact: String,
        field: String,
        thresholds: Vec<AttemptThreshold>,
    },
    /// Check that a field in a JSON artifact equals value (JSON comparison).
    MetricEq {
        artifact: String,
        field: String,
        value: JsonValue,
    },
    /// Check that any .md artifact in the run dir contains all required tag strings.
    MemoryEntryExists {
        tags: Vec<String>,
    },
    /// Check that the named artifact's raw text contains all required snippets.
    ArtifactContainsAll {
        path: String,
        contains_all: Vec<String>,
    },
    ExplorationLogExists,
}

pub fn load_hidden_scenarios(root: &Path, spec_id: &str) -> Result<Vec<HiddenScenarioFile>> {
    let mut scenarios = Vec::new();
    if !root.exists() {
        return Ok(scenarios);
    }

    collect_scenarios(root, spec_id, &mut scenarios)?;
    scenarios.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(scenarios)
}

fn collect_scenarios(
    current: &Path,
    spec_id: &str,
    scenarios: &mut Vec<HiddenScenarioFile>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)
        .with_context(|| format!("reading scenario directory {}", current.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_scenarios(&path, spec_id, scenarios)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("yaml") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading hidden scenario {}", path.display()))?;
        let scenario: HiddenScenarioFile = serde_yaml::from_str(&raw)
            .with_context(|| format!("parsing hidden scenario {}", path.display()))?;
        if scenario.spec_id == spec_id {
            scenarios.push(scenario);
        }
    }
    Ok(())
}

pub fn evaluate_hidden_scenarios(
    scenarios: &[HiddenScenarioFile],
    run_status: &str,
    run_attempt: usize,
    events: &[RunEvent],
    validation: &ValidationSummary,
    twin: &TwinEnvironment,
    agent_executions: &[AgentExecution],
    run_dir: &Path,
) -> HiddenScenarioSummary {
    if scenarios.is_empty() {
        return HiddenScenarioSummary {
            passed: false,
            results: vec![HiddenScenarioEvaluation {
                scenario_id: "scenario_store_empty".to_string(),
                title: "No hidden scenarios available".to_string(),
                passed: false,
                details: "No hidden scenario definitions matched this spec.".to_string(),
                checks: vec![],
            }],
        };
    }

    let results = scenarios
        .iter()
        .map(|scenario| {
            let checks = scenario
                .checks
                .iter()
                .map(|check| {
                    evaluate_check(
                        check,
                        run_status,
                        run_attempt,
                        events,
                        validation,
                        twin,
                        agent_executions,
                        run_dir,
                    )
                })
                .collect::<Vec<_>>();
            let passed = checks.iter().all(|check| check.passed);
            HiddenScenarioEvaluation {
                scenario_id: scenario.id.clone(),
                title: scenario.title.clone(),
                passed,
                details: if passed {
                    scenario.description.clone()
                } else {
                    format!(
                        "{} (one or more hidden checks failed)",
                        scenario.description
                    )
                },
                checks,
            }
        })
        .collect::<Vec<_>>();

    HiddenScenarioSummary {
        passed: results.iter().all(|result| result.passed),
        results,
    }
}

fn evaluate_check(
    check: &HiddenScenarioCheck,
    run_status: &str,
    run_attempt: usize,
    events: &[RunEvent],
    validation: &ValidationSummary,
    twin: &TwinEnvironment,
    agent_executions: &[AgentExecution],
    run_dir: &Path,
) -> HiddenScenarioCheckResult {
    match check {
        HiddenScenarioCheck::RunStatus { equals } => HiddenScenarioCheckResult {
            kind: format!("run_status == {equals}"),
            passed: run_status == equals,
            details: format!("actual status: {run_status}"),
        },
        HiddenScenarioCheck::EventPresent { phase, agent } => {
            let found = events
                .iter()
                .any(|event| event.phase == *phase && event.agent == *agent);
            HiddenScenarioCheckResult {
                kind: format!("event_present({phase}, {agent})"),
                passed: found,
                details: if found {
                    format!("found event for {agent} during {phase}")
                } else {
                    format!("missing event for {agent} during {phase}")
                },
            }
        }
        HiddenScenarioCheck::ArtifactExists { path } => {
            let target = run_dir.join(path);
            HiddenScenarioCheckResult {
                kind: format!("artifact_exists({path})"),
                passed: target.exists(),
                details: target.display().to_string(),
            }
        }
        HiddenScenarioCheck::ValidationPassed => HiddenScenarioCheckResult {
            kind: "validation_passed".to_string(),
            passed: validation.passed,
            details: format!("visible validation passed: {}", validation.passed),
        },
        HiddenScenarioCheck::AgentExecuted { agent } => {
            let found = agent_executions
                .iter()
                .any(|execution| execution.agent_name == *agent);
            HiddenScenarioCheckResult {
                kind: format!("agent_executed({agent})"),
                passed: found,
                details: if found {
                    format!("found execution transcript for {agent}")
                } else {
                    format!("missing execution transcript for {agent}")
                },
            }
        }
        HiddenScenarioCheck::TwinServicePresent { service } => {
            let found = twin
                .services
                .iter()
                .any(|candidate| candidate.name == *service);
            HiddenScenarioCheckResult {
                kind: format!("twin_service_present({service})"),
                passed: found,
                details: if found {
                    format!("twin includes service {service}")
                } else {
                    format!("twin does not include service {service}")
                },
            }
        }
        HiddenScenarioCheck::TestExitCode { equals, label } => {
            let kind = format!("test_exit_code({label}) == {equals}");
            let results_path = run_dir.join("corpus_results.json");
            let json = match read_json_artifact(&results_path) {
                Ok(v) => v,
                Err(msg) => {
                    return HiddenScenarioCheckResult {
                        kind,
                        passed: false,
                        details: msg,
                    }
                }
            };
            let commands = json.get("commands").and_then(|c| c.as_array());
            let entry = commands.and_then(|cmds| {
                cmds.iter().find(|cmd| {
                    cmd.get("label")
                        .and_then(|l| l.as_str())
                        .map(|l| l.contains(label.as_str()))
                        .unwrap_or(false)
                })
            });
            let exit_code = entry
                .and_then(|e| e.get("exit_code"))
                .and_then(|c| c.as_i64())
                .unwrap_or(-1) as i32;
            HiddenScenarioCheckResult {
                kind,
                passed: exit_code == *equals,
                details: format!("actual exit_code: {exit_code}"),
            }
        }
        HiddenScenarioCheck::MetricGte {
            artifact,
            field,
            value,
        } => {
            let kind = format!("metric_gte({artifact}.{field} >= {value})");
            let json = match read_json_artifact(&run_dir.join(artifact)) {
                Ok(v) => v,
                Err(msg) => {
                    return HiddenScenarioCheckResult {
                        kind,
                        passed: false,
                        details: msg,
                    }
                }
            };
            match json_field(&json, field).and_then(|v| v.as_f64()) {
                Some(n) => HiddenScenarioCheckResult {
                    kind,
                    passed: n >= *value,
                    details: format!("actual: {n}, required >= {value}"),
                },
                None => HiddenScenarioCheckResult {
                    kind,
                    passed: false,
                    details: format!("field path '{field}' not found or not numeric in {artifact}"),
                },
            }
        }
        HiddenScenarioCheck::MetricGteByAttempt {
            artifact,
            field,
            thresholds,
        } => {
            let selected = match threshold_for_attempt(thresholds, run_attempt) {
                Some(value) => value,
                None => {
                    return HiddenScenarioCheckResult {
                        kind: format!("metric_gte_by_attempt({artifact}.{field})"),
                        passed: false,
                        details: "no thresholds were configured for adaptive metric check"
                            .to_string(),
                    }
                }
            };
            let kind = format!(
                "metric_gte_by_attempt({artifact}.{field} >= {selected} @ attempt {run_attempt})"
            );
            let json = match read_json_artifact(&run_dir.join(artifact)) {
                Ok(v) => v,
                Err(msg) => {
                    return HiddenScenarioCheckResult {
                        kind,
                        passed: false,
                        details: msg,
                    }
                }
            };
            match json_field(&json, field).and_then(|v| v.as_f64()) {
                Some(n) => HiddenScenarioCheckResult {
                    kind,
                    passed: n >= selected,
                    details: format!(
                        "actual: {n}, required >= {selected} for attempt {run_attempt}"
                    ),
                },
                None => HiddenScenarioCheckResult {
                    kind,
                    passed: false,
                    details: format!("field path '{field}' not found or not numeric in {artifact}"),
                },
            }
        }
        HiddenScenarioCheck::MetricEq {
            artifact,
            field,
            value,
        } => {
            let kind = format!("metric_eq({artifact}.{field} == {value})");
            let json = match read_json_artifact(&run_dir.join(artifact)) {
                Ok(v) => v,
                Err(msg) => {
                    return HiddenScenarioCheckResult {
                        kind,
                        passed: false,
                        details: msg,
                    }
                }
            };
            match json_field(&json, field) {
                Some(actual) => HiddenScenarioCheckResult {
                    kind,
                    passed: actual == value,
                    details: format!("actual: {actual}, expected: {value}"),
                },
                None => HiddenScenarioCheckResult {
                    kind,
                    passed: false,
                    details: format!("field path '{field}' not found in {artifact}"),
                },
            }
        }
        HiddenScenarioCheck::MemoryEntryExists { tags } => {
            let kind = format!("memory_entry_exists(tags: [{}])", tags.join(", "));
            let found = std::fs::read_dir(run_dir)
                .map(|entries| {
                    entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.path().extension().map(|ext| ext == "md").unwrap_or(false))
                        .any(|e| {
                            let content = std::fs::read_to_string(e.path()).unwrap_or_default();
                            tags.iter().all(|tag| content.contains(tag.as_str()))
                        })
                })
                .unwrap_or(false);
            HiddenScenarioCheckResult {
                kind,
                passed: found,
                details: if found {
                    "found matching memory entry in run artifacts".to_string()
                } else {
                    format!("no .md artifact contains all tags: [{}]", tags.join(", "))
                },
            }
        }
        HiddenScenarioCheck::ArtifactContainsAll { path, contains_all } => {
            let kind = format!("artifact_contains_all({path})");
            let target = run_dir.join(path);
            let content = match std::fs::read_to_string(&target) {
                Ok(content) => content,
                Err(error) => {
                    return HiddenScenarioCheckResult {
                        kind,
                        passed: false,
                        details: format!("could not read {}: {error}", target.display()),
                    }
                }
            };
            let missing = contains_all
                .iter()
                .filter(|needle| !content.contains(needle.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            HiddenScenarioCheckResult {
                kind,
                passed: missing.is_empty(),
                details: if missing.is_empty() {
                    format!("all required snippets found in {}", target.display())
                } else {
                    format!(
                        "missing snippets in {}: {}",
                        target.display(),
                        missing.join(", ")
                    )
                },
            }
        }
        HiddenScenarioCheck::ExplorationLogExists => {
            let path = run_dir.join("exploration_log.md");
            HiddenScenarioCheckResult {
                kind: "exploration_log_exists".to_string(),
                passed: path.exists(),
                details: path.display().to_string(),
            }
        }
    }
}

fn threshold_for_attempt(thresholds: &[AttemptThreshold], run_attempt: usize) -> Option<f64> {
    if thresholds.is_empty() {
        return None;
    }

    let mut ordered = thresholds.to_vec();
    ordered.sort_by_key(|threshold| threshold.attempt);

    ordered
        .iter()
        .rev()
        .find(|threshold| run_attempt >= threshold.attempt)
        .or_else(|| ordered.first())
        .map(|threshold| threshold.value)
}

fn json_field<'a>(json: &'a JsonValue, field_path: &str) -> Option<&'a JsonValue> {
    field_path
        .split('.')
        .filter(|segment| !segment.is_empty())
        .try_fold(json, |current, segment| current.get(segment))
}

fn read_json_artifact(path: &Path) -> std::result::Result<JsonValue, String> {
    if !path.exists() {
        return Err(format!("{} not found", path.display()));
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("could not read {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("could not parse {}: {e}", path.display()))
}

// ── Sable LLM-generated scenarios ────────────────────────────────────────────

/// Serde shape for what Sable returns from her LLM call.
#[derive(Debug, Deserialize, Serialize)]
struct SableGeneratedCheck {
    kind: String,
    // optional fields used by different check kinds
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    contains_all: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    equals: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SableGeneratedScenario {
    id: String,
    title: String,
    description: String,
    checks: Vec<SableGeneratedCheck>,
}

#[derive(Debug, Deserialize, Serialize)]
struct SableGeneratedPayload {
    scenarios: Vec<SableGeneratedScenario>,
    rationale: String,
}

/// Convert Sable's generated checks into the typed `HiddenScenarioCheck` enum.
fn parse_sable_check(check: &SableGeneratedCheck) -> Option<HiddenScenarioCheck> {
    match check.kind.as_str() {
        "validation_passed" => Some(HiddenScenarioCheck::ValidationPassed),
        "exploration_log_exists" => Some(HiddenScenarioCheck::ExplorationLogExists),
        "artifact_exists" => check
            .path
            .as_ref()
            .map(|p| HiddenScenarioCheck::ArtifactExists { path: p.clone() }),
        "agent_executed" => check
            .agent
            .as_ref()
            .map(|a| HiddenScenarioCheck::AgentExecuted { agent: a.clone() }),
        "event_present" => match (&check.phase, &check.agent) {
            (Some(phase), Some(agent)) => Some(HiddenScenarioCheck::EventPresent {
                phase: phase.clone(),
                agent: agent.clone(),
            }),
            _ => None,
        },
        "artifact_contains_all" => match (&check.path, &check.contains_all) {
            (Some(path), Some(needles)) if !needles.is_empty() => {
                Some(HiddenScenarioCheck::ArtifactContainsAll {
                    path: path.clone(),
                    contains_all: needles.clone(),
                })
            }
            _ => None,
        },
        _ => {
            tracing::warn!(
                "Sable generated unknown check kind '{}' — skipping",
                check.kind
            );
            None
        }
    }
}

/// Ask Sable (claude-opus) to generate hidden scenarios for this run, then
/// evaluate them using the standard deterministic evaluator.
///
/// Returns `None` if Sable is not configured or the LLM call fails — the
/// caller should treat that as "no scenarios available."
pub async fn sable_generate_and_evaluate(
    spec: &Spec,
    setup: &SetupConfig,
    run_status: &str,
    run_attempt: usize,
    events: &[RunEvent],
    validation: &ValidationSummary,
    twin: &TwinEnvironment,
    agent_executions: &[AgentExecution],
    run_dir: &Path,
) -> Option<(HiddenScenarioSummary, String)> {
    let provider = llm::build_provider("sable", "claude-opus", setup)?;

    // Collect available artifacts so Sable knows what she can check against.
    let artifacts: Vec<String> = std::fs::read_dir(run_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();

    let agents_ran: Vec<&str> = agent_executions
        .iter()
        .map(|a| a.agent_name.as_str())
        .collect();
    let spec_yaml = serde_yaml::to_string(spec).unwrap_or_default();
    let validation_summary = serde_json::to_string_pretty(validation).unwrap_or_default();

    let system = "You are Sable, a hidden scenario evaluator for a software factory. \
Your job is to generate adversarial but fair hidden scenarios that verify a run did what the spec actually asked — \
not just that it didn't crash. \
Respond with a single raw JSON object only. No prose, no markdown fences. \
Schema: {\"scenarios\": [{\"id\": \"string\", \"title\": \"string\", \"description\": \"string\", \
\"checks\": [{\"kind\": \"string\", ...}]}], \"rationale\": \"string\"}. \
Available check kinds and their required fields: \
validation_passed (no extra fields); \
artifact_exists {path: \"filename\"}; \
agent_executed {agent: \"name\"}; \
event_present {phase: \"name\", agent: \"name\"}; \
artifact_contains_all {path: \"filename\", contains_all: [\"snippet1\", \"snippet2\"]}; \
exploration_log_exists (no extra fields). \
Generate 2-4 scenarios. Each scenario should have 1-3 checks. \
Only reference artifacts that are listed in AVAILABLE ARTIFACTS. \
Only reference agents listed in AGENTS THAT RAN. \
Be adversarial: check that the spec's core acceptance criteria are actually met, not just that the pipeline ran.";

    let user = format!(
        "SPEC:\n```yaml\n{spec_yaml}```\n\n\
RUN STATUS: {run_status}\n\
RUN ATTEMPT: {run_attempt}\n\n\
VISIBLE VALIDATION:\n```json\n{validation_summary}```\n\n\
AGENTS THAT RAN: {agents_list}\n\n\
AVAILABLE ARTIFACTS:\n{artifact_list}\n\n\
Generate hidden scenarios that verify the spec's acceptance criteria were genuinely met. \
Focus on what could pass visible validation but still fail the spec's intent.",
        agents_list = agents_ran.join(", "),
        artifact_list = artifacts
            .iter()
            .map(|a| format!("- {a}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let req = LlmRequest {
        messages: vec![Message::system(system), Message::user(user)],
        max_tokens: 2000,
        temperature: 0.3,
    };

    let raw = match provider.complete(req).await {
        Ok(resp) => resp.content,
        Err(e) => {
            tracing::warn!("Sable LLM call failed: {e}");
            return None;
        }
    };

    // Parse Sable's response.
    let stripped = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();

    let payload: SableGeneratedPayload = match serde_json::from_str(stripped) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(
                "Sable returned invalid JSON ({}): {}",
                e,
                &raw[..raw.len().min(300)]
            );
            return None;
        }
    };

    // Convert Sable's generated scenarios into typed HiddenScenarioFile entries.
    let definitions: Vec<HiddenScenarioFile> = payload
        .scenarios
        .iter()
        .map(|s| HiddenScenarioFile {
            id: s.id.clone(),
            spec_id: spec.id.clone(),
            title: s.title.clone(),
            description: s.description.clone(),
            checks: s.checks.iter().filter_map(parse_sable_check).collect(),
        })
        .collect();

    let summary = evaluate_hidden_scenarios(
        &definitions,
        run_status,
        run_attempt,
        events,
        validation,
        twin,
        agent_executions,
        run_dir,
    );

    Some((summary, payload.rationale))
}
