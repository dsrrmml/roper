use std::error::Error;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::PathBuf;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub enum AppError {
    Io {
        path: PathBuf,
        source: io::Error,
    },
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    Validation {
        field: String,
        message: String,
    },
    Conflict {
        path: PathBuf,
        message: String,
    },
    NotFound {
        path: PathBuf,
    },
}

impl AppError {
    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Validation {
            field: field.into(),
            message: message.into(),
        }
    }

    pub fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub fn json(path: impl Into<PathBuf>, source: serde_json::Error) -> Self {
        Self::Json {
            path: path.into(),
            source,
        }
    }

    pub fn conflict(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::Conflict {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Io { path, source } => {
                write!(f, "I/O error at {}: {}", path.display(), source)
            }
            AppError::Json { path, source } => {
                write!(f, "Invalid JSON at {}: {}", path.display(), source)
            }
            AppError::Validation { field, message } => write!(f, "{}: {}", field, message),
            AppError::Conflict { path, message } => {
                write!(f, "Conflict at {}: {}", path.display(), message)
            }
            AppError::NotFound { path } => write!(f, "Not found: {}", path.display()),
        }
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            AppError::Io { source, .. } => Some(source),
            AppError::Json { source, .. } => Some(source),
            AppError::Validation { .. } | AppError::Conflict { .. } | AppError::NotFound { .. } => {
                None
            }
        }
    }
}
