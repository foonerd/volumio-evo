//! Wallpapers for the stock Angular UI (**`/backgrounds/`** URLs, Socket.IO **`pushBackgrounds`**).
//!
//! Persisted under [`crate::paths::backgrounds_data_dir`] (default **`settings/backgrounds/`**): image
//! files plus **`state.toml`** (matches other Evo settings “unity” folders).

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

/// Max upload size — stock Volumio **`http/index.js`** uses **3 MB** for **`/backgrounds-upload`**.
pub const BACKGROUND_UPLOAD_MAX_BYTES: usize = 3_000_000;

/// Thumbnail max dimensions (stock **`miscellanea/appearance`** Jimp **300×200**).
const THUMB_W: u32 = 300;
const THUMB_H: u32 = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundState {
    #[serde(default = "default_background_type")]
    pub background_type: String,
    #[serde(default = "default_background_title")]
    pub background_title: String,
    #[serde(default)]
    pub background_path: String,
    #[serde(default = "default_background_color")]
    pub background_color: String,
    #[serde(default = "default_theme_str")]
    pub theme: String,
}

fn default_background_type() -> String {
    "color".into()
}

fn default_background_title() -> String {
    "".into()
}

fn default_background_color() -> String {
    "#333333".into()
}

fn default_theme_str() -> String {
    "default".into()
}

impl Default for BackgroundState {
    fn default() -> Self {
        Self {
            background_type: default_background_type(),
            background_title: default_background_title(),
            background_path: String::new(),
            background_color: default_background_color(),
            theme: default_theme_str(),
        }
    }
}

/// Loaded appearance state + directory path (images live next to **`state.toml`**).
#[derive(Debug, Clone)]
pub struct BackgroundAppearance {
    pub inner: BackgroundState,
}

fn state_path() -> PathBuf {
    crate::paths::backgrounds_state_path()
}

