//! Album art resolution: path → folder cache → personal → default.
//! Matches Volumio behaviour for GET /albumart, /albumartd, /tinyart/*.

use std::path::{Path, PathBuf};

const COVER_NAMES: &[&str] = &[
    "coverart.jpg", "albumart.jpg", "coverart.png", "albumart.png",
    "cover.JPG", "Cover.JPG", "folder.JPG", "Folder.JPG",
    "cover.PNG", "Cover.PNG", "folder.PNG", "Folder.PNG",
    "cover.jpg", "Cover.jpg", "folder.jpg", "Folder.jpg",
    "cover.png", "Cover.png", "folder.png", "Folder.png",
];

const MAX_COVER_BYTES: u64 = 5_000_000;

/// Sanitize path param: strip music-library/ and mnt/ prefix (caller provides already URL-decoded path).
fn sanitize_path(path: &str) -> Option<String> {
    let s = path
        .trim_start_matches("music-library/")
        .trim_start_matches("mnt/")
        .trim_start_matches('/');
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

/// Resolve path param to an absolute folder path under music_root (for a file, its parent).
fn path_to_folder(music_root: &Path, path_param: &str) -> Option<PathBuf> {
    let rel = sanitize_path(path_param)?;
    let full = music_root.join(&rel);
    if full.exists() {
        if full.is_dir() {
            Some(full)
        } else {
            full.parent().map(Path::to_path_buf)
        }
    } else {
        None
    }
}

/// Check folder cache: albumart_root/folder/<rel>/extralarge.jpeg
fn try_folder_cache(
    albumart_root: &Path,
    music_root: &Path,
    folder: &Path,
) -> Option<(PathBuf, &'static str)> {
    let rel = folder.strip_prefix(music_root).ok()?;
    let cache_path = albumart_root
        .join("folder")
        .join(rel)
        .join("extralarge.jpeg");
    if cache_path.exists() {
        if std::fs::metadata(&cache_path).ok()?.len() > 0 {
            return Some((cache_path, "image/jpeg"));
        }
    }
    None
}

/// Check metadata cache: albumart_root/metadata/<rel>/metadata.jpeg
fn try_metadata_cache(
    albumart_root: &Path,
    music_root: &Path,
    folder: &Path,
) -> Option<(PathBuf, &'static str)> {
    let rel = folder.strip_prefix(music_root).ok()?;
    let meta_path = albumart_root.join("metadata").join(rel).join("metadata.jpeg");
    if meta_path.exists() && std::fs::metadata(&meta_path).ok()?.len() > 0 {
        return Some((meta_path, "image/jpeg"));
    }
    None
}

/// Look for cover file in folder (COVER_NAMES then any .jpg/.png).
fn try_folder_covers(folder: &Path) -> Option<(PathBuf, &'static str)> {
    for name in COVER_NAMES {
        let p = folder.join(name);
        if p.exists() {
            if let Ok(m) = std::fs::metadata(&p) {
                if m.len() > 0 && m.len() <= MAX_COVER_BYTES {
                    let ct = if name.ends_with(".png") || name.ends_with(".PNG") {
                        "image/png"
                    } else {
                        "image/jpeg"
                    };
                    return Some((p, ct));
                }
            }
        }
    }
    let entries = std::fs::read_dir(folder).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        if let Some(ext) = p.extension() {
            let ext = ext.to_string_lossy();
            if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") || ext.eq_ignore_ascii_case("png") {
                if let Ok(m) = std::fs::metadata(&p) {
                    if m.len() > 0 && m.len() <= MAX_COVER_BYTES {
                        let ct = if ext.eq_ignore_ascii_case("png") {
                            "image/png"
                        } else {
                            "image/jpeg"
                        };
                        return Some((p, ct));
                    }
                }
            }
        }
    }
    None
}

/// Check personal art: albumart_root/personal/album/artist/album/ or artist/artist/
fn try_personal(
    albumart_root: &Path,
    artist: &str,
    album: Option<&str>,
) -> Option<(PathBuf, &'static str)> {
    let artist_esc = sanitize_component(artist);
    if artist_esc.is_empty() {
        return None;
    }
    if let Some(album) = album {
        let album_esc = sanitize_component(album);
        let dir = albumart_root.join("personal").join("album").join(&artist_esc).join(&album_esc);
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if let Some(ext) = p.extension() {
                        let ext = ext.to_string_lossy();
                        if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") || ext.eq_ignore_ascii_case("png") {
                            let ct = if ext.eq_ignore_ascii_case("png") { "image/png" } else { "image/jpeg" };
                            return Some((p, ct));
                        }
                    }
                }
            }
        }
    } else {
        let dir = albumart_root.join("personal").join("artist").join(&artist_esc);
        if dir.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if let Some(ext) = p.extension() {
                        let ext = ext.to_string_lossy();
                        if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") || ext.eq_ignore_ascii_case("png") {
                            let ct = if ext.eq_ignore_ascii_case("png") { "image/png" } else { "image/jpeg" };
                            return Some((p, ct));
                        }
                    }
                }
            }
        }
    }
    None
}

fn sanitize_component(s: &str) -> String {
    s.chars()
        .map(|c| if c == '/' || c == '\\' || c == '\0' { '_' } else { c })
        .collect()
}

/// Parse web param: "artist/album/resolution" or "artist//resolution" (artist only).
fn parse_web(web: &str) -> Option<(String, Option<String>)> {
    let parts: Vec<&str> = web.split('/').collect();
    if parts.is_empty() {
        return None;
    }
    let artist = parts[0].to_string();
    let album = if parts.get(1).map(|s| s.is_empty()) == Some(false) {
        parts.get(1).map(|s| s.to_string())
    } else {
        None
    };
    Some((artist, album))
}

/// Resolve album art. Returns (file path, content_type) or None to serve default.
pub fn resolve(
    albumart_root: &Path,
    music_root: &Path,
    path_param: Option<&str>,
    web_param: Option<&str>,
    _metadata: bool,
) -> Option<(PathBuf, &'static str)> {
    // 1) Path-based: folder cache, metadata cache, folder covers
    if let Some(path) = path_param {
        if let Some(folder) = path_to_folder(music_root, path) {
            if let Some(r) = try_folder_cache(albumart_root, music_root, &folder) {
                return Some(r);
            }
            if let Some(r) = try_metadata_cache(albumart_root, music_root, &folder) {
                return Some(r);
            }
            if let Some(r) = try_folder_covers(&folder) {
                return Some(r);
            }
        }
    }

    // 2) Web-based: personal art (artist/album or artist only)
    if let Some(web) = web_param {
        if let Some((artist, album)) = parse_web(web) {
            if let Some(r) = try_personal(albumart_root, &artist, album.as_deref()) {
                return Some(r);
            }
        }
    }

    None
}
