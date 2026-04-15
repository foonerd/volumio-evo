//! Album art resolution: path → folder cache → metadata (incl. exiftool) → folder covers → personal
//! → online: Cover Art Archive (MusicBrainz, multi-release + title variants) → Last.fm → iTunes
//! → Volumio meta (artist-only web=) → default.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use crate::config::AlbumArtProvidersConfig;

const COVER_NAMES: &[&str] = &[
    "coverart.jpg", "albumart.jpg", "coverart.png", "albumart.png",
    "cover.JPG", "Cover.JPG", "folder.JPG", "Folder.JPG",
    "cover.PNG", "Cover.PNG", "folder.PNG", "Folder.PNG",
    "cover.jpg", "Cover.jpg", "folder.jpg", "Folder.jpg",
    "cover.png", "Cover.png", "folder.png", "Folder.png",
];

const MAX_COVER_BYTES: u64 = 5_000_000;
const DEFAULT_USER_AGENT: &str = "VolumioEvo/1.0 (https://volumio.org)";

static AUDIO_EXTENSIONS: &[&str] = &["mp3", "flac", "aif", "aiff", "m4a"];

static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("reqwest client")
    })
}

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

/// True if this browse URI maps to a directory that has a local cover image (`folder.jpg`, etc.).
/// Used to decide `albumart` vs Font Awesome `icon` on folder rows (bundled SVG may be unavailable on device).
pub fn folder_has_browse_cover_file(music_root: &Path, volumio_uri: &str) -> bool {
    let Some(folder) = path_to_folder(music_root, volumio_uri) else {
        return false;
    };
    try_folder_covers(&folder).is_some()
}

