//! RAM playback clock: interpolate between MPD samples; emit seek **before** [`PlaybackClock::sync_from_mpd`].

use crate::mpd::VolumioState;
use std::time::Instant;

/// Grid for outgoing `seek` (ms). **100** trims jitter; use **1000** for whole-second only (mm:ss).
const UI_SEEK_ROUND_MS: u64 = 100;

/// If extrapolated position and fresh MPD `seek` disagree by more than this, trust MPD (user seek, skip, etc.).
const SEEK_EXTRAPOLATION_MAX_DRIFT_MS: u128 = 3_000;

/// Round `seek` for Socket/REST JSON (still ms). Clamped to `duration` when known.
pub fn ui_seek_ms(seek: Option<u64>, duration_secs: Option<u64>) -> Option<u64> {
    let step = UI_SEEK_ROUND_MS.max(1);
    let half = step / 2;
    seek.map(|ms| {
        let cap = duration_secs.map(|d| d.saturating_mul(1000)).unwrap_or(u64::MAX);
        let r = ((ms.saturating_add(half)) / step).saturating_mul(step);
        r.min(cap)
    })
}

#[derive(Debug, Default)]
pub struct PlaybackClock {
    last_mpd_seek_ms: Option<u64>,
    last_mpd_sample_at: Option<Instant>,
    last_mpd_status: Option<String>,
    last_duration_secs: Option<u64>,
    last_position: Option<u32>,
    last_uri: Option<String>,
}

impl PlaybackClock {
    pub fn sync_from_mpd(&mut self, s: &VolumioState) {
        self.last_mpd_seek_ms = s.seek;
        self.last_mpd_sample_at = Some(Instant::now());
        self.last_mpd_status = s.status.clone();
        self.last_duration_secs = s.duration;
        self.last_position = s.position;
        self.last_uri = s.uri.clone();
    }

    /// Elapsed ms from the **current** anchor (after [`sync_from_mpd`]). Use only when you are not
    /// about to emit right after a fresh MPD poll; for `pushState` / REST use
    /// [`seek_for_emit_before_resync`].
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

    /// `seek` to put on the wire after an MPD poll, **before** calling [`sync_from_mpd`] with that poll.
    pub fn seek_for_emit_before_resync(&self, fresh: &VolumioState) -> Option<u64> {
        if fresh.status.as_deref() != Some("play") {
            return fresh.seek;
        }
        let continued = self.last_mpd_status.as_deref() == Some("play")
            && self.last_uri == fresh.uri
            && self.last_position == fresh.position;
        if !continued {
            return fresh.seek;
        }
        let Some(mut ex) = self.interpolated_seek_ms() else {
            return fresh.seek;
        };
        if let Some(d) = fresh.duration {
            ex = ex.min(d.saturating_mul(1000));
        }
        if let Some(m) = fresh.seek {
            let diff = (ex as i128 - m as i128).abs() as u128;
            if diff > SEEK_EXTRAPOLATION_MAX_DRIFT_MS {
                return Some(m);
            }
        }
        Some(ex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        status: &str,
        seek: Option<u64>,
        position: Option<u32>,
        uri: Option<&str>,
        duration: Option<u64>,
    ) -> VolumioState {
        VolumioState {
            status: Some(status.to_string()),
            position,
            seek,
            duration,
            uri: uri.map(String::from),
            volume: None,
            repeat: None,
            random: None,
            repeat_single: None,
            consume: None,
            title: None,
            artist: None,
            album: None,
            track_type: None,
            service: None,
            albumart: None,
            samplerate: None,
            bitdepth: None,
            bitrate: None,
            updatedb: false,
        }
    }

    #[test]
    fn emit_before_resync_advances_between_polls() {
        let mut c = PlaybackClock::default();
        let s0 = state("play", Some(0), Some(0), Some("music-library/a.flac"), Some(300));
        c.sync_from_mpd(&s0);
        std::thread::sleep(std::time::Duration::from_millis(120));
        let s1 = state("play", Some(0), Some(0), Some("music-library/a.flac"), Some(300));
        let raw = c.seek_for_emit_before_resync(&s1).unwrap();
        let out = ui_seek_ms(Some(raw), s1.duration).unwrap();
        assert!(out >= 100, "expected >=100ms advance after round, raw={raw} out={out}");
        c.sync_from_mpd(&s1);
    }

    #[test]
    fn ui_seek_rounds_and_clamps() {
        assert_eq!(ui_seek_ms(Some(1144), Some(300)), Some(1100));
        assert_eq!(ui_seek_ms(Some(215_666), Some(215)), Some(215_000));
        assert_eq!(ui_seek_ms(Some(99), None), Some(100));
    }

    #[test]
    fn sync_then_read_is_near_zero() {
        let mut c = PlaybackClock::default();
        let s0 = state("play", Some(10_000), Some(0), Some("x"), Some(300));
        c.sync_from_mpd(&s0);
        let tiny = c.interpolated_seek_ms().unwrap();
        assert!(tiny < 10_500, "expected ~10s not wall advance; got {}", tiny);
    }
}