impl BackgroundAppearance {
    pub fn load() -> Self {
        let dir = crate::paths::backgrounds_data_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            tracing::warn!(
                "{} backgrounds mkdir {:?}: {}",
                crate::log_tags::EVO_UI,
                dir,
                e
            );
        }
        let path = state_path();
        let inner = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(t) => match toml::from_str::<BackgroundState>(&t) {
                    Ok(mut s) => {
                        s.normalize_in_place();
                        s
                    }
                    Err(e) => {
                        tracing::warn!(
                            "{} backgrounds: parse {:?}: {}; using defaults",
                            crate::log_tags::EVO_UI,
                            path,
                            e
                        );
                        BackgroundState::default()
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "{} backgrounds: read {:?}: {}",
                        crate::log_tags::EVO_UI,
                        path,
                        e
                    );
                    BackgroundState::default()
                }
            }
        } else {
            BackgroundState::default()
        };
        let mut s = Self { inner };
        s.repair_if_broken();
        s
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = state_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;
        }
        let t = toml::to_string_pretty(&self.inner).context("serialize backgrounds state")?;
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, t).with_context(|| format!("write {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;
        Ok(())
    }

    fn repair_if_broken(&mut self) {
        if self.inner.background_type == "background" && !self.inner.background_path.is_empty() {
            let p = backgrounds_file_path(&self.inner.background_path);
            if !p.exists() {
                tracing::info!(
                    "{} backgrounds: missing {:?}; falling back to color",
                    crate::log_tags::EVO_UI,
                    p
                );
                self.inner.background_type = "color".into();
                self.inner.background_title.clear();
                self.inner.background_path.clear();
            }
        }
    }

    /// Build **`pushBackgrounds`** payload (`current` + `available`), stock UI shape.
    pub fn push_backgrounds_value(&self) -> anyhow::Result<serde_json::Value> {
        let dir = crate::paths::backgrounds_data_dir();
        let mut available = Vec::new();
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if !p.is_file() {
                    continue;
                }
                let name = match p.file_name().and_then(|s| s.to_str()) {
                    Some(n) => n.to_string(),
                    None => continue,
                };
                if name == "state.toml" || name.ends_with(".toml.tmp") {
                    continue;
                }
                if name.starts_with("thumbnail-") {
                    continue;
                }
                if !is_allowed_image_name(&name) {
                    continue;
                }
                let thumb = thumbnail_name_for(&name);
                let stem = Path::new(&name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Wallpaper");
                available.push(json!({
                    "name": capitalize_volumio(stem),
                    "path": name,
                    "thumbnail": thumb,
                    "notDeletable": false,
                }));
            }
        }
        available.sort_by(|a, b| {
            let na = a.get("path").and_then(|x| x.as_str()).unwrap_or("");
            let nb = b.get("path").and_then(|x| x.as_str()).unwrap_or("");
            na.cmp(nb)
        });

        let current = json!({
            "name": self.inner.background_title,
            "path": self.inner.background_path,
        });

        Ok(json!({
            "current": current,
            "available": available,
        }))
    }

    /// Merge **`setBackgrounds`** socket payload (color **or** image selection).
    pub fn apply_set_payload(&mut self, data: &serde_json::Value) -> anyhow::Result<()> {
        let mut touched_theme = false;
        match crate::appearance::parse_theme_field(data) {
            crate::appearance::ParsedThemeField::Absent => {}
            crate::appearance::ParsedThemeField::Set(t) => {
                self.inner.theme = t;
                touched_theme = true;
            }
            crate::appearance::ParsedThemeField::Invalid => {
                anyhow::bail!("setBackgrounds: invalid theme");
            }
        }

        if let Some(color) = data.get("color").and_then(|v| v.as_str()) {
            let c = color.trim();
            if c.len() >= 4 && c.starts_with('#') {
                self.inner.background_type = "color".into();
                self.inner.background_color = c.to_string();
                self.inner.background_title.clear();
                self.inner.background_path.clear();
                return Ok(());
            }
        }
        let name = data
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let path_raw = data
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if name.is_none() && path_raw.is_empty() {
            if touched_theme {
                return Ok(());
            }
            anyhow::bail!("setBackgrounds: expected color, wallpaper, or theme");
        }
        let fname = extract_filename(path_raw);
        if fname.is_empty() || !is_allowed_image_name(&fname) {
            anyhow::bail!("setBackgrounds: invalid path");
        }
        let disk = backgrounds_file_path(&fname);
        if !disk.exists() {
            anyhow::bail!("setBackgrounds: file not found");
        }
        self.inner.background_type = "background".into();
        self.inner.background_title = name.unwrap_or_else(|| {
            capitalize_volumio(
                Path::new(&fname)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Wallpaper"),
            )
        });
        self.inner.background_path = fname;
        Ok(())
    }

    pub fn apply_delete_payload(&mut self, data: &serde_json::Value) -> anyhow::Result<()> {
        let path_raw = data
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let fname = extract_filename(path_raw);
        if fname.is_empty() || !is_allowed_image_name(&fname) {
            anyhow::bail!("deleteBackground: invalid path");
        }
        let disk = backgrounds_file_path(&fname);
        let thumb = backgrounds_file_path(&thumbnail_name_for(&fname));
        if disk.exists() {
            fs::remove_file(&disk).with_context(|| format!("remove {:?}", disk))?;
        }
        if thumb.exists() {
            let _ = fs::remove_file(&thumb);
        }
        if self.inner.background_type == "background" && self.inner.background_path == fname {
            self.inner.background_type = "color".into();
            self.inner.background_title.clear();
            self.inner.background_path.clear();
            self.inner.background_color = default_background_color();
        }
        Ok(())
    }

    pub fn merge_into_ui_settings(&self, language: &str, active_layout: &str) -> serde_json::Value {
        let mut base = serde_json::json!({
            "language": language,
            "theme": self.inner.theme,
            "active_layout": active_layout,
        });
        if self.inner.background_type == "background" && !self.inner.background_path.is_empty() {
            base.as_object_mut().unwrap().extend(
                serde_json::json!({
                    "background": {
                        "title": self.inner.background_title,
                        "path": self.inner.background_path,
                    }
                })
                .as_object()
                .unwrap()
                .clone(),
            );
        } else {
            base.as_object_mut().unwrap().extend(
                serde_json::json!({
                    "color": self.inner.background_color,
                })
                .as_object()
                .unwrap()
                .clone(),
            );
        }
        base
    }
}

