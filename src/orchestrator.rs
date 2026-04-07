use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    agents::{self, AgentProfile},
    config::Paths,
    coobie::CoobieReasoner,
    db,
    llm::{self, LlmRequest, Message},
    memory::{MemoryEntry, MemoryIngestOptions, MemoryIngestResult, MemoryProvenance, MemoryStore},
    models::{
        AgentExecution, BlackboardState, CheckpointAnswerRecord, CoobieBriefing,
        CoobieEvidenceCitation, EpisodeRecord, EvidenceAnnotation, EvidenceAnnotationBundle,
        EvidenceAnnotationHistoryEvent, EvidenceMatchAssessment, EvidenceMatchReport,
        EvidenceSource, EvidenceTimeRange, EvidenceWindowMatch, HiddenScenarioCheckResult,
        HiddenScenarioEvaluation, HiddenScenarioSummary, IntentPackage, LessonRecord,
        PhaseAttributionRecord, PriorCauseSignal, ProjectResumeRisk, RunCheckpointRecord, RunEvent,
        RunRecord, ScenarioResult, Spec, TwinEnvironment, TwinService, ValidationSummary,
        WorkerHarnessConfig,
    },
    pidgin, policy, scenarios,
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
    pub run_hidden_scenarios: bool,
    pub failure_harness: Option<FailureHarness>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetGitMetadata {
    branch: Option<String>,
    commit: Option<String>,
    remote_origin: Option<String>,
    clean: Option<bool>,
    #[serde(default)]
    changed_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TargetSourceMetadata {
    label: String,
    source_kind: String,
    source_path: String,
    git: Option<TargetGitMetadata>,
}

#[derive(Debug, Clone)]
struct CheckpointDraft {
    checkpoint_id: String,
    phase: Option<String>,
    agent: Option<String>,
    checkpoint_type: String,
    prompt: String,
    context_json: serde_json::Value,
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
    memory_context: MemoryContextBundle,
    briefing: CoobieBriefing,
}

#[derive(Debug, Clone)]
struct CommandOutcome {
    success: bool,
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, Default)]
struct MemoryContextBundle {
    memory_hits: Vec<String>,
    core_memory_hits: Vec<String>,
    project_memory_hits: Vec<String>,
    project_memory_root: Option<String>,
    core_memory_ids: Vec<String>,
    project_memory_ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct CollectedMemoryHits {
    hits: Vec<String>,
    ids: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct EvidencePromotionResult {
    pub promoted_ids: Vec<String>,
    pub skipped_annotations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExplorationEntry {
    phase: String,
    episode_id: String,
    agent: String,
    strategy: String,
    outcome: String,
    failure_constraint: String,
    surviving_structure: String,
    reformulation: String,
    artifacts: Vec<String>,
    parameters: Vec<String>,
    open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExplorationLogArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    entries: Vec<ExplorationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeadEndRegistryEntry {
    registry_id: String,
    run_id: String,
    spec_id: String,
    product: String,
    phase: String,
    agent: String,
    strategy: String,
    failure_constraint: String,
    surviving_structure: String,
    reformulation: String,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeadEndRegistry {
    entries: Vec<DeadEndRegistryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectScanManifest {
    generated_at: String,
    label: String,
    source_kind: String,
    source_path: String,
    project_memory_root: String,
    git: Option<TargetGitMetadata>,
    detected_files: Vec<String>,
    detected_directories: Vec<String>,
    likely_commands: Vec<String>,
    runtime_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectResumePacket {
    generated_at: String,
    label: String,
    current_git: Option<TargetGitMetadata>,
    summary: Vec<String>,
    stale_memory: Vec<ProjectResumeRisk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RepoLocalContextEntry {
    label: String,
    path: String,
    category: String,
    scope: String,
    summary: String,
    relevance: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverContextBundleArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    project_root: String,
    context_entries: Vec<RepoLocalContextEntry>,
    skill_entries: Vec<RepoLocalContextEntry>,
    preload_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrailDriftGuardEntry {
    role: String,
    path: String,
    fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrailDriftGuardArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    tracked_entries: Vec<TrailDriftGuardEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrailDriftCheckArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    guard_artifact: String,
    passed: bool,
    summary: String,
    verified_paths: Vec<String>,
    changed_paths: Vec<String>,
    missing_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StaleMemoryMitigationStatusEntry {
    memory_id: String,
    severity: String,
    severity_score: i32,
    mitigation_steps: Vec<String>,
    related_checks: Vec<String>,
    status: String,
    evidence: Vec<String>,
    previous_severity_score: Option<i32>,
    risk_reduced_from_previous: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StaleMemoryMitigationStatusArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    entries: Vec<StaleMemoryMitigationStatusEntry>,
    resolved_since_previous: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StaleMemoryMitigationHistory {
    records: Vec<StaleMemoryMitigationStatusArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkerTaskEnvelope {
    job_id: String,
    spec_id: String,
    product: String,
    adapter: String,
    profile: String,
    target_source: String,
    staged_workspace: String,
    allowed_paths: Vec<String>,
    denied_paths: Vec<String>,
    visible_success_conditions: Vec<String>,
    return_artifacts: Vec<String>,
    max_iterations: u32,
    continuity_file: Option<String>,
    context_bundle_artifact: Option<String>,
    trail_drift_guard_artifact: Option<String>,
    repo_local_context_paths: Vec<String>,
    repo_local_skill_paths: Vec<String>,
    repo_local_context_notes: Vec<String>,
    query_terms: Vec<String>,
    preferred_commands: Vec<String>,
    guardrails: Vec<String>,
    required_checks: Vec<String>,
    llm_edits: bool,
    editable_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanReviewStage {
    stage: String,
    owner: String,
    summary: String,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanReviewChainArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    stages: Vec<PlanReviewStage>,
    final_execution_plan: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverDispatchArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    adapter: String,
    profile: String,
    generated_at: String,
    task_packet_artifact: String,
    review_chain_artifact: String,
    context_bundle_artifact: String,
    trail_drift_guard_artifact: String,
    continuity_artifact: String,
    dispatch_summary: String,
    constraints_applied: Vec<String>,
    next_actions: Vec<String>,
    visible_success_conditions: Vec<String>,
    return_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrailStateArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    adapter: String,
    profile: String,
    updated_at: String,
    continuity_file: String,
    active_constraints: Vec<String>,
    next_actions: Vec<String>,
    visible_success_conditions: Vec<String>,
    return_artifacts: Vec<String>,
    last_execution_outcome: Option<String>,
    last_execution_summary: Option<String>,
    last_execution_artifact: Option<String>,
    #[serde(default)]
    executed_commands: Vec<String>,
    #[serde(default)]
    returned_artifacts_snapshot: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverPlannedCommand {
    label: String,
    raw_command: String,
    source: String,
    rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverCommandExecution {
    label: String,
    raw_command: String,
    source: String,
    rationale: String,
    #[serde(default)]
    was_preferred: bool,
    #[serde(default)]
    preference_rank: Option<usize>,
    #[serde(default)]
    preference_outcome: Option<String>,
    passed: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    log_artifact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverExecutionArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    adapter: String,
    profile: String,
    generated_at: String,
    task_packet_artifact: String,
    review_chain_artifact: String,
    dispatch_artifact: String,
    continuity_artifact: String,
    hook_artifact: String,
    passed: bool,
    summary: String,
    #[serde(default)]
    preferred_commands_offered: Vec<String>,
    #[serde(default)]
    preferred_commands_selected: Vec<String>,
    #[serde(default)]
    preferred_commands_helped: Vec<String>,
    #[serde(default)]
    preferred_commands_stale: Vec<String>,
    executed_commands: Vec<RetrieverCommandExecution>,
    returned_artifacts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MasonContextFile {
    path: String,
    content: String,
    truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MasonEdit {
    path: String,
    action: String,
    summary: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MasonEditProposal {
    summary: String,
    #[serde(default)]
    rationale: Vec<String>,
    #[serde(default)]
    edits: Vec<MasonEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MasonEditProposalArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    editable_paths: Vec<String>,
    context_paths: Vec<String>,
    summary: String,
    rationale: Vec<String>,
    edits: Vec<MasonEdit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MasonEditApplicationArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    generated_at: String,
    status: String,
    summary: String,
    proposal_generated: bool,
    changed_files: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ResolvedPinnedSkillExcerpt {
    id: String,
    source: String,
    provider_family: String,
    vendor_path: String,
    rationale: String,
    excerpt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AgentPromptBundleArtifact {
    agent_name: String,
    display_name: String,
    role: String,
    resolved_provider: String,
    resolved_model: Option<String>,
    resolved_surface: Option<String>,
    fingerprint: String,
    shared_personality: String,
    personality_addendum: Option<String>,
    curated_skill_bundle: String,
    pinned_skill_ids: Vec<String>,
    pinned_external_skills: Vec<ResolvedPinnedSkillExcerpt>,
    repo_local_context_entries: Vec<RepoLocalContextEntry>,
    repo_local_skill_entries: Vec<RepoLocalContextEntry>,
    system_instruction: String,
    repo_context_block: String,
}

#[derive(Debug, Clone)]
struct AgentPromptSupport {
    system_instruction: String,
    repo_context_block: String,
    bundle: AgentPromptBundleArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PinnedSkillManifest {
    #[serde(default)]
    sources: HashMap<String, PinnedSkillSource>,
    #[serde(default)]
    skills: Vec<PinnedSkillEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PinnedSkillSource {
    #[serde(default)]
    repo: String,
    #[serde(default)]
    commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PinnedSkillEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    source: String,
    #[serde(default)]
    vendor_path: String,
    #[serde(default)]
    agents: Vec<String>,
    #[serde(default)]
    rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverHookRecord {
    stage: String,
    decision: String,
    tool: String,
    command_label: String,
    raw_command: String,
    source: String,
    rationale: String,
    reasons: Vec<String>,
    passed: Option<bool>,
    exit_code: Option<i32>,
    log_artifact: Option<String>,
    created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RetrieverHookArtifact {
    run_id: String,
    spec_id: String,
    product: String,
    adapter: String,
    profile: String,
    generated_at: String,
    records: Vec<RetrieverHookRecord>,
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

    pub async fn ingest_memory_source(
        &self,
        source: &str,
        scope: &str,
        project_root: Option<&str>,
        id: Option<&str>,
        summary: Option<&str>,
        notes: Option<&str>,
        tags: Vec<String>,
        keep_asset: bool,
    ) -> Result<MemoryIngestResult> {
        let normalized_scope = scope.trim().to_lowercase();
        match normalized_scope.as_str() {
            "core" => {
                self.memory_store
                    .ingest_source(
                        source,
                        MemoryIngestOptions {
                            id: id.map(|value| value.to_string()),
                            summary: summary.map(|value| value.to_string()),
                            notes: notes.map(|value| value.to_string()),
                            tags,
                            provenance: MemoryProvenance::default(),
                            keep_asset,
                            scope_tag: Some("core-memory".to_string()),
                        },
                    )
                    .await
            }
            "project" => {
                let project_root = project_root
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .context("project memory ingest requires --project-root <repo-path>")?;
                let target_source = self.resolve_memory_ingest_target(project_root).await?;
                let store = self.project_memory_store(&target_source).await?;
                let result = store
                    .ingest_source(
                        source,
                        MemoryIngestOptions {
                            id: id.map(|value| value.to_string()),
                            summary: summary.map(|value| value.to_string()),
                            notes: notes.map(|value| value.to_string()),
                            tags,
                            provenance: project_memory_provenance(
                                &target_source,
                                None,
                                None,
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                Vec::new(),
                                vec!["project-memory-ingest".to_string()],
                            ),
                            keep_asset,
                            scope_tag: Some("project-memory".to_string()),
                        },
                    )
                    .await?;
                self.refresh_project_resume_packet(&target_source, &store)
                    .await?;
                Ok(result)
            }
            _ => bail!("unsupported memory ingest scope: {scope}"),
        }
    }

    pub async fn init_project_evidence(&self, project_root: &Path) -> Result<PathBuf> {
        let harkonnen_dir = self.repo_harkonnen_dir(project_root);
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        Ok(harkonnen_dir.join("evidence"))
    }

    pub async fn list_project_evidence_bundles(&self, project_root: &str) -> Result<Vec<String>> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let harkonnen_dir = self.project_harkonnen_dir(&target_source);
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        let annotations_dir = harkonnen_dir.join("evidence").join("annotations");
        let mut bundles = Vec::new();
        let mut reader = tokio::fs::read_dir(&annotations_dir).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_default();
            if ext != "yaml" && ext != "yml" && ext != "json" {
                continue;
            }
            if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
                bundles.push(name.to_string());
            }
        }
        bundles.sort();
        Ok(bundles)
    }

    pub async fn load_project_evidence_bundle(
        &self,
        project_root: &str,
        bundle_name: &str,
    ) -> Result<Option<EvidenceAnnotationBundle>> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let path = self
            .project_evidence_bundle_path(&target_source, bundle_name)
            .await?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(Some(parse_evidence_bundle_text(&raw)?))
    }

    pub async fn load_project_evidence_history(
        &self,
        project_root: &str,
        bundle_name: &str,
        annotation_id: Option<&str>,
    ) -> Result<Vec<EvidenceAnnotationHistoryEvent>> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let path = self
            .project_evidence_history_path(&target_source, bundle_name)
            .await?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let mut events = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<EvidenceAnnotationHistoryEvent>(trimmed) else {
                continue;
            };
            if let Some(annotation_id) = annotation_id.filter(|value| !value.trim().is_empty()) {
                if event.annotation_id != annotation_id {
                    continue;
                }
            }
            events.push(event);
        }
        events.sort_by(|left, right| {
            left.occurred_at
                .cmp(&right.occurred_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        Ok(events)
    }

    pub async fn save_project_evidence_bundle(
        &self,
        project_root: &str,
        bundle_name: &str,
        bundle: &EvidenceAnnotationBundle,
    ) -> Result<PathBuf> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let path = self
            .project_evidence_bundle_path(&target_source, bundle_name)
            .await?;
        let previous = if path.exists() {
            let raw = tokio::fs::read_to_string(&path).await?;
            Some(parse_evidence_bundle_text(&raw)?)
        } else {
            None
        };
        let mut normalized = bundle.clone();
        if normalized.project.trim().is_empty() {
            normalized.project = target_source.label.clone();
        }
        validate_evidence_bundle(&normalized)?;
        let raw = serde_yaml::to_string(&normalized)?;
        tokio::fs::write(&path, raw).await?;
        let history_events =
            collect_bundle_save_history_events(bundle_name, previous.as_ref(), &normalized);
        self.append_project_evidence_history_events(&target_source, bundle_name, &history_events)
            .await?;
        Ok(path)
    }

    pub async fn upsert_project_evidence_annotation(
        &self,
        project_root: &str,
        bundle_name: &str,
        scenario: Option<&str>,
        dataset: Option<&str>,
        notes: &[String],
        sources: &[EvidenceSource],
        annotation: &EvidenceAnnotation,
    ) -> Result<(PathBuf, EvidenceAnnotationBundle)> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let path = self
            .project_evidence_bundle_path(&target_source, bundle_name)
            .await?;
        let mut bundle = if path.exists() {
            let raw = tokio::fs::read_to_string(&path).await?;
            parse_evidence_bundle_text(&raw)?
        } else {
            EvidenceAnnotationBundle {
                schema_version: 1,
                project: target_source.label.clone(),
                scenario: scenario.unwrap_or_default().to_string(),
                dataset: dataset.unwrap_or_default().to_string(),
                notes: Vec::new(),
                sources: Vec::new(),
                annotations: Vec::new(),
            }
        };
        if bundle.project.trim().is_empty() {
            bundle.project = target_source.label.clone();
        }
        if let Some(scenario) = scenario.filter(|value| !value.trim().is_empty()) {
            bundle.scenario = scenario.trim().to_string();
        }
        if let Some(dataset) = dataset.filter(|value| !value.trim().is_empty()) {
            bundle.dataset = dataset.trim().to_string();
        }
        for note in notes {
            push_unique(&mut bundle.notes, note);
        }
        for source in sources {
            if let Some(existing) = bundle
                .sources
                .iter_mut()
                .find(|candidate| candidate.source_id == source.source_id)
            {
                *existing = source.clone();
            } else {
                bundle.sources.push(source.clone());
            }
        }

        let mut normalized_annotation = annotation.clone();
        if normalized_annotation.annotation_id.trim().is_empty() {
            normalized_annotation.annotation_id = format!("ann_{}", Uuid::new_v4().simple());
        }
        let previous_annotation = bundle
            .annotations
            .iter()
            .find(|candidate| candidate.annotation_id == normalized_annotation.annotation_id)
            .cloned();
        if normalized_annotation.status.trim().is_empty() {
            normalized_annotation.status = "draft".to_string();
        }
        let now = Utc::now().to_rfc3339();
        if normalized_annotation.created_at.trim().is_empty() {
            normalized_annotation.created_at = now.clone();
        }
        normalized_annotation.updated_at = now;

        if let Some(existing) = bundle
            .annotations
            .iter_mut()
            .find(|candidate| candidate.annotation_id == normalized_annotation.annotation_id)
        {
            *existing = normalized_annotation.clone();
        } else {
            bundle.annotations.push(normalized_annotation.clone());
        }

        validate_evidence_bundle(&bundle)?;
        let raw = serde_yaml::to_string(&bundle)?;
        tokio::fs::write(&path, raw).await?;

        let history_event = build_annotation_history_event(
            bundle_name,
            &normalized_annotation,
            if previous_annotation.is_some() {
                "updated"
            } else {
                "created"
            },
            previous_annotation
                .as_ref()
                .map(|value| effective_annotation_status(value)),
            Some(annotation_history_actor(&normalized_annotation)),
            if previous_annotation.is_some() {
                Some("Annotation updated via upsert.".to_string())
            } else {
                Some("Annotation created via upsert.".to_string())
            },
            Vec::new(),
        );
        self.append_project_evidence_history_events(&target_source, bundle_name, &[history_event])
            .await?;
        Ok((path, bundle))
    }

    pub async fn review_project_evidence_annotation(
        &self,
        project_root: &str,
        bundle_name: &str,
        annotation_id: &str,
        status: &str,
        reviewed_by: Option<&str>,
        review_note: Option<&str>,
        promote_scope: Option<&str>,
    ) -> Result<(PathBuf, EvidenceAnnotationBundle, EvidencePromotionResult)> {
        if annotation_id.trim().is_empty() {
            bail!("annotation_id cannot be empty");
        }

        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let path = self
            .project_evidence_bundle_path(&target_source, bundle_name)
            .await?;
        if !path.exists() {
            bail!("evidence bundle '{}' not found", bundle_name);
        }

        let raw = tokio::fs::read_to_string(&path).await?;
        let mut bundle = parse_evidence_bundle_text(&raw)?;
        let normalized_status = normalize_annotation_review_status(status)?;
        let now = Utc::now().to_rfc3339();
        let mut history_events = Vec::new();

        {
            let annotation = bundle
                .annotations
                .iter_mut()
                .find(|candidate| candidate.annotation_id == annotation_id)
                .with_context(|| {
                    format!(
                        "annotation '{}' not found in bundle '{}'",
                        annotation_id, bundle_name
                    )
                })?;

            let previous_status = effective_annotation_status(annotation);
            let actor = reviewed_by
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| annotation_history_actor(annotation));

            annotation.status = normalized_status.to_string();
            annotation.updated_at = now.clone();
            match normalized_status {
                "draft" => {
                    annotation.reviewed_by.clear();
                    annotation.reviewed_at.clear();
                }
                _ => {
                    annotation.reviewed_at = now.clone();
                    if let Some(reviewed_by) = reviewed_by.filter(|value| !value.trim().is_empty())
                    {
                        annotation.reviewed_by = reviewed_by.trim().to_string();
                    }
                }
            }

            if let Some(review_note) = review_note.filter(|value| !value.trim().is_empty()) {
                let reviewer = reviewed_by
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        if annotation.reviewed_by.trim().is_empty() {
                            None
                        } else {
                            Some(annotation.reviewed_by.trim())
                        }
                    });
                let prefix = match reviewer {
                    Some(name) => format!("Review [{} by {}]", normalized_status, name),
                    None => format!("Review [{}]", normalized_status),
                };
                append_annotation_note(
                    &mut annotation.notes,
                    &format!("{}: {}", prefix, review_note.trim()),
                );
            }

            history_events.push(build_annotation_history_event(
                bundle_name,
                annotation,
                "status_changed",
                Some(previous_status),
                Some(actor),
                review_note
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty()),
                Vec::new(),
            ));
        }

        validate_evidence_bundle(&bundle)?;
        let serialized = serde_yaml::to_string(&bundle)?;
        tokio::fs::write(&path, serialized).await?;

        let promotion = if let Some(scope) = promote_scope.filter(|value| !value.trim().is_empty())
        {
            let single_annotation = bundle
                .annotations
                .iter()
                .find(|candidate| candidate.annotation_id == annotation_id)
                .cloned()
                .with_context(|| {
                    format!(
                        "annotation '{}' not found after review update",
                        annotation_id
                    )
                })?;
            let mut single_bundle = bundle.clone();
            single_bundle.annotations = vec![single_annotation.clone()];
            let promotion = self
                .promote_evidence_bundle(&path, &single_bundle, scope, Some(project_root))
                .await?;
            if !promotion.promoted_ids.is_empty() {
                history_events.push(build_annotation_history_event(
                    bundle_name,
                    &single_annotation,
                    "promoted",
                    Some(effective_annotation_status(&single_annotation)),
                    Some(annotation_history_actor(&single_annotation)),
                    Some(format!("Promoted to {} memory.", scope.trim())),
                    promotion.promoted_ids.clone(),
                ));
            }
            promotion
        } else {
            EvidencePromotionResult::default()
        };

        self.append_project_evidence_history_events(&target_source, bundle_name, &history_events)
            .await?;

        Ok((path, bundle, promotion))
    }

    pub async fn promote_evidence_bundle(
        &self,
        bundle_path: &Path,
        bundle: &EvidenceAnnotationBundle,
        scope: &str,
        project_root: Option<&str>,
    ) -> Result<EvidencePromotionResult> {
        let normalized_scope = scope.trim().to_lowercase();
        let target_source = if normalized_scope == "project" || normalized_scope == "follow-bundle"
        {
            match project_root {
                Some(root) if !root.trim().is_empty() => {
                    Some(self.resolve_memory_ingest_target(root).await?)
                }
                _ => None,
            }
        } else {
            None
        };

        if normalized_scope == "project" && target_source.is_none() {
            bail!("project evidence promotion requires --project-root <repo-path>");
        }

        let mut result = EvidencePromotionResult::default();
        for annotation in &bundle.annotations {
            let destination =
                match resolve_evidence_promotion_destination(annotation, &normalized_scope) {
                    Some(destination) => destination,
                    None => {
                        result.skipped_annotations.push(format!(
                            "{} (promotion disabled or unsupported scope)",
                            annotation.annotation_id
                        ));
                        continue;
                    }
                };
            if !annotation_is_review_ready(annotation) {
                result.skipped_annotations.push(format!(
                    "{} (status {} is not review-ready)",
                    annotation.annotation_id,
                    if annotation.status.trim().is_empty() {
                        "draft"
                    } else {
                        annotation.status.trim()
                    }
                ));
                continue;
            }

            let memory_id = build_evidence_memory_id(bundle, annotation);
            let summary = build_evidence_memory_summary(bundle, annotation);
            let content = render_evidence_memory_body(bundle_path, bundle, annotation);
            let tags = build_evidence_memory_tags(bundle, annotation, destination);
            let source_uris = collect_evidence_source_uris(bundle, annotation);

            match destination {
                "project" => {
                    let target_source = target_source
                        .as_ref()
                        .context("project evidence promotion requires target source context")?;
                    let provenance = build_evidence_memory_provenance(
                        bundle_path,
                        bundle,
                        annotation,
                        target_source,
                        &source_uris,
                    );
                    self.store_project_memory_entry(
                        target_source,
                        &memory_id,
                        tags,
                        &summary,
                        &content,
                        provenance,
                    )
                    .await?;
                }
                "core" => {
                    let provenance = target_source
                        .as_ref()
                        .map(|target_source| build_evidence_memory_provenance(bundle_path, bundle, annotation, target_source, &source_uris))
                        .unwrap_or_else(|| MemoryProvenance {
                            source_label: Some(bundle.project.clone()),
                            source_kind: Some("causal-evidence-bundle".to_string()),
                            source_path: Some(bundle_path.display().to_string()),
                            observed_paths: source_uris.clone(),
                            observed_surfaces: bundle.sources.iter().map(|source| source.kind.clone()).collect(),
                            stale_when: vec!["evidence sources, dataset semantics, or reviewed causal interpretation change".to_string()],
                            ..MemoryProvenance::default()
                        });
                    self.memory_store
                        .store_with_metadata(&memory_id, tags, &summary, &content, provenance)
                        .await?;
                }
                _ => unreachable!(),
            }
            result.promoted_ids.push(memory_id);
        }

        if let Some(target_source) = target_source.as_ref() {
            let store = self.project_memory_store(target_source).await?;
            self.refresh_project_resume_packet(target_source, &store)
                .await?;
        }

        Ok(result)
    }

    pub async fn search_similar_evidence_windows(
        &self,
        project_root: &str,
        spec_id: Option<&str>,
        query_terms: &[String],
        labels: &[String],
        claims: &[String],
        sources: &[String],
        time_span_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<EvidenceWindowMatch>> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let harkonnen_dir = self.project_harkonnen_dir(&target_source);
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        let annotations_dir = harkonnen_dir.join("evidence").join("annotations");
        if !annotations_dir.exists() {
            return Ok(Vec::new());
        }

        let normalized_labels = labels
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let normalized_claims = claims
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        let normalized_sources = sources
            .iter()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();

        let mut effective_terms = Vec::new();
        for value in query_terms
            .iter()
            .chain(labels.iter())
            .chain(claims.iter())
            .chain(sources.iter())
        {
            push_unique(&mut effective_terms, value);
        }
        if let Some(spec_id) = spec_id.filter(|value| !value.trim().is_empty()) {
            push_unique(&mut effective_terms, spec_id);
        }
        push_unique(&mut effective_terms, &target_source.label);

        let mut scored = Vec::<EvidenceWindowMatch>::new();
        let mut reader = tokio::fs::read_dir(&annotations_dir).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_default();
            if ext != "yaml" && ext != "yml" {
                continue;
            }
            let raw = match tokio::fs::read_to_string(&path).await {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let bundle = match serde_yaml::from_str::<EvidenceAnnotationBundle>(&raw) {
                Ok(bundle) => bundle,
                Err(_) => continue,
            };

            for annotation in &bundle.annotations {
                if !annotation_is_review_ready(annotation) {
                    continue;
                }

                let matched_sources = bundle
                    .sources
                    .iter()
                    .filter(|source| {
                        annotation
                            .source_ids
                            .iter()
                            .any(|id| id == &source.source_id)
                    })
                    .map(|source| format!("{}:{}:{}", source.source_id, source.kind, source.label))
                    .collect::<Vec<_>>();
                let claim_summary = annotation
                    .claims
                    .iter()
                    .map(|claim| format!("{}:{}->{}", claim.relation, claim.cause, claim.effect))
                    .collect::<Vec<_>>();
                let time_summary =
                    render_evidence_time_range_summary(annotation.time_range.as_ref());
                let haystack = format!(
                    "{} {} {} {} {} {} {} {} {} {} {}",
                    bundle.project,
                    bundle.scenario,
                    bundle.dataset,
                    annotation.annotation_id,
                    annotation.annotation_type,
                    annotation.title,
                    annotation.labels.join(" "),
                    annotation.tags.join(" "),
                    annotation.notes,
                    matched_sources.join(" "),
                    claim_summary.join(" "),
                );
                let mut score = score_briefing_evidence(
                    &haystack,
                    spec_id.unwrap_or_default(),
                    &target_source.label,
                    &effective_terms,
                    &[],
                );
                if bundle.project == target_source.label {
                    score += 8;
                }

                let normalized_annotation_terms = annotation
                    .labels
                    .iter()
                    .chain(annotation.tags.iter())
                    .map(|value| value.trim().to_ascii_lowercase())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>();
                let normalized_claim_terms = annotation
                    .claims
                    .iter()
                    .flat_map(|claim| {
                        [
                            claim.relation.clone(),
                            claim.cause.clone(),
                            claim.effect.clone(),
                        ]
                        .into_iter()
                    })
                    .map(|value| value.trim().to_ascii_lowercase())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>();
                let normalized_source_terms = matched_sources
                    .iter()
                    .map(|value| value.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let matched_labels =
                    collect_overlapping_terms(&normalized_labels, &normalized_annotation_terms);
                let matched_claims =
                    collect_overlapping_terms(&normalized_claims, &normalized_claim_terms);
                let matched_source_terms =
                    collect_overlapping_terms(&normalized_sources, &normalized_source_terms);

                score += overlap_bonus(&normalized_labels, &normalized_annotation_terms, 12);
                score += overlap_bonus(&normalized_claims, &normalized_claim_terms, 12);
                score += overlap_bonus(&normalized_sources, &normalized_source_terms, 10);

                let time_span_delta_ms = if let (Some(target_span), Some(candidate_span)) = (
                    time_span_ms,
                    annotation_time_span_ms(annotation.time_range.as_ref()),
                ) {
                    score += time_span_similarity_bonus(target_span, candidate_span);
                    Some((target_span - candidate_span).abs())
                } else {
                    None
                };
                if score <= 0 {
                    continue;
                }

                let title = if annotation.title.trim().is_empty() {
                    annotation.annotation_id.clone()
                } else {
                    annotation.title.trim().to_string()
                };
                scored.push(EvidenceWindowMatch {
                    score,
                    project: bundle.project.clone(),
                    scenario: bundle.scenario.clone(),
                    dataset: bundle.dataset.clone(),
                    bundle_path: path.display().to_string(),
                    annotation_id: annotation.annotation_id.clone(),
                    annotation_type: annotation.annotation_type.clone(),
                    title: title.clone(),
                    time_summary,
                    labels: annotation.labels.clone(),
                    claims: claim_summary.clone(),
                    sources: matched_sources.clone(),
                    matched_labels,
                    matched_claims,
                    matched_sources: matched_source_terms,
                    time_span_delta_ms,
                    citation: CoobieEvidenceCitation {
                        citation_id: format!(
                            "evidence-window:{}:{}",
                            path.display(),
                            annotation.annotation_id
                        ),
                        source_type: "evidence_annotation_window".to_string(),
                        run_id: "annotation".to_string(),
                        episode_id: Some(annotation.annotation_id.clone()),
                        phase: annotation.annotation_type.clone(),
                        agent: "coobie".to_string(),
                        summary: format!(
                            "nearest prior evidence window '{}' from scenario '{}'",
                            title,
                            if bundle.scenario.trim().is_empty() {
                                "unspecified"
                            } else {
                                bundle.scenario.trim()
                            }
                        ),
                        evidence: format!(
                            "bundle={}; dataset={}; time={}; labels={}; claims={}; sources={}",
                            path.display(),
                            if bundle.dataset.trim().is_empty() {
                                "unspecified"
                            } else {
                                bundle.dataset.trim()
                            },
                            render_evidence_time_range_summary(annotation.time_range.as_ref()),
                            if annotation.labels.is_empty() {
                                "none".to_string()
                            } else {
                                annotation.labels.join(" | ")
                            },
                            if claim_summary.is_empty() {
                                "none".to_string()
                            } else {
                                claim_summary.join(" | ")
                            },
                            if matched_sources.is_empty() {
                                "none".to_string()
                            } else {
                                matched_sources.join(" | ")
                            }
                        ),
                    },
                });
            }
        }

        scored.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| right.annotation_id.cmp(&left.annotation_id))
        });
        scored.truncate(limit.max(1).min(20));
        Ok(scored)
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
                if let Err(error) = self
                    .record_memory_context_outcome(
                        &target_source,
                        &output.memory_context,
                        final_status == "completed",
                    )
                    .await
                {
                    let _ = self
                        .record_event(
                            &run_id,
                            None,
                            "memory",
                            "coobie",
                            "warning",
                            &format!("Memory manifest outcome tracking skipped: {error}"),
                            &log_path,
                        )
                        .await;
                }

                let lessons = match self.consolidate_run(&run_id, &spec_obj).await {
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
                if let Err(error) = self
                    .record_stale_memory_mitigation_outcomes(
                        &run_id,
                        &spec_obj,
                        &target_source,
                        &output.briefing,
                        &output.validation,
                        &output.hidden_scenarios,
                        &output.run_dir,
                    )
                    .await
                {
                    let _ = self
                        .record_event(
                            &run_id,
                            None,
                            "memory",
                            "coobie",
                            "warning",
                            &format!("Stale-memory mitigation tracking skipped: {error}"),
                            &log_path,
                        )
                        .await;
                }

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
                self.finalize_blackboard(final_status, &output.run_dir)
                    .await?;
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
                let lessons = match self.consolidate_run(&run_id, &spec_obj).await {
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
                    self.attach_lessons_to_blackboard(&run_dir, &lessons)
                        .await?;
                    if let Ok(Some(briefing)) = self.load_run_briefing(&run_id).await {
                        let fallback_validation = ValidationSummary {
                            passed: false,
                            results: Vec::new(),
                        };
                        let fallback_hidden = HiddenScenarioSummary {
                            passed: false,
                            results: Vec::new(),
                        };
                        if let Err(tracking_error) = self
                            .record_stale_memory_mitigation_outcomes(
                                &run_id,
                                &spec_obj,
                                &target_source,
                                &briefing,
                                &fallback_validation,
                                &fallback_hidden,
                                &run_dir,
                            )
                            .await
                        {
                            let _ = self
                                .record_event(
                                    &run_id,
                                    None,
                                    "memory",
                                    "coobie",
                                    "warning",
                                    &format!("Stale-memory mitigation tracking skipped: {tracking_error}"),
                                    &log_path,
                                )
                                .await;
                        }
                    }
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
        let workspace_root =
            workspace::create_run_workspace(&self.paths.workspaces, run_id).await?;
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
        push_unique(&mut blackboard.artifact_refs, "phase_attributions.json");
        push_unique(&mut blackboard.artifact_refs, "phase_attributions.md");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let mut agent_executions = Vec::new();
        let mut phase_attributions = Vec::new();
        let mut retriever_context_bundle: Option<RetrieverContextBundleArtifact> = None;
        let query_terms = build_coobie_query_terms(spec_obj, target_source);
        let domain_signals = infer_domain_signals(spec_obj, target_source, &query_terms);

        let memory_episode = self
            .start_episode(
                run_id,
                "memory",
                &format!("Coobie preflight for {}", spec_obj.id),
            )
            .await?;
        blackboard.current_phase = "memory".to_string();
        blackboard.active_goal = format!("Coobie preflight for {}", spec_obj.title);
        claim_agent(
            &mut blackboard,
            "coobie",
            "retrieve prior context and emit causal briefing",
        );
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
        let memory_context = self
            .retrieve_coobie_memory_context(target_source, &query_terms)
            .await?;
        let briefing = self
            .build_coobie_briefing(
                spec_obj,
                target_source,
                &query_terms,
                &domain_signals,
                &memory_context,
            )
            .await?;
        let evidence_match_report = self
            .build_evidence_match_report(spec_obj, target_source, &briefing)
            .await?;
        tokio::fs::write(
            run_dir.join("memory_context.md"),
            format_memory_context_bundle(&memory_context),
        )
        .await?;
        self.write_json_file(&run_dir.join("coobie_briefing.json"), &briefing)
            .await?;
        tokio::fs::write(
            run_dir.join("coobie_preflight_response.md"),
            &briefing.coobie_response,
        )
        .await?;
        self.write_json_file(
            &run_dir.join("evidence_match_report.json"),
            &evidence_match_report,
        )
        .await?;
        tokio::fs::write(
            run_dir.join("evidence_match_report.md"),
            render_evidence_match_report(&evidence_match_report),
        )
        .await?;
        push_unique(&mut blackboard.artifact_refs, "memory_context.md");
        push_unique(&mut blackboard.artifact_refs, "coobie_briefing.json");
        push_unique(
            &mut blackboard.artifact_refs,
            "coobie_preflight_response.md",
        );
        push_unique(&mut blackboard.artifact_refs, "evidence_match_report.json");
        push_unique(&mut blackboard.artifact_refs, "evidence_match_report.md");
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
            "memory",
            &memory_episode,
            spec_obj,
            target_source,
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
        self.finish_episode(&memory_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &memory_episode,
            "memory",
            "coobie",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
        self.link_events(
            memory_start.event_id,
            memory_end.event_id,
            "contributed_to",
            1.0,
        )
        .await?;
        release_agent(&mut blackboard, "coobie");
        push_unique(&mut blackboard.resolved_items, "memory");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        let intake_episode = self
            .start_episode(run_id, "intake", &format!("Interpret spec {}", spec_obj.id))
            .await?;
        blackboard.current_phase = "intake".to_string();
        blackboard.active_goal = format!("Interpret spec {}", spec_obj.title);
        claim_agent(
            &mut blackboard,
            "scout",
            "interpret spec and normalize intent with Coobie context",
        );
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
        let intent = self
            .scout_intake(spec_obj, target_source, &briefing)
            .await?;
        self.write_json_file(&run_dir.join("intent.json"), &intent)
            .await?;
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
            "intake",
            &intake_episode,
            spec_obj,
            target_source,
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
        self.finish_episode(&intake_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &intake_episode,
            "intake",
            "scout",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
        self.link_events(
            intake_start.event_id,
            intake_end.event_id,
            "contributed_to",
            1.0,
        )
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
            "workspace",
            &workspace_episode,
            spec_obj,
            target_source,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        if let Some(worker_harness) = &spec_obj.worker_harness {
            let context_bundle = build_retriever_context_bundle(
                run_id,
                spec_obj,
                target_source,
                &staged_product,
                &query_terms,
            )?;
            self.write_json_file(
                &run_dir.join("retriever_context_bundle.json"),
                &context_bundle,
            )
            .await?;
            tokio::fs::write(
                run_dir.join("retriever_context_bundle.md"),
                render_retriever_context_bundle_markdown(&context_bundle),
            )
            .await?;
            push_unique(
                &mut blackboard.artifact_refs,
                "retriever_context_bundle.json",
            );
            push_unique(&mut blackboard.artifact_refs, "retriever_context_bundle.md");
            let drift_guard = build_trail_drift_guard(
                run_id,
                spec_obj,
                target_source,
                &staged_product,
                &context_bundle,
            )?;
            self.write_json_file(&run_dir.join("trail_drift_guard.json"), &drift_guard)
                .await?;
            tokio::fs::write(
                run_dir.join("trail_drift_guard.md"),
                render_trail_drift_guard_markdown(&drift_guard),
            )
            .await?;
            push_unique(&mut blackboard.artifact_refs, "trail_drift_guard.json");
            push_unique(&mut blackboard.artifact_refs, "trail_drift_guard.md");
            let envelope = build_worker_task_envelope(
                run_id,
                spec_obj,
                target_source,
                worker_harness,
                &briefing,
                &self.paths.root,
                &workspace_root,
                &run_dir,
                &staged_product,
                &context_bundle,
            );
            self.write_json_file(&run_dir.join("retriever_task_packet.json"), &envelope)
                .await?;
            tokio::fs::write(
                run_dir.join("retriever_task_packet.md"),
                render_worker_task_envelope_markdown(&envelope),
            )
            .await?;
            push_unique(&mut blackboard.artifact_refs, "retriever_task_packet.json");
            push_unique(&mut blackboard.artifact_refs, "retriever_task_packet.md");
            retriever_context_bundle = Some(context_bundle);
        }
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
        self.finish_episode(&workspace_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &workspace_episode,
            "workspace",
            "keeper",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
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
            .start_episode(
                run_id,
                "implementation",
                &format!("Plan work for {}", target_source.label),
            )
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
        if let Some(worker_harness) = &spec_obj.worker_harness {
            let plan_review_chain = build_plan_review_chain(
                run_id,
                spec_obj,
                target_source,
                &intent,
                &briefing,
                &implementation_plan,
                retriever_context_bundle.as_ref(),
            );
            self.write_json_file(&run_dir.join("trail_review_chain.json"), &plan_review_chain)
                .await?;
            tokio::fs::write(
                run_dir.join("trail_review_chain.md"),
                render_plan_review_chain_markdown(&plan_review_chain),
            )
            .await?;
            push_unique(&mut blackboard.artifact_refs, "trail_review_chain.json");
            push_unique(&mut blackboard.artifact_refs, "trail_review_chain.md");
            let (dispatch, trail_state) = self
                .write_retriever_dispatch_artifacts(
                    run_id,
                    spec_obj,
                    target_source,
                    worker_harness,
                    &run_dir,
                )
                .await?;
            push_unique(&mut blackboard.artifact_refs, "retriever_dispatch.json");
            push_unique(&mut blackboard.artifact_refs, "retriever_dispatch.md");
            push_unique(
                &mut blackboard.artifact_refs,
                &dispatch.context_bundle_artifact,
            );
            push_unique(
                &mut blackboard.artifact_refs,
                &dispatch.trail_drift_guard_artifact,
            );
            push_unique(&mut blackboard.artifact_refs, &dispatch.continuity_artifact);
            self.write_agent_execution(
                &profiles,
                "mason",
                &format!(
                    "Prepare the retriever-forge dispatch contract for target '{}' using the staged workspace.",
                    target_source.label
                ),
                "Prepared the bounded retriever dispatch packet, trail review chain, and continuity state.",
                &format!(
                    "dispatch_summary={}
continuity_file={}
constraints={}
next_actions={}",
                    dispatch.dispatch_summary,
                    trail_state.continuity_file,
                    dispatch.constraints_applied.join(" | "),
                    dispatch.next_actions.join(" | ")
                ),
                "implementation",
                &implementation_episode,
                spec_obj,
                target_source,
                &run_dir,
                &mut agent_executions,
            )
            .await?;

            if worker_harness.llm_edits {
                let mason_edit_application = self
                    .mason_generate_and_apply_edits(
                        run_id,
                        spec_obj,
                        &intent,
                        &briefing,
                        &implementation_plan,
                        target_source,
                        &staged_product,
                        &run_dir,
                    )
                    .await?;
                push_unique(&mut blackboard.artifact_refs, "mason_edit_application.json");
                push_unique(&mut blackboard.artifact_refs, "mason_edit_application.md");
                if mason_edit_application.proposal_generated {
                    push_unique(&mut blackboard.artifact_refs, "mason_edit_proposal.json");
                    push_unique(&mut blackboard.artifact_refs, "mason_edit_proposal.md");
                }
                if mason_edit_application.status == "applied" {
                    if let Some(context_bundle) = retriever_context_bundle.as_ref() {
                        let drift_guard = build_trail_drift_guard(
                            run_id,
                            spec_obj,
                            target_source,
                            &staged_product,
                            context_bundle,
                        )?;
                        self.write_json_file(&run_dir.join("trail_drift_guard.json"), &drift_guard)
                            .await?;
                        tokio::fs::write(
                            run_dir.join("trail_drift_guard.md"),
                            render_trail_drift_guard_markdown(&drift_guard),
                        )
                        .await?;
                    }
                }
                self.write_agent_execution(
                    &profiles,
                    "mason",
                    &format!(
                        "Generate and apply bounded LLM-authored edits for target '{}' inside the staged workspace.",
                        target_source.label
                    ),
                    &mason_edit_application.summary,
                    &serde_json::to_string_pretty(&mason_edit_application)?,
                    "implementation",
                    &implementation_episode,
                    spec_obj,
                    target_source,
                    &run_dir,
                    &mut agent_executions,
                )
                .await?;
            }
        }
        self.write_agent_execution(
            &profiles,
            "mason",
            &format!(
                "Prepare an implementation plan for target '{}' using the staged workspace.",
                target_source.label
            ),
            "Prepared a local implementation plan for the staged product copy.",
            &implementation_plan,
            "implementation",
            &implementation_episode,
            spec_obj,
            target_source,
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
        self.record_phase_attribution(
            run_id,
            &implementation_episode,
            "implementation",
            "mason",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
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
        claim_agent(
            &mut blackboard,
            "piper",
            "review tools and MCP availability",
        );
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
        let tool_plan = self
            .piper_tool_plan(spec_obj, target_source, &briefing)
            .await;
        tokio::fs::write(run_dir.join("tool_plan.md"), &tool_plan).await?;
        push_unique(&mut blackboard.artifact_refs, "tool_plan.md");
        self.write_agent_execution(
            &profiles,
            "piper",
            "Summarize the configured provider and MCP tool surface for this run.",
            "Captured current tool and MCP availability for the run.",
            &tool_plan,
            "tools",
            &tools_episode,
            spec_obj,
            target_source,
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
        self.finish_episode(&tools_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &tools_episode,
            "tools",
            "piper",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
        self.link_events(
            tools_start.event_id,
            tools_end.event_id,
            "contributed_to",
            0.9,
        )
        .await?;
        release_agent(&mut blackboard, "piper");
        push_unique(&mut blackboard.resolved_items, "tools");
        self.sync_blackboard(&blackboard, Some(&run_dir)).await?;

        if let Some(worker_harness) = &spec_obj.worker_harness {
            let forge_episode = self
                .start_episode(
                    run_id,
                    "retriever_forge",
                    "Run bounded retriever forge execution",
                )
                .await?;
            blackboard.current_phase = "retriever_forge".to_string();
            blackboard.active_goal = "Execute the bounded retriever forge packet".to_string();
            claim_agent(&mut blackboard, "mason", "run retriever forge packet");
            self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
            self.update_run_status(run_id, "retriever_forge").await?;
            let forge_start = self
                .record_event(
                    run_id,
                    Some(&forge_episode),
                    "retriever_forge",
                    "mason",
                    "running",
                    "Executing bounded retriever forge commands",
                    log_path,
                )
                .await?;
            let forge_report = self
                .execute_retriever_forge(
                    run_id,
                    spec_obj,
                    target_source,
                    worker_harness,
                    &run_dir,
                    &staged_product,
                )
                .await?;
            push_unique(
                &mut blackboard.artifact_refs,
                "retriever_execution_report.json",
            );
            push_unique(
                &mut blackboard.artifact_refs,
                "retriever_execution_report.md",
            );
            for artifact in &forge_report.returned_artifacts {
                push_unique(&mut blackboard.artifact_refs, artifact);
            }
            self.write_agent_execution(
                &profiles,
                "mason",
                &format!(
                    "Execute the retriever-forge packet for target '{}' inside the staged workspace.",
                    target_source.label
                ),
                &forge_report.summary,
                &serde_json::to_string_pretty(&forge_report)?,
                "retriever_forge",
                &forge_episode,
                spec_obj,
                target_source,
                &run_dir,
                &mut agent_executions,
            )
            .await?;
            let forge_end = self
                .record_event(
                    run_id,
                    Some(&forge_episode),
                    "retriever_forge",
                    "mason",
                    if forge_report.passed {
                        "complete"
                    } else {
                        "warning"
                    },
                    &forge_report.summary,
                    log_path,
                )
                .await?;
            self.finish_episode(
                &forge_episode,
                if forge_report.passed {
                    "success"
                } else {
                    "failure"
                },
                Some(if forge_report.passed { 1.0 } else { 0.5 }),
            )
            .await?;
            self.record_phase_attribution(
                run_id,
                &forge_episode,
                "retriever_forge",
                "mason",
                if forge_report.passed {
                    "success"
                } else {
                    "failure"
                },
                Some(if forge_report.passed { 1.0 } else { 0.5 }),
                &memory_context,
                &briefing,
                &agent_executions,
                &mut phase_attributions,
                &run_dir,
            )
            .await?;
            self.link_events(
                forge_start.event_id,
                forge_end.event_id,
                "contributed_to",
                if forge_report.passed { 1.0 } else { 0.5 },
            )
            .await?;
            release_agent(&mut blackboard, "mason");
            if forge_report.passed {
                push_unique(&mut blackboard.resolved_items, "retriever_forge");
                remove_blocker(&mut blackboard, "retriever_forge_failed");
            } else {
                push_unique(&mut blackboard.open_blockers, "retriever_forge_failed");
            }
            self.sync_blackboard(&blackboard, Some(&run_dir)).await?;
        }

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
        self.write_json_file(&run_dir.join("twin.json"), &twin)
            .await?;
        if let Some(narrative) = self
            .ash_twin_narrative(spec_obj, target_source, &twin, &briefing)
            .await
        {
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
            "twin",
            &twin_episode,
            spec_obj,
            target_source,
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
        self.finish_episode(&twin_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &twin_episode,
            "twin",
            "ash",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
        self.link_events(
            twin_start.event_id,
            twin_end.event_id,
            "contributed_to",
            1.0,
        )
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
        let mut validation = self
            .run_visible_validation(&workspace_root, &staged_product, spec_obj)
            .await?;
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
        if workspace_root
            .join("run")
            .join("corpus_results.json")
            .exists()
        {
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
            "validation",
            &validation_episode,
            spec_obj,
            target_source,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let validation_outcome = if validation.passed {
            "success"
        } else {
            "failure"
        };
        let validation_message = if let Some(message) = req.harness_message("validation") {
            format!("Failure harness forced validation failure: {message}")
        } else {
            format!(
                "Visible validation finished: {} checks, {} passed",
                validation.results.len(),
                validation
                    .results
                    .iter()
                    .filter(|result| result.passed)
                    .count()
            )
        };
        let validation_end = self
            .record_event(
                run_id,
                Some(&validation_episode),
                "validation",
                "bramble",
                if validation.passed {
                    "complete"
                } else {
                    "warning"
                },
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
        self.record_phase_attribution(
            run_id,
            &validation_episode,
            "validation",
            "bramble",
            validation_outcome,
            Some(if validation.passed { 1.0 } else { 0.5 }),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
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
            .bramble_interpret_validation(spec_obj, target_source, &validation, &briefing)
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
        let run_attempt = self.run_attempt_number(run_id).await?;
        let hidden_definitions =
            scenarios::load_hidden_scenarios(&self.paths.scenarios, &spec_obj.id)?;
        let mut hidden_scenarios = if req.run_hidden_scenarios {
            scenarios::evaluate_hidden_scenarios(
                &hidden_definitions,
                predicted_final_status,
                run_attempt,
                &events_so_far,
                &validation,
                &twin,
                &agent_executions,
                &run_dir,
            )
        } else {
            HiddenScenarioSummary {
                passed: true,
                results: vec![HiddenScenarioEvaluation {
                    scenario_id: "operator-skip".to_string(),
                    title: "Hidden scenarios skipped".to_string(),
                    passed: true,
                    details: "Hidden scenarios were skipped for this run by operator request."
                        .to_string(),
                    checks: vec![HiddenScenarioCheckResult {
                        kind: "operator_skip".to_string(),
                        passed: true,
                        details: "Launch request disabled hidden scenarios.".to_string(),
                    }],
                }],
            }
        };
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
            "hidden_scenarios",
            &hidden_episode,
            spec_obj,
            target_source,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        let hidden_outcome = if hidden_scenarios.passed {
            "success"
        } else {
            "failure"
        };
        let hidden_message = if !req.run_hidden_scenarios {
            "Hidden scenarios skipped by operator request".to_string()
        } else if let Some(message) = req.harness_message("hidden_scenarios") {
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
                if hidden_scenarios.passed {
                    "complete"
                } else {
                    "warning"
                },
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
        self.record_phase_attribution(
            run_id,
            &hidden_episode,
            "hidden_scenarios",
            "sable",
            hidden_outcome,
            Some(if hidden_scenarios.passed { 1.0 } else { 0.5 }),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
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
            .start_episode(
                run_id,
                "memory",
                "Store run summary back into long-term memory",
            )
            .await?;
        claim_agent(
            &mut blackboard,
            "coobie",
            "store run summary and prepare future recall",
        );
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
        self.store_project_memory_entry(
            target_source,
            &format!("run-{}", run_id),
            vec![
                "run".to_string(),
                "project-memory".to_string(),
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
                "Spec: {}
Product: {}
Visible validation passed: {}
Hidden scenarios passed: {}
Recommended steps: {}

Project memory root: {}

Top memory hits:
{}",
                spec_obj.id,
                target_source.label,
                validation.passed,
                hidden_scenarios.passed,
                intent.recommended_steps.join(", "),
                self.project_harkonnen_dir(target_source)
                    .join("project-memory")
                    .display(),
                memory_context.memory_hits.join(
                    "

"
                )
            ),
            project_memory_provenance(
                target_source,
                Some(run_id),
                Some(&spec_obj.id),
                vec![run_id.to_string()],
                vec![
                    "git commit or branch changes for the target repo".to_string(),
                    "hidden scenario oracle, dataset, or runtime assumptions change".to_string(),
                ],
                collect_spec_provenance_paths(spec_obj),
                collect_spec_code_under_test_paths(spec_obj),
                collect_spec_provenance_surfaces(spec_obj),
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
        self.finish_episode(&memory_store_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &memory_store_episode,
            "memory",
            "coobie",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
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
            "artifacts",
            &artifacts_episode,
            spec_obj,
            target_source,
            &run_dir,
            &mut agent_executions,
        )
        .await?;
        // Write exploration log before sealing the artifact bundle.
        if let Err(e) = self
            .write_exploration_log(run_id, spec_obj, target_source, &run_dir)
            .await
        {
            tracing::warn!("exploration log write failed: {e}");
        } else {
            push_unique(&mut blackboard.artifact_refs, "exploration_log.md");
            push_unique(&mut blackboard.artifact_refs, "exploration_log.json");
            push_unique(
                &mut blackboard.artifact_refs,
                "dead_end_registry_snapshot.json",
            );
            let _ = self.sync_blackboard(&blackboard, Some(&run_dir)).await;
        }
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
        self.finish_episode(&artifacts_episode, "success", Some(1.0))
            .await?;
        self.record_phase_attribution(
            run_id,
            &artifacts_episode,
            "artifacts",
            "flint",
            "success",
            Some(1.0),
            &memory_context,
            &briefing,
            &agent_executions,
            &mut phase_attributions,
            &run_dir,
        )
        .await?;
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
            phase_attributions: phase_attributions.clone(),
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
                    let _ =
                        tokio::fs::write(run_dir.join("causal_summary.md"), &report_response).await;
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
            memory_context,
            briefing,
        })
    }

    async fn retrieve_coobie_memory_context(
        &self,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
    ) -> Result<MemoryContextBundle> {
        let project_store = self.project_memory_store(target_source).await?;
        let mut project_memory = self
            .collect_memory_hits(&project_store, query_terms, "project memory")
            .await?;
        if let Some(project_context_hit) = self.read_project_context_hit(target_source).await? {
            project_memory.hits.insert(0, project_context_hit);
        }
        if let Some(project_scan_hit) = self.read_project_scan_hit(target_source).await? {
            project_memory
                .hits
                .insert(1.min(project_memory.hits.len()), project_scan_hit);
        }
        if let Some(resume_packet_hit) = self.read_project_resume_packet_hit(target_source).await? {
            project_memory
                .hits
                .insert(project_memory.hits.len().min(2), resume_packet_hit);
        }
        if let Some(strategy_register_hit) = self
            .read_project_strategy_register_hit(target_source)
            .await?
        {
            project_memory
                .hits
                .insert(project_memory.hits.len().min(2), strategy_register_hit);
        }
        if let Some(memory_status_hit) = self.read_project_memory_status_hit(target_source).await? {
            project_memory
                .hits
                .insert(project_memory.hits.len().min(4), memory_status_hit);
        }
        if let Some(mitigation_history_hit) = self
            .read_project_stale_memory_history_hit(target_source)
            .await?
        {
            project_memory
                .hits
                .insert(project_memory.hits.len().min(5), mitigation_history_hit);
        }
        for bundle_hit in self.collect_repo_local_context_hits(target_source, query_terms, 4)? {
            project_memory.hits.push(bundle_hit);
        }

        let mut core_memory = self
            .collect_memory_hits(&self.memory_store, query_terms, "core memory")
            .await?;

        project_memory.ids.sort();
        project_memory.ids.dedup();
        core_memory.ids.sort();
        core_memory.ids.dedup();
        project_store
            .mark_entries_loaded(&project_memory.ids)
            .await?;
        self.memory_store
            .mark_entries_loaded(&core_memory.ids)
            .await?;

        let mut memory_hits = Vec::new();
        let mut seen = HashSet::new();
        for hit in project_memory.hits.iter().chain(core_memory.hits.iter()) {
            if seen.insert(hit.clone()) {
                memory_hits.push(hit.clone());
            }
        }

        project_memory.hits.truncate(6);
        core_memory.hits.truncate(6);
        memory_hits.truncate(12);

        if memory_hits.is_empty() {
            memory_hits.push(format!(
                "No relevant project or core memory found for Coobie preflight queries: {}",
                query_terms.join(", ")
            ));
        }

        Ok(MemoryContextBundle {
            memory_hits,
            core_memory_hits: core_memory.hits,
            project_memory_hits: project_memory.hits,
            project_memory_root: Some(project_store.root.display().to_string()),
            core_memory_ids: core_memory.ids,
            project_memory_ids: project_memory.ids,
        })
    }

    async fn collect_memory_hits(
        &self,
        store: &MemoryStore,
        query_terms: &[String],
        source_label: &str,
    ) -> Result<CollectedMemoryHits> {
        let mut hits = Vec::new();
        let mut ids = Vec::new();
        let mut seen = HashSet::new();

        for term in query_terms {
            if term.trim().is_empty() {
                continue;
            }
            for hit in store.retrieve_context(term).await? {
                if hit.contains("No memories found") || hit.contains("Memory not initialized") {
                    continue;
                }
                if let Some(id) = extract_memory_entry_id(&hit) {
                    ids.push(id);
                }
                let labeled_hit = format!("[{source_label}] {hit}");
                if seen.insert(labeled_hit.clone()) {
                    hits.push(labeled_hit);
                }
            }
        }

        Ok(CollectedMemoryHits { hits, ids })
    }

    async fn record_memory_context_outcome(
        &self,
        target_source: &TargetSourceMetadata,
        memory_context: &MemoryContextBundle,
        success: bool,
    ) -> Result<()> {
        let project_store = self.project_memory_store(target_source).await?;
        project_store
            .record_outcome(&memory_context.project_memory_ids, success)
            .await?;
        self.memory_store
            .record_outcome(&memory_context.core_memory_ids, success)
            .await?;
        Ok(())
    }

    async fn project_memory_store(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<MemoryStore> {
        let store = MemoryStore::new(
            self.project_harkonnen_dir(target_source)
                .join("project-memory"),
        );
        self.ensure_project_memory_bootstrap(target_source, &store)
            .await?;
        store.reindex().await?;
        self.refresh_project_resume_packet(target_source, &store)
            .await?;
        Ok(store)
    }

    async fn ensure_project_evidence_bootstrap(&self, harkonnen_dir: &Path) -> Result<()> {
        let evidence_dir = harkonnen_dir.join("evidence");
        let raw_dir = evidence_dir.join("raw");
        let annotations_dir = evidence_dir.join("annotations");
        let causal_dir = evidence_dir.join("causal");
        let manifests_dir = evidence_dir.join("manifests");
        let history_dir = evidence_dir.join("history");
        tokio::fs::create_dir_all(&raw_dir).await?;
        tokio::fs::create_dir_all(&annotations_dir).await?;
        tokio::fs::create_dir_all(&causal_dir).await?;
        tokio::fs::create_dir_all(&manifests_dir).await?;
        tokio::fs::create_dir_all(&history_dir).await?;
        let guide_path = evidence_dir.join("00-evidence-guide.md");
        if !guide_path.exists() {
            tokio::fs::write(
                &guide_path,
                "# Evidence Guide

- Keep raw evidence in evidence/raw/.
- Store annotation bundles in evidence/annotations/.
- Store reviewed causal summaries in evidence/causal/.
- Audit annotation changes in evidence/history/.
",
            )
            .await?;
        }
        let sample_bundle = annotations_dir.join("sample-causal-window.yaml");
        if !sample_bundle.exists() {
            tokio::fs::write(
                &sample_bundle,
                "schema_version: 1
project: example-project
scenario: pressure-instability-review
dataset: historian-shift-a
notes:
  - Draft example showing how to link timeseries, video, and causal claims for Coobie.
sources:
  - source_id: historian_pressure
    kind: timeseries
    label: Wellhead pressure
    uri: .harkonnen/evidence/raw/historian-pressure.csv
    channels: [pressure_psi]
    tags: [historian, pressure]
  - source_id: pad_camera_01
    kind: video
    label: Pad camera 01
    uri: .harkonnen/evidence/raw/pad-camera-01.mp4
    tags: [video, operator]
annotations:
  - annotation_id: ann_pressure_drop_001
    annotation_type: causal_window
    title: Pressure drop after tension spike
    status: draft
    promote_to_memory: project
    source_ids: [historian_pressure, pad_camera_01]
    time_range:
      start_ms: 120000
      end_ms: 135000
    labels: [pressure_instability, operator_intervention]
    tags: [teaching-set, review]
    anchors:
      - anchor_id: anchor_pressure_spike
        source_id: historian_pressure
        kind: signal_window
        signal_keys: [pressure_psi]
        timestamp_ms: 121500
        time_range:
          start_ms: 121000
          end_ms: 123000
        notes: Pressure spike leading into drop.
      - anchor_id: anchor_operator_action
        source_id: pad_camera_01
        kind: video_window
        frame_index: 3645
        timestamp_ms: 124000
        time_range:
          start_ms: 123500
          end_ms: 126500
        notes: Operator adjusts equipment shortly before recovery.
    claims:
      - claim_id: claim_001
        relation: contributed_to
        cause: wireline_tension_spike
        effect: pressure_drop
        confidence: 0.78
        evidence_anchor_ids: [anchor_pressure_spike, anchor_operator_action]
        notes: Review whether operator action is response or cause.
    notes: Candidate teaching example for Coobie pattern matching and causal review.
    created_by: jerry
    created_at: 2026-04-01T00:00:00Z
    updated_at: 2026-04-01T00:00:00Z
",
            )
            .await?;
        }
        Ok(())
    }

    async fn ensure_project_memory_bootstrap(
        &self,
        target_source: &TargetSourceMetadata,
        store: &MemoryStore,
    ) -> Result<()> {
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        tokio::fs::create_dir_all(&harkonnen_dir).await?;
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        tokio::fs::create_dir_all(&store.root).await?;
        tokio::fs::create_dir_all(store.root.join("imports")).await?;
        tokio::fs::create_dir_all(harkonnen_dir.join("contexts")).await?;
        tokio::fs::create_dir_all(harkonnen_dir.join("skills")).await?;

        let project_context_path = harkonnen_dir.join("project-context.md");
        if !project_context_path.exists() {
            let context = format!(
                "# Project Context

- Project: {}
- Source path: {}
- Project memory root: {}
- Evidence root: {}
- Project scan: {}
- Project manifest: {}
- Resume packet: {}
- Strategy register: {}
- Memory status: {}
- Stale memory history: {}

## Coobie Memory Split
- Put repo-specific lessons, runtime facts, ports, protocols, datasets, oracle semantics, and commissioning notes in `.harkonnen/project-memory/`.
- Keep only strong cross-project or factory-wide learnings in Harkonnen core memory.
- Update this file with stable project facts Coobie should always read before planning.
",
                target_source.label,
                target_source.source_path,
                store.root.display(),
                harkonnen_dir.join("evidence").display(),
                harkonnen_dir.join("project-scan.md").display(),
                harkonnen_dir.join("project-manifest.json").display(),
                harkonnen_dir.join("resume-packet.md").display(),
                harkonnen_dir.join("strategy-register.md").display(),
                harkonnen_dir.join("memory-status.md").display(),
                harkonnen_dir.join("stale-memory-history.md").display(),
            );
            tokio::fs::write(&project_context_path, context).await?;
        }

        let manifest = build_project_scan_manifest(target_source, &store.root);
        self.write_json_file(&harkonnen_dir.join("project-manifest.json"), &manifest)
            .await?;

        let project_scan_path = harkonnen_dir.join("project-scan.md");
        if !project_scan_path.exists() {
            let scan = render_project_scan_markdown(&manifest);
            tokio::fs::write(&project_scan_path, scan).await?;
        }

        let instructions_md = harkonnen_dir.join("instructions.md");
        if !instructions_md.exists() {
            tokio::fs::write(
                &instructions_md,
                format!(
                    "# Repo Instructions

- Project: {}
- Use this file for repo-wide instructions that the retriever forge should preload before acting.
- Put scoped context in `.harkonnen/contexts/` and reusable workflow or domain bundles in `.harkonnen/skills/`.
",
                    target_source.label
                ),
            )
            .await?;
        }
        let contexts_guide = harkonnen_dir
            .join("contexts")
            .join("00-context-bundle-guide.md");
        if !contexts_guide.exists() {
            tokio::fs::write(
                &contexts_guide,
                "# Context Bundle Guide

- Add focused markdown context here for subsystems, interfaces, deployment surfaces, or domains.
- Prefer small, scoped files whose names match the subsystem or runtime surface they describe.
",
            )
            .await?;
        }
        let skills_guide = harkonnen_dir
            .join("skills")
            .join("00-skill-bundle-guide.md");
        if !skills_guide.exists() {
            tokio::fs::write(
                &skills_guide,
                "# Skill Bundle Guide

- Add markdown skill bundles here for repeatable repo-specific workflows.
- Examples: commissioning-checklist, historian-replay-debugging, plc-handshake-validation.
",
            )
            .await?;
        }

        let resume_packet_md = harkonnen_dir.join("resume-packet.md");
        if !resume_packet_md.exists() {
            tokio::fs::write(
                &resume_packet_md,
                "# Resume Packet\n\n- No resume packet has been generated yet.\n",
            )
            .await?;
        }
        let strategy_register_md = harkonnen_dir.join("strategy-register.md");
        if !strategy_register_md.exists() {
            tokio::fs::write(
                &strategy_register_md,
                "# Strategy Register\n\n- No repo-local dead-end strategies have been recorded yet.\n",
            )
            .await?;
        }
        let memory_status_md = harkonnen_dir.join("memory-status.md");
        if !memory_status_md.exists() {
            tokio::fs::write(
                &memory_status_md,
                "# Memory Status\n\n- No project-memory contradictions or supersessions have been recorded yet.\n",
            )
            .await?;
        }

        let stale_memory_history_md = harkonnen_dir.join("stale-memory-history.md");
        if !stale_memory_history_md.exists() {
            tokio::fs::write(
                &stale_memory_history_md,
                "# Stale Memory History\n\n- No stale-memory mitigation history has been recorded yet.\n",
            )
            .await?;
        }

        let guide_path = store.root.join("00-project-memory-guide.md");
        if !guide_path.exists() {
            let guide = format!(
                "---
tags: [project-memory, coobie, repo-local, guidance]
summary: Repo-local Coobie memory guide for {}
---
# Project Memory Guide

This directory is the durable home for knowledge that should travel with this repo.

Store here:
- domain facts specific to this product
- runtime/API details
- dataset and oracle semantics
- line-specific tuning or commissioning lessons
- accepted mitigations and known failure modes

Do not keep everything in Harkonnen core memory. Promote only durable cross-project patterns to the factory memory store.
",
                target_source.label,
            );
            tokio::fs::write(&guide_path, guide).await?;
        }

        Ok(())
    }

    async fn read_project_resume_packet_hit(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<String>> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("resume-packet.md");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "[resume packet] [{}] {}",
            path.display(),
            trimmed.chars().take(800).collect::<String>()
        )))
    }

    async fn read_project_strategy_register_hit(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<String>> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("strategy-register.md");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "[strategy register] [{}] {}",
            path.display(),
            trimmed.chars().take(800).collect::<String>()
        )))
    }

    async fn read_project_memory_status_hit(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<String>> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("memory-status.md");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "[memory status] [{}] {}",
            path.display(),
            trimmed.chars().take(800).collect::<String>()
        )))
    }

    async fn read_project_stale_memory_history_hit(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<String>> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("stale-memory-history.md");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        Ok(Some(format!(
            "[stale memory history] [{}] {}",
            path.display(),
            trimmed.chars().take(800).collect::<String>()
        )))
    }

    fn collect_repo_local_context_hits(
        &self,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        limit: usize,
    ) -> Result<Vec<String>> {
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        let (context_entries, skill_entries) = discover_repo_local_context_entries(
            &harkonnen_dir,
            Some(target_source),
            None,
            query_terms,
        )?;
        let mut hits = Vec::new();
        for entry in context_entries
            .into_iter()
            .chain(skill_entries.into_iter())
            .take(limit)
        {
            hits.push(format!(
                "[repo-local {}] [{}] {}",
                entry.category, entry.path, entry.summary
            ));
        }
        Ok(hits)
    }

    async fn read_project_scan_hit(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<String>> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("project-scan.md");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let snippet = trimmed.chars().take(800).collect::<String>();
        Ok(Some(format!(
            "[project scan] [{}] {}",
            path.display(),
            snippet
        )))
    }

    async fn load_project_resume_packet(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<ProjectResumePacket> {
        let store = MemoryStore::new(
            self.project_harkonnen_dir(target_source)
                .join("project-memory"),
        );
        self.refresh_project_resume_packet(target_source, &store)
            .await
    }

    async fn load_project_stale_memory_history(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<StaleMemoryMitigationHistory> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("stale-memory-history.json");
        if !path.exists() {
            return Ok(StaleMemoryMitigationHistory::default());
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    async fn read_project_context_hit(
        &self,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<String>> {
        let path = self
            .project_harkonnen_dir(target_source)
            .join("project-context.md");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let snippet = trimmed.chars().take(600).collect::<String>();
        Ok(Some(format!(
            "[project context] [{}] {}",
            path.display(),
            snippet
        )))
    }

    fn project_harkonnen_dir(&self, target_source: &TargetSourceMetadata) -> PathBuf {
        self.repo_harkonnen_dir(Path::new(&target_source.source_path))
    }

    fn repo_harkonnen_dir(&self, project_root: &Path) -> PathBuf {
        project_root.join(".harkonnen")
    }

    async fn store_project_memory_entry(
        &self,
        target_source: &TargetSourceMetadata,
        id: &str,
        tags: Vec<String>,
        summary: &str,
        content: &str,
        provenance: MemoryProvenance,
    ) -> Result<()> {
        let store = self.project_memory_store(target_source).await?;
        store
            .store_with_metadata(id, tags, summary, content, provenance)
            .await
    }

    async fn target_source_for_run(&self, run_id: &str) -> Result<Option<TargetSourceMetadata>> {
        let path = self.run_dir(run_id).join("target_source.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(Some(serde_json::from_str(&raw)?))
    }

    async fn load_run_briefing(&self, run_id: &str) -> Result<Option<CoobieBriefing>> {
        let path = self.run_dir(run_id).join("coobie_briefing.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = tokio::fs::read_to_string(&path).await?;
        Ok(Some(serde_json::from_str(&raw)?))
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
                    lesson
                        .intervention
                        .clone()
                        .unwrap_or_default()
                        .to_lowercase(),
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
                .filter(|(_, lesson)| {
                    lesson
                        .tags
                        .iter()
                        .any(|tag| tag == "causal" || tag == "lesson")
                })
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
            let created_at =
                chrono::DateTime::parse_from_rfc3339(row.get::<String, _>("created_at").as_str())?
                    .with_timezone(&Utc);
            let scenario_passed = row.get::<Option<i64>, _>("scenario_passed").unwrap_or(0) != 0;

            let entry = aggregates
                .entry(cause_id)
                .or_insert_with(|| CauseAggregate {
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

    async fn collect_briefing_evidence_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<(
        Vec<CoobieEvidenceCitation>,
        Vec<CoobieEvidenceCitation>,
        Vec<CoobieEvidenceCitation>,
        Vec<CoobieEvidenceCitation>,
        Vec<CoobieEvidenceCitation>,
    )> {
        let resume_packet = self.load_project_resume_packet(target_source).await?;
        let exploration = self
            .collect_relevant_exploration_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let strategy = self
            .collect_relevant_strategy_register_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let mitigation = self
            .collect_relevant_mitigation_history_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
                &resume_packet.stale_memory,
            )
            .await?;
        let forge = self
            .collect_relevant_retriever_forge_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let forge_preference_outcomes = self
            .collect_relevant_preferred_forge_outcome_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        Ok((
            exploration,
            strategy,
            mitigation,
            forge,
            forge_preference_outcomes,
        ))
    }

    async fn collect_relevant_exploration_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<CoobieEvidenceCitation>> {
        let runs = self.list_runs(40).await?;
        let mut scored = Vec::<(i32, chrono::DateTime<Utc>, CoobieEvidenceCitation)>::new();

        for run in runs {
            if run.product != target_source.label {
                continue;
            }
            let exploration_path = self.run_dir(&run.run_id).join("exploration_log.json");
            if !exploration_path.exists() {
                continue;
            }
            let raw = match tokio::fs::read_to_string(&exploration_path).await {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let log = match serde_json::from_str::<ExplorationLogArtifact>(&raw) {
                Ok(log) => log,
                Err(_) => continue,
            };
            for entry in log.entries {
                if !matches!(entry.outcome.as_str(), "failure" | "blocked") {
                    continue;
                }
                let haystack = format!(
                    "{} {} {} {} {} {} {} {} {} {} {} {}",
                    run.spec_id,
                    run.product,
                    entry.phase,
                    entry.agent,
                    entry.strategy,
                    entry.outcome,
                    entry.failure_constraint,
                    entry.surviving_structure,
                    entry.reformulation,
                    entry.artifacts.join(" "),
                    entry.parameters.join(" "),
                    entry.open_questions.join(" ")
                );
                let mut score = score_briefing_evidence(
                    &haystack,
                    &spec_obj.id,
                    &target_source.label,
                    query_terms,
                    domain_signals,
                );
                if run.spec_id == spec_obj.id {
                    score += 8;
                }
                if score <= 0 {
                    continue;
                }
                scored.push((
                    score,
                    run.created_at,
                    CoobieEvidenceCitation {
                        citation_id: format!("exploration:{}:{}", run.run_id, entry.episode_id),
                        source_type: "exploration_log".to_string(),
                        run_id: run.run_id.clone(),
                        episode_id: Some(entry.episode_id.clone()),
                        phase: entry.phase.clone(),
                        agent: entry.agent.clone(),
                        summary: format!(
                            "{} used strategy '{}' and ended {}",
                            entry.agent, entry.strategy, entry.outcome
                        ),
                        evidence: format!(
                            "failure_constraint={}; surviving_structure={}; reformulation={}",
                            entry.failure_constraint,
                            entry.surviving_structure,
                            entry.reformulation
                        ),
                    },
                ));
            }
        }

        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok(scored
            .into_iter()
            .map(|(_, _, citation)| citation)
            .take(3)
            .collect())
    }

    async fn collect_relevant_mitigation_history_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
        current_risks: &[ProjectResumeRisk],
    ) -> Result<Vec<CoobieEvidenceCitation>> {
        let history = self
            .load_project_stale_memory_history(target_source)
            .await?;
        if history.records.is_empty() {
            return Ok(Vec::new());
        }

        let current_risk_ids = current_risks
            .iter()
            .map(|risk| risk.memory_id.as_str())
            .collect::<HashSet<_>>();
        let mut scored = Vec::<(i32, String, CoobieEvidenceCitation)>::new();

        for record in history.records.iter().rev().take(12) {
            for entry in &record.entries {
                let haystack = format!(
                    "{} {} {} {} {} {} {} {} {} {}",
                    record.spec_id,
                    record.product,
                    entry.memory_id,
                    entry.severity,
                    entry.severity_score,
                    entry.status,
                    entry.mitigation_steps.join(" "),
                    entry.related_checks.join(" "),
                    entry.evidence.join(" "),
                    record.resolved_since_previous.join(" ")
                );
                let mut score = score_briefing_evidence(
                    &haystack,
                    &spec_obj.id,
                    &target_source.label,
                    query_terms,
                    domain_signals,
                );
                if record.spec_id == spec_obj.id {
                    score += 8;
                }
                if current_risk_ids.contains(entry.memory_id.as_str()) {
                    score += 12;
                }
                if entry.risk_reduced_from_previous == Some(true) {
                    score += 4;
                }
                if entry.status == "unresolved" {
                    score += 3;
                }
                if score <= 0 {
                    continue;
                }
                scored.push((
                    score,
                    record.generated_at.clone(),
                    CoobieEvidenceCitation {
                        citation_id: format!("mitigation:{}:{}", record.run_id, entry.memory_id),
                        source_type: "stale_memory_history".to_string(),
                        run_id: record.run_id.clone(),
                        episode_id: None,
                        phase: "stale_memory_followup".to_string(),
                        agent: "coobie".to_string(),
                        summary: format!(
                            "memory {} was {} at severity {} score {}",
                            entry.memory_id, entry.status, entry.severity, entry.severity_score
                        ),
                        evidence: format!(
                            "previous_score={}; reduced_from_previous={}; mitigation_steps={}; related_checks={}; evidence={}",
                            entry
                                .previous_severity_score
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "none".to_string()),
                            entry
                                .risk_reduced_from_previous
                                .map(|value| if value { "true" } else { "false" }.to_string())
                                .unwrap_or_else(|| "unknown".to_string()),
                            if entry.mitigation_steps.is_empty() {
                                "none".to_string()
                            } else {
                                entry.mitigation_steps.join(" | ")
                            },
                            if entry.related_checks.is_empty() {
                                "none".to_string()
                            } else {
                                entry.related_checks.join(" | ")
                            },
                            if entry.evidence.is_empty() {
                                "none".to_string()
                            } else {
                                entry.evidence.join(" | ")
                            }
                        ),
                    },
                ));
            }
        }

        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok(scored
            .into_iter()
            .map(|(_, _, citation)| citation)
            .take(4)
            .collect())
    }

    async fn collect_relevant_retriever_forge_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<CoobieEvidenceCitation>> {
        let runs = self.list_runs(40).await?;
        let mut scored = Vec::<(i32, chrono::DateTime<Utc>, CoobieEvidenceCitation)>::new();

        for run in runs {
            if run.product != target_source.label {
                continue;
            }
            let run_dir = self.run_dir(&run.run_id);
            let report_path = run_dir.join("retriever_execution_report.json");
            if !report_path.exists() {
                continue;
            }
            let report_raw = match tokio::fs::read_to_string(&report_path).await {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let report = match serde_json::from_str::<RetrieverExecutionArtifact>(&report_raw) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let hook_path = run_dir.join("retriever_forge_hooks.json");
            let hooks = if hook_path.exists() {
                tokio::fs::read_to_string(&hook_path)
                    .await
                    .ok()
                    .and_then(|raw| serde_json::from_str::<RetrieverHookArtifact>(&raw).ok())
            } else {
                None
            };
            let denied = hooks
                .as_ref()
                .map(|artifact| {
                    artifact
                        .records
                        .iter()
                        .filter(|record| record.decision == "deny")
                        .count()
                })
                .unwrap_or(0);
            let failed = report
                .executed_commands
                .iter()
                .filter(|command| !command.passed)
                .count();
            let haystack = format!(
                "{} {} {} {} {} {} {} {} {}",
                run.spec_id,
                run.product,
                report.adapter,
                report.profile,
                report.summary,
                report.returned_artifacts.join(" "),
                report
                    .executed_commands
                    .iter()
                    .map(|command| format!(
                        "{} {} {} {}",
                        command.label, command.raw_command, command.source, command.rationale
                    ))
                    .collect::<Vec<_>>()
                    .join(" "),
                hooks
                    .as_ref()
                    .map(|artifact| artifact
                        .records
                        .iter()
                        .map(|record| format!(
                            "{} {} {} {}",
                            record.stage,
                            record.decision,
                            record.raw_command,
                            record.reasons.join(" ")
                        ))
                        .collect::<Vec<_>>()
                        .join(" "))
                    .unwrap_or_default(),
                if report.passed { "passed" } else { "failed" },
            );
            let mut score = score_briefing_evidence(
                &haystack,
                &spec_obj.id,
                &target_source.label,
                query_terms,
                domain_signals,
            );
            if run.spec_id == spec_obj.id {
                score += 8;
            }
            if denied > 0 {
                score += 12;
            }
            if failed > 0 {
                score += 8;
            }
            if score <= 0 {
                continue;
            }
            scored.push((
                score,
                run.created_at,
                CoobieEvidenceCitation {
                    citation_id: format!("forge:{}", run.run_id),
                    source_type: "retriever_forge".to_string(),
                    run_id: run.run_id.clone(),
                    episode_id: None,
                    phase: "retriever_forge".to_string(),
                    agent: "mason".to_string(),
                    summary: format!(
                        "retriever forge {} with {} command(s), {} denied, {} failed",
                        if report.passed {
                            "passed"
                        } else {
                            "returned issues"
                        },
                        report.executed_commands.len(),
                        denied,
                        failed
                    ),
                    evidence: format!(
                        "summary={}; hook_artifact={}; returned_artifacts={}; commands={}",
                        report.summary,
                        report.hook_artifact,
                        report.returned_artifacts.join(" | "),
                        report
                            .executed_commands
                            .iter()
                            .map(|command| format!(
                                "{}:{}",
                                command.label,
                                if command.passed { "pass" } else { "fail" }
                            ))
                            .collect::<Vec<_>>()
                            .join(" | ")
                    ),
                },
            ));
        }

        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok(scored
            .into_iter()
            .map(|(_, _, citation)| citation)
            .take(4)
            .collect())
    }

    async fn collect_preferred_retriever_forge_commands(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<String>> {
        let runs = self.list_runs(40).await?;
        let mut scores = HashMap::<String, (i32, chrono::DateTime<Utc>)>::new();

        for run in runs {
            if run.product != target_source.label {
                continue;
            }
            let report_path = self
                .run_dir(&run.run_id)
                .join("retriever_execution_report.json");
            if !report_path.exists() {
                continue;
            }
            let report_raw = match tokio::fs::read_to_string(&report_path).await {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let report = match serde_json::from_str::<RetrieverExecutionArtifact>(&report_raw) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let haystack = format!(
                "{} {} {} {} {} {}",
                run.spec_id,
                run.product,
                report.adapter,
                report.profile,
                report.summary,
                report
                    .executed_commands
                    .iter()
                    .map(|command| format!(
                        "{} {} {} {} {}",
                        command.label,
                        command.raw_command,
                        command.source,
                        command.rationale,
                        if command.passed { "pass" } else { "fail" }
                    ))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            let mut run_score = score_briefing_evidence(
                &haystack,
                &spec_obj.id,
                &target_source.label,
                query_terms,
                domain_signals,
            );
            if run.spec_id == spec_obj.id {
                run_score += 8;
            }
            if report.passed {
                run_score += 10;
            }
            run_score += (report.preferred_commands_helped.len() as i32) * 2;
            run_score -= report.preferred_commands_stale.len() as i32;
            if run_score <= 0 {
                continue;
            }

            for command in &report.executed_commands {
                let mut score = run_score;
                if command.source == "spec.test_commands" {
                    score += 2;
                }
                if command.was_preferred {
                    score += 3;
                }
                match command.preference_outcome.as_deref() {
                    Some("helped") => score += 6,
                    Some("did_not_help") => score -= 5,
                    _ => {}
                }
                if command.passed {
                    score += 4;
                } else {
                    score -= 6;
                }
                if score <= 0 {
                    continue;
                }
                let entry = scores
                    .entry(command.raw_command.clone())
                    .or_insert((0, run.created_at));
                entry.0 += score;
                if run.created_at > entry.1 {
                    entry.1 = run.created_at;
                }
            }
        }

        let mut ranked = scores.into_iter().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                 .0
                .cmp(&left.1 .0)
                .then_with(|| right.1 .1.cmp(&left.1 .1))
                .then_with(|| left.0.cmp(&right.0))
        });
        Ok(ranked
            .into_iter()
            .map(|(command, _)| command)
            .take(5)
            .collect())
    }

    async fn collect_relevant_preferred_forge_outcome_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<CoobieEvidenceCitation>> {
        let runs = self.list_runs(40).await?;
        let mut scored = Vec::<(i32, chrono::DateTime<Utc>, CoobieEvidenceCitation)>::new();

        for run in runs {
            if run.product != target_source.label {
                continue;
            }
            let report_path = self
                .run_dir(&run.run_id)
                .join("retriever_execution_report.json");
            if !report_path.exists() {
                continue;
            }
            let report_raw = match tokio::fs::read_to_string(&report_path).await {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let report = match serde_json::from_str::<RetrieverExecutionArtifact>(&report_raw) {
                Ok(value) => value,
                Err(_) => continue,
            };

            for command in report
                .executed_commands
                .iter()
                .filter(|command| command.was_preferred)
            {
                let haystack = format!(
                    "{} {} {} {} {} {} {} {} {}",
                    run.spec_id,
                    run.product,
                    report.adapter,
                    report.profile,
                    report.summary,
                    command.label,
                    command.raw_command,
                    command.preference_outcome.clone().unwrap_or_default(),
                    command.rationale,
                );
                let mut score = score_briefing_evidence(
                    &haystack,
                    &spec_obj.id,
                    &target_source.label,
                    query_terms,
                    domain_signals,
                );
                if run.spec_id == spec_obj.id {
                    score += 8;
                }
                match command.preference_outcome.as_deref() {
                    Some("helped") => score += 12,
                    Some("did_not_help") => score += 7,
                    _ => score += 2,
                }
                if report
                    .preferred_commands_helped
                    .iter()
                    .any(|value| value == &command.raw_command)
                {
                    score += 3;
                }
                if report
                    .preferred_commands_stale
                    .iter()
                    .any(|value| value == &command.raw_command)
                {
                    score += 2;
                }
                if score <= 0 {
                    continue;
                }
                let summary = match command.preference_outcome.as_deref() {
                    Some("helped") => format!(
                        "preferred command '{}' kept helping in retriever forge run {}",
                        command.raw_command, run.run_id
                    ),
                    Some("did_not_help") => format!(
                        "preferred command '{}' went stale in retriever forge run {}",
                        command.raw_command, run.run_id
                    ),
                    _ => format!(
                        "preferred command '{}' was selected in retriever forge run {}",
                        command.raw_command, run.run_id
                    ),
                };
                scored.push((
                    score,
                    run.created_at,
                    CoobieEvidenceCitation {
                        citation_id: format!(
                            "forge-preference:{}:{}",
                            run.run_id,
                            stable_key_fragment(&command.raw_command)
                        ),
                        source_type: "retriever_forge_preference_outcome".to_string(),
                        run_id: run.run_id.clone(),
                        episode_id: None,
                        phase: "retriever_forge".to_string(),
                        agent: "coobie".to_string(),
                        summary,
                        evidence: format!(
                            "preference_rank={}; preference_outcome={}; run_passed={}; selected={}; helped={}; stale={}",
                            command
                                .preference_rank
                                .map(|value| value.to_string())
                                .unwrap_or_else(|| "n/a".to_string()),
                            command
                                .preference_outcome
                                .clone()
                                .unwrap_or_else(|| "n/a".to_string()),
                            if report.passed { "true" } else { "false" },
                            if report
                                .preferred_commands_selected
                                .iter()
                                .any(|value| value == &command.raw_command)
                            {
                                "true"
                            } else {
                                "false"
                            },
                            if report
                                .preferred_commands_helped
                                .iter()
                                .any(|value| value == &command.raw_command)
                            {
                                "true"
                            } else {
                                "false"
                            },
                            if report
                                .preferred_commands_stale
                                .iter()
                                .any(|value| value == &command.raw_command)
                            {
                                "true"
                            } else {
                                "false"
                            }
                        ),
                    },
                ));
            }
        }

        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok(scored
            .into_iter()
            .map(|(_, _, citation)| citation)
            .take(5)
            .collect())
    }

    async fn collect_evidence_memory_exemplar_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<(Vec<CoobieEvidenceCitation>, Vec<CoobieEvidenceCitation>)> {
        let project_store = self.project_memory_store(target_source).await?;
        let project_entries = project_store.list_entries().await?;
        let core_entries = self.memory_store.list_entries().await?;
        let mut pattern_scored = Vec::<(i32, String, CoobieEvidenceCitation)>::new();
        let mut causal_scored = Vec::<(i32, String, CoobieEvidenceCitation)>::new();

        for (scope, entries) in [("project", project_entries), ("core", core_entries)] {
            for entry in entries {
                if !entry.tags.iter().any(|tag| {
                    matches!(
                        tag.as_str(),
                        "evidence"
                            | "causal-evidence"
                            | "pattern_example"
                            | "pattern-example"
                            | "causal_window"
                            | "causal-window"
                            | "negative_example"
                            | "negative-example"
                    )
                }) {
                    continue;
                }

                let haystack = format!(
                    "{} {} {} {} {} {}",
                    entry.id,
                    entry.summary,
                    entry.tags.join(" "),
                    entry.content,
                    entry.provenance.source_kind.clone().unwrap_or_default(),
                    entry.provenance.observed_surfaces.join(" "),
                );
                let mut score = score_briefing_evidence(
                    &haystack,
                    &spec_obj.id,
                    &target_source.label,
                    query_terms,
                    domain_signals,
                );
                if entry.provenance.source_label.as_deref() == Some(target_source.label.as_str()) {
                    score += 8;
                }
                if entry
                    .tags
                    .iter()
                    .any(|tag| matches!(tag.as_str(), "causal_window" | "causal-window"))
                {
                    score += 5;
                }
                if entry.tags.iter().any(|tag| {
                    matches!(
                        tag.as_str(),
                        "pattern_example"
                            | "pattern-example"
                            | "negative_example"
                            | "negative-example"
                    )
                }) {
                    score += 5;
                }
                if score <= 0 {
                    continue;
                }

                let is_pattern = entry.tags.iter().any(|tag| {
                    matches!(
                        tag.as_str(),
                        "pattern_example"
                            | "pattern-example"
                            | "negative_example"
                            | "negative-example"
                    )
                });
                let citation = CoobieEvidenceCitation {
                    citation_id: format!("evidence-memory:{}:{}", scope, entry.id),
                    source_type: format!("{}_evidence_memory", scope),
                    run_id: entry
                        .provenance
                        .source_run_id
                        .clone()
                        .unwrap_or_else(|| "memory".to_string()),
                    episode_id: None,
                    phase: format!("{}_memory", scope),
                    agent: "coobie".to_string(),
                    summary: if is_pattern {
                        format!("pattern exemplar from {} memory: {}", scope, entry.summary)
                    } else {
                        format!("causal exemplar from {} memory: {}", scope, entry.summary)
                    },
                    evidence: format!(
                        "memory_id={}; tags={}; source_kind={}; observed_paths={}",
                        entry.id,
                        entry.tags.join(" | "),
                        entry
                            .provenance
                            .source_kind
                            .clone()
                            .unwrap_or_else(|| "unknown".to_string()),
                        if entry.provenance.observed_paths.is_empty() {
                            "none".to_string()
                        } else {
                            entry.provenance.observed_paths.join(" | ")
                        }
                    ),
                };

                if is_pattern {
                    pattern_scored.push((score, entry.created_at.clone(), citation));
                } else {
                    causal_scored.push((score, entry.created_at.clone(), citation));
                }
            }
        }

        pattern_scored
            .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        causal_scored
            .sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok((
            pattern_scored
                .into_iter()
                .map(|(_, _, citation)| citation)
                .take(4)
                .collect(),
            causal_scored
                .into_iter()
                .map(|(_, _, citation)| citation)
                .take(4)
                .collect(),
        ))
    }

    async fn collect_nearest_evidence_window_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<CoobieEvidenceCitation>> {
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        let annotations_dir = harkonnen_dir.join("evidence").join("annotations");
        if !annotations_dir.exists() {
            return Ok(Vec::new());
        }

        let mut scored = Vec::<(i32, String, CoobieEvidenceCitation)>::new();
        let mut reader = tokio::fs::read_dir(&annotations_dir).await?;
        while let Some(entry) = reader.next_entry().await? {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase())
                .unwrap_or_default();
            if ext != "yaml" && ext != "yml" {
                continue;
            }
            let raw = match tokio::fs::read_to_string(&path).await {
                Ok(raw) => raw,
                Err(_) => continue,
            };
            let bundle = match serde_yaml::from_str::<EvidenceAnnotationBundle>(&raw) {
                Ok(bundle) => bundle,
                Err(_) => continue,
            };

            for annotation in &bundle.annotations {
                if !annotation_is_review_ready(annotation) {
                    continue;
                }

                let matched_sources = bundle
                    .sources
                    .iter()
                    .filter(|source| {
                        annotation
                            .source_ids
                            .iter()
                            .any(|id| id == &source.source_id)
                    })
                    .map(|source| format!("{}:{}:{}", source.source_id, source.kind, source.label))
                    .collect::<Vec<_>>();
                let anchor_summary = annotation
                    .anchors
                    .iter()
                    .map(|anchor| format!("{}:{}:{}", anchor.anchor_id, anchor.kind, anchor.label))
                    .collect::<Vec<_>>();
                let claim_summary = annotation
                    .claims
                    .iter()
                    .map(|claim| format!("{}:{}->{}", claim.relation, claim.cause, claim.effect))
                    .collect::<Vec<_>>();
                let time_summary =
                    render_evidence_time_range_summary(annotation.time_range.as_ref());
                let haystack = format!(
                    "{} {} {} {} {} {} {} {} {} {} {} {} {}",
                    bundle.project,
                    bundle.scenario,
                    bundle.dataset,
                    bundle.notes.join(" "),
                    annotation.annotation_id,
                    annotation.annotation_type,
                    annotation.title,
                    annotation.labels.join(" "),
                    annotation.tags.join(" "),
                    annotation.notes,
                    matched_sources.join(" "),
                    claim_summary.join(" "),
                    anchor_summary.join(" "),
                );
                let mut score = score_briefing_evidence(
                    &haystack,
                    &spec_obj.id,
                    &target_source.label,
                    query_terms,
                    domain_signals,
                );
                if bundle.project == target_source.label {
                    score += 8;
                }
                if annotation
                    .annotation_type
                    .eq_ignore_ascii_case("causal_window")
                {
                    score += 6;
                }
                if !annotation.claims.is_empty() {
                    score += 5;
                }
                if !annotation.anchors.is_empty() {
                    score += 3;
                }
                if score <= 0 {
                    continue;
                }

                let title = if annotation.title.trim().is_empty() {
                    annotation.annotation_id.clone()
                } else {
                    annotation.title.trim().to_string()
                };
                let bundle_name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("annotation-bundle");
                scored.push((
                    score,
                    if annotation.updated_at.trim().is_empty() {
                        annotation.created_at.clone()
                    } else {
                        annotation.updated_at.clone()
                    },
                    CoobieEvidenceCitation {
                        citation_id: format!(
                            "evidence-window:{}:{}",
                            bundle_name, annotation.annotation_id
                        ),
                        source_type: "evidence_annotation_window".to_string(),
                        run_id: "annotation".to_string(),
                        episode_id: Some(annotation.annotation_id.clone()),
                        phase: annotation.annotation_type.clone(),
                        agent: "coobie".to_string(),
                        summary: format!(
                            "nearest prior evidence window '{}' from scenario '{}'",
                            title,
                            if bundle.scenario.trim().is_empty() {
                                "unspecified"
                            } else {
                                bundle.scenario.trim()
                            }
                        ),
                        evidence: format!(
                            "bundle={}; dataset={}; time={}; labels={}; claims={}; sources={}",
                            path.display(),
                            if bundle.dataset.trim().is_empty() {
                                "unspecified"
                            } else {
                                bundle.dataset.trim()
                            },
                            time_summary,
                            if annotation.labels.is_empty() {
                                "none".to_string()
                            } else {
                                annotation.labels.join(" | ")
                            },
                            if claim_summary.is_empty() {
                                "none".to_string()
                            } else {
                                claim_summary.join(" | ")
                            },
                            if matched_sources.is_empty() {
                                "none".to_string()
                            } else {
                                matched_sources.join(" | ")
                            }
                        ),
                    },
                ));
            }
        }

        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok(scored
            .into_iter()
            .map(|(_, _, citation)| citation)
            .take(5)
            .collect())
    }

    async fn collect_relevant_strategy_register_citations(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
    ) -> Result<Vec<CoobieEvidenceCitation>> {
        let registry_path = self.paths.factory.join("state").join("dead_ends.json");
        if !registry_path.exists() {
            return Ok(Vec::new());
        }
        let raw = tokio::fs::read_to_string(&registry_path).await?;
        let registry = serde_json::from_str::<DeadEndRegistry>(&raw).unwrap_or_default();
        let mut scored = Vec::<(i32, String, CoobieEvidenceCitation)>::new();

        for entry in registry.entries {
            if entry.product != target_source.label {
                continue;
            }
            let haystack = format!(
                "{} {} {} {} {} {} {} {}",
                entry.spec_id,
                entry.product,
                entry.phase,
                entry.agent,
                entry.strategy,
                entry.failure_constraint,
                entry.surviving_structure,
                entry.reformulation
            );
            let mut score = score_briefing_evidence(
                &haystack,
                &spec_obj.id,
                &target_source.label,
                query_terms,
                domain_signals,
            );
            if entry.spec_id == spec_obj.id {
                score += 8;
            }
            if score <= 0 {
                continue;
            }
            scored.push((
                score,
                entry.created_at.clone(),
                CoobieEvidenceCitation {
                    citation_id: entry.registry_id.clone(),
                    source_type: "strategy_register".to_string(),
                    run_id: entry.run_id.clone(),
                    episode_id: None,
                    phase: entry.phase.clone(),
                    agent: entry.agent.clone(),
                    summary: format!(
                        "{} recorded strategy '{}' as a dead end",
                        entry.agent, entry.strategy
                    ),
                    evidence: format!(
                        "failure_constraint={}; surviving_structure={}; reformulation={}",
                        entry.failure_constraint, entry.surviving_structure, entry.reformulation
                    ),
                },
            ));
        }

        scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| right.1.cmp(&left.1)));
        Ok(scored
            .into_iter()
            .map(|(_, _, citation)| citation)
            .take(3)
            .collect())
    }

    async fn build_coobie_briefing(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        query_terms: &[String],
        domain_signals: &[String],
        memory_context: &MemoryContextBundle,
    ) -> Result<CoobieBriefing> {
        let relevant_lessons = self
            .find_relevant_lessons(query_terms, domain_signals)
            .await?;
        let prior_causes = self.summarize_prior_causes(5).await?;
        let (
            exploration_citations,
            strategy_register_citations,
            mitigation_history_citations,
            forge_evidence_citations,
            preferred_forge_outcome_citations,
        ) = self
            .collect_briefing_evidence_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let (evidence_pattern_exemplar_citations, evidence_causal_exemplar_citations) = self
            .collect_evidence_memory_exemplar_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let nearest_evidence_window_citations = self
            .collect_nearest_evidence_window_citations(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let pattern_matching_focus =
            build_pattern_matching_focus(&evidence_pattern_exemplar_citations);
        let causal_chain_focus = build_causal_chain_focus(&evidence_causal_exemplar_citations);
        let mut enriched_query_terms = query_terms.to_vec();
        let preferred_forge_commands = self
            .collect_preferred_retriever_forge_commands(
                spec_obj,
                target_source,
                query_terms,
                domain_signals,
            )
            .await?;
        let resume_packet = self.load_project_resume_packet(target_source).await?;
        let prior_report_count =
            sqlx::query("SELECT COUNT(DISTINCT run_id) AS cnt FROM causal_hypotheses")
                .fetch_one(&self.pool)
                .await?
                .get::<i64, _>("cnt") as usize;
        let application_risks = build_application_risks(
            spec_obj,
            domain_signals,
            &memory_context.memory_hits,
            &prior_causes,
        );
        let environment_risks = build_environment_risks(spec_obj, domain_signals);
        let regulatory_considerations = build_regulatory_considerations(spec_obj, domain_signals);
        let mut stale_memory_mitigation_plan =
            build_stale_memory_mitigation_plan(&resume_packet.stale_memory);
        let mut recommended_guardrails = build_recommended_guardrails(
            spec_obj,
            domain_signals,
            &memory_context.memory_hits,
            &prior_causes,
            &relevant_lessons,
        );
        let mut required_checks = build_required_checks(
            spec_obj,
            domain_signals,
            &regulatory_considerations,
            &relevant_lessons,
        );
        let mut open_questions =
            build_coobie_open_questions(spec_obj, domain_signals, &regulatory_considerations);
        apply_stale_memory_mitigations(
            &resume_packet.stale_memory,
            &mut recommended_guardrails,
            &mut required_checks,
            &mut open_questions,
        );
        apply_mitigation_history_context(
            &mitigation_history_citations,
            &mut stale_memory_mitigation_plan,
            &mut recommended_guardrails,
            &mut open_questions,
        );
        apply_forge_evidence_context(
            &forge_evidence_citations,
            &mut recommended_guardrails,
            &mut required_checks,
            &mut open_questions,
        );
        apply_preferred_forge_outcome_context(
            &preferred_forge_outcome_citations,
            &mut recommended_guardrails,
            &mut required_checks,
            &mut open_questions,
        );
        apply_evidence_exemplar_context(
            &evidence_pattern_exemplar_citations,
            &evidence_causal_exemplar_citations,
            &pattern_matching_focus,
            &causal_chain_focus,
            &mut enriched_query_terms,
            &mut recommended_guardrails,
            &mut required_checks,
            &mut open_questions,
        );
        apply_nearest_evidence_window_context(
            &nearest_evidence_window_citations,
            &mut enriched_query_terms,
            &mut required_checks,
            &mut open_questions,
        );

        let mut briefing = CoobieBriefing {
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            query_terms: enriched_query_terms,
            domain_signals: domain_signals.to_vec(),
            prior_report_count,
            memory_hits: memory_context.memory_hits.clone(),
            core_memory_hits: memory_context.core_memory_hits.clone(),
            project_memory_hits: memory_context.project_memory_hits.clone(),
            resume_packet_summary: resume_packet.summary.clone(),
            resume_packet_risks: resume_packet.stale_memory.clone(),
            stale_memory_mitigation_plan,
            exploration_citations,
            strategy_register_citations,
            mitigation_history_citations,
            evidence_pattern_exemplar_citations,
            evidence_causal_exemplar_citations,
            nearest_evidence_window_citations,
            pattern_matching_focus,
            causal_chain_focus,
            forge_evidence_citations,
            preferred_forge_outcome_citations,
            preferred_forge_commands,
            relevant_lessons,
            prior_causes,
            project_components: spec_obj.project_components.clone(),
            scenario_blueprint: spec_obj.scenario_blueprint.clone(),
            project_memory_root: memory_context.project_memory_root.clone(),
            application_risks,
            environment_risks,
            regulatory_considerations,
            recommended_guardrails,
            required_checks,
            open_questions,
            coobie_response: String::new(),
            generated_at: Utc::now(),
        };
        briefing.coobie_response = self
            .coobie_llm_briefing_response(spec_obj, target_source, &briefing)
            .await
            .unwrap_or_else(|| crate::coobie::render_coobie_briefing_response(&briefing));
        Ok(briefing)
    }

    async fn coobie_llm_briefing_response(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        briefing: &CoobieBriefing,
    ) -> Option<String> {
        let provider = llm::build_provider("coobie", "default", &self.paths.setup)?;
        let prompt_support = self.agent_prompt_support("coobie", spec_obj, target_source);
        let system_instruction = prompt_support
            .as_ref()
            .map(|support| format!(
                "{}

Task contract:
You are Coobie, a memory and causal reasoning Labrador for a software factory. You receive a structured briefing object and must render a concise Markdown preflight for the pack. Summarize the strongest prior context, the biggest risks, the guardrails, required checks, open questions, and the next trail to follow. Stay concrete. No filler.",
                support.system_instruction
            ))
            .unwrap_or_else(|| "You are Coobie, a memory and causal reasoning Labrador for a software factory. You receive a structured briefing object and must render a concise Markdown preflight for the pack. Summarize the strongest prior context, the biggest risks, the guardrails, required checks, open questions, and the next trail to follow. Stay concrete. No filler.".to_string());
        let repo_context_block = prompt_support
            .as_ref()
            .map(|support| support.repo_context_block.as_str())
            .unwrap_or(
                "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
            );
        let briefing_json = serde_json::to_string_pretty(briefing).ok()?;
        let spec_yaml =
            serde_yaml::to_string(spec_obj).unwrap_or_else(|_| format!("{:?}", spec_obj));
        let req = LlmRequest::simple(
            system_instruction,
            format!(
                "SPEC:
```yaml
{spec_yaml}
```

TARGET: {} ({})

BRIEFING FACTS:
```json
{briefing_json}
```

{repo_context_block}

Render Coobie's preflight markdown for the pack. Incorporate repo-local guidance and skill bundles where relevant, but do not invent facts outside the briefing object.",
                target_source.label,
                target_source.source_path,
                repo_context_block = repo_context_block,
            ),
        );

        match provider.complete(req).await {
            Ok(resp) => Some(resp.content),
            Err(error) => {
                tracing::warn!(
                    "Coobie LLM call failed ({}), using procedural briefing",
                    error
                );
                None
            }
        }
    }

    async fn build_evidence_match_report(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        briefing: &CoobieBriefing,
    ) -> Result<EvidenceMatchReport> {
        let mut labels = Vec::new();
        let mut claims = Vec::new();
        let mut sources = Vec::new();

        if let Some(blueprint) = &spec_obj.scenario_blueprint {
            for topic in &blueprint.coobie_memory_topics {
                push_unique(&mut labels, topic);
            }
        }
        for component in &spec_obj.project_components {
            for note in &component.notes {
                push_unique(&mut labels, note);
            }
        }
        for citation in briefing
            .nearest_evidence_window_citations
            .iter()
            .chain(briefing.evidence_causal_exemplar_citations.iter())
        {
            for value in parse_citation_field_values(&citation.evidence, "labels") {
                push_unique(&mut labels, &value);
            }
            for value in parse_citation_field_values(&citation.evidence, "claims") {
                push_unique(&mut claims, &value);
            }
            for value in parse_citation_field_values(&citation.evidence, "sources") {
                push_unique(&mut sources, &value);
            }
        }

        let time_span_ms = briefing
            .nearest_evidence_window_citations
            .iter()
            .find_map(|citation| parse_citation_time_span_ms(&citation.evidence));

        let matches = self
            .search_similar_evidence_windows(
                &target_source.source_path,
                Some(&spec_obj.id),
                &briefing.query_terms,
                &labels,
                &claims,
                &sources,
                time_span_ms,
                8,
            )
            .await?;

        let assessments = matches
            .into_iter()
            .enumerate()
            .map(|(index, window)| build_evidence_match_assessment(index + 1, window))
            .collect::<Vec<_>>();

        let mut summary = vec![format!(
            "Compared {} reviewed evidence window candidate(s) for spec '{}'.",
            assessments.len(),
            spec_obj.id
        )];
        if let Some(best) = assessments.first() {
            summary.push(format!(
                "Top result: {} [{}] score={} confidence={:.0}%.",
                best.window.title,
                best.match_type,
                best.score,
                best.confidence * 100.0
            ));
        } else {
            summary.push(
                "No reviewed evidence windows matched the current query context.".to_string(),
            );
        }
        if !labels.is_empty() {
            summary.push(format!("Labels compared: {}", labels.join(", ")));
        }
        if !claims.is_empty() {
            summary.push(format!("Claims compared: {}", claims.join(" | ")));
        }
        if !sources.is_empty() {
            summary.push(format!("Sources compared: {}", sources.join(" | ")));
        }

        Ok(EvidenceMatchReport {
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            query_source: "coobie_briefing".to_string(),
            selected_window_summary: assessments.first().map(|assessment| {
                format!(
                    "{} [{}] {}",
                    assessment.window.title,
                    assessment.window.annotation_id,
                    assessment.window.time_summary
                )
            }),
            query_terms: briefing.query_terms.clone(),
            labels,
            claims,
            sources,
            time_span_ms,
            summary,
            assessments,
            generated_at: Utc::now(),
        })
    }

    pub async fn build_evidence_match_report_from_query(
        &self,
        project_root: &str,
        spec_id: Option<&str>,
        query_source: &str,
        selected_window_summary: Option<String>,
        query_terms: &[String],
        labels: &[String],
        claims: &[String],
        sources: &[String],
        time_span_ms: Option<i64>,
        limit: usize,
    ) -> Result<EvidenceMatchReport> {
        let target_source = self.resolve_memory_ingest_target(project_root).await?;
        let matches = self
            .search_similar_evidence_windows(
                project_root,
                spec_id,
                query_terms,
                labels,
                claims,
                sources,
                time_span_ms,
                limit,
            )
            .await?;
        let assessments = matches
            .into_iter()
            .enumerate()
            .map(|(index, window)| build_evidence_match_assessment(index + 1, window))
            .collect::<Vec<_>>();
        let resolved_spec_id = spec_id
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("ad_hoc_evidence_match")
            .to_string();
        let mut summary = vec![format!(
            "Compared {} reviewed evidence window candidate(s) for spec '{}'.",
            assessments.len(),
            resolved_spec_id
        )];
        if let Some(selected) = selected_window_summary.as_ref() {
            summary.push(format!("Selected window: {}", selected));
        }
        if let Some(best) = assessments.first() {
            summary.push(format!(
                "Top result: {} [{}] score={} confidence={:.0}%.",
                best.window.title,
                best.match_type,
                best.score,
                best.confidence * 100.0
            ));
        } else {
            summary.push(
                "No reviewed evidence windows matched the current query context.".to_string(),
            );
        }
        if !labels.is_empty() {
            summary.push(format!("Labels compared: {}", labels.join(", ")));
        }
        if !claims.is_empty() {
            summary.push(format!("Claims compared: {}", claims.join(" | ")));
        }
        if !sources.is_empty() {
            summary.push(format!("Sources compared: {}", sources.join(" | ")));
        }

        Ok(EvidenceMatchReport {
            spec_id: resolved_spec_id,
            product: target_source.label,
            query_source: query_source.to_string(),
            selected_window_summary,
            query_terms: query_terms.to_vec(),
            labels: labels.to_vec(),
            claims: claims.to_vec(),
            sources: sources.to_vec(),
            time_span_ms,
            summary,
            assessments,
            generated_at: Utc::now(),
        })
    }

    async fn scout_intake(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        briefing: &CoobieBriefing,
    ) -> Result<IntentPackage> {
        if let Some(provider) = llm::build_provider("scout", "claude", &self.paths.setup) {
            let memory_section = format_memory_context(&briefing.memory_hits);
            let briefing_json = serde_json::to_string_pretty(briefing).unwrap_or_default();
            let spec_yaml =
                serde_yaml::to_string(spec_obj).unwrap_or_else(|_| format!("{:?}", spec_obj));
            let prompt_support = self.agent_prompt_support("scout", spec_obj, target_source);
            let system_instruction = prompt_support
                .as_ref()
                .map(|support| format!(
                    "{}

Task contract:
You are Scout, a spec-intake specialist for a software factory. Read a YAML spec, prior memory context, and repo-local guidance, then produce a concise implementation intent package as JSON with these fields: spec_id (string), summary (one sentence), ambiguity_notes (array of strings), recommended_steps (ordered array of strings). Respond with valid JSON only and no markdown.",
                    support.system_instruction
                ))
                .unwrap_or_else(|| "You are Scout, a spec-intake specialist for a software factory. Read a YAML spec and prior memory context, then produce a concise implementation intent package as JSON with these fields: spec_id (string), summary (one sentence), ambiguity_notes (array of strings), recommended_steps (ordered array of strings). Respond with valid JSON only and no markdown.".to_string());
            let repo_context_block = prompt_support
                .as_ref()
                .map(|support| support.repo_context_block.as_str())
                .unwrap_or(
                    "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
                );

            let req = LlmRequest::simple(
                system_instruction,
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

{repo_context_block}

Produce the intent package JSON and incorporate Coobie guardrails, required checks, open questions, repo-local constraints, and skill bundles when they are relevant.",
                    response = briefing.coobie_response,
                    repo_context_block = repo_context_block,
                ),
            );

            match provider.complete(req).await {
                Ok(resp) => {
                    if let Ok(parsed) = serde_json::from_str::<IntentPackage>(&resp.content.trim())
                    {
                        return Ok(parsed);
                    }
                    let stripped = resp
                        .content
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
        let stub =
            build_implementation_plan(spec_obj, intent, briefing, staged_product, target_source);

        if let Some(provider) = llm::build_provider("mason", "default", &self.paths.setup) {
            let spec_yaml =
                serde_yaml::to_string(spec_obj).unwrap_or_else(|_| format!("{:?}", spec_obj));
            let constraints = mason_slim_briefing(briefing);
            let prompt_support = self.agent_prompt_support("mason", spec_obj, target_source);
            let system_instruction = prompt_support
                .as_ref()
                .map(|support| format!(
                    "{}

Task contract:
You are Mason, an implementation planning specialist for a software factory. You receive a YAML spec and operating constraints. Produce a clear, actionable implementation plan in Markdown with sections: ## Target, ## Scope, ## Acceptance Criteria, ## Recommended Steps, ## Risks. Be specific and avoid filler.",
                    support.system_instruction
                ))
                .unwrap_or_else(|| "You are Mason, an implementation planning specialist for a software factory. You receive a YAML spec and operating constraints. Produce a clear, actionable implementation plan in Markdown with sections: ## Target, ## Scope, ## Acceptance Criteria, ## Recommended Steps, ## Risks. Be specific and avoid filler.".to_string());
            let repo_context_block = prompt_support
                .as_ref()
                .map(|support| support.repo_context_block.as_str())
                .unwrap_or(
                    "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
                );

            let req = LlmRequest::simple(
                system_instruction,
                format!(
                    "SPEC:
```yaml
{spec_yaml}
```

TARGET: {} ({})

CONSTRAINTS:
{constraints}

{repo_context_block}

Produce the implementation plan markdown. Treat guardrails and required checks as hard constraints.",
                    target_source.label,
                    target_source.source_path,
                    repo_context_block = repo_context_block,
                ),
            );

            match provider.complete(req).await {
                Ok(resp) => return resp.content,
                Err(e) => tracing::warn!("Mason LLM call failed ({}), using stub", e),
            }
        }

        stub
    }

    async fn write_mason_edit_proposal(
        &self,
        run_dir: &Path,
        proposal: &MasonEditProposalArtifact,
    ) -> Result<()> {
        self.write_json_file(&run_dir.join("mason_edit_proposal.json"), proposal)
            .await?;
        tokio::fs::write(
            run_dir.join("mason_edit_proposal.md"),
            render_mason_edit_proposal_markdown(proposal),
        )
        .await?;
        Ok(())
    }

    async fn write_mason_edit_application(
        &self,
        run_dir: &Path,
        application: &MasonEditApplicationArtifact,
    ) -> Result<()> {
        self.write_json_file(&run_dir.join("mason_edit_application.json"), application)
            .await?;
        tokio::fs::write(
            run_dir.join("mason_edit_application.md"),
            render_mason_edit_application_markdown(application),
        )
        .await?;
        Ok(())
    }

    async fn mason_generate_and_apply_edits(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        _intent: &IntentPackage,
        briefing: &CoobieBriefing,
        _implementation_plan: &str,
        target_source: &TargetSourceMetadata,
        staged_product: &Path,
        run_dir: &Path,
    ) -> Result<MasonEditApplicationArtifact> {
        let editable_paths =
            collect_staged_code_under_test_paths(spec_obj, target_source, &self.paths.root);
        let generated_at = Utc::now().to_rfc3339();

        if editable_paths.is_empty() {
            let application = MasonEditApplicationArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                generated_at,
                status: "skipped_no_editable_paths".to_string(),
                summary: "Mason edit lane skipped because the spec did not resolve any code-under-test paths inside the staged workspace.".to_string(),
                proposal_generated: false,
                changed_files: Vec::new(),
                git_branch: None,
            };
            self.write_mason_edit_application(run_dir, &application)
                .await?;
            return Ok(application);
        }

        let context_files = build_mason_context_files(staged_product, &editable_paths)?;
        if context_files.is_empty() {
            let application = MasonEditApplicationArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                generated_at,
                status: "skipped_no_context".to_string(),
                summary: "Mason edit lane skipped because no bounded text file context could be loaded for the editable paths.".to_string(),
                proposal_generated: false,
                changed_files: Vec::new(),
                git_branch: None,
            };
            self.write_mason_edit_application(run_dir, &application)
                .await?;
            return Ok(application);
        }

        let Some(provider) = llm::build_provider("mason", "default", &self.paths.setup) else {
            let application = MasonEditApplicationArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                generated_at,
                status: "skipped_no_provider".to_string(),
                summary: "Mason edit lane skipped because no live LLM provider is configured for Mason in the active setup.".to_string(),
                proposal_generated: false,
                changed_files: Vec::new(),
                git_branch: None,
            };
            self.write_mason_edit_application(run_dir, &application)
                .await?;
            return Ok(application);
        };

        let spec_yaml =
            serde_yaml::to_string(spec_obj).unwrap_or_else(|_| format!("{:?}", spec_obj));
        let constraints = mason_slim_briefing(briefing);
        let context_paths = context_files
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();
        let context_block = context_files
            .iter()
            .map(|file| {
                format!(
                    "FILE: {}{}\n```text\n{}\n```",
                    file.path,
                    if file.truncated { " [truncated]" } else { "" },
                    file.content
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n");

        let prompt_support = self.agent_prompt_support("mason", spec_obj, target_source);
        let system_instruction = prompt_support
            .as_ref()
            .map(|support| format!(
                "{}

Task contract:
You are Mason, an implementation specialist for a software factory. Produce valid JSON only. Return an object with keys summary (string), rationale (array of strings), and edits (array). Each edit must contain path (relative path inside the staged workspace), action (must be 'write'), summary (string), and content (the full file contents after your edit). Only edit files within the provided editable paths. Do not emit markdown. Do not explain outside the JSON object.",
                support.system_instruction
            ))
            .unwrap_or_else(|| "You are Mason, an implementation specialist for a software factory. Produce valid JSON only. Return an object with keys summary (string), rationale (array of strings), and edits (array). Each edit must contain path (relative path inside the staged workspace), action (must be 'write'), summary (string), and content (the full file contents after your edit). Only edit files within the provided editable paths. Do not emit markdown. Do not explain outside the JSON object.".to_string());
        let repo_context_block = prompt_support
            .as_ref()
            .map(|support| support.repo_context_block.as_str())
            .unwrap_or(
                "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
            );

        let req = LlmRequest {
            messages: vec![
                Message::system(system_instruction),
                Message::user(format!(
                    "SPEC:
```yaml
{spec_yaml}
```

TARGET: {} ({})

EDITABLE PATHS:
{}

CONSTRAINTS:
{constraints}

{repo_context_block}

CURRENT FILE CONTEXT:
{}

Generate the smallest safe set of file writes needed to satisfy the spec's acceptance criteria. Only touch the editable paths. Respect guardrails and required checks as hard constraints. If no safe edit is justified, return edits as an empty array and explain why in rationale.",
                    target_source.label,
                    staged_product.display(),
                    render_list(&editable_paths, "No editable paths were resolved."),
                    context_block,
                    repo_context_block = repo_context_block,
                )),
            ],
            max_tokens: 8000,
            temperature: 0.1,
        };

        let response = match provider.complete(req).await {
            Ok(response) => response,
            Err(error) => {
                let application = MasonEditApplicationArtifact {
                    run_id: run_id.to_string(),
                    spec_id: spec_obj.id.clone(),
                    product: target_source.label.clone(),
                    generated_at: Utc::now().to_rfc3339(),
                    status: "llm_error".to_string(),
                    summary: format!("Mason edit lane failed before applying edits: {}", error),
                    proposal_generated: false,
                    changed_files: Vec::new(),
                    git_branch: None,
                };
                self.write_mason_edit_application(run_dir, &application)
                    .await?;
                return Ok(application);
            }
        };

        let proposal = match parse_mason_edit_proposal(&response.content) {
            Ok(proposal) => proposal,
            Err(error) => {
                let application = MasonEditApplicationArtifact {
                    run_id: run_id.to_string(),
                    spec_id: spec_obj.id.clone(),
                    product: target_source.label.clone(),
                    generated_at: Utc::now().to_rfc3339(),
                    status: "invalid_llm_edit_response".to_string(),
                    summary: format!(
                        "Mason edit lane produced an invalid JSON edit proposal: {}",
                        error
                    ),
                    proposal_generated: false,
                    changed_files: Vec::new(),
                    git_branch: None,
                };
                self.write_mason_edit_application(run_dir, &application)
                    .await?;
                return Ok(application);
            }
        };

        for edit in &proposal.edits {
            let normalized = normalize_project_path(&edit.path);
            if normalized.is_empty() {
                let application = MasonEditApplicationArtifact {
                    run_id: run_id.to_string(),
                    spec_id: spec_obj.id.clone(),
                    product: target_source.label.clone(),
                    generated_at: Utc::now().to_rfc3339(),
                    status: "invalid_edit_path".to_string(),
                    summary: "Mason proposed an empty edit path, so the edit batch was rejected."
                        .to_string(),
                    proposal_generated: false,
                    changed_files: Vec::new(),
                    git_branch: None,
                };
                self.write_mason_edit_application(run_dir, &application)
                    .await?;
                return Ok(application);
            }
            if !edit.action.eq_ignore_ascii_case("write") {
                let application = MasonEditApplicationArtifact {
                    run_id: run_id.to_string(),
                    spec_id: spec_obj.id.clone(),
                    product: target_source.label.clone(),
                    generated_at: Utc::now().to_rfc3339(),
                    status: "invalid_edit_action".to_string(),
                    summary: format!(
                        "Mason proposed unsupported edit action '{}' for {}.",
                        edit.action, normalized
                    ),
                    proposal_generated: false,
                    changed_files: Vec::new(),
                    git_branch: None,
                };
                self.write_mason_edit_application(run_dir, &application)
                    .await?;
                return Ok(application);
            }
            if !path_allowed_for_edit(&normalized, &editable_paths) {
                let application = MasonEditApplicationArtifact {
                    run_id: run_id.to_string(),
                    spec_id: spec_obj.id.clone(),
                    product: target_source.label.clone(),
                    generated_at: Utc::now().to_rfc3339(),
                    status: "edit_outside_scope".to_string(),
                    summary: format!(
                        "Mason proposed an edit outside the editable scope: {}",
                        normalized
                    ),
                    proposal_generated: false,
                    changed_files: Vec::new(),
                    git_branch: None,
                };
                self.write_mason_edit_application(run_dir, &application)
                    .await?;
                return Ok(application);
            }
            let _ = join_workspace_relative_path(staged_product, &normalized)?;
        }

        let proposal_artifact = MasonEditProposalArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            generated_at: Utc::now().to_rfc3339(),
            editable_paths: editable_paths.clone(),
            context_paths,
            summary: proposal.summary.clone(),
            rationale: proposal.rationale.clone(),
            edits: proposal.edits.clone(),
        };
        self.write_mason_edit_proposal(run_dir, &proposal_artifact)
            .await?;

        if proposal.edits.is_empty() {
            let application = MasonEditApplicationArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                generated_at: Utc::now().to_rfc3339(),
                status: "no_changes".to_string(),
                summary: if proposal.summary.trim().is_empty() {
                    "Mason reviewed the staged workspace but did not propose any file edits."
                        .to_string()
                } else {
                    proposal.summary.clone()
                },
                proposal_generated: true,
                changed_files: Vec::new(),
                git_branch: None,
            };
            self.write_mason_edit_application(run_dir, &application)
                .await?;
            return Ok(application);
        }

        let mut changed_files = Vec::new();
        for edit in &proposal.edits {
            let normalized = normalize_project_path(&edit.path);
            let destination = join_workspace_relative_path(staged_product, &normalized)?;
            if let Some(parent) = destination.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            let existing = tokio::fs::read_to_string(&destination).await.ok();
            if existing.as_deref() != Some(edit.content.as_str()) {
                tokio::fs::write(&destination, &edit.content).await?;
                push_unique(&mut changed_files, &normalized);
            }
        }

        let summary = if changed_files.is_empty() {
            format!(
                "Mason generated an edit proposal for '{}' but every file already matched the requested content.",
                target_source.label
            )
        } else {
            format!(
                "Mason applied {} LLM-authored file edit(s) inside the staged workspace for '{}'.",
                changed_files.len(),
                target_source.label
            )
        };

        // If git_branch is requested and there are real changes, commit them to a
        // new branch in the source repo so a proper diff is always available.
        let git_branch = if spec_obj
            .worker_harness
            .as_ref()
            .map(|h| h.git_branch)
            .unwrap_or(false)
            && !changed_files.is_empty()
        {
            let source_root = PathBuf::from(&target_source.source_path);
            let short_run = &run_id[..8];
            let branch_name = format!("mason/{}-{}", spec_obj.id, short_run);
            match mason_commit_branch(
                &source_root,
                &staged_product,
                &changed_files,
                &branch_name,
                &spec_obj.title,
                run_id,
            )
            .await
            {
                Ok(()) => {
                    tracing::info!("Mason committed edits to branch {}", branch_name);
                    Some(branch_name)
                }
                Err(e) => {
                    tracing::warn!("Mason git branch commit failed ({}), continuing", e);
                    None
                }
            }
        } else {
            None
        };

        let application = MasonEditApplicationArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            generated_at: Utc::now().to_rfc3339(),
            status: "applied".to_string(),
            summary,
            proposal_generated: true,
            changed_files,
            git_branch,
        };
        self.write_mason_edit_application(run_dir, &application)
            .await?;
        Ok(application)
    }

    /// Piper: build a tool and MCP surface plan, using an LLM when available.
    async fn piper_tool_plan(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        briefing: &CoobieBriefing,
    ) -> String {
        let stub = self.build_tool_plan(briefing);

        if let Some(provider) = llm::build_provider("piper", "default", &self.paths.setup) {
            let prompt_support = self.agent_prompt_support("piper", spec_obj, target_source);
            let system_instruction = prompt_support
                .as_ref()
                .map(|support| format!(
                    "{}

Task contract:
You are Piper, a tool and MCP routing specialist for a software factory. You receive the current tool surface and a spec summary. Produce a brief Markdown report describing which tools are available, which are relevant to this spec, and any gaps or warnings. No filler.",
                    support.system_instruction
                ))
                .unwrap_or_else(|| "You are Piper, a tool and MCP routing specialist for a software factory. You receive the current tool surface and a spec summary. Produce a brief Markdown report describing which tools are available, which are relevant to this spec, and any gaps or warnings. No filler.".to_string());
            let repo_context_block = prompt_support
                .as_ref()
                .map(|support| support.repo_context_block.as_str())
                .unwrap_or(
                    "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
                );
            let req = LlmRequest::simple(
                system_instruction,
                format!(
                    "SPEC: {} — {}
TARGET: {} ({})
DOMAIN SIGNALS: {}
REGULATORY: {}
REQUIRED CHECKS: {}

TOOL SURFACE:
{stub}

{repo_context_block}

Produce the tool plan analysis and explicitly call out tools or MCP gaps that block Coobie's required checks or conflict with repo-local skill guidance.",
                    spec_obj.id,
                    spec_obj.title,
                    target_source.label,
                    target_source.source_path,
                    if briefing.domain_signals.is_empty() { "none".to_string() } else { briefing.domain_signals.join(", ") },
                    if briefing.regulatory_considerations.is_empty() { "none".to_string() } else { briefing.regulatory_considerations.join(" | ") },
                    if briefing.required_checks.is_empty() { "none".to_string() } else { briefing.required_checks.join(" | ") },
                    repo_context_block = repo_context_block,
                ),
            );

            match provider.complete(req).await {
                Ok(resp) => return resp.content,
                Err(e) => tracing::warn!("Piper LLM call failed ({}), using stub", e),
            }
        }

        stub
    }

    async fn execute_retriever_forge(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        worker_harness: &WorkerHarnessConfig,
        run_dir: &Path,
        staged_product: &Path,
    ) -> Result<RetrieverExecutionArtifact> {
        let packet_raw =
            tokio::fs::read_to_string(run_dir.join("retriever_task_packet.json")).await?;
        let review_raw = tokio::fs::read_to_string(run_dir.join("trail_review_chain.json")).await?;
        let dispatch_raw =
            tokio::fs::read_to_string(run_dir.join("retriever_dispatch.json")).await?;
        let packet = serde_json::from_str::<WorkerTaskEnvelope>(&packet_raw)?;
        let _review = serde_json::from_str::<PlanReviewChainArtifact>(&review_raw)?;
        let dispatch = serde_json::from_str::<RetrieverDispatchArtifact>(&dispatch_raw)?;
        let continuity_file = worker_harness
            .continuity_file
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "trail-state.json".to_string());
        let hook_artifact_name = "retriever_forge_hooks.json".to_string();
        let command_plan = build_retriever_command_plan(spec_obj, staged_product, &packet)?;
        let preferred_rank_map = packet
            .preferred_commands
            .iter()
            .enumerate()
            .map(|(idx, command)| (command.as_str(), idx + 1))
            .collect::<HashMap<_, _>>();
        let logs_dir = run_dir.join("retriever-forge");
        tokio::fs::create_dir_all(&logs_dir).await?;

        let drift_guard_raw =
            tokio::fs::read_to_string(run_dir.join("trail_drift_guard.json")).await?;
        let drift_guard = serde_json::from_str::<TrailDriftGuardArtifact>(&drift_guard_raw)?;
        let drift_check = verify_trail_drift_guard(run_id, spec_obj, target_source, &drift_guard)?;
        self.write_json_file(&run_dir.join("trail_drift_check.json"), &drift_check)
            .await?;
        tokio::fs::write(
            run_dir.join("trail_drift_check.md"),
            render_trail_drift_check_markdown(&drift_check),
        )
        .await?;

        let mut executed_commands = Vec::new();
        let mut hook_records = Vec::new();
        let mut preferred_commands_selected = Vec::new();
        let mut preferred_commands_helped = Vec::new();
        let mut preferred_commands_stale = Vec::new();
        let mut returned_artifacts = vec![
            "retriever_execution_report.json".to_string(),
            "retriever_execution_report.md".to_string(),
            hook_artifact_name.clone(),
            "retriever_forge_hooks.md".to_string(),
            "trail_drift_guard.json".to_string(),
            "trail_drift_guard.md".to_string(),
            "trail_drift_check.json".to_string(),
            "trail_drift_check.md".to_string(),
        ];
        let mut all_passed = drift_check.passed;
        let mut drift_failure_summary = None;
        if !drift_check.passed {
            hook_records.push(RetrieverHookRecord {
                stage: "pre_execution_guard".to_string(),
                decision: "deny".to_string(),
                tool: "trail_drift_guard".to_string(),
                command_label: "trail drift guard".to_string(),
                raw_command: "guarded workspace fingerprint verification".to_string(),
                source: "trail_drift_guard".to_string(),
                rationale: "The retriever forge must fail closed when guarded code-under-test or repo-local context paths drift after planning.".to_string(),
                reasons: drift_check
                    .changed_paths
                    .iter()
                    .cloned()
                    .chain(drift_check.missing_paths.iter().cloned())
                    .collect(),
                passed: Some(false),
                exit_code: None,
                log_artifact: None,
                created_at: Utc::now().to_rfc3339(),
            });
            drift_failure_summary = Some(format!(
                "Retriever forge halted before execution for '{}' because guarded workspace paths drifted after planning. {}",
                target_source.label,
                drift_check.summary
            ));
        }
        if drift_check.passed {
            for (idx, planned) in command_plan.iter().enumerate() {
                let (decision, reasons) = evaluate_retriever_hook(&packet, planned);
                hook_records.push(RetrieverHookRecord {
                    stage: "pre_tool_use".to_string(),
                    decision: decision.clone(),
                    tool: "shell_command".to_string(),
                    command_label: planned.label.clone(),
                    raw_command: planned.raw_command.clone(),
                    source: planned.source.clone(),
                    rationale: planned.rationale.clone(),
                    reasons: reasons.clone(),
                    passed: None,
                    exit_code: None,
                    log_artifact: None,
                    created_at: Utc::now().to_rfc3339(),
                });

                let log_artifact = format!("retriever-forge/command-{:02}.log", idx + 1);
                let preference_rank = preferred_rank_map
                    .get(planned.raw_command.as_str())
                    .copied();
                let was_preferred = preference_rank.is_some();
                if was_preferred {
                    push_unique(&mut preferred_commands_selected, &planned.raw_command);
                }
                if decision == "deny" {
                    all_passed = false;
                    let denied_message = if reasons.is_empty() {
                        "Keeper denied this retriever-forge command before execution.".to_string()
                    } else {
                        format!(
                            "Keeper denied this retriever-forge command: {}",
                            reasons.join(" | ")
                        )
                    };
                    let log_body = format!(
                        "# {}

- Decision: deny
- Source: {}
- Rationale: {}
- Command: {}
- Reasons: {}
",
                        planned.label,
                        planned.source,
                        planned.rationale,
                        planned.raw_command,
                        if reasons.is_empty() {
                            "none".to_string()
                        } else {
                            reasons.join(" | ")
                        },
                    );
                    tokio::fs::write(run_dir.join(&log_artifact), log_body).await?;
                    returned_artifacts.push(log_artifact.clone());
                    if was_preferred {
                        push_unique(&mut preferred_commands_stale, &planned.raw_command);
                    }
                    executed_commands.push(RetrieverCommandExecution {
                        label: planned.label.clone(),
                        raw_command: planned.raw_command.clone(),
                        source: planned.source.clone(),
                        rationale: planned.rationale.clone(),
                        was_preferred,
                        preference_rank,
                        preference_outcome: Some(if was_preferred {
                            "did_not_help".to_string()
                        } else {
                            "not_preferred".to_string()
                        }),
                        passed: false,
                        exit_code: None,
                        stdout: String::new(),
                        stderr: denied_message.clone(),
                        log_artifact: log_artifact.clone(),
                    });
                    hook_records.push(RetrieverHookRecord {
                        stage: "post_tool_use".to_string(),
                        decision,
                        tool: "shell_command".to_string(),
                        command_label: planned.label.clone(),
                        raw_command: planned.raw_command.clone(),
                        source: planned.source.clone(),
                        rationale: planned.rationale.clone(),
                        reasons,
                        passed: Some(false),
                        exit_code: None,
                        log_artifact: Some(log_artifact),
                        created_at: Utc::now().to_rfc3339(),
                    });
                    continue;
                }

                let outcome = self
                    .run_shell_command_capture(&planned.raw_command, staged_product)
                    .await?;
                let log_body = format!(
                    "# {}

- Source: {}
- Rationale: {}
- Command: {}
- Exit code: {}
- Passed: {}

## stdout
{}

## stderr
{}
",
                    planned.label,
                    planned.source,
                    planned.rationale,
                    planned.raw_command,
                    outcome
                        .code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string()),
                    if outcome.success { "true" } else { "false" },
                    if outcome.stdout.is_empty() {
                        "<empty>"
                    } else {
                        &outcome.stdout
                    },
                    if outcome.stderr.is_empty() {
                        "<empty>"
                    } else {
                        &outcome.stderr
                    },
                );
                tokio::fs::write(run_dir.join(&log_artifact), log_body).await?;
                if !outcome.success {
                    all_passed = false;
                }
                if was_preferred {
                    if outcome.success {
                        push_unique(&mut preferred_commands_helped, &planned.raw_command);
                    } else {
                        push_unique(&mut preferred_commands_stale, &planned.raw_command);
                    }
                }
                returned_artifacts.push(log_artifact.clone());
                executed_commands.push(RetrieverCommandExecution {
                    label: planned.label.clone(),
                    raw_command: planned.raw_command.clone(),
                    source: planned.source.clone(),
                    rationale: planned.rationale.clone(),
                    was_preferred,
                    preference_rank,
                    preference_outcome: Some(if was_preferred {
                        if outcome.success {
                            "helped".to_string()
                        } else {
                            "did_not_help".to_string()
                        }
                    } else {
                        "not_preferred".to_string()
                    }),
                    passed: outcome.success,
                    exit_code: outcome.code,
                    stdout: outcome.stdout.clone(),
                    stderr: outcome.stderr.clone(),
                    log_artifact: log_artifact.clone(),
                });
                hook_records.push(RetrieverHookRecord {
                    stage: "post_tool_use".to_string(),
                    decision: if outcome.success {
                        "allow"
                    } else {
                        "allow_with_failure"
                    }
                    .to_string(),
                    tool: "shell_command".to_string(),
                    command_label: planned.label.clone(),
                    raw_command: planned.raw_command.clone(),
                    source: planned.source.clone(),
                    rationale: planned.rationale.clone(),
                    reasons: if outcome.success {
                        vec!["Command completed inside the bounded staged workspace.".to_string()]
                    } else {
                        vec!["Command was allowed but returned a failing exit code.".to_string()]
                    },
                    passed: Some(outcome.success),
                    exit_code: outcome.code,
                    log_artifact: Some(log_artifact),
                    created_at: Utc::now().to_rfc3339(),
                });
            }
        }

        if executed_commands.is_empty() {
            all_passed = false;
        }

        let hook_artifact = RetrieverHookArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            adapter: packet.adapter.clone(),
            profile: packet.profile.clone(),
            generated_at: Utc::now().to_rfc3339(),
            records: hook_records,
        };
        self.write_json_file(&run_dir.join(&hook_artifact_name), &hook_artifact)
            .await?;
        tokio::fs::write(
            run_dir.join("retriever_forge_hooks.md"),
            render_retriever_hooks_markdown(&hook_artifact),
        )
        .await?;

        let preferred_summary = if packet.preferred_commands.is_empty() {
            "No prior preferred command paths were offered.".to_string()
        } else {
            format!(
                "Preferred offered={}, selected={}, helped={}, stale={}",
                packet.preferred_commands.len(),
                preferred_commands_selected.len(),
                preferred_commands_helped.len(),
                preferred_commands_stale.len()
            )
        };
        let summary = if let Some(summary) = drift_failure_summary {
            summary
        } else if executed_commands.is_empty() {
            format!(
                "Retriever forge found no runnable command plan for '{}' and returned no visible execution evidence. {}",
                target_source.label,
                preferred_summary
            )
        } else if all_passed {
            format!(
                "Retriever forge completed {} command(s) for '{}' and returned normalized visible execution evidence with hook records. {}",
                executed_commands.len(),
                target_source.label,
                preferred_summary
            )
        } else {
            format!(
                "Retriever forge hit visible execution failures for '{}' ({} command(s), {} failed or denied). {}",
                target_source.label,
                executed_commands.len(),
                executed_commands.iter().filter(|command| !command.passed).count(),
                preferred_summary
            )
        };

        let artifact = RetrieverExecutionArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            adapter: packet.adapter.clone(),
            profile: packet.profile.clone(),
            generated_at: Utc::now().to_rfc3339(),
            task_packet_artifact: "retriever_task_packet.json".to_string(),
            review_chain_artifact: "trail_review_chain.json".to_string(),
            dispatch_artifact: "retriever_dispatch.json".to_string(),
            continuity_artifact: continuity_file.clone(),
            hook_artifact: hook_artifact_name.clone(),
            passed: all_passed,
            summary: summary.clone(),
            preferred_commands_offered: packet.preferred_commands.clone(),
            preferred_commands_selected,
            preferred_commands_helped,
            preferred_commands_stale,
            executed_commands,
            returned_artifacts: returned_artifacts.clone(),
        };
        self.write_json_file(&run_dir.join("retriever_execution_report.json"), &artifact)
            .await?;
        tokio::fs::write(
            run_dir.join("retriever_execution_report.md"),
            render_retriever_execution_markdown(&artifact),
        )
        .await?;

        let trail_path = run_dir.join(&continuity_file);
        let mut trail_state = if trail_path.exists() {
            let raw = tokio::fs::read_to_string(&trail_path).await?;
            serde_json::from_str::<TrailStateArtifact>(&raw).unwrap_or(TrailStateArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                adapter: packet.adapter.clone(),
                profile: packet.profile.clone(),
                updated_at: Utc::now().to_rfc3339(),
                continuity_file: continuity_file.clone(),
                active_constraints: dispatch.constraints_applied.clone(),
                next_actions: dispatch.next_actions.clone(),
                visible_success_conditions: packet.visible_success_conditions.clone(),
                return_artifacts: packet.return_artifacts.clone(),
                last_execution_outcome: None,
                last_execution_summary: None,
                last_execution_artifact: None,
                executed_commands: Vec::new(),
                returned_artifacts_snapshot: Vec::new(),
            })
        } else {
            TrailStateArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                adapter: packet.adapter.clone(),
                profile: packet.profile.clone(),
                updated_at: Utc::now().to_rfc3339(),
                continuity_file: continuity_file.clone(),
                active_constraints: dispatch.constraints_applied.clone(),
                next_actions: dispatch.next_actions.clone(),
                visible_success_conditions: packet.visible_success_conditions.clone(),
                return_artifacts: packet.return_artifacts.clone(),
                last_execution_outcome: None,
                last_execution_summary: None,
                last_execution_artifact: None,
                executed_commands: Vec::new(),
                returned_artifacts_snapshot: Vec::new(),
            }
        };
        trail_state.updated_at = Utc::now().to_rfc3339();
        trail_state.last_execution_outcome = Some(
            if artifact.passed {
                "success"
            } else {
                "failure"
            }
            .to_string(),
        );
        trail_state.last_execution_summary = Some(summary);
        trail_state.last_execution_artifact = Some("retriever_execution_report.json".to_string());
        trail_state.executed_commands = artifact
            .executed_commands
            .iter()
            .map(|command| command.label.clone())
            .collect();
        trail_state.returned_artifacts_snapshot = artifact.returned_artifacts.clone();
        self.write_json_file(&trail_path, &trail_state).await?;

        Ok(artifact)
    }

    async fn run_shell_command_capture(
        &self,
        raw_command: &str,
        cwd: &Path,
    ) -> Result<CommandOutcome> {
        #[cfg(target_os = "windows")]
        let mut command = {
            let mut command = Command::new("cmd");
            command.arg("/C").arg(raw_command);
            command
        };

        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut command = Command::new("/bin/sh");
            command.arg("-lc").arg(raw_command);
            command
        };

        let output = command
            .current_dir(cwd)
            .output()
            .await
            .with_context(|| format!("running '{}' in {}", raw_command, cwd.display()))?;

        Ok(CommandOutcome {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
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
        let retriever_report_path = run_dir.join("retriever_execution_report.json");
        if retriever_report_path.exists() {
            if let Ok(raw) = tokio::fs::read_to_string(&retriever_report_path).await {
                if let Ok(report) = serde_json::from_str::<RetrieverExecutionArtifact>(&raw) {
                    results.push(ScenarioResult {
                        scenario_id: "retriever_forge_execution".to_string(),
                        passed: report.passed,
                        details: format!(
                            "{} ({} command(s), artifact={})",
                            report.summary,
                            report.executed_commands.len(),
                            retriever_report_path.display()
                        ),
                    });
                }
            }
        }

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
                            .with_context(|| {
                                format!("writing validation log {}", validation_log_path.display())
                            })?;
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
                        details:
                            "package.json found but no supported Node package manager is available"
                                .to_string(),
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
                        details:
                            "package.json found but no supported Node package manager is available"
                                .to_string(),
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
                        self.run_command_capture("pytest", &["-q"], staged_product)
                            .await?
                    } else {
                        self.run_command_capture(
                            python_command,
                            &["-m", "pytest", "-q"],
                            staged_product,
                        )
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
                        .run_command_capture(
                            python_command,
                            &["-m", "compileall", "."],
                            staged_product,
                        )
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
                .with_context(|| {
                    format!("writing validation log {}", validation_log_path.display())
                })?;
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

    fn agent_prompt_support(
        &self,
        agent_name: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
    ) -> Option<AgentPromptSupport> {
        let profiles =
            agents::load_profiles(&self.paths.factory.join("agents").join("profiles")).ok()?;
        let profile = profiles.get(agent_name)?;
        let resolved_provider = self
            .paths
            .setup
            .resolve_agent_provider_name(&profile.name, &profile.provider);
        let resolved_config = self
            .paths
            .setup
            .resolve_agent_provider(&profile.name, &profile.provider);
        let resolved_model = profile
            .model_override
            .clone()
            .or_else(|| resolved_config.map(|provider| provider.model.clone()));
        let resolved_surface = resolved_config.and_then(|provider| provider.surface.clone());
        let shared_personality_raw = std::fs::read_to_string(
            self.paths
                .factory
                .join("agents")
                .join("personality")
                .join("labrador.md"),
        )
        .unwrap_or_default();
        let shared_personality = if shared_personality_raw.trim().is_empty() {
            "- loyal to the mission
- honest when uncertain
- non-destructive and boundary-aware"
                .to_string()
        } else {
            shared_personality_raw.trim().to_string()
        };
        let personality_addendum = if profile.personality_file.ends_with("labrador.md") {
            None
        } else {
            let personality_path = self
                .paths
                .factory
                .join("agents")
                .join("profiles")
                .join(&profile.personality_file);
            std::fs::read_to_string(&personality_path)
                .ok()
                .map(|raw| raw.trim().to_string())
                .filter(|raw| !raw.is_empty())
        };
        let personality_addendum_block = personality_addendum
            .as_ref()
            .map(|raw| {
                format!(
                    "Agent-specific personality addendum:
{}",
                    raw
                )
            })
            .unwrap_or_default();
        let curated_skill_bundle = std::fs::read_to_string(
            self.paths
                .factory
                .join("agents")
                .join("skills")
                .join(format!("{}.md", agent_name)),
        )
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| {
            "No factory-curated skill bundle is installed for this Labrador yet.".to_string()
        });
        let pinned_external_skills =
            self.load_pinned_skill_excerpts(agent_name, &resolved_provider);
        let query_terms = build_coobie_query_terms(spec_obj, target_source);
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        let (context_entries, skill_entries) = discover_repo_local_context_entries(
            &harkonnen_dir,
            Some(target_source),
            Some(spec_obj),
            &query_terms,
        )
        .unwrap_or_default();
        let pinned_external_skill_block =
            format_resolved_pinned_skill_excerpts(&pinned_external_skills, &resolved_provider);
        let system_instruction = format!(
            "You are {display_name}, the Harkonnen Labrador '{agent_name}'.

Factory profile:
- role: {role}
- provider route: {provider}
- responsibilities:
{responsibilities}
- allowed tools:
{allowed_tools}
- disallowed tools:
{disallowed_tools}

Shared Labrador personality:
{shared_personality}

{personality_addendum}

Factory-curated local skill bundle:
{curated_skill_bundle}

Pinned external skill excerpts:
{pinned_external_skills}",
            display_name = profile.display_name,
            agent_name = profile.name,
            role = profile.role,
            provider = resolved_provider,
            responsibilities = render_list(
                &profile.responsibilities,
                "No explicit responsibilities recorded."
            ),
            allowed_tools = render_list(&profile.allowed_tools, "No allowed tools recorded."),
            disallowed_tools =
                render_list(&profile.disallowed_tools, "No disallowed tools recorded."),
            shared_personality = shared_personality,
            personality_addendum = personality_addendum_block,
            curated_skill_bundle = curated_skill_bundle,
            pinned_external_skills = pinned_external_skill_block,
        );
        let repo_context_block = format!(
            "REPO-LOCAL CONTEXT:
{}

REPO-LOCAL SKILL BUNDLES:
{}",
            render_repo_local_prompt_lines(
                &context_entries,
                "No repo-local context files discovered."
            ),
            render_repo_local_prompt_lines(
                &skill_entries,
                "No repo-local skill bundles discovered."
            ),
        );
        let mut bundle = AgentPromptBundleArtifact {
            agent_name: profile.name.clone(),
            display_name: profile.display_name.clone(),
            role: profile.role.clone(),
            resolved_provider,
            resolved_model,
            resolved_surface,
            fingerprint: String::new(),
            shared_personality,
            personality_addendum,
            curated_skill_bundle,
            pinned_skill_ids: pinned_external_skills
                .iter()
                .map(|entry| entry.id.clone())
                .collect(),
            pinned_external_skills,
            repo_local_context_entries: context_entries,
            repo_local_skill_entries: skill_entries,
            system_instruction: system_instruction.clone(),
            repo_context_block: repo_context_block.clone(),
        };
        bundle.fingerprint = fingerprint_agent_prompt_bundle(&bundle);
        Some(AgentPromptSupport {
            system_instruction,
            repo_context_block,
            bundle,
        })
    }

    fn load_pinned_skill_excerpts(
        &self,
        agent_name: &str,
        resolved_provider: &str,
    ) -> Vec<ResolvedPinnedSkillExcerpt> {
        let manifest_path = self.paths.factory.join("agents").join("pinned-skills.yaml");
        let Ok(raw) = std::fs::read_to_string(&manifest_path) else {
            return Vec::new();
        };
        let Ok(manifest) = serde_yaml::from_str::<PinnedSkillManifest>(&raw) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for entry in manifest.skills.iter().filter(|entry| {
            entry.agents.iter().any(|name| name == agent_name)
                && pinned_skill_matches_provider_route(&entry.source, resolved_provider)
        }) {
            let skill_path = self.paths.root.join(&entry.vendor_path).join("SKILL.md");
            let raw_skill = match std::fs::read_to_string(&skill_path) {
                Ok(raw_skill) => raw_skill,
                Err(_) => continue,
            };
            let source_meta = manifest.sources.get(&entry.source);
            let source_line = source_meta
                .map(|meta| format!("{} @ {}", meta.repo, meta.commit))
                .unwrap_or_else(|| entry.source.clone());
            out.push(ResolvedPinnedSkillExcerpt {
                id: entry.id.clone(),
                source: source_line,
                provider_family: entry.source.clone(),
                vendor_path: entry.vendor_path.clone(),
                rationale: if entry.rationale.trim().is_empty() {
                    "none recorded".to_string()
                } else {
                    entry.rationale.trim().to_string()
                },
                excerpt: summarize_pinned_skill_markdown(&raw_skill, 2200),
            });
        }
        out
    }

    async fn write_agent_execution(
        &self,
        profiles: &HashMap<String, AgentProfile>,
        agent_name: &str,
        prompt: &str,
        summary: &str,
        output: &str,
        phase: &str,
        episode_id: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        run_dir: &Path,
        agent_executions: &mut Vec<AgentExecution>,
    ) -> Result<()> {
        let profile = profiles
            .get(agent_name)
            .with_context(|| format!("agent profile not found: {agent_name}"))?;
        let prompt_support = self.agent_prompt_support(agent_name, spec_obj, target_source);
        let agents_dir = run_dir.join("agents");
        tokio::fs::create_dir_all(&agents_dir).await?;

        let mut execution =
            agents::build_execution(profile, &self.paths.setup, prompt, summary, output);
        execution.phase = Some(phase.to_string());
        execution.episode_id = Some(episode_id.to_string());
        if let Some(prompt_support) = prompt_support.as_ref() {
            let bundle_json_name = format!("{}_prompt_bundle.json", agent_name);
            let bundle_md_name = format!("{}_prompt_bundle.md", agent_name);
            self.write_json_file(&agents_dir.join(&bundle_json_name), &prompt_support.bundle)
                .await?;
            tokio::fs::write(
                agents_dir.join(&bundle_md_name),
                render_prompt_bundle_markdown(&prompt_support.bundle),
            )
            .await?;
            execution.prompt_bundle_fingerprint = Some(prompt_support.bundle.fingerprint.clone());
            execution.prompt_bundle_artifact = Some(format!("agents/{}", bundle_json_name));
            execution.prompt_bundle_provider =
                Some(prompt_support.bundle.resolved_provider.clone());
            execution.pinned_skill_ids = prompt_support.bundle.pinned_skill_ids.clone();
        }

        self.write_json_file(&agents_dir.join(format!("{agent_name}.json")), &execution)
            .await?;

        agent_executions.retain(|existing| existing.episode_id.as_deref() != Some(episode_id));
        agent_executions.push(execution);
        agent_executions.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.agent_name.cmp(&right.agent_name))
        });
        self.write_json_file(&agents_dir.join("index.json"), agent_executions)
            .await?;
        self.write_json_file(&run_dir.join("agent_executions.json"), agent_executions)
            .await?;
        tokio::fs::write(
            run_dir.join("agent_response_log.md"),
            render_agent_response_log(agent_executions),
        )
        .await?;
        Ok(())
    }

    /// Bramble: interpret validation output with an LLM when available.
    /// Returns `None` if no LLM is configured (caller uses command output alone).
    async fn bramble_interpret_validation(
        &self,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        validation: &ValidationSummary,
        briefing: &CoobieBriefing,
    ) -> Option<String> {
        let provider = llm::build_provider("bramble", "default", &self.paths.setup)?;

        let results_text = validation
            .results
            .iter()
            .map(|r| {
                format!(
                    "- [{}] {}: {}",
                    if r.passed { "PASS" } else { "FAIL" },
                    r.scenario_id,
                    r.details
                )
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            );

        let prompt_support = self.agent_prompt_support("bramble", spec_obj, target_source);
        let system_instruction = prompt_support
            .as_ref()
            .map(|support| format!(
                "{}

Task contract:
You are Bramble, a validation analyst for a software factory. You receive a spec summary and the results of visible validation checks. Produce a brief Markdown analysis describing what passed, what failed, likely root causes for failures, and what a developer should inspect first. No filler.",
                support.system_instruction
            ))
            .unwrap_or_else(|| "You are Bramble, a validation analyst for a software factory. You receive a spec summary and the results of visible validation checks. Produce a brief Markdown analysis describing what passed, what failed, likely root causes for failures, and what a developer should inspect first. No filler.".to_string());
        let repo_context_block = prompt_support
            .as_ref()
            .map(|support| support.repo_context_block.as_str())
            .unwrap_or(
                "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
            );

        let req = LlmRequest::simple(
            system_instruction,
            format!(
                "SPEC: {} — {}
TARGET: {} ({})
COOBIE REQUIRED CHECKS: {}
COOBIE GUARDRAILS: {}

VALIDATION RESULTS (passed={}):
{results_text}

{repo_context_block}

Produce the validation analysis and note any checks Coobie asked for that are still unproven or contradicted by repo-local guidance.",
                spec_obj.id,
                spec_obj.title,
                target_source.label,
                target_source.source_path,
                if briefing.required_checks.is_empty() { "none".to_string() } else { briefing.required_checks.join(" | ") },
                if briefing.recommended_guardrails.is_empty() { "none".to_string() } else { briefing.recommended_guardrails.join(" | ") },
                validation.passed,
                repo_context_block = repo_context_block,
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
        target_source: &TargetSourceMetadata,
        twin: &TwinEnvironment,
        briefing: &CoobieBriefing,
    ) -> Option<String> {
        let provider = llm::build_provider("ash", "default", &self.paths.setup)?;
        let prompt_support = self.agent_prompt_support("ash", spec_obj, target_source);
        let ash_addendum = std::fs::read_to_string(
            self.paths
                .factory
                .join("agents")
                .join("personality")
                .join("ash.md"),
        )
        .unwrap_or_default();

        let services = twin
            .services
            .iter()
            .map(|s| {
                format!(
                    "- {} [{}] status={} — {}",
                    s.name, s.kind, s.status, s.details
                )
            })
            .collect::<Vec<_>>()
            .join(
                "
",
            );

        let system_instruction = prompt_support
            .as_ref()
            .map(|support| format!(
                "{}

Task contract:
You are Ash, a digital twin specialist for a software factory. You have just provisioned a local twin environment for a run. Produce a brief Markdown narrative explaining what was provisioned, what each service provides to this run, and any gaps or warnings relevant to the spec. Two to four short paragraphs. No filler.",
                support.system_instruction
            ))
            .unwrap_or_else(|| "You are Ash, a digital twin specialist for a software factory. You have just provisioned a local twin environment for a run. Produce a brief Markdown narrative explaining what was provisioned, what each service provides to this run, and any gaps or warnings relevant to the spec. Two to four short paragraphs. No filler.".to_string());
        let repo_context_block = prompt_support
            .as_ref()
            .map(|support| support.repo_context_block.as_str())
            .unwrap_or(
                "REPO-LOCAL CONTEXT:
- No repo-local context guidance was loaded.

REPO-LOCAL SKILL BUNDLES:
- No repo-local skill bundles were loaded.",
            );

        let req = LlmRequest::simple(
            system_instruction,
            format!(
                "ASH ADDENDUM:
{}

SPEC: {} — {}
TARGET: {} ({})
DEPENDENCIES: {}
COOBIE ENVIRONMENT RISKS: {}
COOBIE REQUIRED CHECKS: {}

TWIN SERVICES:
{services}

{repo_context_block}

Write the twin environment narrative and identify any simulation gaps against Coobie's environment risks. Be explicit about which twin facts came from Harkonnen versus any product runtime assumptions.",
                if ash_addendum.trim().is_empty() { "none" } else { ash_addendum.trim() },
                spec_obj.id,
                spec_obj.title,
                target_source.label,
                target_source.source_path,
                if spec_obj.dependencies.is_empty() { "none".to_string() } else { spec_obj.dependencies.join(", ") },
                if briefing.environment_risks.is_empty() { "none".to_string() } else { briefing.environment_risks.join(" | ") },
                if briefing.required_checks.is_empty() { "none".to_string() } else { briefing.required_checks.join(" | ") },
                repo_context_block = repo_context_block,
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
            lines.push(format!(
                "- {} available={}",
                command,
                command_available(command)
            ));
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
            bail!(
                "target source is not a directory: {}",
                source_path.display()
            );
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

    async fn resolve_memory_ingest_target(
        &self,
        project_root: &str,
    ) -> Result<TargetSourceMetadata> {
        let candidate = PathBuf::from(project_root);
        let source_path = if candidate.is_absolute() {
            candidate
        } else {
            self.paths.root.join(candidate)
        };

        if !source_path.exists() {
            bail!("project memory root not found: {}", source_path.display());
        }
        if !source_path.is_dir() {
            bail!(
                "project memory root is not a directory: {}",
                source_path.display()
            );
        }

        let canonical = source_path.canonicalize()?;
        let label = canonical
            .file_name()
            .and_then(|value| value.to_str())
            .map(|value| value.to_string())
            .unwrap_or_else(|| canonical.display().to_string());
        let git = self.capture_git_metadata(&canonical).await?;

        Ok(TargetSourceMetadata {
            label,
            source_kind: "path".to_string(),
            source_path: canonical.display().to_string(),
            git,
        })
    }

    async fn project_evidence_bundle_path(
        &self,
        target_source: &TargetSourceMetadata,
        bundle_name: &str,
    ) -> Result<PathBuf> {
        let filename = normalize_evidence_bundle_name(bundle_name)?;
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        Ok(harkonnen_dir
            .join("evidence")
            .join("annotations")
            .join(filename))
    }

    async fn project_evidence_history_path(
        &self,
        target_source: &TargetSourceMetadata,
        bundle_name: &str,
    ) -> Result<PathBuf> {
        let filename = normalize_evidence_bundle_name(bundle_name)?;
        let history_name = format!("{}.history.jsonl", filename);
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        self.ensure_project_evidence_bootstrap(&harkonnen_dir)
            .await?;
        Ok(harkonnen_dir
            .join("evidence")
            .join("history")
            .join(history_name))
    }

    async fn append_project_evidence_history_events(
        &self,
        target_source: &TargetSourceMetadata,
        bundle_name: &str,
        events: &[EvidenceAnnotationHistoryEvent],
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let path = self
            .project_evidence_history_path(target_source, bundle_name)
            .await?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        for event in events {
            let line = serde_json::to_string(event)?;
            file.write_all(line.as_bytes()).await?;
            file.write_all(b"\n").await?;
        }
        file.flush().await?;
        Ok(())
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
        let status_outcome = self
            .run_command_capture("git", &["status", "--porcelain"], source)
            .await
            .ok()
            .filter(|outcome| outcome.success);
        let clean = status_outcome
            .as_ref()
            .map(|outcome| outcome.stdout.trim().is_empty());
        let changed_paths = status_outcome
            .as_ref()
            .map(|outcome| parse_git_status_paths(&outcome.stdout))
            .unwrap_or_default();

        Ok(Some(TargetGitMetadata {
            branch,
            commit,
            remote_origin,
            clean,
            changed_paths,
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

    async fn record_phase_attribution(
        &self,
        run_id: &str,
        episode_id: &str,
        phase: &str,
        agent_name: &str,
        outcome: &str,
        confidence: Option<f64>,
        memory_context: &MemoryContextBundle,
        briefing: &CoobieBriefing,
        agent_executions: &[AgentExecution],
        phase_attributions: &mut Vec<PhaseAttributionRecord>,
        run_dir: &Path,
    ) -> Result<()> {
        let execution = agent_executions
            .iter()
            .find(|execution| execution.episode_id.as_deref() == Some(episode_id));
        let created_at = Utc::now();
        let record = PhaseAttributionRecord {
            attribution_id: format!("phase-attribution-{}", episode_id),
            run_id: run_id.to_string(),
            episode_id: episode_id.to_string(),
            phase: phase.to_string(),
            agent_name: agent_name.to_string(),
            outcome: outcome.to_string(),
            confidence,
            prompt_bundle_fingerprint: execution
                .and_then(|execution| execution.prompt_bundle_fingerprint.clone()),
            prompt_bundle_provider: execution
                .and_then(|execution| execution.prompt_bundle_provider.clone()),
            prompt_bundle_artifact: execution
                .and_then(|execution| execution.prompt_bundle_artifact.clone()),
            pinned_skill_ids: execution
                .map(|execution| execution.pinned_skill_ids.clone())
                .unwrap_or_default(),
            memory_hits: memory_context.memory_hits.clone(),
            core_memory_ids: memory_context.core_memory_ids.clone(),
            project_memory_ids: memory_context.project_memory_ids.clone(),
            relevant_lesson_ids: briefing
                .relevant_lessons
                .iter()
                .map(|lesson| lesson.lesson_id.clone())
                .collect(),
            required_checks: briefing.required_checks.clone(),
            guardrails: briefing.recommended_guardrails.clone(),
            query_terms: briefing.query_terms.clone(),
            created_at,
        };
        self.upsert_phase_attribution(&record).await?;

        phase_attributions.retain(|existing| existing.episode_id != episode_id);
        phase_attributions.push(record);
        phase_attributions.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.phase.cmp(&right.phase))
        });
        self.write_json_file(&run_dir.join("phase_attributions.json"), phase_attributions)
            .await?;
        tokio::fs::write(
            run_dir.join("phase_attributions.md"),
            render_phase_attributions_markdown(phase_attributions),
        )
        .await?;
        Ok(())
    }

    async fn upsert_phase_attribution(&self, record: &PhaseAttributionRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO phase_attributions (
                attribution_id,
                run_id,
                episode_id,
                phase,
                agent_name,
                outcome,
                confidence,
                prompt_bundle_fingerprint,
                prompt_bundle_provider,
                prompt_bundle_artifact,
                pinned_skill_ids,
                memory_hits,
                core_memory_ids,
                project_memory_ids,
                relevant_lesson_ids,
                required_checks,
                guardrails,
                query_terms,
                created_at
            )
            VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19
            )
            ON CONFLICT(episode_id) DO UPDATE SET
                attribution_id = excluded.attribution_id,
                run_id = excluded.run_id,
                phase = excluded.phase,
                agent_name = excluded.agent_name,
                outcome = excluded.outcome,
                confidence = excluded.confidence,
                prompt_bundle_fingerprint = excluded.prompt_bundle_fingerprint,
                prompt_bundle_provider = excluded.prompt_bundle_provider,
                prompt_bundle_artifact = excluded.prompt_bundle_artifact,
                pinned_skill_ids = excluded.pinned_skill_ids,
                memory_hits = excluded.memory_hits,
                core_memory_ids = excluded.core_memory_ids,
                project_memory_ids = excluded.project_memory_ids,
                relevant_lesson_ids = excluded.relevant_lesson_ids,
                required_checks = excluded.required_checks,
                guardrails = excluded.guardrails,
                query_terms = excluded.query_terms,
                created_at = excluded.created_at
            "#,
        )
        .bind(&record.attribution_id)
        .bind(&record.run_id)
        .bind(&record.episode_id)
        .bind(&record.phase)
        .bind(&record.agent_name)
        .bind(&record.outcome)
        .bind(record.confidence)
        .bind(record.prompt_bundle_fingerprint.clone())
        .bind(record.prompt_bundle_provider.clone())
        .bind(record.prompt_bundle_artifact.clone())
        .bind(serde_json::to_string(&record.pinned_skill_ids)?)
        .bind(serde_json::to_string(&record.memory_hits)?)
        .bind(serde_json::to_string(&record.core_memory_ids)?)
        .bind(serde_json::to_string(&record.project_memory_ids)?)
        .bind(serde_json::to_string(&record.relevant_lesson_ids)?)
        .bind(serde_json::to_string(&record.required_checks)?)
        .bind(serde_json::to_string(&record.guardrails)?)
        .bind(serde_json::to_string(&record.query_terms)?)
        .bind(record.created_at.to_rfc3339())
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

    /// Build and write `exploration_log.md` to the run directory.
    ///
    /// One entry per episode, using the Residue five-field format:
    /// strategy / outcome / failure_constraint / surviving_structure / reformulation.
    /// Coobie reads this during consolidation for lesson extraction.
    async fn write_exploration_log(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        run_dir: &Path,
    ) -> Result<()> {
        let episodes = self.list_run_episodes(run_id).await?;
        let mut lines = Vec::new();
        let mut entries = Vec::new();

        lines.push("# Exploration Log".to_string());
        lines.push(format!("run: {run_id}"));
        lines.push(format!("spec: {} - {}", spec_obj.id, spec_obj.title));
        lines.push(format!("product: {}", target_source.label));
        lines.push(format!("generated: {}", Utc::now().to_rfc3339()));
        lines.push(String::new());

        for (i, episode) in episodes.iter().enumerate() {
            let events = self
                .list_events_for_episode(&episode.episode_id)
                .await
                .unwrap_or_default();

            let outcome = episode.outcome.as_deref().unwrap_or("unknown").to_string();
            let confidence = episode
                .confidence
                .map(|c| format!("{:.2}", c))
                .unwrap_or_else(|| "unknown".to_string());
            let agent = events
                .first()
                .map(|event| event.agent.clone())
                .unwrap_or_else(|| "unknown".to_string());
            let strategy = events
                .first()
                .map(|event| format!("{} - {}", event.agent, event.message))
                .unwrap_or_else(|| format!("{} phase", episode.phase));
            let failure_constraint = if matches!(outcome.as_str(), "failure" | "blocked") {
                events
                    .iter()
                    .rev()
                    .find(|event| event.status != "running")
                    .map(|event| event.message.clone())
                    .unwrap_or_else(|| "no constraint recorded".to_string())
            } else {
                "none".to_string()
            };
            let surviving_structure = events
                .iter()
                .rev()
                .find(|event| event.status == "complete")
                .map(|event| event.message.clone())
                .unwrap_or_else(|| "none".to_string());
            let reformulation = match outcome.as_str() {
                "success" => format!(
                    "{} phase completed with confidence {confidence}",
                    episode.phase
                ),
                "failure" | "blocked" => {
                    format!("{} phase failed; preserve surviving structure and change strategy on retry", episode.phase)
                }
                _ => format!("{} phase outcome: {outcome}", episode.phase),
            };
            let artifacts = phase_artifact_hints(&episode.phase);
            let parameters = vec![
                format!("confidence={confidence}"),
                format!("event_count={}", events.len()),
            ];
            let open_questions = if matches!(outcome.as_str(), "failure" | "blocked") {
                vec![format!(
                    "What changed would let the {} phase succeed without repeating '{}' ?",
                    episode.phase, failure_constraint
                )]
            } else {
                Vec::new()
            };

            let entry = ExplorationEntry {
                phase: episode.phase.clone(),
                episode_id: episode.episode_id.clone(),
                agent,
                strategy,
                outcome,
                failure_constraint,
                surviving_structure,
                reformulation,
                artifacts,
                parameters,
                open_questions,
            };

            lines.push(format!("## Exploration {}", i + 1));
            lines.push("```yaml".to_string());
            lines.push(format!("phase: {}", entry.phase));
            lines.push(format!("episode: {}", entry.episode_id));
            lines.push(format!("agent: {}", entry.agent));
            lines.push(format!("strategy: {}", entry.strategy));
            lines.push(format!("outcome: {}", entry.outcome));
            lines.push(format!("failure_constraint: {}", entry.failure_constraint));
            lines.push(format!(
                "surviving_structure: {}",
                entry.surviving_structure
            ));
            lines.push(format!("reformulation: {}", entry.reformulation));
            lines.push(format!("artifacts: {}", format_yaml_list(&entry.artifacts)));
            lines.push(format!(
                "parameters: {}",
                format_yaml_list(&entry.parameters)
            ));
            lines.push(format!(
                "open_questions: {}",
                format_yaml_list(&entry.open_questions)
            ));
            lines.push("```".to_string());
            lines.push(String::new());

            entries.push(entry);
        }

        let passed = entries
            .iter()
            .filter(|entry| entry.outcome == "success")
            .count();
        let failed = entries
            .iter()
            .filter(|entry| matches!(entry.outcome.as_str(), "failure" | "blocked"))
            .count();
        lines.push("## Summary".to_string());
        lines.push(format!("total_explorations: {}", entries.len()));
        lines.push(format!("passed: {passed}"));
        lines.push(format!("failed: {failed}"));
        lines.push(format!(
            "spec_tags: {}",
            std::iter::once(spec_obj.id.as_str())
                .chain(spec_obj.scope.iter().map(|scope| scope.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        ));
        lines.push(String::new());

        tokio::fs::write(run_dir.join("exploration_log.md"), lines.join("\n"))
            .await
            .context("writing exploration_log.md")?;
        self.write_json_file(
            &run_dir.join("exploration_log.json"),
            &ExplorationLogArtifact {
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                generated_at: Utc::now().to_rfc3339(),
                entries: entries.clone(),
            },
        )
        .await?;
        self.update_dead_end_registry(run_id, spec_obj, target_source, &entries, run_dir)
            .await?;
        Ok(())
    }

    async fn update_dead_end_registry(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        entries: &[ExplorationEntry],
        run_dir: &Path,
    ) -> Result<()> {
        let state_dir = self.paths.factory.join("state");
        tokio::fs::create_dir_all(&state_dir).await?;
        let registry_path = state_dir.join("dead_ends.json");
        let mut registry = if registry_path.exists() {
            let raw = tokio::fs::read_to_string(&registry_path).await?;
            serde_json::from_str::<DeadEndRegistry>(&raw).unwrap_or_default()
        } else {
            DeadEndRegistry::default()
        };

        for entry in entries {
            if !matches!(entry.outcome.as_str(), "failure" | "blocked") {
                continue;
            }
            let registry_id = format!("{}:{}", run_id, entry.episode_id);
            if registry
                .entries
                .iter()
                .any(|existing| existing.registry_id == registry_id)
            {
                continue;
            }
            registry.entries.push(DeadEndRegistryEntry {
                registry_id,
                run_id: run_id.to_string(),
                spec_id: spec_obj.id.clone(),
                product: target_source.label.clone(),
                phase: entry.phase.clone(),
                agent: entry.agent.clone(),
                strategy: entry.strategy.clone(),
                failure_constraint: entry.failure_constraint.clone(),
                surviving_structure: entry.surviving_structure.clone(),
                reformulation: entry.reformulation.clone(),
                created_at: Utc::now().to_rfc3339(),
            });
        }

        registry
            .entries
            .sort_by(|left, right| left.created_at.cmp(&right.created_at));
        self.write_json_file(&registry_path, &registry).await?;
        self.sync_project_strategy_register(target_source, &registry)
            .await?;
        let snapshot = registry
            .entries
            .iter()
            .filter(|entry| entry.run_id == run_id)
            .cloned()
            .collect::<Vec<_>>();
        self.write_json_file(&run_dir.join("dead_end_registry_snapshot.json"), &snapshot)
            .await?;
        Ok(())
    }

    async fn refresh_project_resume_packet(
        &self,
        target_source: &TargetSourceMetadata,
        store: &MemoryStore,
    ) -> Result<ProjectResumePacket> {
        let entries = store.list_entries().await?;
        let mut stale_memory = Vec::new();
        for entry in &entries {
            if let Some(risk) = self.build_resume_risk(entry, target_source).await? {
                stale_memory.push(risk);
            }
        }
        stale_memory.sort_by(|left, right| {
            right
                .severity_score
                .cmp(&left.severity_score)
                .then_with(|| left.memory_id.cmp(&right.memory_id))
        });
        let status_count = entries
            .iter()
            .filter(|entry| entry.provenance.status.is_some())
            .count();
        let mut summary = vec![format!(
            "Current git: {}",
            render_target_git_summary(target_source.git.as_ref())
        )];
        summary.push(format!("Project memory entries indexed: {}", entries.len()));
        summary.push(format!("Entries currently at risk: {}", stale_memory.len()));
        if status_count > 0 {
            summary.push(format!(
                "Entries already marked challenged/superseded: {}",
                status_count
            ));
        }
        if !stale_memory.is_empty() {
            let critical = stale_memory
                .iter()
                .filter(|risk| risk.severity == "critical")
                .count();
            let high = stale_memory
                .iter()
                .filter(|risk| risk.severity == "high")
                .count();
            let medium = stale_memory
                .iter()
                .filter(|risk| risk.severity == "medium")
                .count();
            let low = stale_memory
                .iter()
                .filter(|risk| risk.severity == "low")
                .count();
            summary.push(format!(
                "Risk mix: critical={} high={} medium={} low={}",
                critical, high, medium, low
            ));
        }
        if let Some(git) = target_source.git.as_ref() {
            if !git.changed_paths.is_empty() {
                summary.push(format!(
                    "Working tree changed paths: {}",
                    git.changed_paths
                        .iter()
                        .take(8)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            if git.clean.is_some_and(|clean| !clean) {
                summary.push("Working tree is dirty; provenance checks may need fresh review before trusting older lessons.".to_string());
            }
        }

        let packet = ProjectResumePacket {
            generated_at: Utc::now().to_rfc3339(),
            label: target_source.label.clone(),
            current_git: target_source.git.clone(),
            summary,
            stale_memory,
        };

        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        self.write_json_file(&harkonnen_dir.join("resume-packet.json"), &packet)
            .await?;
        tokio::fs::write(
            harkonnen_dir.join("resume-packet.md"),
            render_project_resume_packet_markdown(&packet),
        )
        .await?;
        Ok(packet)
    }

    async fn build_resume_risk(
        &self,
        entry: &MemoryEntry,
        target_source: &TargetSourceMetadata,
    ) -> Result<Option<ProjectResumeRisk>> {
        let mut reasons = Vec::new();
        let mut affected_paths = Vec::new();
        let mut severity_score = 0;
        let current_git = target_source.git.as_ref();
        let observed_paths = entry
            .provenance
            .observed_paths
            .iter()
            .map(|path| normalize_project_path(path))
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        let code_under_test_paths = entry
            .provenance
            .code_under_test_paths
            .iter()
            .map(|path| normalize_project_path(path))
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();

        if let (Some(stored), Some(current)) = (
            entry.provenance.git_commit.as_deref(),
            current_git.and_then(|git| git.commit.as_deref()),
        ) {
            if stored != current {
                if observed_paths.is_empty() {
                    reasons.push(format!(
                        "stored commit {} differs from current commit {}",
                        stored, current
                    ));
                    severity_score = severity_score.max(25);
                } else {
                    let changed = self
                        .git_paths_changed_since(
                            Path::new(&target_source.source_path),
                            stored,
                            current,
                            &observed_paths,
                        )
                        .await
                        .unwrap_or_default();
                    if !changed.is_empty() {
                        reasons.push(format!(
                            "recorded paths changed since memory commit: {}",
                            changed.join(", ")
                        ));
                        severity_score = severity_score.max(max_path_impact_score(&changed));
                        affected_paths.extend(changed);
                    } else {
                        severity_score = severity_score.max(15);
                    }
                }
            }
        }
        if let (Some(stored), Some(current)) = (
            entry.provenance.git_branch.as_deref(),
            current_git.and_then(|git| git.branch.as_deref()),
        ) {
            if stored != current {
                reasons.push(format!(
                    "stored branch {} differs from current branch {}",
                    stored, current
                ));
                severity_score = severity_score.max(30);
            }
        }
        if let Some(stored_path) = entry.provenance.source_path.as_deref() {
            if stored_path != target_source.source_path {
                reasons.push("memory was recorded against a different source path".to_string());
                severity_score = severity_score.max(40);
            }
        }
        if let Some(git) = current_git {
            let overlapping_dirty = intersect_project_paths(&git.changed_paths, &observed_paths);
            if !overlapping_dirty.is_empty() {
                reasons.push(format!(
                    "working tree changes overlap recorded paths: {}",
                    overlapping_dirty.join(", ")
                ));
                severity_score = severity_score.max(max_path_impact_score(&overlapping_dirty));
                affected_paths.extend(overlapping_dirty);
            } else if git.clean.is_some_and(|clean| !clean)
                && observed_paths.is_empty()
                && !entry.provenance.stale_when.is_empty()
            {
                reasons.push(format!(
                    "stale_when conditions: {}",
                    entry.provenance.stale_when.join(" | ")
                ));
                severity_score = severity_score.max(35);
            }
        }
        if let Some(status) = entry.provenance.status.as_deref() {
            reasons.push(format!("memory status is {}", status));
            severity_score = severity_score.max(status_severity_score(status));
        }
        if let Some(superseded_by) = entry.provenance.superseded_by.as_deref() {
            reasons.push(format!("superseded by {}", superseded_by));
            severity_score = severity_score.max(95);
        }
        if !entry.provenance.challenged_by.is_empty() {
            reasons.push(format!(
                "challenged by {}",
                entry.provenance.challenged_by.join(", ")
            ));
            severity_score = severity_score.max(70);
        }
        let affected_code_paths = intersect_project_paths(&affected_paths, &code_under_test_paths);
        if !affected_code_paths.is_empty() {
            reasons.push(format!(
                "explicit code_under_test paths changed: {}",
                affected_code_paths.join(", ")
            ));
            severity_score = severity_score.max(95);
        }
        if !entry.provenance.observed_surfaces.is_empty() && !affected_paths.is_empty() {
            reasons.push(format!(
                "memory covers surfaces: {}",
                entry.provenance.observed_surfaces.join(", ")
            ));
            severity_score = (severity_score + 10).min(100);
        }

        affected_paths.sort();
        affected_paths.dedup();
        if !affected_paths.is_empty() {
            reasons.push(format!("affected paths: {}", affected_paths.join(", ")));
        }

        if reasons.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ProjectResumeRisk {
                memory_id: entry.id.clone(),
                summary: entry.summary.clone(),
                status: entry.provenance.status.clone(),
                severity: resume_risk_severity(severity_score).to_string(),
                severity_score,
                reasons,
            }))
        }
    }

    async fn git_paths_changed_since(
        &self,
        repo_root: &Path,
        from_commit: &str,
        to_commit: &str,
        observed_paths: &[String],
    ) -> Result<Vec<String>> {
        if !command_available("git") || observed_paths.is_empty() {
            return Ok(Vec::new());
        }

        let mut command = Command::new("git");
        command.current_dir(repo_root);
        command.arg("diff");
        command.arg("--name-only");
        command.arg(format!("{}..{}", from_commit, to_commit));
        command.arg("--");
        for path in observed_paths {
            command.arg(path);
        }

        let output = command
            .output()
            .await
            .with_context(|| format!("running git diff in {}", repo_root.display()))?;
        if !output.status.success() {
            return Ok(Vec::new());
        }

        let mut changed = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(normalize_project_path)
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        changed.sort();
        changed.dedup();
        Ok(changed)
    }

    async fn sync_project_strategy_register(
        &self,
        target_source: &TargetSourceMetadata,
        registry: &DeadEndRegistry,
    ) -> Result<()> {
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        tokio::fs::create_dir_all(&harkonnen_dir).await?;
        let relevant = registry
            .entries
            .iter()
            .filter(|entry| entry.product == target_source.label)
            .cloned()
            .collect::<Vec<_>>();
        self.write_json_file(&harkonnen_dir.join("strategy-register.json"), &relevant)
            .await?;
        tokio::fs::write(
            harkonnen_dir.join("strategy-register.md"),
            render_project_strategy_register_markdown(target_source, &relevant),
        )
        .await?;
        Ok(())
    }

    async fn record_stale_memory_mitigation_outcomes(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        briefing: &CoobieBriefing,
        validation: &ValidationSummary,
        hidden_scenarios: &HiddenScenarioSummary,
        run_dir: &Path,
    ) -> Result<()> {
        let mut history = self
            .load_project_stale_memory_history(target_source)
            .await?;
        let previous_record = history.records.last().cloned();
        let mut previous_scores = HashMap::new();
        if let Some(record) = &previous_record {
            for entry in &record.entries {
                previous_scores.insert(entry.memory_id.clone(), entry.severity_score);
            }
        }

        let exploration_exists = run_dir.join("exploration_log.json").exists();
        let current_ids = briefing
            .resume_packet_risks
            .iter()
            .map(|risk| risk.memory_id.clone())
            .collect::<HashSet<_>>();
        let mut entries = Vec::new();
        for risk in &briefing.resume_packet_risks {
            let mitigation_steps = briefing
                .stale_memory_mitigation_plan
                .iter()
                .filter(|step| step.contains(&risk.memory_id))
                .cloned()
                .collect::<Vec<_>>();
            let related_checks = briefing
                .required_checks
                .iter()
                .filter(|check| check.contains(&risk.memory_id))
                .cloned()
                .collect::<Vec<_>>();
            let status = derive_stale_memory_mitigation_status(
                risk,
                validation,
                hidden_scenarios,
                exploration_exists,
            );
            let mut evidence = Vec::new();
            if exploration_exists {
                evidence.push("exploration_log.json present".to_string());
            }
            if validation.passed {
                evidence.push("visible validation passed".to_string());
            }
            if hidden_scenarios.passed {
                evidence.push("hidden scenarios passed".to_string());
            }
            if !mitigation_steps.is_empty() {
                evidence.push(format!(
                    "{} mitigation step(s) generated",
                    mitigation_steps.len()
                ));
            }
            if !related_checks.is_empty() {
                evidence.push(format!(
                    "{} mitigation check(s) generated",
                    related_checks.len()
                ));
            }
            let previous_severity_score = previous_scores.get(&risk.memory_id).copied();
            let risk_reduced_from_previous =
                previous_severity_score.map(|previous| risk.severity_score < previous);
            entries.push(StaleMemoryMitigationStatusEntry {
                memory_id: risk.memory_id.clone(),
                severity: risk.severity.clone(),
                severity_score: risk.severity_score,
                mitigation_steps,
                related_checks,
                status,
                evidence,
                previous_severity_score,
                risk_reduced_from_previous,
            });
        }

        let resolved_since_previous = previous_record
            .as_ref()
            .map(|record| {
                record
                    .entries
                    .iter()
                    .filter(|entry| !current_ids.contains(&entry.memory_id))
                    .map(|entry| {
                        format!(
                            "{} dropped from the stale-risk list after prior status {}",
                            entry.memory_id, entry.status
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let artifact = StaleMemoryMitigationStatusArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            generated_at: Utc::now().to_rfc3339(),
            entries,
            resolved_since_previous,
        };
        self.write_json_file(
            &run_dir.join("stale_memory_mitigation_status.json"),
            &artifact,
        )
        .await?;
        tokio::fs::write(
            run_dir.join("stale_memory_mitigation_status.md"),
            render_stale_memory_mitigation_status_markdown(&artifact),
        )
        .await?;

        history.records.push(artifact);
        if history.records.len() > 50 {
            let drain = history.records.len() - 50;
            history.records.drain(0..drain);
        }
        self.sync_project_stale_memory_history(target_source, &history)
            .await?;
        Ok(())
    }

    async fn sync_project_stale_memory_history(
        &self,
        target_source: &TargetSourceMetadata,
        history: &StaleMemoryMitigationHistory,
    ) -> Result<()> {
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        tokio::fs::create_dir_all(&harkonnen_dir).await?;
        self.write_json_file(&harkonnen_dir.join("stale-memory-history.json"), history)
            .await?;
        tokio::fs::write(
            harkonnen_dir.join("stale-memory-history.md"),
            render_stale_memory_history_markdown(target_source, history),
        )
        .await?;
        Ok(())
    }

    async fn write_retriever_dispatch_artifacts(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: &TargetSourceMetadata,
        worker_harness: &WorkerHarnessConfig,
        run_dir: &Path,
    ) -> Result<(RetrieverDispatchArtifact, TrailStateArtifact)> {
        let packet_raw =
            tokio::fs::read_to_string(run_dir.join("retriever_task_packet.json")).await?;
        let review_raw = tokio::fs::read_to_string(run_dir.join("trail_review_chain.json")).await?;
        let packet = serde_json::from_str::<WorkerTaskEnvelope>(&packet_raw)?;
        let review = serde_json::from_str::<PlanReviewChainArtifact>(&review_raw)?;

        let continuity_file = worker_harness
            .continuity_file
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "trail-state.json".to_string());
        let active_constraints = packet
            .denied_paths
            .iter()
            .take(4)
            .cloned()
            .chain(packet.required_checks.iter().take(6).cloned())
            .collect::<Vec<_>>();
        let next_actions = if review.final_execution_plan.is_empty() {
            vec!["No final execution plan steps were recorded yet.".to_string()]
        } else {
            review
                .final_execution_plan
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
        };

        let dispatch = RetrieverDispatchArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            adapter: packet.adapter.clone(),
            profile: packet.profile.clone(),
            generated_at: Utc::now().to_rfc3339(),
            task_packet_artifact: "retriever_task_packet.json".to_string(),
            review_chain_artifact: "trail_review_chain.json".to_string(),
            context_bundle_artifact: packet
                .context_bundle_artifact
                .clone()
                .unwrap_or_else(|| "retriever_context_bundle.json".to_string()),
            trail_drift_guard_artifact: packet
                .trail_drift_guard_artifact
                .clone()
                .unwrap_or_else(|| "trail_drift_guard.json".to_string()),
            continuity_artifact: continuity_file.clone(),
            dispatch_summary: format!(
                "Dispatch retriever forge '{}' for '{}' with {} allowed path(s) and {} visible success condition(s).",
                packet.profile,
                target_source.label,
                packet.allowed_paths.len(),
                packet.visible_success_conditions.len()
            ),
            constraints_applied: active_constraints.clone(),
            next_actions: next_actions.clone(),
            visible_success_conditions: packet.visible_success_conditions.clone(),
            return_artifacts: packet.return_artifacts.clone(),
        };
        let trail_state = TrailStateArtifact {
            run_id: run_id.to_string(),
            spec_id: spec_obj.id.clone(),
            product: target_source.label.clone(),
            adapter: packet.adapter.clone(),
            profile: packet.profile.clone(),
            updated_at: Utc::now().to_rfc3339(),
            continuity_file: continuity_file.clone(),
            active_constraints,
            next_actions,
            visible_success_conditions: packet.visible_success_conditions.clone(),
            return_artifacts: packet.return_artifacts.clone(),
            last_execution_outcome: None,
            last_execution_summary: None,
            last_execution_artifact: None,
            executed_commands: Vec::new(),
            returned_artifacts_snapshot: Vec::new(),
        };

        self.write_json_file(&run_dir.join("retriever_dispatch.json"), &dispatch)
            .await?;
        tokio::fs::write(
            run_dir.join("retriever_dispatch.md"),
            render_retriever_dispatch_markdown(&dispatch),
        )
        .await?;
        self.write_json_file(&run_dir.join(&continuity_file), &trail_state)
            .await?;

        Ok((dispatch, trail_state))
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
            self.write_json_file(&run_dir.join("blackboard.json"), board)
                .await?;
            if !board.run_id.trim().is_empty() {
                self.sync_checkpoints_for_board(&board.run_id, board)
                    .await?;
            }
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
        if !snapshot.run_id.trim().is_empty() {
            self.sync_checkpoints_for_board(&snapshot.run_id, &snapshot)
                .await?;
        }
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
            if !snapshot.run_id.trim().is_empty() {
                self.sync_checkpoints_for_board(&snapshot.run_id, &snapshot)
                    .await?;
            }
        }
        Ok(())
    }

    async fn attach_lessons_to_blackboard(
        &self,
        run_dir: &Path,
        lessons: &[LessonRecord],
    ) -> Result<()> {
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
        self.write_json_file(&run_dir.join("lessons.json"), &lessons)
            .await?;
        if !snapshot.run_id.trim().is_empty() {
            self.sync_checkpoints_for_board(&snapshot.run_id, &snapshot)
                .await?;
        }
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

    async fn run_attempt_number(&self, run_id: &str) -> Result<usize> {
        let Some(run) = self.get_run(run_id).await? else {
            bail!("run not found for hidden scenario evaluation: {run_id}");
        };

        let rows = sqlx::query(
            "SELECT run_id FROM runs WHERE spec_id = ?1 AND product = ?2 ORDER BY created_at ASC, run_id ASC",
        )
        .bind(&run.spec_id)
        .bind(&run.product)
        .fetch_all(&self.pool)
        .await?;

        rows.iter()
            .position(|row| row.get::<String, _>("run_id") == run_id)
            .map(|index| index + 1)
            .ok_or_else(|| anyhow::anyhow!("run {run_id} missing from attempt history"))
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

    pub async fn list_phase_attributions_for_run(
        &self,
        run_id: &str,
    ) -> Result<Vec<PhaseAttributionRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT attribution_id, run_id, episode_id, phase, agent_name, outcome, confidence,
                   prompt_bundle_fingerprint, prompt_bundle_provider, prompt_bundle_artifact,
                   pinned_skill_ids, memory_hits, core_memory_ids, project_memory_ids,
                   relevant_lesson_ids, required_checks, guardrails, query_terms, created_at
            FROM phase_attributions
            WHERE run_id = ?1
            ORDER BY created_at ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::new();
        for row in rows {
            records.push(PhaseAttributionRecord {
                attribution_id: row.get::<String, _>("attribution_id"),
                run_id: row.get::<String, _>("run_id"),
                episode_id: row.get::<String, _>("episode_id"),
                phase: row.get::<String, _>("phase"),
                agent_name: row.get::<String, _>("agent_name"),
                outcome: row.get::<String, _>("outcome"),
                confidence: row.get::<Option<f64>, _>("confidence"),
                prompt_bundle_fingerprint: row
                    .get::<Option<String>, _>("prompt_bundle_fingerprint"),
                prompt_bundle_provider: row.get::<Option<String>, _>("prompt_bundle_provider"),
                prompt_bundle_artifact: row.get::<Option<String>, _>("prompt_bundle_artifact"),
                pinned_skill_ids: serde_json::from_str(
                    row.get::<String, _>("pinned_skill_ids").as_str(),
                )
                .with_context(|| "parsing phase attribution pinned_skill_ids")?,
                memory_hits: serde_json::from_str(row.get::<String, _>("memory_hits").as_str())
                    .with_context(|| "parsing phase attribution memory_hits")?,
                core_memory_ids: serde_json::from_str(
                    row.get::<String, _>("core_memory_ids").as_str(),
                )
                .with_context(|| "parsing phase attribution core_memory_ids")?,
                project_memory_ids: serde_json::from_str(
                    row.get::<String, _>("project_memory_ids").as_str(),
                )
                .with_context(|| "parsing phase attribution project_memory_ids")?,
                relevant_lesson_ids: serde_json::from_str(
                    row.get::<String, _>("relevant_lesson_ids").as_str(),
                )
                .with_context(|| "parsing phase attribution relevant_lesson_ids")?,
                required_checks: serde_json::from_str(
                    row.get::<String, _>("required_checks").as_str(),
                )
                .with_context(|| "parsing phase attribution required_checks")?,
                guardrails: serde_json::from_str(row.get::<String, _>("guardrails").as_str())
                    .with_context(|| "parsing phase attribution guardrails")?,
                query_terms: serde_json::from_str(row.get::<String, _>("query_terms").as_str())
                    .with_context(|| "parsing phase attribution query_terms")?,
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
            });
        }
        Ok(records)
    }

    pub async fn consolidate_run_for_operator(&self, run_id: &str) -> Result<Vec<LessonRecord>> {
        let run_dir = self.run_dir(run_id);
        let spec_path = run_dir.join("spec.yaml");
        if !spec_path.exists() {
            bail!("run {run_id} is missing spec.yaml; cannot consolidate");
        }

        let spec_path_string = spec_path.to_string_lossy().to_string();
        let spec_obj = spec::load_spec(&spec_path_string)?;
        let lessons = self.consolidate_run(run_id, &spec_obj).await?;
        self.attach_lessons_to_blackboard(&run_dir, &lessons)
            .await?;
        Ok(lessons)
    }

    pub async fn materialize_run_checkpoints(&self, run_id: &str) -> Result<()> {
        let blackboard_path = self.run_dir(run_id).join("blackboard.json");
        if !blackboard_path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read_to_string(&blackboard_path).await?;
        let board = serde_json::from_str::<BlackboardState>(&raw)?;
        self.sync_checkpoints_for_board(run_id, &board).await
    }

    pub async fn list_run_checkpoints(&self, run_id: &str) -> Result<Vec<RunCheckpointRecord>> {
        self.materialize_run_checkpoints(run_id).await?;

        let rows = sqlx::query(
            r#"
            SELECT checkpoint_id, run_id, phase, agent, checkpoint_type, status, prompt,
                   context_json, created_at, resolved_at
            FROM run_checkpoints
            WHERE run_id = ?1
            ORDER BY created_at ASC, checkpoint_id ASC
            "#,
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;

        let mut records = Vec::new();
        for row in rows {
            let checkpoint_id = row.get::<String, _>("checkpoint_id");
            let answers = self.list_checkpoint_answers(&checkpoint_id).await?;
            records.push(RunCheckpointRecord {
                checkpoint_id,
                run_id: row.get::<String, _>("run_id"),
                phase: row.get::<Option<String>, _>("phase"),
                agent: row.get::<Option<String>, _>("agent"),
                checkpoint_type: row.get::<String, _>("checkpoint_type"),
                status: row.get::<String, _>("status"),
                prompt: row.get::<String, _>("prompt"),
                context_json: serde_json::from_str(row.get::<String, _>("context_json").as_str())?,
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
                resolved_at: row
                    .get::<Option<String>, _>("resolved_at")
                    .map(|value| chrono::DateTime::parse_from_rfc3339(&value))
                    .transpose()?
                    .map(|value| value.with_timezone(&Utc)),
                answers,
            });
        }
        Ok(records)
    }

    pub async fn reply_to_checkpoint(
        &self,
        run_id: &str,
        checkpoint_id: &str,
        answered_by: &str,
        answer_text: &str,
        decision_json: Option<serde_json::Value>,
        resolve: bool,
    ) -> Result<RunCheckpointRecord> {
        self.materialize_run_checkpoints(run_id).await?;
        let checkpoint = self
            .get_run_checkpoint(run_id, checkpoint_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("checkpoint {checkpoint_id} not found for run {run_id}")
            })?;

        let trimmed_answer = answer_text.trim();
        if trimmed_answer.is_empty() && decision_json.is_none() {
            bail!("checkpoint replies need answer_text or decision_json");
        }

        sqlx::query(
            r#"
            INSERT INTO checkpoint_answers (answer_id, checkpoint_id, answered_by, answer_text, decision_json, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(checkpoint_id)
        .bind(answered_by)
        .bind(trimmed_answer)
        .bind(serde_json::to_string(&decision_json.clone().unwrap_or_else(|| serde_json::json!({})))?)
        .bind(Utc::now().to_rfc3339())
        .execute(&self.pool)
        .await?;

        let next_status = if resolve { "resolved" } else { "answered" };
        let resolved_at = if resolve {
            Some(Utc::now().to_rfc3339())
        } else {
            None
        };

        sqlx::query(
            "UPDATE run_checkpoints SET status = ?1, resolved_at = ?2 WHERE checkpoint_id = ?3",
        )
        .bind(next_status)
        .bind(resolved_at)
        .bind(checkpoint_id)
        .execute(&self.pool)
        .await?;

        if resolve {
            self.resolve_checkpoint_on_blackboard(run_id, checkpoint_id)
                .await?;
        }

        let phase = checkpoint.phase.as_deref().unwrap_or("interaction");
        self.audit_checkpoint_activity(
            run_id,
            phase,
            answered_by,
            if resolve { "resolved" } else { "answered" },
            &format!(
                "{} {} checkpoint {}{}",
                answered_by,
                if resolve { "resolved" } else { "answered" },
                checkpoint_id,
                if trimmed_answer.is_empty() {
                    String::new()
                } else {
                    format!(": {}", trimmed_answer)
                }
            ),
        )
        .await?;

        self.get_run_checkpoint(run_id, checkpoint_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("checkpoint {checkpoint_id} disappeared after reply"))
    }

    pub async fn unblock_agent_checkpoints(
        &self,
        run_id: &str,
        agent: &str,
        checkpoint_id: Option<&str>,
        answered_by: &str,
        answer_text: Option<&str>,
        decision_json: Option<serde_json::Value>,
    ) -> Result<Vec<RunCheckpointRecord>> {
        let checkpoints = self
            .list_run_checkpoints(run_id)
            .await?
            .into_iter()
            .filter(|checkpoint| matches!(checkpoint.status.as_str(), "open" | "answered"))
            .filter(|checkpoint| {
                checkpoint
                    .agent
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(agent))
                    .unwrap_or(false)
            })
            .filter(|checkpoint| {
                checkpoint_id
                    .map(|expected| checkpoint.checkpoint_id == expected)
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        if checkpoints.is_empty() {
            if let Some(checkpoint_id) = checkpoint_id {
                bail!("checkpoint {checkpoint_id} is not open for agent {agent} on run {run_id}");
            }
            return Ok(Vec::new());
        }

        let default_note = format!("Operator unblocked agent {agent}");
        let answer_text = answer_text.unwrap_or(default_note.as_str());
        let mut resolved = Vec::new();
        for checkpoint in checkpoints {
            resolved.push(
                self.reply_to_checkpoint(
                    run_id,
                    &checkpoint.checkpoint_id,
                    answered_by,
                    answer_text,
                    decision_json.clone(),
                    true,
                )
                .await?,
            );
        }

        Ok(resolved)
    }

    pub async fn audit_checkpoint_activity(
        &self,
        run_id: &str,
        phase: &str,
        agent: &str,
        status: &str,
        message: &str,
    ) -> Result<()> {
        let log_path = self.run_log_path(run_id);
        self.record_event(run_id, None, phase, agent, status, message, &log_path)
            .await?;
        Ok(())
    }

    async fn list_checkpoint_answers(
        &self,
        checkpoint_id: &str,
    ) -> Result<Vec<CheckpointAnswerRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT answer_id, checkpoint_id, answered_by, answer_text, decision_json, created_at
            FROM checkpoint_answers
            WHERE checkpoint_id = ?1
            ORDER BY created_at ASC
            "#,
        )
        .bind(checkpoint_id)
        .fetch_all(&self.pool)
        .await?;

        let mut answers = Vec::new();
        for row in rows {
            let decision_value = serde_json::from_str::<serde_json::Value>(
                row.get::<String, _>("decision_json").as_str(),
            )?;
            answers.push(CheckpointAnswerRecord {
                answer_id: row.get::<String, _>("answer_id"),
                checkpoint_id: row.get::<String, _>("checkpoint_id"),
                answered_by: row.get::<String, _>("answered_by"),
                answer_text: row.get::<String, _>("answer_text"),
                decision_json: if decision_value.is_null()
                    || decision_value == serde_json::json!({})
                {
                    None
                } else {
                    Some(decision_value)
                },
                created_at: chrono::DateTime::parse_from_rfc3339(
                    row.get::<String, _>("created_at").as_str(),
                )?
                .with_timezone(&Utc),
            });
        }

        Ok(answers)
    }

    async fn get_run_checkpoint(
        &self,
        run_id: &str,
        checkpoint_id: &str,
    ) -> Result<Option<RunCheckpointRecord>> {
        let row = sqlx::query(
            r#"
            SELECT checkpoint_id, run_id, phase, agent, checkpoint_type, status, prompt,
                   context_json, created_at, resolved_at
            FROM run_checkpoints
            WHERE run_id = ?1 AND checkpoint_id = ?2
            "#,
        )
        .bind(run_id)
        .bind(checkpoint_id)
        .fetch_optional(&self.pool)
        .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        Ok(Some(RunCheckpointRecord {
            checkpoint_id: row.get::<String, _>("checkpoint_id"),
            run_id: row.get::<String, _>("run_id"),
            phase: row.get::<Option<String>, _>("phase"),
            agent: row.get::<Option<String>, _>("agent"),
            checkpoint_type: row.get::<String, _>("checkpoint_type"),
            status: row.get::<String, _>("status"),
            prompt: row.get::<String, _>("prompt"),
            context_json: serde_json::from_str(row.get::<String, _>("context_json").as_str())?,
            created_at: chrono::DateTime::parse_from_rfc3339(
                row.get::<String, _>("created_at").as_str(),
            )?
            .with_timezone(&Utc),
            resolved_at: row
                .get::<Option<String>, _>("resolved_at")
                .map(|value| chrono::DateTime::parse_from_rfc3339(&value))
                .transpose()?
                .map(|value| value.with_timezone(&Utc)),
            answers: self.list_checkpoint_answers(checkpoint_id).await?,
        }))
    }

    async fn resolve_checkpoint_on_blackboard(
        &self,
        run_id: &str,
        checkpoint_id: &str,
    ) -> Result<()> {
        let checkpoint = self
            .get_run_checkpoint(run_id, checkpoint_id)
            .await?
            .ok_or_else(|| {
                anyhow::anyhow!("checkpoint {checkpoint_id} not found for run {run_id}")
            })?;
        let blocker = checkpoint
            .context_json
            .get("blocker")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let blackboard_path = self.run_dir(run_id).join("blackboard.json");
        if !blackboard_path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read_to_string(&blackboard_path).await?;
        let mut board = serde_json::from_str::<BlackboardState>(&raw)?;
        if let Some(blocker) = blocker.as_deref() {
            remove_blocker(&mut board, blocker);
        }
        self.sync_blackboard(&board, Some(&self.run_dir(run_id)))
            .await
    }

    async fn sync_checkpoints_for_board(
        &self,
        run_id: &str,
        board: &BlackboardState,
    ) -> Result<()> {
        let desired = board
            .open_blockers
            .iter()
            .map(|blocker| checkpoint_draft(run_id, board, blocker))
            .collect::<Vec<_>>();
        let desired_ids = desired
            .iter()
            .map(|draft| draft.checkpoint_id.clone())
            .collect::<HashSet<_>>();

        let rows =
            sqlx::query("SELECT checkpoint_id, status FROM run_checkpoints WHERE run_id = ?1")
                .bind(run_id)
                .fetch_all(&self.pool)
                .await?;

        let mut existing_status = HashMap::new();
        for row in rows {
            existing_status.insert(
                row.get::<String, _>("checkpoint_id"),
                row.get::<String, _>("status"),
            );
        }

        for draft in desired {
            let next_status = match existing_status
                .get(&draft.checkpoint_id)
                .map(|value| value.as_str())
            {
                Some("answered") => "answered",
                Some("open") => "open",
                _ => "open",
            };
            sqlx::query(
                r#"
                INSERT INTO run_checkpoints (
                    checkpoint_id, run_id, phase, agent, checkpoint_type, status, prompt,
                    context_json, created_at, resolved_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                ON CONFLICT(checkpoint_id) DO UPDATE SET
                    phase = excluded.phase,
                    agent = excluded.agent,
                    checkpoint_type = excluded.checkpoint_type,
                    status = excluded.status,
                    prompt = excluded.prompt,
                    context_json = excluded.context_json,
                    resolved_at = excluded.resolved_at
                "#,
            )
            .bind(&draft.checkpoint_id)
            .bind(run_id)
            .bind(&draft.phase)
            .bind(&draft.agent)
            .bind(&draft.checkpoint_type)
            .bind(next_status)
            .bind(&draft.prompt)
            .bind(serde_json::to_string(&draft.context_json)?)
            .bind(Utc::now().to_rfc3339())
            .bind(Option::<String>::None)
            .execute(&self.pool)
            .await?;
        }

        for (checkpoint_id, status) in existing_status {
            if desired_ids.contains(&checkpoint_id) {
                continue;
            }
            if matches!(status.as_str(), "open" | "answered") {
                sqlx::query(
                    "UPDATE run_checkpoints SET status = 'resolved', resolved_at = ?1 WHERE checkpoint_id = ?2",
                )
                .bind(Utc::now().to_rfc3339())
                .bind(checkpoint_id)
                .execute(&self.pool)
                .await?;
            }
        }

        Ok(())
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

    async fn consolidate_run(&self, run_id: &str, spec_obj: &Spec) -> Result<Vec<LessonRecord>> {
        let episodes = self.list_run_episodes(run_id).await?;
        let prior_lessons = self.list_lessons().await?;
        let target_source = self.target_source_for_run(run_id).await?;
        let mut new_lessons = Vec::new();
        let mut known_lesson_ids = prior_lessons
            .iter()
            .map(|lesson| lesson.lesson_id.clone())
            .collect::<HashSet<_>>();

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
            if known_lesson_ids.contains(&lesson_id) {
                continue;
            }

            let lesson = LessonRecord {
                lesson_id: lesson_id.clone(),
                source_episode: Some(episode.episode_id.clone()),
                pattern: format!("Repeated failure pattern in {}: {}", episode.phase, pattern),
                intervention: infer_intervention(&events),
                tags: vec![
                    "lesson".to_string(),
                    "causal".to_string(),
                    "project-memory".to_string(),
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
            let lesson_body = format!(
                "Source episode: {}
Phase: {}
Intervention: {}
Observed pattern: {}",
                episode.episode_id,
                episode.phase,
                lesson
                    .intervention
                    .clone()
                    .unwrap_or_else(|| "No intervention recorded yet".to_string()),
                pattern
            );
            self.persist_lesson(
                lesson,
                lesson_body,
                target_source.as_ref(),
                Some(spec_obj),
                &mut known_lesson_ids,
                &mut new_lessons,
            )
            .await?;

            let intake_episode = crate::models::FactoryEpisode {
                run_id: run_id.to_string(),
                product: String::new(),
                spec_id: String::new(),
                features: vec![],
                agent_events: events.clone(),
                tool_events: vec![],
                phase_attributions: Vec::new(),
                twin_env: None,
                validation: None,
                scenarios: None,
                decision: None,
                created_at: Utc::now(),
            };
            let _ = self.coobie.ingest_episode(&intake_episode).await;
        }

        self.consolidate_phase_attribution_lessons(
            run_id,
            spec_obj,
            target_source.as_ref(),
            &mut known_lesson_ids,
            &mut new_lessons,
        )
        .await?;

        self.consolidate_exploration_artifacts(
            run_id,
            spec_obj,
            target_source.as_ref(),
            &mut known_lesson_ids,
            &mut new_lessons,
        )
        .await?;

        Ok(new_lessons)
    }

    async fn consolidate_phase_attribution_lessons(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: Option<&TargetSourceMetadata>,
        known_lesson_ids: &mut HashSet<String>,
        new_lessons: &mut Vec<LessonRecord>,
    ) -> Result<()> {
        let attributions = self.list_phase_attributions_for_run(run_id).await?;
        if attributions.is_empty() {
            return Ok(());
        }

        let successful_run = attributions
            .iter()
            .any(|record| record.phase == "validation" && record.outcome == "success")
            && attributions
                .iter()
                .any(|record| record.phase == "hidden_scenarios" && record.outcome == "success");

        for record in attributions {
            let outcome = record.outcome.to_ascii_lowercase();
            if outcome == "success" && !successful_run {
                continue;
            }
            if !matches!(outcome.as_str(), "success" | "failure" | "blocked") {
                continue;
            }

            let provider = record
                .prompt_bundle_provider
                .clone()
                .unwrap_or_else(|| "unresolved".to_string());
            let pinned_skill_ids_json = serde_json::to_string(&record.pinned_skill_ids)?;
            let supporting_rows = sqlx::query(
                r#"
                SELECT DISTINCT run_id
                FROM phase_attributions
                WHERE run_id != ?1
                  AND phase = ?2
                  AND agent_name = ?3
                  AND outcome = ?4
                  AND COALESCE(prompt_bundle_provider, '') = ?5
                  AND pinned_skill_ids = ?6
                ORDER BY created_at DESC
                LIMIT 6
                "#,
            )
            .bind(run_id)
            .bind(&record.phase)
            .bind(&record.agent_name)
            .bind(&outcome)
            .bind(&provider)
            .bind(&pinned_skill_ids_json)
            .fetch_all(&self.pool)
            .await?;
            if supporting_rows.is_empty() {
                continue;
            }
            let supporting_runs = supporting_rows
                .into_iter()
                .map(|row| row.get::<String, _>("run_id"))
                .collect::<Vec<_>>();

            let skill_label = if record.pinned_skill_ids.is_empty() {
                "no-pinned-skills".to_string()
            } else {
                record.pinned_skill_ids.join("+")
            };
            let lesson_id = format!(
                "lesson-phase-pattern-{}",
                stable_key_fragment(&format!(
                    "{}|{}|{}|{}|{}",
                    record.phase, record.agent_name, outcome, provider, skill_label
                ))
            );
            if known_lesson_ids.contains(&lesson_id) {
                continue;
            }

            let occurrence_count = supporting_runs.len() + 1;
            let pattern = if outcome == "success" {
                format!(
                    "Repeatable success pattern in {} / {} via {} with {}",
                    record.phase, record.agent_name, provider, skill_label
                )
            } else {
                format!(
                    "Repeatable failure pattern in {} / {} via {} with {}",
                    record.phase, record.agent_name, provider, skill_label
                )
            };
            let intervention = if outcome == "success" {
                Some(
                    "Reuse this provider and pinned-skill mix when similar work appears again."
                        .to_string(),
                )
            } else {
                Some(format!(
                    "Inspect the {} / {} prompt bundle, provider route, pinned skills, and required checks before rerunning this phase.",
                    record.phase, record.agent_name
                ))
            };
            let mut tags = vec![
                "lesson".to_string(),
                "phase-attribution".to_string(),
                "project-memory".to_string(),
                record.phase.clone(),
                record.agent_name.clone(),
                outcome.clone(),
                provider.to_ascii_lowercase(),
            ];
            if outcome == "success" {
                tags.push("success-pattern".to_string());
            } else {
                tags.push("failure-pattern".to_string());
                tags.push("causal".to_string());
            }
            let lesson = LessonRecord {
                lesson_id,
                source_episode: Some(record.episode_id.clone()),
                pattern,
                intervention,
                tags,
                strength: if outcome == "success" {
                    (0.8 + supporting_runs.len() as f64 * 0.1).min(1.5)
                } else {
                    (1.0 + supporting_runs.len() as f64 * 0.15).min(2.0)
                },
                recall_count: 0,
                last_recalled: None,
                created_at: Utc::now(),
            };
            let lesson_body = format!(
                "Occurrences: {}
Supporting runs: {}
Provider route: {}
Prompt bundle fingerprint: {}
Prompt bundle artifact: {}
Pinned skills: {}
Required checks: {}
Guardrails: {}
Memory ids: {}
Relevant lesson ids: {}
Query terms: {}",
                occurrence_count,
                supporting_runs.join(", "),
                provider,
                record
                    .prompt_bundle_fingerprint
                    .as_deref()
                    .unwrap_or("none recorded"),
                record
                    .prompt_bundle_artifact
                    .as_deref()
                    .unwrap_or("none recorded"),
                if record.pinned_skill_ids.is_empty() {
                    "none".to_string()
                } else {
                    record.pinned_skill_ids.join(" | ")
                },
                if record.required_checks.is_empty() {
                    "none".to_string()
                } else {
                    record.required_checks.join(" | ")
                },
                if record.guardrails.is_empty() {
                    "none".to_string()
                } else {
                    record.guardrails.join(" | ")
                },
                record
                    .project_memory_ids
                    .iter()
                    .chain(record.core_memory_ids.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" | "),
                if record.relevant_lesson_ids.is_empty() {
                    "none".to_string()
                } else {
                    record.relevant_lesson_ids.join(" | ")
                },
                if record.query_terms.is_empty() {
                    "none".to_string()
                } else {
                    record.query_terms.join(" | ")
                },
            );
            self.persist_lesson(
                lesson,
                lesson_body,
                target_source,
                Some(spec_obj),
                known_lesson_ids,
                new_lessons,
            )
            .await?;
        }

        Ok(())
    }

    async fn persist_lesson(
        &self,
        lesson: LessonRecord,
        lesson_body: String,
        target_source: Option<&TargetSourceMetadata>,
        spec_obj: Option<&Spec>,
        known_lesson_ids: &mut HashSet<String>,
        new_lessons: &mut Vec<LessonRecord>,
    ) -> Result<()> {
        if !known_lesson_ids.insert(lesson.lesson_id.clone()) {
            return Ok(());
        }

        self.insert_lesson(&lesson).await?;
        if let Some(target_source) = target_source {
            let provenance = project_memory_provenance(
                target_source,
                None,
                None,
                Vec::new(),
                vec![
                    "implementation behavior, oracle semantics, or runtime assumptions change"
                        .to_string(),
                ],
                spec_obj
                    .map(collect_spec_provenance_paths)
                    .unwrap_or_default(),
                spec_obj
                    .map(collect_spec_code_under_test_paths)
                    .unwrap_or_default(),
                spec_obj
                    .map(collect_spec_provenance_surfaces)
                    .unwrap_or_default(),
            );
            self.store_project_memory_entry(
                target_source,
                &lesson.lesson_id,
                lesson.tags.clone(),
                &lesson.pattern,
                &lesson_body,
                provenance.clone(),
            )
            .await?;
            self.reconcile_project_memory_statuses(target_source, &lesson)
                .await?;
        } else {
            self.memory_store
                .store_with_metadata(
                    &lesson.lesson_id,
                    lesson.tags.clone(),
                    &lesson.pattern,
                    &lesson_body,
                    MemoryProvenance::default(),
                )
                .await?;
        }

        if should_promote_to_core_memory(&lesson.tags) {
            let provenance = target_source
                .map(|target| {
                    project_memory_provenance(
                        target,
                        None,
                        None,
                        Vec::new(),
                        vec![
                            "cross-project applicability or external-system assumptions are contradicted".to_string(),
                        ],
                        spec_obj.map(collect_spec_provenance_paths).unwrap_or_default(),
                        spec_obj.map(collect_spec_code_under_test_paths).unwrap_or_default(),
                        spec_obj.map(collect_spec_provenance_surfaces).unwrap_or_default(),
                    )
                })
                .unwrap_or_default();
            self.memory_store
                .store_with_metadata(
                    &lesson.lesson_id,
                    lesson.tags.clone(),
                    &lesson.pattern,
                    &lesson_body,
                    provenance,
                )
                .await?;
        }

        new_lessons.push(lesson);
        Ok(())
    }

    async fn reconcile_project_memory_statuses(
        &self,
        target_source: &TargetSourceMetadata,
        lesson: &LessonRecord,
    ) -> Result<()> {
        if !lesson.tags.iter().any(|tag| tag == "lesson") {
            return Ok(());
        }

        let store = self.project_memory_store(target_source).await?;
        let entries = store.list_entries().await?;
        let lesson_key = normalize_memory_text(&lesson.pattern);
        let lesson_intervention = lesson
            .intervention
            .clone()
            .unwrap_or_default()
            .to_lowercase();

        for entry in entries {
            if entry.id == lesson.lesson_id || !entry.tags.iter().any(|tag| tag == "lesson") {
                continue;
            }

            let overlap = shared_specific_tag_count(&entry.tags, &lesson.tags);
            let entry_key = normalize_memory_text(&entry.summary);
            let same_pattern = !entry_key.is_empty() && entry_key == lesson_key;

            if same_pattern
                && !lesson_intervention.is_empty()
                && !entry.content.to_lowercase().contains(&lesson_intervention)
            {
                store
                    .annotate_entry_status(&entry.id, "superseded", Some(&lesson.lesson_id))
                    .await?;
            } else if overlap >= 2 && entry_key != lesson_key {
                store
                    .annotate_entry_status(&entry.id, "challenged", Some(&lesson.lesson_id))
                    .await?;
            }
        }

        self.write_project_memory_status_snapshot(target_source, &store)
            .await?;
        Ok(())
    }

    async fn write_project_memory_status_snapshot(
        &self,
        target_source: &TargetSourceMetadata,
        store: &MemoryStore,
    ) -> Result<()> {
        let entries = store.list_entries().await?;
        let relevant = entries
            .into_iter()
            .filter(|entry| {
                entry.tags.iter().any(|tag| tag == "lesson")
                    && (entry.provenance.status.is_some()
                        || entry.provenance.superseded_by.is_some()
                        || !entry.provenance.challenged_by.is_empty())
            })
            .collect::<Vec<_>>();
        let harkonnen_dir = self.project_harkonnen_dir(target_source);
        self.write_json_file(&harkonnen_dir.join("memory-status.json"), &relevant)
            .await?;
        tokio::fs::write(
            harkonnen_dir.join("memory-status.md"),
            render_project_memory_status_markdown(&relevant),
        )
        .await?;
        Ok(())
    }

    async fn consolidate_exploration_artifacts(
        &self,
        run_id: &str,
        spec_obj: &Spec,
        target_source: Option<&TargetSourceMetadata>,
        known_lesson_ids: &mut HashSet<String>,
        new_lessons: &mut Vec<LessonRecord>,
    ) -> Result<()> {
        let run_dir = self.run_dir(run_id);
        let run_record = self.get_run(run_id).await?;
        let exploration_path = run_dir.join("exploration_log.json");
        if exploration_path.exists() {
            let raw = tokio::fs::read_to_string(&exploration_path).await?;
            if let Ok(log) = serde_json::from_str::<ExplorationLogArtifact>(&raw) {
                for entry in log.entries {
                    if !matches!(entry.outcome.as_str(), "failure" | "blocked") {
                        continue;
                    }
                    let lesson = LessonRecord {
                        lesson_id: format!("lesson-exploration-{}", entry.episode_id),
                        source_episode: Some(entry.episode_id.clone()),
                        pattern: format!(
                            "Residue exploration dead-end in {}: {}",
                            entry.phase, entry.strategy
                        ),
                        intervention: Some(entry.reformulation.clone()),
                        tags: vec![
                            "lesson".to_string(),
                            "residue".to_string(),
                            "exploration".to_string(),
                            "project-memory".to_string(),
                            entry.phase.clone(),
                            entry.agent.clone(),
                        ],
                        strength: 0.6,
                        recall_count: 0,
                        last_recalled: None,
                        created_at: Utc::now(),
                    };
                    let lesson_body = format!(
                        "Strategy: {}
Failure constraint: {}
Surviving structure: {}
Reformulation: {}
Artifacts: {}
Parameters: {}
Open questions: {}",
                        entry.strategy,
                        entry.failure_constraint,
                        entry.surviving_structure,
                        entry.reformulation,
                        entry.artifacts.join(" | "),
                        entry.parameters.join(" | "),
                        entry.open_questions.join(" | "),
                    );
                    self.persist_lesson(
                        lesson,
                        lesson_body,
                        target_source,
                        Some(spec_obj),
                        known_lesson_ids,
                        new_lessons,
                    )
                    .await?;
                }
            }
        }

        let Some(run_record) = run_record else {
            return Ok(());
        };
        let registry_path = self.paths.factory.join("state").join("dead_ends.json");
        if !registry_path.exists() {
            return Ok(());
        }
        let raw = tokio::fs::read_to_string(&registry_path).await?;
        let registry = serde_json::from_str::<DeadEndRegistry>(&raw).unwrap_or_default();
        let relevant = registry
            .entries
            .into_iter()
            .filter(|entry| {
                entry.spec_id == run_record.spec_id && entry.product == run_record.product
            })
            .collect::<Vec<_>>();
        let mut grouped = HashMap::<String, Vec<DeadEndRegistryEntry>>::new();
        for entry in relevant {
            grouped
                .entry(format!(
                    "{}|{}|{}",
                    entry.phase, entry.agent, entry.strategy
                ))
                .or_default()
                .push(entry);
        }
        for (key, entries) in grouped {
            if entries.len() < 2 {
                continue;
            }
            let latest = entries
                .last()
                .cloned()
                .unwrap_or_else(|| entries[0].clone());
            let lesson = LessonRecord {
                lesson_id: format!("lesson-dead-end-{}-{}", run_id, stable_key_fragment(&key)),
                source_episode: None,
                pattern: format!(
                    "Recurring dead-end strategy in {} / {}: {}",
                    latest.phase, latest.agent, latest.strategy
                ),
                intervention: Some(latest.reformulation.clone()),
                tags: vec![
                    "lesson".to_string(),
                    "residue".to_string(),
                    "dead-end".to_string(),
                    "strategy-register".to_string(),
                    "project-memory".to_string(),
                    latest.phase.clone(),
                    latest.agent.clone(),
                ],
                strength: (entries.len() as f64).min(3.0) / 2.0,
                recall_count: 0,
                last_recalled: None,
                created_at: Utc::now(),
            };
            let lesson_body = format!(
                "Occurrences: {}
Latest failure constraint: {}
Latest surviving structure: {}
Latest reformulation: {}
Run ids: {}",
                entries.len(),
                latest.failure_constraint,
                latest.surviving_structure,
                latest.reformulation,
                entries
                    .iter()
                    .map(|entry| entry.run_id.clone())
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            self.persist_lesson(
                lesson,
                lesson_body,
                target_source,
                Some(spec_obj),
                known_lesson_ids,
                new_lessons,
            )
            .await?;
        }

        Ok(())
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
        self.write_json_file(&bundle_dir.join("run.json"), &run)
            .await?;
        self.write_json_file(&bundle_dir.join("events.json"), &events)
            .await?;

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
            tokio::fs::write(
                bundle_dir.join("workspace_manifest.txt"),
                manifest.join("\n"),
            )
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
    let pattern_matching_focus = render_list(
        &briefing.pattern_matching_focus,
        "No pattern-matching focus was derived from promoted evidence exemplars.",
    );
    let causal_chain_focus = render_list(
        &briefing.causal_chain_focus,
        "No causal-chain focus was derived from promoted evidence exemplars.",
    );
    let nearest_evidence_windows = render_list(
        &briefing
            .nearest_evidence_window_citations
            .iter()
            .map(|citation| format!("{} [{}]", citation.summary, citation.evidence))
            .collect::<Vec<_>>(),
        "No reviewed evidence windows were retrieved from project annotation bundles.",
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

## Pattern Matching Focus
{}

## Causal Chains To Probe
{}

## Nearest Reviewed Evidence Windows
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
        pattern_matching_focus,
        causal_chain_focus,
        nearest_evidence_windows,
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
        spec_obj.scope.join(
            "
"
        ),
        spec_obj.constraints.join(
            "
"
        ),
        spec_obj.inputs.join(
            "
"
        ),
        spec_obj.outputs.join(
            "
"
        ),
        spec_obj.acceptance_criteria.join(
            "
"
        ),
        spec_obj.dependencies.join(
            "
"
        ),
        spec_obj.performance_expectations.join(
            "
"
        ),
        spec_obj.security_expectations.join(
            "
"
        ),
        query_terms.join(
            "
"
        ),
    )
    .to_lowercase();

    let signal_map = [
        (
            ["sensor", "telemetry", "sampling", "daq"].as_slice(),
            "high_speed_sensing",
        ),
        (
            ["plc", "opc ua", "modbus", "ethernet/ip", "fieldbus"].as_slice(),
            "plc_control",
        ),
        (
            ["histori", "time series", "pi system"].as_slice(),
            "historian_integration",
        ),
        (
            ["scada", "hmi", "alarm", "operator"].as_slice(),
            "scada_operations",
        ),
        (
            [
                "simulator",
                "digital twin",
                "emulator",
                "hardware in the loop",
            ]
            .as_slice(),
            "simulation",
        ),
        (
            ["analytics", "model", "inference", "prediction"].as_slice(),
            "analytics",
        ),
        (
            ["latency", "throughput", "real-time", "jitter", "cycle time"].as_slice(),
            "timing_sensitive",
        ),
        (
            [
                "fail-safe",
                "interlock",
                "shutdown",
                "degraded mode",
                "safety",
            ]
            .as_slice(),
            "safety_critical",
        ),
        (
            ["gmp", "gxp", "21 cfr part 11", "audit trail", "validation"].as_slice(),
            "regulated_environment",
        ),
        (
            ["batch", "recipe", "traceability", "electronic record"].as_slice(),
            "manufacturing_execution",
        ),
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

    if domain_signals
        .iter()
        .any(|signal| signal == "timing_sensitive")
    {
        risks.push("Throughput, latency, or jitter budgets may be violated without explicit buffering and backpressure handling.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "analytics") {
        risks.push("Analytics outputs may look plausible while operating on stale, replayed, or low-quality plant data.".to_string());
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "manufacturing_execution")
    {
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
    if memory_hits
        .iter()
        .any(|hit| hit.contains("No memories found"))
    {
        risks.push("Coobie found little directly reusable prior context, so assumptions need stronger explicit checks and telemetry.".to_string());
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        if !blueprint.coobie_memory_topics.is_empty()
            && memory_hits
                .iter()
                .any(|hit| hit.contains("No memories found"))
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

    if domain_signals
        .iter()
        .any(|signal| signal == "high_speed_sensing")
    {
        risks.push("High-rate sensor ingest can drop samples or reorder packets unless the twin exercises burst conditions and queue saturation.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "plc_control") {
        risks.push("PLC handshakes, state transitions, and command acknowledgement timing can diverge from nominal flows on the shop floor.".to_string());
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "historian_integration")
    {
        risks.push("Historian lag, replay, and tag-quality changes can create false confidence if only happy-path reads are simulated.".to_string());
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "scada_operations")
    {
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
        spec_obj.constraints.join(
            "
"
        ),
        spec_obj.acceptance_criteria.join(
            "
"
        ),
        spec_obj.security_expectations.join(
            "
"
        ),
        spec_obj.outputs.join(
            "
"
        ),
        spec_obj.purpose,
    )
    .to_lowercase();

    if domain_signals
        .iter()
        .any(|signal| signal == "regulated_environment")
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
    relevant_lessons: &[LessonRecord],
) -> Vec<String> {
    let mut guardrails = vec![
        "Require every planning agent to consume Coobie's briefing before finalizing its output.".to_string(),
        "Prefer explicit state machines and quality-coded data flows over implicit happy-path assumptions.".to_string(),
        "Capture evidence artifacts for each critical validation claim instead of relying on narrative confidence.".to_string(),
    ];

    if domain_signals
        .iter()
        .any(|signal| signal == "timing_sensitive")
    {
        guardrails.push("Model latency budgets, queue limits, retry windows, and timeout behavior as first-class constraints.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "plc_control") {
        guardrails.push("Do not assume PLC writes succeeded until acknowledgement, state echo, and timeout handling are explicitly checked.".to_string());
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "regulated_environment")
    {
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
    if memory_hits
        .iter()
        .any(|hit| hit.contains("No memories found"))
    {
        guardrails.push("When prior memory is weak, convert assumptions into explicit open questions and required checks before implementation proceeds.".to_string());
    }
    if let Some(cause) = prior_causes.first() {
        guardrails.push(format!(
            "Account for recurring cause '{}' early instead of waiting for validation to rediscover it.",
            cause.description
        ));
    }
    for lesson in relevant_lessons.iter().take(3) {
        guardrails.push(format!(
            "Apply distilled lesson before acting: {}.",
            lesson.pattern
        ));
        if let Some(intervention) = &lesson.intervention {
            guardrails.push(format!(
                "Respect learned intervention from Coobie memory: {}.",
                intervention
            ));
        }
    }
    if relevant_lessons.iter().any(|lesson| {
        lesson
            .tags
            .iter()
            .any(|tag| tag == "dead-end" || tag == "strategy-register")
    }) {
        guardrails.push("Do not repeat a recorded dead-end strategy unless this run introduces new evidence that explicitly changes the constraint.".to_string());
    }
    if relevant_lessons.iter().any(|lesson| {
        lesson
            .tags
            .iter()
            .any(|tag| tag == "residue" || tag == "exploration")
    }) {
        guardrails.push("Record each serious attempt with strategy, failure constraint, surviving structure, and reformulation so Coobie can compare retries structurally.".to_string());
    }

    guardrails.dedup();
    guardrails
}

fn build_required_checks(
    spec_obj: &Spec,
    domain_signals: &[String],
    regulatory_considerations: &[String],
    relevant_lessons: &[LessonRecord],
) -> Vec<String> {
    let mut checks = vec![
        "Visible validation must prove the main project still builds or executes in the staged workspace.".to_string(),
        "The twin narrative must state which external systems are simulated, stubbed, or still missing.".to_string(),
    ];

    if domain_signals
        .iter()
        .any(|signal| signal == "high_speed_sensing")
    {
        checks.push("Exercise burst input conditions and verify sample loss, ordering, and backpressure behavior.".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "plc_control") {
        checks.push(
            "Verify PLC command/acknowledgement, heartbeat loss, and safe timeout behavior."
                .to_string(),
        );
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "historian_integration")
    {
        checks.push("Test historian lag, stale-tag quality, and replay behavior before trusting analytics outputs.".to_string());
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "scada_operations")
    {
        checks.push(
            "Validate alarm semantics, acknowledgement flow, and operator-visible degraded modes."
                .to_string(),
        );
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
    if relevant_lessons.iter().any(|lesson| {
        lesson
            .tags
            .iter()
            .any(|tag| tag == "residue" || tag == "exploration")
    }) {
        checks.push("Write or update exploration_log.json with strategy, failure constraint, surviving structure, reformulation, artifacts, and open questions before the run closes.".to_string());
    }
    if relevant_lessons.iter().any(|lesson| {
        lesson
            .tags
            .iter()
            .any(|tag| tag == "dead-end" || tag == "strategy-register")
    }) {
        checks.push("When retrying a recorded dead-end, emit evidence showing what changed relative to the last failed strategy before claiming parity or recovery.".to_string());
    }

    checks.dedup();
    checks
}

fn build_stale_memory_mitigation_plan(risks: &[ProjectResumeRisk]) -> Vec<String> {
    let priority = risks
        .iter()
        .filter(|risk| matches!(risk.severity.as_str(), "critical" | "high"))
        .collect::<Vec<_>>();
    if priority.is_empty() {
        return Vec::new();
    }

    let mut steps = Vec::new();
    for risk in priority.into_iter().take(5) {
        steps.push(format!(
            "Revalidate memory {} before relying on it because severity={} score={}.",
            risk.memory_id, risk.severity, risk.severity_score
        ));
        if risk
            .reasons
            .iter()
            .any(|reason| reason.contains("explicit code_under_test paths changed"))
        {
            steps.push(format!(
                "Run targeted checks against the changed code-under-test paths linked to memory {}.",
                risk.memory_id
            ));
        } else if risk.reasons.iter().any(|reason| {
            reason.contains("recorded paths changed since memory commit")
                || reason.contains("working tree changes overlap recorded paths")
        }) {
            steps.push(format!(
                "Compare current behavior on the changed paths against the assumption captured in memory {}.",
                risk.memory_id
            ));
        }
        if risk
            .status
            .as_deref()
            .is_some_and(|status| matches!(status, "superseded" | "challenged"))
        {
            steps.push(format!(
                "Find the newer evidence that replaces or challenges memory {} before planning continues.",
                risk.memory_id
            ));
        }
    }
    steps.dedup();
    steps
}

fn apply_stale_memory_mitigations(
    risks: &[ProjectResumeRisk],
    recommended_guardrails: &mut Vec<String>,
    required_checks: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    let priority = risks
        .iter()
        .filter(|risk| matches!(risk.severity.as_str(), "critical" | "high"))
        .collect::<Vec<_>>();
    if priority.is_empty() {
        return;
    }

    recommended_guardrails.push(
        "Treat high-risk stale project memories as provisional until Coobie’s mitigation steps are satisfied.".to_string(),
    );

    for risk in priority.into_iter().take(4) {
        required_checks.push(format!(
            "Revalidate stale memory {} before relying on it; severity={} score={}.",
            risk.memory_id, risk.severity, risk.severity_score
        ));
        if risk
            .reasons
            .iter()
            .any(|reason| reason.contains("explicit code_under_test paths changed"))
        {
            required_checks.push(format!(
                "Run targeted validation against the changed code_under_test paths for memory {} before Mason, Ash, or Bramble reuses it.",
                risk.memory_id
            ));
            open_questions.push(format!(
                "Does memory {} still hold after the explicit code_under_test changes named in the resume packet?",
                risk.memory_id
            ));
        } else if risk.reasons.iter().any(|reason| {
            reason.contains("recorded paths changed since memory commit")
                || reason.contains("working tree changes overlap recorded paths")
        }) {
            required_checks.push(format!(
                "Compare current behavior on the changed paths to the assumption captured in memory {} before using it as guidance.",
                risk.memory_id
            ));
            open_questions.push(format!(
                "Which part of memory {} is now at risk because the underlying paths changed?",
                risk.memory_id
            ));
        }
        if risk
            .status
            .as_deref()
            .is_some_and(|status| matches!(status, "superseded" | "challenged"))
        {
            recommended_guardrails.push(format!(
                "Do not rely on memory {} as settled truth until the newer contradicting evidence is reviewed.",
                risk.memory_id
            ));
            open_questions.push(format!(
                "Which newer evidence should replace or qualify memory {} before planning proceeds?",
                risk.memory_id
            ));
        }
    }

    recommended_guardrails.dedup();
    required_checks.dedup();
    open_questions.dedup();
}

fn apply_mitigation_history_context(
    citations: &[CoobieEvidenceCitation],
    stale_memory_mitigation_plan: &mut Vec<String>,
    recommended_guardrails: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    if citations.is_empty() {
        return;
    }

    for citation in citations.iter().take(3) {
        stale_memory_mitigation_plan.push(format!(
            "Reuse prior mitigation evidence: {} ({})",
            citation.summary, citation.evidence
        ));
    }
    if citations
        .iter()
        .any(|citation| citation.evidence.contains("reduced_from_previous=true"))
    {
        recommended_guardrails.push(
            "When a prior stale-memory mitigation reduced severity, preserve that check path instead of rediscovering it from scratch.".to_string(),
        );
    }
    if citations
        .iter()
        .any(|citation| citation.summary.contains("unresolved"))
    {
        open_questions.push(
            "Which stale-memory mitigation remained unresolved on the last comparable run, and what evidence is still missing?".to_string(),
        );
    }

    stale_memory_mitigation_plan.dedup();
    recommended_guardrails.dedup();
    open_questions.dedup();
}

fn apply_forge_evidence_context(
    citations: &[CoobieEvidenceCitation],
    recommended_guardrails: &mut Vec<String>,
    required_checks: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    if citations.is_empty() {
        return;
    }

    for citation in citations.iter().take(3) {
        if citation.summary.contains("denied") || citation.evidence.contains(":fail") {
            recommended_guardrails.push(format!(
                "Respect prior retriever-forge evidence from {} before rerunning the same bounded command path.",
                citation.run_id
            ));
            required_checks.push(format!(
                "Explain what changed since retriever-forge evidence {} before claiming the same command path will now pass.",
                citation.citation_id
            ));
            open_questions.push(format!(
                "Which prior forge denial or command failure from {} is still relevant to this run?",
                citation.run_id
            ));
        }
        if citation.summary.contains("passed") {
            recommended_guardrails.push(format!(
                "Reuse the successful retriever-forge evidence path recorded in {} instead of inventing a new bounded execution route without cause.",
                citation.run_id
            ));
        }
    }

    recommended_guardrails.dedup();
    required_checks.dedup();
    open_questions.dedup();
}

fn apply_preferred_forge_outcome_context(
    citations: &[CoobieEvidenceCitation],
    recommended_guardrails: &mut Vec<String>,
    required_checks: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    if citations.is_empty() {
        return;
    }

    for citation in citations.iter().take(4) {
        let summary = citation.summary.to_lowercase();
        if summary.contains("kept helping") {
            recommended_guardrails.push(format!(
                "Prefer the previously helpful bounded command path cited in {} unless current repo evidence contradicts it.",
                citation.citation_id
            ));
        }
        if summary.contains("went stale") {
            required_checks.push(format!(
                "Explain why the stale preferred command path cited in {} should be trusted again before reusing it.",
                citation.citation_id
            ));
            open_questions.push(format!(
                "What changed since preferred command evidence {} caused that path to go stale?",
                citation.citation_id
            ));
        }
    }

    recommended_guardrails.dedup();
    required_checks.dedup();
    open_questions.dedup();
}

fn build_pattern_matching_focus(citations: &[CoobieEvidenceCitation]) -> Vec<String> {
    let mut focus = Vec::new();
    for citation in citations.iter().take(4) {
        push_unique(
            &mut focus,
            &format!(
                "Pattern-match the current evidence against promoted exemplar: {}",
                citation.summary
            ),
        );
    }
    focus
}

fn build_causal_chain_focus(citations: &[CoobieEvidenceCitation]) -> Vec<String> {
    let mut focus = Vec::new();
    for citation in citations.iter().take(4) {
        push_unique(
            &mut focus,
            &format!(
                "Probe whether the current run repeats this cause/effect/intervention chain: {}",
                citation.summary
            ),
        );
    }
    focus
}

fn apply_evidence_exemplar_context(
    pattern_citations: &[CoobieEvidenceCitation],
    causal_citations: &[CoobieEvidenceCitation],
    pattern_focus: &[String],
    causal_focus: &[String],
    query_terms: &mut Vec<String>,
    recommended_guardrails: &mut Vec<String>,
    required_checks: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    for focus in pattern_focus.iter().take(3) {
        push_unique(query_terms, focus);
    }
    for focus in causal_focus.iter().take(3) {
        push_unique(query_terms, focus);
    }

    for citation in pattern_citations.iter().take(3) {
        push_unique(query_terms, &citation.summary);
        push_unique(
            query_terms,
            &format!("pattern exemplar {}", citation.summary),
        );
    }
    for citation in causal_citations.iter().take(3) {
        push_unique(query_terms, &citation.summary);
        push_unique(
            query_terms,
            &format!("causal exemplar {}", citation.summary),
        );
    }

    if !pattern_citations.is_empty() {
        recommended_guardrails.push(
            "When promoted pattern exemplars exist, search for similar windows, shapes, or timelines before inventing a fresh classification story.".to_string(),
        );
        required_checks.push(
            "Compare current evidence against the cited pattern exemplars and record why each candidate window matches or differs.".to_string(),
        );
        open_questions.push(
            "Which current windows most closely resemble the promoted pattern exemplars, and what important differences remain?".to_string(),
        );
    }

    if !causal_citations.is_empty() {
        recommended_guardrails.push(
            "When promoted causal exemplars exist, reason in explicit cause -> effect -> intervention chains instead of isolated anomalies.".to_string(),
        );
        required_checks.push(
            "Trace whether the current run shows the same preconditions, effect window, and intervention outcome as the cited causal exemplars.".to_string(),
        );
        open_questions.push(
            "Which intervention, missing intervention, or causal precondition distinguishes the current run from the promoted causal exemplars?".to_string(),
        );
    }

    recommended_guardrails.dedup();
    required_checks.dedup();
    open_questions.dedup();
}

fn apply_nearest_evidence_window_context(
    citations: &[CoobieEvidenceCitation],
    query_terms: &mut Vec<String>,
    required_checks: &mut Vec<String>,
    open_questions: &mut Vec<String>,
) {
    if citations.is_empty() {
        return;
    }

    for citation in citations.iter().take(3) {
        push_unique(query_terms, &citation.summary);
    }
    required_checks.push(
        "Compare the current run against the nearest reviewed evidence windows and record why each is a match, near-match, or mismatch.".to_string(),
    );
    open_questions.push(
        "Which retrieved prior evidence window is the closest match to the current behavior, and does it confirm the same explanation or reveal a new branch?".to_string(),
    );

    required_checks.dedup();
    open_questions.dedup();
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
    if domain_signals
        .iter()
        .any(|signal| signal == "high_speed_sensing")
    {
        questions.push("What sampling rates, burst sizes, and loss tolerances define acceptable behavior for incoming sensor data?".to_string());
    }
    if domain_signals
        .iter()
        .any(|signal| signal == "historian_integration")
    {
        questions.push("What historian freshness, replay, and tag-quality guarantees must the application respect?".to_string());
    }
    if domain_signals.iter().any(|signal| signal == "simulation") {
        questions.push("Which simulator behaviors are trusted representations of the plant, and which are convenience stubs only?".to_string());
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        if blueprint
            .pattern
            .eq_ignore_ascii_case("reference_oracle_regression")
        {
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
        questions.push(
            "What performance envelope should the system honor under realistic plant load?"
                .to_string(),
        );
    }

    questions.dedup();
    questions
}

fn render_target_git_summary(git: Option<&TargetGitMetadata>) -> String {
    match git {
        Some(git) => format!(
            "branch={} commit={} remote={} clean={} changed_paths={}",
            git.branch.as_deref().unwrap_or("unknown"),
            git.commit.as_deref().unwrap_or("unknown"),
            git.remote_origin.as_deref().unwrap_or("unknown"),
            git.clean
                .map(|value| if value { "true" } else { "false" })
                .unwrap_or("unknown"),
            git.changed_paths.len(),
        ),
        None => "git metadata unavailable".to_string(),
    }
}

fn build_worker_task_envelope(
    run_id: &str,
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    worker_harness: &WorkerHarnessConfig,
    briefing: &CoobieBriefing,
    repo_root: &Path,
    workspace_root: &Path,
    run_dir: &Path,
    staged_product: &Path,
    context_bundle: &RetrieverContextBundleArtifact,
) -> WorkerTaskEnvelope {
    let mut allowed_paths = vec![staged_product.display().to_string()];
    allowed_paths.push(run_dir.join("spec.yaml").display().to_string());
    allowed_paths.push(run_dir.join("intent.json").display().to_string());
    allowed_paths.push(run_dir.join("coobie_briefing.json").display().to_string());
    allowed_paths.push(
        run_dir
            .join("coobie_preflight_response.md")
            .display()
            .to_string(),
    );

    for entry in context_bundle
        .context_entries
        .iter()
        .chain(context_bundle.skill_entries.iter())
    {
        push_unique(&mut allowed_paths, &entry.path);
    }

    for component in &spec_obj.project_components {
        if worker_harness.allowed_components.is_empty()
            || worker_harness
                .allowed_components
                .iter()
                .any(|name| name == &component.name || name == &component.role)
        {
            let component_path = component.path.trim();
            if !component_path.is_empty() {
                push_unique(&mut allowed_paths, component_path);
            }
        }
    }

    let mut denied_paths = vec![
        workspace_root
            .join("factory/scenarios/hidden")
            .display()
            .to_string(),
        workspace_root.join("factory/memory").display().to_string(),
        Path::new(&target_source.source_path)
            .join(".harkonnen/project-memory")
            .display()
            .to_string(),
    ];
    for path in &worker_harness.denied_paths {
        push_unique(&mut denied_paths, path);
    }

    let visible_success_conditions = if worker_harness.visible_success_conditions.is_empty() {
        spec_obj.acceptance_criteria.clone()
    } else {
        worker_harness.visible_success_conditions.clone()
    };
    let return_artifacts = if worker_harness.return_artifacts.is_empty() {
        vec![
            "changed_files".to_string(),
            "execution_log".to_string(),
            "visible_validation_report".to_string(),
            "rationale_summary".to_string(),
        ]
    } else {
        worker_harness.return_artifacts.clone()
    };
    let editable_paths = collect_staged_code_under_test_paths(spec_obj, target_source, repo_root);

    WorkerTaskEnvelope {
        job_id: run_id.to_string(),
        spec_id: spec_obj.id.clone(),
        product: target_source.label.clone(),
        adapter: fallback_worker_value(&worker_harness.adapter, "manual"),
        profile: fallback_worker_value(&worker_harness.profile, "trail_pack_forge"),
        target_source: target_source.source_path.clone(),
        staged_workspace: staged_product.display().to_string(),
        allowed_paths,
        denied_paths,
        visible_success_conditions,
        return_artifacts,
        max_iterations: worker_harness.max_iterations.unwrap_or(6),
        continuity_file: worker_harness.continuity_file.clone(),
        context_bundle_artifact: Some("retriever_context_bundle.json".to_string()),
        trail_drift_guard_artifact: Some("trail_drift_guard.json".to_string()),
        repo_local_context_paths: context_bundle
            .context_entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect(),
        repo_local_skill_paths: context_bundle
            .skill_entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect(),
        repo_local_context_notes: context_bundle.preload_notes.clone(),
        query_terms: briefing.query_terms.clone(),
        preferred_commands: briefing.preferred_forge_commands.clone(),
        guardrails: briefing.recommended_guardrails.clone(),
        required_checks: briefing.required_checks.clone(),
        llm_edits: worker_harness.llm_edits,
        editable_paths,
    }
}

fn build_plan_review_chain(
    run_id: &str,
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    intent: &IntentPackage,
    briefing: &CoobieBriefing,
    implementation_plan: &str,
    context_bundle: Option<&RetrieverContextBundleArtifact>,
) -> PlanReviewChainArtifact {
    let mut stages = Vec::new();
    stages.push(PlanReviewStage {
        stage: "draft_plan".to_string(),
        owner: "scout".to_string(),
        summary: intent.summary.clone(),
        evidence: intent.recommended_steps.clone(),
    });
    stages.push(PlanReviewStage {
        stage: "gap_review".to_string(),
        owner: "scout".to_string(),
        summary: if intent.ambiguity_notes.is_empty() {
            "No obvious spec ambiguity was recorded in Scout intake.".to_string()
        } else {
            format!(
                "Scout identified {} ambiguity note(s) that should constrain execution.",
                intent.ambiguity_notes.len()
            )
        },
        evidence: if intent.ambiguity_notes.is_empty() {
            vec!["No ambiguity notes recorded.".to_string()]
        } else {
            intent.ambiguity_notes.clone()
        },
    });
    stages.push(PlanReviewStage {
        stage: "repo_local_context_review".to_string(),
        owner: "coobie".to_string(),
        summary: match context_bundle {
            Some(bundle) => format!(
                "Loaded {} repo-local context file(s) and {} skill bundle(s) for '{}' before forge execution.",
                bundle.context_entries.len(),
                bundle.skill_entries.len(),
                target_source.label
            ),
            None => "No repo-local context bundle was attached to the forge plan.".to_string(),
        },
        evidence: match context_bundle {
            Some(bundle) => bundle
                .preload_notes
                .iter()
                .cloned()
                .chain(bundle.context_entries.iter().take(4).map(|entry| format!("{} [{}]", entry.label, entry.path)))
                .chain(bundle.skill_entries.iter().take(4).map(|entry| format!("{} [{}]", entry.label, entry.path)))
                .take(10)
                .collect(),
            None => vec!["No repo-local context or skill bundles were discovered for this run.".to_string()],
        },
    });
    stages.push(PlanReviewStage {
        stage: "ruthless_review".to_string(),
        owner: "coobie".to_string(),
        summary: format!(
            "Coobie highlighted {} application/environment risk(s) and {} stale-memory risk(s) before execution.",
            briefing.application_risks.len() + briefing.environment_risks.len(),
            briefing.resume_packet_risks.len()
        ),
        evidence: briefing
            .application_risks
            .iter()
            .chain(briefing.environment_risks.iter())
            .chain(briefing.resume_packet_risks.iter().map(|risk| &risk.summary))
            .take(8)
            .cloned()
            .collect(),
    });
    stages.push(PlanReviewStage {
        stage: "coobie_critique".to_string(),
        owner: "coobie".to_string(),
        summary: format!(
            "Coobie required {} guardrail(s), {} check(s), and {} open question(s) before '{}' proceeds.",
            briefing.recommended_guardrails.len(),
            briefing.required_checks.len(),
            briefing.open_questions.len(),
            target_source.label
        ),
        evidence: briefing
            .recommended_guardrails
            .iter()
            .chain(briefing.required_checks.iter())
            .chain(briefing.open_questions.iter())
            .take(10)
            .cloned()
            .collect(),
    });
    stages.push(PlanReviewStage {
        stage: "forge_preference_review".to_string(),
        owner: "coobie".to_string(),
        summary: if briefing.preferred_forge_commands.is_empty() {
            "Coobie found no previously successful bounded command paths strong enough to bias the forge plan yet.".to_string()
        } else {
            format!(
                "Coobie prefers {} previously successful bounded command path(s) for '{}' before the forge invents a new route.",
                briefing.preferred_forge_commands.len(),
                target_source.label
            )
        },
        evidence: if briefing.preferred_forge_commands.is_empty() {
            vec!["No preferred retriever-forge commands recovered from prior successful runs.".to_string()]
        } else {
            briefing.preferred_forge_commands.iter().take(5).cloned().collect()
        },
    });
    stages.push(PlanReviewStage {
        stage: "final_execution_plan".to_string(),
        owner: "mason".to_string(),
        summary: format!(
            "Bounded execution plan for spec '{}' against '{}'.",
            spec_obj.id, target_source.label
        ),
        evidence: implementation_plan
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(12)
            .map(ToString::to_string)
            .collect(),
    });

    let final_execution_plan = implementation_plan
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && (line.starts_with("- ")
                    || line.chars().next().is_some_and(|ch| ch.is_ascii_digit()))
        })
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    PlanReviewChainArtifact {
        run_id: run_id.to_string(),
        spec_id: spec_obj.id.clone(),
        product: target_source.label.clone(),
        generated_at: Utc::now().to_rfc3339(),
        stages,
        final_execution_plan,
    }
}

fn render_worker_task_envelope_markdown(envelope: &WorkerTaskEnvelope) -> String {
    format!(
        "# Retriever Task Packet

- Job: {}
- Spec: {}
- Product: {}
- Adapter: {}
- Profile: {}
- Target source: {}
- Staged workspace: {}
- Max iterations: {}
- Continuity file: {}

## Allowed Paths
{}

## Denied Paths
{}

## Visible Success Conditions
{}

## Return Artifacts
{}

## Repo-Local Context Notes
{}

## Repo-Local Context Paths
{}

## Repo-Local Skill Paths
{}

## Trail Drift Guard Artifact
{}

## Query Terms
{}

## Preferred Commands
{}

## Guardrails
{}

## Required Checks
{}

## LLM Edits
- Enabled: {}

## Editable Paths
{}
",
        envelope.job_id,
        envelope.spec_id,
        envelope.product,
        envelope.adapter,
        envelope.profile,
        envelope.target_source,
        envelope.staged_workspace,
        envelope.max_iterations,
        envelope
            .continuity_file
            .clone()
            .unwrap_or_else(|| "none".to_string()),
        render_list(&envelope.allowed_paths, "No allowed paths recorded."),
        render_list(&envelope.denied_paths, "No denied paths recorded."),
        render_list(
            &envelope.visible_success_conditions,
            "No visible success conditions recorded.",
        ),
        render_list(&envelope.return_artifacts, "No return artifacts recorded."),
        render_list(
            &envelope.repo_local_context_notes,
            "No repo-local context notes were recorded.",
        ),
        render_list(
            &envelope.repo_local_context_paths,
            "No repo-local context paths were attached.",
        ),
        render_list(
            &envelope.repo_local_skill_paths,
            "No repo-local skill paths were attached.",
        ),
        envelope
            .trail_drift_guard_artifact
            .clone()
            .unwrap_or_else(|| "No trail drift guard artifact was attached.".to_string()),
        render_list(&envelope.query_terms, "No query terms recorded."),
        render_list(
            &envelope.preferred_commands,
            "No preferred retriever-forge commands were recorded.",
        ),
        render_list(&envelope.guardrails, "No guardrails recorded."),
        render_list(&envelope.required_checks, "No required checks recorded."),
        if envelope.llm_edits { "true" } else { "false" },
        render_list(&envelope.editable_paths, "No editable paths were resolved."),
    )
}

fn render_plan_review_chain_markdown(chain: &PlanReviewChainArtifact) -> String {
    let stages = chain
        .stages
        .iter()
        .map(|stage| {
            format!(
                "### {}
- owner: {}
- summary: {}
- evidence:
{}",
                stage.stage,
                stage.owner,
                stage.summary,
                render_list(&stage.evidence, "No evidence recorded for this stage."),
            )
        })
        .collect::<Vec<_>>()
        .join(
            "

",
        );
    format!(
        "# Trail Review Chain

- Run: {}
- Spec: {}
- Product: {}
- Generated at: {}

## Review Stages
{}

## Final Execution Plan
{}
",
        chain.run_id,
        chain.spec_id,
        chain.product,
        chain.generated_at,
        stages,
        render_list(
            &chain.final_execution_plan,
            "No final execution plan steps were captured.",
        ),
    )
}

fn fallback_worker_value(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_retriever_command_plan(
    spec_obj: &Spec,
    staged_product: &Path,
    packet: &WorkerTaskEnvelope,
) -> Result<Vec<RetrieverPlannedCommand>> {
    let mut commands = Vec::new();
    for raw_cmd in &spec_obj.test_commands {
        let trimmed = raw_cmd.trim();
        if trimmed.is_empty() {
            continue;
        }
        commands.push(RetrieverPlannedCommand {
            label: trimmed.to_string(),
            raw_command: trimmed.to_string(),
            source: "spec.test_commands".to_string(),
            rationale: "The spec declared this command as visible execution evidence for the run."
                .to_string(),
        });
    }
    if !commands.is_empty() {
        return Ok(apply_preferred_retriever_command_order(
            commands,
            &packet.preferred_commands,
        ));
    }

    let cargo_manifest = staged_product.join("Cargo.toml");
    let package_json = staged_product.join("package.json");
    let pyproject_toml = staged_product.join("pyproject.toml");
    let requirements_txt = staged_product.join("requirements.txt");
    let go_mod = staged_product.join("go.mod");

    if cargo_manifest.exists() {
        commands.push(RetrieverPlannedCommand {
            label: "cargo check --quiet".to_string(),
            raw_command: "cargo check --quiet".to_string(),
            source: "manifest_inference".to_string(),
            rationale: "Cargo manifest detected; a lightweight Rust compile check is the safest bounded forge default.".to_string(),
        });
    } else if package_json.exists() {
        if let Some((program, args, label)) = detect_node_bootstrap(staged_product) {
            commands.push(RetrieverPlannedCommand {
                label: label.clone(),
                raw_command: if args.is_empty() {
                    program.to_string()
                } else {
                    format!("{} {}", program, args.join(" "))
                },
                source: "manifest_inference".to_string(),
                rationale: "The Node workspace needs dependency/bootstrap preparation before bounded visible execution can continue.".to_string(),
            });
        }
        let scripts = detect_package_scripts(&package_json)?;
        if scripts.contains(&"build".to_string()) {
            if command_available("npm") {
                commands.push(RetrieverPlannedCommand {
                    label: "npm run build".to_string(),
                    raw_command: "npm run build".to_string(),
                    source: "manifest_inference".to_string(),
                    rationale: "package.json declares a build script, so the forge uses that as visible execution evidence.".to_string(),
                });
            } else if staged_product.join("pnpm-lock.yaml").exists() && command_available("pnpm") {
                commands.push(RetrieverPlannedCommand {
                    label: "pnpm build".to_string(),
                    raw_command: "pnpm build".to_string(),
                    source: "manifest_inference".to_string(),
                    rationale: "pnpm lockfile detected; the forge uses pnpm build as visible execution evidence.".to_string(),
                });
            } else if staged_product.join("yarn.lock").exists() && command_available("yarn") {
                commands.push(RetrieverPlannedCommand {
                    label: "yarn build".to_string(),
                    raw_command: "yarn build".to_string(),
                    source: "manifest_inference".to_string(),
                    rationale: "yarn lockfile detected; the forge uses yarn build as visible execution evidence.".to_string(),
                });
            }
        } else if scripts.contains(&"test".to_string()) {
            if command_available("npm") {
                commands.push(RetrieverPlannedCommand {
                    label: "npm run test".to_string(),
                    raw_command: "npm run test".to_string(),
                    source: "manifest_inference".to_string(),
                    rationale: "package.json declares a test script, so the forge uses that as visible execution evidence.".to_string(),
                });
            } else if staged_product.join("pnpm-lock.yaml").exists() && command_available("pnpm") {
                commands.push(RetrieverPlannedCommand {
                    label: "pnpm test".to_string(),
                    raw_command: "pnpm test".to_string(),
                    source: "manifest_inference".to_string(),
                    rationale: "pnpm lockfile detected; the forge uses pnpm test as visible execution evidence.".to_string(),
                });
            } else if staged_product.join("yarn.lock").exists() && command_available("yarn") {
                commands.push(RetrieverPlannedCommand {
                    label: "yarn test".to_string(),
                    raw_command: "yarn test".to_string(),
                    source: "manifest_inference".to_string(),
                    rationale: "yarn lockfile detected; the forge uses yarn test as visible execution evidence.".to_string(),
                });
            }
        }
    } else if go_mod.exists() && command_available("go") {
        commands.push(RetrieverPlannedCommand {
            label: "go test ./...".to_string(),
            raw_command: "go test ./...".to_string(),
            source: "manifest_inference".to_string(),
            rationale:
                "go.mod detected; the forge uses go test as bounded visible execution evidence."
                    .to_string(),
        });
    } else if pyproject_toml.exists() || requirements_txt.exists() {
        if let Some(python_command) = detect_python_command() {
            let run_pytest = staged_product.join("tests").exists()
                || pyproject_mentions_pytest(&pyproject_toml)?;
            let raw_command = if run_pytest {
                if command_available("pytest") {
                    "pytest -q".to_string()
                } else {
                    format!("{} -m pytest -q", python_command)
                }
            } else {
                format!("{} -m compileall .", python_command)
            };
            commands.push(RetrieverPlannedCommand {
                label: raw_command.clone(),
                raw_command,
                source: "manifest_inference".to_string(),
                rationale: "Python project detected; the forge uses pytest or compileall as bounded visible execution evidence.".to_string(),
            });
        }
    }

    Ok(apply_preferred_retriever_command_order(
        commands,
        &packet.preferred_commands,
    ))
}

fn apply_preferred_retriever_command_order(
    mut commands: Vec<RetrieverPlannedCommand>,
    preferred_commands: &[String],
) -> Vec<RetrieverPlannedCommand> {
    if commands.is_empty() || preferred_commands.is_empty() {
        return commands;
    }

    let preferred_positions = preferred_commands
        .iter()
        .enumerate()
        .map(|(idx, command)| (command.as_str(), idx))
        .collect::<HashMap<_, _>>();

    commands.sort_by(|left, right| {
        let left_rank = preferred_positions
            .get(left.raw_command.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        let right_rank = preferred_positions
            .get(right.raw_command.as_str())
            .copied()
            .unwrap_or(usize::MAX);
        left_rank
            .cmp(&right_rank)
            .then_with(|| left.label.cmp(&right.label))
    });

    for command in &mut commands {
        if let Some(rank) = preferred_positions.get(command.raw_command.as_str()) {
            command.rationale = format!(
                "{} Preferred because Coobie saw this bounded command path succeed in prior similar forge runs (preference rank {}).",
                command.rationale,
                rank + 1
            );
            if command.source == "manifest_inference" {
                command.source = "manifest_inference+forge_memory".to_string();
            }
        }
    }

    commands
}

fn evaluate_retriever_hook(
    packet: &WorkerTaskEnvelope,
    planned: &RetrieverPlannedCommand,
) -> (String, Vec<String>) {
    let raw = planned.raw_command.to_lowercase();
    let mut reasons = Vec::new();

    for denied in &packet.denied_paths {
        let normalized = denied.replace('\\', "/").to_lowercase();
        if !normalized.is_empty() && raw.contains(&normalized) {
            reasons.push(format!("Command references denied path {}.", denied));
        }
    }

    for forbidden in [
        "factory/scenarios/hidden",
        "factory/memory",
        ".harkonnen/project-memory",
        "git reset --hard",
        "git checkout --",
        "rm -rf /",
    ] {
        if raw.contains(forbidden) {
            reasons.push(format!(
                "Command matched forbidden pattern '{}'.",
                forbidden
            ));
        }
    }

    if reasons.is_empty() {
        (
            "allow".to_string(),
            vec!["Command stayed within the bounded forge policy surface.".to_string()],
        )
    } else {
        ("deny".to_string(), reasons)
    }
}

fn render_retriever_hooks_markdown(artifact: &RetrieverHookArtifact) -> String {
    let sections = if artifact.records.is_empty() {
        "No forge hook records were captured.".to_string()
    } else {
        artifact
            .records
            .iter()
            .map(|record| {
                format!(
                    "### {} :: {}
- decision: {}
- tool: {}
- command: {}
- source: {}
- rationale: {}
- reasons: {}
- passed: {}
- exit_code: {}
- log: {}
- created_at: {}",
                    record.stage,
                    record.command_label,
                    record.decision,
                    record.tool,
                    record.raw_command,
                    record.source,
                    record.rationale,
                    if record.reasons.is_empty() {
                        "none".to_string()
                    } else {
                        record.reasons.join(" | ")
                    },
                    record
                        .passed
                        .map(|value| if value { "true" } else { "false" }.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    record
                        .exit_code
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    record
                        .log_artifact
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                    record.created_at,
                )
            })
            .collect::<Vec<_>>()
            .join(
                "

",
            )
    };
    format!(
        "# Retriever Forge Hooks

- Run: {}
- Spec: {}
- Product: {}
- Adapter: {}
- Profile: {}
- Generated at: {}

## Records
{}
",
        artifact.run_id,
        artifact.spec_id,
        artifact.product,
        artifact.adapter,
        artifact.profile,
        artifact.generated_at,
        sections,
    )
}

fn render_retriever_execution_markdown(report: &RetrieverExecutionArtifact) -> String {
    let command_sections = if report.executed_commands.is_empty() {
        "No retriever-forge commands were executed.".to_string()
    } else {
        report
            .executed_commands
            .iter()
            .map(|command| {
                format!(
                    "### {}
- source: {}
- rationale: {}
- command: {}
- preferred: {}
- preference_rank: {}
- preference_outcome: {}
- passed: {}
- exit_code: {}
- log: {}",
                    command.label,
                    command.source,
                    command.rationale,
                    command.raw_command,
                    if command.was_preferred {
                        "true"
                    } else {
                        "false"
                    },
                    command
                        .preference_rank
                        .map(|rank| rank.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    command
                        .preference_outcome
                        .clone()
                        .unwrap_or_else(|| "n/a".to_string()),
                    if command.passed { "true" } else { "false" },
                    command
                        .exit_code
                        .map(|code| code.to_string())
                        .unwrap_or_else(|| "signal".to_string()),
                    command.log_artifact,
                )
            })
            .collect::<Vec<_>>()
            .join(
                "

",
            )
    };
    format!(
        "# Retriever Execution Report

- Run: {}
- Spec: {}
- Product: {}
- Adapter: {}
- Profile: {}
- Generated at: {}
- Passed: {}
- Task packet artifact: {}
- Review chain artifact: {}
- Dispatch artifact: {}
- Continuity artifact: {}
- Hook artifact: {}
- Preferred commands offered: {}
- Preferred commands selected: {}
- Preferred commands helped: {}
- Preferred commands stale: {}

## Summary
{}

## Commands
{}

## Returned Artifacts
{}
",
        report.run_id,
        report.spec_id,
        report.product,
        report.adapter,
        report.profile,
        report.generated_at,
        if report.passed { "true" } else { "false" },
        report.task_packet_artifact,
        report.review_chain_artifact,
        report.dispatch_artifact,
        report.continuity_artifact,
        report.hook_artifact,
        render_list(
            &report.preferred_commands_offered,
            "No preferred commands were offered."
        ),
        render_list(
            &report.preferred_commands_selected,
            "No preferred commands were selected."
        ),
        render_list(
            &report.preferred_commands_helped,
            "No preferred commands helped in this run."
        ),
        render_list(
            &report.preferred_commands_stale,
            "No preferred commands went stale in this run."
        ),
        report.summary,
        command_sections,
        render_list(&report.returned_artifacts, "No artifacts were returned."),
    )
}

fn render_mason_edit_proposal_markdown(proposal: &MasonEditProposalArtifact) -> String {
    let edit_sections = if proposal.edits.is_empty() {
        "No edits were proposed.".to_string()
    } else {
        proposal
            .edits
            .iter()
            .map(|edit| {
                format!(
                    "### {}
- action: {}
- summary: {}
- content_bytes: {}",
                    edit.path,
                    edit.action,
                    edit.summary,
                    edit.content.len()
                )
            })
            .collect::<Vec<_>>()
            .join(
                "

",
            )
    };

    format!(
        "# Mason Edit Proposal

- Run: {}
- Spec: {}
- Product: {}
- Generated at: {}

## Summary
{}

## Rationale
{}

## Editable Paths
{}

## Context Paths
{}

## Edits
{}
",
        proposal.run_id,
        proposal.spec_id,
        proposal.product,
        proposal.generated_at,
        proposal.summary,
        render_list(&proposal.rationale, "No rationale recorded."),
        render_list(&proposal.editable_paths, "No editable paths recorded."),
        render_list(&proposal.context_paths, "No context paths recorded."),
        edit_sections,
    )
}

fn render_mason_edit_application_markdown(application: &MasonEditApplicationArtifact) -> String {
    format!(
        "# Mason Edit Application

- Run: {}
- Spec: {}
- Product: {}
- Generated at: {}
- Status: {}
- Proposal generated: {}

## Summary
{}

## Changed Files
{}
",
        application.run_id,
        application.spec_id,
        application.product,
        application.generated_at,
        application.status,
        if application.proposal_generated {
            "true"
        } else {
            "false"
        },
        application.summary,
        render_list(&application.changed_files, "No files changed."),
    )
}

fn render_retriever_dispatch_markdown(dispatch: &RetrieverDispatchArtifact) -> String {
    format!(
        "# Retriever Dispatch

- Run: {}
- Spec: {}
- Product: {}
- Adapter: {}
- Profile: {}
- Generated at: {}
- Task packet artifact: {}
- Review chain artifact: {}
- Context bundle artifact: {}
- Trail drift guard artifact: {}
- Continuity artifact: {}

## Dispatch Summary
{}

## Constraints Applied
{}

## Next Actions
{}

## Visible Success Conditions
{}

## Return Artifacts
{}
",
        dispatch.run_id,
        dispatch.spec_id,
        dispatch.product,
        dispatch.adapter,
        dispatch.profile,
        dispatch.generated_at,
        dispatch.task_packet_artifact,
        dispatch.review_chain_artifact,
        dispatch.context_bundle_artifact,
        dispatch.trail_drift_guard_artifact,
        dispatch.continuity_artifact,
        dispatch.dispatch_summary,
        render_list(
            &dispatch.constraints_applied,
            "No constraints were captured."
        ),
        render_list(&dispatch.next_actions, "No next actions were captured."),
        render_list(
            &dispatch.visible_success_conditions,
            "No visible success conditions were captured.",
        ),
        render_list(
            &dispatch.return_artifacts,
            "No return artifacts were captured."
        ),
    )
}

fn build_retriever_context_bundle(
    run_id: &str,
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    staged_product: &Path,
    query_terms: &[String],
) -> Result<RetrieverContextBundleArtifact> {
    let harkonnen_dir = staged_product.join(".harkonnen");
    let (context_entries, skill_entries) = discover_repo_local_context_entries(
        &harkonnen_dir,
        Some(target_source),
        Some(spec_obj),
        query_terms,
    )?;
    let preload_notes =
        build_repo_local_preload_notes(&context_entries, &skill_entries, target_source);
    Ok(RetrieverContextBundleArtifact {
        run_id: run_id.to_string(),
        spec_id: spec_obj.id.clone(),
        product: target_source.label.clone(),
        generated_at: Utc::now().to_rfc3339(),
        project_root: staged_product.display().to_string(),
        context_entries,
        skill_entries,
        preload_notes,
    })
}

fn discover_repo_local_context_entries(
    harkonnen_dir: &Path,
    target_source: Option<&TargetSourceMetadata>,
    spec_obj: Option<&Spec>,
    query_terms: &[String],
) -> Result<(Vec<RepoLocalContextEntry>, Vec<RepoLocalContextEntry>)> {
    let mut paths = Vec::new();
    for name in [
        "project-context.md",
        "project-scan.md",
        "resume-packet.md",
        "strategy-register.md",
        "memory-status.md",
        "stale-memory-history.md",
        "instructions.md",
        "launch-guide.md",
    ] {
        let path = harkonnen_dir.join(name);
        if path.exists() {
            paths.push(path);
        }
    }
    collect_markdown_paths(&harkonnen_dir.join("contexts"), &mut paths)?;
    collect_markdown_paths(&harkonnen_dir.join("skills"), &mut paths)?;

    let mut entries = Vec::new();
    for path in paths {
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let summary = summarize_repo_local_document(&raw);
        let rel = path
            .strip_prefix(harkonnen_dir.parent().unwrap_or(harkonnen_dir))
            .unwrap_or(&path)
            .display()
            .to_string();
        let lower_rel = rel.to_lowercase();
        let category = if lower_rel.contains("/skills/") {
            "skill".to_string()
        } else if lower_rel.ends_with("instructions.md") {
            "instruction".to_string()
        } else {
            "context".to_string()
        };
        let scope = if lower_rel.contains("/skills/") {
            "skills".to_string()
        } else if lower_rel.contains("/contexts/") {
            "contexts".to_string()
        } else {
            "project".to_string()
        };
        let relevance = score_repo_local_document(&rel, &raw, target_source, spec_obj, query_terms);
        entries.push(RepoLocalContextEntry {
            label: path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("context")
                .replace('-', " "),
            path: path.display().to_string(),
            category,
            scope,
            summary,
            relevance,
        });
    }

    entries.sort_by(|left, right| {
        right
            .relevance
            .cmp(&left.relevance)
            .then_with(|| left.path.cmp(&right.path))
    });
    let mut context_entries = Vec::new();
    let mut skill_entries = Vec::new();
    for entry in entries {
        if entry.category == "skill" {
            if skill_entries.len() < 8 {
                skill_entries.push(entry);
            }
        } else if context_entries.len() < 12 {
            context_entries.push(entry);
        }
    }
    Ok((context_entries, skill_entries))
}

fn collect_markdown_paths(root: &Path, acc: &mut Vec<PathBuf>) -> Result<()> {
    if !root.exists() || !root.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_paths(&path, acc)?;
            continue;
        }
        let is_md = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if is_md && !acc.iter().any(|existing| existing == &path) {
            acc.push(path);
        }
    }
    Ok(())
}

fn summarize_repo_local_document(raw: &str) -> String {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "---" {
            continue;
        }
        return trimmed.chars().take(220).collect();
    }
    raw.trim().chars().take(220).collect()
}

fn score_repo_local_document(
    rel_path: &str,
    raw: &str,
    target_source: Option<&TargetSourceMetadata>,
    spec_obj: Option<&Spec>,
    query_terms: &[String],
) -> i32 {
    let mut score = 0;
    let haystack = format!(
        "{}
{}",
        rel_path.to_lowercase(),
        raw.to_lowercase()
    );
    if rel_path.ends_with("instructions.md") {
        score += 40;
    }
    for (name, boost) in [
        ("project-context.md", 36),
        ("project-scan.md", 32),
        ("resume-packet.md", 28),
        ("strategy-register.md", 24),
        ("memory-status.md", 22),
        ("stale-memory-history.md", 20),
    ] {
        if rel_path.ends_with(name) {
            score += boost;
        }
    }
    if rel_path.contains("/contexts/") {
        score += 14;
    }
    if rel_path.contains("/skills/") {
        score += 12;
    }
    for term in query_terms {
        let needle = term.trim().to_lowercase();
        if needle.len() >= 3 && haystack.contains(&needle) {
            score += 6;
        }
    }
    if let Some(target_source) = target_source {
        let label = target_source.label.to_lowercase();
        if !label.is_empty() && haystack.contains(&label) {
            score += 6;
        }
    }
    if let Some(spec_obj) = spec_obj {
        for component in &spec_obj.project_components {
            for value in [&component.name, &component.role, &component.kind] {
                let needle = value.trim().to_lowercase();
                if needle.len() >= 3 && haystack.contains(&needle) {
                    score += 5;
                }
            }
        }
        if let Some(blueprint) = &spec_obj.scenario_blueprint {
            for value in blueprint
                .code_under_test
                .iter()
                .chain(blueprint.runtime_surfaces.iter())
                .chain(blueprint.coobie_memory_topics.iter())
            {
                let needle = value.trim().to_lowercase();
                if needle.len() >= 3 && haystack.contains(&needle) {
                    score += 4;
                }
            }
        }
    }
    score
}

fn build_repo_local_preload_notes(
    context_entries: &[RepoLocalContextEntry],
    skill_entries: &[RepoLocalContextEntry],
    target_source: &TargetSourceMetadata,
) -> Vec<String> {
    let mut notes = Vec::new();
    if !context_entries.is_empty() {
        notes.push(format!(
            "Load {} repo-local context file(s) for '{}' before forge execution.",
            context_entries.len(),
            target_source.label
        ));
        if let Some(first) = context_entries.first() {
            notes.push(format!(
                "Start with '{}' because it is the highest-relevance repo-local context document.",
                first.label
            ));
        }
    } else {
        notes.push("No repo-local context files were discovered beyond the default project bootstrap files.".to_string());
    }
    if !skill_entries.is_empty() {
        notes.push(format!(
            "Preload {} repo-local skill bundle(s) so the forge inherits product-specific workflows before inventing new ones.",
            skill_entries.len()
        ));
    } else {
        notes.push("No repo-local skill bundles were discovered yet; add markdown files under `.harkonnen/skills/` for repeatable workflows.".to_string());
    }
    notes
}

fn render_retriever_context_bundle_markdown(bundle: &RetrieverContextBundleArtifact) -> String {
    let context_lines = bundle
        .context_entries
        .iter()
        .map(|entry| {
            format!(
                "- {} [{} | relevance={}] {}",
                entry.label, entry.path, entry.relevance, entry.summary
            )
        })
        .collect::<Vec<_>>();
    let skill_lines = bundle
        .skill_entries
        .iter()
        .map(|entry| {
            format!(
                "- {} [{} | relevance={}] {}",
                entry.label, entry.path, entry.relevance, entry.summary
            )
        })
        .collect::<Vec<_>>();
    format!(
        "# Retriever Context Bundle

- Run: {}
- Spec: {}
- Product: {}
- Generated at: {}
- Project root: {}

## Preload Notes
{}

## Context Entries
{}

## Skill Entries
{}
",
        bundle.run_id,
        bundle.spec_id,
        bundle.product,
        bundle.generated_at,
        bundle.project_root,
        render_list(&bundle.preload_notes, "No preload notes were generated."),
        render_list(
            &context_lines,
            "No repo-local context entries were discovered."
        ),
        render_list(&skill_lines, "No repo-local skill entries were discovered."),
    )
}

fn build_trail_drift_guard(
    run_id: &str,
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    staged_product: &Path,
    context_bundle: &RetrieverContextBundleArtifact,
) -> Result<TrailDriftGuardArtifact> {
    let tracked_entries =
        collect_trail_drift_guard_entries(spec_obj, staged_product, context_bundle)?;
    Ok(TrailDriftGuardArtifact {
        run_id: run_id.to_string(),
        spec_id: spec_obj.id.clone(),
        product: target_source.label.clone(),
        generated_at: Utc::now().to_rfc3339(),
        tracked_entries,
    })
}

fn collect_trail_drift_guard_entries(
    spec_obj: &Spec,
    staged_product: &Path,
    context_bundle: &RetrieverContextBundleArtifact,
) -> Result<Vec<TrailDriftGuardEntry>> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    for rel in collect_spec_code_under_test_paths(spec_obj) {
        let path = staged_product.join(&rel);
        if !path.exists() {
            continue;
        }
        let display = path.display().to_string();
        if seen.insert(display.clone()) {
            entries.push(TrailDriftGuardEntry {
                role: "code_under_test".to_string(),
                path: display,
                fingerprint: fingerprint_trail_drift_target(&path)?,
            });
        }
    }
    for entry in context_bundle
        .context_entries
        .iter()
        .chain(context_bundle.skill_entries.iter())
    {
        let path = PathBuf::from(&entry.path);
        if !path.exists() {
            continue;
        }
        let display = path.display().to_string();
        if seen.insert(display.clone()) {
            entries.push(TrailDriftGuardEntry {
                role: format!("repo_local_{}", entry.category),
                path: display,
                fingerprint: fingerprint_trail_drift_target(&path)?,
            });
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

fn verify_trail_drift_guard(
    run_id: &str,
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    guard: &TrailDriftGuardArtifact,
) -> Result<TrailDriftCheckArtifact> {
    let mut verified_paths = Vec::new();
    let mut changed_paths = Vec::new();
    let mut missing_paths = Vec::new();
    for entry in &guard.tracked_entries {
        let path = Path::new(&entry.path);
        if !path.exists() {
            missing_paths.push(format!("{} [{}]", entry.role, entry.path));
            continue;
        }
        let fingerprint = fingerprint_trail_drift_target(path)?;
        if fingerprint == entry.fingerprint {
            verified_paths.push(format!("{} [{}]", entry.role, entry.path));
        } else {
            changed_paths.push(format!("{} [{}]", entry.role, entry.path));
        }
    }
    let passed = changed_paths.is_empty() && missing_paths.is_empty();
    let summary = if passed {
        format!(
            "Verified {} guarded path(s) without drift before retriever execution.",
            verified_paths.len()
        )
    } else {
        format!(
            "Guarded workspace drift detected: {} changed path(s), {} missing path(s).",
            changed_paths.len(),
            missing_paths.len()
        )
    };
    Ok(TrailDriftCheckArtifact {
        run_id: run_id.to_string(),
        spec_id: spec_obj.id.clone(),
        product: target_source.label.clone(),
        generated_at: Utc::now().to_rfc3339(),
        guard_artifact: "trail_drift_guard.json".to_string(),
        passed,
        summary,
        verified_paths,
        changed_paths,
        missing_paths,
    })
}

fn fingerprint_trail_drift_target(path: &Path) -> Result<String> {
    let mut files = Vec::new();
    collect_paths_for_drift_fingerprint(path, path, &mut files)?;
    let mut hasher = DefaultHasher::new();
    path.display().to_string().hash(&mut hasher);
    for file in files {
        let rel = file
            .strip_prefix(path)
            .or_else(|_| file.strip_prefix(path.parent().unwrap_or(path)))
            .unwrap_or(&file)
            .display()
            .to_string();
        rel.hash(&mut hasher);
        let data = std::fs::read(&file)?;
        data.len().hash(&mut hasher);
        data.hash(&mut hasher);
    }
    Ok(format!("{:016x}", hasher.finish()))
}

fn collect_paths_for_drift_fingerprint(
    root: &Path,
    path: &Path,
    acc: &mut Vec<PathBuf>,
) -> Result<()> {
    if path.is_file() {
        acc.push(path.to_path_buf());
        return Ok(());
    }
    if path.is_dir() {
        let mut children = std::fs::read_dir(path)?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            collect_paths_for_drift_fingerprint(root, &child, acc)?;
        }
        if acc.is_empty() && path == root {
            acc.push(path.to_path_buf());
        }
    }
    Ok(())
}

fn render_trail_drift_guard_markdown(artifact: &TrailDriftGuardArtifact) -> String {
    let lines = artifact
        .tracked_entries
        .iter()
        .map(|entry| {
            format!(
                "- {} [{}] fingerprint={}",
                entry.role, entry.path, entry.fingerprint
            )
        })
        .collect::<Vec<_>>();
    format!(
        "# Trail Drift Guard

- Run: {}
- Spec: {}
- Product: {}
- Generated at: {}

## Tracked Entries
{}
",
        artifact.run_id,
        artifact.spec_id,
        artifact.product,
        artifact.generated_at,
        render_list(&lines, "No guarded paths were recorded."),
    )
}

fn render_trail_drift_check_markdown(artifact: &TrailDriftCheckArtifact) -> String {
    format!(
        "# Trail Drift Check

- Run: {}
- Spec: {}
- Product: {}
- Generated at: {}
- Guard artifact: {}
- Passed: {}

## Summary
{}

## Verified Paths
{}

## Changed Paths
{}

## Missing Paths
{}
",
        artifact.run_id,
        artifact.spec_id,
        artifact.product,
        artifact.generated_at,
        artifact.guard_artifact,
        if artifact.passed { "true" } else { "false" },
        artifact.summary,
        render_list(&artifact.verified_paths, "No guarded paths were verified."),
        render_list(&artifact.changed_paths, "No guarded paths changed."),
        render_list(&artifact.missing_paths, "No guarded paths went missing."),
    )
}

fn render_project_resume_packet_markdown(packet: &ProjectResumePacket) -> String {
    let risk_lines = if packet.stale_memory.is_empty() {
        "- No project-memory entries are currently flagged as stale or contradicted.".to_string()
    } else {
        packet
            .stale_memory
            .iter()
            .map(|risk| {
                format!(
                    "- {} [{} | severity={} score={}] {}",
                    risk.memory_id,
                    risk.status.clone().unwrap_or_else(|| "review".to_string()),
                    risk.severity,
                    risk.severity_score,
                    if risk.reasons.is_empty() {
                        "no reasons recorded".to_string()
                    } else {
                        risk.reasons.join(" | ")
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Resume Packet\n\n- Generated at: {}\n- Project: {}\n- Current git: {}\n\n## Summary\n{}\n\n## Project Memory At Risk\n{}\n",
        packet.generated_at,
        packet.label,
        render_target_git_summary(packet.current_git.as_ref()),
        render_list(&packet.summary, "No resume summary generated yet."),
        risk_lines,
    )
}

fn render_evidence_time_range_summary(time_range: Option<&EvidenceTimeRange>) -> String {
    match time_range {
        Some(range) => {
            if let (Some(start), Some(end)) = (range.start_ms, range.end_ms) {
                return format!("{}..{} ms", start, end);
            }
            if let (Some(start), Some(end)) = (range.start_iso.as_deref(), range.end_iso.as_deref())
            {
                return format!("{}..{}", start, end);
            }
            if let Some(start) = range.start_ms {
                return format!("start={} ms", start);
            }
            if let Some(end) = range.end_ms {
                return format!("end={} ms", end);
            }
            if let Some(start) = range.start_iso.as_deref() {
                return format!("start={}", start);
            }
            if let Some(end) = range.end_iso.as_deref() {
                return format!("end={}", end);
            }
            "unspecified".to_string()
        }
        None => "unspecified".to_string(),
    }
}

fn annotation_time_span_ms(time_range: Option<&EvidenceTimeRange>) -> Option<i64> {
    let range = time_range?;
    match (range.start_ms, range.end_ms) {
        (Some(start), Some(end)) if end >= start => Some(end - start),
        _ => None,
    }
}

fn overlap_bonus(needles: &[String], haystack: &[String], per_match: i32) -> i32 {
    let mut score = 0;
    for needle in needles {
        if haystack
            .iter()
            .any(|candidate| candidate.contains(needle) || needle.contains(candidate))
        {
            score += per_match;
        }
    }
    score
}

fn time_span_similarity_bonus(target_span: i64, candidate_span: i64) -> i32 {
    if target_span <= 0 || candidate_span <= 0 {
        return 0;
    }
    let delta = (target_span - candidate_span).abs() as f64;
    let base = target_span.max(candidate_span) as f64;
    let ratio = delta / base;
    if ratio <= 0.10 {
        20
    } else if ratio <= 0.25 {
        12
    } else if ratio <= 0.50 {
        6
    } else {
        0
    }
}

fn collect_overlapping_terms(needles: &[String], haystack: &[String]) -> Vec<String> {
    let mut matches = Vec::new();
    for needle in needles {
        if haystack
            .iter()
            .any(|candidate| candidate.contains(needle) || needle.contains(candidate))
        {
            push_unique(&mut matches, needle);
        }
    }
    matches
}

fn classify_evidence_match(window: &EvidenceWindowMatch) -> String {
    if window.score >= 90
        || (!window.matched_claims.is_empty() && window.matched_labels.len() >= 2)
        || (!window.matched_sources.is_empty() && window.matched_labels.len() >= 2)
    {
        "match".to_string()
    } else if window.score >= 45
        || !window.matched_labels.is_empty()
        || !window.matched_claims.is_empty()
    {
        "near_match".to_string()
    } else {
        "mismatch".to_string()
    }
}

fn confidence_from_match_score(score: i32) -> f64 {
    ((score as f64) / 120.0).clamp(0.0, 1.0)
}

fn build_evidence_match_rationale(window: &EvidenceWindowMatch, match_type: &str) -> Vec<String> {
    let mut rationale = Vec::new();
    if !window.matched_labels.is_empty() {
        rationale.push(format!(
            "matched labels: {}",
            window.matched_labels.join(", ")
        ));
    }
    if !window.matched_claims.is_empty() {
        rationale.push(format!(
            "matched claims: {}",
            window.matched_claims.join(" | ")
        ));
    }
    if !window.matched_sources.is_empty() {
        rationale.push(format!(
            "matched sources: {}",
            window.matched_sources.join(" | ")
        ));
    }
    if let Some(delta) = window.time_span_delta_ms {
        rationale.push(format!("time-span delta: {} ms", delta));
    }
    if rationale.is_empty() {
        rationale.push("match classification is based on broad query overlap only".to_string());
    }
    rationale.push(format!(
        "classified as {} from similarity score {}",
        match_type, window.score
    ));
    rationale
}

fn build_evidence_match_assessment(
    rank: usize,
    window: EvidenceWindowMatch,
) -> EvidenceMatchAssessment {
    let match_type = classify_evidence_match(&window);
    let confidence = confidence_from_match_score(window.score);
    let rationale = build_evidence_match_rationale(&window, &match_type);
    EvidenceMatchAssessment {
        rank,
        match_type,
        score: window.score,
        confidence,
        rationale,
        window,
    }
}

fn parse_citation_field_values(evidence: &str, field: &str) -> Vec<String> {
    let prefix = format!("{}=", field);
    evidence
        .split(';')
        .find_map(|segment| {
            let trimmed = segment.trim();
            trimmed.strip_prefix(&prefix).map(|value| {
                value
                    .split('|')
                    .map(|part| part.trim().to_string())
                    .filter(|part| !part.is_empty() && part != "none")
                    .collect::<Vec<_>>()
            })
        })
        .unwrap_or_default()
}

fn parse_citation_time_span_ms(evidence: &str) -> Option<i64> {
    let raw = evidence
        .split(';')
        .find_map(|segment| segment.trim().strip_prefix("time="))?
        .trim();
    let raw = raw.strip_suffix(" ms").unwrap_or(raw);
    let (start, end) = raw.split_once("..")?;
    let start = start.trim().parse::<i64>().ok()?;
    let end = end.trim().parse::<i64>().ok()?;
    (end >= start).then_some(end - start)
}

fn render_evidence_match_report(report: &EvidenceMatchReport) -> String {
    let mut lines = vec![
        "# Evidence Match Report".to_string(),
        String::new(),
        format!("- Spec: {}", report.spec_id),
        format!("- Product: {}", report.product),
        format!("- Generated: {}", report.generated_at.to_rfc3339()),
    ];
    if !report.summary.is_empty() {
        lines.push(String::new());
        lines.push("## Summary".to_string());
        for item in &report.summary {
            lines.push(format!("- {}", item));
        }
    }
    lines.push(String::new());
    lines.push("## Assessments".to_string());
    if report.assessments.is_empty() {
        lines.push("- No reviewed evidence windows matched the current context.".to_string());
        return lines.join("\n");
    }
    for assessment in &report.assessments {
        lines.push(format!(
            "- #{} {} [{}] score={} confidence={:.0}%",
            assessment.rank,
            assessment.window.title,
            assessment.match_type,
            assessment.score,
            assessment.confidence * 100.0
        ));
        lines.push(format!(
            "  scenario={} dataset={} time={}",
            assessment.window.scenario, assessment.window.dataset, assessment.window.time_summary
        ));
        if !assessment.rationale.is_empty() {
            lines.push(format!("  rationale={}", assessment.rationale.join(" | ")));
        }
    }
    lines.join("\n")
}

fn parse_evidence_bundle_text(raw: &str) -> Result<EvidenceAnnotationBundle> {
    serde_yaml::from_str(raw)
        .or_else(|_| serde_json::from_str(raw))
        .context("failed to parse evidence bundle")
}

fn validate_evidence_bundle(bundle: &EvidenceAnnotationBundle) -> Result<()> {
    if bundle.schema_version == 0 {
        bail!("schema_version must be >= 1");
    }
    let source_ids = bundle
        .sources
        .iter()
        .map(|source| source.source_id.trim())
        .collect::<HashSet<_>>();
    for annotation in &bundle.annotations {
        if annotation.annotation_id.trim().is_empty() {
            bail!("annotation_id cannot be empty");
        }
        let anchor_ids = annotation
            .anchors
            .iter()
            .map(|anchor| anchor.anchor_id.as_str())
            .collect::<HashSet<_>>();
        for source_id in &annotation.source_ids {
            if !source_ids.contains(source_id.trim()) {
                bail!(
                    "annotation '{}' references unknown source '{}'",
                    annotation.annotation_id,
                    source_id
                );
            }
        }
        for anchor in &annotation.anchors {
            if anchor.anchor_id.trim().is_empty() {
                bail!(
                    "annotation '{}' has anchor with empty anchor_id",
                    annotation.annotation_id
                );
            }
            if !source_ids.contains(anchor.source_id.trim()) {
                bail!(
                    "annotation '{}' anchor '{}' references unknown source '{}'",
                    annotation.annotation_id,
                    anchor.anchor_id,
                    anchor.source_id
                );
            }
        }
        for claim in &annotation.claims {
            if claim.claim_id.trim().is_empty() {
                bail!(
                    "annotation '{}' has claim with empty claim_id",
                    annotation.annotation_id
                );
            }
            for anchor_id in &claim.evidence_anchor_ids {
                if !anchor_ids.contains(anchor_id.as_str()) {
                    bail!(
                        "annotation '{}' claim '{}' references unknown anchor '{}'",
                        annotation.annotation_id,
                        claim.claim_id,
                        anchor_id
                    );
                }
            }
        }
    }
    Ok(())
}

fn normalize_evidence_bundle_name(bundle_name: &str) -> Result<String> {
    let trimmed = bundle_name.trim();
    if trimmed.is_empty() {
        bail!("bundle_name cannot be empty");
    }
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("..") {
        bail!("bundle_name must be a plain filename");
    }
    let mut normalized = trimmed.to_string();
    let lower = normalized.to_ascii_lowercase();
    if !lower.ends_with(".yaml") && !lower.ends_with(".yml") && !lower.ends_with(".json") {
        normalized.push_str(".yaml");
    }
    Ok(normalized)
}

fn normalize_annotation_review_status(status: &str) -> Result<&'static str> {
    match status.trim().to_ascii_lowercase().as_str() {
        "draft" => Ok("draft"),
        "reviewed" => Ok("reviewed"),
        "approved" => Ok("approved"),
        other => bail!("unsupported evidence annotation status '{}'", other),
    }
}

fn append_annotation_note(notes: &mut String, entry: &str) {
    let trimmed = entry.trim();
    if trimmed.is_empty() {
        return;
    }
    if notes.trim().is_empty() {
        notes.push_str(trimmed);
        return;
    }
    if notes.contains(trimmed) {
        return;
    }
    notes.push_str("\n\n");
    notes.push_str(trimmed);
}

fn effective_annotation_status(annotation: &EvidenceAnnotation) -> String {
    let status = annotation.status.trim();
    if status.is_empty() {
        "draft".to_string()
    } else {
        status.to_string()
    }
}

fn annotation_history_actor(annotation: &EvidenceAnnotation) -> String {
    if !annotation.reviewed_by.trim().is_empty() {
        annotation.reviewed_by.trim().to_string()
    } else if !annotation.created_by.trim().is_empty() {
        annotation.created_by.trim().to_string()
    } else {
        "unknown".to_string()
    }
}

fn build_annotation_history_event(
    bundle_name: &str,
    annotation: &EvidenceAnnotation,
    event_type: &str,
    previous_status: Option<String>,
    actor: Option<String>,
    note: Option<String>,
    promoted_ids: Vec<String>,
) -> EvidenceAnnotationHistoryEvent {
    EvidenceAnnotationHistoryEvent {
        event_id: format!("eah_{}", Uuid::new_v4().simple()),
        bundle_name: normalize_evidence_bundle_name(bundle_name)
            .unwrap_or_else(|_| bundle_name.trim().to_string()),
        annotation_id: annotation.annotation_id.clone(),
        annotation_title: annotation.title.clone(),
        event_type: event_type.to_string(),
        status: effective_annotation_status(annotation),
        previous_status: previous_status.unwrap_or_default(),
        actor: actor
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| annotation_history_actor(annotation)),
        note: note.unwrap_or_default(),
        promoted_ids,
        occurred_at: if !annotation.updated_at.trim().is_empty() {
            annotation.updated_at.clone()
        } else if !annotation.reviewed_at.trim().is_empty() {
            annotation.reviewed_at.clone()
        } else if !annotation.created_at.trim().is_empty() {
            annotation.created_at.clone()
        } else {
            Utc::now().to_rfc3339()
        },
    }
}

fn annotations_equal(left: &EvidenceAnnotation, right: &EvidenceAnnotation) -> bool {
    serde_json::to_string(left).ok() == serde_json::to_string(right).ok()
}

fn collect_bundle_save_history_events(
    bundle_name: &str,
    previous: Option<&EvidenceAnnotationBundle>,
    current: &EvidenceAnnotationBundle,
) -> Vec<EvidenceAnnotationHistoryEvent> {
    let mut events = Vec::new();
    let previous_map = previous
        .map(|bundle| {
            bundle
                .annotations
                .iter()
                .map(|annotation| (annotation.annotation_id.clone(), annotation.clone()))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    for annotation in &current.annotations {
        match previous_map.get(&annotation.annotation_id) {
            None => events.push(build_annotation_history_event(
                bundle_name,
                annotation,
                "created",
                None,
                Some(annotation_history_actor(annotation)),
                Some("Annotation created through bundle save.".to_string()),
                Vec::new(),
            )),
            Some(previous_annotation) if !annotations_equal(previous_annotation, annotation) => {
                events.push(build_annotation_history_event(
                    bundle_name,
                    annotation,
                    "updated",
                    Some(effective_annotation_status(previous_annotation)),
                    Some(annotation_history_actor(annotation)),
                    Some("Annotation updated through bundle save.".to_string()),
                    Vec::new(),
                ))
            }
            _ => {}
        }
    }

    if let Some(previous) = previous {
        for annotation in &previous.annotations {
            if current
                .annotations
                .iter()
                .any(|candidate| candidate.annotation_id == annotation.annotation_id)
            {
                continue;
            }
            let mut removed = annotation.clone();
            removed.status = "removed".to_string();
            removed.updated_at = Utc::now().to_rfc3339();
            events.push(build_annotation_history_event(
                bundle_name,
                &removed,
                "removed",
                Some(effective_annotation_status(annotation)),
                Some(annotation_history_actor(annotation)),
                Some("Annotation removed through bundle save.".to_string()),
                Vec::new(),
            ));
        }
    }

    events
}

fn annotation_is_review_ready(annotation: &EvidenceAnnotation) -> bool {
    let status = annotation.status.trim().to_ascii_lowercase();
    matches!(status.as_str(), "reviewed" | "approved")
}

fn resolve_evidence_promotion_destination<'a>(
    annotation: &EvidenceAnnotation,
    scope: &'a str,
) -> Option<&'a str> {
    match scope {
        "project" => Some("project"),
        "core" => Some("core"),
        "follow-bundle" => match annotation
            .promote_to_memory
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "project" => Some("project"),
            "core" | "core_candidate" => Some("core"),
            _ => None,
        },
        _ => None,
    }
}

fn build_evidence_memory_id(
    bundle: &EvidenceAnnotationBundle,
    annotation: &EvidenceAnnotation,
) -> String {
    format!(
        "evidence-{}-{}",
        slugify_memory_fragment(&bundle.project),
        slugify_memory_fragment(&annotation.annotation_id)
    )
}

fn slugify_memory_fragment(value: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn build_evidence_memory_summary(
    bundle: &EvidenceAnnotationBundle,
    annotation: &EvidenceAnnotation,
) -> String {
    let title = if annotation.title.trim().is_empty() {
        annotation.annotation_id.as_str()
    } else {
        annotation.title.trim()
    };
    if bundle.scenario.trim().is_empty() {
        format!("Causal evidence: {}", title)
    } else {
        format!("Causal evidence for {}: {}", bundle.scenario.trim(), title)
    }
}

fn build_evidence_memory_tags(
    bundle: &EvidenceAnnotationBundle,
    annotation: &EvidenceAnnotation,
    destination: &str,
) -> Vec<String> {
    let mut tags = vec![
        "evidence".to_string(),
        "causal".to_string(),
        "causal-evidence".to_string(),
        annotation.annotation_type.trim().to_ascii_lowercase(),
        destination.to_string() + "-memory",
    ];
    if !bundle.dataset.trim().is_empty() {
        tags.push(slugify_memory_fragment(&bundle.dataset));
    }
    for value in annotation.labels.iter().chain(annotation.tags.iter()) {
        let slug = slugify_memory_fragment(value);
        if !slug.is_empty() {
            tags.push(slug);
        }
    }
    tags.sort();
    tags.dedup();
    tags
}

fn collect_evidence_source_uris(
    bundle: &EvidenceAnnotationBundle,
    annotation: &EvidenceAnnotation,
) -> Vec<String> {
    let mut paths = Vec::new();
    for source in &bundle.sources {
        if annotation
            .source_ids
            .iter()
            .any(|id| id == &source.source_id)
            && !source.uri.trim().is_empty()
        {
            let normalized = normalize_project_path(&source.uri);
            if !normalized.is_empty() && !paths.iter().any(|existing| existing == &normalized) {
                paths.push(normalized);
            }
        }
    }
    paths
}

fn render_evidence_memory_body(
    bundle_path: &Path,
    bundle: &EvidenceAnnotationBundle,
    annotation: &EvidenceAnnotation,
) -> String {
    let source_lines = bundle
        .sources
        .iter()
        .filter(|source| {
            annotation
                .source_ids
                .iter()
                .any(|id| id == &source.source_id)
        })
        .map(render_evidence_source_line)
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let anchor_lines = annotation
        .anchors
        .iter()
        .map(|anchor| {
            format!(
                "- {} [{}] source={} signal_keys={} frame_index={:?} timestamp_ms={:?}",
                anchor.anchor_id,
                anchor.kind,
                anchor.source_id,
                if anchor.signal_keys.is_empty() {
                    "none".to_string()
                } else {
                    anchor.signal_keys.join(", ")
                },
                anchor.frame_index,
                anchor.timestamp_ms
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let claim_lines = annotation
        .claims
        .iter()
        .map(|claim| {
            format!(
                "- {}: {} -> {} (confidence={}) anchors={} notes={}",
                claim.relation,
                claim.cause,
                claim.effect,
                claim
                    .confidence
                    .map(|value| format!("{value:.2}"))
                    .unwrap_or_else(|| "unknown".to_string()),
                if claim.evidence_anchor_ids.is_empty() {
                    "none".to_string()
                } else {
                    claim.evidence_anchor_ids.join(", ")
                },
                if claim.notes.trim().is_empty() {
                    "none".to_string()
                } else {
                    claim.notes.trim().to_string()
                }
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    format!(
        "Bundle: {}
Project: {}
Scenario: {}
Dataset: {}
Annotation: {}
Status: {}
Reviewed by: {}
Reviewed at: {}
Promote to memory: {}
Time range: {:?} - {:?} ms

Bundle notes:
{}

Labels: {}
Tags: {}

Sources:
{}

Anchors:
{}

Claims:
{}

Annotation notes:
{}
",
        bundle_path.display(),
        bundle.project,
        if bundle.scenario.trim().is_empty() {
            "n/a"
        } else {
            bundle.scenario.trim()
        },
        if bundle.dataset.trim().is_empty() {
            "n/a"
        } else {
            bundle.dataset.trim()
        },
        annotation.annotation_id,
        if annotation.status.trim().is_empty() {
            "draft"
        } else {
            annotation.status.trim()
        },
        if annotation.reviewed_by.trim().is_empty() {
            "n/a"
        } else {
            annotation.reviewed_by.trim()
        },
        if annotation.reviewed_at.trim().is_empty() {
            "n/a"
        } else {
            annotation.reviewed_at.trim()
        },
        if annotation.promote_to_memory.trim().is_empty() {
            "none"
        } else {
            annotation.promote_to_memory.trim()
        },
        annotation
            .time_range
            .as_ref()
            .and_then(|range| range.start_ms),
        annotation
            .time_range
            .as_ref()
            .and_then(|range| range.end_ms),
        if bundle.notes.is_empty() {
            "- none".to_string()
        } else {
            bundle
                .notes
                .iter()
                .map(|note| format!("- {}", note))
                .collect::<Vec<_>>()
                .join(
                    "
",
                )
        },
        if annotation.labels.is_empty() {
            "none".to_string()
        } else {
            annotation.labels.join(", ")
        },
        if annotation.tags.is_empty() {
            "none".to_string()
        } else {
            annotation.tags.join(", ")
        },
        if source_lines.is_empty() {
            "- none".to_string()
        } else {
            source_lines
        },
        if anchor_lines.is_empty() {
            "- none".to_string()
        } else {
            anchor_lines
        },
        if claim_lines.is_empty() {
            "- none".to_string()
        } else {
            claim_lines
        },
        if annotation.notes.trim().is_empty() {
            "none".to_string()
        } else {
            annotation.notes.trim().to_string()
        },
    )
}

fn render_evidence_source_line(source: &EvidenceSource) -> String {
    format!(
        "- {} [{}] uri={} channels={} tags={}",
        source.source_id,
        source.kind,
        if source.uri.trim().is_empty() {
            "n/a"
        } else {
            source.uri.trim()
        },
        if source.channels.is_empty() {
            "none".to_string()
        } else {
            source.channels.join(", ")
        },
        if source.tags.is_empty() {
            "none".to_string()
        } else {
            source.tags.join(", ")
        },
    )
}

fn build_evidence_memory_provenance(
    bundle_path: &Path,
    bundle: &EvidenceAnnotationBundle,
    annotation: &EvidenceAnnotation,
    target_source: &TargetSourceMetadata,
    source_uris: &[String],
) -> MemoryProvenance {
    let mut observed_paths = vec![normalize_project_path(&bundle_path.display().to_string())];
    for path in source_uris {
        if !observed_paths.iter().any(|existing| existing == path) {
            observed_paths.push(path.clone());
        }
    }
    let mut observed_surfaces = bundle
        .sources
        .iter()
        .filter(|source| {
            annotation
                .source_ids
                .iter()
                .any(|id| id == &source.source_id)
        })
        .map(|source| format!("{}:{}", source.kind, source.label))
        .collect::<Vec<_>>();
    observed_surfaces.sort();
    observed_surfaces.dedup();

    project_memory_provenance(
        target_source,
        None,
        None,
        Vec::new(),
        vec![
            "evidence sources, dataset semantics, or reviewed causal interpretation change"
                .to_string(),
        ],
        observed_paths,
        Vec::new(),
        observed_surfaces,
    )
}

fn project_memory_provenance(
    target_source: &TargetSourceMetadata,
    source_run_id: Option<&str>,
    source_spec_id: Option<&str>,
    evidence_run_ids: Vec<String>,
    stale_when: Vec<String>,
    observed_paths: Vec<String>,
    code_under_test_paths: Vec<String>,
    observed_surfaces: Vec<String>,
) -> MemoryProvenance {
    MemoryProvenance {
        source_label: Some(target_source.label.clone()),
        source_kind: Some(target_source.source_kind.clone()),
        source_path: Some(target_source.source_path.clone()),
        source_run_id: source_run_id.map(|value| value.to_string()),
        source_spec_id: source_spec_id.map(|value| value.to_string()),
        git_branch: target_source
            .git
            .as_ref()
            .and_then(|git| git.branch.clone()),
        git_commit: target_source
            .git
            .as_ref()
            .and_then(|git| git.commit.clone()),
        git_remote: target_source
            .git
            .as_ref()
            .and_then(|git| git.remote_origin.clone()),
        evidence_run_ids,
        stale_when,
        observed_paths,
        code_under_test_paths,
        observed_surfaces,
        status: None,
        superseded_by: None,
        challenged_by: Vec::new(),
    }
}

fn normalize_project_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string()
}

fn parse_git_status_paths(status: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in status.lines() {
        let trimmed = line.trim();
        if trimmed.len() <= 3 {
            continue;
        }
        let raw_path = trimmed[3..].trim();
        let path = raw_path
            .split(" -> ")
            .last()
            .map(normalize_project_path)
            .unwrap_or_default();
        if !path.is_empty() && !paths.iter().any(|existing| existing == &path) {
            paths.push(path);
        }
    }
    paths
}

fn project_paths_overlap(left: &str, right: &str) -> bool {
    let left = normalize_project_path(left);
    let right = normalize_project_path(right);
    left == right
        || left.ends_with(&format!("/{}", right))
        || right.ends_with(&format!("/{}", left))
}

fn intersect_project_paths(left: &[String], right: &[String]) -> Vec<String> {
    let mut matches = Vec::new();
    for candidate in left {
        if right
            .iter()
            .any(|observed| project_paths_overlap(candidate, observed))
        {
            let normalized = normalize_project_path(candidate);
            if !normalized.is_empty() && !matches.iter().any(|existing| existing == &normalized) {
                matches.push(normalized);
            }
        }
    }
    matches
}

fn status_severity_score(status: &str) -> i32 {
    match status {
        "superseded" => 95,
        "challenged" => 70,
        "stale" => 60,
        _ => 45,
    }
}

fn resume_risk_severity(score: i32) -> &'static str {
    match score {
        90..=i32::MAX => "critical",
        65..=89 => "high",
        35..=64 => "medium",
        _ => "low",
    }
}

fn path_impact_score(path: &str) -> i32 {
    let path = normalize_project_path(path).to_ascii_lowercase();
    if path.is_empty() {
        return 0;
    }
    if path.starts_with("src/")
        || path.starts_with("crates/")
        || path.starts_with("backend/")
        || path.starts_with("frontend/")
        || path.starts_with("ui/src/")
        || path.starts_with("apps/")
        || path.starts_with("services/")
        || [
            "rs", "py", "ts", "tsx", "js", "jsx", "go", "java", "cs", "cpp", "c", "h", "hpp",
        ]
        .iter()
        .any(|ext| path.ends_with(&format!(".{}", ext)))
    {
        return 80;
    }
    if path.ends_with("cargo.toml")
        || path.ends_with("package.json")
        || path.ends_with("package-lock.json")
        || path.ends_with("pnpm-lock.yaml")
        || path.ends_with("pyproject.toml")
        || path.ends_with("requirements.txt")
        || path.contains("config")
        || path.contains("schema")
        || path.contains("migration")
    {
        return 60;
    }
    if path.contains("dataset")
        || path.contains("fixtures")
        || ["csv", "jsonl", "parquet", "ndjson"]
            .iter()
            .any(|ext| path.ends_with(&format!(".{}", ext)))
    {
        return 50;
    }
    if path.starts_with("examples/")
        || path.starts_with("docs/")
        || path.ends_with("readme.md")
        || path.ends_with(".md")
        || path.ends_with(".txt")
    {
        return 20;
    }
    35
}

fn max_path_impact_score(paths: &[String]) -> i32 {
    paths
        .iter()
        .map(|path| path_impact_score(path))
        .max()
        .unwrap_or(0)
}

fn looks_like_project_path(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && !trimmed.contains("://")
        && (trimmed.contains('/')
            || trimmed.contains('.')
            || trimmed.starts_with("src")
            || trimmed.starts_with("crates"))
}

fn collect_spec_code_under_test_paths(spec_obj: &Spec) -> Vec<String> {
    let mut paths = Vec::new();
    for component in &spec_obj.project_components {
        if component.role.eq_ignore_ascii_case("code_under_test")
            && looks_like_project_path(&component.path)
        {
            let normalized = normalize_project_path(&component.path);
            if !normalized.is_empty() && !paths.iter().any(|existing| existing == &normalized) {
                paths.push(normalized);
            }
        }
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        for value in &blueprint.code_under_test {
            if looks_like_project_path(value) {
                let normalized = normalize_project_path(value);
                if !normalized.is_empty() && !paths.iter().any(|existing| existing == &normalized) {
                    paths.push(normalized);
                }
            }
        }
    }
    paths
}

fn collect_staged_code_under_test_paths(
    spec_obj: &Spec,
    target_source: &TargetSourceMetadata,
    repo_root: &Path,
) -> Vec<String> {
    let source_root = PathBuf::from(&target_source.source_path);
    let mut resolved = Vec::new();
    for raw in collect_spec_code_under_test_paths(spec_obj) {
        if let Some(path) = resolve_path_for_staged_workspace(&raw, &source_root, repo_root) {
            push_unique(&mut resolved, &path);
        }
    }
    resolved
}

fn resolve_path_for_staged_workspace(
    raw: &str,
    source_root: &Path,
    repo_root: &Path,
) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let candidate = PathBuf::from(trimmed);
    let candidates = if candidate.is_absolute() {
        vec![candidate]
    } else {
        vec![source_root.join(trimmed), repo_root.join(trimmed)]
    };

    for candidate in candidates {
        let Ok(canonical) = candidate.canonicalize() else {
            continue;
        };
        if canonical == source_root {
            return Some(".".to_string());
        }
        if canonical.starts_with(source_root) {
            let Ok(relative) = canonical.strip_prefix(source_root) else {
                continue;
            };
            let normalized = normalize_project_path(&relative.display().to_string());
            return Some(if normalized.is_empty() {
                ".".to_string()
            } else {
                normalized
            });
        }
    }

    None
}

fn collect_spec_provenance_paths(spec_obj: &Spec) -> Vec<String> {
    let mut paths = Vec::new();
    for component in &spec_obj.project_components {
        if looks_like_project_path(&component.path) {
            let normalized = normalize_project_path(&component.path);
            if !normalized.is_empty() && !paths.iter().any(|existing| existing == &normalized) {
                paths.push(normalized);
            }
        }
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        for value in blueprint
            .code_under_test
            .iter()
            .chain(blueprint.datasets.iter())
        {
            if looks_like_project_path(value) {
                let normalized = normalize_project_path(value);
                if !normalized.is_empty() && !paths.iter().any(|existing| existing == &normalized) {
                    paths.push(normalized);
                }
            }
        }
    }
    paths
}

fn collect_spec_provenance_surfaces(spec_obj: &Spec) -> Vec<String> {
    let mut surfaces = Vec::new();
    for component in &spec_obj.project_components {
        for interface in &component.interfaces {
            let trimmed = interface.trim();
            if !trimmed.is_empty() && !surfaces.iter().any(|existing| existing == trimmed) {
                surfaces.push(trimmed.to_string());
            }
        }
    }
    if let Some(blueprint) = &spec_obj.scenario_blueprint {
        for surface in &blueprint.runtime_surfaces {
            let trimmed = surface.trim();
            if !trimmed.is_empty() && !surfaces.iter().any(|existing| existing == trimmed) {
                surfaces.push(trimmed.to_string());
            }
        }
    }
    surfaces
}

fn build_project_scan_manifest(
    target_source: &TargetSourceMetadata,
    project_memory_root: &Path,
) -> ProjectScanManifest {
    let source_path = PathBuf::from(&target_source.source_path);
    let detected_files = detect_project_files(&source_path);
    let detected_directories = detect_project_directories(&source_path);
    let likely_commands = detect_project_commands(&source_path);
    let runtime_hints = detect_runtime_hints(&source_path, &detected_files, &detected_directories);

    ProjectScanManifest {
        generated_at: Utc::now().to_rfc3339(),
        label: target_source.label.clone(),
        source_kind: target_source.source_kind.clone(),
        source_path: target_source.source_path.clone(),
        project_memory_root: project_memory_root.display().to_string(),
        git: target_source.git.clone(),
        detected_files,
        detected_directories,
        likely_commands,
        runtime_hints,
    }
}

fn detect_project_files(source_path: &Path) -> Vec<String> {
    let candidates = [
        "Cargo.toml",
        "package.json",
        "pnpm-lock.yaml",
        "package-lock.json",
        "pyproject.toml",
        "requirements.txt",
        "docker-compose.yml",
        "docker-compose.yaml",
        "README.md",
        "docs",
    ];
    candidates
        .iter()
        .filter_map(|candidate| {
            let path = source_path.join(candidate);
            path.exists().then(|| candidate.to_string())
        })
        .collect()
}

fn detect_project_directories(source_path: &Path) -> Vec<String> {
    let candidates = [
        "src", "crates", "ui", "frontend", "backend", "apps", "services", "examples", "tests",
        "scripts", "docs", "data",
    ];
    candidates
        .iter()
        .filter_map(|candidate| {
            let path = source_path.join(candidate);
            path.is_dir().then(|| candidate.to_string())
        })
        .collect()
}

fn detect_project_commands(source_path: &Path) -> Vec<String> {
    let mut commands = Vec::new();
    if source_path.join("Cargo.toml").exists() {
        commands.push("cargo check".to_string());
        commands.push("cargo test -q".to_string());
    }
    if source_path.join("package.json").exists() {
        commands.push("npm run build".to_string());
        commands.push("npm run dev".to_string());
    }
    if source_path.join("pyproject.toml").exists() || source_path.join("requirements.txt").exists()
    {
        commands.push("python3 -m pytest".to_string());
    }
    commands.sort();
    commands.dedup();
    commands
}

fn normalize_memory_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn shared_specific_tag_count(left: &[String], right: &[String]) -> usize {
    let generic = [
        "lesson",
        "project-memory",
        "causal",
        "residue",
        "exploration",
        "dead-end",
        "strategy-register",
    ];
    let left = left
        .iter()
        .filter(|tag| !generic.contains(&tag.as_str()))
        .collect::<Vec<_>>();
    let right = right
        .iter()
        .filter(|tag| !generic.contains(&tag.as_str()))
        .collect::<Vec<_>>();
    left.iter()
        .filter(|tag| right.iter().any(|candidate| candidate == *tag))
        .count()
}

fn render_project_strategy_register_markdown(
    target_source: &TargetSourceMetadata,
    entries: &[DeadEndRegistryEntry],
) -> String {
    if entries.is_empty() {
        return format!(
            "# Strategy Register\n\n- Project: {}\n- No repo-local dead-end strategies have been recorded yet.\n",
            target_source.label
        );
    }

    let lines = entries
        .iter()
        .map(|entry| {
            format!(
                "- [{}] phase={} agent={} strategy={} failure_constraint={} reformulation={}",
                entry.registry_id,
                entry.phase,
                entry.agent,
                entry.strategy,
                entry.failure_constraint,
                entry.reformulation
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "# Strategy Register\n\n- Project: {}\n- Entries: {}\n\n{}\n",
        target_source.label,
        entries.len(),
        lines
    )
}

fn render_project_memory_status_markdown(entries: &[MemoryEntry]) -> String {
    if entries.is_empty() {
        return "# Memory Status\n\n- No project-memory contradictions or supersessions have been recorded yet.\n".to_string();
    }

    let lines = entries
        .iter()
        .map(|entry| {
            format!(
                "- {} status={} superseded_by={} challenged_by={}",
                entry.id,
                entry
                    .provenance
                    .status
                    .clone()
                    .unwrap_or_else(|| "active".to_string()),
                entry
                    .provenance
                    .superseded_by
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
                if entry.provenance.challenged_by.is_empty() {
                    "none".to_string()
                } else {
                    entry.provenance.challenged_by.join(", ")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("# Memory Status\n\n{}\n", lines)
}

fn derive_stale_memory_mitigation_status(
    risk: &ProjectResumeRisk,
    validation: &ValidationSummary,
    hidden_scenarios: &HiddenScenarioSummary,
    exploration_exists: bool,
) -> String {
    let evidence_count = usize::from(validation.passed)
        + usize::from(hidden_scenarios.passed)
        + usize::from(exploration_exists);

    if validation.passed && hidden_scenarios.passed && exploration_exists {
        return "satisfied".to_string();
    }
    if validation.passed && hidden_scenarios.passed && !matches!(risk.severity.as_str(), "critical")
    {
        return "satisfied".to_string();
    }
    if evidence_count >= 2 {
        return "partially_satisfied".to_string();
    }
    if evidence_count == 1 && matches!(risk.severity.as_str(), "low" | "medium") {
        return "partially_satisfied".to_string();
    }
    "unresolved".to_string()
}

fn render_stale_memory_mitigation_status_markdown(
    artifact: &StaleMemoryMitigationStatusArtifact,
) -> String {
    let satisfied = artifact
        .entries
        .iter()
        .filter(|entry| entry.status == "satisfied")
        .count();
    let partial = artifact
        .entries
        .iter()
        .filter(|entry| entry.status == "partially_satisfied")
        .count();
    let unresolved = artifact
        .entries
        .iter()
        .filter(|entry| entry.status == "unresolved")
        .count();

    let entry_lines = if artifact.entries.is_empty() {
        "- No stale-memory risks were active for this run.".to_string()
    } else {
        artifact
            .entries
            .iter()
            .map(|entry| {
                format!(
                    "- {} [status={} severity={} score={} previous_score={} reduced={}] mitigation_steps={} related_checks={} evidence={} ",
                    entry.memory_id,
                    entry.status,
                    entry.severity,
                    entry.severity_score,
                    entry
                        .previous_severity_score
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "none".to_string()),
                    entry
                        .risk_reduced_from_previous
                        .map(|value| if value { "true" } else { "false" }.to_string())
                        .unwrap_or_else(|| "unknown".to_string()),
                    if entry.mitigation_steps.is_empty() {
                        "none".to_string()
                    } else {
                        entry.mitigation_steps.join(" | ")
                    },
                    if entry.related_checks.is_empty() {
                        "none".to_string()
                    } else {
                        entry.related_checks.join(" | ")
                    },
                    if entry.evidence.is_empty() {
                        "none".to_string()
                    } else {
                        entry.evidence.join(" | ")
                    },
                )
                .trim_end()
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Stale Memory Mitigation Status\n\n- Run: {}\n- Spec: {}\n- Product: {}\n- Generated at: {}\n- Entries: {}\n- Status mix: satisfied={} partially_satisfied={} unresolved={}\n\n## Current Risk Status\n{}\n\n## Resolved Since Previous Run\n{}\n",
        artifact.run_id,
        artifact.spec_id,
        artifact.product,
        artifact.generated_at,
        artifact.entries.len(),
        satisfied,
        partial,
        unresolved,
        entry_lines,
        render_list(
            &artifact.resolved_since_previous,
            "No stale-memory items dropped out of the risk list since the previous recorded run.",
        ),
    )
}

fn render_stale_memory_history_markdown(
    target_source: &TargetSourceMetadata,
    history: &StaleMemoryMitigationHistory,
) -> String {
    if history.records.is_empty() {
        return format!(
            "# Stale Memory History\n\n- Project: {}\n- No stale-memory mitigation history has been recorded yet.\n",
            target_source.label
        );
    }

    let recent_records = history
        .records
        .iter()
        .rev()
        .take(5)
        .map(|record| {
            let satisfied = record
                .entries
                .iter()
                .filter(|entry| entry.status == "satisfied")
                .count();
            let partial = record
                .entries
                .iter()
                .filter(|entry| entry.status == "partially_satisfied")
                .count();
            let unresolved = record
                .entries
                .iter()
                .filter(|entry| entry.status == "unresolved")
                .count();
            format!(
                "- run={} spec={} generated={} entries={} satisfied={} partially_satisfied={} unresolved={} resolved_since_previous={}",
                record.run_id,
                record.spec_id,
                record.generated_at,
                record.entries.len(),
                satisfied,
                partial,
                unresolved,
                record.resolved_since_previous.len(),
            )
        })
        .collect::<Vec<_>>();

    let latest_summary = history
        .records
        .last()
        .map(|record| {
            if record.entries.is_empty() {
                "- Latest record had no active stale-memory risks.".to_string()
            } else {
                record
                    .entries
                    .iter()
                    .take(8)
                    .map(|entry| {
                        format!(
                            "- {} status={} severity={} score={} reduced_from_previous={}",
                            entry.memory_id,
                            entry.status,
                            entry.severity,
                            entry.severity_score,
                            entry
                                .risk_reduced_from_previous
                                .map(|value| if value { "true" } else { "false" }.to_string())
                                .unwrap_or_else(|| "unknown".to_string())
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        })
        .unwrap_or_else(|| "- No latest record available.".to_string());

    format!(
        "# Stale Memory History\n\n- Project: {}\n- Records retained: {}\n\n## Recent Runs\n{}\n\n## Latest Risk Snapshot\n{}\n",
        target_source.label,
        history.records.len(),
        render_list(&recent_records, "No recent stale-memory records available."),
        latest_summary,
    )
}

fn detect_runtime_hints(
    source_path: &Path,
    detected_files: &[String],
    detected_directories: &[String],
) -> Vec<String> {
    let mut hints = Vec::new();
    if detected_files.iter().any(|value| value == "Cargo.toml")
        && detected_directories
            .iter()
            .any(|value| value == "ui" || value == "frontend")
    {
        hints.push("Repo appears to contain both backend and UI surfaces.".to_string());
    }
    if detected_directories.iter().any(|value| value == "examples") {
        hints.push(
            "Example datasets or reference integrations may live under examples/.".to_string(),
        );
    }
    if detected_directories.iter().any(|value| value == "tests") {
        hints.push("Repo exposes explicit test surfaces under tests/.".to_string());
    }
    if source_path.join(".harkonnen").exists() {
        hints.push("Repo already contains Harkonnen-local continuity files.".to_string());
    }
    hints
}

fn render_project_scan_markdown(manifest: &ProjectScanManifest) -> String {
    let git_summary = manifest
        .git
        .as_ref()
        .map(|git| {
            format!(
                "branch={} commit={} remote={} clean={}",
                git.branch.clone().unwrap_or_else(|| "unknown".to_string()),
                git.commit.clone().unwrap_or_else(|| "unknown".to_string()),
                git.remote_origin
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
                git.clean
                    .map(|value| if value { "true" } else { "false" }.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            )
        })
        .unwrap_or_else(|| "git metadata unavailable".to_string());

    format!(
        "# Project Scan\n\n- Generated at: {}\n- Project: {}\n- Source kind: {}\n- Source path: {}\n- Project memory root: {}\n- Git: {}\n\n## Detected Files\n{}\n\n## Detected Directories\n{}\n\n## Likely Commands\n{}\n\n## Runtime Hints\n{}\n",
        manifest.generated_at,
        manifest.label,
        manifest.source_kind,
        manifest.source_path,
        manifest.project_memory_root,
        git_summary,
        render_list(&manifest.detected_files, "No key top-level files detected yet."),
        render_list(&manifest.detected_directories, "No key top-level directories detected yet."),
        render_list(&manifest.likely_commands, "No likely commands inferred yet."),
        render_list(&manifest.runtime_hints, "No runtime hints inferred yet."),
    )
}

fn score_briefing_evidence(
    haystack: &str,
    spec_id: &str,
    product: &str,
    query_terms: &[String],
    domain_signals: &[String],
) -> i32 {
    let haystack = haystack.to_lowercase();
    let mut score = 0;

    if haystack.contains(&spec_id.to_lowercase()) {
        score += 8;
    }
    if haystack.contains(&product.to_lowercase()) {
        score += 5;
    }
    for term in query_terms {
        let needle = term.trim().to_lowercase();
        if needle.len() >= 3 && haystack.contains(&needle) {
            score += 3;
        }
    }
    for signal in domain_signals {
        let needle = signal.trim().to_lowercase();
        if !needle.is_empty() && haystack.contains(&needle) {
            score += 2;
        }
    }

    score
}

fn stable_key_fragment(value: &str) -> String {
    let mut fragment = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    while fragment.contains("--") {
        fragment = fragment.replace("--", "-");
    }
    let fragment = fragment.trim_matches('-');
    let fragment = if fragment.is_empty() {
        "dead-end"
    } else {
        fragment
    };
    fragment.chars().take(48).collect()
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
            let mut details = vec![format!(
                "role={}",
                fallback_component_value(&component.role)
            )];
            details.push(format!(
                "kind={}",
                fallback_component_value(&component.kind)
            ));
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
        lines.push(format!(
            "code_under_test={}",
            blueprint.code_under_test.join(", ")
        ));
    }
    if !blueprint.hidden_oracles.is_empty() {
        lines.push(format!(
            "hidden_oracles={}",
            blueprint.hidden_oracles.join(", ")
        ));
    }
    if !blueprint.datasets.is_empty() {
        lines.push(format!("datasets={}", blueprint.datasets.join(", ")));
    }
    if !blueprint.runtime_surfaces.is_empty() {
        lines.push(format!(
            "runtime_surfaces={}",
            blueprint.runtime_surfaces.join(", ")
        ));
    }
    if !blueprint.coobie_memory_topics.is_empty() {
        lines.push(format!(
            "coobie_memory_topics={}",
            blueprint.coobie_memory_topics.join(", ")
        ));
    }
    if !blueprint.required_artifacts.is_empty() {
        lines.push(format!(
            "required_artifacts={}",
            blueprint.required_artifacts.join(", ")
        ));
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

fn summarize_pinned_skill_markdown(raw: &str, max_chars: usize) -> String {
    let mut in_frontmatter = false;
    let mut lines = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter {
            continue;
        }
        lines.push(line);
    }
    let cleaned = lines
        .join(
            "
",
        )
        .trim()
        .to_string();
    if cleaned.chars().count() <= max_chars {
        cleaned
    } else {
        let mut excerpt = cleaned.chars().take(max_chars).collect::<String>();
        excerpt.push_str(
            "
...[truncated]",
        );
        excerpt
    }
}

fn indent_block(text: &str, prefix: &str) -> String {
    text.lines()
        .map(|line| format!("{}{}", prefix, line))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

fn pinned_skill_matches_provider_route(source: &str, resolved_provider: &str) -> bool {
    let source = source.trim().to_ascii_lowercase();
    let provider = resolved_provider.trim().to_ascii_lowercase();
    match source.as_str() {
        "anthropic" => provider == "claude" || provider == "anthropic",
        "openai" => provider == "openai" || provider == "codex",
        "google" => provider == "gemini" || provider == "google",
        _ => source == provider || source.is_empty(),
    }
}

fn format_resolved_pinned_skill_excerpts(
    entries: &[ResolvedPinnedSkillExcerpt],
    resolved_provider: &str,
) -> String {
    if entries.is_empty() {
        return format!(
            "- No pinned external skills are mapped to this Labrador for provider route '{}'.",
            resolved_provider
        );
    }

    entries
        .iter()
        .map(|entry| {
            format!(
                "- {}
  provider family: {}
  source: {}
  rationale: {}
  excerpt:
{}",
                entry.id,
                entry.provider_family,
                entry.source,
                entry.rationale,
                indent_block(&entry.excerpt, "    "),
            )
        })
        .collect::<Vec<_>>()
        .join(
            "

",
        )
}

fn fingerprint_agent_prompt_bundle(bundle: &AgentPromptBundleArtifact) -> String {
    let mut hasher = DefaultHasher::new();
    bundle.agent_name.hash(&mut hasher);
    bundle.display_name.hash(&mut hasher);
    bundle.role.hash(&mut hasher);
    bundle.resolved_provider.hash(&mut hasher);
    bundle.resolved_model.hash(&mut hasher);
    bundle.resolved_surface.hash(&mut hasher);
    bundle.shared_personality.hash(&mut hasher);
    bundle.personality_addendum.hash(&mut hasher);
    bundle.curated_skill_bundle.hash(&mut hasher);
    bundle.system_instruction.hash(&mut hasher);
    bundle.repo_context_block.hash(&mut hasher);
    for skill_id in &bundle.pinned_skill_ids {
        skill_id.hash(&mut hasher);
    }
    for entry in &bundle.pinned_external_skills {
        entry.id.hash(&mut hasher);
        entry.source.hash(&mut hasher);
        entry.provider_family.hash(&mut hasher);
        entry.vendor_path.hash(&mut hasher);
        entry.rationale.hash(&mut hasher);
        entry.excerpt.hash(&mut hasher);
    }
    for entry in &bundle.repo_local_context_entries {
        entry.label.hash(&mut hasher);
        entry.path.hash(&mut hasher);
        entry.category.hash(&mut hasher);
        entry.scope.hash(&mut hasher);
        entry.summary.hash(&mut hasher);
        entry.relevance.hash(&mut hasher);
    }
    for entry in &bundle.repo_local_skill_entries {
        entry.label.hash(&mut hasher);
        entry.path.hash(&mut hasher);
        entry.category.hash(&mut hasher);
        entry.scope.hash(&mut hasher);
        entry.summary.hash(&mut hasher);
        entry.relevance.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn render_prompt_bundle_markdown(bundle: &AgentPromptBundleArtifact) -> String {
    let pinned_skill_lines = if bundle.pinned_external_skills.is_empty() {
        vec![format!(
            "No provider-matched pinned external skills were resolved for route '{}'.",
            bundle.resolved_provider
        )]
    } else {
        bundle
            .pinned_external_skills
            .iter()
            .map(|entry| format!("{} [{}]", entry.id, entry.provider_family))
            .collect::<Vec<_>>()
    };
    let repo_context_lines = bundle
        .repo_local_context_entries
        .iter()
        .take(8)
        .map(|entry| format!("{} [{}]", entry.label, entry.path))
        .collect::<Vec<_>>();
    let repo_skill_lines = bundle
        .repo_local_skill_entries
        .iter()
        .take(8)
        .map(|entry| format!("{} [{}]", entry.label, entry.path))
        .collect::<Vec<_>>();

    format!(
        "# Prompt Bundle

- Agent: {}
- Role: {}
- Provider route: {}
- Model: {}
- Surface: {}
- Fingerprint: {}

## Pinned External Skills
{}

## Repo-Local Context
{}

## Repo-Local Skill Bundles
{}

## System Instruction
```text
{}
```

## Repo Context Block
```text
{}
```
",
        bundle.display_name,
        bundle.role,
        bundle.resolved_provider,
        bundle.resolved_model.as_deref().unwrap_or("unresolved"),
        bundle.resolved_surface.as_deref().unwrap_or("unspecified"),
        bundle.fingerprint,
        render_list(
            &pinned_skill_lines,
            "No pinned external skills were resolved."
        ),
        render_list(
            &repo_context_lines,
            "No repo-local context entries were resolved."
        ),
        render_list(
            &repo_skill_lines,
            "No repo-local skill bundles were resolved."
        ),
        bundle.system_instruction,
        bundle.repo_context_block,
    )
}

fn render_phase_attributions_markdown(records: &[PhaseAttributionRecord]) -> String {
    if records.is_empty() {
        return "# Phase Attributions\n\n- No phase attributions recorded yet.".to_string();
    }

    let mut lines = vec!["# Phase Attributions".to_string(), String::new()];
    for record in records {
        lines.push(format!("## {} / {}", record.phase, record.agent_name));
        lines.push(format!("- Episode: {}", record.episode_id));
        lines.push(format!("- Outcome: {}", record.outcome));
        lines.push(format!(
            "- Confidence: {}",
            record
                .confidence
                .map(|value| format!("{:.2}", value))
                .unwrap_or_else(|| "unspecified".to_string())
        ));
        lines.push(format!(
            "- Prompt bundle: {}",
            record
                .prompt_bundle_fingerprint
                .as_deref()
                .unwrap_or("none recorded")
        ));
        lines.push(format!(
            "- Provider route: {}",
            record
                .prompt_bundle_provider
                .as_deref()
                .unwrap_or("none recorded")
        ));
        lines.push(format!(
            "- Pinned skills: {}",
            if record.pinned_skill_ids.is_empty() {
                "none".to_string()
            } else {
                record.pinned_skill_ids.join(", ")
            }
        ));
        lines.push(format!(
            "- Memory ids: {}",
            if record.project_memory_ids.is_empty() && record.core_memory_ids.is_empty() {
                "none".to_string()
            } else {
                record
                    .project_memory_ids
                    .iter()
                    .chain(record.core_memory_ids.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
        lines.push(format!(
            "- Required checks: {}",
            if record.required_checks.is_empty() {
                "none".to_string()
            } else {
                record.required_checks.join(" | ")
            }
        ));
        lines.push(String::new());
    }
    lines.join("\n")
}

fn render_repo_local_prompt_lines(
    entries: &[RepoLocalContextEntry],
    empty_message: &str,
) -> String {
    if entries.is_empty() {
        return format!("- {}", empty_message);
    }
    entries
        .iter()
        .take(6)
        .map(|entry| {
            format!(
                "- {} [{} | relevance={}] {}",
                entry.label, entry.category, entry.relevance, entry.summary
            )
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

fn render_list(items: &[String], empty_message: &str) -> String {
    if items.is_empty() {
        format!("- {}", empty_message)
    } else {
        format!(
            "- {}",
            items.join(
                "
- "
            )
        )
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles
        .iter()
        .any(|needle| haystack.contains(&needle.to_lowercase()))
}

fn format_memory_context(memory_hits: &[String]) -> String {
    if memory_hits.is_empty() {
        "No memory hits collected for this run.".to_string()
    } else {
        memory_hits.join(
            "

---

",
        )
    }
}

fn format_memory_context_bundle(bundle: &MemoryContextBundle) -> String {
    let mut sections = Vec::new();
    sections.push("Coobie Memory Context".to_string());
    sections.push("=====================".to_string());
    sections.push(format!(
        "Project memory root: {}",
        bundle
            .project_memory_root
            .clone()
            .unwrap_or_else(|| "not available".to_string())
    ));
    sections.push(String::new());
    sections.push("Project Memory Hits".to_string());
    sections.push("-------------------".to_string());
    sections.push(format_memory_context(&bundle.project_memory_hits));
    sections.push(String::new());
    sections.push("Core Memory Hits".to_string());
    sections.push("----------------".to_string());
    sections.push(format_memory_context(&bundle.core_memory_hits));
    sections.push(String::new());
    sections.push("Combined Memory Hits".to_string());
    sections.push("--------------------".to_string());
    sections.push(format_memory_context(&bundle.memory_hits));
    sections.join(
        "
",
    ) + "
"
}

fn should_promote_to_core_memory(tags: &[String]) -> bool {
    tags.iter().any(|tag| {
        matches!(
            tag.to_ascii_lowercase().as_str(),
            "core" | "universal" | "cross-project" | "factory"
        )
    })
}

fn extract_memory_entry_id(hit: &str) -> Option<String> {
    let start = hit.find('[')?;
    let end = hit[start + 1..].find(']')? + start + 1;
    let id = hit[start + 1..end].trim();
    if id.is_empty() || id.contains("memory") || id.contains("context") {
        None
    } else {
        Some(id.to_string())
    }
}

fn format_yaml_list(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            items
                .iter()
                .map(|item| format!("\"{}\"", item.replace('"', "'")))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

fn phase_artifact_hints(phase: &str) -> Vec<String> {
    match phase {
        "memory" => vec![
            "memory_context.md".to_string(),
            "coobie_briefing.json".to_string(),
            "coobie_preflight_response.md".to_string(),
        ],
        "intake" => vec!["intent.json".to_string()],
        "implementation" => vec!["implementation_plan.md".to_string()],
        "tools" => vec!["tool_plan.md".to_string()],
        "twin" => vec!["twin.json".to_string(), "twin_narrative.md".to_string()],
        "validation" => vec![
            "validation.json".to_string(),
            "validation_output.log".to_string(),
            "corpus_results.json".to_string(),
        ],
        "hidden_scenarios" => vec!["hidden_scenarios.json".to_string()],
        "artifacts" => vec![
            "exploration_log.md".to_string(),
            "exploration_log.json".to_string(),
            "dead_end_registry_snapshot.json".to_string(),
        ],
        _ => Vec::new(),
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
                vec![
                    "install".to_string(),
                    "--no-fund".to_string(),
                    "--no-audit".to_string(),
                ],
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
                vec![
                    "install".to_string(),
                    "--no-fund".to_string(),
                    "--no-audit".to_string(),
                ],
                "npm install --no-fund --no-audit".to_string(),
            ));
        }
    }

    if staged_product.join("package-lock.json").exists() && command_available("npm") {
        return Some((
            "npm".to_string(),
            vec![
                "ci".to_string(),
                "--no-fund".to_string(),
                "--no-audit".to_string(),
            ],
            "npm ci --no-fund --no-audit".to_string(),
        ));
    }

    if command_available("npm") {
        return Some((
            "npm".to_string(),
            vec![
                "install".to_string(),
                "--no-fund".to_string(),
                "--no-audit".to_string(),
            ],
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
    for entry in
        std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))?
    {
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

fn is_mason_context_candidate(path: &str) -> bool {
    let normalized = normalize_project_path(path);
    if normalized.is_empty() {
        return false;
    }
    let blocked_prefixes = [
        ".git/",
        ".harkonnen/",
        "target/",
        "dist/",
        "build/",
        "node_modules/",
        "factory/",
    ];
    if blocked_prefixes
        .iter()
        .any(|prefix| normalized.starts_with(prefix))
    {
        return false;
    }
    let Some(ext) = Path::new(&normalized)
        .extension()
        .and_then(|value| value.to_str())
    else {
        return matches!(
            normalized.as_str(),
            "Cargo.toml" | "go.mod" | "package.json" | "pyproject.toml" | "requirements.txt"
        );
    };
    matches!(
        ext,
        "rs" | "toml"
            | "json"
            | "yaml"
            | "yml"
            | "md"
            | "txt"
            | "js"
            | "jsx"
            | "ts"
            | "tsx"
            | "py"
            | "go"
            | "java"
            | "c"
            | "cc"
            | "cpp"
            | "h"
            | "hpp"
            | "cs"
    )
}

fn mason_context_priority(path: &str) -> (u8, usize, String) {
    let normalized = normalize_project_path(path);
    let priority = if normalized == "Cargo.toml"
        || normalized.ends_with("/Cargo.toml")
        || normalized == "package.json"
        || normalized.ends_with("/package.json")
        || normalized == "pyproject.toml"
        || normalized.ends_with("/pyproject.toml")
        || normalized == "go.mod"
        || normalized.ends_with("/go.mod")
    {
        0
    } else if normalized.ends_with("/src/lib.rs")
        || normalized.ends_with("/src/main.rs")
        || normalized.ends_with("/lib.rs")
        || normalized.ends_with("/main.rs")
    {
        1
    } else {
        2
    };
    (priority, normalized.len(), normalized)
}

fn read_mason_context_file(
    staged_product: &Path,
    relative: &str,
) -> Result<Option<MasonContextFile>> {
    let path = join_workspace_relative_path(staged_product, relative)?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if bytes.contains(&0) {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&bytes).to_string();
    let max_chars = 12000usize;
    let mut content = text.chars().take(max_chars).collect::<String>();
    let truncated = text.chars().count() > max_chars;
    if truncated {
        content.push_str("\n...[truncated]");
    }
    Ok(Some(MasonContextFile {
        path: normalize_project_path(relative),
        content,
        truncated,
    }))
}

fn build_mason_context_files(
    staged_product: &Path,
    editable_paths: &[String],
) -> Result<Vec<MasonContextFile>> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    for relative in editable_paths {
        let target = join_workspace_relative_path(staged_product, relative)?;
        if target.is_file() {
            let normalized = normalize_project_path(relative);
            if is_mason_context_candidate(&normalized) && seen.insert(normalized.clone()) {
                selected.push(normalized);
            }
            continue;
        }
        if !target.is_dir() {
            continue;
        }
        let mut files = list_relative_files(staged_product, &target)?;
        files.retain(|path| is_mason_context_candidate(path));
        files.sort_by_key(|path| mason_context_priority(path));
        for path in files {
            if seen.insert(path.clone()) {
                selected.push(path);
            }
            if selected.len() >= 8 {
                break;
            }
        }
        if selected.len() >= 8 {
            break;
        }
    }

    let mut context = Vec::new();
    for relative in selected.into_iter().take(8) {
        if let Some(file) = read_mason_context_file(staged_product, &relative)? {
            context.push(file);
        }
    }
    Ok(context)
}

fn join_workspace_relative_path(base: &Path, relative: &str) -> Result<PathBuf> {
    let base = base.canonicalize()?;
    let relative = Path::new(relative);
    if relative.is_absolute() {
        bail!("absolute paths are not allowed inside the staged workspace");
    }

    let mut joined = base.clone();
    for component in relative.components() {
        match component {
            Component::Normal(value) => joined.push(value),
            Component::CurDir => {}
            _ => bail!("path escapes allowed workspace"),
        }
    }
    Ok(joined)
}

/// Compact constraint block sent to Mason — only what it needs to act.
/// Strips all citation chains, reasoning histories, and memory blobs.
fn mason_slim_briefing(briefing: &CoobieBriefing) -> String {
    let mut out = String::new();

    if !briefing.required_checks.is_empty() {
        out.push_str("REQUIRED CHECKS:\n");
        for c in &briefing.required_checks {
            out.push_str(&format!("- {c}\n"));
        }
        out.push('\n');
    }

    if !briefing.recommended_guardrails.is_empty() {
        out.push_str("GUARDRAILS:\n");
        for g in &briefing.recommended_guardrails {
            out.push_str(&format!("- {g}\n"));
        }
        out.push('\n');
    }

    if !briefing.application_risks.is_empty() {
        out.push_str("RISKS:\n");
        for r in &briefing.application_risks {
            out.push_str(&format!("- {r}\n"));
        }
        out.push('\n');
    }

    // Coobie's distilled verdict — capped so it can't explode
    let response = briefing.coobie_response.trim();
    if !response.is_empty() {
        let capped: String = response.chars().take(600).collect();
        out.push_str("COOBIE SUMMARY:\n");
        out.push_str(&capped);
        if response.chars().count() > 600 {
            out.push_str("...");
        }
        out.push('\n');
    }

    if out.is_empty() {
        "No constraints loaded.".to_string()
    } else {
        out.trim_end().to_string()
    }
}

fn path_allowed_for_edit(path: &str, editable_paths: &[String]) -> bool {
    let path = normalize_project_path(path);
    editable_paths.iter().any(|root| {
        let root = normalize_project_path(root);
        root == "." || path == root || path.starts_with(&format!("{root}/"))
    })
}

/// Create a git branch in `source_root`, copy the changed files from the
/// staged workspace, commit them, and restore the original branch.
///
/// The branch is named `mason/<spec-id>-<short-run-id>` and is created from
/// whatever branch is currently checked out (typically main/master). This gives
/// reviewers a proper `git diff main...mason/<branch>` without touching live
/// working-tree files outside a controlled commit.
async fn mason_commit_branch(
    source_root: &Path,
    staged_product: &Path,
    changed_files: &[String],
    branch_name: &str,
    spec_title: &str,
    run_id: &str,
) -> Result<()> {
    // Capture the current branch so we can restore it.
    let current_branch = {
        let out = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(source_root)
            .output()
            .await
            .context("git rev-parse HEAD")?;
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    // Create and switch to the new branch.
    let checkout = Command::new("git")
        .args(["checkout", "-b", branch_name])
        .current_dir(source_root)
        .output()
        .await
        .context("git checkout -b")?;
    if !checkout.status.success() {
        anyhow::bail!(
            "git checkout -b {} failed: {}",
            branch_name,
            String::from_utf8_lossy(&checkout.stderr)
        );
    }

    // Copy each changed file from the staged workspace into the real repo.
    for rel_path in changed_files {
        let src = staged_product.join(rel_path);
        let dst = source_root.join(rel_path);
        if let Some(parent) = dst.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(&src, &dst)
            .await
            .with_context(|| format!("copying {} to branch workspace", rel_path))?;
    }

    // Stage changed files.
    let mut add_args = vec!["add", "--"];
    let file_refs: Vec<&str> = changed_files.iter().map(|s| s.as_str()).collect();
    add_args.extend(file_refs.iter());
    let add = Command::new("git")
        .args(&add_args)
        .current_dir(source_root)
        .output()
        .await
        .context("git add")?;
    if !add.status.success() {
        // Restore branch before bailing.
        let _ = Command::new("git")
            .args(["checkout", &current_branch])
            .current_dir(source_root)
            .output()
            .await;
        anyhow::bail!("git add failed: {}", String::from_utf8_lossy(&add.stderr));
    }

    // Commit.
    let message = format!(
        "mason: {} [run:{}]\n\nAutomated edit by Mason agent.\nSpec: {}\nRun: {}",
        spec_title,
        &run_id[..8],
        spec_title,
        run_id,
    );
    let commit = Command::new("git")
        .args(["commit", "-m", &message])
        .current_dir(source_root)
        .output()
        .await
        .context("git commit")?;
    if !commit.status.success() {
        let _ = Command::new("git")
            .args(["checkout", &current_branch])
            .current_dir(source_root)
            .output()
            .await;
        anyhow::bail!(
            "git commit failed: {}",
            String::from_utf8_lossy(&commit.stderr)
        );
    }

    // Restore original branch.
    Command::new("git")
        .args(["checkout", &current_branch])
        .current_dir(source_root)
        .output()
        .await
        .context("git checkout restore")?;

    Ok(())
}

fn parse_mason_edit_proposal(raw: &str) -> Result<MasonEditProposal> {
    let stripped = raw
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    let proposal = serde_json::from_str::<MasonEditProposal>(stripped)
        .with_context(|| "parsing Mason edit proposal JSON")?;
    Ok(proposal)
}

fn copy_tree_contents(source_root: &Path, current: &Path, destination_root: &Path) -> Result<()> {
    for entry in
        std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))?
    {
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
            std::fs::copy(&path, &destination).with_context(|| {
                format!("copying {} -> {}", path.display(), destination.display())
            })?;
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

fn checkpoint_draft(run_id: &str, board: &BlackboardState, blocker: &str) -> CheckpointDraft {
    let normalized = checkpoint_slug(blocker);
    let phase = checkpoint_phase_for_blocker(blocker, &board.current_phase);
    let agent =
        checkpoint_agent_for_phase(phase.as_deref().unwrap_or(board.current_phase.as_str()));
    let checkpoint_type = checkpoint_type_for_blocker(blocker).to_string();
    let prompt = checkpoint_prompt_for_blocker(blocker, phase.as_deref(), agent.as_deref());
    let context_json = serde_json::json!({
        "blocker": blocker,
        "current_phase": board.current_phase,
        "active_goal": board.active_goal,
        "policy_flags": board.policy_flags,
        "artifact_refs": board.artifact_refs,
        "agent_claims": board.agent_claims,
    });

    CheckpointDraft {
        checkpoint_id: format!("checkpoint-{run_id}-{normalized}"),
        phase,
        agent,
        checkpoint_type,
        prompt,
        context_json,
    }
}

fn checkpoint_type_for_blocker(blocker: &str) -> &'static str {
    match blocker {
        "visible_validation_failed" => "needs_validation_review",
        "hidden_scenarios_failed" => "needs_hidden_scenario_review",
        "retriever_forge_failed" => "needs_operator_answer",
        _ => "needs_operator_answer",
    }
}

fn checkpoint_phase_for_blocker(blocker: &str, current_phase: &str) -> Option<String> {
    match blocker {
        "visible_validation_failed" => Some("validation".to_string()),
        "hidden_scenarios_failed" => Some("hidden_scenarios".to_string()),
        "retriever_forge_failed" => Some("retriever_forge".to_string()),
        _ if current_phase.trim().is_empty() => None,
        _ => Some(current_phase.to_string()),
    }
}

fn checkpoint_agent_for_phase(phase: &str) -> Option<String> {
    match phase {
        "validation" => Some("bramble".to_string()),
        "hidden_scenarios" => Some("sable".to_string()),
        "retriever_forge" | "implementation" => Some("mason".to_string()),
        "memory" => Some("coobie".to_string()),
        "workspace" => Some("keeper".to_string()),
        "tools" => Some("piper".to_string()),
        "twin" => Some("ash".to_string()),
        "artifacts" => Some("flint".to_string()),
        "intake" => Some("scout".to_string()),
        _ => None,
    }
}

fn checkpoint_prompt_for_blocker(
    blocker: &str,
    phase: Option<&str>,
    agent: Option<&str>,
) -> String {
    match blocker {
        "visible_validation_failed" => "Bramble reported a visible validation failure. Review the evidence and decide whether to rerun, adjust the plan, or explicitly accept the risk.".to_string(),
        "hidden_scenarios_failed" => "Sable found a hidden-scenario failure. Review the scenario evidence and provide an operator decision before treating the run as recovered.".to_string(),
        "retriever_forge_failed" => "Mason's retriever forge packet failed. Provide revised direction or an operator decision before treating this blocker as resolved.".to_string(),
        _ => format!(
            "Operator review needed for blocker `{}`{}{}.",
            blocker,
            phase.map(|value| format!(" during phase `{value}`")).unwrap_or_default(),
            agent.map(|value| format!(" for agent `{value}`")).unwrap_or_default(),
        ),
    }
}

fn checkpoint_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars() {
        let next = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if next == '-' {
            if !last_was_dash && !slug.is_empty() {
                slug.push(next);
            }
            last_was_dash = true;
        } else {
            slug.push(next);
            last_was_dash = false;
        }
    }
    slug.trim_matches('-').to_string()
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

fn render_agent_response_log(agent_executions: &[AgentExecution]) -> String {
    let mut out = String::from(
        "# Labrador Response Log

",
    );
    for execution in agent_executions {
        out.push_str(&format!(
            "## {} ({})

",
            execution.display_name, execution.agent_name
        ));
        out.push_str(&format!(
            "- Role: {}
",
            execution.role
        ));
        out.push_str(&format!(
            "- Provider: {}
",
            execution.provider
        ));
        out.push_str(&format!(
            "- Model: {}
",
            execution.model
        ));
        out.push_str(&format!(
            "- Mode: {}
",
            execution.mode
        ));
        if !execution.summary.trim().is_empty() {
            out.push_str(&format!(
                "- Summary: {}
",
                execution.summary.trim()
            ));
        }
        out.push_str(
            "
### Prompt

```text
",
        );
        out.push_str(execution.prompt.trim());
        out.push_str(
            "
```

### Output

```text
",
        );
        out.push_str(execution.output.trim());
        out.push_str(
            "
```

",
        );
    }
    out
}