/// Resolve path param to an absolute folder path under music_root (for a file, its parent).
///
/// If the track file path does not exist on disk (stale MPD path, mount mismatch), we still try the
/// **parent directory** when it exists so `folder.jpg` / `cover.jpg` in the album folder are found.
fn path_to_folder(music_root: &Path, path_param: &str) -> Option<PathBuf> {
    let rel = sanitize_path(path_param)?;
    let full = music_root.join(&rel);
    if full.exists() {
        if full.is_dir() {
            return Some(full);
        }
        return full.parent().map(Path::to_path_buf);
    }
    // File missing or wrong root — album folder may still exist with user-placed art.
    if let Some(parent) = full.parent() {
        if parent.is_dir() {
            return Some(parent.to_path_buf());
        }
    }
    None
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

// ----- Online providers -----

#[derive(serde::Deserialize)]
struct MusicBrainzReleaseSearch {
    releases: Option<Vec<MusicBrainzRelease>>,
}

#[derive(serde::Deserialize)]
struct MusicBrainzRelease {
    id: Option<String>,
}

fn escape_lucene(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Match Node `miscellanea/albumart` behavior so "A & B" searches like "A and B" on providers.
fn normalize_provider_query(s: &str) -> String {
    s.replace('&', "and")
}

/// Strip common multi-disc / reissue suffixes so MusicBrainz matches classical box sets and opera splits.
fn album_title_search_variants(album: &str) -> Vec<String> {
    let base = album.trim();
    if base.is_empty() {
        return vec![];
    }
    let mut out = vec![base.to_string()];
    let lower = base.to_ascii_lowercase();
    let strip_tail = |s: &str, tail: &str| -> Option<String> {
        let t = s.trim_end();
        if t.len() >= tail.len() && t[t.len() - tail.len()..].eq_ignore_ascii_case(tail) {
            Some(t[..t.len() - tail.len()].trim().to_string())
        } else {
            None
        }
    };
    for tail in [
        " - disc 1",
        " - disc 2",
        " - disc 3",
        " - cd 1",
        " - cd 2",
        " (disc 1)",
        " (disc 2)",
        " (cd 1)",
        " (cd 2)",
    ] {
        if lower.ends_with(tail) {
            if let Some(s) = strip_tail(base, tail) {
                if !s.is_empty() && !out.iter().any(|x| x == &s) {
                    out.push(s);
                }
            }
            break;
        }
    }
    out
}

/// MusicBrainz asks for ~1 request/s to coverartarchive.org; MB search is separate, but space out repeated MB queries.
async fn sleep_mb_courtesy() {
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
}

async fn mb_release_search_mbids(
    client: &reqwest::Client,
    lucene_query: &str,
    user_agent: &str,
    limit: u8,
) -> Option<Vec<String>> {
    let url = format!(
        "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit={}",
        urlencoding::encode(lucene_query),
        limit
    );
    let resp = client
        .get(&url)
        .header("User-Agent", user_agent)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let search: MusicBrainzReleaseSearch = resp.json().await.ok()?;
    let releases = search.releases.unwrap_or_default();
    let ids: Vec<String> = releases
        .into_iter()
        .filter_map(|r| r.id)
        .collect();
    Some(ids)
}

async fn fetch_cover_art_archive_front(
    client: &reqwest::Client,
    release_mbid: &str,
    user_agent: &str,
) -> Option<Vec<u8>> {
    let caa_url = format!("https://coverartarchive.org/release/{}/front", release_mbid);
    let img_resp = client
        .get(&caa_url)
        .header("User-Agent", user_agent)
        .send()
        .await
        .ok()?;
    if !img_resp.status().is_success() {
        return None;
    }
    let bytes = img_resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes.to_vec())
}

/// Cover Art Archive (MusicBrainz): search releases, try several MBIDs (first hit often has no front art).
async fn try_cover_art_archive(
    client: &reqwest::Client,
    artist: &str,
    album: &str,
    user_agent: &str,
) -> Option<Vec<u8>> {
    const MB_LIMIT: u8 = 5;
    let mut first_album_variant = true;
    for album_part in album_title_search_variants(album) {
        if !first_album_variant {
            sleep_mb_courtesy().await;
        }
        first_album_variant = false;
        let query = format!(
            "artist:\"{}\" AND release:\"{}\"",
            escape_lucene(artist),
            escape_lucene(&album_part)
        );
        let Some(mbids) = mb_release_search_mbids(client, &query, user_agent, MB_LIMIT).await else {
            continue;
        };
        if mbids.is_empty() {
            continue;
        }
        for mbid in mbids {
            if let Some(bytes) = fetch_cover_art_archive_front(client, &mbid, user_agent).await {
                return Some(bytes);
            }
        }
    }
    None
}

#[derive(serde::Deserialize)]
struct LastFmAlbumInfo {
    album: Option<LastFmAlbum>,
}

#[derive(serde::Deserialize)]
struct LastFmAlbum {
    image: Option<Vec<LastFmImage>>,
}

#[derive(serde::Deserialize)]
struct LastFmImage {
    #[serde(rename = "#text")]
    url: String,
    size: Option<String>,
}

/// Last.fm: album.getinfo (or artist.getinfo if no album).
async fn try_lastfm(
    client: &reqwest::Client,
    artist: &str,
    album: Option<&str>,
    api_key: &str,
) -> Option<Vec<u8>> {
    let (method, url) = if let Some(album) = album {
        (
            "album.getinfo",
            format!(
                "https://ws.audioscrobbler.com/2.0/?format=json&api_key={}&method=album.getinfo&artist={}&album={}",
                api_key,
                urlencoding::encode(artist),
                urlencoding::encode(album)
            ),
        )
    } else {
        (
            "artist.getinfo",
            format!(
                "https://ws.audioscrobbler.com/2.0/?format=json&api_key={}&method=artist.getinfo&artist={}",
                api_key,
                urlencoding::encode(artist)
            ),
        )
    };
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let text = resp.text().await.ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    if json.get("error").is_some() {
        return None;
    }
    let image_url = if method == "album.getinfo" {
        let info: LastFmAlbumInfo = serde_json::from_str(&text).ok()?;
        let images = info.album?.image?;
        // Prefer extralarge then large then last
        images
            .iter()
            .find(|i| i.size.as_deref() == Some("extralarge"))
            .or_else(|| images.iter().find(|i| i.size.as_deref() == Some("large")))
            .or_else(|| images.last())
            .map(|i| i.url.as_str())?
            .to_string()
    } else {
        let artist_obj = json.get("artist")?;
        let images = artist_obj.get("image")?.as_array()?;
        let img = images
            .iter()
            .find(|i| i.get("size").and_then(|s| s.as_str()) == Some("extralarge"))
            .or_else(|| images.iter().find(|i| i.get("size").and_then(|s| s.as_str()) == Some("large")))
            .or_else(|| images.last())?;
        img.get("#text")?.as_str()?.to_string()
    };
    if image_url.is_empty() {
        return None;
    }
    let img_resp = client.get(&image_url).send().await.ok()?;
    if !img_resp.status().is_success() {
        return None;
    }
    let bytes = img_resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes.to_vec())
}

#[derive(serde::Deserialize)]
struct ITunesSearchResult {
    results: Option<Vec<ITunesResult>>,
}

#[derive(serde::Deserialize)]
struct ITunesResult {
    #[serde(rename = "artworkUrl100")]
    artwork_url100: Option<String>,
    #[serde(rename = "artworkUrl600")]
    artwork_url600: Option<String>,
}

/// Apple Search often omits `artworkUrl600`; `artworkUrl100` is literally 100×100. mzstatic URLs use a
/// `100x100bb` path segment that can be swapped for `600x600bb` to fetch the same asset at higher res.
fn itunes_artwork_fetch_url(url: &str) -> String {
    if url.contains("100x100bb") {
        return url.replace("100x100bb", "600x600bb");
    }
    url.to_string()
}

