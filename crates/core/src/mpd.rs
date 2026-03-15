//! MPD client: connect and run commands. Used by the v1 API to mirror Volumio backend behaviour.

use anyhow::Result;
use mpd_client::{
    commands::{
        ClearQueue, CurrentSong, Next, Play, Previous, Queue, Seek as MpdSeekCmd,
        SeekMode, SetPause as MpdPause, SetRandom, SetRepeat, SetVolume, Status, Stop,
    },
    protocol::command::Command as RawCommand,
    responses::PlayState,
    Client,
};
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

/// Remove item at queue position (0-based). Volumio UI may send 1-based; caller can pass pos - 1.
pub async fn remove_from_queue_connected(config: &MpdConfig, position: u32) -> Result<()> {
    let stream = TcpStream::connect(config.addr()).await?;
    let (client, _) = Client::connect(stream).await?;
    client
        .raw_command(RawCommand::new("delete").argument(position.to_string()))
        .await?;
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

/// Connect to MPD, run lsinfo for the given Volumio uri (e.g. "music-library" or "music-library/INTERNAL"), return browse response.
pub async fn browse_connected(config: &MpdConfig, uri: &str) -> Result<BrowseResponse> {
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
