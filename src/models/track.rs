use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CasingMode {
    Preserve,
    Uppercase,
    Lowercase,
}

impl CasingMode {
    pub fn next(self) -> Self {
        match self {
            Self::Preserve => Self::Uppercase,
            Self::Uppercase => Self::Lowercase,
            Self::Lowercase => Self::Preserve,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Preserve => "Aa",
            Self::Uppercase => "AA",
            Self::Lowercase => "aa",
        }
    }
}

impl Default for CasingMode {
    fn default() -> Self {
        Self::Preserve
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct UsedMaterial {
    pub normalized_hash: String,
    pub occurrence: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TrackSettings {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub artist_id: String,
    pub name: String,
    pub tempo: u16,
    pub length: String,
    pub working_directory: Option<PathBuf>,
    pub artwork: Option<PathBuf>,
    pub casing_mode: CasingMode,
    #[serde(default)]
    pub used_material: Vec<UsedMaterial>,
    #[serde(default)]
    pub dismissed_material: Vec<UsedMaterial>,
    #[serde(default)]
    pub last_opened_unix: Option<u64>,
}

impl TrackSettings {
    pub fn new(id: String, artist_id: String, name: String, tempo: u16, length: String) -> Self {
        Self {
            schema_version: 1,
            id,
            artist_id,
            name,
            tempo,
            length,
            working_directory: None,
            artwork: None,
            casing_mode: CasingMode::Preserve,
            used_material: Vec::new(),
            dismissed_material: Vec::new(),
            last_opened_unix: None,
        }
    }
}
