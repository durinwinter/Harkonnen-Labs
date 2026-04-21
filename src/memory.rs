use anyhow::{bail, Context, Result};
use chrono::Utc;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use tokio::process::Command;
use uuid::Uuid;

use crate::setup::{command_available, SetupConfig};

// ── Stored entry ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryProvenance {
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub source_run_id: Option<String>,
    #[serde(default)]
    pub source_spec_id: Option<String>,
    #[serde(default)]
    pub git_branch: Option<String>,
    #[serde(default)]
    pub git_commit: Option<String>,
    #[serde(default)]
    pub git_remote: Option<String>,
    #[serde(default)]
    pub evidence_run_ids: Vec<String>,
    #[serde(default)]
    pub stale_when: Vec<String>,
    #[serde(default)]
    pub observed_paths: Vec<String>,
    #[serde(default)]
    pub code_under_test_paths: Vec<String>,
    #[serde(default)]
    pub observed_surfaces: Vec<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub challenged_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub tags: Vec<String>,
    pub summary: String,
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub provenance: MemoryProvenance,
    #[serde(default)]
    pub recall_count: i64,
    #[serde(default)]
    pub last_recalled: Option<String>,
    #[serde(default)]
    pub loaded_for_run_count: i64,
    #[serde(default)]
    pub contributed_to_success_count: i64,
    #[serde(default)]
    pub contributed_to_failure_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MemoryIndex {
    pub entries: Vec<MemoryEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRetrievalHit {
    pub id: String,
    pub summary: String,
    pub snippet: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub score: f32,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub superseded_by: Option<String>,
    #[serde(default)]
    pub challenged_by: Vec<String>,
    #[serde(default)]
    pub invalidation_reasons: Vec<String>,
    #[serde(default)]
    pub surfaced_via: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MemoryIngestOptions {
    pub id: Option<String>,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub tags: Vec<String>,
    pub provenance: MemoryProvenance,
    pub keep_asset: bool,
    pub scope_tag: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryIngestResult {
    pub note_path: PathBuf,
    pub asset_path: Option<PathBuf>,
    pub extracted_text_sidecar_path: Option<PathBuf>,
    pub title: String,
    pub extracted_chars: usize,
    pub memory_root: PathBuf,
}

#[derive(Debug, Clone)]
struct ExtractedMemorySource {
    title: String,
    source_kind: String,
    source_locator: String,
    text: String,
    media_kind: String,
    extension_tag: Option<String>,
    asset_source_path: Option<PathBuf>,
    extraction_method: String,
    ocr_applied: bool,
}

#[derive(Debug, Clone)]
struct ExtractedTextPayload {
    text: String,
    extraction_method: String,
    ocr_applied: bool,
}

impl ExtractedTextPayload {
    fn new(
        text: impl Into<String>,
        extraction_method: impl Into<String>,
        ocr_applied: bool,
    ) -> Self {
        Self {
            text: text.into(),
            extraction_method: extraction_method.into(),
            ocr_applied,
        }
    }
}

// ── Store ─────────────────────────────────────────────────────────────────────

/// File-backed memory store rooted at `factory/memory/`.
///
/// Source of truth: `*.md` files in that directory.
/// Query cache:     `index.json` — rebuilt by `reindex()`.
///
/// Two optional external backends read from the same directory:
///   - AnythingLLM (home-linux): document RAG over the md files
///   - MCP memory server (all setups): key-value retrieval, seeded from index
///
/// Backends are configured in the active setup TOML; this store
/// handles only the local file layer.
#[derive(Debug, Clone)]
pub struct MemoryStore {
    pub root: PathBuf,
}

#[derive(Debug, Clone, Default)]
struct MemoryEntryStats {
    recall_count: i64,
    last_recalled: Option<String>,
    loaded_for_run_count: i64,
    contributed_to_success_count: i64,
    contributed_to_failure_count: i64,
}

#[derive(Debug, Clone)]
struct MemoryRetrievalCandidate<'a> {
    score: f32,
    entry: &'a MemoryEntry,
    invalidation_reasons: Vec<String>,
    surfaced_via: Vec<String>,
}

impl MemoryStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Create the memory directory, write seed documents, and build the index.
    /// Prints next steps for AnythingLLM and MCP memory server setup.
    pub async fn init(&self, setup: &SetupConfig) -> Result<()> {
        tokio::fs::create_dir_all(&self.root).await?;
        tokio::fs::create_dir_all(self.root.join("imports")).await?;
        self.write_seed_docs(setup).await?;
        self.reindex().await?;

        println!("Memory directory:  {}", self.root.display());
        println!(
            "Index built:       {}",
            self.root.join("index.json").display()
        );
        println!("Imports folder:    {}", self.root.join("imports").display());
        println!();
        println!("Coobie will auto-refresh her local index when memory files change.");
        println!("Import raw assets with:    harkonnen memory import <file>");
        println!("Ingest docs or URLs with: harkonnen memory ingest <file-or-url>");
        println!();

        if setup.setup.anythingllm.unwrap_or(false) {
            println!("AnythingLLM backend (home-linux):");
            println!("  Run:  ./scripts/coobie-seed-anythingllm.sh");
            println!("  The script now uploads markdown plus imported assets recursively.");
            println!();
        }

        println!("MCP memory backend:");
        println!("  Start: npx -y @modelcontextprotocol/server-memory");
        println!("  Seed:  ./scripts/coobie-seed-mcp.sh  (prints entity payloads)");
        println!();
        println!(
            "Coobie is ready. Manual markdown drops are picked up automatically on next retrieval."
        );

        Ok(())
    }

    /// Scan markdown notes plus imported assets and write index.json for retrieval.
    pub async fn reindex(&self) -> Result<()> {
        tokio::fs::create_dir_all(&self.root).await?;
        tokio::fs::create_dir_all(self.root.join("imports")).await?;

        let existing_index = read_memory_index_if_present(&self.root.join("index.json")).await?;
        let existing_stats = memory_entry_stats_map(existing_index.as_ref());

        let mut entries = Vec::new();
        collect_entries(&self.root, &self.root, &mut entries)?;
        for entry in &mut entries {
            apply_memory_entry_stats(entry, existing_stats.get(&entry.id));
        }
        entries.sort_by(|left, right| left.id.cmp(&right.id));

        let index = MemoryIndex { entries };
        write_memory_index(&self.root.join("index.json"), &index).await?;

        Ok(())
    }
    /// Keyword search across the index. Returns formatted snippets.
    pub async fn retrieve_context(&self, query: &str) -> Result<Vec<String>> {
        let hits = self.retrieve_ranked_entries(query, None, 20).await?;
        Ok(render_memory_hits(query, &hits))
    }

    /// Hybrid retrieval: semantic cosine search (primary) blended with historical
    /// success/failure scores. Falls back to `retrieve_context` on any error so
    /// the pipeline never stalls because of an embedding failure.
    pub async fn retrieve_context_hybrid(
        &self,
        query: &str,
        embedding_store: &crate::embeddings::EmbeddingStore,
    ) -> Result<Vec<String>> {
        let hits = self
            .retrieve_ranked_entries(query, Some(embedding_store), 20)
            .await?;
        Ok(render_memory_hits(query, &hits))
    }

    /// Multi-hop retrieval: performs a first-pass retrieval, then extracts
    /// secondary query terms from the top results to chain a second retrieval
    /// pass. The two result sets are merged and re-ranked by score.
    ///
    /// `retrieval_depth`:
    ///  - 1 = single-pass (same as `retrieve_context_hybrid`)
    ///  - 2 = one chaining step (default for FRAMES-style multi-hop queries)
    ///
    /// Falls back gracefully — a failing hop is ignored rather than propagated.
    pub async fn retrieve_context_multihop(
        &self,
        query: &str,
        embedding_store: &crate::embeddings::EmbeddingStore,
        retrieval_depth: u8,
    ) -> Result<Vec<String>> {
        let depth = retrieval_depth.clamp(1, 3);

        // First pass.
        let mut first_hits = self
            .retrieve_ranked_entries(query, Some(embedding_store), 20)
            .await?;

        if depth == 1 || first_hits.is_empty() {
            return Ok(render_memory_hits(query, &first_hits));
        }

        // Build secondary query from the top-3 first-pass hits: extract words
        // that appear in tags or the first 120 chars of each snippet.
        let secondary_terms: Vec<String> = first_hits
            .iter()
            .take(3)
            .flat_map(|hit| {
                let tag_words = hit.tags.iter().flat_map(|t| {
                    t.split_whitespace()
                        .map(|w| {
                            w.trim_matches(|c: char| !c.is_alphanumeric())
                                .to_lowercase()
                        })
                        .filter(|w| w.len() > 3)
                        .collect::<Vec<_>>()
                });
                let snippet_words = hit
                    .snippet
                    .chars()
                    .take(120)
                    .collect::<String>()
                    .split_whitespace()
                    .map(|w| {
                        w.trim_matches(|c: char| !c.is_alphanumeric())
                            .to_lowercase()
                    })
                    .filter(|w| w.len() > 4)
                    .collect::<Vec<_>>();
                tag_words.chain(snippet_words)
            })
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        if secondary_terms.is_empty() {
            return Ok(render_memory_hits(query, &first_hits));
        }

        let secondary_query = secondary_terms.join(" ");
        let mut second_hits = self
            .retrieve_ranked_entries(&secondary_query, Some(embedding_store), 20)
            .await
            .unwrap_or_default();

        // Merge: keep all first_hits, add second_hits not already present.
        let seen_ids: std::collections::HashSet<String> =
            first_hits.iter().map(|h| h.id.clone()).collect();
        for hit in second_hits.drain(..) {
            if !seen_ids.contains(&hit.id) {
                first_hits.push(hit);
            }
        }

        // Re-rank merged set by score descending, keep top 20.
        first_hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        first_hits.truncate(20);

        Ok(render_memory_hits(query, &first_hits))
    }

    /// Structured retrieval for query-time chaining and source tracing.
    pub async fn retrieve_ranked_entries(
        &self,
        query: &str,
        embedding_store: Option<&crate::embeddings::EmbeddingStore>,
        limit: usize,
    ) -> Result<Vec<MemoryRetrievalHit>> {
        self.ensure_fresh_index().await?;

        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return Ok(Vec::new());
        }

        let index = read_memory_index(&index_path).await?;
        if index.entries.is_empty() {
            return Ok(Vec::new());
        }

        let q = query.to_lowercase();
        let limit = limit.max(1);
        let entry_map: HashMap<&str, &MemoryEntry> = index
            .entries
            .iter()
            .map(|entry| (entry.id.as_str(), entry))
            .collect();

        let results: Vec<(f32, &MemoryEntry)> = if let Some(embedding_store) = embedding_store {
            let memory_root = self.root.display().to_string();
            if let Err(e) = embedding_store
                .ensure_embedded(&index.entries, &memory_root)
                .await
            {
                tracing::warn!("embed failed, falling back to keyword search: {e}");
                keyword_ranked_entries(&index.entries, &q)
            } else {
                match embedding_store
                    .query_semantic(query, &memory_root, limit.max(20))
                    .await
                {
                    Ok(semantic_hits) => {
                        let mut blended = semantic_hits
                            .into_iter()
                            .filter_map(|(sem_score, id)| {
                                let entry = entry_map.get(id.as_str()).copied()?;
                                let historical = (entry.contributed_to_success_count * 10
                                    - entry.contributed_to_failure_count * 4
                                    + entry.recall_count.min(20))
                                    as f32;
                                let kw_bonus = if memory_matches(entry, &q) { 10.0 } else { 0.0 };
                                Some((sem_score * 100.0 + historical + kw_bonus, entry))
                            })
                            .collect::<Vec<_>>();
                        if blended.is_empty() {
                            blended = keyword_ranked_entries(&index.entries, &q);
                        }
                        blended
                    }
                    Err(e) => {
                        tracing::warn!(
                            "semantic query failed, falling back to keyword search: {e}"
                        );
                        keyword_ranked_entries(&index.entries, &q)
                    }
                }
            }
        } else {
            keyword_ranked_entries(&index.entries, &q)
        };

        let mut candidates: HashMap<String, MemoryRetrievalCandidate<'_>> = HashMap::new();
        for (base_score, entry) in results {
            let invalidation_reasons = memory_invalidation_reasons(entry, &entry_map);
            let adjusted_score = adjust_retrieval_score_for_provenance(base_score, entry);
            upsert_memory_candidate(
                &mut candidates,
                entry,
                adjusted_score,
                invalidation_reasons,
                Vec::new(),
            );

            if let Some(successor_id) = entry.provenance.superseded_by.as_deref() {
                if let Some(successor) = entry_map.get(successor_id).copied() {
                    let mut surfaced_via = vec![format!("superseded fact {}", entry.id)];
                    if !entry.summary.trim().is_empty() {
                        surfaced_via.push(format!(
                            "query matched older lesson {}",
                            entry.summary.trim()
                        ));
                    }
                    upsert_memory_candidate(
                        &mut candidates,
                        successor,
                        (base_score + 20.0).max(adjusted_score + 25.0),
                        memory_invalidation_reasons(successor, &entry_map),
                        surfaced_via,
                    );
                }
            }
        }

        let mut ranked = candidates.into_values().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.entry.id.cmp(&right.entry.id))
        });

        Ok(ranked
            .into_iter()
            .take(limit)
            .map(|candidate| MemoryRetrievalHit {
                id: candidate.entry.id.clone(),
                summary: candidate.entry.summary.clone(),
                snippet: candidate.entry.content[..candidate.entry.content.len().min(400)]
                    .to_string(),
                tags: candidate.entry.tags.clone(),
                score: candidate.score,
                status: candidate.entry.provenance.status.clone(),
                superseded_by: candidate.entry.provenance.superseded_by.clone(),
                challenged_by: candidate.entry.provenance.challenged_by.clone(),
                invalidation_reasons: candidate.invalidation_reasons,
                surfaced_via: candidate.surfaced_via,
            })
            .collect())
    }

    /// Write a new memory entry as a markdown file and reindex.
    pub async fn store(
        &self,
        id: &str,
        tags: Vec<String>,
        summary: &str,
        content: &str,
    ) -> Result<()> {
        self.store_with_metadata(id, tags, summary, content, MemoryProvenance::default())
            .await
    }

    pub async fn store_with_metadata(
        &self,
        id: &str,
        tags: Vec<String>,
        summary: &str,
        content: &str,
        provenance: MemoryProvenance,
    ) -> Result<()> {
        let filename = format!("{}.md", sanitize_memory_id(id));
        let doc = render_memory_document(&tags, summary, content, &provenance);
        tokio::fs::write(self.root.join(filename), doc).await?;
        self.reindex().await?;
        Ok(())
    }

    pub async fn list_entries(&self) -> Result<Vec<MemoryEntry>> {
        self.ensure_fresh_index().await?;
        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return Ok(Vec::new());
        }
        Ok(read_memory_index(&index_path).await?.entries)
    }

    pub async fn annotate_entry_status(
        &self,
        id: &str,
        status: &str,
        related_id: Option<&str>,
    ) -> Result<()> {
        let path = self.root.join(format!("{}.md", sanitize_memory_id(id)));
        if !path.exists() {
            return Ok(());
        }

        let raw = tokio::fs::read_to_string(&path).await?;
        let mut parsed = parse_frontmatter(&raw);
        annotate_memory_provenance_status(&mut parsed.provenance, status, related_id);
        let doc = render_memory_document(
            &parsed.tags,
            &parsed.summary,
            &parsed.body,
            &parsed.provenance,
        );
        tokio::fs::write(&path, doc).await?;
        self.reindex().await?;
        Ok(())
    }

    pub async fn mark_entries_loaded(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.ensure_fresh_index().await?;
        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return Ok(());
        }

        let mut index = read_memory_index(&index_path).await?;
        let id_set = ids.iter().cloned().collect::<HashSet<_>>();
        let now = Utc::now().to_rfc3339();
        let mut changed = false;
        for entry in &mut index.entries {
            if id_set.contains(&entry.id) {
                entry.recall_count += 1;
                entry.loaded_for_run_count += 1;
                entry.last_recalled = Some(now.clone());
                changed = true;
            }
        }
        if changed {
            write_memory_index(&index_path, &index).await?;
        }
        Ok(())
    }

    pub async fn record_outcome(&self, ids: &[String], success: bool) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.ensure_fresh_index().await?;
        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return Ok(());
        }

        let mut index = read_memory_index(&index_path).await?;
        let id_set = ids.iter().cloned().collect::<HashSet<_>>();
        let mut changed = false;
        for entry in &mut index.entries {
            if id_set.contains(&entry.id) {
                if success {
                    entry.contributed_to_success_count += 1;
                } else {
                    entry.contributed_to_failure_count += 1;
                }
                changed = true;
            }
        }
        if changed {
            write_memory_index(&index_path, &index).await?;
        }
        Ok(())
    }

    /// Import a file asset into memory/imports and create a searchable sidecar note.
    pub async fn import_asset(
        &self,
        source: &Path,
        id: Option<&str>,
        tags: Vec<String>,
        summary: Option<&str>,
        notes: Option<&str>,
    ) -> Result<PathBuf> {
        if !source.exists() {
            bail!("memory import source not found: {}", source.display());
        }
        if !source.is_file() {
            bail!("memory import source is not a file: {}", source.display());
        }

        let imports_dir = self.root.join("imports");
        tokio::fs::create_dir_all(&imports_dir).await?;

        let source_name = source
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("memory-asset");
        let source_stem = source
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("memory-asset");
        let ext = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if ext.eq_ignore_ascii_case("md") {
            bail!("markdown files can be copied directly into factory/memory; use memory import for PDFs, images, and other assets");
        }

        let mut base_id = sanitize_memory_id(id.unwrap_or(source_stem));
        if base_id.is_empty() {
            base_id = format!("memory-asset-{}", Utc::now().format("%Y%m%d%H%M%S"));
        }
        let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
        let mut unique_id = base_id.clone();
        let mut asset_path = asset_path_for(&imports_dir, &unique_id, ext);
        let mut note_path = imports_dir.join(format!("{}.md", unique_id));
        if asset_path.exists() || note_path.exists() {
            unique_id = format!("{}-{}", base_id, timestamp);
            asset_path = asset_path_for(&imports_dir, &unique_id, ext);
            note_path = imports_dir.join(format!("{}.md", unique_id));
        }

        tokio::fs::copy(source, &asset_path)
            .await
            .with_context(|| format!("copying {} -> {}", source.display(), asset_path.display()))?;

        let mut merged_tags = vec!["asset".to_string()];
        let media_kind = detect_media_kind(&asset_path);
        merged_tags.push(media_kind.to_string());
        if !ext.is_empty() {
            merged_tags.push(ext.to_lowercase());
        }
        for tag in tags {
            let tag = tag.trim();
            if !tag.is_empty() && !merged_tags.iter().any(|existing| existing == tag) {
                merged_tags.push(tag.to_string());
            }
        }

        let rel_asset = asset_path
            .strip_prefix(&self.root)
            .unwrap_or(&asset_path)
            .display()
            .to_string();
        let summary = summary
            .filter(|value| !value.trim().is_empty())
            .map(|value| value.to_string())
            .unwrap_or_else(|| format!("Imported {} asset {}", media_kind, source_name));
        let note_body = format!(
            "# Imported Memory Asset

- Asset path: {}
- Original file: {}
- Media type: {}
- Imported at: {}

## Notes
{}
",
            rel_asset,
            source.display(),
            media_kind,
            Utc::now().to_rfc3339(),
            notes
                .map(|value| value.trim())
                .filter(|value| !value.is_empty())
                .unwrap_or("Imported without additional notes.")
        );
        let note_doc = render_memory_document(
            &merged_tags,
            &summary,
            &note_body,
            &MemoryProvenance::default(),
        );
        tokio::fs::write(&note_path, note_doc).await?;
        self.reindex().await?;
        Ok(note_path)
    }

    pub async fn ingest_source(
        &self,
        source: &str,
        options: MemoryIngestOptions,
    ) -> Result<MemoryIngestResult> {
        tokio::fs::create_dir_all(&self.root).await?;
        tokio::fs::create_dir_all(self.root.join("imports")).await?;

        let extracted = extract_memory_source(source).await?;
        let note_id = next_available_memory_id(
            &self.root,
            options.id.as_deref().unwrap_or(&extracted.title),
        );
        let mut provenance = options.provenance.clone();
        if provenance.source_label.is_none() {
            provenance.source_label = Some(extracted.title.clone());
        }
        if provenance.source_kind.is_none() {
            provenance.source_kind = Some(extracted.source_kind.clone());
        }
        if provenance.source_path.is_none() {
            provenance.source_path = Some(extracted.source_locator.clone());
        }

        let imported_asset = if options.keep_asset {
            if let Some(asset_source_path) = extracted.asset_source_path.as_ref() {
                Some(
                    copy_asset_to_imports(&self.root.join("imports"), asset_source_path, &note_id)
                        .await?,
                )
            } else {
                None
            }
        } else {
            None
        };

        let extracted_text_sidecar = if let Some(imported_asset) = imported_asset.as_ref() {
            if should_write_extracted_text_sidecar(&extracted) {
                Some(
                    write_extracted_text_sidecar(imported_asset, &extracted.text)
                        .await
                        .with_context(|| {
                            format!(
                                "writing extracted text sidecar for {}",
                                imported_asset.display()
                            )
                        })?,
                )
            } else {
                None
            }
        } else {
            None
        };

        let mut tags = vec!["ingested".to_string(), "document".to_string()];
        append_unique_tag(&mut tags, extracted.media_kind.clone());
        if let Some(extension_tag) = extracted.extension_tag.clone() {
            append_unique_tag(&mut tags, extension_tag);
        }
        if let Some(scope_tag) = options.scope_tag.clone() {
            append_unique_tag(&mut tags, scope_tag);
        }
        for tag in options.tags {
            append_unique_tag(&mut tags, tag);
        }

        let summary = options
            .summary
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| default_ingest_summary(&extracted));
        let note_body = render_ingested_memory_body(
            &extracted,
            imported_asset.as_ref(),
            extracted_text_sidecar.as_ref(),
            options.notes.as_deref(),
        );
        let note_doc = render_memory_document(&tags, &summary, &note_body, &provenance);
        let note_path = self
            .root
            .join(format!("{}.md", sanitize_memory_id(&note_id)));
        tokio::fs::write(&note_path, note_doc).await?;
        self.reindex().await?;

        Ok(MemoryIngestResult {
            note_path,
            asset_path: imported_asset,
            extracted_text_sidecar_path: extracted_text_sidecar,
            title: extracted.title,
            extracted_chars: extracted.text.chars().count(),
            memory_root: self.root.clone(),
        })
    }

    async fn ensure_fresh_index(&self) -> Result<()> {
        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return self.reindex().await;
        }

        let index_modified = tokio::fs::metadata(&index_path).await?.modified()?;
        let latest_source = latest_relevant_mtime(&self.root, &self.root)?;
        if latest_source > index_modified {
            self.reindex().await?;
        }
        Ok(())
    }

    // ── Seed documents ────────────────────────────────────────────────────────

    /// Write seed documents only if they do not already exist.
    /// Custom memories in factory/memory/ are never overwritten.
    async fn write_seed_docs(&self, setup: &SetupConfig) -> Result<()> {
        let docs = seed_docs(&setup.setup.name);
        for (filename, content) in &docs {
            let dest = self.root.join(filename);
            if !dest.exists() {
                tokio::fs::write(&dest, content).await?;
            }
        }
        Ok(())
    }
}