/// iTunes Search API: search by artist+album, use first result artwork.
async fn try_itunes(client: &reqwest::Client, artist: &str, album: &str) -> Option<Vec<u8>> {
    let term = format!("{} {}", artist, album);
    let url = format!(
        "https://itunes.apple.com/search?term={}&entity=album&limit=1",
        urlencoding::encode(&term)
    );
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let search: ITunesSearchResult = resp.json().await.ok()?;
    let results = search.results?;
    let first = results.first()?;
    let raw = first
        .artwork_url600
        .as_deref()
        .filter(|s| !s.is_empty())
        .or_else(|| first.artwork_url100.as_deref().filter(|s| !s.is_empty()))?;
    let image_url = itunes_artwork_fetch_url(raw);
    if image_url.is_empty() {
        return None;
    }
    let img_resp = client.get(image_url).send().await.ok()?;
    if !img_resp.status().is_success() {
        return None;
    }
    let bytes = img_resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes.to_vec())
}

/// Volumio meta: artist art only.
async fn try_volumio_meta_artist(client: &reqwest::Client, artist: &str) -> Option<Vec<u8>> {
    let url = format!(
        "https://meta.volumio.org/metas/v1/getDatas?mode=artistArt&artist={}&variant=default",
        urlencoding::encode(artist)
    );
    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let data = json.get("data")?;
    let image_url = data.as_str().or_else(|| data.as_array()?.first()?.as_str())?;
    if image_url.is_empty() {
        return None;
    }
    let img_resp = client.get(image_url).send().await.ok()?;
    if !img_resp.status().is_success() {
        return None;
    }
    let bytes = img_resp.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes.to_vec())
}

/// Save downloaded image bytes to albumart_root/web/<artist>/<album>/<uuid>.jpeg and return (path, content_type).
fn save_web_cache(
    albumart_root: &Path,
    artist: &str,
    album: Option<&str>,
    bytes: &[u8],
) -> Option<(PathBuf, &'static str)> {
    let artist_esc = sanitize_component(artist);
    let album_esc = album.map(sanitize_component).unwrap_or_else(|| "_".to_string());
    let dir = albumart_root.join("web").join(&artist_esc).join(&album_esc);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::debug!("albumart web cache mkdir {:?}: {}", dir, e);
        return None;
    }
    let ext = "jpeg";
    let filename = format!("{}.{}", uuid::Uuid::new_v4(), ext);
    let path = dir.join(&filename);
    if std::fs::write(&path, bytes).is_err() {
        return None;
    }
    Some((path, "image/jpeg"))
}

/// Find first audio file in folder (mp3, flac, aif, etc.) for exiftool extraction.
fn first_audio_file_in_folder(folder: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(folder).ok()?;
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.path())
        .collect();
    files.sort();
    for p in files {
        if let Some(ext) = p.extension() {
            let ext = ext.to_string_lossy();
            if AUDIO_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) {
                return Some(p);
            }
        }
    }
    None
}

/// Extract embedded picture via exiftool into metadata cache. Returns (cache_path, "image/jpeg") or None.
/// Call from spawn_blocking (runs subprocess).
pub fn extract_metadata_to_cache(
    albumart_root: &Path,
    music_root: &Path,
    folder: &Path,
    exiftool_path: &Path,
) -> Option<(PathBuf, &'static str)> {
    if !exiftool_path.exists() {
        return None;
    }
    let audio_file = first_audio_file_in_folder(folder)?;
    let check = Command::new(exiftool_path)
        .arg(&audio_file)
        .output()
        .ok()?;
    if !check.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&check.stdout);
    if !stdout.contains("Picture") {
        return None;
    }
    let rel = folder.strip_prefix(music_root).ok()?;
    let meta_path = albumart_root.join("metadata").join(rel).join("metadata.jpeg");
    if let Err(e) = std::fs::create_dir_all(meta_path.parent()?) {
        tracing::debug!("albumart metadata mkdir {:?}: {}", meta_path.parent(), e);
        return None;
    }
    let extract = Command::new(exiftool_path)
        .args(["-b", "-Picture"])
        .arg(&audio_file)
        .output()
        .ok()?;
    if !extract.status.success() || extract.stdout.is_empty() {
        return None;
    }
    if std::fs::write(&meta_path, &extract.stdout).is_err() {
        return None;
    }
    Some((meta_path, "image/jpeg"))
}

