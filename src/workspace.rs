use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub async fn create_run_workspace(base: &Path, run_id: &str) -> Result<PathBuf> {
    let path = base.join(run_id);
    tokio::fs::create_dir_all(&path).await?;
    Ok(path)
}

pub async fn stage_target_workspace(source: &Path, workspace_root: &Path) -> Result<PathBuf> {
    if !source.exists() {
        bail!("target source not found: {}", source.display());
    }
    if !source.is_dir() {
        bail!("target source is not a directory: {}", source.display());
    }

    let destination = workspace_root.join("product");
    tokio::fs::create_dir_all(&destination).await?;

    // Canonicalize both paths so we can detect if destination is inside source
    // and exclude that subtree from the copy to prevent recursive explosion.
    let source_canon = source
        .canonicalize()
        .unwrap_or_else(|_| source.to_path_buf());
    let dest_canon = destination
        .canonicalize()
        .unwrap_or_else(|_| destination.to_path_buf());

    copy_dir_all(source, &destination, &source_canon, &dest_canon)?;
    Ok(destination)
}

fn copy_dir_all(
    source: &Path,
    destination: &Path,
    source_canon: &Path,
    dest_canon: &Path,
) -> Result<()> {
    for entry in fs::read_dir(source)
        .with_context(|| format!("reading source directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_name = entry.file_name();
        let file_name = file_name.to_string_lossy();
        let file_type = entry.file_type()?;

        if should_skip_entry(file_name.as_ref(), file_type.is_dir()) {
            continue;
        }

        // Skip if this source path is a prefix of or equal to the destination
        // (prevents copying the workspace into itself when product path == repo root).
        if file_type.is_dir() {
            let entry_canon = source_path
                .canonicalize()
                .unwrap_or_else(|_| source_path.clone());
            if dest_canon.starts_with(&entry_canon) || entry_canon.starts_with(dest_canon) {
                continue;
            }
        }

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).with_context(|| {
                format!(
                    "creating destination directory {}",
                    destination_path.display()
                )
            })?;
            copy_dir_all(&source_path, &destination_path, source_canon, dest_canon)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).with_context(|| {
                format!(
                    "copying target file {} -> {}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn should_skip_entry(name: &str, is_dir: bool) -> bool {
    if is_dir {
        matches!(
            name,
            ".git"
                | "node_modules"
                | "target"
                | "dist"
                | "build"
                | "coverage"
                | ".next"
                | ".turbo"
                | ".venv"
                | "venv"
                | "__pycache__"
        )
    } else {
        matches!(name, ".DS_Store")
    }
}