fn append_unique_tag(tags: &mut Vec<String>, tag: String) {
    let normalized = tag.trim();
    if normalized.is_empty() {
        return;
    }
    if !tags.iter().any(|existing| existing == normalized) {
        tags.push(normalized.to_string());
    }
}

fn next_available_memory_id(root: &Path, raw: &str) -> String {
    let mut base = sanitize_memory_id(raw);
    if base.is_empty() {
        base = format!("memory-ingest-{}", Utc::now().format("%Y%m%d%H%M%S"));
    }
    let candidate = root.join(format!("{}.md", base));
    if !candidate.exists() {
        return base;
    }
    format!("{}-{}", base, Utc::now().format("%Y%m%d%H%M%S"))
}

async fn copy_asset_to_imports(
    imports_dir: &Path,
    source: &Path,
    note_id: &str,
) -> Result<PathBuf> {
    tokio::fs::create_dir_all(imports_dir).await?;
    let ext = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    let mut path = asset_path_for(imports_dir, note_id, ext);
    if path.exists() {
        path = asset_path_for(
            imports_dir,
            &format!("{}-asset-{}", note_id, Utc::now().format("%Y%m%d%H%M%S")),
            ext,
        );
    }
    tokio::fs::copy(source, &path)
        .await
        .with_context(|| format!("copying {} -> {}", source.display(), path.display()))?;
    Ok(path)
}

