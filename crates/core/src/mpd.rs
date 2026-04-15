//! MPD client: connect and run commands. Used by the v1 API to mirror Volumio backend behaviour.

use crate::albumart;
use crate::config::MUSIC_SOURCE_NAMES;
use anyhow::Result;
use std::path::Path;
use urlencoding::decode;
use mpd_client::{
    commands::{
        Add, ClearQueue, CurrentSong, Move, Next, Play, Previous, Queue, Rescan, Seek as MpdSeekCmd,
        SeekMode, SetConsume, SetPause as MpdPause, SetRandom, SetRepeat, SetVolume, Song, SongPosition,
        Status, Stop, Update,
    },
    protocol::command::Command as RawCommand,
    responses::PlayState,
    Client,
};
use std::io;
use serde::Serialize;
use std::time::Duration;
use tokio::net::TcpStream;

/// MPD connection config (host and port from main config).
#[derive(Clone, Debug)]
pub struct MpdConfig {
    pub host: String,
    pub port: u16,
}

impl MpdConfig {
    pub fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

/// Connect to MPD, run get_state, then close. Avoids closure lifetime issues.
pub async fn get_state_connected(config: &MpdConfig, music_root: &Path) -> Result<VolumioState> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    get_state(&mut client, music_root).await
}

/// Connect to MPD, run get_queue, then close.
pub async fn get_queue_connected(config: &MpdConfig) -> Result<Vec<QueueItem>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    get_queue(&mut client).await
}

/// Connect to MPD, run run_command, then close.
pub async fn run_command_connected(
    config: &MpdConfig,
    cmd: &str,
    volume: Option<u8>,
    position: Option<i64>,
    repeat: Option<bool>,
    random: Option<bool>,
) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    run_command(&mut client, cmd, volume, position, repeat, random).await
}

/// Volumio URI to MPD path: strip "music-library/" prefix.
fn volumio_uri_to_mpd_path(uri: &str) -> &str {
    uri.strip_prefix("music-library/").unwrap_or(uri)
}

/// Add URI to queue (value is Volumio-style URI e.g. music-library/INTERNAL/path/file.mp3).
pub async fn add_to_queue_connected(config: &MpdConfig, uri: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let path = volumio_uri_to_mpd_path(uri);
    let raw = RawCommand::new("add").argument(path);
    client.raw_command(raw).await?;
    Ok(())
}

/// Add multiple URIs to queue (one MPD connection). For addQueueUids.
pub async fn add_multiple_to_queue_connected(config: &MpdConfig, uris: &[String]) -> Result<()> {
    if uris.is_empty() {
        return Ok(());
    }
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    for uri in uris {
        let path = volumio_uri_to_mpd_path(uri.trim());
        if path.is_empty() {
            continue;
        }
        client
            .raw_command(RawCommand::new("add").argument(path))
            .await?;
    }
    Ok(())
}

/// Remove item at queue position (0-based). Volumio UI may send 1-based; caller can pass pos - 1.
pub async fn remove_from_queue_connected(config: &MpdConfig, position: u32) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("delete").argument(position.to_string()))
        .await?;
    Ok(())
}

/// Move queue item from position `from` to position `to` (0-based). Matches Volumio moveQueue.
pub async fn move_queue_connected(config: &MpdConfig, from: u32, to: u32) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let cmd = Move::position(SongPosition(from as usize)).to_position(SongPosition(to as usize));
    client.command(cmd).await?;
    Ok(())
}

/// Add URI to play next (insert after current song). Matches Volumio playNext.
pub async fn play_next_connected(config: &MpdConfig, uri: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let path = volumio_uri_to_mpd_path(uri);
    client.command(Add::uri(path).after_current(0)).await?;
    Ok(())
}

/// Clear queue, add URI, and start playing.
pub async fn add_play_connected(config: &MpdConfig, uri: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client.command(ClearQueue).await?;
    let path = volumio_uri_to_mpd_path(uri);
    client.raw_command(RawCommand::new("add").argument(path)).await?;
    client.command(Play::current()).await?;
    Ok(())
}

/// Append URI to queue and play the new track (Volumio Web UI `addPlay` → `commandRouter.addPlay`).
pub async fn add_play_append_connected(config: &MpdConfig, uri: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let path = volumio_uri_to_mpd_path(uri);
    let n = client.command(Queue::all()).await?.len();
    client.raw_command(RawCommand::new("add").argument(path)).await?;
    client
        .command(Play::song(Song::Position(SongPosition(n))))
        .await?;
    Ok(())
}

