use crate::error_handling::AppError;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutosaveTarget {
    Final,
    Raw,
    Settings,
}

#[derive(Clone, Debug)]
pub struct AutosaveStatus {
    pub target: AutosaveTarget,
    pub path: PathBuf,
    pub dirty: bool,
    pub last_saved: Option<Instant>,
    pub last_error: Option<String>,
}

impl AutosaveStatus {
    pub fn new(target: AutosaveTarget, path: PathBuf) -> Self {
        Self {
            target,
            path,
            dirty: false,
            last_saved: None,
            last_error: None,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
        self.last_saved = Some(Instant::now());
        self.last_error = None;
    }

    pub fn mark_failed(&mut self, error: &AppError) {
        self.dirty = true;
        self.last_error = Some(error.to_string());
    }
}