fn default_ingest_summary(extracted: &ExtractedMemorySource) -> String {
    match extracted.source_kind.as_str() {
        "url" => format!("Ingested web knowledge: {}", extracted.title),
        _ => format!(
            "Ingested {} document: {}",
            extracted.media_kind, extracted.title
        ),
    }
}

fn render_ingested_memory_body(
    extracted: &ExtractedMemorySource,
    imported_asset: Option<&PathBuf>,
    extracted_text_sidecar: Option<&PathBuf>,
    notes: Option<&str>,
) -> String {
    let highlights = summarize_extracted_text(&extracted.text, 12);
    let rel_asset = imported_asset
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not stored".to_string());
    let rel_sidecar = extracted_text_sidecar
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not stored".to_string());
    let extracted_text = truncate_for_memory(&extracted.text, 120_000);
    format!(
        "# Ingested Knowledge\n\n- Title: {}\n- Source kind: {}\n- Source locator: {}\n- Imported asset: {}\n- Extracted text sidecar: {}\n- Extraction method: {}\n- OCR applied: {}\n- Ingested at: {}\n- Extracted chars: {}\n\n## Notes\n{}\n\n## Distilled Highlights\n{}\n\n## Extracted Text\n{}\n",
        extracted.title,
        extracted.source_kind,
        extracted.source_locator,
        rel_asset,
        rel_sidecar,
        extracted.extraction_method,
        if extracted.ocr_applied { "yes" } else { "no" },
        Utc::now().to_rfc3339(),
        extracted.text.chars().count(),
        notes
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .unwrap_or("No additional notes were provided."),
        render_highlight_lines(&highlights),
        extracted_text,
    )
}

