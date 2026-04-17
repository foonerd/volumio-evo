//! Favourites and JSON-backed playlists (Node `/data/favourites/`, `/data/playlist/`).
//! File I/O only — browse/UI types live in [`crate::mpd`].

use crate::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn favourites_dir() -> PathBuf {
    paths::settings_dir().join("favourites")
}

pub fn playlist_json_dir() -> PathBuf {
    paths::settings_dir().join("playlist")
}

pub fn ensure_playlist_dirs() -> Result<()> {
    fs::create_dir_all(favourites_dir()).context("favourites")?;
    fs::create_dir_all(playlist_json_dir()).context("playlist")?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntry {
    pub service: String,
    pub uri: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub albumart: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
}

/// Node `sanitizeUri`: strip `music-library/` and `mnt/` for comparisons.
pub fn sanitize_uri(u: &str) -> String {
    u.replace("music-library/", "").replace("mnt/", "")
}

fn read_entries(path: &Path) -> Result<Vec<PlaylistEntry>> {
    if !path.is_file() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(path)?;
    if data.trim().is_empty() {
        return Ok(vec![]);
    }
    Ok(serde_json::from_str(&data).context("parse playlist json")?)
}

fn write_entries(path: &Path, entries: &[PlaylistEntry]) -> Result<()> {
    if let Some(p) = path.parent() {
        fs::create_dir_all(p)?;
    }
    fs::write(path, serde_json::to_string_pretty(entries)?)?;
    Ok(())
}

pub fn load_favourites() -> Vec<PlaylistEntry> {
    read_entries(&favourites_dir().join("favourites")).unwrap_or_default()
}

pub fn load_radio_favourites() -> Vec<PlaylistEntry> {
    read_entries(&favourites_dir().join("radio-favourites")).unwrap_or_default()
}

pub fn json_playlist_path(name: &str) -> PathBuf {
    playlist_json_dir().join(name)
}

pub fn json_playlist_exists(name: &str) -> bool {
    json_playlist_path(name).is_file()
}

pub fn load_json_playlist(name: &str) -> Option<Vec<PlaylistEntry>> {
    let p = json_playlist_path(name);
    if p.is_file() {
        read_entries(&p).ok()
    } else {
        None
    }
}

pub fn write_json_playlist(name: &str, entries: &[PlaylistEntry]) -> Result<()> {
    ensure_playlist_dirs()?;
    write_entries(&json_playlist_path(name), entries)
}

pub fn create_empty_json_playlist(name: &str) -> Result<()> {
    write_json_playlist(name, &[])
}

pub fn delete_json_playlist(name: &str) -> Result<()> {
    let p = json_playlist_path(name);
    if p.is_file() {
        fs::remove_file(&p)?;
    }
    Ok(())
}

/// Play / enqueue (`playPlaylist`): Node special-cases **`favourites`** to the library favourites file only.
pub fn load_entries_for_play(name: &str) -> Option<Vec<PlaylistEntry>> {
    if name == "favourites" {
        return Some(load_favourites());
    }
    load_json_playlist(name)
}

/// Browse `playlists/…` and `getPlaylistContent`: per-playlist JSON if present; else library favourites for `favourites`.
pub fn load_entries_for_playlist_browse(name: &str) -> Option<Vec<PlaylistEntry>> {
    if name == "favourites" {
        if json_playlist_exists("favourites") {
            let from_pl = load_json_playlist("favourites").unwrap_or_default();
            if !from_pl.is_empty() {
                return Some(from_pl);
            }
        }
        return Some(load_favourites());
    }
    load_json_playlist(name)
}

pub fn list_json_playlist_names() -> Vec<String> {
    let Ok(rd) = fs::read_dir(playlist_json_dir()) else {
        return vec![];
    };
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    v.sort();
    v
}

pub fn merge_name_lists(json: Vec<String>, mpd: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for n in json.into_iter().chain(mpd.into_iter()) {
        if seen.insert(n.clone()) {
            out.push(n);
        }
    }
    out
}

/// True if `uri` is listed in either favourites file (Node `checkFavourites` matches sanitized URIs only).
pub fn is_uri_in_favourites(uri: Option<&str>) -> bool {
    let Some(u) = uri.filter(|s| !s.is_empty()) else {
        return false;
    };
    let want = sanitize_uri(u);
    for list in [load_favourites(), load_radio_favourites()] {
        if list.iter().any(|e| sanitize_uri(&e.uri) == want) {
            return true;
        }
    }
    false
}

pub fn urifavourites_for_state(service: Option<String>, uri: Option<String>) -> serde_json::Value {
    let fav = is_uri_in_favourites(uri.as_deref());
    serde_json::json!({
        "service": service,
        "uri": uri,
        "favourite": fav
    })
}

pub fn normalize_volumio_uri(uri: &str) -> String {
    let u = uri.trim();
    if u.starts_with("music-library/")
        || u.contains("://")
        || u.starts_with("http://")
        || u.starts_with("https://")
    {
        return u.to_string();
    }
    if u.starts_with("mnt/") {
        return format!("music-library/{}", u.trim_start_matches("mnt/"));
    }
    format!("music-library/{}", u.trim_start_matches('/'))
}

pub fn entries_to_play_uris(entries: &[PlaylistEntry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| normalize_volumio_uri(&e.uri))
        .filter(|u| !u.is_empty())
        .collect()
}

pub fn add_to_favourites_entry(entry: PlaylistEntry) -> Result<()> {
    ensure_playlist_dirs()?;
    let radio = entry.service == "webradio";
    let path = if radio {
        favourites_dir().join("radio-favourites")
    } else {
        favourites_dir().join("favourites")
    };
    let mut list = read_entries(&path).unwrap_or_default();
    let key = sanitize_uri(&entry.uri);
    if list.iter().any(|e| sanitize_uri(&e.uri) == key) {
        return Ok(());
    }
    list.push(entry);
    write_entries(&path, &list)
}

pub fn remove_from_favourites(service: &str, uri: &str) -> Result<bool> {
    let key = sanitize_uri(uri);
    let radio = service == "webradio";
    let path = if radio {
        favourites_dir().join("radio-favourites")
    } else {
        favourites_dir().join("favourites")
    };
    let mut list = read_entries(&path)?;
    let before = list.len();
    list.retain(|e| !(sanitize_uri(&e.uri) == key && e.service == service));
    if list.len() == before {
        return Ok(false);
    }
    write_entries(&path, &list)?;
    Ok(true)
}

pub fn add_to_json_playlist(name: &str, entry: PlaylistEntry) -> Result<()> {
    ensure_playlist_dirs()?;
    let path = json_playlist_path(name);
    let mut list = read_entries(&path).unwrap_or_default();
    if list
        .iter()
        .any(|e| e.service == entry.service && sanitize_uri(&e.uri) == sanitize_uri(&entry.uri))
    {
        return Ok(());
    }
    list.push(entry);
    write_entries(&path, &list)
}

pub fn remove_from_json_playlist(name: &str, service: &str, uri: &str) -> Result<bool> {
    let path = json_playlist_path(name);
    let mut list = read_entries(&path)?;
    let want = sanitize_uri(uri);
    let before = list.len();
    list.retain(|e| !(e.service == service && sanitize_uri(&e.uri) == want));
    if list.len() == before {
        return Ok(false);
    }
    write_entries(&path, &list)?;
    Ok(true)
}
