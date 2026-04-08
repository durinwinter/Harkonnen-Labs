use anyhow::Result;
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};

use crate::config::Paths;

pub async fn init_db(paths: &Paths) -> Result<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(&paths.db_file)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS runs (
            run_id TEXT PRIMARY KEY,
            spec_id TEXT NOT NULL,
            product TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS run_events (
            event_id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            episode_id TEXT,
            agent TEXT NOT NULL,
            status TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "run_events",
        "episode_id",
        "ALTER TABLE run_events ADD COLUMN episode_id TEXT",
    )
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS episodes (
            episode_id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL,
            phase TEXT NOT NULL,
            goal TEXT NOT NULL,
            outcome TEXT,
            confidence REAL,
            started_at TEXT NOT NULL,
            ended_at TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS phase_attributions (
            attribution_id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL,
            episode_id TEXT NOT NULL UNIQUE,
            phase TEXT NOT NULL,
            agent_name TEXT NOT NULL,
            outcome TEXT NOT NULL,
            confidence REAL,
            prompt_bundle_fingerprint TEXT,
            prompt_bundle_provider TEXT,
            prompt_bundle_artifact TEXT,
            pinned_skill_ids TEXT NOT NULL DEFAULT '[]',
            memory_hits TEXT NOT NULL DEFAULT '[]',
            core_memory_ids TEXT NOT NULL DEFAULT '[]',
            project_memory_ids TEXT NOT NULL DEFAULT '[]',
            relevant_lesson_ids TEXT NOT NULL DEFAULT '[]',
            required_checks TEXT NOT NULL DEFAULT '[]',
            guardrails TEXT NOT NULL DEFAULT '[]',
            query_terms TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL,
            FOREIGN KEY (episode_id) REFERENCES episodes(episode_id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_phase_attributions_run_id_phase
        ON phase_attributions (run_id, phase)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS run_checkpoints (
            checkpoint_id TEXT PRIMARY KEY,
            run_id TEXT NOT NULL,
            phase TEXT,
            agent TEXT,
            checkpoint_type TEXT NOT NULL,
            status TEXT NOT NULL,
            prompt TEXT NOT NULL,
            context_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            resolved_at TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_run_checkpoints_run_status
        ON run_checkpoints (run_id, status, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS checkpoint_answers (
            answer_id TEXT PRIMARY KEY,
            checkpoint_id TEXT NOT NULL,
            answered_by TEXT NOT NULL,
            answer_text TEXT NOT NULL,
            decision_json TEXT NOT NULL DEFAULT '{}',
            created_at TEXT NOT NULL,
            FOREIGN KEY (checkpoint_id) REFERENCES run_checkpoints(checkpoint_id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_checkpoint_answers_checkpoint
        ON checkpoint_answers (checkpoint_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS causal_links (
            link_id TEXT PRIMARY KEY,
            from_event INTEGER NOT NULL REFERENCES run_events(event_id),
            to_event INTEGER NOT NULL REFERENCES run_events(event_id),
            link_type TEXT NOT NULL,
            confidence REAL NOT NULL DEFAULT 0.5,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS lessons (
            lesson_id TEXT PRIMARY KEY,
            source_episode TEXT REFERENCES episodes(episode_id),
            pattern TEXT NOT NULL,
            intervention TEXT,
            tags TEXT NOT NULL,
            strength REAL NOT NULL DEFAULT 1.0,
            recall_count INTEGER NOT NULL DEFAULT 0,
            last_recalled TEXT,
            created_at TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_run_events_run_id_event_id
        ON run_events (run_id, event_id)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_causal_links_from
        ON causal_links (from_event)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_causal_links_to
        ON causal_links (to_event)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_lessons_tags
        ON lessons (tags)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── Coobie causal reasoning tables ────────────────────────────────────────

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS coobie_episode_scores (
            run_id                  TEXT PRIMARY KEY,
            spec_clarity_score      REAL NOT NULL DEFAULT 0.5,
            change_scope_score      REAL NOT NULL DEFAULT 0.5,
            twin_fidelity_score     REAL NOT NULL DEFAULT 0.5,
            test_coverage_score     REAL NOT NULL DEFAULT 0.0,
            memory_retrieval_score  REAL NOT NULL DEFAULT 0.0,
            phase_success_score     REAL NOT NULL DEFAULT 1.0,
            scenario_passed         INTEGER NOT NULL DEFAULT 0,
            validation_passed       INTEGER NOT NULL DEFAULT 0,
            human_accepted          INTEGER,
            scored_at               TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "coobie_episode_scores",
        "phase_success_score",
        "ALTER TABLE coobie_episode_scores ADD COLUMN phase_success_score REAL NOT NULL DEFAULT 1.0",
    )
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS causal_hypotheses (
            hypothesis_id   TEXT PRIMARY KEY,
            run_id          TEXT NOT NULL,
            cause_id        TEXT NOT NULL,
            description     TEXT NOT NULL,
            confidence      REAL NOT NULL DEFAULT 0.5,
            supporting_runs TEXT NOT NULL DEFAULT '[]',
            counterfactuals TEXT NOT NULL DEFAULT '[]',
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS interventions (
            intervention_id TEXT PRIMARY KEY,
            run_id          TEXT NOT NULL,
            target          TEXT NOT NULL,
            action          TEXT NOT NULL,
            expected_impact TEXT NOT NULL,
            applied         INTEGER NOT NULL DEFAULT 0,
            actual_outcome  TEXT,
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS heuristics (
            heuristic_id    TEXT PRIMARY KEY,
            cause_pattern   TEXT NOT NULL,
            effect_pattern  TEXT NOT NULL,
            intervention    TEXT NOT NULL,
            hit_count       INTEGER NOT NULL DEFAULT 0,
            success_count   INTEGER NOT NULL DEFAULT 0,
            strength        REAL NOT NULL DEFAULT 1.0,
            accepted        INTEGER NOT NULL DEFAULT 1,
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_causal_hypotheses_run_id
        ON causal_hypotheses (run_id)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_coobie_scores_scenario
        ON coobie_episode_scores (scenario_passed, validation_passed)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_embeddings (
            entry_id    TEXT NOT NULL,
            memory_root TEXT NOT NULL,
            backend_id  TEXT NOT NULL DEFAULT '',
            model_id    TEXT NOT NULL DEFAULT '',
            embedding   BLOB NOT NULL,
            embedded_at TEXT NOT NULL,
            PRIMARY KEY (entry_id, memory_root)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "memory_embeddings",
        "backend_id",
        "ALTER TABLE memory_embeddings ADD COLUMN backend_id TEXT NOT NULL DEFAULT ''",
    )
    .await?;

    ensure_column(
        &pool,
        "memory_embeddings",
        "model_id",
        "ALTER TABLE memory_embeddings ADD COLUMN model_id TEXT NOT NULL DEFAULT ''",
    )
    .await?;

    // ── PackChat tables ───────────────────────────────────────────────────────

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_threads (
            thread_id   TEXT PRIMARY KEY,
            run_id      TEXT,
            spec_id     TEXT,
            title       TEXT NOT NULL DEFAULT '',
            status      TEXT NOT NULL DEFAULT 'open',
            created_at  TEXT NOT NULL,
            updated_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_threads_run_id
        ON chat_threads (run_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS chat_messages (
            message_id      TEXT PRIMARY KEY,
            thread_id       TEXT NOT NULL REFERENCES chat_threads(thread_id),
            role            TEXT NOT NULL,  -- 'operator' | 'agent' | 'system'
            agent           TEXT,           -- which agent sent/received this
            content         TEXT NOT NULL,
            checkpoint_id   TEXT,           -- non-null when this msg resolves a checkpoint
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_messages_thread_id
        ON chat_messages (thread_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

async fn ensure_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let rows = sqlx::query(&pragma).fetch_all(pool).await?;
    let exists = rows
        .iter()
        .any(|row| row.get::<String, _>("name") == column);
    if !exists {
        sqlx::query(alter_sql).execute(pool).await?;
    }
    Ok(())
}
