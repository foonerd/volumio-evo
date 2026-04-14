Optional reference: Evo-owned deltas on top of upstream Volumio2-UI (volumio3 theme).

Bootstrap does NOT use this folder. Player installs copy static UI from layer/web/ only.

Use this tree when you build Volumio2-UI yourself (developer machine or CI): rsync this folder
onto a Volumio2-UI checkout after normalizing package.json for Node 20 / Dart Sass, then
npm install / gulp — same paths as documented historically.

gulp/styles.js may override upstream to pass Dart Sass silenceDeprecations until upstream migrates.

header/volumio3-header.scss: footer play-button offsets must not apply in the top toolbar
(classic/contemporary); see playPauseBtnGreyWrapper override.

Keep changes here if you maintain a parallel UI build pipeline; layer/web/ is what ships on device.