fn render_highlight_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return "- No distilled highlights were extracted automatically yet.".to_string();
    }
    lines
        .iter()
        .map(|line| format!("- {}", line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn summarize_extracted_text(text: &str, limit: usize) -> Vec<String> {
    let mut highlights = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.len() < 18 {
            continue;
        }
        if trimmed.chars().all(|ch| !ch.is_alphanumeric()) {
            continue;
        }
        if highlights.iter().any(|existing| existing == trimmed) {
            continue;
        }
        highlights.push(trimmed.to_string());
        if highlights.len() >= limit {
            break;
        }
    }
    highlights
}

fn truncate_for_memory(text: &str, max_chars: usize) -> String {
    let collected = text.chars().take(max_chars).collect::<String>();
    if text.chars().count() > max_chars {
        format!(
            "{}\n\n[truncated after {} chars during ingest]",
            collected, max_chars
        )
    } else {
        collected
    }
}

async fn extract_memory_source(source: &str) -> Result<ExtractedMemorySource> {
    if is_url_source(source) {
        extract_memory_source_from_url(source).await
    } else {
        extract_memory_source_from_file(Path::new(source)).await
    }
}

fn is_url_source(source: &str) -> bool {
    let lower = source.to_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

async fn extract_memory_source_from_url(source: &str) -> Result<ExtractedMemorySource> {
    let client = reqwest::Client::builder().build()?;
    let response = client
        .get(source)
        .header("User-Agent", "Harkonnen-Labs/0.1 memory-ingest")
        .send()
        .await?
        .error_for_status()?;
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    let url_path = reqwest::Url::parse(source)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_default();

    if content_type.contains("pdf") || url_path.to_lowercase().ends_with(".pdf") {
        let bytes = response.bytes().await?;
        return extract_remote_binary_to_text(source, &bytes, "pdf").await;
    }
    if content_type.contains("word") || url_path.to_lowercase().ends_with(".docx") {
        let bytes = response.bytes().await?;
        return extract_remote_binary_to_text(source, &bytes, "docx").await;
    }
    if content_type.contains("presentation") || url_path.to_lowercase().ends_with(".pptx") {
        let bytes = response.bytes().await?;
        return extract_remote_binary_to_text(source, &bytes, "pptx").await;
    }

    let raw = response.text().await?;
    let is_html = content_type.contains("html")
        || raw.to_lowercase().contains("<html")
        || raw.to_lowercase().contains("<body");
    let title = if is_html {
        extract_html_title(&raw).unwrap_or_else(|| derive_title_from_source(source))
    } else {
        derive_title_from_source(source)
    };
    let text = if is_html { html_to_text(&raw) } else { raw };
    let text = normalize_ingested_text(&text);
    if text.trim().is_empty() {
        bail!("no extractable text found at {}", source);
    }
    Ok(ExtractedMemorySource {
        title,
        source_kind: "url".to_string(),
        source_locator: source.to_string(),
        text,
        media_kind: if is_html {
            "html".to_string()
        } else {
            "text".to_string()
        },
        extension_tag: Some(if is_html {
            "html".to_string()
        } else {
            "txt".to_string()
        }),
        asset_source_path: None,
        extraction_method: if is_html {
            "http_html_to_text".to_string()
        } else {
            "http_text".to_string()
        },
        ocr_applied: false,
    })
}

async fn extract_remote_binary_to_text(
    source: &str,
    bytes: &[u8],
    ext: &str,
) -> Result<ExtractedMemorySource> {
    let temp_path = std::env::temp_dir().join(format!(
        "harkonnen-memory-ingest-{}.{}",
        Uuid::new_v4(),
        ext
    ));
    tokio::fs::write(&temp_path, bytes).await?;
    let result = extract_memory_source_from_file(&temp_path).await;
    let _ = tokio::fs::remove_file(&temp_path).await;
    let mut extracted = result?;
    extracted.source_kind = "url".to_string();
    extracted.source_locator = source.to_string();
    extracted.asset_source_path = None;
    Ok(extracted)
}

async fn extract_memory_source_from_file(source: &Path) -> Result<ExtractedMemorySource> {
    if !source.exists() {
        bail!("memory ingest source not found: {}", source.display());
    }
    if !source.is_file() {
        bail!("memory ingest source is not a file: {}", source.display());
    }

    let canonical = source.canonicalize()?;
    let ext = canonical
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_lowercase();
    let title = derive_title_from_path(&canonical);
    let media_kind = detect_media_kind(&canonical).to_string();
    let extracted_payload = match ext.as_str() {
        "txt" | "md" | "csv" | "json" | "toml" | "yaml" | "yml" | "log" => {
            ExtractedTextPayload::new(tokio::fs::read_to_string(&canonical).await?, "read_to_string", false)
        }
        "html" | "htm" | "xml" => {
            let raw = tokio::fs::read_to_string(&canonical).await?;
            ExtractedTextPayload::new(html_to_text(&raw), "html_to_text", false)
        }
        "pdf" => extract_pdf_text(&canonical).await?,
        "png" | "jpg" | "jpeg" | "gif" | "webp" => extract_image_text_with_ocr(&canonical).await?,
        "docx" => extract_docx_text(&canonical).await?,
        "pptx" => extract_pptx_text(&canonical).await?,
        "doc" | "ppt" | "odt" | "odp" => extract_with_libreoffice(&canonical).await?,
        _ => match tokio::fs::read_to_string(&canonical).await {
            Ok(text) => ExtractedTextPayload::new(text, "read_to_string", false),
            Err(_) => bail!(
                "unsupported ingest format for {}. Use memory import for raw asset storage or convert it to txt/pdf/docx/pptx/html first",
                canonical.display()
            ),
        },
    };
    let text = normalize_ingested_text(&extracted_payload.text);
    if text.trim().is_empty() {
        bail!("no extractable text found in {}", canonical.display());
    }
    Ok(ExtractedMemorySource {
        title,
        source_kind: "file".to_string(),
        source_locator: canonical.display().to_string(),
        text,
        media_kind,
        extension_tag: if ext.is_empty() { None } else { Some(ext) },
        asset_source_path: Some(canonical),
        extraction_method: extracted_payload.extraction_method,
        ocr_applied: extracted_payload.ocr_applied,
    })
}

async fn extract_pdf_text(source: &Path) -> Result<ExtractedTextPayload> {
    if let Some(text) = try_extract_pdf_text_native(source).await? {
        return Ok(ExtractedTextPayload::new(text, "pdftotext", false));
    }

    if let Some(text) =
        try_extract_text_via_external_command(source, "HARKONNEN_MEMORY_PDF_EXTRACT_COMMAND")
            .await?
    {
        return Ok(ExtractedTextPayload::new(
            text,
            "external_pdf_extractor",
            false,
        ));
    }

    if let Some(text) = try_extract_pdf_text_with_ocr(source).await? {
        return Ok(ExtractedTextPayload::new(text, "ocr_pdf", true));
    }

    bail!(
        "no extractable text found in {}. Install pdftotext, configure HARKONNEN_MEMORY_PDF_EXTRACT_COMMAND, or install pdftoppm+tesseract for OCR fallback",
        source.display()
    );
}

async fn try_extract_pdf_text_native(source: &Path) -> Result<Option<String>> {
    if !command_available("pdftotext") {
        return Ok(None);
    }
    let output = Command::new("pdftotext")
        .arg("-layout")
        .arg(source)
        .arg("-")
        .output()
        .await
        .with_context(|| format!("running pdftotext for {}", source.display()))?;
    if !output.status.success() {
        tracing::warn!(
            "pdftotext failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(None);
    }
    let text = normalize_ingested_text(&String::from_utf8_lossy(&output.stdout));
    if text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(text))
}

async fn extract_docx_text(source: &Path) -> Result<ExtractedTextPayload> {
    Ok(ExtractedTextPayload::new(
        extract_zip_text_with_python(source, "docx").await?,
        "docx_zip_xml",
        false,
    ))
}

async fn extract_pptx_text(source: &Path) -> Result<ExtractedTextPayload> {
    Ok(ExtractedTextPayload::new(
        extract_zip_text_with_python(source, "pptx").await?,
        "pptx_zip_xml",
        false,
    ))
}

async fn extract_zip_text_with_python(source: &Path, mode: &str) -> Result<String> {
    let script = if mode == "docx" {
        r#"import html, re, sys, zipfile
path = sys.argv[1]
parts = []
with zipfile.ZipFile(path) as zf:
    names = sorted(name for name in zf.namelist() if name.startswith('word/') and name.endswith('.xml'))
    for name in names:
        data = zf.read(name).decode('utf-8', 'ignore')
        data = re.sub(r'</w:p[^>]*>', '\n', data)
        data = re.sub(r'<[^>]+>', ' ', data)
        data = html.unescape(data)
        data = re.sub(r'\s+', ' ', data)
        data = re.sub(r' ?\n ?', '\n', data)
        if data.strip():
            parts.append(data.strip())
print('\n\n'.join(parts))"#
    } else {
        r#"import html, re, sys, zipfile
path = sys.argv[1]
parts = []
with zipfile.ZipFile(path) as zf:
    names = sorted(name for name in zf.namelist() if name.startswith('ppt/slides/') and name.endswith('.xml'))
    for name in names:
        data = zf.read(name).decode('utf-8', 'ignore')
        data = re.sub(r'</a:p[^>]*>', '\n', data)
        data = re.sub(r'<[^>]+>', ' ', data)
        data = html.unescape(data)
        data = re.sub(r'\s+', ' ', data)
        data = re.sub(r' ?\n ?', '\n', data)
        if data.strip():
            parts.append(data.strip())
print('\n\n'.join(parts))"#
    };
    let output = Command::new("python3")
        .arg("-c")
        .arg(script)
        .arg(source)
        .output()
        .await
        .with_context(|| format!("extracting zipped office text from {}", source.display()))?;
    if !output.status.success() {
        bail!(
            "python office extractor failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn extract_with_libreoffice(source: &Path) -> Result<ExtractedTextPayload> {
    let out_dir = std::env::temp_dir().join(format!("harkonnen-memory-lo-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&out_dir).await?;
    let output = Command::new("libreoffice")
        .arg("--headless")
        .arg("--convert-to")
        .arg("txt:Text")
        .arg("--outdir")
        .arg(&out_dir)
        .arg(source)
        .output()
        .await
        .with_context(|| format!("running libreoffice for {}", source.display()))?;
    if !output.status.success() {
        let _ = tokio::fs::remove_dir_all(&out_dir).await;
        bail!(
            "libreoffice conversion failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let mut entries = tokio::fs::read_dir(&out_dir).await?;
    let mut text = None;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("txt") {
            text = Some(tokio::fs::read_to_string(&path).await?);
            break;
        }
    }
    let _ = tokio::fs::remove_dir_all(&out_dir).await;
    let text = text.with_context(|| {
        format!(
            "libreoffice did not produce a txt output for {}",
            source.display()
        )
    })?;
    Ok(ExtractedTextPayload::new(text, "libreoffice_txt", false))
}

async fn extract_image_text_with_ocr(source: &Path) -> Result<ExtractedTextPayload> {
    if !command_available("tesseract") {
        bail!(
            "OCR requires tesseract for {}. Install tesseract or use memory import for raw asset storage",
            source.display()
        );
    }
    let output = Command::new("tesseract")
        .arg(source)
        .arg("stdout")
        .output()
        .await
        .with_context(|| format!("running tesseract for {}", source.display()))?;
    if !output.status.success() {
        bail!(
            "tesseract failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(ExtractedTextPayload::new(
        String::from_utf8_lossy(&output.stdout).to_string(),
        "ocr_image",
        true,
    ))
}

async fn try_extract_pdf_text_with_ocr(source: &Path) -> Result<Option<String>> {
    if !command_available("pdftoppm") || !command_available("tesseract") {
        return Ok(None);
    }

    let temp_dir =
        std::env::temp_dir().join(format!("harkonnen-memory-pdf-ocr-{}", Uuid::new_v4()));
    tokio::fs::create_dir_all(&temp_dir).await?;
    let page_prefix = temp_dir.join("page");
    let render_output = Command::new("pdftoppm")
        .arg("-png")
        .arg(source)
        .arg(&page_prefix)
        .output()
        .await
        .with_context(|| format!("running pdftoppm for {}", source.display()))?;
    if !render_output.status.success() {
        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
        tracing::warn!(
            "pdftoppm failed for {}: {}",
            source.display(),
            String::from_utf8_lossy(&render_output.stderr).trim()
        );
        return Ok(None);
    }

    let mut pages = Vec::new();
    let mut entries = tokio::fs::read_dir(&temp_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) == Some("png") {
            pages.push(path);
        }
    }
    pages.sort();

    let mut parts = Vec::new();
    for page in &pages {
        let output = Command::new("tesseract")
            .arg(page)
            .arg("stdout")
            .output()
            .await
            .with_context(|| format!("running tesseract for {}", page.display()))?;
        if !output.status.success() {
            tracing::warn!(
                "tesseract failed for OCR page {}: {}",
                page.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            );
            continue;
        }
        let text = normalize_ingested_text(&String::from_utf8_lossy(&output.stdout));
        if !text.trim().is_empty() {
            parts.push(text);
        }
    }

    let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    if parts.is_empty() {
        return Ok(None);
    }
    Ok(Some(parts.join("\n\n")))
}

async fn try_extract_text_via_external_command(
    source: &Path,
    env_var: &str,
) -> Result<Option<String>> {
    let template = match std::env::var(env_var) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return Ok(None),
    };

    let source_arg = shell_escape_arg(&source.to_string_lossy());
    let raw_command = if template.contains("{source}") {
        template.replace("{source}", &source_arg)
    } else {
        format!("{template} {source_arg}")
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(&raw_command);
        command
    };

    #[cfg(not(target_os = "windows"))]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        command.arg("-lc").arg(&raw_command);
        command
    };

    let output = command.output().await.with_context(|| {
        format!(
            "running external extractor '{}' for {}",
            env_var,
            source.display()
        )
    })?;
    if !output.status.success() {
        tracing::warn!(
            "external extractor {} failed for {}: {}",
            env_var,
            source.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(None);
    }

    let text = normalize_ingested_text(&String::from_utf8_lossy(&output.stdout));
    if text.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(text))
}

fn shell_escape_arg(arg: &str) -> String {
    #[cfg(target_os = "windows")]
    {
        format!("\"{}\"", arg.replace('"', "\\\""))
    }

    #[cfg(not(target_os = "windows"))]
    {
        format!("'{}'", arg.replace('\'', "'\"'\"'"))
    }
}

fn derive_title_from_path(source: &Path) -> String {
    source
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.replace(['_', '-'], " "))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| source.display().to_string())
}

fn derive_title_from_source(source: &str) -> String {
    source
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .map(|value| value.replace(['_', '-'], " "))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| source.to_string())
}

fn extract_html_title(raw: &str) -> Option<String> {
    let lower = raw.to_lowercase();
    let start = lower.find("<title")?;
    let after_open = lower[start..].find('>')? + start + 1;
    let end = lower[after_open..].find("</title>")? + after_open;
    let title = raw[after_open..end].trim();
    if title.is_empty() {
        None
    } else {
        Some(decode_basic_html_entities(title))
    }
}

fn html_to_text(raw: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    let mut in_script = false;
    let mut in_style = false;

    for ch in raw.chars() {
        if in_tag {
            if ch == '>' {
                let lowered = tag.trim().to_lowercase();
                if lowered.starts_with("script") {
                    in_script = true;
                } else if lowered.starts_with("/script") {
                    in_script = false;
                } else if lowered.starts_with("style") {
                    in_style = true;
                } else if lowered.starts_with("/style") {
                    in_style = false;
                } else if lowered.starts_with("br")
                    || lowered.starts_with("/p")
                    || lowered.starts_with("/div")
                    || lowered.starts_with("/li")
                    || lowered.starts_with("/tr")
                    || lowered.starts_with("h1")
                    || lowered.starts_with("h2")
                    || lowered.starts_with("h3")
                    || lowered.starts_with("/h1")
                    || lowered.starts_with("/h2")
                    || lowered.starts_with("/h3")
                {
                    out.push('\n');
                }
                tag.clear();
                in_tag = false;
            } else {
                tag.push(ch);
            }
            continue;
        }

        if ch == '<' {
            in_tag = true;
            tag.clear();
            continue;
        }

        if !in_script && !in_style {
            out.push(ch);
        }
    }

    normalize_ingested_text(&decode_basic_html_entities(&out))
}

fn decode_basic_html_entities(raw: &str) -> String {
    raw.replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

fn normalize_ingested_text(raw: &str) -> String {
    let mut normalized = String::new();
    let mut blank_run = 0usize;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                normalized.push('\n');
            }
            continue;
        }
        blank_run = 0;
        normalized.push_str(trimmed);
        normalized.push('\n');
    }
    normalized.trim().to_string()
}

