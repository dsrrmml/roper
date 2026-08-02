use crate::error_handling::{AppError, AppResult};
use crate::persistence::atomic_write::write_atomic;
use crate::persistence::track_store::TrackPaths;
use crate::services::validation::validate_artwork_path;
use image::imageops::FilterType;
use png::{BitDepth, ColorType, PixelDimensions, Unit};
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const ARTWORK_SIZE: u32 = 1417;
pub const ARTWORK_DPI: u32 = 300;
pub const ARTWORK_PIXELS_PER_METER: u32 = 11_811;
pub const ARTIST_IMAGE_SIZE: u32 = 150;
pub const ARTIST_IMAGE_DOWNSAMPLE_THRESHOLD_BYTES: u64 = 1_500_000;

pub fn import_track_artwork(source: &Path, _paths: &TrackPaths) -> AppResult<PathBuf> {
    validate_artwork_path(source)?;
    Ok(source.to_path_buf())
}

pub fn preferred_track_artwork_in_working_directory(directory: &Path) -> Option<PathBuf> {
    let artwork_dir = directory.join("artwork");
    [artwork_dir.join("artwork.jpg"), artwork_dir.join("artwork.png")]
        .into_iter()
        .find(|path| path.is_file() && validate_artwork_path(path).is_ok())
}

pub fn import_artist_image(source: &Path, target: &Path) -> AppResult<PathBuf> {
    let _should_downsample = fs::metadata(source)
        .map_err(|err| AppError::io(source, err))?
        .len()
        > ARTIST_IMAGE_DOWNSAMPLE_THRESHOLD_BYTES;
    process_square_png(source, target, ARTIST_IMAGE_SIZE, None)?;
    Ok(target.to_path_buf())
}

fn process_square_png(
    source: &Path,
    target: &Path,
    size: u32,
    pixels_per_meter: Option<u32>,
) -> AppResult<()> {
    validate_artwork_path(source)?;
    let reader = image::ImageReader::open(source).map_err(|err| AppError::io(source, err))?;
    let reader = reader
        .with_guessed_format()
        .map_err(|err| AppError::io(source, err))?;
    let image = reader.decode().map_err(|err| {
        AppError::validation(
            "artwork",
            format!("could not decode {}: {}", source.display(), err),
        )
    })?;

    let processed = image
        .resize_to_fill(size, size, FilterType::Lanczos3)
        .to_rgba8();
    let encoded = encode_png(processed.as_raw(), size, pixels_per_meter)?;
    write_atomic(target, &encoded)
}

fn encode_png(rgba: &[u8], size: u32, pixels_per_meter: Option<u32>) -> AppResult<Vec<u8>> {
    let mut bytes = Vec::new();
    {
        let cursor = Cursor::new(&mut bytes);
        let mut encoder = png::Encoder::new(cursor, size, size);
        encoder.set_color(ColorType::Rgba);
        encoder.set_depth(BitDepth::Eight);
        if let Some(pixels_per_meter) = pixels_per_meter {
            encoder.set_pixel_dims(Some(PixelDimensions {
                xppu: pixels_per_meter,
                yppu: pixels_per_meter,
                unit: Unit::Meter,
            }));
        }
        let mut writer = encoder.write_header().map_err(|err| {
            AppError::validation("artwork", format!("could not write PNG header: {err}"))
        })?;
        writer.write_image_data(rgba).map_err(|err| {
            AppError::validation("artwork", format!("could not write PNG data: {err}"))
        })?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::persistence::track_store::TrackPaths;
    use image::{ImageBuffer, Rgb};
    use std::fs::File;
    use std::io::BufReader;
    use tempfile::tempdir;

    #[test]
    fn artwork_selection_uses_original_path_without_copying() {
        let dir = tempdir().expect("temp dir can be created");
        let source = dir.path().join("source.jpg");
        let image = ImageBuffer::from_fn(320, 180, |x, y| {
            Rgb([(x % 255) as u8, (y % 255) as u8, 120])
        });
        image.save(&source).expect("source artwork can be saved");

        let paths = TrackPaths {
            directory: dir.path().join("track"),
            final_path: dir.path().join("track").join("final.txt"),
            raw_path: dir.path().join("track").join("raw.txt"),
            settings_path: dir.path().join("track").join("settings.json"),
            artwork_path: dir.path().join("track").join("artwork.png"),
        };

        let imported = import_track_artwork(&source, &paths).expect("artwork imports");
        assert_eq!(imported, source);
        assert!(source.exists());
        assert!(!paths.artwork_path.exists());
    }

    #[test]
    fn preferred_track_artwork_prefers_jpg_over_png() {
        let dir = tempdir().expect("temp dir can be created");
        let artwork_dir = dir.path().join("artwork");
        fs::create_dir_all(&artwork_dir).expect("artwork dir can be created");

        let png = artwork_dir.join("artwork.png");
        let jpg = artwork_dir.join("artwork.jpg");
        let image = ImageBuffer::from_fn(32, 32, |x, y| Rgb([(x % 255) as u8, 60, (y % 255) as u8]));
        image.save(&png).expect("png artwork can be saved");
        image.save(&jpg).expect("jpg artwork can be saved");

        assert_eq!(
            preferred_track_artwork_in_working_directory(dir.path()),
            Some(jpg)
        );
    }

    #[test]
    fn preferred_track_artwork_falls_back_to_png() {
        let dir = tempdir().expect("temp dir can be created");
        let artwork_dir = dir.path().join("artwork");
        fs::create_dir_all(&artwork_dir).expect("artwork dir can be created");

        let png = artwork_dir.join("artwork.png");
        let image = ImageBuffer::from_fn(32, 32, |x, y| Rgb([(x % 255) as u8, 60, (y % 255) as u8]));
        image.save(&png).expect("png artwork can be saved");

        assert_eq!(
            preferred_track_artwork_in_working_directory(dir.path()),
            Some(png)
        );
    }

    #[test]
    fn preferred_track_artwork_returns_none_without_supported_file() {
        let dir = tempdir().expect("temp dir can be created");
        let artwork_dir = dir.path().join("artwork");
        fs::create_dir_all(&artwork_dir).expect("artwork dir can be created");
        fs::write(artwork_dir.join("artwork.webp"), b"nope").expect("webp placeholder can be saved");

        assert_eq!(preferred_track_artwork_in_working_directory(dir.path()), None);
    }

    #[test]
    fn artist_image_import_writes_square_preview_png() {
        let dir = tempdir().expect("temp dir can be created");
        let source = dir.path().join("artist.png");
        let target = dir.path().join("artist-images").join("abcdef123456.png");
        let image =
            ImageBuffer::from_fn(512, 300, |x, y| Rgb([(x % 255) as u8, 60, (y % 255) as u8]));
        image.save(&source).expect("source image can be saved");

        let imported = import_artist_image(&source, &target).expect("artist image imports");
        assert_eq!(imported, target);
        let file = File::open(&imported).expect("processed artist image exists");
        let decoder = png::Decoder::new(BufReader::new(file));
        let reader = decoder.read_info().expect("artist PNG can be decoded");
        let info = reader.info();
        assert_eq!(info.width, ARTIST_IMAGE_SIZE);
        assert_eq!(info.height, ARTIST_IMAGE_SIZE);
    }
}
