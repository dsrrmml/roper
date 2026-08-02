use crate::error_handling::{AppError, AppResult};
use crate::models::TrackSettings;
use crate::persistence::artist_store::default_data_dir;
use crate::persistence::atomic_write::write_atomic;
use crate::services::artwork::import_track_artwork;
use crate::services::id_generation::generate_unique_id;
use crate::services::validation::{
    validate_absolute_path, validate_id, validate_length, validate_name, validate_track_child_path,
    validate_track_settings,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct TrackPaths {
    pub directory: PathBuf,
    pub final_path: PathBuf,
    pub raw_path: PathBuf,
    pub settings_path: PathBuf,
    pub artwork_path: PathBuf,
}

impl TrackPaths {
    pub fn from_working_directory(
        directory: &Path,
        config_root: &Path,
        id: &str,
    ) -> AppResult<Self> {
        let lyrics_dir = directory.join("lyrics");
        Ok(Self {
            final_path: lyrics_dir.join("final.txt"),
            raw_path: lyrics_dir.join("raw.txt"),
            settings_path: config_root.join(id).join("settings.json"),
            artwork_path: directory.join("artwork.png"),
            directory: directory.to_path_buf(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct TrackDraft {
    pub id: Option<String>,
    pub artist_id: String,
    pub name: String,
    pub tempo: u16,
    pub length: String,
    pub working_directory: Option<PathBuf>,
    pub artwork_source: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct TrackListItem {
    pub settings: TrackSettings,
    pub paths: TrackPaths,
}

#[derive(Clone)]
pub struct TrackStore {
    config_dir: PathBuf,
}

impl TrackStore {
    pub fn new_default() -> AppResult<Self> {
        Ok(Self {
            config_dir: default_data_dir()?,
        })
    }

    pub fn new(config_dir: PathBuf) -> Self {
        Self { config_dir }
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    fn track_storage_root(&self) -> PathBuf {
        self.config_dir.join("tracks")
    }

    fn index_path(&self) -> PathBuf {
        self.track_storage_root().join(".roper-track-index.json")
    }

    fn load_track_index(&self) -> AppResult<HashMap<String, PathBuf>> {
        let index_path = self.index_path();
        if !index_path.exists() {
            return Ok(HashMap::new());
        }
        let bytes = fs::read(&index_path).map_err(|err| AppError::io(index_path.clone(), err))?;
        let index =
            serde_json::from_slice(&bytes).map_err(|err| AppError::json(index_path, err))?;
        Ok(index)
    }

    fn save_track_index(&self, index: &HashMap<String, PathBuf>) -> AppResult<()> {
        let index_path = self.index_path();
        let json = serde_json::to_vec_pretty(index)
            .map_err(|err| AppError::json(index_path.clone(), err))?;
        write_atomic(&index_path, &json)
    }

    fn resolve_paths(&self, id: &str, explicit_directory: Option<&Path>) -> AppResult<TrackPaths> {
        if let Some(directory) = explicit_directory {
            return TrackPaths::from_working_directory(directory, &self.track_storage_root(), id);
        }
        if let Some(directory) = self.load_track_index()?.get(id) {
            return TrackPaths::from_working_directory(directory, &self.track_storage_root(), id);
        }
        TrackPaths::from_working_directory(
            &self.track_storage_root().join(id),
            &self.track_storage_root(),
            id,
        )
    }

    pub fn create_track(&self, draft: TrackDraft) -> AppResult<(TrackSettings, TrackPaths)> {
        validate_id(&draft.artist_id)?;
        let name = validate_name(&draft.name, "track.name")?;
        validate_length(&draft.length)?;
        let existing_ids = self.existing_track_ids()?;
        let id = if let Some(id) = draft.id {
            validate_id(&id)?;
            if existing_ids.contains(&id) {
                return Err(AppError::conflict(
                    validate_track_child_path(&self.track_storage_root(), &id)?,
                    "track id already exists",
                ));
            }
            id
        } else {
            generate_unique_id(&format!("track:{name}"), |candidate| {
                existing_ids.contains(candidate)
            })?
        };
        let working_directory = match &draft.working_directory {
            Some(path) => {
                validate_absolute_path(path, "working_directory")?;
                path.clone()
            }
            None => validate_track_child_path(&self.track_storage_root(), &id)?,
        };
        if !working_directory.exists() {
            return Err(AppError::validation(
                "working_directory",
                "must refer to an existing directory",
            ));
        }
        if !working_directory.is_dir() {
            return Err(AppError::validation(
                "working_directory",
                "must refer to a directory",
            ));
        }
        let paths = TrackPaths::from_working_directory(
            &working_directory,
            &self.track_storage_root(),
            &id,
        )?;
        fs::create_dir_all(paths.final_path.parent().unwrap_or(&working_directory)).map_err(
            |err| AppError::io(paths.final_path.parent().unwrap_or(&working_directory), err),
        )?;

        let result = (|| {
            let mut settings = TrackSettings::new(
                id.clone(),
                draft.artist_id.clone(),
                name,
                draft.tempo,
                draft.length,
            );
            settings.working_directory = Some(paths.directory.clone());
            if let Some(source) = draft.artwork_source {
                settings.artwork = Some(import_track_artwork(&source, &paths)?);
            }
            self.save_final(&paths, "")?;
            self.save_raw(&paths, "")?;
            self.save_settings(&paths, &settings)?;
            let mut index = self.load_track_index()?;
            index.insert(id.clone(), paths.directory.clone());
            self.save_track_index(&index)?;
            Ok(settings)
        })();

        match result {
            Ok(settings) => Ok((settings, paths)),
            Err(error) => {
                let _ = cleanup_created_track_dir(&paths);
                Err(error)
            }
        }
    }

    pub fn load_track(&self, id: &str) -> AppResult<(TrackSettings, String, String, TrackPaths)> {
        let paths = self.resolve_paths(id, None)?;
        let settings = self.load_settings(&paths)?;
        if settings.id != id {
            return Err(AppError::validation(
                "track.id",
                "settings id must match track directory",
            ));
        }
        let final_text = fs::read_to_string(&paths.final_path)
            .map_err(|err| AppError::io(&paths.final_path, err))?;
        let raw_text = fs::read_to_string(&paths.raw_path)
            .map_err(|err| AppError::io(&paths.raw_path, err))?;
        Ok((settings, final_text, raw_text, paths))
    }

    pub fn mark_opened(&self, paths: &TrackPaths, settings: &mut TrackSettings) -> AppResult<()> {
        settings.last_opened_unix = Some(current_unix_seconds());
        self.save_settings(paths, settings)
    }

    pub fn load_settings(&self, paths: &TrackPaths) -> AppResult<TrackSettings> {
        let bytes = fs::read(&paths.settings_path)
            .map_err(|err| AppError::io(&paths.settings_path, err))?;
        let settings: TrackSettings = serde_json::from_slice(&bytes)
            .map_err(|err| AppError::json(&paths.settings_path, err))?;
        validate_track_settings(&settings)?;
        Ok(settings)
    }

    pub fn save_settings(&self, paths: &TrackPaths, settings: &TrackSettings) -> AppResult<()> {
        validate_track_settings(settings)?;
        let json = serde_json::to_vec_pretty(settings)
            .map_err(|err| AppError::json(&paths.settings_path, err))?;
        write_atomic(&paths.settings_path, &json)
    }

    pub fn save_final(&self, paths: &TrackPaths, text: &str) -> AppResult<()> {
        write_atomic(&paths.final_path, text.as_bytes())
    }

    pub fn save_raw(&self, paths: &TrackPaths, text: &str) -> AppResult<()> {
        write_atomic(&paths.raw_path, text.as_bytes())
    }

    pub fn remove_track(&self, id: &str) -> AppResult<()> {
        let paths = self.resolve_paths(id, None)?;
        let mut index = self.load_track_index()?;
        index.remove(id);
        self.save_track_index(&index)?;
        remove_track_files(&paths)
    }

    pub fn update_metadata(
        &self,
        paths: &TrackPaths,
        settings: &mut TrackSettings,
        name: &str,
        tempo: u16,
        length: &str,
        artwork: Option<PathBuf>,
    ) -> AppResult<()> {
        let name = validate_name(name, "track.name")?;
        validate_length(length)?;
        settings.name = name;
        settings.tempo = tempo;
        settings.length = length.to_owned();
        settings.artwork = artwork;
        self.save_settings(paths, settings)
    }

    pub fn latest_opened_track_for_artist(
        &self,
        artist_id: &str,
    ) -> AppResult<Option<TrackListItem>> {
        validate_id(artist_id)?;
        let ids = self.existing_track_ids()?;
        let mut latest: Option<TrackListItem> = None;
        for id in ids {
            let paths = self.resolve_paths(&id, None)?;
            let Ok(settings) = self.load_settings(&paths) else {
                continue;
            };
            if settings.artist_id != artist_id {
                continue;
            }
            let item = TrackListItem { settings, paths };
            let replace = latest
                .as_ref()
                .map(|current| {
                    track_order_key(&item)
                        .cmp(&track_order_key(current))
                        .is_gt()
                })
                .unwrap_or(true);
            if replace {
                latest = Some(item);
            }
        }
        Ok(latest)
    }

    pub fn existing_track_ids(&self) -> AppResult<HashSet<String>> {
        let mut ids = HashSet::new();
        let index = self.load_track_index()?;
        ids.extend(index.keys().cloned());
        let track_root = self.track_storage_root();
        if !track_root.exists() {
            return Ok(ids);
        }
        let entries = fs::read_dir(&track_root).map_err(|err| AppError::io(&track_root, err))?;
        for entry in entries {
            let entry = entry.map_err(|err| AppError::io(&track_root, err))?;
            let file_type = entry
                .file_type()
                .map_err(|err| AppError::io(entry.path(), err))?;
            if file_type.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    if validate_id(name).is_ok() {
                        ids.insert(name.to_owned());
                    }
                }
            }
        }
        Ok(ids)
    }

    pub fn track_counts_by_artist(&self) -> AppResult<HashMap<String, usize>> {
        let mut counts = HashMap::new();
        for id in self.existing_track_ids()? {
            let paths = self.resolve_paths(&id, None)?;
            let Ok(settings) = self.load_settings(&paths) else {
                continue;
            };
            if settings.artist_id.is_empty() {
                continue;
            }
            *counts.entry(settings.artist_id).or_insert(0) += 1;
        }
        Ok(counts)
    }
}

pub struct TrackPager {
    store: TrackStore,
    ids: Vec<String>,
    loaded: usize,
    exhausted: bool,
}

impl TrackPager {
    pub fn new(store: TrackStore, artist_id: &str) -> AppResult<Self> {
        validate_id(artist_id)?;
        let mut ids: Vec<String> = store
            .existing_track_ids()?
            .into_iter()
            .filter(|id| {
                store
                    .resolve_paths(id, None)
                    .ok()
                    .and_then(|paths| store.load_settings(&paths).ok())
                    .is_some_and(|settings| settings.artist_id == artist_id)
            })
            .collect();
        ids.sort();
        ids.reverse();
        Ok(Self {
            store,
            ids,
            loaded: 0,
            exhausted: false,
        })
    }

    pub fn load_next(&mut self, limit: usize) -> AppResult<Vec<TrackListItem>> {
        if self.exhausted || limit == 0 {
            return Ok(Vec::new());
        }
        let end = self.loaded.saturating_add(limit).min(self.ids.len());
        let mut items = Vec::new();
        for id in &self.ids[self.loaded..end] {
            let paths = self.store.resolve_paths(id, None)?;
            match self.store.load_settings(&paths) {
                Ok(settings) => items.push(TrackListItem { settings, paths }),
                Err(error) => eprintln!(
                    "Skipping invalid track {}: {}",
                    paths.directory.display(),
                    error
                ),
            }
        }
        self.loaded = end;
        if self.loaded >= self.ids.len() {
            self.exhausted = true;
        }

        items.sort_by(|left, right| {
            track_order_key(right)
                .cmp(&track_order_key(left))
                .then_with(|| left.settings.id.cmp(&right.settings.id))
        });
        Ok(items)
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }
}

fn track_order_key(item: &TrackListItem) -> (Option<u64>, String) {
    (
        item.settings.last_opened_unix,
        item.settings.name.to_lowercase(),
    )
}

fn cleanup_created_track_dir(paths: &TrackPaths) -> AppResult<()> {
    remove_track_files(paths)
}

fn remove_track_files(paths: &TrackPaths) -> AppResult<()> {
    if paths.settings_path.exists() {
        fs::remove_file(&paths.settings_path)
            .map_err(|err| AppError::io(&paths.settings_path, err))?;
    }
    if let Some(parent) = paths.settings_path.parent() {
        if parent.exists() {
            let mut entries = fs::read_dir(parent).map_err(|err| AppError::io(parent, err))?;
            if entries.next().is_none() {
                fs::remove_dir(parent).map_err(|err| AppError::io(parent, err))?;
            }
        }
    }
    Ok(())
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_existing_working_directory(parent: &Path, name: &str) -> PathBuf {
        let directory = parent.join(name);
        fs::create_dir_all(directory.join("lyrics")).expect("lyrics dir can be created");
        directory
    }

    #[test]
    fn track_creation_writes_core_files() {
        let dir = tempdir().expect("temp dir can be created");
        let working_directory = create_existing_working_directory(dir.path(), "track-one");
        let store = TrackStore::new(dir.path().to_path_buf());
        let (settings, paths) = store
            .create_track(TrackDraft {
                id: None,
                artist_id: "abcdef123456".to_owned(),
                name: "First".to_owned(),
                tempo: 90,
                length: "03:42".to_owned(),
                working_directory: Some(working_directory.clone()),
                artwork_source: None,
            })
            .expect("track can be created");

        assert!(paths.final_path.exists());
        assert!(paths.raw_path.exists());
        assert!(paths.settings_path.exists());
        assert_eq!(settings.name, "First");
        assert_eq!(
            settings.working_directory.as_deref(),
            Some(working_directory.as_path())
        );
    }

    #[test]
    fn track_creation_uses_existing_working_directory() {
        let dir = tempdir().expect("temp dir can be created");
        let working_directory = dir.path().join("track-workspace");
        fs::create_dir_all(working_directory.join("lyrics")).expect("lyrics dir can be created");
        fs::write(
            working_directory.join("lyrics").join("lyrics.txt"),
            b"existing",
        )
        .expect("lyrics file can be created");
        fs::write(
            working_directory.join("lyrics").join("raw.txt"),
            b"existing",
        )
        .expect("raw file can be created");
        let store = TrackStore::new(dir.path().to_path_buf());
        let (settings, paths) = store
            .create_track(TrackDraft {
                id: None,
                artist_id: "abcdef123456".to_owned(),
                name: "Working Dir".to_owned(),
                tempo: 90,
                length: "03:42".to_owned(),
                artwork_source: None,
                working_directory: Some(working_directory.clone()),
            })
            .expect("track can be created");

        assert_eq!(paths.directory, working_directory);
        assert_eq!(
            paths.final_path,
            working_directory.join("lyrics").join("final.txt")
        );
        assert_eq!(
            paths.raw_path,
            working_directory.join("lyrics").join("raw.txt")
        );
        assert_eq!(
            settings.working_directory.as_deref(),
            Some(working_directory.as_path())
        );
    }

    #[test]
    fn remove_track_preserves_working_directory() {
        let dir = tempdir().expect("temp dir can be created");
        let working_directory = create_existing_working_directory(dir.path(), "track-remove");
        let store = TrackStore::new(dir.path().to_path_buf());
        let (settings, paths) = store
            .create_track(TrackDraft {
                id: None,
                artist_id: "abcdef123456".to_owned(),
                name: "Remove Me".to_owned(),
                tempo: 90,
                length: "03:42".to_owned(),
                working_directory: Some(working_directory.clone()),
                artwork_source: None,
            })
            .expect("track can be created");

        store
            .remove_track(&settings.id)
            .expect("track can be removed");

        assert!(paths.directory.exists());
        assert!(paths.final_path.exists());
        assert!(paths.raw_path.exists());
        assert!(!paths.settings_path.exists());
        assert!(
            !store
                .existing_track_ids()
                .expect("track ids load")
                .contains(&settings.id)
        );
    }

    #[test]
    fn track_detection_requires_valid_settings() {
        let dir = tempdir().expect("temp dir can be created");
        let track_dir = dir.path().join("abcdef123456");
        fs::create_dir(&track_dir).expect("track dir can be created");
        fs::write(track_dir.join("settings.json"), b"{bad").expect("bad settings can be written");
        let store = TrackStore::new(dir.path().to_path_buf());
        let mut pager = TrackPager::new(store, "abcdef123456").expect("pager can be built");
        let page = pager.load_next(10).expect("bad tracks are skipped");
        assert!(page.is_empty());
    }

    #[test]
    fn lazy_loading_pages_do_not_duplicate() {
        let dir = tempdir().expect("temp dir can be created");
        let store = TrackStore::new(dir.path().to_path_buf());
        for index in 0..12 {
            let working_directory =
                create_existing_working_directory(dir.path(), &format!("track-{index}"));
            let (mut settings, paths) = store
                .create_track(TrackDraft {
                    id: None,
                    artist_id: "abcdef123456".to_owned(),
                    name: format!("Track {index}"),
                    tempo: 90,
                    length: "03:42".to_owned(),
                    working_directory: Some(working_directory),
                    artwork_source: None,
                })
                .expect("track can be created");
            settings.last_opened_unix = Some(index);
            store
                .save_settings(&paths, &settings)
                .expect("settings save");
        }

        let mut pager = TrackPager::new(TrackStore::new(dir.path().to_path_buf()), "abcdef123456")
            .expect("pager");
        let first = pager.load_next(10).expect("first page");
        let second = pager.load_next(10).expect("second page");
        let first_ids: HashSet<String> =
            first.iter().map(|item| item.settings.id.clone()).collect();
        assert_eq!(first.len(), 10);
        assert_eq!(second.len(), 2);
        assert!(
            second
                .iter()
                .all(|item| !first_ids.contains(&item.settings.id))
        );
        assert!(pager.is_exhausted());
    }

    #[test]
    fn latest_opened_track_finds_true_latest_session_track() {
        let dir = tempdir().expect("temp dir can be created");
        let store = TrackStore::new(dir.path().to_path_buf());
        let mut latest_id = String::new();
        for (index, opened_at) in [20, 10, 40, 30].into_iter().enumerate() {
            let working_directory =
                create_existing_working_directory(dir.path(), &format!("track-latest-{index}"));
            let (mut settings, paths) = store
                .create_track(TrackDraft {
                    id: None,
                    artist_id: "abcdef123456".to_owned(),
                    name: format!("Track {index}"),
                    tempo: 90,
                    length: "03:42".to_owned(),
                    working_directory: Some(working_directory),
                    artwork_source: None,
                })
                .expect("track can be created");
            settings.last_opened_unix = Some(opened_at);
            if opened_at == 40 {
                latest_id = settings.id.clone();
            }
            store
                .save_settings(&paths, &settings)
                .expect("settings save");
        }

        let latest = store
            .latest_opened_track_for_artist("abcdef123456")
            .expect("latest track lookup works")
            .expect("latest track exists");
        assert_eq!(latest.settings.id, latest_id);
    }

    #[test]
    fn track_counts_are_grouped_per_artist() {
        let dir = tempdir().expect("temp dir can be created");
        let store = TrackStore::new(dir.path().to_path_buf());

        for (index, artist_id) in ["abcdef123456", "abcdef123456", "fedcba654321"]
            .into_iter()
            .enumerate()
        {
            let working_directory =
                create_existing_working_directory(dir.path(), &format!("track-count-{index}"));
            store
                .create_track(TrackDraft {
                    id: None,
                    artist_id: artist_id.to_owned(),
                    name: format!("Track {index}"),
                    tempo: 90,
                    length: "03:42".to_owned(),
                    working_directory: Some(working_directory),
                    artwork_source: None,
                })
                .expect("track can be created");
        }

        let counts = store.track_counts_by_artist().expect("counts can be built");
        assert_eq!(counts.get("abcdef123456"), Some(&2));
        assert_eq!(counts.get("fedcba654321"), Some(&1));
        assert_eq!(counts.get("0123456789ab"), None);
    }
}