async fn read_memory_index_if_present(path: &Path) -> Result<Option<MemoryIndex>> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(read_memory_index(path).await?))
}

async fn read_memory_index(path: &Path) -> Result<MemoryIndex> {
    let json = tokio::fs::read_to_string(path).await?;
    serde_json::from_str(&json).context("reading memory index")
}

async fn write_memory_index(path: &Path, index: &MemoryIndex) -> Result<()> {
    let json = serde_json::to_string_pretty(index).context("serializing memory index")?;
    tokio::fs::write(path, json).await?;
    Ok(())
}

fn memory_entry_stats_map(index: Option<&MemoryIndex>) -> HashMap<String, MemoryEntryStats> {
    index
        .map(|index| {
            index
                .entries
                .iter()
                .map(|entry| {
                    (
                        entry.id.clone(),
                        MemoryEntryStats {
                            recall_count: entry.recall_count,
                            last_recalled: entry.last_recalled.clone(),
                            loaded_for_run_count: entry.loaded_for_run_count,
                            contributed_to_success_count: entry.contributed_to_success_count,
                            contributed_to_failure_count: entry.contributed_to_failure_count,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}
fn apply_memory_entry_stats(entry: &mut MemoryEntry, stats: Option<&MemoryEntryStats>) {
    let Some(stats) = stats else {
        return;
    };
    entry.recall_count = stats.recall_count;
    entry.last_recalled = stats.last_recalled.clone();
    entry.loaded_for_run_count = stats.loaded_for_run_count;
    entry.contributed_to_success_count = stats.contributed_to_success_count;
    entry.contributed_to_failure_count = stats.contributed_to_failure_count;
}

fn render_memory_hits(query: &str, hits: &[MemoryRetrievalHit]) -> Vec<String> {
    if hits.is_empty() {
        vec![format!("No memories found for: {query}")]
    } else {
        hits.iter()
            .map(|hit| {
                let note = format_memory_hit_note(hit);
                if note.is_empty() {
                    format!(
                        "[{}] {}
{}",
                        hit.id, hit.summary, hit.snippet
                    )
                } else {
                    format!(
                        "[{}] {}
{}
{}",
                        hit.id, hit.summary, note, hit.snippet
                    )
                }
            })
            .collect()
    }
}

fn format_memory_hit_note(hit: &MemoryRetrievalHit) -> String {
    let mut parts = Vec::new();
    if let Some(status) = hit.status.as_deref() {
        if !status.trim().is_empty() {
            parts.push(format!("status={}", status.trim()));
        }
    }
    if let Some(superseded_by) = hit.superseded_by.as_deref() {
        if !superseded_by.trim().is_empty() {
            parts.push(format!("superseded_by={}", superseded_by.trim()));
        }
    }
    if !hit.challenged_by.is_empty() {
        parts.push(format!("challenged_by={}", hit.challenged_by.join(", ")));
    }
    if !hit.surfaced_via.is_empty() {
        parts.push(format!("via {}", hit.surfaced_via.join("; ")));
    }
    if !hit.invalidation_reasons.is_empty() {
        parts.push(hit.invalidation_reasons.join("; "));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("note: {}", parts.join(" | "))
    }
}

fn memory_matches(entry: &MemoryEntry, query: &str) -> bool {
    entry.summary.to_lowercase().contains(query)
        || entry
            .tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(query))
        || entry.content.to_lowercase().contains(query)
}

fn memory_match_score(entry: &MemoryEntry, query: &str) -> i64 {
    let mut score = 0_i64;
    let summary = entry.summary.to_lowercase();
    let content = entry.content.to_lowercase();
    if summary.contains(query) {
        score += 40;
    }
    if entry
        .tags
        .iter()
        .any(|tag| tag.to_lowercase().contains(query))
    {
        score += 25;
    }
    if content.contains(query) {
        score += 15;
    }
    score += entry.contributed_to_success_count * 10;
    score -= entry.contributed_to_failure_count * 4;
    score += entry.recall_count.min(20);
    score
}

fn adjust_retrieval_score_for_provenance(base_score: f32, entry: &MemoryEntry) -> f32 {
    let mut multiplier = 1.0_f32;
    if let Some(status) = entry.provenance.status.as_deref() {
        match status.trim().to_ascii_lowercase().as_str() {
            "superseded" => multiplier = multiplier.min(0.08),
            "challenged" => multiplier = multiplier.min(0.55),
            "stale" | "deprecated" => multiplier = multiplier.min(0.7),
            _ => {}
        }
    }
    if entry.provenance.superseded_by.is_some() {
        multiplier = multiplier.min(0.08);
    }
    if !entry.provenance.challenged_by.is_empty() {
        multiplier = multiplier.min(0.55);
    }
    base_score * multiplier
}

fn memory_invalidation_reasons(
    entry: &MemoryEntry,
    entry_map: &HashMap<&str, &MemoryEntry>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if let Some(status) = entry.provenance.status.as_deref() {
        let status = status.trim();
        if !status.is_empty() {
            reasons.push(format!("status={}", status));
        }
    }
    if let Some(superseded_by) = entry.provenance.superseded_by.as_deref() {
        let superseded_by = superseded_by.trim();
        if !superseded_by.is_empty() {
            if let Some(successor) = entry_map.get(superseded_by).copied() {
                reasons.push(format!(
                    "superseded by {} ({})",
                    successor.id, successor.summary
                ));
            } else {
                reasons.push(format!("superseded by {}", superseded_by));
            }
        }
    }
    if !entry.provenance.challenged_by.is_empty() {
        reasons.push(format!(
            "challenged by {}",
            entry.provenance.challenged_by.join(", ")
        ));
    }
    reasons
}

fn upsert_memory_candidate<'a>(
    candidates: &mut HashMap<String, MemoryRetrievalCandidate<'a>>,
    entry: &'a MemoryEntry,
    score: f32,
    invalidation_reasons: Vec<String>,
    surfaced_via: Vec<String>,
) {
    if let Some(existing) = candidates.get_mut(&entry.id) {
        existing.score = existing.score.max(score);
        extend_unique_strings(&mut existing.invalidation_reasons, invalidation_reasons);
        extend_unique_strings(&mut existing.surfaced_via, surfaced_via);
    } else {
        candidates.insert(
            entry.id.clone(),
            MemoryRetrievalCandidate {
                score,
                entry,
                invalidation_reasons,
                surfaced_via,
            },
        );
    }
}

fn extend_unique_strings(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.iter().any(|existing| existing == &value) {
            target.push(value);
        }
    }
}

fn keyword_ranked_entries<'a>(
    entries: &'a [MemoryEntry],
    query: &str,
) -> Vec<(f32, &'a MemoryEntry)> {
    entries
        .iter()
        .filter(|entry| memory_matches(entry, query))
        .map(|entry| (memory_match_score(entry, query) as f32, entry))
        .collect()
}

fn collect_entries(root: &Path, current: &Path, entries: &mut Vec<MemoryEntry>) -> Result<()> {
    for dent in
        std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))?
    {
        let dent = dent?;
        let path = dent.path();
        let file_type = dent.file_type()?;
        if file_type.is_dir() {
            collect_entries(root, &path, entries)?;
            continue;
        }
        if !file_type.is_file() || should_skip_memory_file(&path) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("stripping prefix {}", root.display()))?;
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let parsed = parse_frontmatter(&raw);
            entries.push(MemoryEntry {
                id: relative_entry_id(relative),
                tags: parsed.tags,
                summary: parsed.summary,
                content: parsed.body,
                created_at: file_created_at(&path)?,
                provenance: parsed.provenance,
                recall_count: 0,
                last_recalled: None,
                loaded_for_run_count: 0,
                contributed_to_success_count: 0,
                contributed_to_failure_count: 0,
            });
            continue;
        }
        if !is_supported_asset(&path) || has_markdown_sidecar(&path) {
            continue;
        }
        let media_kind = detect_media_kind(&path).to_string();
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("asset");
        let rel = relative.display().to_string();
        entries.push(MemoryEntry {
            id: relative_entry_id(relative),
            tags: vec!["asset".to_string(), media_kind.clone(), asset_extension_tag(&path)],
            summary: format!("Imported {} asset {}", media_kind, filename),
            content: format!(
                "Asset path: {}
Filename: {}
Media type: {}
This asset is available to Coobie as reference material and can be synced to AnythingLLM for richer retrieval.",
                rel,
                filename,
                media_kind
            ),
            created_at: file_created_at(&path)?,
            provenance: MemoryProvenance::default(),
            recall_count: 0,
            last_recalled: None,
            loaded_for_run_count: 0,
            contributed_to_success_count: 0,
            contributed_to_failure_count: 0,
        });
    }
    Ok(())
}

