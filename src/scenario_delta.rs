//! Hidden Scenario Delta benchmark.
//!
//! Measures the gap between visible validation pass rate and hidden scenario
//! pass rate over recent runs, using the persisted Coobie episode-score table.
//!
//! Env / manifest overrides:
//! - `SCENARIO_DELTA_LIMIT`   — max runs to include
//! - `SCENARIO_DELTA_OUTPUT`  — output directory

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use crate::{benchmark::BenchmarkStatus, config::Paths};

#[derive(Debug, Clone)]
pub struct ScenarioDeltaRunConfig {
    pub output_dir: PathBuf,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScenarioDeltaRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub metrics: ScenarioDeltaMetrics,
}

#[derive(Debug, Clone)]
pub enum ScenarioDeltaSuiteOutcome {
    Completed(ScenarioDeltaRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScenarioDeltaMetrics {
    pub total_runs: usize,
    pub visible_passed_runs: usize,
    pub hidden_passed_runs: usize,
    pub visible_only_failures: usize,
    pub hidden_only_passes: usize,
    pub both_passed_runs: usize,
    pub both_failed_runs: usize,
    pub visible_pass_rate: f64,
    pub hidden_pass_rate: f64,
    pub delta: f64,
}

#[derive(Debug, Clone, Serialize)]
struct ScenarioDeltaEntry {
    run_id: String,
    validation_passed: bool,
    scenario_passed: bool,
    scored_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct ScenarioDeltaSummary {
    generated_at: String,
    limit: Option<usize>,
    metrics: ScenarioDeltaMetrics,
    entries: Vec<ScenarioDeltaEntry>,
}

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<ScenarioDeltaSuiteOutcome> {
    let limit = read_env("SCENARIO_DELTA_LIMIT", overrides).and_then(|value| value.parse().ok());
    let output_dir = read_env("SCENARIO_DELTA_OUTPUT", overrides)
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.artifacts.join("benchmarks").join("scenario_delta"));

    let config = ScenarioDeltaRunConfig { output_dir, limit };
    let pool = open_pool(paths).await?;
    let rows = load_entries(&pool, config.limit).await?;
    if rows.is_empty() {
        return Ok(ScenarioDeltaSuiteOutcome::Skipped(
            "No coobie_episode_scores rows available for scenario-delta benchmarking.".to_string(),
        ));
    }

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .with_context(|| format!("creating {}", config.output_dir.display()))?;

    let metrics = compute_metrics(&rows);
    let summary = ScenarioDeltaSummary {
        generated_at: Utc::now().to_rfc3339(),
        limit: config.limit,
        metrics: metrics.clone(),
        entries: rows,
    };

    let summary_path = config.output_dir.join("scenario_delta_summary.json");
    let markdown_path = config.output_dir.join("scenario_delta_report.md");
    tokio::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .await
        .with_context(|| format!("writing {}", summary_path.display()))?;
    tokio::fs::write(&markdown_path, render_markdown(&summary))
        .await
        .with_context(|| format!("writing {}", markdown_path.display()))?;

    Ok(ScenarioDeltaSuiteOutcome::Completed(
        ScenarioDeltaRunOutput {
            output_dir: config.output_dir,
            summary_path,
            markdown_path,
            metrics,
        },
    ))
}

pub fn status_for_output(_: &ScenarioDeltaRunOutput) -> BenchmarkStatus {
    BenchmarkStatus::Passed
}

pub fn reason_for_output(_: &ScenarioDeltaRunOutput) -> Option<String> {
    None
}

pub fn render_step_stdout(output: &ScenarioDeltaRunOutput) -> String {
    format!(
        "ScenarioDelta n={} visible={:.1}% hidden={:.1}% delta={:.1}% visible_only_failures={}\n",
        output.metrics.total_runs,
        output.metrics.visible_pass_rate * 100.0,
        output.metrics.hidden_pass_rate * 100.0,
        output.metrics.delta * 100.0,
        output.metrics.visible_only_failures,
    )
}

async fn open_pool(paths: &Paths) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.db_file)
        .create_if_missing(false);
    SqlitePool::connect_with(options)
        .await
        .with_context(|| format!("opening sqlite db {}", paths.db_file.display()))
}

async fn load_entries(pool: &SqlitePool, limit: Option<usize>) -> Result<Vec<ScenarioDeltaEntry>> {
    let row_limit = limit.unwrap_or(50) as i64;
    let rows = sqlx::query(
        "SELECT run_id, validation_passed, scenario_passed, scored_at
         FROM coobie_episode_scores
         ORDER BY scored_at DESC
         LIMIT ?1",
    )
    .bind(row_limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| ScenarioDeltaEntry {
            run_id: row.get::<String, _>("run_id"),
            validation_passed: row.get::<i64, _>("validation_passed") != 0,
            scenario_passed: row.get::<i64, _>("scenario_passed") != 0,
            scored_at: row.get::<String, _>("scored_at"),
        })
        .collect())
}

