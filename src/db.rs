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
            ended_at TEXT,
            state_before TEXT,
            state_after TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "episodes",
        "state_before",
        "ALTER TABLE episodes ADD COLUMN state_before TEXT",
    )
    .await?;

    ensure_column(
        &pool,
        "episodes",
        "state_after",
        "ALTER TABLE episodes ADD COLUMN state_after TEXT",
    )
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
            briefing_scope TEXT,
            briefing_token_budget INTEGER NOT NULL DEFAULT 0,
            briefing_tokens_used INTEGER NOT NULL DEFAULT 0,
            briefing_hits_provided INTEGER NOT NULL DEFAULT 0,
            stakeholder_alignment_json TEXT NOT NULL DEFAULT 'null',
            created_at TEXT NOT NULL,
            FOREIGN KEY (episode_id) REFERENCES episodes(episode_id)
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "phase_attributions",
        "briefing_scope",
        "ALTER TABLE phase_attributions ADD COLUMN briefing_scope TEXT",
    )
    .await?;

    ensure_column(
        &pool,
        "phase_attributions",
        "briefing_token_budget",
        "ALTER TABLE phase_attributions ADD COLUMN briefing_token_budget INTEGER NOT NULL DEFAULT 0",
    )
    .await?;

    ensure_column(
        &pool,
        "phase_attributions",
        "briefing_tokens_used",
        "ALTER TABLE phase_attributions ADD COLUMN briefing_tokens_used INTEGER NOT NULL DEFAULT 0",
    )
    .await?;

    ensure_column(
        &pool,
        "phase_attributions",
        "briefing_hits_provided",
        "ALTER TABLE phase_attributions ADD COLUMN briefing_hits_provided INTEGER NOT NULL DEFAULT 0",
    )
    .await?;

    ensure_column(
        &pool,
        "phase_attributions",
        "stakeholder_alignment_json",
        "ALTER TABLE phase_attributions ADD COLUMN stakeholder_alignment_json TEXT NOT NULL DEFAULT 'null'",
    )
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
            hierarchy_level TEXT NOT NULL DEFAULT 'associational',
            supporting_runs TEXT NOT NULL DEFAULT '[]',
            evidence        TEXT NOT NULL DEFAULT '[]',
            counterfactuals TEXT NOT NULL DEFAULT '[]',
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "causal_hypotheses",
        "hierarchy_level",
        "ALTER TABLE causal_hypotheses ADD COLUMN hierarchy_level TEXT NOT NULL DEFAULT 'associational'",
    )
    .await?;

    ensure_column(
        &pool,
        "causal_hypotheses",
        "evidence",
        "ALTER TABLE causal_hypotheses ADD COLUMN evidence TEXT NOT NULL DEFAULT '[]'",
    )
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
            thread_id      TEXT PRIMARY KEY,
            run_id         TEXT,
            spec_id        TEXT,
            title          TEXT NOT NULL DEFAULT '',
            status         TEXT NOT NULL DEFAULT 'open',
            thread_kind    TEXT NOT NULL DEFAULT 'general',
            metadata_json  TEXT NOT NULL DEFAULT '{}',
            created_at     TEXT NOT NULL,
            updated_at     TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "chat_threads",
        "thread_kind",
        "ALTER TABLE chat_threads ADD COLUMN thread_kind TEXT NOT NULL DEFAULT 'general'",
    )
    .await?;

    ensure_column(
        &pool,
        "chat_threads",
        "metadata_json",
        "ALTER TABLE chat_threads ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}'",
    )
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
            agent_runtime_id TEXT,          -- stable dog instance identifier when present
            content         TEXT NOT NULL,
            checkpoint_id   TEXT,           -- non-null when this msg resolves a checkpoint
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "chat_messages",
        "agent_runtime_id",
        "ALTER TABLE chat_messages ADD COLUMN agent_runtime_id TEXT",
    )
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_chat_messages_thread_id
        ON chat_messages (thread_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_candidates (
            candidate_id      TEXT PRIMARY KEY,
            source_event_id   TEXT NOT NULL UNIQUE,
            thread_id         TEXT,
            run_id            TEXT,
            spec_id           TEXT,
            message_id        TEXT,
            agent_runtime_id  TEXT,
            agent             TEXT,
            role              TEXT NOT NULL DEFAULT 'system',
            operation         TEXT NOT NULL,
            raw_payload       TEXT NOT NULL DEFAULT '{}',
            distilled_content TEXT,
            dedupe_key        TEXT,
            importance_score  REAL NOT NULL DEFAULT 0.0,
            retention_class   TEXT NOT NULL DEFAULT 'working',
            sensitivity_label TEXT NOT NULL DEFAULT 'normal',
            evidence_refs     TEXT NOT NULL DEFAULT '[]',
            causality_json    TEXT NOT NULL DEFAULT '{}',
            status            TEXT NOT NULL DEFAULT 'pending',
            openbrain_ref     TEXT,
            calvin_contract_json TEXT,
            created_at        TEXT NOT NULL,
            processed_at      TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "memory_candidates",
        "dedupe_key",
        "ALTER TABLE memory_candidates ADD COLUMN dedupe_key TEXT",
    )
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_run_status
        ON memory_candidates (run_id, status, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_thread_status
        ON memory_candidates (thread_id, status, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_candidates_dedupe_status
        ON memory_candidates (dedupe_key, status)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── Operator Model Activation tables ──────────────────────────────────────

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS operator_model_profiles (
            profile_id       TEXT PRIMARY KEY,
            scope            TEXT NOT NULL,
            project_root     TEXT,
            display_name     TEXT NOT NULL DEFAULT '',
            status           TEXT NOT NULL DEFAULT 'active',
            current_version  INTEGER NOT NULL DEFAULT 0,
            created_at       TEXT NOT NULL,
            updated_at       TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_operator_model_profiles_scope_project
        ON operator_model_profiles (scope, project_root)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS operator_model_sessions (
            session_id      TEXT PRIMARY KEY,
            profile_id      TEXT NOT NULL REFERENCES operator_model_profiles(profile_id),
            thread_id       TEXT,
            status          TEXT NOT NULL,
            pending_layer   TEXT,
            started_by      TEXT,
            created_at      TEXT NOT NULL,
            updated_at      TEXT NOT NULL,
            completed_at    TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_operator_model_sessions_profile_status
        ON operator_model_sessions (profile_id, status, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS operator_model_layer_checkpoints (
            checkpoint_id    TEXT PRIMARY KEY,
            session_id       TEXT NOT NULL REFERENCES operator_model_sessions(session_id),
            profile_id       TEXT NOT NULL REFERENCES operator_model_profiles(profile_id),
            version          INTEGER NOT NULL,
            layer            TEXT NOT NULL,
            status           TEXT NOT NULL,
            summary_md       TEXT NOT NULL,
            raw_notes_json   TEXT NOT NULL DEFAULT '{}',
            approved_by      TEXT,
            created_at       TEXT NOT NULL,
            approved_at      TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_operator_model_checkpoints_profile_layer
        ON operator_model_layer_checkpoints (profile_id, version, layer, status)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS operator_model_entries (
            entry_id              TEXT PRIMARY KEY,
            profile_id            TEXT NOT NULL REFERENCES operator_model_profiles(profile_id),
            version               INTEGER NOT NULL,
            layer                 TEXT NOT NULL,
            entry_type            TEXT NOT NULL,
            title                 TEXT NOT NULL,
            content               TEXT NOT NULL,
            details_json          TEXT NOT NULL DEFAULT '{}',
            source_checkpoint_id  TEXT NOT NULL REFERENCES operator_model_layer_checkpoints(checkpoint_id),
            status                TEXT NOT NULL DEFAULT 'current',
            superseded_by         TEXT,
            created_at            TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_operator_model_entries_profile_layer
        ON operator_model_entries (profile_id, version, layer, status)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS operator_model_exports (
            export_id       TEXT PRIMARY KEY,
            profile_id      TEXT NOT NULL REFERENCES operator_model_profiles(profile_id),
            version         INTEGER NOT NULL,
            artifact_name   TEXT NOT NULL,
            content         TEXT NOT NULL,
            content_type    TEXT NOT NULL,
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_operator_model_exports_profile_version
        ON operator_model_exports (profile_id, version, artifact_name)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS operator_model_update_candidates (
            candidate_id    TEXT PRIMARY KEY,
            profile_id      TEXT NOT NULL REFERENCES operator_model_profiles(profile_id),
            run_id          TEXT,
            entry_id        TEXT,
            proposal_kind   TEXT NOT NULL,
            summary         TEXT NOT NULL,
            proposal_json   TEXT NOT NULL DEFAULT '{}',
            status          TEXT NOT NULL DEFAULT 'open',
            confidence      REAL NOT NULL DEFAULT 0.0,
            created_at      TEXT NOT NULL,
            reviewed_at     TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_operator_model_update_candidates_profile_status
        ON operator_model_update_candidates (profile_id, status, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── Phase 5 — Consolidation Workbench ─────────────────────────────────────
    // Candidates are generated by Coobie at the end of a run and surface what
    // it *proposes* to promote.  The operator reviews and keeps/discards/edits
    // each one before the final consolidate action writes anything durable.

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS consolidation_candidates (
            candidate_id    TEXT PRIMARY KEY,
            run_id          TEXT NOT NULL,
            kind            TEXT NOT NULL,   -- 'lesson' | 'causal_link' | 'pattern'
            status          TEXT NOT NULL DEFAULT 'pending',  -- 'pending' | 'kept' | 'discarded'
            content_json    TEXT NOT NULL DEFAULT '{}',
            edited_json     TEXT,            -- operator-edited content, NULL = use content_json
            confidence      REAL NOT NULL DEFAULT 0.5,
            label           TEXT NOT NULL DEFAULT '',
            created_at      TEXT NOT NULL,
            reviewed_at     TEXT
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_consolidation_candidates_run_status
        ON consolidation_candidates (run_id, status, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── A1: LLM cost events ───────────────────────────────────────────────────
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS run_cost_events (
            event_id    TEXT PRIMARY KEY,
            run_id      TEXT NOT NULL,
            agent       TEXT NOT NULL DEFAULT '',
            phase       TEXT NOT NULL DEFAULT '',
            provider    TEXT NOT NULL DEFAULT '',
            model       TEXT NOT NULL DEFAULT '',
            input_tokens  INTEGER NOT NULL DEFAULT 0,
            output_tokens INTEGER NOT NULL DEFAULT 0,
            latency_ms    INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_run_cost_events_run_id
        ON run_cost_events (run_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── A2: Decision log ──────────────────────────────────────────────────────
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS decision_log (
            decision_id     TEXT PRIMARY KEY,
            run_id          TEXT NOT NULL,
            agent           TEXT NOT NULL DEFAULT '',
            phase           TEXT NOT NULL DEFAULT '',
            decision_kind   TEXT NOT NULL DEFAULT '',
            chose           TEXT NOT NULL DEFAULT '',
            alternatives_json TEXT NOT NULL DEFAULT '[]',
            justification   TEXT NOT NULL DEFAULT '',
            approved_by     TEXT,
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_decision_log_run_id
        ON decision_log (run_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── A3: Coordination registry / ActionLease mirror ───────────────────────
    // The JSON assignments file remains the hot-path interchange surface for
    // now, but the active leases and Keeper policy events are mirrored into
    // SQLite so coordination state survives restarts and can be queried
    // alongside the rest of the run metadata.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS coordination_leases (
            agent             TEXT PRIMARY KEY,
            task              TEXT NOT NULL DEFAULT '',
            files_json        TEXT NOT NULL DEFAULT '[]',
            claimed_at        TEXT NOT NULL,
            last_heartbeat_at TEXT NOT NULL DEFAULT '',
            status            TEXT NOT NULL DEFAULT 'active',
            resource_kind     TEXT NOT NULL DEFAULT 'file',
            ttl_secs          INTEGER NOT NULL DEFAULT 0,
            guardrails_json   TEXT NOT NULL DEFAULT '[]',
            expires_at        TEXT NOT NULL DEFAULT '',
            updated_at        TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_coordination_leases_status_updated
        ON coordination_leases (status, updated_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS coordination_policy_events_db (
            event_id            TEXT PRIMARY KEY,
            managed_by          TEXT NOT NULL DEFAULT 'keeper',
            event_type          TEXT NOT NULL DEFAULT '',
            status              TEXT NOT NULL DEFAULT '',
            agent               TEXT,
            conflicting_agent   TEXT,
            files_json          TEXT NOT NULL DEFAULT '[]',
            message             TEXT NOT NULL DEFAULT '',
            created_at          TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_coordination_policy_events_created_at
        ON coordination_policy_events_db (created_at DESC)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_runtime_state (
            runtime_id         TEXT PRIMARY KEY,
            run_id             TEXT NOT NULL,
            thread_id          TEXT,
            canonical_role     TEXT NOT NULL DEFAULT '',
            display_name       TEXT NOT NULL DEFAULT '',
            ownership          TEXT NOT NULL DEFAULT '',
            status             TEXT NOT NULL DEFAULT 'active',
            provider           TEXT,
            surface            TEXT,
            source             TEXT NOT NULL DEFAULT '',
            last_heartbeat_at  TEXT NOT NULL DEFAULT '',
            started_at         TEXT NOT NULL,
            updated_at         TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_state_run
        ON agent_runtime_state (run_id, canonical_role, updated_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_runtime_state_thread
        ON agent_runtime_state (thread_id, updated_at)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── v1-B: Memory supersession / invalidation persistence ─────────────────
    // Tracks when a new memory entry supersedes an older one. The old entry's
    // provenance.superseded_by field points at new_memory_id via this table.
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS memory_updates (
            update_id       TEXT PRIMARY KEY,
            old_memory_id   TEXT NOT NULL,
            new_memory_id   TEXT NOT NULL,
            memory_root     TEXT NOT NULL DEFAULT '',
            reason          TEXT NOT NULL DEFAULT '',
            review_status   TEXT NOT NULL DEFAULT 'pending',
            reviewed_by     TEXT,
            review_note     TEXT,
            reviewed_at     TEXT,
            created_at      TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    ensure_column(
        &pool,
        "memory_updates",
        "memory_root",
        "ALTER TABLE memory_updates ADD COLUMN memory_root TEXT NOT NULL DEFAULT ''",
    )
    .await?;

    ensure_column(
        &pool,
        "memory_updates",
        "review_status",
        "ALTER TABLE memory_updates ADD COLUMN review_status TEXT NOT NULL DEFAULT 'pending'",
    )
    .await?;

    ensure_column(
        &pool,
        "memory_updates",
        "reviewed_by",
        "ALTER TABLE memory_updates ADD COLUMN reviewed_by TEXT",
    )
    .await?;

    ensure_column(
        &pool,
        "memory_updates",
        "review_note",
        "ALTER TABLE memory_updates ADD COLUMN review_note TEXT",
    )
    .await?;

    ensure_column(
        &pool,
        "memory_updates",
        "reviewed_at",
        "ALTER TABLE memory_updates ADD COLUMN reviewed_at TEXT",
    )
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_updates_old_memory_id
        ON memory_updates (old_memory_id, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_memory_updates_created_at
        ON memory_updates (created_at DESC)
        "#,
    )
    .execute(&pool)
    .await?;

    // ── Phase B: Agent Trace Spine ────────────────────────────────────────────
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS agent_traces (
            trace_id          TEXT PRIMARY KEY,
            run_id            TEXT NOT NULL,
            agent             TEXT NOT NULL DEFAULT '',
            phase             TEXT NOT NULL DEFAULT '',
            input_summary     TEXT NOT NULL DEFAULT '',
            reasoning_steps   TEXT NOT NULL DEFAULT '[]',
            actions_taken     TEXT NOT NULL DEFAULT '[]',
            outcome           TEXT NOT NULL DEFAULT '',
            input_tokens      INTEGER NOT NULL DEFAULT 0,
            output_tokens     INTEGER NOT NULL DEFAULT 0,
            latency_ms        INTEGER NOT NULL DEFAULT 0,
            created_at        TEXT NOT NULL
        )
        "#,
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_agent_traces_run_agent
        ON agent_traces (run_id, agent, created_at)
        "#,
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

pub async fn sync_coordination_leases(
    pool: &SqlitePool,
    state: &crate::api::AssignmentsState,
) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM coordination_leases")
        .execute(&mut *tx)
        .await?;

    for assignment in state.active.values() {
        sqlx::query(
            r#"
            INSERT INTO coordination_leases (
                agent,
                task,
                files_json,
                claimed_at,
                last_heartbeat_at,
                status,
                resource_kind,
                ttl_secs,
                guardrails_json,
                expires_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
        )
        .bind(&assignment.agent)
        .bind(&assignment.task)
        .bind(serde_json::to_string(&assignment.files)?)
        .bind(&assignment.claimed_at)
        .bind(&assignment.last_heartbeat_at)
        .bind(&assignment.status)
        .bind(&assignment.resource_kind)
        .bind(assignment.ttl_secs)
        .bind(serde_json::to_string(&assignment.guardrails)?)
        .bind(&assignment.expires_at)
        .bind(&state.updated_at)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn sync_agent_runtime_state(
    pool: &SqlitePool,
    run_id: &str,
    runtimes: &[crate::models::AgentRuntimeState],
) -> Result<()> {
    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM agent_runtime_state WHERE run_id = ?1")
        .bind(run_id)
        .execute(&mut *tx)
        .await?;

    for runtime in runtimes {
        let started_at = if runtime.last_heartbeat_at.trim().is_empty() {
            chrono::Utc::now().to_rfc3339()
        } else {
            runtime.last_heartbeat_at.clone()
        };
        sqlx::query(
            r#"
            INSERT INTO agent_runtime_state (
                runtime_id,
                run_id,
                thread_id,
                canonical_role,
                display_name,
                ownership,
                status,
                provider,
                surface,
                source,
                last_heartbeat_at,
                started_at,
                updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            "#,
        )
        .bind(&runtime.runtime_id)
        .bind(run_id)
        .bind(&runtime.thread_id)
        .bind(&runtime.canonical_role)
        .bind(&runtime.display_name)
        .bind(&runtime.ownership)
        .bind(&runtime.status)
        .bind(&runtime.provider)
        .bind(&runtime.surface)
        .bind(&runtime.source)
        .bind(&runtime.last_heartbeat_at)
        .bind(&started_at)
        .bind(&runtime.last_heartbeat_at)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(())
}

pub async fn insert_coordination_policy_event(
    pool: &SqlitePool,
    event: &crate::api::CoordinationPolicyEvent,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT OR REPLACE INTO coordination_policy_events_db (
            event_id,
            managed_by,
            event_type,
            status,
            agent,
            conflicting_agent,
            files_json,
            message,
            created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
    )
    .bind(&event.event_id)
    .bind(&event.managed_by)
    .bind(&event.event_type)
    .bind(&event.status)
    .bind(&event.agent)
    .bind(&event.conflicting_agent)
    .bind(serde_json::to_string(&event.files)?)
    .bind(&event.message)
    .bind(&event.created_at)
    .execute(pool)
    .await?;
    Ok(())
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
