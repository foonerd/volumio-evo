# CUE sheets in Volumio Evo (Rust)

## Implemented in `crates/core/src/cue_normalize.rs` + MPD

- **Text normalization** (BOM, line endings, `FILE` backslashes → `/`, `INDEX` time padding) before a small in-tree **first-`FILE`-only** track list parse.
- **Browse**: MPD `lsinfo` `playlist:` lines for `*.cue` expand to `cuefile` + `cuesong` rows (aligned with legacy Node behaviour). `BrowseItem.number` carries the legacy “track index” for `cuesong` (`track_no - 1`).
- **Playback**: `replaceAndPlayCue` → `stop`, `clear`, **`load`** cue path, **`play`** optional 0-based position. `addPlayCue` → **`load`** (append); if `number` is set, **`play`** at queue offset before append + index.
- **Resolved URIs**: `replace_and_play_resolved`, `add_play_append_resolved`, and `add_to_queue_resolved` detect `music-library/.../*.cue` and use **`load`** instead of **`add`** so generic UI actions behave like playlists (append-and-play-first-track for “add play”, append-only for “add to queue”).

## Deferred (optional sidecar plugin / later refinement)

| Topic | Risk if rushed |
|--------|----------------|
| Writing repaired `.cue` next to originals for MPD | Destructive; requires UX + backups |
| Multi-`FILE` cues in browse/queue | Needs product rules beyond `files[0]` |
| Embedded FLAC cues | MPD decoder / DB behaviour |
| Full cue grammar vs rippers | Needs curated failing samples |

Implement these behind explicit settings or offline tools first.
