use crate::error_handling::{AppError, AppResult};
use std::env;
use std::path::PathBuf;

pub fn storage_dir() -> AppResult<PathBuf> {
    xdg_dir("XDG_DATA_HOME", ".local/share")
}

pub fn config_dir() -> AppResult<PathBuf> {
    storage_dir()
}

pub fn data_dir() -> AppResult<PathBuf> {
    storage_dir()
}

pub fn cache_dir() -> AppResult<PathBuf> {
    storage_dir()
}

pub fn state_dir() -> AppResult<PathBuf> {
    storage_dir()
}

pub fn legacy_config_dir() -> AppResult<PathBuf> {
    xdg_dir("XDG_CONFIG_HOME", ".config")
}

pub fn log_file_path() -> AppResult<PathBuf> {
    Ok(storage_dir()?.join("roper.log"))
}

fn xdg_dir(env_key: &str, fallback_suffix: &str) -> AppResult<PathBuf> {
    if let Some(path) = env::var_os(env_key).map(PathBuf::from) {
        if path.is_absolute() {
            return Ok(path.join("roper"));
        }
    }

    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| {
            AppError::validation(
                "HOME",
                format!("HOME must be set when {env_key} is not available"),
            )
        })?;
    Ok(home.join(fallback_suffix).join("roper"))
}