/// Resolve album art (sync only: local + personal). Returns (file path, content_type) or None.
pub fn resolve(
    albumart_root: &Path,
    music_root: &Path,
    path_param: Option<&str>,
    web_param: Option<&str>,
    _metadata: bool,
) -> Option<(PathBuf, &'static str)> {
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

    if let Some(web) = web_param {
        if let Some((artist, album)) = parse_web(web) {
            if let Some(r) = try_personal(albumart_root, &artist, album.as_deref()) {
                return Some(r);
            }
            // Check web cache (previously downloaded from online)
            let artist_esc = sanitize_component(&artist);
            let album_esc = album.as_deref().map(sanitize_component).unwrap_or_else(|| "_".to_string());
            let dir = albumart_root.join("web").join(&artist_esc).join(&album_esc);
            if dir.is_dir() {
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for e in entries.flatten() {
                        let p = e.path();
                        if p.extension().map(|e| e.eq_ignore_ascii_case("jpeg") || e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("png")) == Some(true) {
                            if std::fs::metadata(&p).map(|m| m.len() > 0).unwrap_or(false) {
                                let ct = if p.extension().map(|e| e.eq_ignore_ascii_case("png")) == Some(true) {
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
        }
    }

    None
}

/// Resolve album art with online providers. Tries local/personal/web cache first (sync), then
/// MPD readpicture (if path + mpd_config), then exiftool (if metadata=true), then online.
pub async fn resolve_async(
    albumart_root: &Path,
    music_root: &Path,
    path_param: Option<&str>,
    web_param: Option<&str>,
    metadata: bool,
    providers: &AlbumArtProvidersConfig,
    exiftool_path: &Path,
    mpd_config: Option<&crate::mpd::MpdConfig>,
) -> Option<(PathBuf, &'static str)> {
    // 1) Sync local + personal + existing web cache
    if let Some(r) = resolve(albumart_root, music_root, path_param, web_param, metadata) {
        return Some(r);
    }

    // 2) MPD readpicture: embedded art from file (when path_param is a file URI)
    if let (Some(path), Some(config)) = (path_param, mpd_config) {
        let mpd_path = path.strip_prefix("music-library/").unwrap_or(path);
        if let Ok(Some((bytes, mime))) = crate::mpd::readpicture_connected(config, mpd_path).await
        {
            if let Some(folder) = path_to_folder(music_root, path) {
                if let Ok(rel) = folder.strip_prefix(music_root) {
                    let cache_dir = albumart_root.join("metadata").join(rel);
                    let (ext, ct) = match mime.as_deref() {
                        Some(m) if m.contains("png") => ("png", "image/png"),
                        _ => ("jpeg", "image/jpeg"),
                    };
                    let cache_path = cache_dir.join(format!("readpicture.{}", ext));
                    if std::fs::create_dir_all(&cache_dir).is_ok()
                        && std::fs::write(&cache_path, &bytes).is_ok()
                    {
                        return Some((cache_path, ct));
                    }
                }
            }
        }
    }

    // 3) Exiftool: extract embedded art to metadata cache when metadata=true and we have path
    if metadata {
        if let Some(path) = path_param {
            if let Some(folder) = path_to_folder(music_root, path) {
                let art_root = albumart_root.to_path_buf();
                let mus_root = music_root.to_path_buf();
                let exiftool = exiftool_path.to_path_buf();
                if let Some(r) = tokio::task::spawn_blocking(move || {
                    extract_metadata_to_cache(&art_root, &mus_root, &folder, &exiftool)
                })
                .await
                .ok()
                .and_then(|x| x)
                {
                    return Some(r);
                }
            }
        }
    }

    let (artist_raw, album_raw) = web_param.and_then(parse_web)?;
    let artist = normalize_provider_query(&artist_raw);
    let album: Option<String> = album_raw.map(|a| normalize_provider_query(&a));
    let client = http_client();
    let user_agent = providers
        .musicbrainz_user_agent
        .as_deref()
        .unwrap_or(DEFAULT_USER_AGENT);

    // 4) Cover Art Archive (album only)
    if let Some(ref album_name) = album {
        if let Some(bytes) = try_cover_art_archive(client, &artist, album_name, user_agent).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, Some(album_name), &bytes) {
                return Some(r);
            }
        }
    }

    // 5) Last.fm (album or artist)
    if let Some(ref key) = providers.lastfm_api_key {
        if let Some(bytes) = try_lastfm(client, &artist, album.as_deref(), key).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, album.as_deref(), &bytes) {
                return Some(r);
            }
        }
    }

    // 6) iTunes (album only)
    if let Some(ref album_name) = album {
        if let Some(bytes) = try_itunes(client, &artist, album_name).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, Some(album_name), &bytes) {
                return Some(r);
            }
        }
    }

    // 7) Volumio meta (artist-only lookups from web= artist//…)
    if album.is_none() {
        if let Some(bytes) = try_volumio_meta_artist(client, &artist).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, None, &bytes) {
                return Some(r);
            }
        }
    }

    None
}
