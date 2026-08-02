use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub image: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArtistFile {
    pub schema_version: u32,
    pub artists: Vec<Artist>,
}

impl Default for ArtistFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            artists: Vec::new(),
        }
    }
}
