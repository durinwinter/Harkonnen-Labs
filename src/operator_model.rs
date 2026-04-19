#![allow(dead_code)]

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::{
    config::Paths,
    models::{
        OperatorModelEntry, OperatorModelExport, OperatorModelLayerCheckpoint,
        OperatorModelProfile, OperatorModelScope, OperatorModelSession,
        OperatorModelUpdateCandidate,
    },
};

pub const OPERATOR_MODEL_DIRNAME: &str = "operator-model";
pub const DEFAULT_OPERATOR_MODEL_LAYER: &str = "operating_rhythms";
pub const LIGHT_GLOBAL_PROFILE_TOPICS: &[&str] = &[
    "communication style",
    "approval boundaries",
    "escalation style",
    "working rhythm",
];

#[derive(Debug, Clone)]
pub struct OperatorModelStore {
    pool: SqlitePool,
}

impl OperatorModelStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Project profiles are the product default: stamp the commissioned repo first,
    /// then fall back to a light global baseline only when no project profile exists.
    pub async fn create_project_profile(
        &self,
        project_root: &Path,
        display_name: &str,
    ) -> Result<OperatorModelProfile> {
        self.create_profile(
            OperatorModelScope::Project,
            Some(path_to_string(project_root)?),
            display_name,
        )
        .await
    }

    /// Global profiles are intentionally small and stable. They should capture only
    /// broad operator defaults, not repo-specific workflow detail.
    pub async fn create_global_profile(&self, display_name: &str) -> Result<OperatorModelProfile> {
        self.create_profile(OperatorModelScope::Global, None, display_name)
            .await
    }

    pub async fn create_profile(
        &self,
        scope: OperatorModelScope,
        project_root: Option<String>,
        display_name: &str,
    ) -> Result<OperatorModelProfile> {
        if scope == OperatorModelScope::Project && project_root.is_none() {
            return Err(anyhow!(
                "project operator-model profiles require a project_root"
            ));
        }
        if scope == OperatorModelScope::Global && project_root.is_some() {
            return Err(anyhow!(
                "global operator-model profiles cannot carry a project_root"
            ));
        }

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
        .bind(scope.as_str())
        .bind(project_root.as_deref())
        .bind(display_name)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("insert operator_model_profile")?;

        Ok(OperatorModelProfile {
            profile_id,
            scope,
            project_root,
            display_name: display_name.to_string(),
            status: "active".to_string(),
            current_version: 0,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn find_project_profile(
        &self,
        project_root: &Path,
    ) -> Result<Option<OperatorModelProfile>> {
        let project_root = path_to_string(project_root)?;
        let row = sqlx::query(
            r#"
            SELECT profile_id, scope, project_root, display_name, status, current_version, created_at, updated_at
            FROM operator_model_profiles
            WHERE scope = 'project' AND project_root = ?1 AND status = 'active'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(project_root)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_profile).transpose()
    }

    pub async fn find_active_global_profile(&self) -> Result<Option<OperatorModelProfile>> {
        let row = sqlx::query(
            r#"
            SELECT profile_id, scope, project_root, display_name, status, current_version, created_at, updated_at
            FROM operator_model_profiles
            WHERE scope = 'global' AND status = 'active'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_profile).transpose()
    }

    /// Project-first resolution used by commissioning and preflight.
    pub async fn resolve_effective_profile(
        &self,
        project_root: &Path,
    ) -> Result<Option<OperatorModelProfile>> {
        if let Some(profile) = self.find_project_profile(project_root).await? {
            return Ok(Some(profile));
        }
        self.find_active_global_profile().await
    }

    pub async fn ensure_project_profile(
        &self,
        project_root: &Path,
        display_name: &str,
    ) -> Result<OperatorModelProfile> {
        if let Some(profile) = self.find_project_profile(project_root).await? {
            return Ok(profile);
        }
        self.create_project_profile(project_root, display_name)
            .await
    }

    pub async fn start_project_session(
        &self,
        project_root: &Path,
        display_name: &str,
        thread_id: Option<&str>,
        started_by: Option<&str>,
    ) -> Result<OperatorModelSession> {
        let profile = self
            .ensure_project_profile(project_root, display_name)
            .await?;
        self.create_session(
            &profile.profile_id,
            thread_id,
            Some(DEFAULT_OPERATOR_MODEL_LAYER),
            started_by,
        )
        .await
    }

    pub async fn find_active_session_for_profile(
        &self,
        profile_id: &str,
    ) -> Result<Option<OperatorModelSession>> {
        let row = sqlx::query(
            r#"
            SELECT session_id, profile_id, thread_id, status, pending_layer, started_by, created_at, updated_at, completed_at
            FROM operator_model_sessions
            WHERE profile_id = ?1 AND status IN ('active', 'review')
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
        )
        .bind(profile_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(parse_session).transpose()
    }

    pub async fn update_session_thread(&self, session_id: &str, thread_id: &str) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            "UPDATE operator_model_sessions SET thread_id = ?2, updated_at = ?3 WHERE session_id = ?1",
        )
        .bind(session_id)
        .bind(thread_id)
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await
        .context("update operator_model_session thread")?;
        Ok(())
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

    pub async fn list_profiles_by_scope(
        &self,
        scope: OperatorModelScope,
    ) -> Result<Vec<OperatorModelProfile>> {
        let rows = sqlx::query(
            r#"
            SELECT profile_id, scope, project_root, display_name, status, current_version, created_at, updated_at
            FROM operator_model_profiles
            WHERE scope = ?1
            ORDER BY updated_at DESC
            "#,
        )
        .bind(scope.as_str())
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

    pub fn export_root_for_profile(
        &self,
        paths: &Paths,
        profile: &OperatorModelProfile,
    ) -> PathBuf {
        match profile.scope {
            OperatorModelScope::Project => profile
                .project_root
                .as_deref()
                .map(PathBuf::from)
                .unwrap_or_else(|| paths.products.clone())
                .join(".harkonnen")
                .join(OPERATOR_MODEL_DIRNAME),
            OperatorModelScope::Global => paths.factory.join(OPERATOR_MODEL_DIRNAME).join("global"),
        }
    }

    /// Persist an approved checkpoint for a layer and advance the session's pending_layer.
    /// `next_layer` is `None` when both MVP layers are complete.
    pub async fn save_layer_checkpoint(
        &self,
        session_id: &str,
        profile_id: &str,
        layer: &str,
        summary_md: &str,
        raw_notes: &Value,
        approved_by: Option<&str>,
        next_layer: Option<&str>,
    ) -> Result<OperatorModelLayerCheckpoint> {
        let checkpoint_id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let raw_notes_json = serde_json::to_string(raw_notes)?;

        let version = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM operator_model_layer_checkpoints WHERE profile_id = ?1",
        )
        .bind(profile_id)
        .fetch_one(&self.pool)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO operator_model_layer_checkpoints
                (checkpoint_id, session_id, profile_id, version, layer, status, summary_md, raw_notes_json, approved_by, created_at, approved_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'approved', ?6, ?7, ?8, ?9, ?9)
            "#,
        )
        .bind(&checkpoint_id)
        .bind(session_id)
        .bind(profile_id)
        .bind(version)
        .bind(layer)
        .bind(summary_md)
        .bind(&raw_notes_json)
        .bind(approved_by)
        .bind(&now)
        .execute(&self.pool)
        .await?;

        // Advance session
        let (new_status, new_layer): (&str, Option<&str>) = match next_layer {
            Some(l) => ("active", Some(l)),
            None => ("completed", None),
        };
        sqlx::query(
            "UPDATE operator_model_sessions SET status = ?1, pending_layer = ?2, updated_at = ?3 WHERE session_id = ?4",
        )
        .bind(new_status)
        .bind(new_layer)
        .bind(&now)
        .bind(session_id)
        .execute(&self.pool)
        .await?;

        let checkpoint = OperatorModelLayerCheckpoint {
            checkpoint_id,
            session_id: session_id.to_string(),
            profile_id: profile_id.to_string(),
            version,
            layer: layer.to_string(),
            status: "approved".to_string(),
            summary_md: summary_md.to_string(),
            raw_notes_json: raw_notes.clone(),
            approved_by: approved_by.map(str::to_string),
            created_at: DateTime::parse_from_rfc3339(&now)?.with_timezone(&Utc),
            approved_at: Some(DateTime::parse_from_rfc3339(&now)?.with_timezone(&Utc)),
        };
        Ok(checkpoint)
    }

    /// Return all approved checkpoints for a profile, ordered by version.
    pub async fn list_approved_checkpoints_for_profile(
        &self,
        profile_id: &str,
    ) -> Result<Vec<OperatorModelLayerCheckpoint>> {
        let rows = sqlx::query(
            r#"
            SELECT checkpoint_id, session_id, profile_id, version, layer, status,
                   summary_md, raw_notes_json, approved_by, created_at, approved_at
            FROM operator_model_layer_checkpoints
            WHERE profile_id = ?1 AND status = 'approved'
            ORDER BY version ASC
            "#,
        )
        .bind(profile_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(parse_checkpoint).collect()
    }
}

fn parse_profile(row: sqlx::sqlite::SqliteRow) -> Result<OperatorModelProfile> {
    Ok(OperatorModelProfile {
        profile_id: row.get("profile_id"),
        scope: parse_scope(row.get("scope"))?,
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

fn parse_scope(value: String) -> Result<OperatorModelScope> {
    match value.as_str() {
        "project" => Ok(OperatorModelScope::Project),
        "global" => Ok(OperatorModelScope::Global),
        other => Err(anyhow!("unknown operator-model scope: {other}")),
    }
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

fn path_to_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(|value| value.to_string())
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))
}