fn compute_metrics(entries: &[ScenarioDeltaEntry]) -> ScenarioDeltaMetrics {
    let total_runs = entries.len();
    let visible_passed_runs = entries
        .iter()
        .filter(|entry| entry.validation_passed)
        .count();
    let hidden_passed_runs = entries.iter().filter(|entry| entry.scenario_passed).count();
    let visible_only_failures = entries
        .iter()
        .filter(|entry| entry.validation_passed && !entry.scenario_passed)
        .count();
    let hidden_only_passes = entries
        .iter()
        .filter(|entry| !entry.validation_passed && entry.scenario_passed)
        .count();
    let both_passed_runs = entries
        .iter()
        .filter(|entry| entry.validation_passed && entry.scenario_passed)
        .count();
    let both_failed_runs = entries
        .iter()
        .filter(|entry| !entry.validation_passed && !entry.scenario_passed)
        .count();
    let visible_pass_rate = if total_runs == 0 {
        0.0
    } else {
        visible_passed_runs as f64 / total_runs as f64
    };
    let hidden_pass_rate = if total_runs == 0 {
        0.0
    } else {
        hidden_passed_runs as f64 / total_runs as f64
    };

    ScenarioDeltaMetrics {
        total_runs,
        visible_passed_runs,
        hidden_passed_runs,
        visible_only_failures,
        hidden_only_passes,
        both_passed_runs,
        both_failed_runs,
        visible_pass_rate,
        hidden_pass_rate,
        delta: visible_pass_rate - hidden_pass_rate,
    }
}

fn render_markdown(summary: &ScenarioDeltaSummary) -> String {
    let metrics = &summary.metrics;
    let mut out = vec![
        "# Hidden Scenario Delta".to_string(),
        String::new(),
        format!("- Generated: {}", summary.generated_at),
        format!("- Runs included: {}", metrics.total_runs),
        format!(
            "- Visible pass rate: {:.1}% ({}/{})",
            metrics.visible_pass_rate * 100.0,
            metrics.visible_passed_runs,
            metrics.total_runs
        ),
        format!(
            "- Hidden pass rate: {:.1}% ({}/{})",
            metrics.hidden_pass_rate * 100.0,
            metrics.hidden_passed_runs,
            metrics.total_runs
        ),
        format!("- Delta: {:.1}%", metrics.delta * 100.0),
        format!("- Visible-only failures: {}", metrics.visible_only_failures),
        format!("- Hidden-only passes: {}", metrics.hidden_only_passes),
        format!("- Both passed: {}", metrics.both_passed_runs),
        format!("- Both failed: {}", metrics.both_failed_runs),
        String::new(),
        "## Runs".to_string(),
    ];

    for entry in &summary.entries {
        out.push(format!(
            "- {} visible={} hidden={} scored_at={}",
            entry.run_id, entry.validation_passed, entry.scenario_passed, entry.scored_at
        ));
    }

    out.join("\n")
}

fn read_env(name: &str, overrides: &BTreeMap<String, String>) -> Option<String> {
    overrides
        .get(name)
        .cloned()
        .or_else(|| env::var(name).ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::{compute_metrics, ScenarioDeltaEntry};

    #[test]
    fn delta_metrics_are_computed_from_visibility_gap() {
        let entries = vec![
            ScenarioDeltaEntry {
                run_id: "a".to_string(),
                validation_passed: true,
                scenario_passed: false,
                scored_at: "2026-01-01T00:00:00Z".to_string(),
            },
            ScenarioDeltaEntry {
                run_id: "b".to_string(),
                validation_passed: true,
                scenario_passed: true,
                scored_at: "2026-01-02T00:00:00Z".to_string(),
            },
            ScenarioDeltaEntry {
                run_id: "c".to_string(),
                validation_passed: false,
                scenario_passed: false,
                scored_at: "2026-01-03T00:00:00Z".to_string(),
            },
        ];
        let metrics = compute_metrics(&entries);
        assert_eq!(metrics.total_runs, 3);
        assert_eq!(metrics.visible_only_failures, 1);
        assert!((metrics.visible_pass_rate - (2.0 / 3.0)).abs() < 1e-9);
        assert!((metrics.hidden_pass_rate - (1.0 / 3.0)).abs() < 1e-9);
        assert!((metrics.delta - (1.0 / 3.0)).abs() < 1e-9);
    }
}
