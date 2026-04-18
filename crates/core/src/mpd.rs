//! MPD client: connect and run commands. Used by the v1 API to mirror Volumio backend behaviour.

use crate::albumart;
use crate::artist_normalize;
use crate::config::MUSIC_SOURCE_NAMES;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use urlencoding::decode;
use mpd_client::{
    commands::{
        Add, ClearQueue, CurrentSong, List as MpdListCmd, Move, Next, Play, Previous, Queue, Rescan,
        Seek as MpdSeekCmd, SeekMode, SetConsume, SetPause as MpdPause, SetRandom, SetRepeat,
        SetSingle, SetVolume, SingleMode, Song, SongPosition, Status, Stop, Update,
    },
    protocol::command::Command as RawCommand,
    responses::PlayState,
    tag::Tag,
    Client,
};
use std::io;
use serde::Serialize;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc::UnboundedSender;

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
///
/// **`master_volume_from_alsa`:** when `Some`, used as the **master fader** level for `pushState`
/// (same ALSA control as [`crate::alsa::get_system_volume_percent`]). When `None`, uses MPD
/// `status.volume` (e.g. mixer type **None** or ALSA read failed).
pub async fn get_state_connected(
    config: &MpdConfig,
    music_root: &Path,
    master_volume_from_alsa: Option<u8>,
) -> Result<VolumioState> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    get_state(&mut client, music_root, master_volume_from_alsa).await
}

/// Connect to MPD, run get_queue, then close.
pub async fn get_queue_connected(
    config: &MpdConfig,
    music_root: &std::path::Path,
) -> Result<Vec<QueueItem>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    get_queue(&mut client, music_root).await
}

/// One TCP session: full state then queue (e.g. single round-trip when callers need both).
#[allow(dead_code)]
pub async fn get_state_and_queue_connected(
    config: &MpdConfig,
    music_root: &Path,
    master_volume_from_alsa: Option<u8>,
) -> Result<(VolumioState, Vec<QueueItem>)> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    let s = get_state(&mut client, music_root, master_volume_from_alsa).await?;
    let q = get_queue(&mut client, music_root).await?;
    Ok((s, q))
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

/// Append several `music-library/...` URIs and start playback at the first appended song (Node `addPlay` after `explodeUri`).
pub async fn add_play_append_many_connected(config: &MpdConfig, uris: &[String]) -> Result<()> {
    if uris.is_empty() {
        return Ok(());
    }
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let start = client.command(Queue::all()).await?.len();
    for uri in uris {
        let path = volumio_uri_to_mpd_path(uri.trim());
        if path.is_empty() {
            continue;
        }
        client
            .raw_command(RawCommand::new("add").argument(path))
            .await?;
    }
    client
        .command(Play::song(Song::Position(SongPosition(start))))
        .await?;
    Ok(())
}

/// Insert multiple tracks after the current song (`add URI [POSITION]`). Used when `explodeUri` returns many rows.
pub async fn play_next_tracks_connected(config: &MpdConfig, uris: &[String]) -> Result<()> {
    if uris.is_empty() {
        return Ok(());
    }
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let status = client.command(Status).await?;
    let qlen = client.command(Queue::all()).await?.len();
    let insert_at = status
        .current_song
        .map(|(pos, _)| pos.0 + 1)
        .unwrap_or(qlen);
    let mut pos = insert_at;
    for uri in uris {
        let path = volumio_uri_to_mpd_path(uri.trim());
        if path.is_empty() {
            continue;
        }
        client
            .raw_command(RawCommand::new("add").argument(path).argument(pos.to_string()))
            .await?;
        pos += 1;
    }
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<String>,
}

/// Response for GET /api/v1/browse. Matches Volumio navigation structure.
#[derive(Debug, Clone, Serialize)]
pub struct BrowseResponse {
    pub navigation: BrowseNavigation,
}

/// Album / folder header when opening a drill-down browse (Node `navigation.info`, e.g. `listAlbumSongs`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseNavigationInfo {
    pub uri: String,
    pub service: &'static str,
    pub artist: String,
    pub album: String,
    pub albumart: String,
    #[serde(rename = "type")]
    pub browse_kind: &'static str,
    /// Total album duration like Node (`M:SS` or `MM:SS`).
    pub duration: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "trackType")]
    pub track_type: Option<String>,
}

