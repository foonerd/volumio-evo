//! Every Evo log line includes one of these substrings so **`journalctl | grep 'EVO '`** (or
//! `grep 'EVO QUEUE'`, `grep 'EVO VOLUME'`, …) is reliable.
//!
//! **Process-wide:** [`EVO_LINE`] is written at the start of every formatted log line (including
//! `tower_http`, `mpd_protocol`, …). Example: `journalctl -u volumio-evo | grep -F '[EVO]'`.

/// First characters on every log line from this process (high-contrast; safe for `grep -F`).
pub const EVO_LINE: &str = "[EVO] ";

pub const EVO_BOOT: &str = "EVO BOOT -->";
pub const EVO_CONFIG: &str = "EVO CONFIG -->";
pub const EVO_META: &str = "EVO META -->";
pub const EVO_PLAYBACK: &str = "EVO PLAYBACK -->";
pub const EVO_I2S: &str = "EVO I2S -->";
pub const EVO_ALBUMART: &str = "EVO ALBUMART -->";
pub const EVO_ALSA: &str = "EVO ALSA -->";
pub const EVO_VOLUME: &str = "EVO VOLUME -->";
#[allow(dead_code)]
pub const EVO_MPD: &str = "EVO MPD -->";
pub const EVO_BROWSE: &str = "EVO BROWSE -->";
pub const EVO_QUEUE: &str = "EVO QUEUE -->";
pub const EVO_PLAY: &str = "EVO PLAY -->";
pub const EVO_SEARCH: &str = "EVO SEARCH -->";
pub const EVO_PLAYLIST: &str = "EVO PLAYLIST -->";
pub const EVO_STATE: &str = "EVO STATE -->";
pub const EVO_SOCKET: &str = "EVO SOCKET -->";
#[allow(dead_code)]
pub const EVO_HTTP: &str = "EVO HTTP -->";
pub const EVO_API: &str = "EVO API -->";
pub const EVO_OUTPUT: &str = "EVO OUTPUT -->";
pub const EVO_UI: &str = "EVO UI -->";
pub const EVO_DB: &str = "EVO DB -->";
#[cfg_attr(not(feature = "wasm"), allow(dead_code))]
pub const EVO_PLUGIN: &str = "EVO PLUGIN -->";
