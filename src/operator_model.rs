#![allow(dead_code)]

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::models::{
    OperatorModelEntry, OperatorModelExport, OperatorModelLayerCheckpoint, OperatorModelProfile,
    OperatorModelSession, OperatorModelUpdateCandidate,
};

#[derive(Debug, Clone)]
pub struct OperatorModelStore {
    pool: SqlitePool,
}

impl OperatorModelStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn create_profile(
        &self,
        scope: &str,
        project_root: Option<&str>,
        display_name: &str,
    ) -> Result<OperatorModelProfile> {
        let profile_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO operator_model_profiles
                (profile_id, scope, project_root, display_name, status, current_version, created_at, updated_at)
            VALUES
                (?1, ?2, ?3, ?4, 'active', 0, ?5, ?6)
            "#,
        )
        .bind(&profile_id)
        .bind(scope)
        .bind(project_root)
        .bind(display_name)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert operator_model_profile")?;

        Ok(OperatorModelProfile {
            profile_id,
            scope: scope.to_string(),
            project_root: project_root.map(|value| value.to_string()),
            display_name: display_name.to_string(),
            status: "active".to_string(),
            current_version: 0,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_profile(&self, profile_id: &str) -> Result<Option<OperatorModelProfile>> {
        let row = sqlx::query(
            r#"
            SELECT profile_id, scope, project_root, display_name, status, current_version, created_at, updated_at
            FROM operator_model_profiles
            WHERE profile_id = ?1
            "#,
        )
        .bind(profile_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_profile).transpose()
    }

    pub async fn list_profiles(&self) -> Result<Vec<OperatorModelProfile>> {
        let rows = sqlx::query(
            r#"
            SELECT profile_id, scope, project_root, display_name, status, current_version, created_at, updated_at
            FROM operator_model_profiles
            ORDER BY updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(parse_profile).collect()
    }

    pub async fn create_session(
        &self,
        profile_id: &str,
        thread_id: Option<&str>,
        pending_layer: Option<&str>,
        started_by: Option<&str>,
    ) -> Result<OperatorModelSession> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO operator_model_sessions
                (session_id, profile_id, thread_id, status, pending_layer, started_by, created_at, updated_at, completed_at)
            VALUES
                (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, NULL)
            "#,
        )
        .bind(&session_id)
        .bind(profile_id)
        .bind(thread_id)
        .bind(pending_layer)
        .bind(started_by)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert operator_model_session")?;

        Ok(OperatorModelSession {
            session_id,
            profile_id: profile_id.to_string(),
            thread_id: thread_id.map(|value| value.to_string()),
            status: "active".to_string(),
            pending_layer: pending_layer.map(|value| value.to_string()),
            started_by: started_by.map(|value| value.to_string()),
            created_at: now,
            updated_at: now,
            completed_at: None,
        })
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Option<OperatorModelSession>> {
        let row = sqlx::query(
            r#"
            SELECT session_id, profile_id, thread_id, status, pending_layer, started_by, created_at, updated_at, completed_at
            FROM operator_model_sessions
            WHERE session_id = ?1
            "#,
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_session).transpose()
    }
}

fn parse_profile(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelProfile> {
    Ok(OperatorModelProfile {
        profile_id: row.get("profile_id"),
        scope: row.get("scope"),
        project_root: row.get("project_root"),
        display_name: row.get("display_name"),
        status: row.get("status"),
        current_version: row.get("current_version"),
        created_at: parse_dt(row.get("created_at"))?,
        updated_at: parse_dt(row.get("updated_at"))?,
    })
}

fn parse_session(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelSession> {
    Ok(OperatorModelSession {
        session_id: row.get("session_id"),
        profile_id: row.get("profile_id"),
        thread_id: row.get("thread_id"),
        status: row.get("status"),
        pending_layer: row.get("pending_layer"),
        started_by: row.get("started_by"),
        created_at: parse_dt(row.get("created_at"))?,
        updated_at: parse_dt(row.get("updated_at"))?,
        completed_at: parse_optional_dt(row.get("completed_at"))?,
    })
}

#[allow(dead_code)]
fn parse_checkpoint(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelLayerCheckpoint> {
    Ok(OperatorModelLayerCheckpoint {
        checkpoint_id: row.get("checkpoint_id"),
        session_id: row.get("session_id"),
        profile_id: row.get("profile_id"),
        version: row.get("version"),
        layer: row.get("layer"),
        status: row.get("status"),
        summary_md: row.get("summary_md"),
        raw_notes_json: parse_json_value(row.get("raw_notes_json"))?,
        approved_by: row.get("approved_by"),
        created_at: parse_dt(row.get("created_at"))?,
        approved_at: parse_optional_dt(row.get("approved_at"))?,
    })
}

#[allow(dead_code)]
fn parse_entry(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelEntry> {
    Ok(OperatorModelEntry {
        entry_id: row.get("entry_id"),
        profile_id: row.get("profile_id"),
        version: row.get("version"),
        layer: row.get("layer"),
        entry_type: row.get("entry_type"),
        title: row.get("title"),
        content: row.get("content"),
        details_json: parse_json_value(row.get("details_json"))?,
        source_checkpoint_id: row.get("source_checkpoint_id"),
        status: row.get("status"),
        superseded_by: row.get("superseded_by"),
        created_at: parse_dt(row.get("created_at"))?,
    })
}

#[allow(dead_code)]
fn parse_export(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelExport> {
    Ok(OperatorModelExport {
        export_id: row.get("export_id"),
        profile_id: row.get("profile_id"),
        version: row.get("version"),
        artifact_name: row.get("artifact_name"),
        content: row.get("content"),
        content_type: row.get("content_type"),
        created_at: parse_dt(row.get("created_at"))?,
    })
}

#[allow(dead_code)]
fn parse_update_candidate(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelUpdateCandidate> {
    Ok(OperatorModelUpdateCandidate {
        candidate_id: row.get("candidate_id"),
        profile_id: row.get("profile_id"),
        run_id: row.get("run_id"),
        entry_id: row.get("entry_id"),
        proposal_kind: row.get("proposal_kind"),
        summary: row.get("summary"),
        proposal_json: parse_json_value(row.get("proposal_json"))?,
        status: row.get("status"),
        confidence: row.get("confidence"),
        created_at: parse_dt(row.get("created_at"))?,
        reviewed_at: parse_optional_dt(row.get("reviewed_at"))?,
    })
}

fn parse_dt(value: String) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(&value)?.with_timezone(&Utc))
}

fn parse_optional_dt(value: Option<String>) -> Result<Option<DateTime<Utc>>> {
    value.map(parse_dt).transpose()
}

fn parse_json_value(value: String) -> Result<Value> {
    Ok(serde_json::from_str(&value)?)
}
