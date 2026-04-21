//! Twin Fidelity benchmark.
//!
//! Measures the recent distribution of `twin_fidelity_score` from
//! `coobie_episode_scores`, giving Harkonnen a simple internal baseline for how
//! often the twin environment is actually running enough declared services to
//! resemble production conditions.
//!
//! Env / manifest overrides:
//! - `TWIN_FIDELITY_LIMIT`        — max runs to include
//! - `TWIN_FIDELITY_OUTPUT`       — output directory
//! - `TWIN_FIDELITY_GOOD_MIN`     — optional threshold for "good" fidelity

use anyhow::{Context, Result};
use chrono::Utc;
use serde::Serialize;
use sqlx::{sqlite::SqliteConnectOptions, Row, SqlitePool};
use std::collections::BTreeMap;
use std::env;
use std::path::PathBuf;

use crate::{benchmark::BenchmarkStatus, config::Paths};

const DEFAULT_GOOD_THRESHOLD: f64 = 0.5;

#[derive(Debug, Clone)]
pub struct TwinFidelityRunConfig {
    pub output_dir: PathBuf,
    pub limit: Option<usize>,
    pub good_min: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TwinFidelityRunOutput {
    pub output_dir: PathBuf,
    pub summary_path: PathBuf,
    pub markdown_path: PathBuf,
    pub metrics: TwinFidelityMetrics,
}

#[derive(Debug, Clone)]
pub enum TwinFidelitySuiteOutcome {
    Completed(TwinFidelityRunOutput),
    Skipped(String),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TwinFidelityMetrics {
    pub total_runs: usize,
    pub good_threshold: f64,
    pub average_score: f64,
    pub min_score: f64,
    pub max_score: f64,
    pub median_score: f64,
    pub good_runs: usize,
    pub partial_runs: usize,
    pub simulated_or_low_runs: usize,
    pub good_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
struct TwinFidelityEntry {
    run_id: String,
    twin_fidelity_score: f64,
    scenario_passed: bool,
    validation_passed: bool,
    scored_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct TwinFidelitySummary {
    generated_at: String,
    limit: Option<usize>,
    metrics: TwinFidelityMetrics,
    entries: Vec<TwinFidelityEntry>,
}

pub async fn run_with_overrides(
    paths: &Paths,
    overrides: &BTreeMap<String, String>,
) -> Result<TwinFidelitySuiteOutcome> {
    let limit = read_env("TWIN_FIDELITY_LIMIT", overrides).and_then(|value| value.parse().ok());
    let good_min = read_env("TWIN_FIDELITY_GOOD_MIN", overrides)
        .and_then(|value| value.parse().ok())
        .unwrap_or(DEFAULT_GOOD_THRESHOLD);
    let output_dir = read_env("TWIN_FIDELITY_OUTPUT", overrides)
        .map(PathBuf::from)
        .unwrap_or_else(|| paths.artifacts.join("benchmarks").join("twin_fidelity"));

    let config = TwinFidelityRunConfig {
        output_dir,
        limit,
        good_min,
    };

    let pool = open_pool(paths).await?;
    let rows = load_entries(&pool, config.limit).await?;
    if rows.is_empty() {
        return Ok(TwinFidelitySuiteOutcome::Skipped(
            "No coobie_episode_scores rows available for twin-fidelity benchmarking.".to_string(),
        ));
    }

    tokio::fs::create_dir_all(&config.output_dir)
        .await
        .with_context(|| format!("creating {}", config.output_dir.display()))?;

    let metrics = compute_metrics(&rows, config.good_min);
    let summary = TwinFidelitySummary {
        generated_at: Utc::now().to_rfc3339(),
        limit: config.limit,
        metrics: metrics.clone(),
        entries: rows,
    };

    let summary_path = config.output_dir.join("twin_fidelity_summary.json");
    let markdown_path = config.output_dir.join("twin_fidelity_report.md");
    tokio::fs::write(&summary_path, serde_json::to_string_pretty(&summary)?)
        .await
        .with_context(|| format!("writing {}", summary_path.display()))?;
    tokio::fs::write(&markdown_path, render_markdown(&summary))
        .await
        .with_context(|| format!("writing {}", markdown_path.display()))?;

    Ok(TwinFidelitySuiteOutcome::Completed(TwinFidelityRunOutput {
        output_dir: config.output_dir,
        summary_path,
        markdown_path,
        metrics,
    }))
}

pub fn status_for_output(_: &TwinFidelityRunOutput) -> BenchmarkStatus {
    BenchmarkStatus::Passed
}

pub fn reason_for_output(_: &TwinFidelityRunOutput) -> Option<String> {
    None
}

pub fn render_step_stdout(output: &TwinFidelityRunOutput) -> String {
    format!(
        "TwinFidelity n={} avg={:.2} median={:.2} min={:.2} max={:.2} good_rate={:.1}%\n",
        output.metrics.total_runs,
        output.metrics.average_score,
        output.metrics.median_score,
        output.metrics.min_score,
        output.metrics.max_score,
        output.metrics.good_rate * 100.0,
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

async fn load_entries(pool: &SqlitePool, limit: Option<usize>) -> Result<Vec<TwinFidelityEntry>> {
    let row_limit = limit.unwrap_or(50) as i64;
    let rows = sqlx::query(
        "SELECT run_id, twin_fidelity_score, scenario_passed, validation_passed, scored_at
         FROM coobie_episode_scores
         ORDER BY scored_at DESC
         LIMIT ?1",
    )
    .bind(row_limit)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|row| TwinFidelityEntry {
            run_id: row.get::<String, _>("run_id"),
            twin_fidelity_score: row.get::<f64, _>("twin_fidelity_score"),
            scenario_passed: row.get::<i64, _>("scenario_passed") != 0,
            validation_passed: row.get::<i64, _>("validation_passed") != 0,
            scored_at: row.get::<String, _>("scored_at"),
        })
        .collect())
}

fn compute_metrics(entries: &[TwinFidelityEntry], good_min: f64) -> TwinFidelityMetrics {
    if entries.is_empty() {
        return TwinFidelityMetrics {
            good_threshold: good_min,
            ..Default::default()
        };
    }

    let mut scores = entries
        .iter()
        .map(|entry| entry.twin_fidelity_score)
        .collect::<Vec<_>>();
    scores.sort_by(|a, b| a.total_cmp(b));

    let total_runs = scores.len();
    let score_sum = scores.iter().sum::<f64>();
    let average_score = score_sum / total_runs as f64;
    let min_score = *scores.first().unwrap_or(&0.0);
    let max_score = *scores.last().unwrap_or(&0.0);
    let median_score = if total_runs % 2 == 1 {
        scores[total_runs / 2]
    } else {
        let upper = scores[total_runs / 2];
        let lower = scores[(total_runs / 2) - 1];
        (lower + upper) / 2.0
    };

    let good_runs = scores.iter().filter(|score| **score > good_min).count();
    let partial_runs = scores
        .iter()
        .filter(|score| **score > 0.1 && **score <= good_min)
        .count();
    let simulated_or_low_runs = scores.iter().filter(|score| **score <= 0.1).count();
    let good_rate = good_runs as f64 / total_runs as f64;

    TwinFidelityMetrics {
        total_runs,
        good_threshold: good_min,
        average_score,
        min_score,
        max_score,
        median_score,
        good_runs,
        partial_runs,
        simulated_or_low_runs,
        good_rate,
    }
}

fn render_markdown(summary: &TwinFidelitySummary) -> String {
    let metrics = &summary.metrics;
    let mut out = vec![
        "# Twin Fidelity".to_string(),
        String::new(),
        format!("- Generated: {}", summary.generated_at),
        format!("- Runs included: {}", metrics.total_runs),
        format!("- Average score: {:.2}", metrics.average_score),
        format!("- Median score: {:.2}", metrics.median_score),
        format!("- Min score: {:.2}", metrics.min_score),
        format!("- Max score: {:.2}", metrics.max_score),
        format!(
            "- Good threshold: {:.2} (good rate {:.1}%)",
            metrics.good_threshold,
            metrics.good_rate * 100.0
        ),
        format!("- Good runs: {}", metrics.good_runs),
        format!("- Partial runs: {}", metrics.partial_runs),
        format!("- Simulated or low runs: {}", metrics.simulated_or_low_runs),
        String::new(),
        "## Runs".to_string(),
    ];

    for entry in &summary.entries {
        out.push(format!(
            "- {} score={:.2} visible={} hidden={} scored_at={}",
            entry.run_id,
            entry.twin_fidelity_score,
            entry.validation_passed,
            entry.scenario_passed,
            entry.scored_at
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
    use super::{compute_metrics, TwinFidelityEntry};

    #[test]
    fn twin_fidelity_distribution_metrics_are_derived_correctly() {
        let entries = vec![
            TwinFidelityEntry {
                run_id: "a".to_string(),
                twin_fidelity_score: 0.1,
                scenario_passed: false,
                validation_passed: false,
                scored_at: "2026-01-01T00:00:00Z".to_string(),
            },
            TwinFidelityEntry {
                run_id: "b".to_string(),
                twin_fidelity_score: 0.6,
                scenario_passed: false,
                validation_passed: true,
                scored_at: "2026-01-02T00:00:00Z".to_string(),
            },
            TwinFidelityEntry {
                run_id: "c".to_string(),
                twin_fidelity_score: 0.9,
                scenario_passed: true,
                validation_passed: true,
                scored_at: "2026-01-03T00:00:00Z".to_string(),
            },
        ];

        let metrics = compute_metrics(&entries, 0.5);
        assert_eq!(metrics.total_runs, 3);
        assert_eq!(metrics.good_runs, 2);
        assert_eq!(metrics.partial_runs, 0);
        assert_eq!(metrics.simulated_or_low_runs, 1);
        assert!((metrics.average_score - (1.6 / 3.0)).abs() < 1e-9);
        assert!((metrics.median_score - 0.6).abs() < 1e-9);
        assert!((metrics.good_rate - (2.0 / 3.0)).abs() < 1e-9);
    }
}
