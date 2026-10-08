//! The list of recently opened projects shown on the home screen, and the
//! folder new projects are saved to.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAX_ENTRIES: usize = 40;

/// Per-user data folder: `%APPDATA%\AetherEase` on Windows,
/// `~/Library/Application Support/AetherEase` on macOS and
/// `$XDG_DATA_HOME/aetherease` (or `~/.local/share/aetherease`) elsewhere.
pub fn data_dir() -> Option<PathBuf> {
    let env = |key| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if cfg!(windows) {
        env("APPDATA").map(|p| p.join("AetherEase"))
    } else if cfg!(target_os = "macos") {
        std::env::home_dir().map(|p| p.join("Library/Application Support/AetherEase"))
    } else {
        env("XDG_DATA_HOME")
            .or_else(|| std::env::home_dir().map(|p| p.join(".local/share")))
            .map(|p| p.join("aetherease"))
    }
}

/// Where projects created from the home screen are saved.
pub fn projects_dir() -> Option<PathBuf> {
    data_dir().map(|d| d.join("projects"))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub path: PathBuf,
    /// Seconds since the Unix epoch when the project was last opened or saved.
    pub last_used: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecentProjects {
    /// Most recently used first.
    pub entries: Vec<RecentEntry>,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl RecentProjects {
    fn file() -> Option<PathBuf> {
        data_dir().map(|d| d.join("recent.json"))
    }

    pub fn load() -> Self {
        Self::file()
            .and_then(|f| std::fs::read_to_string(f).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(file) = Self::file() else {
            return;
        };
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(file, json);
        }
    }

    /// Moves `path` to the top of the list, adding it if needed.
    pub fn touch(&mut self, path: &Path, now: u64) {
        self.entries.retain(|e| e.path != path);
        self.entries.insert(
            0,
            RecentEntry {
                path: path.to_owned(),
                last_used: now,
            },
        );
        self.entries.truncate(MAX_ENTRIES);
    }

    pub fn remove(&mut self, path: &Path) {
        self.entries.retain(|e| e.path != path);
    }
}

/// A path in `dir` for a new project called `name` that doesn't overwrite an
/// existing file.
pub fn unique_project_path(dir: &Path, name: &str, extension: &str) -> PathBuf {
    let stem: String = name
        .chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let stem = stem.trim().trim_end_matches('.');
    let stem = if stem.is_empty() { "Untitled" } else { stem };
    let mut path = dir.join(format!("{stem}.{extension}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem} ({n}).{extension}"));
        n += 1;
    }
    path
}

/// "just now", "5 min ago", "3 h ago", "2 days ago".
pub fn relative_time(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_moves_to_front_without_duplicates() {
        let mut recent = RecentProjects::default();
        recent.touch(Path::new("a.aether"), 1);
        recent.touch(Path::new("b.aether"), 2);
        recent.touch(Path::new("a.aether"), 3);
        let paths: Vec<_> = recent.entries.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            vec![PathBuf::from("a.aether"), PathBuf::from("b.aether")]
        );
        assert_eq!(recent.entries[0].last_used, 3);
        recent.remove(Path::new("a.aether"));
        assert_eq!(recent.entries.len(), 1);
    }

    #[test]
    fn list_is_capped() {
        let mut recent = RecentProjects::default();
        for i in 0..100 {
            recent.touch(Path::new(&format!("{i}.aether")), i);
        }
        assert_eq!(recent.entries.len(), MAX_ENTRIES);
        assert_eq!(recent.entries[0].path, PathBuf::from("99.aether"));
    }

    #[test]
    fn unique_paths_avoid_existing_files_and_bad_characters() {
        let dir = std::env::temp_dir().join(format!("aetherease-test-{}", now_secs()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = unique_project_path(&dir, "My: clip?", "aether");
        assert_eq!(first.file_name().unwrap(), "My_ clip_.aether");
        std::fs::write(&first, "{}").unwrap();
        let second = unique_project_path(&dir, "My: clip?", "aether");
        assert_eq!(second.file_name().unwrap(), "My_ clip_ (2).aether");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn relative_times() {
        assert_eq!(relative_time(100, 110), "just now");
        assert_eq!(relative_time(0, 600), "10 min ago");
        assert_eq!(relative_time(0, 7200), "2 h ago");
        assert_eq!(relative_time(0, 90_000), "yesterday");
        assert_eq!(relative_time(0, 86_400 * 5), "5 days ago");
    }
}
