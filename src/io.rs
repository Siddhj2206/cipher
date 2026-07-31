use crate::error::{Error, Result};
use std::path::Path;

pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| Error::io(format!("Failed to create {}", parent.display()), e))?;
    }
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, content)
        .map_err(|e| Error::io(format!("Failed to write {}", temp_path.display()), e))?;
    if let Err(e) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(Error::io(
            format!("Failed to rename to {}", path.display()),
            e,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_creates_file_with_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("out.txt");
        atomic_write(&path, "hello").unwrap();
        assert!(path.exists());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        std::fs::write(&path, "old").unwrap();
        atomic_write(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }
}
