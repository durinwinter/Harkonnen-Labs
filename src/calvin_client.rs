use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::setup::CalvinConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveExperience {
    pub run_id: String,
    pub episode_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub narrative_summary: String,
    pub scope: String,
    pub chamber: Chamber,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Chamber {
    Mythos,
    Episteme,
    Ethos,
    Pathos,
    Logos,
    Praxis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefRevision {
    pub belief_id: String,
    pub revised_summary: String,
    pub new_confidence: f64,
    pub revision_reason: String,
    pub preservation_note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub agent_id: String,
    pub run_id: String,
    pub phase: Option<String>,
    pub action_type: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub outcome: String,
    pub latency_ms: Option<i32>,
    pub tokens_in: Option<i32>,
    pub tokens_out: Option<i32>,
    pub drift_score: Option<f64>,
    pub lab_ness_score: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsSnapshot {
    pub agent_id: String,
    pub d_star: f64,
    pub ssa: f64,
    pub stress: f64,
    pub hysteresis: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct CalvinClient {
    base_url: String,
    client: Client,
}

impl CalvinClient {
    pub fn new(config: &CalvinConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_millis(500))
            .build()
            .context("building CalvinClient")?;
        Ok(Self {
            base_url: config.harmony_url.trim_end_matches('/').to_string(),
            client,
        })
    }

    pub async fn health_check(&self) -> bool {
        self.client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false)
    }

    pub async fn status(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/status", self.base_url))
            .send()
            .await
            .context("GET /status")?;
        Ok(resp.json().await?)
    }

    pub async fn open_run(
        &self,
        run_id: &str,
        spec_id: &str,
        provider: &str,
        model: &str,
    ) -> Result<()> {
        let body = serde_json::json!({
            "run_id": run_id,
            "spec_id": spec_id,
            "provider": provider,
            "model": model,
        });
        self.client
            .post(format!("{}/runs", self.base_url))
            .json(&body)
            .send()
            .await
            .context("POST /runs")?;
        Ok(())
    }

    pub async fn record_experience(&self, run_id: &str, exp: &ArchiveExperience) -> Result<()> {
        self.client
            .post(format!("{}/runs/{run_id}/experiences", self.base_url))
            .json(exp)
            .send()
            .await
            .context("POST /runs/{run_id}/experiences")?;
        Ok(())
    }

    pub async fn revise_belief(&self, run_id: &str, rev: &BeliefRevision) -> Result<()> {
        self.client
            .post(format!("{}/runs/{run_id}/beliefs", self.base_url))
            .json(rev)
            .send()
            .await
            .context("POST /runs/{run_id}/beliefs")?;
        Ok(())
    }

    pub async fn close_run(&self, run_id: &str, outcome: &str) -> Result<()> {
        let body = serde_json::json!({"outcome": outcome});
        self.client
            .patch(format!("{}/runs/{run_id}/close", self.base_url))
            .json(&body)
            .send()
            .await
            .context("PATCH /runs/{run_id}/close")?;
        Ok(())
    }

    pub async fn update_agent_status(&self, agent_name: &str, status: &str) -> Result<()> {
        self.client
            .patch(format!("{}/agents/{agent_name}/status", self.base_url))
            .json(&serde_json::json!({"status": status}))
            .send()
            .await
            .context("PATCH /agents/{agent_name}/status")?;
        Ok(())
    }

    pub async fn get_kernel_traits(&self, agent_name: &str) -> Result<Vec<String>> {
        let resp = self
            .client
            .get(format!("{}/agents/{agent_name}/traits", self.base_url))
            .send()
            .await
            .context("GET /agents/{agent_name}/traits")?;
        Ok(resp.json().await?)
    }

    pub async fn get_active_beliefs(&self, agent_name: &str) -> Result<Vec<String>> {
        let resp = self
            .client
            .get(format!("{}/agents/{agent_name}/beliefs", self.base_url))
            .send()
            .await
            .context("GET /agents/{agent_name}/beliefs")?;
        Ok(resp.json().await?)
    }

    pub async fn check_adaptation_safe(
        &self,
        agent_name: &str,
        adaptation_summary: &str,
    ) -> Result<bool> {
        let body = serde_json::json!({"adaptation_summary": adaptation_summary});
        let resp = self
            .client
            .post(format!("{}/agents/{agent_name}/check", self.base_url))
            .json(&body)
            .send()
            .await
            .context("POST /agents/{agent_name}/check")?;
        let v: serde_json::Value = resp.json().await?;
        Ok(v["safe"].as_bool().unwrap_or(true))
    }

    pub async fn get_metrics(&self, agent_name: &str) -> Result<MetricsSnapshot> {
        let resp = self
            .client
            .get(format!("{}/agents/{agent_name}/metrics", self.base_url))
            .send()
            .await
            .context("GET /agents/{agent_name}/metrics")?;
        Ok(resp.json().await?)
    }

    pub async fn write_event(&self, evt: &TelemetryEvent) -> Result<()> {
        self.client
            .post(format!("{}/telemetry", self.base_url))
            .json(evt)
            .send()
            .await
            .context("POST /telemetry")?;
        Ok(())
    }

    pub async fn write_events_batch(&self, evts: &[TelemetryEvent]) -> Result<()> {
        self.client
            .post(format!("{}/telemetry/batch", self.base_url))
            .json(&evts)
            .send()
            .await
            .context("POST /telemetry/batch")?;
        Ok(())
    }
}

/// Try to create a CalvinClient; returns None with a warning if disabled or unreachable.
pub async fn try_connect(config: &CalvinConfig) -> Option<CalvinClient> {
    if !config.enabled {
        return None;
    }
    match CalvinClient::new(config) {
        Ok(client) => {
            if client.health_check().await {
                Some(client)
            } else {
                tracing::warn!(
                    "Calvin Archive enabled but harmony is not responding at {}",
                    config.harmony_url
                );
                None
            }
        }
        Err(e) => {
            tracing::warn!("Calvin Archive disabled — client init failed: {e}");
            None
        }
    }
}
