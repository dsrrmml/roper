use crate::error_handling::{AppError, AppResult};
use crate::persistence::artist_store::default_data_dir;
use crate::persistence::atomic_write::write_atomic;
use crate::services::id_generation::generate_unique_id;
use crate::services::validation::{validate_id, validate_name};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct IdeaSettings {
    pub schema_version: u32,
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub created_unix: u64,
    pub updated_unix: u64,
    pub last_opened_unix: u64,
}

impl IdeaSettings {
    pub fn new(id: String) -> Self {
        let now = current_unix_seconds();
        Self {
            schema_version: 1,
            id,
            name: String::new(),
            created_unix: now,
            updated_unix: now,
            last_opened_unix: now,
        }
    }
}

#[derive(Clone, Debug)]
pub struct IdeaPaths {
    pub directory: PathBuf,
    pub settings_path: PathBuf,
    pub in_out_path: PathBuf,
    pub verses_path: PathBuf,
    pub hooks_bridges_path: PathBuf,
}

impl IdeaPaths {
    fn from_id(root: &Path, id: &str) -> AppResult<Self> {
        validate_id(id)?;
        let directory = root.join(id);
        Ok(Self {
            settings_path: directory.join("settings.json"),
            in_out_path: directory.join("in_out.txt"),
            verses_path: directory.join("verses.txt"),
            hooks_bridges_path: directory.join("hooks_bridges.txt"),
            directory,
        })
    }
}

#[derive(Clone, Debug)]
pub struct IdeaSnapshot {
    pub settings: IdeaSettings,
    pub in_out: String,
    pub verses: String,
    pub hooks_bridges: String,
    pub paths: IdeaPaths,
}

#[derive(Clone)]
pub struct IdeaStore {
    root: PathBuf,
}

impl IdeaStore {
    pub fn new_default() -> AppResult<Self> {
        Ok(Self {
            root: default_data_dir()?.join("ideas"),
        })
    }

    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn existing_ids(&self) -> AppResult<HashSet<String>> {
        let mut ids = HashSet::new();
        if !self.root.exists() {
            return Ok(ids);
        }
        for entry in fs::read_dir(&self.root).map_err(|err| AppError::io(&self.root, err))? {
            let entry = entry.map_err(|err| AppError::io(&self.root, err))?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if validate_id(name).is_ok() {
                ids.insert(name.to_owned());
            }
        }
        Ok(ids)
    }

    pub fn create_idea(&self, maybe_name: Option<&str>) -> AppResult<IdeaSnapshot> {
        fs::create_dir_all(&self.root).map_err(|err| AppError::io(&self.root, err))?;
        let existing = self.existing_ids()?;
        let id = generate_unique_id("idea", |candidate| existing.contains(candidate))?;
        let paths = IdeaPaths::from_id(&self.root, &id)?;
        fs::create_dir_all(&paths.directory).map_err(|err| AppError::io(&paths.directory, err))?;

        let mut settings = IdeaSettings::new(id);
        if let Some(name) = maybe_name {
            settings.name = validate_name(name, "idea.name")?;
        }

        self.save_settings(&paths, &settings)?;
        self.save_panes(&paths, "", "", "")?;

        Ok(IdeaSnapshot {
            settings,
            in_out: String::new(),
            verses: String::new(),
            hooks_bridges: String::new(),
            paths,
        })
    }

    pub fn load_idea(&self, id: &str) -> AppResult<IdeaSnapshot> {
        let paths = IdeaPaths::from_id(&self.root, id)?;
        let mut settings = self.load_settings(&paths)?;
        settings.last_opened_unix = current_unix_seconds();
        self.save_settings(&paths, &settings)?;

        let in_out = read_text_or_empty(&paths.in_out_path)?;
        let verses = read_text_or_empty(&paths.verses_path)?;
        let hooks_bridges = read_text_or_empty(&paths.hooks_bridges_path)?;

        Ok(IdeaSnapshot {
            settings,
            in_out,
            verses,
            hooks_bridges,
            paths,
        })
    }

