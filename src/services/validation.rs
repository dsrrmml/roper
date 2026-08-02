use crate::error_handling::{AppError, AppResult};
use crate::models::{Artist, CasingMode, TrackSettings, UsedMaterial};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn is_valid_id(id: &str) -> bool {
    id.len() == 12
        && id
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

pub fn validate_id(id: &str) -> AppResult<()> {
    if is_valid_id(id) {
        Ok(())
    } else {
        Err(AppError::validation(
            "id",
            "must be exactly 12 lowercase hexadecimal characters",
        ))
    }
}

pub fn validate_name(name: &str, field: &str) -> AppResult<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        Err(AppError::validation(field, "must not be empty"))
    } else {
        Ok(trimmed.to_owned())
    }
}

pub fn validate_absolute_path(path: &Path, field: &str) -> AppResult<()> {
    if path.is_absolute() {
        Ok(())
    } else {
        Err(AppError::validation(
            field,
            "must be an absolute local path",
        ))
    }
}

pub fn validate_track_child_path(base: &Path, id: &str) -> AppResult<PathBuf> {
    validate_id(id)?;
    let path = base.join(id);
    if path.parent() == Some(base) {
        Ok(path)
    } else {
        Err(AppError::validation(
            "track_path",
            "track id must not escape the artist working directory",
        ))
    }
}

pub fn ensure_writable_directory(path: &Path) -> AppResult<()> {
    validate_absolute_path(path, "working_directory")?;
    fs::create_dir_all(path).map_err(|err| AppError::io(path, err))?;

    let probe = path.join(".roper-write-test");
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
        .map_err(|err| AppError::io(&probe, err))?;
    file.write_all(b"ok")
        .map_err(|err| AppError::io(&probe, err))?;
    file.sync_all().map_err(|err| AppError::io(&probe, err))?;
    fs::remove_file(&probe).map_err(|err| AppError::io(&probe, err))?;
    Ok(())
}

pub fn validate_tempo(tempo: u16) -> AppResult<()> {
    if (20..=300).contains(&tempo) {
        Ok(())
    } else {
        Err(AppError::validation(
            "tempo",
            "must be an integer BPM value from 20 to 300",
        ))
    }
}

pub fn validate_length(length: &str) -> AppResult<()> {
    let parts: Vec<&str> = length.split(':').collect();
    let valid = match parts.as_slice() {
        [minutes, seconds] => {
            is_numeric(minutes) && is_two_digit_seconds(seconds) && !minutes.is_empty()
        }
        [hours, minutes, seconds] => {
            is_numeric(hours)
                && is_two_digit_component(minutes)
                && is_two_digit_seconds(seconds)
                && !hours.is_empty()
        }
        _ => false,
    };

    if valid {
        Ok(())
    } else {
        Err(AppError::validation(
            "length",
            "must use MM:SS or HH:MM:SS with seconds from 00 to 59",
        ))
    }
}

fn is_numeric(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_digit())
}

fn is_two_digit_component(value: &str) -> bool {
    value.len() == 2 && is_numeric(value)
}

fn is_two_digit_seconds(value: &str) -> bool {
    if !is_two_digit_component(value) {
        return false;
    }
    value
        .parse::<u8>()
        .map(|seconds| seconds < 60)
        .unwrap_or(false)
}

pub fn validate_artist(artist: &Artist) -> AppResult<()> {
    validate_id(&artist.id)?;
    validate_name(&artist.name, "artist.name")?;
    if let Some(path) = &artist.image {
        validate_artwork_path(path)?;
    }
    Ok(())
}

pub fn validate_casing_mode(_mode: CasingMode) -> AppResult<()> {
    Ok(())
}

pub fn validate_used_material(entry: &UsedMaterial) -> AppResult<()> {
    let hash_is_valid = entry.normalized_hash.len() == 32
        && entry
            .normalized_hash
            .chars()
            .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase());
    if hash_is_valid {
        Ok(())
    } else {
        Err(AppError::validation(
            "used_material.normalized_hash",
            "must be a full lowercase MD5 hex value",
        ))
    }
}

pub fn validate_artwork_path(path: &Path) -> AppResult<()> {
    validate_absolute_path(path, "artwork")?;
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return Err(AppError::validation("artwork", "must be a PNG or JPG file"));
    };
    let extension = extension.to_ascii_lowercase();
    if extension == "png" || extension == "jpg" || extension == "jpeg" {
        Ok(())
    } else {
        Err(AppError::validation("artwork", "must be a PNG or JPG file"))
    }
}

pub fn validate_track_settings(settings: &TrackSettings) -> AppResult<()> {
    if settings.schema_version != 1 {
        return Err(AppError::validation(
            "schema_version",
            "unsupported track settings schema",
        ));
    }
    validate_id(&settings.id)?;
    validate_id(&settings.artist_id)?;
    validate_name(&settings.name, "track.name")?;
    validate_tempo(settings.tempo)?;
    validate_length(&settings.length)?;
    if let Some(path) = &settings.working_directory {
        validate_absolute_path(path, "working_directory")?;
    }
    validate_casing_mode(settings.casing_mode)?;
    for entry in &settings.used_material {
        validate_used_material(entry)?;
    }
    for entry in &settings.dismissed_material {
        validate_used_material(entry)?;
    }
    if let Some(path) = &settings.artwork {
        validate_artwork_path(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Artist, CasingMode, TrackSettings, UsedMaterial};

    #[test]
    fn artist_validation_rejects_empty_name() {
        let artist = Artist {
            id: "abcdef123456".to_owned(),
            name: "  ".to_owned(),
            description: String::new(),
            image: None,
        };
        assert!(validate_artist(&artist).is_err());
    }

    #[test]
    fn artist_validation_accepts_absolute_path() {
        let artist = Artist {
            id: "abcdef123456".to_owned(),
            name: "RMML".to_owned(),
            description: String::new(),
            image: None,
        };
        assert!(validate_artist(&artist).is_ok());
    }

    #[test]
    fn tempo_boundaries_are_enforced() {
        assert!(validate_tempo(19).is_err());
        assert!(validate_tempo(20).is_ok());
        assert!(validate_tempo(300).is_ok());
        assert!(validate_tempo(301).is_err());
    }

    #[test]
    fn length_format_accepts_required_shapes() {
        assert!(validate_length("03:42").is_ok());
        assert!(validate_length("74:10").is_ok());
        assert!(validate_length("01:12:08").is_ok());
        assert!(validate_length("01:12:60").is_err());
        assert!(validate_length("1:2").is_err());
    }

    #[test]
    fn track_settings_validation_checks_nested_fields() {
        let mut settings = TrackSettings::new(
            "abcdef123456".to_owned(),
            "fedcba654321".to_owned(),
            "Song".to_owned(),
            90,
            "03:42".to_owned(),
        );
        settings.casing_mode = CasingMode::Uppercase;
        settings.used_material = vec![UsedMaterial {
            normalized_hash: "0123456789abcdef0123456789abcdef".to_owned(),
            occurrence: 0,
        }];
        assert!(validate_track_settings(&settings).is_ok());
    }

    #[test]
    fn child_path_requires_valid_id() {
        assert!(validate_track_child_path(Path::new("/tmp/root"), "../oops").is_err());
        assert!(validate_track_child_path(Path::new("/tmp/root"), "abcdef123456").is_ok());
    }
}
