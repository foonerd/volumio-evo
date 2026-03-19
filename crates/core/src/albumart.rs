//! Album art resolution: path → folder cache → metadata → folder covers → personal
//! → online providers (Cover Art Archive, Last.fm, iTunes, Volumio meta) → default.

use std::path::{Path, PathBuf};
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

/// Cover Art Archive (MusicBrainz): search release by artist+album, then get front image.
async fn try_cover_art_archive(
    client: &reqwest::Client,
    artist: &str,
    album: &str,
    user_agent: &str,
) -> Option<Vec<u8>> {
    let query = format!(
        "artist:\"{}\" AND release:\"{}\"",
        escape_lucene(artist),
        escape_lucene(album)
    );
    let url = format!(
        "https://musicbrainz.org/ws/2/release/?query={}&fmt=json&limit=1",
        urlencoding::encode(&query)
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
    let releases = search.releases?;
    let mbid = releases.first()?.id.as_deref()?;
    let caa_url = format!("https://coverartarchive.org/release/{}/front", mbid);
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
    let image_url = first
        .artwork_url600
        .as_deref()
        .or_else(|| first.artwork_url100.as_deref())?;
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
/// fetches from Cover Art Archive → Last.fm → iTunes → Volumio meta (artist only), caches to web/, returns path.
pub async fn resolve_async(
    albumart_root: &Path,
    music_root: &Path,
    path_param: Option<&str>,
    web_param: Option<&str>,
    metadata: bool,
    providers: &AlbumArtProvidersConfig,
) -> Option<(PathBuf, &'static str)> {
    // 1) Sync local + personal + existing web cache
    if let Some(r) = resolve(albumart_root, music_root, path_param, web_param, metadata) {
        return Some(r);
    }

    let (artist, album) = web_param.and_then(parse_web)?;
    let client = http_client();
    let user_agent = providers
        .musicbrainz_user_agent
        .as_deref()
        .unwrap_or(DEFAULT_USER_AGENT);

    // 2) Cover Art Archive (album only)
    if let Some(ref album_name) = album {
        if let Some(bytes) = try_cover_art_archive(client, &artist, album_name, user_agent).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, Some(album_name), &bytes) {
                return Some(r);
            }
        }
    }

    // 3) Last.fm (album or artist)
    if let Some(ref key) = providers.lastfm_api_key {
        if let Some(bytes) = try_lastfm(client, &artist, album.as_deref(), key).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, album.as_deref(), &bytes) {
                return Some(r);
            }
        }
    }

    // 4) iTunes (album only)
    if let Some(ref album_name) = album {
        if let Some(bytes) = try_itunes(client, &artist, album_name).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, Some(album_name), &bytes) {
                return Some(r);
            }
        }
    }

    // 5) Volumio meta (artist only when no album)
    if album.is_none() {
        if let Some(bytes) = try_volumio_meta_artist(client, &artist).await {
            if let Some(r) = save_web_cache(albumart_root, &artist, None, &bytes) {
                return Some(r);
            }
        }
    }

    None
}
