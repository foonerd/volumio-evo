Evo-owned deltas on top of upstream Volumio2-UI (volumio3 theme).

Directory tree mirrors paths under the Volumio2-UI repo root (e.g. src/app/...).
bootstrap-volumio-evo-player.sh rsyncs this folder onto the UI checkout after
normalize_volumio2_ui_checkout() and before npm install / gulp build.

Do not rely on ad-hoc local edits only in a separate Volumio2-UI clone: either
maintain a git fork whose branch matches this overlay, or commit changes here
so the Rust player repo is the single source of truth for shipped UI assets.
