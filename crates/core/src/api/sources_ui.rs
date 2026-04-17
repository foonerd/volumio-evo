//! **Settings → Sources** (`miscellanea/my_music`) UI config for the stock Volumio2-UI plugin page.
//! Mirrors `volumio3-backend/app/plugins/miscellanea/my_music/UIConfig.json` so `getUiConfig` can
//! render the same sections as Node (core `my-music`, `network-drives`, declarative album-art / library rows).

/// Stock Sources page: `pushUiConfig` payload matching Node `getUIConfig` for `miscellanea/my_music`.
pub fn my_music_ui_config() -> serde_json::Value {
    serde_json::from_str(include_str!("my_music_ui_config.json"))
        .expect("embedded my_music_ui_config.json must be valid JSON")
}
