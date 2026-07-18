use anyhow::{Context, Result};
use std::path::Path;

pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, content)
        .with_context(|| format!("Failed to write {}", temp_path.display()))?;
    if let Err(e) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(e).with_context(|| format!("Failed to rename to {}", path.display()));
    }
    Ok(())
}