fn should_skip_memory_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|value| value.to_str()),
        Some("index.json") | Some("store.json")
    )
}

fn latest_relevant_mtime(root: &Path, current: &Path) -> Result<SystemTime> {
    let mut latest = SystemTime::UNIX_EPOCH;
    for dent in
        std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))?
    {
        let dent = dent?;
        let path = dent.path();
        let file_type = dent.file_type()?;
        if file_type.is_dir() {
            let child = latest_relevant_mtime(root, &path)?;
            if child > latest {
                latest = child;
            }
            continue;
        }
        if !file_type.is_file() || should_skip_memory_file(&path) {
            continue;
        }
        let modified = std::fs::metadata(&path)
            .with_context(|| format!("reading metadata for {}", path.display()))?
            .modified()?;
        if modified > latest {
            latest = modified;
        }
    }
    let _ = root;
    Ok(latest)
}

fn relative_entry_id(relative: &Path) -> String {
    let mut parts = Vec::new();
    for component in relative.components() {
        let value = component.as_os_str().to_string_lossy();
        let mut cleaned = sanitize_memory_id(&value);
        if cleaned.ends_with("-md") {
            cleaned.truncate(cleaned.len().saturating_sub(3));
        }
        if !cleaned.is_empty() {
            parts.push(cleaned);
        }
    }
    parts.join("__")
}

fn has_markdown_sidecar(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    parent.join(format!("{}.md", stem)).exists()
}

fn is_supported_asset(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|value| value.to_str()).map(|value| value.to_ascii_lowercase()),
        Some(ext) if matches!(ext.as_str(), "pdf" | "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "txt" | "csv" | "doc" | "docx" | "ppt" | "pptx" | "xls" | "xlsx")
    )
}

fn asset_extension_tag(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| "asset".to_string())
}

fn detect_media_kind(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("pdf") => "pdf",
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("svg") => {
            "image"
        }
        Some("txt") | Some("csv") | Some("doc") | Some("docx") | Some("ppt") | Some("pptx")
        | Some("xls") | Some("xlsx") => "document",
        _ => "asset",
    }
}

fn should_write_extracted_text_sidecar(extracted: &ExtractedMemorySource) -> bool {
    matches!(extracted.media_kind.as_str(), "pdf" | "image" | "document")
        && extracted.extension_tag.as_deref().is_none_or(|ext| {
            !matches!(
                ext,
                "txt"
                    | "md"
                    | "csv"
                    | "json"
                    | "toml"
                    | "yaml"
                    | "yml"
                    | "log"
                    | "html"
                    | "htm"
                    | "xml"
            )
        })
}