    pub fn save_snapshot(
        &self,
        paths: &IdeaPaths,
        settings: &mut IdeaSettings,
        in_out: &str,
        verses: &str,
        hooks_bridges: &str,
    ) -> AppResult<()> {
        if settings.name.trim().is_empty() {
            if let Some(name) = auto_name_for_snapshot(in_out, verses, hooks_bridges) {
                settings.name = name;
            }
        }
        settings.updated_unix = current_unix_seconds();
        settings.last_opened_unix = settings.updated_unix;
        self.save_settings(paths, settings)?;
        self.save_panes(paths, in_out, verses, hooks_bridges)
    }

    pub fn update_name(
        &self,
        paths: &IdeaPaths,
        settings: &mut IdeaSettings,
        name: &str,
    ) -> AppResult<()> {
        settings.name = validate_name(name, "idea.name")?;
        settings.updated_unix = current_unix_seconds();
        settings.last_opened_unix = settings.updated_unix;
        self.save_settings(paths, settings)
    }

    pub fn remove_idea(&self, id: &str) -> AppResult<()> {
        let paths = IdeaPaths::from_id(&self.root, id)?;
        if !paths.directory.exists() {
            return Ok(());
        }
        fs::remove_dir_all(&paths.directory).map_err(|err| AppError::io(&paths.directory, err))
    }

    pub fn list_named_ideas(&self) -> AppResult<Vec<IdeaSnapshot>> {
        let mut snapshots = Vec::new();
        for id in self.existing_ids()? {
            let Ok(snapshot) = self.load_idea_without_touch(&id) else {
                continue;
            };
            if snapshot.settings.name.trim().is_empty() {
                continue;
            }
            snapshots.push(snapshot);
        }
        snapshots.sort_by(|left, right| {
            right
                .settings
                .last_opened_unix
                .cmp(&left.settings.last_opened_unix)
                .then_with(|| left.settings.id.cmp(&right.settings.id))
        });
        Ok(snapshots)
    }

    pub fn latest_idea(&self) -> AppResult<Option<IdeaSnapshot>> {
        let mut latest: Option<IdeaSnapshot> = None;
        for id in self.existing_ids()? {
            let Ok(snapshot) = self.load_idea_without_touch(&id) else {
                continue;
            };
            if latest
                .as_ref()
                .map(|current| {
                    snapshot.settings.last_opened_unix > current.settings.last_opened_unix
                })
                .unwrap_or(true)
            {
                latest = Some(snapshot);
            }
        }
        Ok(latest)
    }

    fn load_idea_without_touch(&self, id: &str) -> AppResult<IdeaSnapshot> {
        let paths = IdeaPaths::from_id(&self.root, id)?;
        let settings = self.load_settings(&paths)?;
        let in_out = read_text_or_empty(&paths.in_out_path)?;
        let verses = read_text_or_empty(&paths.verses_path)?;
        let hooks_bridges = read_text_or_empty(&paths.hooks_bridges_path)?;
        Ok(IdeaSnapshot {
            settings,
            in_out,
            verses,
            hooks_bridges,
            paths,
        })
    }

    fn load_settings(&self, paths: &IdeaPaths) -> AppResult<IdeaSettings> {
        let bytes = fs::read(&paths.settings_path)
            .map_err(|err| AppError::io(&paths.settings_path, err))?;
        let settings: IdeaSettings = serde_json::from_slice(&bytes)
            .map_err(|err| AppError::json(&paths.settings_path, err))?;
        validate_idea_settings(&settings)?;
        Ok(settings)
    }

    fn save_settings(&self, paths: &IdeaPaths, settings: &IdeaSettings) -> AppResult<()> {
        validate_idea_settings(settings)?;
        let bytes = serde_json::to_vec_pretty(settings)
            .map_err(|err| AppError::json(&paths.settings_path, err))?;
        write_atomic(&paths.settings_path, &bytes)
    }

    fn save_panes(
        &self,
        paths: &IdeaPaths,
        in_out: &str,
        verses: &str,
        hooks_bridges: &str,
    ) -> AppResult<()> {
        write_atomic(&paths.in_out_path, in_out.as_bytes())?;
        write_atomic(&paths.verses_path, verses.as_bytes())?;
        write_atomic(&paths.hooks_bridges_path, hooks_bridges.as_bytes())?;
        Ok(())
    }
}