/// Clear queue and add single URI (no play). For replaceAndPlayCue.
pub async fn clear_and_add_connected(config: &MpdConfig, uri: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client.command(ClearQueue).await?;
    let path = volumio_uri_to_mpd_path(uri);
    client.raw_command(RawCommand::new("add").argument(path)).await?;
    Ok(())
}

/// Clear queue, add URIs in order, start playing at play_index (0-based). For playItemsList.
pub async fn play_items_list_connected(
    config: &MpdConfig,
    uris: &[String],
    play_index: usize,
) -> Result<()> {
    if uris.is_empty() {
        return Ok(());
    }
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client.command(ClearQueue).await?;
    for uri in uris {
        let path = volumio_uri_to_mpd_path(uri);
        client.raw_command(RawCommand::new("add").argument(path)).await?;
    }
    client
        .command(Play::song(Song::Position(SongPosition(play_index))))
        .await?;
    Ok(())
}

/// One item in a browse listing (folder or song). Matches Volumio pushBrowseLibrary shape.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub title: String,
    pub uri: String,
    pub service: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub albumart: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// Response for GET /api/v1/browse. Matches Volumio navigation structure.
#[derive(Debug, Serialize)]
pub struct BrowseResponse {
    pub navigation: BrowseNavigation,
}

#[derive(Debug, Serialize)]
pub struct BrowseNavigation {
    pub prev: BrowsePrev,
    pub lists: Vec<BrowseList>,
}