impl BackgroundState {
    fn normalize_in_place(&mut self) {
        self.background_type = self.background_type.trim().to_string();
        if self.background_type != "background" && self.background_type != "color" {
            self.background_type = "color".into();
        }
        self.theme = self.theme.trim().to_string();
        if self.theme.is_empty() {
            self.theme = default_theme_str();
        }
    }
}

pub fn backgrounds_file_path(filename: &str) -> PathBuf {
    crate::paths::backgrounds_data_dir().join(filename)
}

fn thumbnail_name_for(fname: &str) -> String {
    format!("thumbnail-{fname}")
}

fn is_allowed_image_name(name: &str) -> bool {
    let lower: String = name.chars().map(|c| c.to_ascii_lowercase()).collect();
    lower.ends_with(".jpg") || lower.ends_with(".jpeg") || lower.ends_with(".png")
}

fn extract_filename(path_or_url: &str) -> String {
    let s = path_or_url.trim();
    if s.is_empty() {
        return String::new();
    }
    let no_q = s.split('?').next().unwrap_or(s);
    Path::new(no_q)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

fn capitalize_volumio(stem: &str) -> String {
    let mut c = stem.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

/// Sanitize an upload **basename** (stock: spaces → **`-`**).
pub fn sanitize_upload_basename(original: &str) -> Option<String> {
    let base = Path::new(original)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(original);
    let base = base.replace(' ', "-");
    let base = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if base.is_empty() || !is_allowed_image_name(&base) {
        return None;
    }
    Some(base)
}

/// Write image bytes to the backgrounds dir, generate thumbnail, return basename.
pub fn save_upload_bytes(data: &[u8], filename: &str) -> anyhow::Result<String> {
    if data.len() > BACKGROUND_UPLOAD_MAX_BYTES {
        anyhow::bail!("background too large");
    }
    let Some(safe_name) = sanitize_upload_basename(filename) else {
        anyhow::bail!("invalid filename");
    };
    let dir = crate::paths::backgrounds_data_dir();
    fs::create_dir_all(&dir).with_context(|| format!("mkdir {}", dir.display()))?;
    let path = dir.join(&safe_name);
    fs::write(&path, data).with_context(|| format!("write {:?}", path))?;
    generate_thumbnail_for_file(&path).with_context(|| format!("thumbnail {:?}", path))?;
    Ok(safe_name)
}

/// Create or replace **`thumbnail-<name>`** next to **`name`** (stock Jimp behaviour).
pub fn generate_thumbnail_for_file(image_path: &Path) -> anyhow::Result<()> {
    let data = fs::read(image_path).with_context(|| format!("read {:?}", image_path))?;
    let img = image::load_from_memory(&data).context("decode background image")?;
    let thumb = img.thumbnail(THUMB_W, THUMB_H);
    let thumb_path = image_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(thumbnail_name_for(
            image_path.file_name().unwrap_or_default().to_string_lossy().as_ref(),
        ));
    let mut out = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Jpeg)
        .context("encode thumbnail")?;
    fs::write(&thumb_path, out).with_context(|| format!("write {:?}", thumb_path))?;
    Ok(())
}

/// Regenerate thumbnails for every non-thumbnail image in the backgrounds folder.
pub fn regenerate_all_thumbnails() -> anyhow::Result<usize> {
    let dir = crate::paths::backgrounds_data_dir();
    fs::create_dir_all(&dir).ok();
    let mut n = 0;
    if let Ok(rd) = fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = match p.file_name().and_then(|s| s.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if name == "state.toml" || name.starts_with("thumbnail-") {
                continue;
            }
            if !is_allowed_image_name(name) {
                continue;
            }
            if generate_thumbnail_for_file(&p).is_ok() {
                n += 1;
            }
        }
    }
    Ok(n)
}
