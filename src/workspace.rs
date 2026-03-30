use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

pub async fn create_run_workspace(base: &Path, run_id: &str) -> Result<PathBuf> {
    let path = base.join(run_id);
    tokio::fs::create_dir_all(&path).await?;
    Ok(path)
}

pub async fn stage_product_workspace(
    products_root: &Path,
    workspace_root: &Path,
    product_name: &str,
) -> Result<PathBuf> {
    let source = products_root.join(product_name);
    stage_target_workspace(&source, workspace_root).await
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
    copy_dir_all(source, &destination)?;
    Ok(destination)
}

fn copy_dir_all(source: &Path, destination: &Path) -> Result<()> {
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

        if file_type.is_dir() {
            fs::create_dir_all(&destination_path).with_context(|| {
                format!("creating destination directory {}", destination_path.display())
            })?;
            copy_dir_all(&source_path, &destination_path)?;
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
            ".git" | "node_modules" | "target" | "dist" | "build" | "coverage" | ".next" | ".turbo" | ".venv" | "venv" | "__pycache__"
        )
    } else {
        matches!(name, ".DS_Store")
    }
}