#[derive(Debug, Serialize)]
pub struct BrowsePrev {
    pub uri: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseList {
    pub available_list_views: Vec<&'static str>,
    pub items: Vec<BrowseItem>,
}

/// In-progress file entry while parsing lsinfo.
struct FileEntry {
    uri: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<u64>,
}

fn flush_file_item(current: &mut Option<FileEntry>, items: &mut Vec<BrowseItem>) {
    if let Some(f) = current.take() {
        let albumart = Some(format!(
            "/albumart?path={}&icon=music",
            urlencoding::encode(&f.uri)
        ));
        items.push(BrowseItem {
            item_type: "song".to_string(),
            title: f.title,
            uri: f.uri,
            service: "mpd".to_string(),
            artist: f.artist,
            album: f.album,
            duration: f.duration,
            albumart,
            icon: None,
        });
    }
}

/// Matches Node `uri.indexOf('music-library/INTERNAL')` for `internal-folder` vs `folder`.
fn browse_uri_is_under_internal(browse_uri: &str) -> bool {
    browse_uri == "music-library/INTERNAL"
        || browse_uri.starts_with("music-library/INTERNAL/")
}

/// Node `lsInfo` directory row types: `remdisk`, `internal-folder`, or `folder`.
fn lsinfo_directory_item_type(browse_uri: &str, mpd_path: &str) -> String {
    let segments: Vec<&str> = mpd_path.split('/').filter(|s| !s.is_empty()).collect();
    let is_remdisk = segments.len() == 2 && segments[0] == "USB";
    if is_remdisk {
        "remdisk".to_string()
    } else if browse_uri_is_under_internal(browse_uri) {
        "internal-folder".to_string()
    } else {
        "folder".to_string()
    }
}

/// Parse MPD lsinfo Frame into BrowseItems. Frame has key-value pairs; "directory" and "file" start new entries.
/// `browse_uri` is the listing URI (e.g. `music-library/INTERNAL`), used like Node `lsInfo` `uri` for typing rows.
/// `music_root` resolves folder cover files vs Font Awesome folder icon (stock UI; no bundled `/albumart` SVG required).
fn parse_lsinfo_frame(
    frame: mpd_client::protocol::response::Frame,
    browse_uri: &str,
    music_root: &Path,
) -> Vec<BrowseItem> {
    let mut items = Vec::new();
    let mut current_file: Option<FileEntry> = None;

    for (key, value) in frame.fields() {
        match key {
            "directory" => {
                flush_file_item(&mut current_file, &mut items);
                let name = value
                    .rsplit('/')
                    .next()
                    .unwrap_or(value)
                    .to_string();
                if name.starts_with('.') {
                    continue;
                }
                let item_uri = format!("music-library/{}", value);
                let item_type = lsinfo_directory_item_type(browse_uri, value);
                let has_cover = albumart::folder_has_browse_cover_file(music_root, &item_uri);
                let (albumart, icon) = if has_cover {
                    (
                        Some(format!(
                            "/albumart?path={}",
                            urlencoding::encode(&item_uri)
                        )),
                        None,
                    )
                } else if item_type == "remdisk" {
                    (None, Some("fa fa-usb".to_string()))
                } else {
                    (None, Some("fa fa-folder-open-o".to_string()))
                };
                items.push(BrowseItem {
                    item_type,
                    title: name,
                    uri: item_uri,
                    service: "mpd".to_string(),
                    artist: None,
                    album: None,
                    duration: None,
                    albumart,
                    icon,
                });
            }
            "file" => {
                flush_file_item(&mut current_file, &mut items);
                let name = value
                    .rsplit('/')
                    .next()
                    .unwrap_or(value)
                    .to_string();
                let item_uri = format!("music-library/{}", value);
                current_file = Some(FileEntry {
                    uri: item_uri,
                    title: name,
                    artist: None,
                    album: None,
                    duration: None,
                });
            }
            "Title" => {
                if let Some(ref mut f) = current_file {
                    f.title = value.to_string();
                }
            }
            "Artist" => {
                if let Some(ref mut f) = current_file {
                    f.artist = Some(value.to_string());
                }
            }
            "Album" => {
                if let Some(ref mut f) = current_file {
                    f.album = Some(value.to_string());
                }
            }
            "Time" => {
                if let Some(ref mut f) = current_file {
                    if let Ok(secs) = value.parse::<u64>() {
                        f.duration = Some(secs);
                    }
                }
            }
            _ => {}
        }
    }
    flush_file_item(&mut current_file, &mut items);

    items
}

/// Collection stats from MPD (stats command). Returns artists, albums, songs, playtime (HH:MM:SS).
#[derive(Debug, Default, Serialize)]
pub struct CollectionStats {
    pub artists: u64,
    pub albums: u64,
    pub songs: u64,
    pub playtime: String,
}

pub async fn collection_stats_connected(config: &MpdConfig) -> Result<CollectionStats> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("stats");
    let frame = client.raw_command(raw).await?;
    let mut stats = CollectionStats::default();
    stats.playtime = "00:00:00".to_string();
    for (k, v) in frame.fields() {
        match k {
            "songs" => stats.songs = v.parse().unwrap_or(0),
            "artists" => stats.artists = v.parse().unwrap_or(0),
            "albums" => stats.albums = v.parse().unwrap_or(0),
            "db_playtime" => {
                if let Ok(secs) = v.parse::<u64>() {
                    let h = secs / 3600;
                    let m = (secs % 3600) / 60;
                    let s = secs % 60;
                    stats.playtime = format!("{:02}:{:02}:{:02}", h, m, s);
                }
            }
            _ => {}
        }
    }
    Ok(stats)
}

/// List MPD stored playlists (listplaylists). Returns playlist names.
pub async fn list_playlists_connected(config: &MpdConfig) -> Result<Vec<String>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("listplaylists");
    let frame = client.raw_command(raw).await?;
    let names: Vec<String> = frame
        .fields()
        .filter(|(k, _)| *k == "playlist")
        .map(|(_, v)| v.to_string())
        .collect();
    Ok(names)
}

/// Set MPD consume mode (remove from queue when played).
pub async fn set_consume_connected(config: &MpdConfig, value: bool) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client.command(SetConsume(value)).await?;
    Ok(())
}

/// Rescan MPD database (optional path/uri). Returns job id.
pub async fn rescan_connected(config: &MpdConfig, path: Option<&str>) -> Result<u64> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let id = match path {
        None => client.command(Rescan::new()).await?,
        Some(p) => client.command(Rescan::new().uri(p)).await?,
    };
    Ok(id)
}

/// Update MPD database (optional path/uri). Returns job id.
pub async fn update_connected(config: &MpdConfig, path: Option<&str>) -> Result<u64> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let id = match path {
        None => client.command(Update::new()).await?,
        Some(p) => client.command(Update::new().uri(p)).await?,
    };
    Ok(id)
}

