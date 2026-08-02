use std::env;
use std::path::{Path, PathBuf};

const INSTALLED_DATA_DIR: &str = "/usr/share/roper";

pub fn data_dir() -> PathBuf {
    data_dir_from_env().unwrap_or_else(default_data_dir)
}

pub fn icon_path(icon_name: &str) -> PathBuf {
    data_dir().join("icons").join(icon_name)
}

pub fn splash_path() -> PathBuf {
    data_dir().join("splash.jpg")
}

fn data_dir_from_env() -> Option<PathBuf> {
    let path = env::var_os("ROPER_DATA_DIR")?;
    let path = PathBuf::from(path);
    if path.is_absolute() { Some(path) } else { None }
}

fn default_data_dir() -> PathBuf {
    let development_dir = development_data_dir_from_executable();
    if development_dir.exists() {
        development_dir
    } else {
        PathBuf::from(INSTALLED_DATA_DIR)
    }
}

fn development_data_dir_from_executable() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .map(|path| path.join("src").join("resources"))
        .unwrap_or_else(|| PathBuf::from(INSTALLED_DATA_DIR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_path_appends_icons_directory() {
        let path = icon_path("menu.svg");
        assert!(path.ends_with("icons/menu.svg"));
    }

    #[test]
    fn splash_path_points_to_splash_image() {
        assert!(splash_path().ends_with("splash.jpg"));
    }
}
