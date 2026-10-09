//! Fonts for text layers: the built-in face plus every font installed on
//! the system, found by scanning the usual font folders once.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use ab_glyph::FontArc;
use serde::{Deserialize, Serialize};

/// Which font a text layer uses, by name, so projects open on machines
/// where the font file lives somewhere else. Empty means the built-in face.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FontChoice {
    pub family: String,
    pub style: String,
}

impl FontChoice {
    pub fn is_builtin(&self) -> bool {
        self.family.is_empty()
    }
}

pub const BUILTIN_NAME: &str = "Ubuntu Light (built in)";

/// One installed font face.
#[derive(Clone, Debug)]
pub struct FontFace {
    pub family: String,
    pub style: String,
    path: PathBuf,
    index: u32,
    /// 100 (thin) to 900 (black), for listing styles lightest first.
    weight: u16,
    italic: bool,
}

/// Every installed face, sorted by family then style. Scans on first use;
/// call [`scan_in_background`] at startup so that is rarely a wait.
pub fn faces() -> &'static [FontFace] {
    static FACES: OnceLock<Vec<FontFace>> = OnceLock::new();
    FACES.get_or_init(scan)
}

pub fn scan_in_background() {
    std::thread::spawn(faces);
}

/// Installed family names with their styles.
pub fn families() -> &'static [(String, Vec<String>)] {
    static FAMILIES: OnceLock<Vec<(String, Vec<String>)>> = OnceLock::new();
    FAMILIES.get_or_init(|| {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        for face in faces() {
            match out.last_mut() {
                Some((family, styles)) if *family == face.family => {
                    if !styles.contains(&face.style) {
                        styles.push(face.style.clone());
                    }
                }
                _ => out.push((face.family.clone(), vec![face.style.clone()])),
            }
        }
        out
    })
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        dirs.push(PathBuf::from(windir).join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join("Library/Fonts"));
        }
    } else {
        dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(&home).join(".local/share/fonts"));
            dirs.push(PathBuf::from(home).join(".fonts"));
        }
    }
    dirs
}

fn scan() -> Vec<FontFace> {
    let mut files = Vec::new();
    for dir in font_dirs() {
        walk(&dir, 0, &mut files);
    }
    let mut faces = Vec::new();
    for path in files {
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        let count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for index in 0..count {
            let Ok(face) = ttf_parser::Face::parse(&data, index) else {
                continue;
            };
            let Some(family) = name(&face, &[16, 1]) else {
                continue;
            };
            let style = name(&face, &[17, 2]).unwrap_or_else(|| "Regular".into());
            faces.push(FontFace {
                family,
                style,
                path: path.clone(),
                index,
                weight: face.weight().to_number(),
                italic: face.is_italic(),
            });
        }
    }
    faces.sort_by(|a, b| {
        let key = |f: &FontFace| (f.family.to_lowercase(), f.weight, f.italic, f.style.clone());
        key(a).cmp(&key(b))
    });
    faces.dedup_by(|a, b| a.family == b.family && a.style == b.style);
    faces
}

fn walk(dir: &std::path::Path, depth: u32, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < 4 {
                walk(&path, depth + 1, out);
            }
        } else if path.extension().is_some_and(|e| {
            ["ttf", "otf", "ttc", "otc"].contains(&e.to_string_lossy().to_lowercase().as_str())
        }) {
            out.push(path);
        }
    }
}

/// The first readable English (or any) name with one of `ids`, in order.
fn name(face: &ttf_parser::Face, ids: &[u16]) -> Option<String> {
    ids.iter().find_map(|&id| {
        let mut names = face.names().into_iter().filter(|n| n.name_id == id);
        let english = names
            .clone()
            .find(|n| n.is_unicode() && n.language_id == 0x0409)
            .and_then(|n| n.to_string());
        english.or_else(|| names.find_map(|n| n.to_string()))
    })
}

static BUILTIN: LazyLock<Arc<FontArc>> = LazyLock::new(|| {
    Arc::new(
        FontArc::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("bundled font is valid"),
    )
});

/// The font for `choice`, loading it the first time. Missing fonts fall
/// back to the family's first style, then to the built-in face.
pub fn font(choice: &FontChoice) -> Arc<FontArc> {
    if choice.is_builtin() {
        return BUILTIN.clone();
    }
    static LOADED: LazyLock<Mutex<HashMap<FontChoice, Arc<FontArc>>>> =
        LazyLock::new(Default::default);
    if let Some(font) = LOADED.lock().ok().and_then(|m| m.get(choice).cloned()) {
        return font;
    }
    let faces = faces();
    let face = faces
        .iter()
        .find(|f| f.family == choice.family && f.style == choice.style)
        .or_else(|| faces.iter().find(|f| f.family == choice.family));
    let font = face
        .and_then(|f| {
            let data = std::fs::read(&f.path).ok()?;
            ab_glyph::FontVec::try_from_vec_and_index(data, f.index)
                .ok()
                .map(FontArc::new)
        })
        .map(Arc::new)
        .unwrap_or_else(|| BUILTIN.clone());
    if let Ok(mut m) = LOADED.lock() {
        m.insert(choice.clone(), font.clone());
    }
    font
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fonts_fall_back_to_the_builtin_face() {
        let missing = FontChoice {
            family: "No Such Font 12345".into(),
            style: "Bold".into(),
        };
        assert!(Arc::ptr_eq(&font(&missing), &font(&FontChoice::default())));
    }

    #[test]
    fn installed_fonts_load_by_name() {
        // Only meaningful where the machine has fonts installed.
        let Some((family, styles)) = families().first() else {
            return;
        };
        let choice = FontChoice {
            family: family.clone(),
            style: styles[0].clone(),
        };
        let loaded = font(&choice);
        assert!(!Arc::ptr_eq(&loaded, &font(&FontChoice::default())));
    }
}