/// Read embedded picture from a file via MPD readpicture. URI is MPD path (e.g. "local/Artist/Album/file.flac").
/// Returns (picture bytes, optional mime type) or None if no art.
pub async fn readpicture_connected(
    config: &MpdConfig,
    mpd_path: &str,
) -> Result<Option<(Vec<u8>, Option<String>)>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    match client.album_art(mpd_path).await {
        Ok(Some((bytes, mime))) => Ok(Some((bytes.to_vec(), mime))),
        Ok(None) => Ok(None),
        Err(e) => Err(io::Error::new(io::ErrorKind::Other, e.to_string()).into()),
    }
}

/// List content of a stored playlist (listplaylist "name"). Returns URIs (music-library/...).
pub async fn list_playlist_content_connected(config: &MpdConfig, name: &str) -> Result<Vec<String>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("listplaylist").argument(name);
    let frame = client.raw_command(raw).await?;
    let uris: Vec<String> = frame
        .fields()
        .filter(|(k, _)| *k == "file")
        .map(|(_, v)| {
            let path = v.to_string();
            if path.starts_with("music-library/") {
                path
            } else {
                format!("music-library/{}", path)
            }
        })
        .collect();
    Ok(uris)
}

/// Load stored playlist into queue and start playing (replaces queue).
pub async fn load_playlist_connected(config: &MpdConfig, name: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("load").argument(name))
        .await?;
    client.command(Play::current()).await?;
    Ok(())
}

/// Save current queue as stored playlist.
pub async fn save_queue_to_playlist_connected(config: &MpdConfig, name: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("save").argument(name))
        .await?;
    Ok(())
}

/// Remove stored playlist.
pub async fn delete_playlist_connected(config: &MpdConfig, name: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("rm").argument(name))
        .await?;
    Ok(())
}

/// Create empty stored playlist (clear queue, save as name).
pub async fn create_playlist_connected(config: &MpdConfig, name: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client.command(ClearQueue).await?;
    client
        .raw_command(RawCommand::new("save").argument(name))
        .await?;
    Ok(())
}

/// Add URI to stored playlist. URI can be Volumio (music-library/...) or MPD path.
pub async fn add_to_playlist_connected(config: &MpdConfig, name: &str, uri: &str) -> Result<()> {
    let path = volumio_uri_to_mpd_path(uri);
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("playlistadd").argument(name).argument(path))
        .await?;
    Ok(())
}

/// Remove song at position (0-based) from stored playlist.
pub async fn remove_from_playlist_connected(
    config: &MpdConfig,
    name: &str,
    position: u32,
) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(
            RawCommand::new("playlistdelete")
                .argument(name)
                .argument(position.to_string()),
        )
        .await?;
    Ok(())
}

/// Load stored playlist into queue (append, do not clear). Then emit queue/state.
pub async fn enqueue_playlist_connected(config: &MpdConfig, name: &str) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("load").argument(name))
        .await?;
    Ok(())
}

/// Search MPD library (find any <query>). Returns browse-style response.
pub async fn search_connected(
    config: &MpdConfig,
    music_root: &Path,
    query: &str,
) -> Result<BrowseResponse> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(BrowseResponse {
            navigation: BrowseNavigation {
                prev: BrowsePrev {
                    uri: "music-library".to_string(),
                },
                lists: vec![BrowseList {
                    available_list_views: vec!["list", "grid"],
                    items: vec![],
                }],
            },
        });
    }
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    // find any "term" searches in any tag
    let raw = RawCommand::new("find").argument("any").argument(query);
    let frame = client.raw_command(raw).await?;
    let items = parse_lsinfo_frame(frame, "music-library", music_root);
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "music-library".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// Bundled `sourceicon` paths for the four storage roots (Node: `stickingMusicLibrary` + `lsinfo` names).
fn music_source_albumart(path_segment: &str) -> &'static str {
    match path_segment {
        "INTERNAL" => "/albumart?sourceicon=music_service/mpd/musiclibraryicon.png",
        "USB" => "/albumart?sourceicon=music_service/mpd/playlisticon.png",
        "NAS" => "/albumart?sourceicon=music_service/mpd/albumicon.png",
        "SMB" => "/albumart?sourceicon=music_service/mpd/artisticon.png",
        _ => "/albumart?sourceicon=music_service/mpd/musiclibraryicon.png",
    }
}