async fn write_extracted_text_sidecar(asset_path: &Path, text: &str) -> Result<PathBuf> {
    let sidecar_path = extracted_text_sidecar_path(asset_path);
    tokio::fs::write(&sidecar_path, text).await?;
    Ok(sidecar_path)
}

fn extracted_text_sidecar_path(asset_path: &Path) -> PathBuf {
    let stem = asset_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("extracted");
    let parent = asset_path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!("{stem}.extracted.txt"))
}

fn asset_path_for(imports_dir: &Path, id: &str, ext: &str) -> PathBuf {
    if ext.is_empty() {
        imports_dir.join(id)
    } else {
        imports_dir.join(format!("{}.{}", id, ext))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extracted_text_sidecar_path, shell_escape_arg, should_write_extracted_text_sidecar,
        ExtractedMemorySource,
    };
    use std::path::Path;

    #[test]
    fn extracted_text_sidecar_uses_asset_stem() {
        let path = extracted_text_sidecar_path(Path::new("/tmp/scan.pdf"));
        assert_eq!(path, Path::new("/tmp/scan.extracted.txt"));
    }

    #[test]
    fn binary_ingests_request_text_sidecar() {
        let extracted = ExtractedMemorySource {
            title: "scan".to_string(),
            source_kind: "file".to_string(),
            source_locator: "/tmp/scan.pdf".to_string(),
            text: "hello".to_string(),
            media_kind: "pdf".to_string(),
            extension_tag: Some("pdf".to_string()),
            asset_source_path: None,
            extraction_method: "pdftotext".to_string(),
            ocr_applied: false,
        };
        assert!(should_write_extracted_text_sidecar(&extracted));
    }

    #[test]
    fn plain_text_ingests_skip_sidecar() {
        let extracted = ExtractedMemorySource {
            title: "note".to_string(),
            source_kind: "file".to_string(),
            source_locator: "/tmp/note.txt".to_string(),
            text: "hello".to_string(),
            media_kind: "document".to_string(),
            extension_tag: Some("txt".to_string()),
            asset_source_path: None,
            extraction_method: "read_to_string".to_string(),
            ocr_applied: false,
        };
        assert!(!should_write_extracted_text_sidecar(&extracted));
    }

    #[test]
    fn shell_escape_contains_wrapped_argument() {
        let escaped = shell_escape_arg("/tmp/with spaces/file.pdf");
        assert!(escaped.starts_with('\'') || escaped.starts_with('"'));
    }
}

fn file_created_at(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("reading metadata for {}", path.display()))?;
    let timestamp = metadata
        .modified()
        .or_else(|_| metadata.created())
        .map(chrono::DateTime::<Utc>::from)
        .unwrap_or_else(|_| Utc::now());
    Ok(timestamp.to_rfc3339())
}

fn sanitize_memory_id(value: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

// ── Frontmatter parser ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
struct ParsedFrontmatter {
    tags: Vec<String>,
    summary: String,
    body: String,
    provenance: MemoryProvenance,
}

/// Extract frontmatter plus body from an optional `---` metadata block.
fn parse_frontmatter(raw: &str) -> ParsedFrontmatter {
    if !raw.starts_with("---") {
        return ParsedFrontmatter {
            tags: vec![],
            summary: String::new(),
            body: raw.to_string(),
            provenance: MemoryProvenance::default(),
        };
    }

    let rest = &raw[3..];
    let Some(end) = rest.find("\n---") else {
        return ParsedFrontmatter {
            tags: vec![],
            summary: String::new(),
            body: raw.to_string(),
            provenance: MemoryProvenance::default(),
        };
    };

    let front = &rest[..end];
    let body = rest[end + 4..].trim_start_matches('\n').to_string();

    ParsedFrontmatter {
        tags: extract_frontmatter_list(front, "tags"),
        summary: extract_frontmatter_scalar(front, "summary"),
        body,
        provenance: MemoryProvenance {
            source_label: extract_optional_frontmatter_scalar(front, "source_label"),
            source_kind: extract_optional_frontmatter_scalar(front, "source_kind"),
            source_path: extract_optional_frontmatter_scalar(front, "source_path"),
            source_run_id: extract_optional_frontmatter_scalar(front, "source_run_id"),
            source_spec_id: extract_optional_frontmatter_scalar(front, "source_spec_id"),
            git_branch: extract_optional_frontmatter_scalar(front, "git_branch"),
            git_commit: extract_optional_frontmatter_scalar(front, "git_commit"),
            git_remote: extract_optional_frontmatter_scalar(front, "git_remote"),
            evidence_run_ids: extract_frontmatter_list(front, "evidence_run_ids"),
            stale_when: extract_frontmatter_list(front, "stale_when"),
            observed_paths: extract_frontmatter_list(front, "observed_paths"),
            code_under_test_paths: extract_frontmatter_list(front, "code_under_test_paths"),
            observed_surfaces: extract_frontmatter_list(front, "observed_surfaces"),
            status: extract_optional_frontmatter_scalar(front, "status"),
            superseded_by: extract_optional_frontmatter_scalar(front, "superseded_by"),
            challenged_by: extract_frontmatter_list(front, "challenged_by"),
        },
    }
}

fn extract_frontmatter_list(front: &str, key: &str) -> Vec<String> {
    for line in front.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}:")) {
            let trimmed = rest.trim().trim_start_matches('[').trim_end_matches(']');
            return trimmed
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    vec![]
}

fn extract_frontmatter_scalar(front: &str, key: &str) -> String {
    extract_optional_frontmatter_scalar(front, key).unwrap_or_default()
}

fn extract_optional_frontmatter_scalar(front: &str, key: &str) -> Option<String> {
    for line in front.lines() {
        if let Some(rest) = line.strip_prefix(&format!("{key}:")) {
            let value = rest.trim();
            if value.is_empty() {
                return None;
            }
            return Some(value.to_string());
        }
    }
    None
}

fn render_memory_document(
    tags: &[String],
    summary: &str,
    content: &str,
    provenance: &MemoryProvenance,
) -> String {
    let mut frontmatter = vec![
        format!("tags: [{}]", tags.join(", ")),
        format!("summary: {}", summary),
    ];

    push_frontmatter_scalar(
        &mut frontmatter,
        "source_label",
        provenance.source_label.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "source_kind",
        provenance.source_kind.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "source_path",
        provenance.source_path.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "source_run_id",
        provenance.source_run_id.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "source_spec_id",
        provenance.source_spec_id.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "git_branch",
        provenance.git_branch.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "git_commit",
        provenance.git_commit.as_deref(),
    );
    push_frontmatter_scalar(
        &mut frontmatter,
        "git_remote",
        provenance.git_remote.as_deref(),
    );
    push_frontmatter_list(
        &mut frontmatter,
        "evidence_run_ids",
        &provenance.evidence_run_ids,
    );
    push_frontmatter_list(&mut frontmatter, "stale_when", &provenance.stale_when);
    push_frontmatter_list(
        &mut frontmatter,
        "observed_paths",
        &provenance.observed_paths,
    );
    push_frontmatter_list(
        &mut frontmatter,
        "code_under_test_paths",
        &provenance.code_under_test_paths,
    );
    push_frontmatter_list(
        &mut frontmatter,
        "observed_surfaces",
        &provenance.observed_surfaces,
    );
    push_frontmatter_scalar(&mut frontmatter, "status", provenance.status.as_deref());
    push_frontmatter_scalar(
        &mut frontmatter,
        "superseded_by",
        provenance.superseded_by.as_deref(),
    );
    push_frontmatter_list(&mut frontmatter, "challenged_by", &provenance.challenged_by);

    format!("---\n{}\n---\n\n{}", frontmatter.join("\n"), content)
}

fn annotate_memory_provenance_status(
    provenance: &mut MemoryProvenance,
    status: &str,
    related_id: Option<&str>,
) {
    provenance.status = Some(status.to_string());
    match status {
        "superseded" => {
            if let Some(related_id) = related_id {
                provenance.superseded_by = Some(related_id.to_string());
            }
        }
        "challenged" => {
            if let Some(related_id) = related_id {
                append_unique_string(&mut provenance.challenged_by, related_id.to_string());
            }
        }
        _ => {}
    }
}

