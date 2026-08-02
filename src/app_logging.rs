use crate::app_dirs;
use crate::error_handling::{AppError, AppResult};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init_logging() -> AppResult<PathBuf> {
    let path = app_dirs::log_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| AppError::io(parent, err))?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| AppError::io(&path, err))?;
    let _ = LOG_PATH.set(path.clone());
    let _ = LOG_FILE.set(Mutex::new(file));
    log_line("INFO", "logging initialized");
    install_panic_hook();
    Ok(path)
}

pub fn log_path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}

pub fn log_info(message: impl AsRef<str>) {
    log_line("INFO", message.as_ref());
}

pub fn log_error(message: impl AsRef<str>) {
    log_line("ERROR", message.as_ref());
}

fn install_panic_hook() {
    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "unknown location".to_owned());
        let payload = panic_info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(String::as_str)
            })
            .unwrap_or("panic without message");
        log_line("PANIC", &format!("{payload} ({location})"));
    }));
}

fn log_line(level: &str, message: &str) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default();
    let line = format!("[{timestamp}] {level}: {message}\n");
    eprint!("{line}");
    if let Some(file) = LOG_FILE.get() {
        if let Ok(mut file) = file.lock() {
            let _ = file.write_all(line.as_bytes());
            let _ = file.flush();
        }
    }
}