/// Root `music-library` listing: storage roots only (INTERNAL, USB, NAS, SMB), with art like Node browse rows.
/// Favourites / tag library / playlists are reached from the sidebar (`browseSources`), not duplicated here.
pub fn music_library_root_response() -> BrowseResponse {
    let items: Vec<BrowseItem> = MUSIC_SOURCE_NAMES
        .iter()
        .map(|(path_segment, title)| BrowseItem {
            item_type: "folder".to_string(),
            title: (*title).to_string(),
            uri: format!("music-library/{}", path_segment),
            service: "mpd".to_string(),
            artist: None,
            album: None,
            duration: None,
            albumart: Some(music_source_albumart(path_segment).to_string()),
            icon: None,
        })
        .collect();
    BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: String::new(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    }
}

/// When MPD browse fails, emit this so the stock UI still gets `navigation.lists[].items` arrays
/// (otherwise `browse.controller` / `browse-music` can throw on `forEach` / `map`).
pub fn empty_browse_response(prev_uri: impl Into<String>) -> BrowseResponse {
    BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: prev_uri.into(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items: vec![],
            }],
        },
    }
}

/// Legacy favourites are not backed by MPD; return empty list so the UI matches navigation shape.
pub fn browse_favourites_stub() -> BrowseResponse {
    BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "music-library".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items: vec![],
            }],
        },
    }
}

/// Collect unique values for one `list <tag>` MPD command.
async fn list_tag_values(config: &MpdConfig, tag: &str) -> Result<Vec<String>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("list").argument(tag);
    let frame = client.raw_command(raw).await?;
    let mut vals: Vec<String> = frame
        .fields()
        .filter_map(|(k, v)| {
            if k == tag {
                Some(v.to_string())
            } else {
                None
            }
        })
        .collect();
    vals.sort();
    vals.dedup();
    Ok(vals)
}

