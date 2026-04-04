//! MPD client: connect and run commands. Used by the v1 API to mirror Volumio backend behaviour.

use anyhow::Result;
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
pub async fn get_state_connected(config: &MpdConfig) -> Result<VolumioState> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    get_state(&mut client).await
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
        items.push(BrowseItem {
            item_type: "song".to_string(),
            title: f.title,
            uri: f.uri,
            service: "mpd".to_string(),
            artist: f.artist,
            album: f.album,
            duration: f.duration,
        });
    }
}

/// Parse MPD lsinfo Frame into BrowseItems. Frame has key-value pairs; "directory" and "file" start new entries.
fn parse_lsinfo_frame(frame: mpd_client::protocol::response::Frame, uri_prefix: &str) -> Vec<BrowseItem> {
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
                let item_uri = if uri_prefix.is_empty() {
                    format!("music-library/{}", value)
                } else {
                    format!("{}/{}", uri_prefix, value)
                };
                items.push(BrowseItem {
                    item_type: "folder".to_string(),
                    title: name,
                    uri: item_uri,
                    service: "mpd".to_string(),
                    artist: None,
                    album: None,
                    duration: None,
                });
            }
            "file" => {
                flush_file_item(&mut current_file, &mut items);
                let name = value
                    .rsplit('/')
                    .next()
                    .unwrap_or(value)
                    .to_string();
                let item_uri = if uri_prefix.is_empty() {
                    format!("music-library/{}", value)
                } else {
                    format!("{}/{}", uri_prefix, value)
                };
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
pub async fn search_connected(config: &MpdConfig, query: &str) -> Result<BrowseResponse> {
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
    let items = parse_lsinfo_frame(frame, "music-library");
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
        .filter_map(|(k, v)| if *k == *"Album" { Some(v.to_string()) } else { None })
        .collect();
    albums.sort();
    albums.dedup();
    let items: Vec<BrowseItem> = albums
        .into_iter()
        .map(|album| BrowseItem {
            item_type: "folder".to_string(),
            title: album.clone(),
            uri: format!("albums://{}/{}", artist, album),
            service: "mpd".to_string(),
            artist: Some(artist.to_string()),
            album: Some(album),
            duration: None,
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
    let items = parse_lsinfo_frame(frame, "music-library");
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

/// Connect to MPD, run lsinfo for the given Volumio uri (e.g. "music-library" or "music-library/INTERNAL"), return browse response.
/// Also handles virtual URIs: artists://<name> (albums by artist), albums://<artist>/<album> (songs in album).
pub async fn browse_connected(config: &MpdConfig, uri: &str) -> Result<BrowseResponse> {
    if uri.starts_with("artists://") && uri != "artists://" {
        let artist = uri.strip_prefix("artists://").unwrap_or("").trim();
        if !artist.is_empty() {
            return browse_artist_connected(config, artist).await;
        }
    }
    if uri.starts_with("albums://") {
        let rest = uri.strip_prefix("albums://").unwrap_or("");
        if let Some((a, b)) = rest.split_once('/') {
            let artist = a.trim();
            let album = b.trim();
            if !artist.is_empty() && !album.is_empty() {
                return browse_album_songs_connected(config, artist, album).await;
            }
        }
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

    let items = parse_lsinfo_frame(frame, &uri_prefix);

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
    pub seek: Option<u64>,
    pub duration: Option<u64>,
    pub volume: Option<u8>,
    pub repeat: Option<bool>,
    pub random: Option<bool>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub uri: Option<String>,
    pub track_type: Option<String>,
    pub service: Option<String>,
}

pub async fn get_state(client: &mut Client) -> Result<VolumioState> {
    let status = client.command(Status).await?;

    let seek_ms = status
        .elapsed
        .map(|d| d.as_millis() as u64);
    let duration_ms = status
        .duration
        .map(|d| d.as_millis() as u64);
    let position = status
        .current_song
        .map(|(pos, _)| pos.0 as u32);

    let (title, artist, album, uri, track_type) = if position.is_some() {
        match client.command(CurrentSong).await? {
            Some(song_in_queue) => {
                let s = &song_in_queue.song;
                let title = s.title().map(String::from);
                let artist = s.artists().first().map(String::from);
                let album = s.album().map(String::from);
                let uri = Some(s.url.clone());
                let track_type = s.url.split('.').last().map(String::from);
                (title, artist, album, uri, track_type)
            }
            None => (None, None, None, None, None),
        }
    } else {
        (None, None, None, None, None)
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
        duration: duration_ms,
        volume: Some(status.volume),
        repeat: Some(status.repeat),
        random: Some(status.random),
        title,
        artist,
        album,
        uri,
        track_type,
        service: Some("mpd".to_string()),
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
                let d = Duration::from_millis(pos as u64);
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