fn validate_idea_settings(settings: &IdeaSettings) -> AppResult<()> {
    if settings.schema_version != 1 {
        return Err(AppError::validation(
            "schema_version",
            "unsupported idea settings schema",
        ));
    }
    validate_id(&settings.id)?;
    if !settings.name.trim().is_empty() {
        validate_name(&settings.name, "idea.name")?;
    }
    Ok(())
}

fn read_text_or_empty(path: &Path) -> AppResult<String> {
    if !path.exists() {
        return Ok(String::new());
    }
    fs::read_to_string(path).map_err(|err| AppError::io(path, err))
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn auto_name_for_snapshot(in_out: &str, verses: &str, hooks_bridges: &str) -> Option<String> {
    let preview = [in_out, verses, hooks_bridges]
        .into_iter()
        .flat_map(|text| text.split_whitespace())
        .take(12)
        .collect::<Vec<_>>()
        .join(" ");

    if preview.is_empty() {
        None
    } else {
        Some(limit_idea_name(&preview, 48))
    }
}

fn limit_idea_name(value: &str, max_chars: usize) -> String {
    let clipped = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() <= max_chars {
        clipped
    } else {
        format!("{}…", clipped.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_and_load_idea_roundtrip() {
        let dir = tempdir().expect("temp dir");
        let store = IdeaStore::new(dir.path().join("ideas"));
        let created = store
            .create_idea(Some("Verse seed"))
            .expect("idea can be created");

        assert!(created.paths.settings_path.exists());
        assert!(created.paths.in_out_path.exists());
        assert!(created.paths.verses_path.exists());
        assert!(created.paths.hooks_bridges_path.exists());

        let loaded = store
            .load_idea(&created.settings.id)
            .expect("idea can be loaded");
        assert_eq!(loaded.settings.name, "Verse seed");
    }

    #[test]
    fn list_named_ideas_excludes_unnamed() {
        let dir = tempdir().expect("temp dir");
        let store = IdeaStore::new(dir.path().join("ideas"));
        let _ = store.create_idea(None).expect("can create unnamed");
        let _ = store.create_idea(Some("Named")).expect("can create named");

        let listed = store.list_named_ideas().expect("list works");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].settings.name, "Named");
    }

    #[test]
    fn save_snapshot_auto_names_unnamed_idea_with_content() {
        let dir = tempdir().expect("temp dir");
        let store = IdeaStore::new(dir.path().join("ideas"));
        let mut snapshot = store.create_idea(None).expect("can create unnamed");

        store
            .save_snapshot(
                &snapshot.paths,
                &mut snapshot.settings,
                "Fresh concept",
                "",
                "",
            )
            .expect("snapshot can be saved");

        assert_eq!(snapshot.settings.name, "Fresh concept");

        let listed = store.list_named_ideas().expect("list works");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].settings.id, snapshot.settings.id);

        let loaded = store
            .load_idea(&snapshot.settings.id)
            .expect("idea can be loaded");
        assert_eq!(loaded.settings.name, "Fresh concept");
    }

    #[test]
    fn save_snapshot_keeps_whitespace_only_idea_unnamed() {
        let dir = tempdir().expect("temp dir");
        let store = IdeaStore::new(dir.path().join("ideas"));
        let mut snapshot = store.create_idea(None).expect("can create unnamed");

        store
            .save_snapshot(&snapshot.paths, &mut snapshot.settings, " \n\t ", "", "")
            .expect("snapshot can be saved");

        assert!(snapshot.settings.name.is_empty());
        assert!(store.list_named_ideas().expect("list works").is_empty());
    }

    #[test]
    fn save_snapshot_updates_panes() {
        let dir = tempdir().expect("temp dir");
        let store = IdeaStore::new(dir.path().join("ideas"));
        let mut snapshot = store
            .create_idea(Some("Draft"))
            .expect("idea can be created");

        store
            .save_snapshot(
                &snapshot.paths,
                &mut snapshot.settings,
                "[INTRO]\nA",
                "[VERSE 1]\nB",
                "[HOOK]\nC",
            )
            .expect("snapshot can be saved");

        let loaded = store
            .load_idea(&snapshot.settings.id)
            .expect("idea can be loaded");
        assert!(loaded.in_out.contains("[INTRO]"));
        assert!(loaded.verses.contains("[VERSE 1]"));
        assert!(loaded.hooks_bridges.contains("[HOOK]"));
    }
}