/// All artists (`artists://` root).
async fn browse_all_artists_connected(config: &MpdConfig) -> Result<BrowseResponse> {
    let artists = list_tag_values(config, "Artist").await?;
    let items: Vec<BrowseItem> = artists
        .into_iter()
        .map(|a| BrowseItem {
            item_type: "folder".to_string(),
            title: a.clone(),
            uri: format!(
                "artists://{}",
                urlencoding::encode(a.as_str())
            ),
            service: "mpd".to_string(),
            artist: Some(a),
            album: None,
            duration: None,
            albumart: None,
            icon: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "music-library".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// All distinct albums (`albums://` root). Click opens tracks tagged with that album (any artist).
async fn browse_all_albums_connected(config: &MpdConfig) -> Result<BrowseResponse> {
    let albums = list_tag_values(config, "Album").await?;
    let items: Vec<BrowseItem> = albums
        .into_iter()
        .map(|album| BrowseItem {
            item_type: "folder".to_string(),
            title: album.clone(),
            uri: format!("albums://{}", urlencoding::encode(album.as_str())),
            service: "mpd".to_string(),
            artist: None,
            album: Some(album),
            duration: None,
            albumart: None,
            icon: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "music-library".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// Songs when browsing by album title only (flat album list).
async fn browse_album_only_songs_connected(
    config: &MpdConfig,
    music_root: &Path,
    album: &str,
) -> Result<BrowseResponse> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("find")
        .argument("Album")
        .argument(album);
    let frame = client.raw_command(raw).await?;
    let items = parse_lsinfo_frame(frame, "music-library", music_root);
    let prev = "albums://".to_string();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev { uri: prev },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// All genres (`genres://` root).
async fn browse_all_genres_connected(config: &MpdConfig) -> Result<BrowseResponse> {
    let genres = list_tag_values(config, "Genre").await?;
    let items: Vec<BrowseItem> = genres
        .into_iter()
        .map(|g| BrowseItem {
            item_type: "folder".to_string(),
            title: g.clone(),
            uri: format!("genres://{}", urlencoding::encode(g.as_str())),
            service: "mpd".to_string(),
            artist: None,
            album: None,
            duration: None,
            albumart: None,
            icon: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "music-library".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// Artists under a genre (`genres://Rock` → list Artist Genre "Rock"`).
async fn browse_genre_connected(config: &MpdConfig, genre: &str) -> Result<BrowseResponse> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("list")
        .argument("Artist")
        .argument("Genre")
        .argument(genre);
    let frame = client.raw_command(raw).await?;
    let mut artists: Vec<String> = frame
        .fields()
        .filter_map(|(k, v)| if k == "Artist" { Some(v.to_string()) } else { None })
        .collect();
    artists.sort();
    artists.dedup();
    let items: Vec<BrowseItem> = artists
        .into_iter()
        .map(|a| BrowseItem {
            item_type: "folder".to_string(),
            title: a.clone(),
            uri: format!("artists://{}", urlencoding::encode(a.as_str())),
            service: "mpd".to_string(),
            artist: Some(a),
            album: None,
            duration: None,
            albumart: None,
            icon: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "genres://".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// List albums by artist (goTo type=artist). Uses MPD "list Album Artist <name>", returns folder items.
async fn browse_artist_connected(config: &MpdConfig, artist: &str) -> Result<BrowseResponse> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("list")
        .argument("Album")
        .argument("Artist")
        .argument(artist);
    let frame = client.raw_command(raw).await?;
    let mut albums: Vec<String> = frame
        .fields()
        .filter_map(|(k, v)| if k == "Album" { Some(v.to_string()) } else { None })
        .collect();
    albums.sort();
    albums.dedup();
    let items: Vec<BrowseItem> = albums
        .into_iter()
        .map(|album| {
            BrowseItem {
                item_type: "folder".to_string(),
                title: album.clone(),
                uri: format!("albums://{}/{}", artist, album),
                service: "mpd".to_string(),
                artist: Some(artist.to_string()),
                album: Some(album),
                duration: None,
                albumart: None,
                icon: None,
            }
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev {
                uri: "artists://".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// List songs in an album (goTo type=album). Uses MPD find Artist/Album, returns song items.
async fn browse_album_songs_connected(
    config: &MpdConfig,
    music_root: &Path,
    artist: &str,
    album: &str,
) -> Result<BrowseResponse> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("find")
        .argument("Artist")
        .argument(artist)
        .argument("Album")
        .argument(album);
    let frame = client.raw_command(raw).await?;
    let items = parse_lsinfo_frame(frame, "albums://find-tracks", music_root);
    let prev = format!("artists://{}", artist);
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev { uri: prev },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// Connect to MPD, run lsinfo for the given Volumio uri (e.g. "music-library/INTERNAL/..."), return browse response.
/// Handles virtual URIs: `artists://`, `albums://`, `genres://` (tag-based library, like classic Volumio).
pub async fn browse_connected(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<BrowseResponse> {
    if uri == "favourites" {
        return Ok(browse_favourites_stub());
    }

    if uri.starts_with("genres://") {
        let rest = uri.strip_prefix("genres://").unwrap_or("");
        let rest_dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        if rest_dec.is_empty() {
            return browse_all_genres_connected(config).await;
        }
        return browse_genre_connected(config, &rest_dec).await;
    }

    if uri.starts_with("artists://") {
        let rest = uri.strip_prefix("artists://").unwrap_or("");
        let rest_dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        if rest_dec.is_empty() {
            return browse_all_artists_connected(config).await;
        }
        return browse_artist_connected(config, &rest_dec).await;
    }

    if uri.starts_with("albums://") {
        let rest = uri.strip_prefix("albums://").unwrap_or("");
        let rest_dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        if rest_dec.is_empty() {
            return browse_all_albums_connected(config).await;
        }
        if let Some((a, b)) = rest_dec.split_once('/') {
            let artist = a.trim();
            let album = b.trim();
            if !artist.is_empty() && !album.is_empty() {
                return browse_album_songs_connected(config, music_root, artist, album).await;
            }
        }
        return browse_album_only_songs_connected(config, music_root, &rest_dec).await;
    }

    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;

    // Volumio uri: "music-library" or "music-library/path". MPD path: "" or "path".
    let (uri_prefix, mpd_path) = if uri == "music-library" || uri.is_empty() {
        ("music-library".to_string(), "")
    } else if let Some(stripped) = uri.strip_prefix("music-library/") {
        (uri.to_string(), stripped)
    } else {
        (uri.to_string(), uri)
    };

    let raw = if mpd_path.is_empty() {
        RawCommand::new("lsinfo")
    } else {
        RawCommand::new("lsinfo").argument(mpd_path)
    };
    let frame = client.raw_command(raw).await?;

    let items = parse_lsinfo_frame(frame, &uri_prefix, music_root);

    let prev = if uri == "music-library" || uri.is_empty() {
        "".to_string()
    } else {
        uri.rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_else(|| "music-library".to_string())
    };

    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            prev: BrowsePrev { uri: prev },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// Volumio-style state JSON (matches what the UI expects from getState).
#[derive(Debug, Serialize)]
pub struct VolumioState {
    pub status: Option<String>,
    pub position: Option<u32>,
    /// Elapsed position in **milliseconds** (Node `parseState`: `elapsed * 1000`).
    pub seek: Option<u64>,
    /// Track length in **seconds** (Node `parseState`: `time` field part after `:`).
    pub duration: Option<u64>,
    pub volume: Option<u8>,
    pub repeat: Option<bool>,
    pub random: Option<bool>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Volumio browse URI (`music-library/...`).
    pub uri: Option<String>,
    pub track_type: Option<String>,
    pub service: Option<String>,
    /// Cover URL for playback view (`GET /albumart?...`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub albumart: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub samplerate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitdepth: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<String>,
}

/// Map MPD `file` URL to Volumio `music-library/...` for album-art + browse parity.
fn volumio_uri_from_mpd_url(url: &str, music_root: &Path) -> String {
    let u = url.trim();
    if u.starts_with("music-library/") {
        return u.to_string();
    }
    if let Some(rest) = u.strip_prefix("file://") {
        let p = Path::new(rest);
        if let Ok(rel) = p.strip_prefix(music_root) {
            let s = rel.to_string_lossy().replace('\\', "/");
            return format!("music-library/{}", s.trim_start_matches('/'));
        }
    }
    format!("music-library/{}", u.trim_start_matches('/'))
}

/// Parse MPD `audio` field (`44100:24:2`, `dsd64:1:2`, …) like `volumio3-backend` `parseState`.
fn parse_mpd_audio(audio: &str) -> (Option<String>, Option<String>) {
    let parts: Vec<&str> = audio.split(':').collect();
    if parts.len() < 2 {
        return (None, None);
    }
    let sr = parts[0];
    let bd = parts[1];
    match sr {
        "dsd64" => return (Some("2.82 MHz".to_string()), Some("1 bit".to_string())),
        "dsd128" => return (Some("5.64 MHz".to_string()), Some("1 bit".to_string())),
        "dsd256" => return (Some("11.28 MHz".to_string()), Some("1 bit".to_string())),
        "dsd512" => return (Some("22.58 MHz".to_string()), Some("1 bit".to_string())),
        _ => {}
    }
    if let Ok(hz) = sr.parse::<f64>() {
        let khz = (hz / 1000.0 * 10.0).round() / 10.0;
        let sr_str = format!("{} kHz", khz);
        let bd_str = if bd == "f" {
            "32 bit".to_string()
        } else if bd == "dsd" {
            "1 bit".to_string()
        } else {
            bd.parse::<u32>()
                .map(|b| format!("{} bit", b))
                .unwrap_or_else(|_| format!("{} bit", bd))
        };
        return (Some(sr_str), Some(bd_str));
    }
    (None, None)
}

/// Node `miscellanea/albumart` `getAlbumArt`: when `data.artist` is set, the URL includes
/// `web=artist/album/extralarge` **in addition to** `path=` so `searchOnline` runs after local/embed fails.
fn push_state_albumart_url(
    volumio_uri: &str,
    artist: &Option<String>,
    album: &Option<String>,
) -> String {
    let mut url = format!(
        "/albumart?metadata=true&path={}",
        urlencoding::encode(volumio_uri)
    );
    if let Some(a) = artist {
        let a = a.trim();
        if !a.is_empty() {
            let web_inner = match album {
                Some(b) if !b.trim().is_empty() => {
                    format!("{}/{}/extralarge", a, b.trim())
                }
                _ => format!("{}//extralarge", a),
            };
            url.push_str("&web=");
            url.push_str(&urlencoding::encode(&web_inner));
        }
    }
    url
}

pub async fn get_state(client: &mut Client, music_root: &Path) -> Result<VolumioState> {
    let status = client.command(Status).await?;

    let seek_ms = status
        .elapsed
        .map(|d| d.as_millis() as u64);
    let duration_secs = status.duration.map(|d| d.as_secs());
    let position = status
        .current_song
        .map(|(pos, _)| pos.0 as u32);

    let audio_line = client
        .raw_command(RawCommand::new("status"))
        .await
        .ok()
        .and_then(|f| f.find("audio").map(std::string::ToString::to_string));
    let (samplerate, bitdepth) = audio_line
        .as_deref()
        .map(parse_mpd_audio)
        .unwrap_or((None, None));

    let bitrate = status
        .bitrate
        .map(|b| format!("{} Kbps", b));

    let (title, artist, album, uri, track_type, albumart) = if position.is_some() {
        match client.command(CurrentSong).await? {
            Some(song_in_queue) => {
                let s = &song_in_queue.song;
                let title = s.title().map(String::from);
                let artist = s.artists().first().map(String::from);
                let album = s.album().map(String::from);
                let volumio_uri = volumio_uri_from_mpd_url(&s.url, music_root);
                let uri = Some(volumio_uri.clone());
                let track_type = s.url.split('.').last().map(String::from);
                let albumart = Some(push_state_albumart_url(
                    &volumio_uri,
                    &artist,
                    &album,
                ));
                (title, artist, album, uri, track_type, albumart)
            }
            None => (None, None, None, None, None, None),
        }
    } else {
        (None, None, None, None, None, None)
    };

    let status_str = match status.state {
        PlayState::Playing => Some("play".to_string()),
        PlayState::Paused => Some("pause".to_string()),
        PlayState::Stopped => Some("stop".to_string()),
    };

    Ok(VolumioState {
        status: status_str,
        position,
        seek: seek_ms,
        duration: duration_secs,
        volume: Some(status.volume),
        repeat: Some(status.repeat),
        random: Some(status.random),
        title,
        artist,
        album,
        uri,
        track_type,
        service: Some("mpd".to_string()),
        albumart,
        samplerate,
        bitdepth,
        bitrate,
    })
}

/// Queue item for getQueue response.
#[derive(Debug, Serialize)]
pub struct QueueItem {
    pub position: u32,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub uri: Option<String>,
    pub duration: Option<u64>,
}

pub async fn get_queue(client: &mut Client) -> Result<Vec<QueueItem>> {
    let list = client.command(Queue::all()).await?;
    let items = list
        .into_iter()
        .map(|song_in_queue| {
            let s = &song_in_queue.song;
            let duration = s.duration.map(|d| d.as_secs());
            QueueItem {
                position: song_in_queue.position.0 as u32,
                title: s.title().map(String::from),
                artist: s.artists().first().map(String::from),
                album: s.album().map(String::from),
                uri: Some(s.url.clone()),
                duration,
            }
        })
        .collect();
    Ok(items)
}

/// Run a playback command (play, pause, stop, next, prev, clearQueue, volume, seek, repeat, random).
pub async fn run_command(
    client: &mut Client,
    cmd: &str,
    volume: Option<u8>,
    position: Option<i64>,
    repeat: Option<bool>,
    random: Option<bool>,
) -> Result<()> {
    use mpd_client::commands::{Song, SongPosition};

    match cmd {
        "play" => {
            if let Some(n) = position {
                client
                    .command(Play::song(Song::Position(SongPosition(n as usize))))
                    .await?;
            } else {
                client.command(Play::current()).await?;
            }
        }
        "pause" => {
            client.command(MpdPause(true)).await?;
        }
        "toggle" => {
            let status = client.command(Status).await?;
            let pause = status.state != PlayState::Paused;
            client.command(MpdPause(pause)).await?;
        }
        "stop" => {
            client.command(Stop).await?;
        }
        "next" => {
            client.command(Next).await?;
        }
        "prev" => {
            client.command(Previous).await?;
        }
        "clearQueue" => {
            client.command(ClearQueue).await?;
        }
        "volume" => {
            if let Some(v) = volume {
                client.command(SetVolume(v)).await?;
            }
        }
        "seek" => {
            if let Some(pos) = position {
                // Volumio2-UI `player.service.js` emits seek as whole seconds (see `set seek`).
                let d = Duration::from_secs(pos as u64);
                client.command(MpdSeekCmd(SeekMode::Absolute(d))).await?;
            }
        }
        "repeat" => {
            if let Some(r) = repeat {
                client.command(SetRepeat(r)).await?;
            }
        }
        "random" => {
            if let Some(r) = random {
                client.command(SetRandom(r)).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Seek backwards within current song (default 10 seconds). For skipBackwards.
pub async fn skip_backwards_connected(config: &MpdConfig, seconds: u64) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .command(MpdSeekCmd(SeekMode::Backward(Duration::from_secs(seconds))))
        .await?;
    Ok(())
}

/// Seek forward within current song (default 10 seconds). For skipForward.
pub async fn skip_forward_connected(config: &MpdConfig, seconds: u64) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .command(MpdSeekCmd(SeekMode::Forward(Duration::from_secs(seconds))))
        .await?;
    Ok(())
}
