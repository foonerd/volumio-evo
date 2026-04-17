//! DEBUG / WARN lines for playback state delivered to the UI (`pushState`, REST `getState`, `pushQueue`).
//! Every line includes [`crate::log_tags::EVO_PUSHSTATE`] or [`EVO_STATE`] / [`EVO_QUEUE`] so
//! `journalctl | grep -F 'EVO PUSHSTATE'` shows what was built and whether delivery succeeded.

use crate::log_tags::{EVO_PUSHSTATE, EVO_QUEUE, EVO_STATE};
use crate::mpd::VolumioState;

#[inline]
pub(crate) fn debug_volumio_state(source: &'static str, s: &VolumioState) {
    tracing::debug!(
        "{} {} status={:?} seek_ms={:?} duration_s={:?} position={:?} volume={:?} mute={}",
        EVO_PUSHSTATE,
        source,
        s.status,
        s.seek,
        s.duration,
        s.position,
        s.volume,
        s.mute,
    );
}

#[inline]
pub(crate) fn debug_broadcast_push_state_after_emit(s: &VolumioState, emit_ok: bool) {
    tracing::debug!(
        "{} broadcast io.emit(pushState) status={:?} seek_ms={:?} duration_s={:?} position={:?} volume={:?} mute={} emit={}",
        EVO_PUSHSTATE,
        s.status,
        s.seek,
        s.duration,
        s.position,
        s.volume,
        s.mute,
        if emit_ok { "ok" } else { "err" },
    );
}

#[inline]
pub(crate) fn warn_broadcast_push_state_emit(err: impl std::fmt::Display) {
    tracing::warn!("{} broadcast io.emit(pushState) failed: {}", EVO_PUSHSTATE, err);
}

#[inline]
pub(crate) fn warn_broadcast_get_state(err: impl std::fmt::Display) {
    tracing::warn!("{} broadcast get_state_connected failed (no pushState): {}", EVO_STATE, err);
}

#[inline]
pub(crate) fn warn_broadcast_get_queue(err: impl std::fmt::Display) {
    tracing::warn!("{} broadcast get_queue_connected failed (no pushQueue): {}", EVO_QUEUE, err);
}

#[inline]
pub(crate) fn debug_broadcast_push_queue_after_emit(queue_len: usize, emit_ok: bool) {
    tracing::debug!(
        "{} broadcast io.emit(pushQueue) queue_len={} emit={}",
        EVO_PUSHSTATE,
        queue_len,
        if emit_ok { "ok" } else { "err" },
    );
}

#[inline]
pub(crate) fn warn_broadcast_push_queue_emit(err: impl std::fmt::Display) {
    tracing::warn!("{} broadcast io.emit(pushQueue) failed: {}", EVO_PUSHSTATE, err);
}

#[inline]
pub(crate) fn debug_socket_push_state_after_emit(source: &'static str, s: &VolumioState, emit_ok: bool) {
    tracing::debug!(
        "{} {} status={:?} seek_ms={:?} duration_s={:?} position={:?} volume={:?} mute={} emit={}",
        EVO_PUSHSTATE,
        source,
        s.status,
        s.seek,
        s.duration,
        s.position,
        s.volume,
        s.mute,
        if emit_ok { "ok" } else { "err" },
    );
}

#[inline]
pub(crate) fn warn_socket_push_state_emit(source: &'static str, err: impl std::fmt::Display) {
    tracing::warn!("{} {} SocketRef.emit(pushState) failed: {}", EVO_PUSHSTATE, source, err);
}

#[inline]
pub(crate) fn debug_queue_snapshot(source: &'static str, queue_len: usize) {
    tracing::debug!("{} {} queue_len={}", EVO_PUSHSTATE, source, queue_len);
}

#[inline]
pub(crate) fn debug_socket_push_queue_after_emit(source: &'static str, queue_len: usize, emit_ok: bool) {
    tracing::debug!(
        "{} {} queue_len={} emit={}",
        EVO_PUSHSTATE,
        source,
        queue_len,
        if emit_ok { "ok" } else { "err" },
    );
}

#[inline]
pub(crate) fn warn_socket_push_queue_emit(source: &'static str, err: impl std::fmt::Display) {
    tracing::warn!("{} {} SocketRef.emit(pushQueue) failed: {}", EVO_PUSHSTATE, source, err);
}
