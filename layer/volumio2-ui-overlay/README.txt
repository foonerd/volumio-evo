Evo-owned deltas on top of upstream Volumio2-UI (volumio3 theme).

On-device full UI build + test: only scripts/bootstrap-volumio-evo-player.sh applies
this overlay and builds — not a hand-maintained Volumio2-UI checkout.

Directory tree mirrors paths under the Volumio2-UI repo root (e.g. src/app/...).
bootstrap-volumio-evo-player.sh rsyncs this folder onto the UI checkout after
normalize_volumio2_ui_checkout() and before npm install / gulp build.

gulp/styles.js may override upstream to pass Dart Sass silenceDeprecations until
Volumio2-UI migrates off @import / legacy APIs (large upstream change).

header/volumio3-header.scss: footer play-button offsets must not apply in the top
toolbar (classic/contemporary); see playPauseBtnGreyWrapper override.

Do not rely on ad-hoc local edits only in a separate Volumio2-UI clone: either
maintain a git fork whose branch matches this overlay, or commit changes here
so the Rust player repo is the single source of truth for shipped UI assets.
