use crate::error_handling::{AppError, AppResult};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn write_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| AppError::validation("path", "target file must have a parent directory"))?;
    fs::create_dir_all(parent).map_err(|err| AppError::io(parent, err))?;

    let temp_path = temp_path_for(path);
    let write_result = write_temp_file(&temp_path, bytes)
        .and_then(|()| fs::rename(&temp_path, path).map_err(|err| AppError::io(path, err)))
        .and_then(|()| sync_parent(parent));

    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }

    write_result
}

fn write_temp_file(path: &Path, bytes: &[u8]) -> AppResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| AppError::io(path, err))?;
    file.write_all(bytes)
        .map_err(|err| AppError::io(path, err))?;
    file.flush().map_err(|err| AppError::io(path, err))?;
    file.sync_all().map_err(|err| AppError::io(path, err))?;
    Ok(())
}

fn temp_path_for(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("roper-file");
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{}.tmp.{}.{}", name, std::process::id(), counter))
}

fn sync_parent(parent: &Path) -> AppResult<()> {
    match File::open(parent) {
        Ok(file) => file.sync_all().map_err(|err| AppError::io(parent, err)),
        Err(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn atomic_write_creates_expected_file() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("final");
        write_atomic(&path, "eins".as_bytes()).expect("atomic write succeeds");
        assert_eq!(fs::read_to_string(&path).expect("file readable"), "eins");
        assert!(
            fs::read_dir(dir.path())
                .expect("dir readable")
                .all(|entry| !entry
                    .expect("entry readable")
                    .file_name()
                    .to_string_lossy()
                    .contains(".tmp."))
        );
    }
}