/// Playlist / favourites header (`listFavourites`, `browsePlaylist` in Node).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowsePlaylistNavInfo {
    pub uri: String,
    pub title: String,
    pub name: String,
    pub service: &'static str,
    #[serde(rename = "type")]
    pub nav_type: &'static str,
    pub albumart: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum BrowseNavInfo {
    Album(BrowseNavigationInfo),
    Playlist(BrowsePlaylistNavInfo),
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowseNavigation {
    pub prev: BrowsePrev,
    pub lists: Vec<BrowseList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub info: Option<BrowseNavInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BrowsePrev {
    pub uri: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseList {
    pub available_list_views: Vec<&'static str>,
    pub items: Vec<BrowseItem>,
}

/// Grid layouts read `meta` for the subtitle; list views often use `artist`. Copy `artist` → `meta` when
/// `meta` is unset, except where a second line would duplicate the title.
///
/// **Browse → Albums** (`albums://<artist>/<album>`): `title` is the album name, so it can equal the artist
/// string (self-titled releases). Still set `meta` whenever `artist` is present — unlike genre → artist rows
/// where `title` and `artist` are the same field.
pub fn browse_response_fill_meta_from_artist(resp: &mut BrowseResponse) {
    for list in &mut resp.navigation.lists {
        for item in &mut list.items {
            if item.meta.is_some() {
                continue;
            }
            let Some(ref a) = item.artist else {
                continue;
            };
            if a.trim().is_empty() {
                continue;
            }
            if is_albums_artist_album_browse_uri(&item.uri) {
                item.meta = Some(a.clone());
                continue;
            }
            if a == &item.title {
                continue;
            }
            item.meta = Some(a.clone());
        }
    }
}

/// `albums://Artist/Album` rows from the tag library (Node `listAlbums`). Not flat `albums://Album` fallback.
fn is_albums_artist_album_browse_uri(uri: &str) -> bool {
    let Some(rest) = uri.strip_prefix("albums://") else {
        return false;
    };
    // Two segments: `encode(artist)/encode(album)`. Reject `albums:///Album` (empty first segment).
    rest.contains('/')
        && !rest.starts_with('/')
        && rest.splitn(2, '/').next().is_some_and(|first| !first.is_empty())
}

/// In-progress file entry while parsing lsinfo.
struct FileEntry {
    uri: String,
    title: String,
    artist: Option<String>,
    album: Option<String>,
    duration: Option<u64>,
    date: Option<String>,
    genre: Option<String>,
}

/// Aggregates tags across tracks when parsing an album drill-down (`find` + `lsinfo`).
#[derive(Default)]
struct AlbumDrillAgg {
    total_secs: u64,
    year: Option<String>,
    genre: Option<String>,
    first_rep_uri: Option<String>,
    last_ext: String,
}

fn year_from_mpd_date_tag(d: &str) -> String {
    let t = d.trim();
    if t.len() >= 4 && t[..4].chars().all(|c| c.is_ascii_digit()) {
        t[..4].to_string()
    } else {
        t.to_string()
    }
}

/// Node `listAlbumSongs` duration string: total seconds as `M:SS` / `MM:SS`.
fn format_album_total_duration_node_style(total_secs: u64) -> String {
    let m = total_secs / 60;
    let s = total_secs % 60;
    format!("{}:{:02}", m, s)
}

fn flush_file_item(
    current: &mut Option<FileEntry>,
    items: &mut Vec<BrowseItem>,
    agg: Option<&mut AlbumDrillAgg>,
) {
    if let Some(f) = current.take() {
        if let Some(a) = agg {
            a.total_secs += f.duration.unwrap_or(0);
            if a.first_rep_uri.is_none() {
                a.first_rep_uri = Some(f.uri.clone());
            }
            if let Some(ref d) = f.date {
                a.year = Some(year_from_mpd_date_tag(d));
            }
            if let Some(ref g) = f.genre {
                a.genre = Some(g.clone());
            }
            if let Some(ext) = codec_track_type_from_song_url(&f.uri) {
                a.last_ext = ext;
            }
        }
        let albumart = Some(volumio_albumart_url(
            &f.uri,
            &f.artist,
            &f.album,
            true,
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
            meta: None,
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
/// `music_root` resolves folder cover files. Rows without `folder.jpg` get `/albumart?metadata=true&path=…`
/// plus heuristic `web=` (artist/album from path: INTERNAL/Artist/Album, USB/…/Artist/Album, or leaf as artist)
/// so online providers can fill thumbnails; then `icon=folder-o` if all else fails.
fn parse_lsinfo_frame(
    frame: mpd_client::protocol::response::Frame,
    browse_uri: &str,
    music_root: &Path,
    drill: &mut AlbumDrillAgg,
    aggregate: bool,
) -> Vec<BrowseItem> {
    let mut items = Vec::new();
    let mut current_file: Option<FileEntry> = None;

    for (key, value) in frame.fields() {
        match key {
            "directory" => {
                flush_file_item(
                    &mut current_file,
                    &mut items,
                    if aggregate { Some(drill) } else { None },
                );
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
                            "/albumart?metadata=true&path={}",
                            urlencoding::encode(&item_uri)
                        )),
                        None,
                    )
                } else if item_type == "remdisk" {
                    (None, Some("fa fa-usb".to_string()))
                } else {
                    (
                        Some(browse_directory_albumart_url(
                            &item_uri,
                            value,
                            browse_uri,
                        )),
                        None,
                    )
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
                    meta: None,
                });
            }
            "file" => {
                flush_file_item(
                    &mut current_file,
                    &mut items,
                    if aggregate { Some(drill) } else { None },
                );
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
                    date: None,
                    genre: None,
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
            "Date" => {
                if let Some(ref mut f) = current_file {
                    f.date = Some(value.to_string());
                }
            }
            "Genre" => {
                if let Some(ref mut f) = current_file {
                    f.genre = Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    flush_file_item(
        &mut current_file,
        &mut items,
        if aggregate { Some(drill) } else { None },
    );

    items
}

/// Collection stats for “My Music” (same shape as Node `pushMyCollectionStats`).
///
/// **Must** use the same MPD queries as `volumio3-backend` `getMyCollectionStats`: `count group artist`
/// and `list album group albumartist`. Those reflect **partial** database contents during
/// `update` / `rescan`. The `stats` command reports **final** totals and typically does not move until
/// the scan completes, so the UI would look frozen while polling.
#[derive(Debug, Default, Serialize)]
pub struct CollectionStats {
    pub artists: u64,
    pub albums: u64,
    pub songs: u64,
    pub playtime: String,
}

fn playtime_string_from_seconds_total(mut secs: u64) -> String {
    let h = secs / 3600;
    secs %= 3600;
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{}:{}", h, m, s)
}

pub async fn collection_stats_connected(config: &MpdConfig) -> Result<CollectionStats> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    collection_stats_with_client(&mut client).await
}

async fn collection_stats_with_client(client: &mut Client) -> Result<CollectionStats> {
    let count_frame = match client
        .raw_command(
            RawCommand::new("count")
                .argument("group")
                .argument("artist"),
        )
        .await
    {
        Ok(f) => f,
        Err(_) => {
            return Ok(CollectionStats {
                artists: 0,
                albums: 0,
                songs: 0,
                playtime: "0:0:0".to_string(),
            });
        }
    };

    let mut artists = 0u64;
    let mut songs = 0u64;
    let mut playtime_secs = 0u64;
    for (k, v) in count_frame.fields() {
        if k.eq_ignore_ascii_case("artist") {
            artists += 1;
        } else if k.eq_ignore_ascii_case("songs") {
            songs += v.parse::<u64>().unwrap_or(0);
        } else if k.eq_ignore_ascii_case("playtime") {
            playtime_secs += v.parse::<u64>().unwrap_or(0);
        }
    }

    let albums = match client
        .raw_command(
            RawCommand::new("list")
                .argument("album")
                .argument("group")
                .argument("albumartist"),
        )
        .await
    {
        Ok(frame) => frame
            .fields()
            .filter(|(k, _)| k.eq_ignore_ascii_case("album"))
            .count() as u64,
        Err(_) => 0,
    };

    Ok(CollectionStats {
        artists,
        albums,
        songs,
        playtime: playtime_string_from_seconds_total(playtime_secs),
    })
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

/// Socket.IO `setRepeat`: MPD **`repeat`** (queue) + **`single`** (repeat one track). When repeat is off,
/// single is cleared (matches Node state machine).
pub async fn set_repeat_modes_connected(
    config: &MpdConfig,
    repeat: bool,
    repeat_single: bool,
) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client.command(SetRepeat(repeat)).await?;
    let single = if !repeat {
        SingleMode::Disabled
    } else if repeat_single {
        SingleMode::Enabled
    } else {
        SingleMode::Disabled
    };
    client.command(SetSingle(single)).await?;
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

/// One line from a JSON playlist / favourites file → browse row (Node uses `type: playlist` for list root;
/// lines use `song`, `webradio`, or tag URIs as `folder`).
pub fn browse_item_from_playlist_entry(e: &crate::playlist_library::PlaylistEntry) -> BrowseItem {
    let uri = crate::playlist_library::normalize_volumio_uri(&e.uri);
    let svc = if e.service.is_empty() {
        "mpd".to_string()
    } else {
        e.service.clone()
    };

    if svc == "webradio" || uri.starts_with("http://") || uri.starts_with("https://") {
        let title = e.title.clone().unwrap_or_else(|| uri.clone());
        return BrowseItem {
            item_type: "webradio".to_string(),
            title,
            uri,
            service: "webradio".to_string(),
            artist: e.artist.clone(),
            album: e.album.clone(),
            duration: None,
            albumart: e.albumart.clone(),
            icon: e.icon.clone(),
            meta: None,
        };
    }

    if uri.starts_with("albums://") || uri.starts_with("artists://") || uri.starts_with("genres://") {
        return BrowseItem {
            item_type: "folder".to_string(),
            title: tag_uri_display_title(&uri),
            uri,
            service: "mpd".to_string(),
            artist: None,
            album: None,
            duration: None,
            albumart: e.albumart.clone(),
            icon: None,
            meta: None,
        };
    }

    let title = e
        .title
        .clone()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| leaf_from_volumio_uri(&uri));
    BrowseItem {
        item_type: "song".to_string(),
        title,
        uri,
        service: svc,
        artist: e.artist.clone(),
        album: e.album.clone(),
        duration: None,
        albumart: e.albumart.clone(),
        icon: e.icon.clone(),
        meta: None,
    }
}

fn leaf_from_volumio_uri(u: &str) -> String {
    u.rsplit('/').next().unwrap_or(u).to_string()
}

/// Human-readable label for `albums://` / `artists://` / `genres://` rows in playlists.
fn tag_uri_display_title(uri: &str) -> String {
    if let Some(rest) = uri.strip_prefix("albums://") {
        let dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        if let Some((a, b)) = dec.split_once('/') {
            let artist = a.trim();
            let album = b.trim();
            if !album.is_empty() {
                return format!("{} — {}", artist, album);
            }
        }
        return dec;
    }
    if let Some(rest) = uri.strip_prefix("artists://") {
        let dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        return dec.trim_matches('/').to_string();
    }
    if let Some(rest) = uri.strip_prefix("genres://") {
        let dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        return dec.trim_matches('/').to_string();
    }
    leaf_from_volumio_uri(uri)
}

/// Fill title/artist/album/albumart from MPD tags for `music-library/...` **song** rows (playlist / favourites).
pub async fn enrich_playlist_browse_items_from_mpd(
    config: &MpdConfig,
    music_root: &Path,
    items: &mut [BrowseItem],
) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    for item in items.iter_mut() {
        if item.item_type != "song" {
            continue;
        }
        if !item.uri.starts_with("music-library/") {
            continue;
        }
        let path = volumio_uri_to_mpd_path(&item.uri);
        if path.is_empty() {
            continue;
        }
        let frame = match client
            .raw_command(RawCommand::new("lsinfo").argument(path))
            .await
        {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut drill = AlbumDrillAgg::default();
        let parsed = parse_lsinfo_frame(frame, "music-library", music_root, &mut drill, false);
        if let Some(song) = parsed.into_iter().find(|i| i.item_type == "song") {
            *item = song;
        }
    }
    Ok(())
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

/// Play a playlist by name: library favourites / JSON playlists under `settings/playlist/`, else MPD `load`.
pub async fn play_playlist_by_name(config: &MpdConfig, name: &str) -> Result<()> {
    if let Some(entries) = crate::playlist_library::load_entries_for_play(name) {
        let uris = crate::playlist_library::entries_to_play_uris(&entries);
        if uris.is_empty() {
            return Ok(());
        }
        return play_items_list_connected(config, &uris, 0).await;
    }
    load_playlist_connected(config, name).await
}

/// Append playlist to queue: resolved JSON / favourites, else MPD `load` (append).
pub async fn enqueue_playlist_by_name(config: &MpdConfig, name: &str) -> Result<()> {
    if let Some(entries) = crate::playlist_library::load_entries_for_play(name) {
        let uris = crate::playlist_library::entries_to_play_uris(&entries);
        if uris.is_empty() {
            return Ok(());
        }
        return add_multiple_to_queue_connected(config, &uris).await;
    }
    enqueue_playlist_connected(config, name).await
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

/// Create empty stored playlist (clear queue, save as name). Unused when JSON playlists are enabled.
#[allow(dead_code)]
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

/// Like [`add_to_playlist_connected`], but expands virtual browse URIs (`albums://`, `artists://`, `genres://`)
/// via [`resolve_uri_for_queue`] and runs **`playlistadd`** once per concrete file (MPD cannot add virtual URIs).
pub async fn add_to_playlist_resolved(
    config: &MpdConfig,
    music_root: &Path,
    playlist_name: &str,
    uri: &str,
) -> Result<()> {
    let paths = resolve_uri_for_queue(config, music_root, uri).await?;
    if paths.is_empty() {
        anyhow::bail!("no playable files resolved for playlist add");
    }
    if paths.len() == 1 {
        return add_to_playlist_connected(config, playlist_name, &paths[0]).await;
    }
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    for p in paths {
        let path = volumio_uri_to_mpd_path(&p);
        if path.is_empty() {
            continue;
        }
        client
            .raw_command(
                RawCommand::new("playlistadd")
                    .argument(playlist_name)
                    .argument(path),
            )
            .await?;
    }
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
                info: None,
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
    let mut drill = AlbumDrillAgg::default();
    let items = parse_lsinfo_frame(frame, "music-library", music_root, &mut drill, false);
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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

/// Top-level storage roots: same as Node `getAlbumArt('', '', 'microchip'|'server'|'usb')` → `/albumart?icon=…`.
fn music_source_albumart(path_segment: &str) -> &'static str {
    match path_segment {
        "INTERNAL" => "/albumart?icon=microchip",
        "NAS" => "/albumart?icon=server",
        "USB" => "/albumart?icon=usb",
        // No `smb.svg` in miscellanea/albumart; treat like network attach.
        "SMB" => "/albumart?icon=server",
        _ => "/albumart?icon=microchip",
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
            meta: None,
        })
        .collect();
    BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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
            info: None,
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

/// Browse `uri=favourites` — JSON under `settings/favourites/favourites` (Node `/data/favourites/favourites`).
pub fn browse_favourites_response() -> BrowseResponse {
    let entries = crate::playlist_library::load_favourites();
    let items: Vec<BrowseItem> = entries
        .into_iter()
        .map(|e| {
            let title = e
                .title
                .clone()
                .unwrap_or_else(|| e.uri.clone());
            let uri = crate::playlist_library::normalize_volumio_uri(&e.uri);
            BrowseItem {
                item_type: "song".to_string(),
                title,
                uri,
                service: e.service,
                artist: e.artist,
                album: e.album,
                duration: None,
                albumart: e.albumart,
                icon: e.icon,
                meta: None,
            }
        })
        .collect();

    BrowseResponse {
        navigation: BrowseNavigation {
            info: Some(BrowseNavInfo::Playlist(BrowsePlaylistNavInfo {
                uri: "playlists/favourites".to_string(),
                title: "Favourites".to_string(),
                name: "favourites".to_string(),
                service: "mpd",
                nav_type: "play-playlist",
                albumart: "/albumart?sourceicon=music_service/mpd/favouritesicon.png".to_string(),
            })),
            prev: BrowsePrev {
                uri: "music-library".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
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

/// Stable key to merge **Album artist** / **Artist** spellings that differ only by case or spacing (Node uses
/// `list albumartist` for browse; we group so "b mars" / "B Mars" collapse when MPD lists both).
fn artist_browse_normalize_key(s: &str) -> String {
    let t = s.trim();
    t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn is_heavy_uppercase(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return false;
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    upper * 3 >= letters.len() * 2
}

/// Pick a single display string for browse (prefer not ALL‑CAPS; then shortest).
fn pick_canonical_artist_title(candidates: &[String]) -> String {
    if candidates.len() == 1 {
        return candidates[0].clone();
    }
    let has_mixed = candidates.iter().any(|s| !is_heavy_uppercase(s));
    let pool: Vec<&String> = if has_mixed {
        candidates.iter().filter(|s| !is_heavy_uppercase(s)).collect()
    } else {
        candidates.iter().collect()
    };
    pool.iter()
        .min_by_key(|s| (s.len(), s.as_str()))
        .expect("non-empty pool")
        .to_string()
}

/// Deduplicate raw MPD tag values by [`artist_browse_normalize_key`]; returns `(display_title, uri_token)`.
fn group_artist_names_for_browse(raw: Vec<String>) -> Vec<(String, String)> {
    let mut buckets: HashMap<String, Vec<String>> = HashMap::new();
    for r in raw {
        let k = artist_browse_normalize_key(&r);
        if k.is_empty() {
            continue;
        }
        buckets.entry(k).or_default().push(r);
    }
    let mut out: Vec<(String, String)> = buckets
        .into_values()
        .filter_map(|mut group| {
            if group.is_empty() {
                return None;
            }
            group.sort();
            let title = pick_canonical_artist_title(&group);
            // URI uses canonical title so links stay readable; drill-down resolves all spellings via normalize.
            Some((title.clone(), title))
        })
        .collect();
    out.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    out
}

/// All spellings of the same logical artist (same normalize key) for union album queries, and whether
/// the library uses **AlbumArtist** tags (Node `artistsort` / non-empty `list albumartist`).
async fn expand_artist_spelling_variants(
    config: &MpdConfig,
    artist_decoded: &str,
) -> Result<(Vec<String>, bool)> {
    let aa = list_tag_values(config, "AlbumArtist").await?;
    let (pool, use_aa) = if !aa.is_empty() {
        (aa, true)
    } else {
        (list_tag_values(config, "Artist").await?, false)
    };
    let key = artist_browse_normalize_key(artist_decoded);
    let mut variants: Vec<String> = pool
        .into_iter()
        .filter(|s| artist_browse_normalize_key(s) == key)
        .collect();
    if variants.is_empty() {
        variants.push(artist_decoded.to_string());
    }
    variants.sort();
    variants.dedup();
    Ok((variants, use_aa))
}

/// `list Album` constrained by AlbumArtist or Artist tag (exact MPD value).
async fn list_albums_for_artist_tag(
    config: &MpdConfig,
    tag: &str,
    exact_value: &str,
) -> Result<Vec<String>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("list")
        .argument("Album")
        .argument(tag)
        .argument(exact_value);
    let frame = client.raw_command(raw).await?;
    let mut albums: Vec<String> = frame
        .fields()
        .filter_map(|(k, v)| if k == "Album" { Some(v.to_string()) } else { None })
        .collect();
    albums.sort();
    albums.dedup();
    Ok(albums)
}

#[derive(Default)]
struct PendingAlbumSongTags {
    /// MPD `file:` value for this block (representative path for album-art `path=`, like Node `getParentFolder`).
    file_path: Option<String>,
    album: Option<String>,
    albumartist: Option<String>,
    artist: Option<String>,
}

/// Node `listAlbums`: `search album ""`, then one browse row per distinct `albumName + artistName` where
/// `artistName` is **AlbumArtist** if set, else **Artist** (per track). This avoids wrong subtitles when
/// `list album group albumartist` collapses homonym album titles to an arbitrary `groups[0]`.
fn parse_node_style_album_rows_from_frame(
    frame: &mpd_client::protocol::response::Frame,
) -> Vec<(String, String, Option<String>)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut rows: Vec<(String, String, Option<String>)> = Vec::new();

    let mut flush = |p: PendingAlbumSongTags| {
        let album_name = p.album.unwrap_or_default();
        // Node `listAlbums`: missing tags → `''`, not `*` (`*` is only for orphaned tracks with no album).
        let artist_name = p
            .albumartist
            .or(p.artist)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_default();
        if album_name.trim().is_empty() {
            return;
        }
        let album_id = format!("{}{}", album_name, artist_name);
        if seen.insert(album_id) {
            let rep = p.file_path.as_ref().map(|fp| {
                format!("music-library/{}", fp.trim_start_matches('/'))
            });
            rows.push((album_name, artist_name, rep));
        }
    };

    let mut current: Option<PendingAlbumSongTags> = None;
    for (key, value) in frame.fields() {
        match key.to_lowercase().as_str() {
            "file" => {
                if let Some(p) = current.take() {
                    flush(p);
                }
                current = Some(PendingAlbumSongTags {
                    file_path: Some(value.to_string()),
                    ..Default::default()
                });
            }
            "album" => {
                if let Some(ref mut c) = current {
                    c.album = Some(value.to_string());
                }
            }
            "albumartist" => {
                if let Some(ref mut c) = current {
                    c.albumartist = Some(value.to_string());
                }
            }
            "artist" => {
                if let Some(ref mut c) = current {
                    if c.artist.is_none() {
                        c.artist = Some(value.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(p) = current {
        flush(p);
    }
    rows
}

/// Same source as Node `listAlbums` (`search album ""`). Third tuple element is a representative
/// `music-library/...` file URI for `/albumart?path=` (first track per album), matching Node.
async fn list_albums_via_search_album_empty(
    config: &MpdConfig,
) -> Result<Vec<(String, String, Option<String>)>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("search")
        .argument("album")
        .argument("");
    let frame = client.raw_command(raw).await?;
    Ok(parse_node_style_album_rows_from_frame(&frame))
}

/// Fallback when [`list_albums_via_search_album_empty`] is empty (some MPD builds/configs return no rows).
/// Matches pre-search Evo + MPD **`list album group albumartist`** / **`list album group artist`** —
/// no per-album `path=` (third tuple `None`); `web=` still gets artist/album for online art.
async fn list_album_artist_pairs_via_list_group(
    config: &MpdConfig,
) -> Result<Vec<(String, String, Option<String>)>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut rows: Vec<(String, String, Option<String>)> = Vec::new();

    let mut push_pair = |album: &str, artist: &str| {
        let ar = artist.trim();
        let al = album.trim();
        if al.is_empty() || ar.is_empty() {
            return;
        }
        let key = (al.to_string(), ar.to_string());
        if seen.insert(key.clone()) {
            rows.push((key.0, key.1, None));
        }
    };

    if let Ok(list) = client
        .command(MpdListCmd::new(Tag::Album).group_by([Tag::AlbumArtist]))
        .await
    {
        for (album, groups) in list.grouped_values() {
            if let Some(g0) = groups.first() {
                push_pair(album, g0);
            }
        }
    }
    if let Ok(list) = client
        .command(MpdListCmd::new(Tag::Album).group_by([Tag::Artist]))
        .await
    {
        for (album, groups) in list.grouped_values() {
            if let Some(g0) = groups.first() {
                push_pair(album, g0);
            }
        }
    }

    rows.sort_by(|x, y| {
        x.0.to_lowercase()
            .cmp(&y.0.to_lowercase())
            .then_with(|| x.1.to_lowercase().cmp(&y.1.to_lowercase()))
    });
    Ok(rows)
}

/// All artists (`artists://` root). Matches Node default `artistsort`: **`list albumartist`**, not `list artist`.
/// Falls back to **`list artist`** if no AlbumArtist tags exist. Merges case/whitespace variants for browse.
async fn browse_all_artists_connected(config: &MpdConfig) -> Result<BrowseResponse> {
    let raw = list_tag_values(config, "AlbumArtist").await?;
    let grouped = if !raw.is_empty() {
        group_artist_names_for_browse(raw)
    } else {
        group_artist_names_for_browse(list_tag_values(config, "Artist").await?)
    };
    // Node `listArtists`: each item has `title`, `albumart`, `uri` only — no `artist` (avoids duplicate
    // title/subtitle lines on Browse → Artists).
    let items: Vec<BrowseItem> = grouped
        .into_iter()
        .map(|(title, uri_token)| BrowseItem {
            item_type: "folder".to_string(),
            title: artist_normalize::normalize_for_artist_tile_title(&title),
            uri: format!("artists://{}", urlencoding::encode(uri_token.as_str())),
            service: "mpd".to_string(),
            artist: None,
            album: None,
            duration: None,
            albumart: Some(browse_artist_list_albumart_url(uri_token.as_str())),
            icon: None,
            meta: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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

/// All distinct albums (`albums://` root). Rows are `albums://artist/album` (like Node). Set `artist` from
/// tags; [`browse_response_fill_meta_from_artist`] adds `meta` for grid layouts (including self-titled albums).
///
/// **Order:** Node `search album ""` (with `path=` when possible) → MPD `list album group …` → last resort
/// flat `list Album` (no per-album artist; synthetic Various Artists art only).
async fn browse_all_albums_connected(config: &MpdConfig) -> Result<BrowseResponse> {
    let mut pairs = list_albums_via_search_album_empty(config).await.unwrap_or_else(|e| {
        tracing::warn!("{} albums browse: search album \"\" failed: {}", crate::log_tags::EVO_BROWSE, e);
        Vec::new()
    });
    if pairs.is_empty() {
        match list_album_artist_pairs_via_list_group(config).await {
            Ok(p) if !p.is_empty() => {
                tracing::info!(
                    "{} albums browse: using list album group fallback ({} rows)",
                    crate::log_tags::EVO_BROWSE,
                    p.len()
                );
                pairs = p;
            }
            Ok(_) => tracing::warn!(
                "{} albums browse: list album group returned empty; trying flat Album list",
                crate::log_tags::EVO_BROWSE
            ),
            Err(e) => tracing::warn!(
                "{} albums browse: list album group failed: {}",
                crate::log_tags::EVO_BROWSE,
                e
            ),
        }
    }
    let items: Vec<BrowseItem> = if !pairs.is_empty() {
        pairs
            .into_iter()
            .map(|(album, artist, rep_path)| {
                let artist_for_item = (!artist.is_empty()).then(|| artist.clone());
                BrowseItem {
                    item_type: "folder".to_string(),
                    title: album.clone(),
                    uri: format!(
                        "albums://{}/{}",
                        urlencoding::encode(artist.as_str()),
                        urlencoding::encode(album.as_str())
                    ),
                    service: "mpd".to_string(),
                    artist: artist_for_item,
                    album: None,
                    duration: None,
                    albumart: Some(browse_album_list_albumart_url(
                        rep_path.as_deref(),
                        artist.as_str(),
                        album.as_str(),
                    )),
                    icon: None,
                    meta: None,
                }
            })
            .collect()
    } else {
        let albums = list_tag_values(config, "Album").await?;
        albums
            .into_iter()
            .map(|album| BrowseItem {
                item_type: "folder".to_string(),
                title: album.clone(),
                uri: format!("albums://{}", urlencoding::encode(album.as_str())),
                service: "mpd".to_string(),
                artist: None,
                album: Some(album.clone()),
                duration: None,
                albumart: Some(browse_virtual_albumart_url_with_icon(
                    Some("Various Artists"),
                    Some(album.as_str()),
                    "dot-circle-o",
                )),
                icon: None,
                meta: None,
            })
            .collect()
    };
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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
    let mut drill = AlbumDrillAgg::default();
    let items = parse_lsinfo_frame(frame, "music-library", music_root, &mut drill, false);
    let prev = "albums://".to_string();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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
            albumart: Some(browse_virtual_folder_albumart_url(None, None)),
            icon: None,
            meta: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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

/// Artists under a genre (`genres://Rock`). Node uses **albumartist** when `artistsort` (default): `list AlbumArtist Genre "Rock"`.
async fn browse_genre_connected(config: &MpdConfig, genre: &str) -> Result<BrowseResponse> {
    let use_aa = !list_tag_values(config, "AlbumArtist").await?.is_empty();
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = if use_aa {
        RawCommand::new("list")
            .argument("AlbumArtist")
            .argument("Genre")
            .argument(genre)
    } else {
        RawCommand::new("list")
            .argument("Artist")
            .argument("Genre")
            .argument(genre)
    };
    let frame = client.raw_command(raw).await?;
    let tag = if use_aa { "AlbumArtist" } else { "Artist" };
    let mut artists: Vec<String> = frame
        .fields()
        .filter_map(|(k, v)| if k == tag { Some(v.to_string()) } else { None })
        .collect();
    artists.sort();
    artists.dedup();
    let grouped = group_artist_names_for_browse(artists);
    let items: Vec<BrowseItem> = grouped
        .into_iter()
        .map(|(title, uri_token)| BrowseItem {
            item_type: "folder".to_string(),
            title: title.clone(),
            uri: format!("artists://{}", urlencoding::encode(uri_token.as_str())),
            service: "mpd".to_string(),
            artist: Some(title),
            album: None,
            duration: None,
            albumart: Some(browse_artist_list_albumart_url(uri_token.as_str())),
            icon: None,
            meta: None,
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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

/// List albums by artist. Node `artistsort`: **`list Album AlbumArtist <value>`** (exact tag spellings).
/// Unions albums across all spelling variants that share [`artist_browse_normalize_key`] with `artist`.
async fn browse_artist_connected(config: &MpdConfig, artist: &str) -> Result<BrowseResponse> {
    let (variants, use_aa) = expand_artist_spelling_variants(config, artist).await?;
    let tag = if use_aa { "AlbumArtist" } else { "Artist" };
    let mut album_set: HashSet<String> = HashSet::new();
    for v in &variants {
        for a in list_albums_for_artist_tag(config, tag, v).await? {
            if !a.is_empty() {
                album_set.insert(a);
            }
        }
    }
    let mut albums: Vec<String> = album_set.into_iter().collect();
    albums.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    let items: Vec<BrowseItem> = albums
        .into_iter()
        .map(|album| BrowseItem {
            item_type: "folder".to_string(),
            title: album.clone(),
            uri: format!(
                "albums://{}/{}",
                urlencoding::encode(artist),
                urlencoding::encode(album.as_str())
            ),
            service: "mpd".to_string(),
            artist: Some(artist.to_string()),
            album: Some(album.clone()),
            duration: None,
            albumart: Some(browse_virtual_folder_albumart_url(
                Some(artist),
                Some(album.as_str()),
            )),
            icon: None,
            meta: Some(artist.to_string()),
        })
        .collect();
    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
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

/// List songs in an album from `albums://artist/album`. Matches Node: `find` by Album + AlbumArtist,
/// then Album + Artist if the library has no AlbumArtist tags. Emits `navigation.info` like Node
/// `listAlbumSongs` (album header: art, duration, year, genre) and **`list`** view only for tracks.
async fn browse_album_songs_connected(
    config: &MpdConfig,
    music_root: &Path,
    artist: &str,
    album: &str,
) -> Result<BrowseResponse> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let mut agg = AlbumDrillAgg::default();

    let raw_aa = RawCommand::new("find")
        .argument("Album")
        .argument(album)
        .argument("AlbumArtist")
        .argument(artist);
    let frame = client.raw_command(raw_aa).await?;
    let mut items = parse_lsinfo_frame(frame, "albums://find-tracks", music_root, &mut agg, true);
    if items.is_empty() {
        agg = AlbumDrillAgg::default();
        let raw_ar = RawCommand::new("find")
            .argument("Album")
            .argument(album)
            .argument("Artist")
            .argument(artist);
        let frame2 = client.raw_command(raw_ar).await?;
        items = parse_lsinfo_frame(frame2, "albums://find-tracks", music_root, &mut agg, true);
    }
    // Missing artist tags are not always the same as empty-string tags in MPD.
    if items.is_empty() && artist.is_empty() {
        agg = AlbumDrillAgg::default();
        let raw_album_only = RawCommand::new("find").argument("Album").argument(album);
        let frame3 = client.raw_command(raw_album_only).await?;
        items = parse_lsinfo_frame(frame3, "albums://find-tracks", music_root, &mut agg, true);
    }

    let browse_uri = format!(
        "albums://{}/{}",
        urlencoding::encode(artist),
        urlencoding::encode(album)
    );

    let info = if items.is_empty() {
        None
    } else {
        let albumart = browse_album_list_albumart_url(agg.first_rep_uri.as_deref(), artist, album);
        Some(BrowseNavInfo::Album(BrowseNavigationInfo {
            uri: browse_uri,
            service: "mpd",
            artist: artist.to_string(),
            album: album.to_string(),
            albumart,
            browse_kind: "album",
            duration: format_album_total_duration_node_style(agg.total_secs),
            year: agg.year,
            genre: agg.genre,
            track_type: if agg.last_ext.is_empty() {
                None
            } else {
                Some(agg.last_ext)
            },
        }))
    };

    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info,
            prev: BrowsePrev {
                uri: "albums://".to_string(),
            },
            lists: vec![BrowseList {
                available_list_views: vec!["list"],
                items,
            }],
        },
    })
}

fn song_uris_from_browse(resp: &BrowseResponse) -> Vec<String> {
    resp.navigation
        .lists
        .iter()
        .flat_map(|l| &l.items)
        .filter(|i| i.item_type == "song")
        .map(|i| i.uri.clone())
        .collect()
}

/// All tracks for an artist tag (Node `explodeUri` `artists://…`: `find Artist`, then `find AlbumArtist` if empty).
async fn find_artist_all_tracks(
    config: &MpdConfig,
    music_root: &Path,
    artist: &str,
) -> Result<Vec<String>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("find")
        .argument("Artist")
        .argument(artist);
    let frame = client.raw_command(raw).await?;
    let mut drill = AlbumDrillAgg::default();
    let items = parse_lsinfo_frame(frame, "artists://explode", music_root, &mut drill, false);
    let mut uris: Vec<String> = items
        .into_iter()
        .filter(|i| i.item_type == "song")
        .map(|i| i.uri)
        .collect();
    if uris.is_empty() {
        let raw2 = RawCommand::new("find")
            .argument("AlbumArtist")
            .argument(artist);
        let frame2 = client.raw_command(raw2).await?;
        let mut drill2 = AlbumDrillAgg::default();
        let items2 = parse_lsinfo_frame(frame2, "artists://explode", music_root, &mut drill2, false);
        uris = items2
            .into_iter()
            .filter(|i| i.item_type == "song")
            .map(|i| i.uri)
            .collect();
    }
    Ok(uris)
}

/// All tracks with this genre tag (`find Genre`). Used when resolving `genres://…` for queue / playlist add.
async fn find_genre_all_tracks(
    config: &MpdConfig,
    music_root: &Path,
    genre: &str,
) -> Result<Vec<String>> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    let raw = RawCommand::new("find")
        .argument("Genre")
        .argument(genre);
    let frame = client.raw_command(raw).await?;
    let mut drill = AlbumDrillAgg::default();
    let items = parse_lsinfo_frame(frame, "genres://explode", music_root, &mut drill, false);
    Ok(items
        .into_iter()
        .filter(|i| i.item_type == "song")
        .map(|i| i.uri)
        .collect())
}

/// Node `music_service` `explodeUri`: turn virtual browse URIs into concrete `music-library/...` paths. Pass-through otherwise.
pub async fn resolve_uri_for_queue(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<Vec<String>> {
    let uri = uri.trim();
    if uri.starts_with("genres://") {
        let rest = uri.strip_prefix("genres://").unwrap_or("");
        let rest_dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        let genre = rest_dec.trim();
        if genre.is_empty() {
            return Ok(vec![]);
        }
        return find_genre_all_tracks(config, music_root, genre).await;
    }
    if uri.starts_with("albums://") {
        let rest = uri.strip_prefix("albums://").unwrap_or("");
        let rest_dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        if let Some((a, b)) = rest_dec.split_once('/') {
            let artist = a.trim();
            let album = b.trim();
            if !album.is_empty() {
                let resp = browse_album_songs_connected(config, music_root, artist, album).await?;
                return Ok(song_uris_from_browse(&resp));
            }
        }
        let album = rest_dec.trim();
        if album.is_empty() {
            return Ok(vec![]);
        }
        let resp = browse_album_only_songs_connected(config, music_root, album).await?;
        return Ok(song_uris_from_browse(&resp));
    }
    if uri.starts_with("artists://") {
        let rest = uri.strip_prefix("artists://").unwrap_or("");
        let rest_dec = decode(rest)
            .map(|c| c.into_owned())
            .unwrap_or_else(|_| rest.to_string());
        if let Some((a, b)) = rest_dec.split_once('/') {
            let artist = a.trim();
            let album = b.trim();
            if !artist.is_empty() && !album.is_empty() {
                let resp = browse_album_songs_connected(config, music_root, artist, album).await?;
                return Ok(song_uris_from_browse(&resp));
            }
        }
        let artist = rest_dec.trim();
        if artist.is_empty() {
            return Ok(vec![]);
        }
        return find_artist_all_tracks(config, music_root, artist).await;
    }
    Ok(vec![uri.to_string()])
}

pub async fn replace_and_play_resolved(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<()> {
    let paths = resolve_uri_for_queue(config, music_root, uri).await?;
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() == 1 {
        add_play_connected(config, &paths[0]).await
    } else {
        play_items_list_connected(config, &paths, 0).await
    }
}

pub async fn add_play_append_resolved(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<()> {
    let paths = resolve_uri_for_queue(config, music_root, uri).await?;
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() == 1 {
        add_play_append_connected(config, &paths[0]).await
    } else {
        add_play_append_many_connected(config, &paths).await
    }
}

pub async fn add_to_queue_resolved(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<()> {
    let paths = resolve_uri_for_queue(config, music_root, uri).await?;
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() == 1 {
        add_to_queue_connected(config, &paths[0]).await
    } else {
        add_multiple_to_queue_connected(config, &paths).await
    }
}

pub async fn play_next_resolved(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<()> {
    let paths = resolve_uri_for_queue(config, music_root, uri).await?;
    if paths.is_empty() {
        return Ok(());
    }
    if paths.len() == 1 {
        play_next_connected(config, &paths[0]).await
    } else {
        play_next_tracks_connected(config, &paths).await
    }
}

/// Connect to MPD, run lsinfo for the given Volumio uri (e.g. "music-library/INTERNAL/..."), return browse response.
/// Handles virtual URIs: `artists://`, `albums://`, `genres://` (tag-based library, like classic Volumio).
pub async fn browse_connected(
    config: &MpdConfig,
    music_root: &Path,
    uri: &str,
) -> Result<BrowseResponse> {
    if uri == "favourites" {
        let mut resp = browse_favourites_response();
        for list in &mut resp.navigation.lists {
            enrich_playlist_browse_items_from_mpd(config, music_root, &mut list.items).await?;
        }
        return Ok(resp);
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
            if !album.is_empty() {
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

    let mut drill = AlbumDrillAgg::default();
    let items = parse_lsinfo_frame(frame, &uri_prefix, music_root, &mut drill, false);

    let prev = if uri == "music-library" || uri.is_empty() {
        "".to_string()
    } else {
        uri.rsplit_once('/')
            .map(|(p, _)| p.to_string())
            .unwrap_or_else(|| "music-library".to_string())
    };

    Ok(BrowseResponse {
        navigation: BrowseNavigation {
            info: None,
            prev: BrowsePrev { uri: prev },
            lists: vec![BrowseList {
                available_list_views: vec!["list", "grid"],
                items,
            }],
        },
    })
}

/// Volumio-style state JSON (matches what the UI expects from getState).
#[derive(Debug, Clone, Serialize)]
pub struct VolumioState {
    pub status: Option<String>,
    pub position: Option<u32>,
    /// Elapsed position in **milliseconds** (Node `parseState`: `elapsed * 1000`).
    pub seek: Option<u64>,
    /// Track length in **seconds** (Node `parseState`: `time` field part after `:`).
    pub duration: Option<u64>,
    pub volume: Option<u8>,
    /// Output muted (Node `statemachine`: logical level stays in `volume` when muted — see Evo `VolumeUiMuteState`).
    #[serde(default)]
    pub mute: bool,
    /// Mixer type **None**: UI disables fader (Node `disableVolumeControl`).
    #[serde(rename = "disableVolumeControl", default)]
    pub disable_volume_control: bool,
    pub repeat: Option<bool>,
    pub random: Option<bool>,
    /// MPD `single` / Node `repeatSingle`: repeat one song when repeat is on (`true` if single is enabled or oneshot).
    #[serde(rename = "repeatSingle", skip_serializing_if = "Option::is_none")]
    pub repeat_single: Option<bool>,
    /// MPD consume: remove played songs from the queue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consume: Option<bool>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Volumio browse URI (`music-library/...`).
    pub uri: Option<String>,
    /// File extension / codec key for the playback dial (UI: `state.trackType`, `loadFileFormatIcon`).
    #[serde(rename = "trackType")]
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
    /// Mirrors Node `parseState` / `stateService.updatedb`: MPD is updating the music database (`status` → `updating_db`).
    #[serde(rename = "updatedb")]
    pub updatedb: bool,
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

/// Last path segment’s extension, lowercased — must match `player.service.js` `loadFileFormatIcon`
/// (`case 'mp3':`, `case 'flac':`, …) and `/app/assets-common/format-icons/<ext>.svg` on disk.
fn codec_track_type_from_song_url(url: &str) -> Option<String> {
    let leaf = url.rsplit('/').next()?.trim();
    let (_, ext) = leaf.rsplit_once('.')?;
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
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

/// `Artist - Album` in a single folder title (e.g. `Queen - Greatest Hits III`).
fn artist_album_from_leaf_name(leaf: &str) -> Option<(String, String)> {
    for sep in [" – ", " — ", " - "] {
        if let Some((a, b)) = leaf.split_once(sep) {
            let a = a.trim();
            let b = b.trim();
            if !a.is_empty() && !b.is_empty() {
                return Some((a.to_string(), b.to_string()));
            }
        }
    }
    None
}

/// MPD `directory` path under `music_directory` (e.g. `INTERNAL/Adele`, `INTERNAL/Adele/21`).
/// Build `web=` for online providers: `INTERNAL/Artist/Album` → artist+album; `Root/…/Artist/Album` with
/// four or more segments → last two as artist+album; leaf `Artist - Album` → split; else artist-only.
fn web_inner_for_mpd_directory(mpd_path: &str) -> Option<String> {
    const ROOTS: &[&str] = &["INTERNAL", "USB", "NAS", "SMB"];
    let segs: Vec<&str> = mpd_path.split('/').filter(|s| !s.is_empty()).collect();
    let n = segs.len();
    if n < 2 {
        return None;
    }
    if n == 3 && segs[0] == "INTERNAL" {
        return Some(format!("{}/{}/extralarge", segs[1], segs[2]));
    }
    if n >= 4 && ROOTS.contains(&segs[0]) {
        return Some(format!(
            "{}/{}/extralarge",
            segs[n - 2],
            segs[n - 1]
        ));
    }
    let leaf = segs[n - 1];
    if leaf.is_empty() {
        return None;
    }
    if let Some((ar, al)) = artist_album_from_leaf_name(leaf) {
        return Some(format!("{}/{}/extralarge", ar, al));
    }
    Some(format!("{}//extralarge", leaf))
}

/// Fallback icon for directory rows: storage roots under `music-library` match Node (`microchip` / `server` / `usb`).
fn browse_directory_fallback_icon(browse_listing_uri: &str, mpd_directory_path: &str) -> &'static str {
    if browse_listing_uri == "music-library" {
        return match mpd_directory_path {
            "INTERNAL" => "microchip",
            "NAS" => "server",
            "USB" => "usb",
            "SMB" => "server",
            _ => "folder-o",
        };
    }
    "folder-o"
}

/// Filesystem browse folder row: `path=` for local `folder.jpg` / cache, plus heuristic `web=` from folder path.
fn browse_directory_albumart_url(
    volumio_uri: &str,
    mpd_directory_path: &str,
    browse_listing_uri: &str,
) -> String {
    let mut url = format!(
        "/albumart?metadata=true&path={}",
        urlencoding::encode(volumio_uri)
    );
    if let Some(inner) = web_inner_for_mpd_directory(mpd_directory_path) {
        url.push_str("&web=");
        url.push_str(&urlencoding::encode(&inner));
    }
    url.push_str("&icon=");
    url.push_str(browse_directory_fallback_icon(
        browse_listing_uri,
        mpd_directory_path,
    ));
    url
}

/// Tag-library virtual rows: `web=` for online art; `icon` basename must match `icons/<icon>.svg` under plugin dirs.
///
/// Album-only rows (flat album list) use **"Various Artists"** as a synthetic first segment so online
/// album search can run; wrong for some compilations but better than no art.
///
/// A single **album** field like `Queen - Greatest Hits III` is split into artist+album when possible.
fn browse_virtual_albumart_url_with_icon(
    artist: Option<&str>,
    album: Option<&str>,
    icon: &str,
) -> String {
    let mut a = artist.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let mut b = album.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if a.is_none() && b.is_some() {
        if let Some(ref b_s) = b {
            if let Some((ar, al)) = artist_album_from_leaf_name(b_s) {
                a = Some(ar);
                b = Some(al);
            }
        }
    }
    let web_inner = match (a.as_deref(), b.as_deref()) {
        (Some(ar), Some(al)) => Some(format!("{}/{}/extralarge", ar, al)),
        (Some(ar), None) => Some(format!("{}//extralarge", ar)),
        (None, Some(al)) => Some(format!("Various Artists/{}/extralarge", al)),
        (None, None) => None,
    };
    let mut url = String::from("/albumart?");
    if let Some(w) = web_inner {
        url.push_str("web=");
        url.push_str(&urlencoding::encode(&w));
        url.push_str("&");
    }
    url.push_str("icon=");
    url.push_str(icon);
    url
}

/// `albums://` root grid rows (Node `getAlbumArt` from `listAlbums`): `metadata=true` + `path=` from a
/// representative track (folder/embed art), plus `web=` for online fallback, then `dot-circle-o` icon.
/// When `representative_music_library_uri` is None, omits `path`/`metadata` (same as web-only virtual rows).
pub fn browse_album_list_albumart_url(
    representative_music_library_uri: Option<&str>,
    artist: &str,
    album: &str,
) -> String {
    let artist_web = artist_normalize::normalize_for_art_lookup(artist);
    let web_inner = format!("{}/{}/extralarge", artist_web, album.trim());
    let mut url = String::from("/albumart?");
    if let Some(path) = representative_music_library_uri {
        url.push_str("metadata=true&path=");
        url.push_str(&urlencoding::encode(path));
        url.push_str("&");
    }
    url.push_str("web=");
    url.push_str(&urlencoding::encode(&web_inner));
    url.push_str("&icon=dot-circle-o");
    url
}

/// `albums://`, genres, album-under-artist folders, etc. — fallback `folder-o` (Node miscellanea/albumart).
pub fn browse_virtual_folder_albumart_url(artist: Option<&str>, album: Option<&str>) -> String {
    browse_virtual_albumart_url_with_icon(artist, album, "folder-o")
}

/// `artists://` list rows: Node uses `icons/users.svg` when fetched artist art is unavailable.
pub fn browse_artist_list_albumart_url(artist: &str) -> String {
    let for_web = artist_normalize::normalize_for_art_lookup(artist);
    browse_virtual_albumart_url_with_icon(Some(for_web.as_str()), None, "users")
}

/// Node `miscellanea/albumart` `getAlbumArt`: `metadata=true` + `path=` for embed/exiftool/MPD readpicture;
/// when `artist` is set, add `web=artist/album/extralarge` so online providers run after local fails.
///
/// Browse track rows set `browse_icon_music`: append `icon=music` so `/albumart` can fall back to the
/// bundled music note SVG like classic Volumio (grid/list thumbnails).
fn volumio_albumart_url(
    volumio_uri: &str,
    artist: &Option<String>,
    album: &Option<String>,
    browse_icon_music: bool,
) -> String {
    let mut url = format!(
        "/albumart?metadata=true&path={}",
        urlencoding::encode(volumio_uri)
    );
    if browse_icon_music {
        url.push_str("&icon=music");
    }
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

fn push_state_albumart_url(
    volumio_uri: &str,
    artist: &Option<String>,
    album: &Option<String>,
) -> String {
    volumio_albumart_url(volumio_uri, artist, album, false)
}

/// Playlist rows and other browse paths where we only have a `music-library/...` URI (no tags yet).
pub fn browse_song_albumart_path_only(volumio_uri: &str) -> String {
    volumio_albumart_url(volumio_uri, &None, &None, true)
}

pub async fn get_state(
    client: &mut Client,
    music_root: &Path,
    master_volume_from_alsa: Option<u8>,
) -> Result<VolumioState> {
    let status = client.command(Status).await?;

    let seek_ms = status
        .elapsed
        .map(|d| d.as_millis() as u64);
    let duration_secs = status.duration.map(|d| d.as_secs());
    let position = status
        .current_song
        .map(|(pos, _)| pos.0 as u32);

    let raw_status = client.raw_command(RawCommand::new("status")).await.ok();
    let audio_line = raw_status
        .as_ref()
        .and_then(|f| f.find("audio").map(std::string::ToString::to_string));
    let (samplerate, bitdepth) = audio_line
        .as_deref()
        .map(parse_mpd_audio)
        .unwrap_or((None, None));
    // MPD wire format uses `updating_db`; mpd_client maps it to `update_job` — accept both + raw frame.
    let updating_db = status.update_job.is_some()
        || raw_status.as_ref().is_some_and(|f| {
            f.find("updating_db").is_some() || f.find("update_job").is_some()
        });

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
                let track_type = codec_track_type_from_song_url(&s.url);
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

    let volume = master_volume_from_alsa.or(Some(status.volume));

    let repeat_single = matches!(
        status.single,
        SingleMode::Enabled | SingleMode::Oneshot
    );

    Ok(VolumioState {
        status: status_str,
        position,
        seek: seek_ms,
        duration: duration_secs,
        volume,
        mute: false,
        disable_volume_control: false,
        repeat: Some(status.repeat),
        random: Some(status.random),
        repeat_single: Some(repeat_single),
        consume: Some(status.consume),
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
        updatedb: updating_db,
    })
}

/// Queue item for getQueue / `pushQueue` (Node playQueue shape; Evo is MPD-only).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItem {
    pub position: u32,
    /// Stock Volumio2-UI `play-queue.controller.js` renders `item.name` (Node sets `name` from `title`).
    pub name: Option<String>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub uri: Option<String>,
    pub duration: Option<u64>,
    /// Always `mpd` in Evo (no multi-service virtual queue).
    pub service: String,
    /// Per-track `/albumart?...` URL (same shape as `pushState.albumart` / Node `getAlbumArt`).
    pub albumart: String,
    /// File extension / rough type for local files (empty when unknown).
    pub track_type: String,
}

fn track_type_from_uri(url: &str) -> String {
    let path = url.rsplit('/').next().unwrap_or("");
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "mp3" | "flac" | "ogg" | "opus" | "wav" | "m4a" | "aac" | "wma" | "dsf" | "dff" => ext,
        _ => String::new(),
    }
}

pub async fn get_queue(client: &mut Client, music_root: &std::path::Path) -> Result<Vec<QueueItem>> {
    let list = client.command(Queue::all()).await?;
    let items = list
        .into_iter()
        .map(|song_in_queue| {
            let s = &song_in_queue.song;
            let duration = s.duration.map(|d| d.as_secs());
            let volumio_uri = volumio_uri_from_mpd_url(&s.url, music_root);
            let track_type = track_type_from_uri(&s.url);
            let title = s.title().map(String::from);
            let artist = s.artists().first().map(String::from);
            let album = s.album().map(String::from);
            let albumart = push_state_albumart_url(&volumio_uri, &artist, &album);
            QueueItem {
                position: song_in_queue.position.0 as u32,
                name: title.clone(),
                title,
                artist,
                album,
                uri: Some(volumio_uri),
                duration,
                service: "mpd".to_string(),
                albumart,
                track_type,
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
                if !r {
                    client.command(SetSingle(SingleMode::Disabled)).await?;
                }
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

/// Dedicated MPD connection: `idle player playlist` → wake UI when track or queue changes (no 2s poll wait).
pub async fn idle_push_state_wake_loop(config: MpdConfig, wake: UnboundedSender<()>) {
    loop {
        if let Err(e) = idle_player_playlist_session(&config, &wake).await {
            tracing::debug!(
                target: "volumio_evo::mpd_idle",
                "idle session ended: {}",
                e
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }
}

async fn idle_player_playlist_session(config: &MpdConfig, wake: &UnboundedSender<()>) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (read_half, mut write_half) = tokio::io::split(stream);
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    loop {
        write_half.write_all(b"idle player playlist\n").await?;
        write_half.flush().await?;
        loop {
            line.clear();
            let n = reader.read_line(&mut line).await?;
            if n == 0 {
                anyhow::bail!("MPD closed connection");
            }
            let t = line.trim();
            if t.is_empty() {
                continue;
            }
            if t == "OK" {
                break;
            }
            if t.starts_with("ACK") {
                anyhow::bail!("{t}");
            }
            if t.starts_with("changed:") {
                let _ = wake.send(());
            }
        }
    }
}
