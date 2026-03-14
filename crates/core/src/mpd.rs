//! MPD client: connect and run commands. Used by the v1 API to mirror Volumio backend behaviour.

use anyhow::Result;
use mpd_client::{
    commands::{
        ClearQueue, CurrentSong, Next, Play, Previous, Queue, Seek as MpdSeekCmd,
        SeekMode, SetPause as MpdPause, SetRandom, SetRepeat, SetVolume, Status, Stop,
    },
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

/// Connect to MPD and run a function with the client, then close. One-shot for simplicity.
pub async fn with_mpd<F, Fut, T>(config: &MpdConfig, f: F) -> Result<T>
where
    F: FnOnce(&mut Client) -> Fut,
    Fut: std::future::Future<Output = Result<T>> + Send,
{
    let stream = TcpStream::connect(config.addr()).await?;
    let (mut client, _) = Client::connect(stream).await?;
    f(&mut client).await
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
