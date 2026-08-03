use crate::error_handling::{AppError, AppResult};
use crate::models::CasingMode;
use crate::persistence::artist_store::{default_config_dir, legacy_config_dir};
use crate::persistence::atomic_write::write_atomic;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const VALID_FONT_SIZES: [u16; 5] = [10, 12, 14, 16, 18];
pub const DEFAULT_FONT_SIZE: u16 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartBehavior {
    FreshIdea,
    LastIdea,
    LastTrack,
    TrackList,
}

impl Default for StartBehavior {
    fn default() -> Self {
        Self::LastTrack
    }
}

impl StartBehavior {
    pub fn label(self) -> &'static str {
        match self {
            Self::FreshIdea => "FRESH IDEA",
            Self::LastIdea => "LAST IDEA",
            Self::LastTrack => "LAST TRACK",
            Self::TrackList => "TRACK LIST",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "FRESH IDEA" => Some(Self::FreshIdea),
            "LAST IDEA" => Some(Self::LastIdea),
            "LAST TRACK" => Some(Self::LastTrack),
            "TRACK LIST" => Some(Self::TrackList),
            _ => None,
        }
    }

    pub fn combo_index(self) -> u32 {
        match self {
            Self::FreshIdea => 0,
            Self::LastIdea => 1,
            Self::LastTrack => 2,
            Self::TrackList => 3,
        }
    }

    pub fn from_combo_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::FreshIdea),
            1 => Some(Self::LastIdea),
            2 => Some(Self::LastTrack),
            3 => Some(Self::TrackList),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSettings {
    pub schema_version: u32,
    pub font_size_pt: u16,
    pub fullscreen: bool,
    pub default_casing_mode: CasingMode,
    #[serde(default)]
    pub start_behavior: StartBehavior,
    #[serde(default = "default_workspace_mode")]
    pub last_workspace_mode: String,
    #[serde(default)]
    pub last_idea_id: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            font_size_pt: DEFAULT_FONT_SIZE,
            fullscreen: true,
            default_casing_mode: CasingMode::Preserve,
            start_behavior: StartBehavior::default(),
            last_workspace_mode: default_workspace_mode(),
            last_idea_id: None,
        }
    }
}

fn default_workspace_mode() -> String {
    "tracks".to_owned()
}

pub struct SettingsStore {
    path: PathBuf,
    legacy_path: Option<PathBuf>,
}

impl SettingsStore {
    pub fn new_default() -> AppResult<Self> {
        Ok(Self {
            path: default_config_dir()?.join("settings.json"),
            legacy_path: Some(legacy_config_dir()?.join("settings.json")),
        })
    }

    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            legacy_path: None,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load(&self) -> AppResult<AppSettings> {
        let (load_path, migrate_after_load) = match self.load_path() {
            Some(result) => result,
            None => return Ok(AppSettings::default()),
        };
        let bytes = fs::read(&load_path).map_err(|err| AppError::io(&load_path, err))?;
        let mut settings: AppSettings = serde_json::from_slice(&bytes)
            .map_err(|err| AppError::json(&load_path, err))?;
        if settings.schema_version != 1 {
            return Err(AppError::validation(
                "schema_version",
                "unsupported app settings schema",
            ));
        }
        if !is_valid_font_size(settings.font_size_pt) {
            settings.font_size_pt = DEFAULT_FONT_SIZE;
        }
        if settings.start_behavior == StartBehavior::default() && settings.last_workspace_mode.is_empty() {
            settings.start_behavior = StartBehavior::LastTrack;
        }
        if migrate_after_load {
            self.save(&settings)?;
        }
        Ok(settings)
    }

    fn load_path(&self) -> Option<(PathBuf, bool)> {
        if self.path.exists() {
            return Some((self.path.clone(), false));
        }
        self.legacy_path
            .as_ref()
            .filter(|path| path.exists())
            .cloned()
            .map(|path| (path, true))
    }

    pub fn save(&self, settings: &AppSettings) -> AppResult<()> {
        let mut copy = settings.clone();
        if !is_valid_font_size(copy.font_size_pt) {
            copy.font_size_pt = DEFAULT_FONT_SIZE;
        }
        let json =
            serde_json::to_vec_pretty(&copy).map_err(|err| AppError::json(&self.path, err))?;
        write_atomic(&self.path, &json)
    }
}

pub fn is_valid_font_size(font_size_pt: u16) -> bool {
    VALID_FONT_SIZES.contains(&font_size_pt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn default_settings_start_fullscreen() {
        let settings = AppSettings::default();
        assert!(settings.fullscreen);
        assert_eq!(settings.font_size_pt, DEFAULT_FONT_SIZE);
        assert_eq!(settings.start_behavior, StartBehavior::LastTrack);
    }

    #[test]
    fn save_and_load_roundtrip_start_behavior() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let settings = AppSettings {
            start_behavior: StartBehavior::FreshIdea,
            ..AppSettings::default()
        };
        store.save(&settings).expect("settings can be saved");
        let loaded = store.load().expect("settings can be loaded");
        assert_eq!(loaded.start_behavior, StartBehavior::FreshIdea);
    }

    #[test]
    fn invalid_font_size_falls_back_on_load() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            r#"{
                "schema_version": 1,
                "font_size_pt": 99,
                "fullscreen": false,
                "default_casing_mode": "preserve"
            }"#,
        )
        .expect("settings can be written");
        let store = SettingsStore::new(path);
        let settings = store.load().expect("settings can load");
        assert_eq!(settings.font_size_pt, DEFAULT_FONT_SIZE);
        assert!(!settings.fullscreen);
    }

    #[test]
    fn save_preserves_fullscreen_preference() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("settings.json");
        let store = SettingsStore::new(path.clone());
        let settings = AppSettings {
            fullscreen: false,
            ..AppSettings::default()
        };
        store.save(&settings).expect("settings can be saved");
        let saved: AppSettings =
            serde_json::from_slice(&fs::read(path).expect("settings json exists"))
                .expect("settings json is valid");
        assert!(!saved.fullscreen);
    }

    #[test]
    fn default_store_migrates_legacy_settings_to_single_root() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("data").join("settings.json");
        let legacy_path = dir.path().join("config").join("settings.json");
        fs::create_dir_all(legacy_path.parent().expect("legacy parent exists"))
            .expect("legacy parent can be created");
        fs::write(
            &legacy_path,
            r#"{
                "schema_version": 1,
                "font_size_pt": 14,
                "fullscreen": false,
                "default_casing_mode": "preserve"
            }"#,
        )
        .expect("legacy settings can be written");

        let store = SettingsStore {
            path: path.clone(),
            legacy_path: Some(legacy_path),
        };

        let loaded = store.load().expect("legacy settings can load");
        assert_eq!(loaded.font_size_pt, 14);
        assert!(!loaded.fullscreen);
        assert!(path.exists());
    }
}
