use anyhow::{bail, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::setup::SetupConfig;

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
        println!("Index built:       {}", self.root.join("index.json").display());
        println!("Imports folder:    {}", self.root.join("imports").display());
        println!();
        println!("Coobie will auto-refresh her local index when memory files change.");
        println!("Import docs/images/PDFs with: harkonnen memory import <file>");
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
        println!("Coobie is ready. Manual markdown drops are picked up automatically on next retrieval.");

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
        self.ensure_fresh_index().await?;

        let index_path = self.root.join("index.json");
        if !index_path.exists() {
            return Ok(vec![
                "Memory not initialized. Run: harkonnen memory init".to_string(),
            ]);
        }

        let index = read_memory_index(&index_path).await?;
        let q = query.to_lowercase();
        let mut hits = index
            .entries
            .iter()
            .filter(|entry| memory_matches(entry, &q))
            .map(|entry| (memory_match_score(entry, &q), entry))
            .collect::<Vec<_>>();

        hits.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.id.cmp(&right.1.id)));

        let rendered = hits
            .into_iter()
            .map(|(_, entry)| {
                let snippet = &entry.content[..entry.content.len().min(400)];
                format!("[{}] {}
{}", entry.id, entry.summary, snippet)
            })
            .collect::<Vec<_>>();

        if rendered.is_empty() {
            Ok(vec![format!("No memories found for: {query}")])
        } else {
            Ok(rendered)
        }
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
        let doc = render_memory_document(&parsed.tags, &parsed.summary, &parsed.body, &parsed.provenance);
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
        let ext = source.extension().and_then(|value| value.to_str()).unwrap_or("");
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

fn memory_matches(entry: &MemoryEntry, query: &str) -> bool {
    entry.summary.to_lowercase().contains(query)
        || entry.tags.iter().any(|tag| tag.to_lowercase().contains(query))
        || entry.content.to_lowercase().contains(query)
}

fn memory_match_score(entry: &MemoryEntry, query: &str) -> i64 {
    let mut score = 0_i64;
    let summary = entry.summary.to_lowercase();
    let content = entry.content.to_lowercase();
    if summary.contains(query) {
        score += 40;
    }
    if entry.tags.iter().any(|tag| tag.to_lowercase().contains(query)) {
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

fn collect_entries(root: &Path, current: &Path, entries: &mut Vec<MemoryEntry>) -> Result<()> {
    for dent in std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))? {
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
        let filename = path.file_name().and_then(|value| value.to_str()).unwrap_or("asset");
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
    matches!(path.file_name().and_then(|value| value.to_str()), Some("index.json") | Some("store.json"))
}

fn latest_relevant_mtime(root: &Path, current: &Path) -> Result<SystemTime> {
    let mut latest = SystemTime::UNIX_EPOCH;
    for dent in std::fs::read_dir(current).with_context(|| format!("reading {}", current.display()))? {
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
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("svg") => "image",
        Some("txt") | Some("csv") | Some("doc") | Some("docx") | Some("ppt") | Some("pptx") | Some("xls") | Some("xlsx") => "document",
        _ => "asset",
    }
}

fn asset_path_for(imports_dir: &Path, id: &str, ext: &str) -> PathBuf {
    if ext.is_empty() {
        imports_dir.join(id)
    } else {
        imports_dir.join(format!("{}.{}", id, ext))
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

    push_frontmatter_scalar(&mut frontmatter, "source_label", provenance.source_label.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "source_kind", provenance.source_kind.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "source_path", provenance.source_path.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "source_run_id", provenance.source_run_id.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "source_spec_id", provenance.source_spec_id.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "git_branch", provenance.git_branch.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "git_commit", provenance.git_commit.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "git_remote", provenance.git_remote.as_deref());
    push_frontmatter_list(&mut frontmatter, "evidence_run_ids", &provenance.evidence_run_ids);
    push_frontmatter_list(&mut frontmatter, "stale_when", &provenance.stale_when);
    push_frontmatter_scalar(&mut frontmatter, "status", provenance.status.as_deref());
    push_frontmatter_scalar(&mut frontmatter, "superseded_by", provenance.superseded_by.as_deref());
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
"#.into(),
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
"#.into(),
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
"#.into(),
        ),
    ]
}