fn append_unique_string(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

fn push_frontmatter_scalar(lines: &mut Vec<String>, key: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    let value = value.trim();
    if value.is_empty() {
        return;
    }
    lines.push(format!("{}: {}", key, value));
}

fn push_frontmatter_list(lines: &mut Vec<String>, key: &str, values: &[String]) {
    if values.is_empty() {
        return;
    }
    lines.push(format!("{}: [{}]", key, values.join(", ")));
}

// ── Seed document content ─────────────────────────────────────────────────────

/// Returns (filename, content) pairs for each seed document.
/// Called once at `harkonnen memory init`. Files are written only if absent.
fn seed_docs(setup_name: &str) -> Vec<(String, String)> {
    vec![
        (
            "00-system-context.md".into(),
            format!(
                r#"---
tags: [system, overview, factory, harkonnen-labs]
summary: Harkonnen Labs system overview, purpose, and architecture
---

# Harkonnen Labs — System Context

Active setup: {setup_name}

## What It Is

A local-first, spec-driven AI software factory. Humans define intent in YAML specs.
A pack of specialist agents (Scout, Mason, Piper, Bramble, Sable, Ash, Flint, Coobie, Keeper)
perform the implementation work inside a constrained, observable factory.

## Core Goals

- Spec-first: precise intent precedes all implementation
- Outcome-based: acceptance is driven by behavioral evidence, not code review
- Local-first: all state (specs, runs, artifacts, memory) lives on disk
- Role separation: each agent has bounded tools and responsibilities
- Compound learning: each run makes future runs better via Coobie

## Key Directories

- factory/specs/        YAML spec files (the factory's input)
- factory/scenarios/    Hidden behavioral scenarios (Sable only)
- factory/workspaces/   Per-run isolated workspaces
- factory/artifacts/    Output bundles after each run
- factory/memory/       Coobie's memory store (this directory)
- factory/logs/         Run logs
- products/             Target product codebases
- src/                  Rust source for the factory CLI
- setups/               Named setup TOML files (one per environment)

## CLI Entry Points

    cargo run -- spec validate <file>
    cargo run -- run start <spec> --product <name>
    cargo run -- run status <run-id>
    cargo run -- run report <run-id>
    cargo run -- artifact package <run-id>
    cargo run -- memory init
    cargo run -- memory index
    cargo run -- setup check

## Current Status (MVP)

Spec loading, run creation, workspace isolation, artifact packaging, and memory
indexing are implemented. Agent execution adapters, hidden scenario evaluation,
and digital twin provisioning are planned for the next build layer.
"#
            ),
        ),
        (
            "01-agent-roster.md".into(),
            r#"---
tags: [agents, roster, roles, tools, provider]
summary: Full roster of the nine specialist agents with roles, tools, and provider assignments
---

# Agent Roster

Each agent has a bounded role, explicit tool permissions, and a provider assignment.
Trust-critical agents (Scout, Sable, Keeper) pin to Claude.
Implementation agents use the setup's default provider and can be swapped.

| Agent   | Role                | Provider  | Key Allowed Tools                          |
|---------|---------------------|-----------|--------------------------------------------|
| Scout   | Spec retriever      | claude    | filesystem_read, spec_loader, mcp:filesystem |
| Mason   | Build retriever     | default   | workspace_write, filesystem_read, mcp:filesystem |
| Piper   | Tool retriever      | default   | build_runner, filesystem_read, container_runner |
| Bramble | Test retriever      | default   | build_runner, workspace_write, mcp:filesystem |
| Sable   | Scenario retriever  | claude    | scenario_store, twin_runner, report_writer |
| Ash     | Twin retriever      | default   | container_runner, filesystem_read          |
| Flint   | Artifact retriever  | default   | artifact_writer, filesystem_read           |
| Coobie  | Memory retriever    | default   | memory_store, metadata_query, mcp:memory   |
| Keeper  | Boundary retriever  | claude    | policy_engine, filesystem_read, secret_scanner |

## Key Invariants

- Mason cannot read scenario_store (prevents test gaming)
- Sable cannot write implementation code
- Keeper alone holds policy_engine access
- All agents share the labrador personality: loyal, persistent, honest, non-bluffing

## Profile Files

factory/agents/profiles/<name>.yaml — one file per agent
factory/agents/personality/labrador.md — shared personality
"#
            .into(),
        ),
        (
            "02-setup-guide.md".into(),
            format!(
                r#"---
tags: [setup, configuration, providers, environments, windows, linux]
summary: How to switch between setup environments and what each provides
---

# Setup Guide

Active setup: {setup_name}

## Setup Files

| File                          | Environment            | Providers           | Docker? |
|-------------------------------|------------------------|---------------------|---------|
| harkonnen.toml                | Home Linux (default)   | Claude+Gemini+Codex | Yes     |
| setups/home-linux.toml        | Home Linux (explicit)  | Claude+Gemini+Codex | Yes     |
| setups/work-windows.toml      | Work Windows           | Claude only         | No      |
| setups/ci.toml                | CI / GitHub Actions    | Claude Haiku only   | No      |

## Switching Setups

Linux/Mac:
    export HARKONNEN_SETUP=work-windows
    cargo run -- setup check

Windows (PowerShell):
    $env:HARKONNEN_SETUP = "work-windows"
    cargo run -- setup check

## Provider Routing

Agents with `provider: default` use whichever provider is named in `[providers] default`.
Agents with `provider: claude` always use Claude regardless of setup.
Change the default provider by editing `[providers] default = "gemini"` in the active TOML.

## Work Windows — No Docker Needed

AnythingLLM and OpenClaw are replaced by:
- @modelcontextprotocol/server-memory  (Coobie's memory/RAG)
- @modelcontextprotocol/server-filesystem (file access for all agents)
- Claude's 200K context (large-context retrieval without chunking)
- Claude Code's native MCP support

Prerequisites: Node.js, Rust/cargo, ANTHROPIC_API_KEY.

Bootstrap:
    .\scripts\bootstrap-windows.ps1
    # set ANTHROPIC_API_KEY in .env
    cargo run -- memory init
    cargo run -- setup check
"#
            ),
        ),
        (
            "03-mcp-tools.md".into(),
            r#"---
tags: [mcp, tools, servers, integration, filesystem, memory, github, sqlite, brave]
summary: MCP server registry — which servers back which abstract tool aliases
---

# MCP Tools Registry

MCP servers are configured in the active setup TOML under [[mcp.servers]].
Each server declares tool_aliases that match the allowed_tools entries in agent profiles.

## Available Servers

### filesystem
Package: @modelcontextprotocol/server-filesystem
Tool aliases: filesystem_read, workspace_write, artifact_writer
Purpose: Read/write access to ./products, ./factory/workspaces, ./factory/artifacts
Platform: all

### memory
Package: @modelcontextprotocol/server-memory
Tool aliases: memory_store, metadata_query
Purpose: Persistent key-value memory for Coobie. Replaces AnythingLLM on work-windows.
Platform: all
Note: Set MEMORY_FILE_PATH=./factory/memory/store.json for cross-session persistence.

### sqlite
Package: @modelcontextprotocol/server-sqlite
Tool aliases: metadata_query, db_read
Purpose: Agent-level read access to factory/state.db (run metadata, history)
Platform: all

### github
Package: @modelcontextprotocol/server-github
Tool aliases: fetch_docs, github_read
Purpose: Repo search, file reads, issue/PR access for Piper and Scout
Requires: GITHUB_TOKEN env var
Platform: all

### brave-search
Package: @modelcontextprotocol/server-brave-search
Tool aliases: fetch_docs, web_search
Purpose: External doc lookup and dependency research for Piper
Requires: BRAVE_API_KEY env var (free tier: 2000 queries/month)
Platform: all

## Adding a New MCP Server

1. Add [[mcp.servers]] entry to the active setup TOML (harkonnen.toml or setups/*.toml)
2. Create factory/mcp/<name>.yaml with the server's documentation
3. Add the tool_aliases that agents will use in their allowed_tools list
4. Run: cargo run -- setup check  (verifies the command is on PATH)
"#
            .into(),
        ),
        (
            "04-spec-format.md".into(),
            r#"---
tags: [spec, format, yaml, intake, scout, sample]
summary: YAML spec format — required fields, optional fields, and a complete example
---

# Spec Format

Specs are YAML files in factory/specs/. They are the factory's primary input.
Scout reads and validates them. All fields below are required unless marked optional.

## Required Fields

```yaml
id:     string   # unique identifier, snake_case
title:  string   # human-readable name
purpose: string  # one-sentence intent
scope:
  - string       # list of things in scope
constraints:
  - string       # list of things that must not happen
inputs:
  - string       # what the factory receives
outputs:
  - string       # what the factory produces
acceptance_criteria:
  - string       # visible pass/fail conditions (Bramble validates these)
forbidden_behaviors:
  - string       # things that must never occur (Keeper enforces)
rollback_requirements:
  - string       # what must survive if the run fails
dependencies:
  - string       # external packages, services, or tools required
performance_expectations:
  - string       # timing or throughput targets
security_expectations:
  - string       # auth, secrets, isolation requirements
```

## Complete Example

```yaml
id: user-auth-feature
title: User Authentication
purpose: Add JWT-based login and session management to the API
scope:
  - login endpoint
  - token validation middleware
  - logout endpoint
constraints:
  - no changes to existing user data schema
  - must not break existing API contracts
inputs:
  - yaml spec
  - product: sample-app
outputs:
  - implemented auth endpoints
  - visible test suite
  - artifact bundle
acceptance_criteria:
  - POST /login returns 200 with valid JWT on correct credentials
  - invalid credentials return 401
  - protected routes reject requests without valid token
  - all visible tests pass
forbidden_behaviors:
  - storing plaintext passwords
  - logging JWT tokens
  - path escape from workspace
rollback_requirements:
  - prior artifacts retained unless explicitly cleaned
dependencies:
  - jsonwebtoken
  - bcrypt
performance_expectations:
  - login endpoint responds within 200ms
security_expectations:
  - JWT secret loaded from env var, never hardcoded
  - bcrypt cost factor >= 10
```

## Running a Spec

    cargo run -- spec validate factory/specs/my-spec.yaml
    cargo run -- run start factory/specs/my-spec.yaml --product my-app
"#
            .into(),
        ),
    ]
}
