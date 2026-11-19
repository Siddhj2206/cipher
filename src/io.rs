use crate::error::{Error, Result};
#[cfg(unix)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{ErrorKind, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn atomic_write(path: &Path, content: &str) -> Result<()> {
    atomic_write_with_permissions(path, content, false)
}

pub(crate) fn atomic_write_private(path: &Path, content: &str) -> Result<()> {
    atomic_write_with_permissions(path, content, true)
}

fn atomic_write_with_permissions(path: &Path, content: &str, private: bool) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)
        .map_err(|e| Error::io(format!("Failed to create {}", parent.display()), e))?;
    let filename = path.file_name().ok_or_else(|| {
        Error::io(
            "Missing destination filename",
            std::io::Error::from(ErrorKind::InvalidInput),
        )
    })?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let (temp_path, mut file) = loop {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let mut name = std::ffi::OsString::from(".");
        name.push(filename);
        name.push(format!(".{}.{sequence}.tmp", std::process::id()));
        let temp_path = parent.join(name);
        match options.open(&temp_path) {
            Ok(file) => break (temp_path, file),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(Error::io(
                    format!("Failed to create {}", temp_path.display()),
                    e,
                ));
            }
        }
    };
    let result = (|| {
        file.write_all(content.as_bytes())
            .map_err(|e| Error::io(format!("Failed to write {}", temp_path.display()), e))?;
        file.sync_all()
            .map_err(|e| Error::io(format!("Failed to sync {}", temp_path.display()), e))?;
        drop(file);
        std::fs::rename(&temp_path, path)
            .map_err(|e| Error::io(format!("Failed to rename to {}", path.display()), e))?;
        #[cfg(unix)]
        {
            let result = File::open(parent).and_then(|dir| dir.sync_all());
            if let Err(e) = result
                && !matches!(e.kind(), ErrorKind::Unsupported | ErrorKind::InvalidInput)
            {
                return Err(Error::io(format!("Failed to sync {}", parent.display()), e));
            }
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
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

    #[test]
    fn atomic_write_empty_content_leaves_no_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        atomic_write(&path, "").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_write_rename_failure_cleans_up_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        std::fs::create_dir(&path).unwrap();
        assert!(atomic_write_private(&path, "secret").is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn atomic_writes_to_same_destination_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        std::thread::scope(|scope| {
            for _ in 0..16 {
                scope.spawn(|| atomic_write(&path, "hello").unwrap());
            }
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn private_write_creates_owner_only_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        atomic_write_private(&path, "secret").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn private_write_replaces_loose_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        atomic_write_private(&path, "secret").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
