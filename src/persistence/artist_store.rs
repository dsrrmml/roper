use crate::app_dirs;
use crate::error_handling::{AppError, AppResult};
use crate::models::{Artist, ArtistFile};
use crate::persistence::atomic_write::write_atomic;
use crate::services::artwork::import_artist_image;
use crate::services::id_generation::generate_unique_id;
use crate::services::validation::{validate_artist, validate_id, validate_name};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub struct ArtistStore {
    path: PathBuf,
    legacy_path: Option<PathBuf>,
}

impl ArtistStore {
    pub fn new_default() -> AppResult<Self> {
        Ok(Self {
            path: default_artist_store_path()?,
            legacy_path: Some(legacy_artist_store_path()?),
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

    pub fn load(&self) -> AppResult<ArtistFile> {
        let (load_path, migrate_after_load) = match self.load_path() {
            Some(result) => result,
            None => return Ok(ArtistFile::default()),
        };

        let file = self.load_from_path(&load_path)?;
        if migrate_after_load {
            self.save(&file)?;
        }
        Ok(file)
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

    fn load_from_path(&self, path: &Path) -> AppResult<ArtistFile> {
        if !path.exists() {
            return Ok(ArtistFile::default());
        }

        if path.is_file() {
            let bytes = fs::read(path).map_err(|err| AppError::io(path, err))?;
            let file: ArtistFile =
                serde_json::from_slice(&bytes).map_err(|err| AppError::json(path, err))?;
            validate_artist_file(&file)?;
            return Ok(file);
        }

        let mut artists = Vec::new();
        let entries = fs::read_dir(path).map_err(|err| AppError::io(path, err))?;
        for entry in entries {
            let entry = entry.map_err(|err| AppError::io(path, err))?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let bytes = fs::read(&path).map_err(|err| AppError::io(&path, err))?;
            let artist: Artist =
                serde_json::from_slice(&bytes).map_err(|err| AppError::json(&path, err))?;
            validate_artist(&artist)?;
            artists.push(artist);
        }
        artists.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        let file = ArtistFile {
            schema_version: 1,
            artists,
        };
        validate_artist_file(&file)?;
        Ok(file)
    }

    pub fn save(&self, file: &ArtistFile) -> AppResult<()> {
        validate_artist_file(file)?;
        if self.path.is_file() {
            let json =
                serde_json::to_vec_pretty(file).map_err(|err| AppError::json(&self.path, err))?;
            return write_atomic(&self.path, &json);
        }

        fs::create_dir_all(&self.path).map_err(|err| AppError::io(&self.path, err))?;
        let entries = fs::read_dir(&self.path).map_err(|err| AppError::io(&self.path, err))?;
        let mut files_to_remove = Vec::new();
        let expected_names = file
            .artists
            .iter()
            .map(|artist| format!("{}.json", artist.id))
            .collect::<HashSet<_>>();

        for entry in entries {
            let entry = entry.map_err(|err| AppError::io(&self.path, err))?;
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|value| value.to_str()) == Some("json")
                && !expected_names.contains(
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                )
            {
                files_to_remove.push(path);
            }
        }

        for path in files_to_remove {
            fs::remove_file(&path).map_err(|err| AppError::io(&path, err))?;
        }

        for artist in &file.artists {
            let artist_path = self.path.join(format!("{}.json", artist.id));
            let json = serde_json::to_vec_pretty(artist)
                .map_err(|err| AppError::json(&artist_path, err))?;
            write_atomic(&artist_path, &json)?;
        }
        Ok(())
    }

    pub fn create_artist(&self, name: &str, description: &str) -> AppResult<Artist> {
        self.create_artist_with_image(name, description, None)
    }

    pub fn create_artist_with_image(
        &self,
        name: &str,
        description: &str,
        image_source: Option<PathBuf>,
    ) -> AppResult<Artist> {
        let mut file = self.load()?;
        let name = validate_name(name, "artist.name")?;
        let existing: HashSet<String> = file
            .artists
            .iter()
            .map(|artist| artist.id.clone())
            .collect();
        let id = generate_unique_id(&format!("artist:{name}"), |candidate| {
            existing.contains(candidate)
        })?;
        let image = if let Some(source) = image_source {
            let target = artist_image_path(&id)?;
            Some(import_artist_image(&source, &target)?)
        } else {
            None
        };
        let artist = Artist {
            id,
            name,
            description: description.trim().to_owned(),
            image,
        };
        validate_artist(&artist)?;
        file.artists.push(artist.clone());
        file.artists
            .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        self.save(&file)?;
        Ok(artist)
    }

    pub fn update_artist_with_image(
        &self,
        id: &str,
        name: &str,
        description: &str,
        image_source: Option<PathBuf>,
    ) -> AppResult<Artist> {
        validate_id(id)?;
        let mut file = self.load()?;
        let Some(index) = file.artists.iter().position(|artist| artist.id == id) else {
            return Err(AppError::NotFound {
                path: self.path.clone(),
            });
        };

        let name = validate_name(name, "artist.name")?;
        let image = if let Some(source) = image_source {
            let target = artist_image_path(id)?;
            Some(import_artist_image(&source, &target)?)
        } else {
            file.artists[index].image.clone()
        };

        let artist = Artist {
            id: id.to_owned(),
            name,
            description: description.trim().to_owned(),
            image,
        };
        validate_artist(&artist)?;
        file.artists[index] = artist.clone();
        file.artists
            .sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
        self.save(&file)?;
        Ok(artist)
    }

    pub fn remove_artist(&self, id: &str) -> AppResult<Artist> {
        validate_id(id)?;
        let mut file = self.load()?;
        let Some(index) = file.artists.iter().position(|artist| artist.id == id) else {
            return Err(AppError::NotFound {
                path: self.path.clone(),
            });
        };
        let artist = file.artists.remove(index);
        self.save(&file)?;
        Ok(artist)
    }
}

pub fn default_config_dir() -> AppResult<PathBuf> {
    app_dirs::config_dir()
}

pub fn default_data_dir() -> AppResult<PathBuf> {
    app_dirs::data_dir()
}

pub fn legacy_config_dir() -> AppResult<PathBuf> {
    app_dirs::legacy_config_dir()
}

pub fn default_artist_store_path() -> AppResult<PathBuf> {
    Ok(default_config_dir()?.join("artists"))
}

pub fn legacy_artist_store_path() -> AppResult<PathBuf> {
    Ok(legacy_config_dir()?.join("artists"))
}

pub fn artist_image_path(artist_id: &str) -> AppResult<PathBuf> {
    Ok(default_data_dir()?
        .join("artist_images")
        .join(format!("{artist_id}.png")))
}

pub fn validate_artist_file(file: &ArtistFile) -> AppResult<()> {
    if file.schema_version != 1 {
        return Err(AppError::validation(
            "schema_version",
            "unsupported artist schema",
        ));
    }

    let mut ids = HashSet::new();
    for artist in &file.artists {
        validate_artist(artist)?;
        if !ids.insert(artist.id.clone()) {
            return Err(AppError::validation("artist.id", "duplicate artist id"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn corrupted_artist_json_is_reported() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("artists.json");
        fs::write(&path, b"{not-json").expect("corrupt file can be written");
        let store = ArtistStore::new(path);
        assert!(matches!(store.load(), Err(AppError::Json { .. })));
    }

    #[test]
    fn artist_store_roundtrips() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("config").join("artists.json");
        let store = ArtistStore::new(path);
        let artist = store
            .create_artist("  RMML  ", "desc")
            .expect("artist can be created");
        assert_eq!(artist.name, "RMML");
        let loaded = store.load().expect("artists can be loaded");
        assert_eq!(loaded.artists, vec![artist]);
    }

    #[test]
    fn artist_store_updates_existing_artist() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("config").join("artists.json");
        let store = ArtistStore::new(path);
        let artist = store
            .create_artist("RMML", "desc")
            .expect("artist can be created");

        let updated = store
            .update_artist_with_image(&artist.id, "Updated", "new desc", None)
            .expect("artist can be updated");

        assert_eq!(updated.id, artist.id);
        assert_eq!(updated.name, "Updated");
        assert_eq!(updated.description, "new desc");
        let loaded = store.load().expect("artists can load");
        assert_eq!(loaded.artists, vec![updated]);
    }

    #[test]
    fn artist_store_writes_per_artist_files_in_artists_directory() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("config").join("artists");
        let store = ArtistStore::new(path.clone());
        let artist = store
            .create_artist("RMML", "desc")
            .expect("artist can be created");

        let artist_path = path.join(format!("{}.json", artist.id));
        assert!(artist_path.exists());
        assert!(!path.join("artists.json").exists());

        let loaded = store.load().expect("artists can load");
        assert_eq!(loaded.artists, vec![artist]);
    }

    #[test]
    fn artist_store_removes_existing_artist() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("config").join("artists.json");
        let store = ArtistStore::new(path);
        let first = store
            .create_artist("First", "")
            .expect("first artist can be created");
        let second = store
            .create_artist("Second", "")
            .expect("second artist can be created");

        let removed = store
            .remove_artist(&first.id)
            .expect("artist can be removed");

        assert_eq!(removed, first);
        let loaded = store.load().expect("artists can load");
        assert_eq!(loaded.artists, vec![second]);
    }

    #[test]
    fn old_artist_json_without_image_still_loads() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("artists.json");
        fs::write(
            &path,
            r#"{
                "schema_version": 1,
                "artists": [{
                    "id": "abcdef123456",
                    "name": "Old Artist",
                    "description": ""
                }]
            }"#,
        )
        .expect("old artist json can be written");
        let store = ArtistStore::new(path);
        let loaded = store.load().expect("old artists can be loaded");
        assert_eq!(loaded.artists[0].image, None);
    }

    #[test]
    fn default_store_migrates_legacy_artist_metadata_to_single_root() {
        let dir = tempdir().expect("temp dir can be created");
        let path = dir.path().join("data").join("artists");
        let legacy_path = dir.path().join("config").join("artists");
        fs::create_dir_all(&legacy_path).expect("legacy artist dir can be created");
        fs::write(
            legacy_path.join("abcdef123456.json"),
            r#"{
                "id": "abcdef123456",
                "name": "Legacy Artist",
                "description": "",
                "image": null
            }"#,
        )
        .expect("legacy artist file can be written");

        let store = ArtistStore {
            path: path.clone(),
            legacy_path: Some(legacy_path),
        };

        let loaded = store.load().expect("legacy artists can load");
        assert_eq!(loaded.artists.len(), 1);
        assert_eq!(loaded.artists[0].name, "Legacy Artist");
        assert!(path.join("abcdef123456.json").exists());
    }
}
