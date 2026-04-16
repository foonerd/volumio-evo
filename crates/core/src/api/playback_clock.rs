//! Playback position in RAM between MPD polls (Node `currentSeek`–style).
//! `interpolated_seek_ms()` advances from the last MPD sample using wall time when `status == play`.

use crate::mpd::VolumioState;
use std::time::Instant;

#[derive(Debug, Default)]
pub struct PlaybackClock {
    last_mpd_seek_ms: Option<u64>,
    last_mpd_sample_at: Option<Instant>,
    last_mpd_status: Option<String>,
    last_duration_secs: Option<u64>,
}

impl PlaybackClock {
    pub fn sync_from_mpd(&mut self, s: &VolumioState) {
        self.last_mpd_seek_ms = s.seek;
        self.last_mpd_sample_at = Some(Instant::now());
        self.last_mpd_status = s.status.clone();
        self.last_duration_secs = s.duration;
    }

    /// Elapsed ms for `pushState` / REST: MPD truth + wall time since last sample when playing.
    pub fn interpolated_seek_ms(&self) -> Option<u64> {
        if self.last_mpd_status.as_deref() != Some("play") {
            return self.last_mpd_seek_ms;
        }
        let t0 = self.last_mpd_sample_at?;
        let base = self.last_mpd_seek_ms.unwrap_or(0);
        let elapsed = t0.elapsed().as_millis() as u64;
        let mut seek = base.saturating_add(elapsed);
        if let Some(d) = self.last_duration_secs {
            seek = seek.min(d.saturating_mul(1000));
        }
        Some(seek)
    }
}
