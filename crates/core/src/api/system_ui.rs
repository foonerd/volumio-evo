//! **Settings → System** (`system_controller/system`) UI config for stock Volumio2-UI.
//!
//! Omits install-to-disk. **UI layout** (manifest / contemporary / classic) is configured under **Settings → Appearance**
//! (see [`miscellanea_appearance_ui_config`]); the same control is **repeated at the bottom of this page** so users on
//! the manifest layout (no main-menu link to Appearance) can still switch layout. This page includes locale, WPE
//! kiosk placeholders, updates,
//! credits, privacy — matching agreed Evo scope.

use serde_json::{json, Value};

use crate::system_settings::{normalize_plymouth_rotation, SystemSettings};

use super::sources_ui::resolve_translate_tokens;

/// ISO 3166-1 alpha-2 → label (English). Expand as needed.
const COUNTRY_OPTIONS: &[(&str, &str)] = &[
    ("US", "United States"),
    ("GB", "United Kingdom"),
    ("DE", "Germany"),
    ("FR", "France"),
    ("IT", "Italy"),
    ("ES", "Spain"),
    ("NL", "Netherlands"),
    ("BE", "Belgium"),
    ("AT", "Austria"),
    ("CH", "Switzerland"),
    ("PL", "Poland"),
    ("SE", "Sweden"),
    ("NO", "Norway"),
    ("FI", "Finland"),
    ("DK", "Denmark"),
    ("IE", "Ireland"),
    ("PT", "Portugal"),
    ("GR", "Greece"),
    ("CZ", "Czech Republic"),
    ("AU", "Australia"),
    ("NZ", "New Zealand"),
    ("JP", "Japan"),
    ("KR", "South Korea"),
    ("CN", "China"),
    ("TW", "Taiwan"),
    ("IN", "India"),
    ("BR", "Brazil"),
    ("CA", "Canada"),
    ("MX", "Mexico"),
];

/// Stock UI language codes / labels (subset; extend when `getAvailableLanguages` grows).
pub(crate) const LANGUAGE_OPTIONS: &[(&str, &str)] = &[
    ("en", "English"),
    ("de", "Deutsch"),
    ("fr", "Français"),
    ("it", "Italiano"),
    ("es", "Español"),
    ("nl", "Nederlands"),
    ("pt", "Português"),
    ("ja", "日本語"),
];

fn hour_options() -> Vec<Value> {
    (0..=23)
        .map(|h| json!({ "value": h, "label": format!("{h}") }))
        .collect()
}

fn country_select_options() -> Vec<Value> {
    COUNTRY_OPTIONS
        .iter()
        .map(|(code, name)| json!({ "value": *code, "label": *name }))
        .collect()
}

fn language_select_options() -> Vec<Value> {
    LANGUAGE_OPTIONS
        .iter()
        .map(|(code, name)| json!({ "value": *code, "label": *name }))
        .collect()
}

fn timezone_select_options(zones: &[String]) -> Vec<Value> {
    zones
        .iter()
        .map(|z| json!({ "value": z, "label": z }))
        .collect()
}

fn country_value_label(code: &str) -> Value {
    let trimmed = code.trim().to_uppercase();
    let code_str = if trimmed.len() == 2 {
        trimmed
    } else {
        "US".to_string()
    };
    let label = COUNTRY_OPTIONS
        .iter()
        .find(|(c, _)| *c == code_str.as_str())
        .map(|(_, l)| (*l).to_string())
        .unwrap_or_else(|| code_str.clone());
    json!({ "value": code_str, "label": label })
}

pub(crate) fn language_label_for_code(code: &str) -> &'static str {
    let c = code.trim().to_ascii_lowercase();
    LANGUAGE_OPTIONS
        .iter()
        .find(|(code, _)| *code == c.as_str())
        .map(|(_, label)| *label)
        .unwrap_or("English")
}

/// `pushAvailableLanguages` payload (Node appearance shape).
pub fn available_languages_payload(default_code: &str) -> Value {
    let c = default_code.trim().to_ascii_lowercase();
    let c = if c.is_empty() {
        "en".to_string()
    } else {
        c
    };
    let label = language_label_for_code(&c);
    let available: Vec<Value> = LANGUAGE_OPTIONS
        .iter()
        .map(|(code, name)| json!({ "language": name, "code": code }))
        .collect();
    json!({
        "defaultLanguage": { "language": label, "code": c },
        "available": available
    })
}

fn language_value_label(code: &str) -> Value {
    let c = code.trim().to_ascii_lowercase();
    let c = if c.is_empty() {
        "en".to_string()
    } else {
        c
    };
    json!({ "value": c.clone(), "label": language_label_for_code(&c) })
}

fn timezone_value_label(tz: &str) -> Value {
    let t = tz.trim();
    let v = if t.is_empty() { "UTC" } else { t };
    json!({ "value": v, "label": v })
}

fn boot_branding_rotation_value_label(deg: u16) -> Value {
    let d = normalize_plymouth_rotation(deg);
    json!({ "value": d, "label": format!("{d}°") })
}

fn volumio3_ui_options_array() -> Value {
    json!([
        {
            "value": "manifest",
            "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_MANIFEST"
        },
        {
            "value": "contemporary",
            "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_CONTEMPORARY"
        },
        {
            "value": "classic",
            "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_CLASSIC"
        }
    ])
}

fn volumio3_ui_current_value(active_layout: &str) -> Value {
    let v = active_layout.trim().to_lowercase();
    let v = if matches!(v.as_str(), "manifest" | "contemporary" | "classic") {
        v
    } else {
        "contemporary".to_string()
    };
    let label = match v.as_str() {
        "manifest" => "TRANSLATE.APPEARANCE.USER_INTERFACE_MANIFEST",
        "classic" => "TRANSLATE.APPEARANCE.USER_INTERFACE_CLASSIC",
        _ => "TRANSLATE.APPEARANCE.USER_INTERFACE_CONTEMPORARY",
    };
    json!({ "value": v, "label": label })
}

/// Full `pushUiConfig` payload for **`system_controller/system`** with **`TRANSLATE.*`** resolved to English.
pub fn system_settings_ui_config(settings: &SystemSettings, zones: &[String], active_layout: &str) -> Value {
    let hour_opts = hour_options();
    let v3_val = volumio3_ui_current_value(active_layout);
    let v3_opts = volumio3_ui_options_array();
    let mut out = json!({
      "page": { "label": "TRANSLATE.SYSTEM.SYSTEM_SETTINGS" },
      "sections": [
        {
          "id": "section_general_settings",
          "element": "section",
          "label": "TRANSLATE.SYSTEM.GENERAL_SETTINGS",
          "icon": "fa-wrench",
          "onSave": {
            "type": "controller",
            "endpoint": "system_controller/system",
            "method": "saveGeneralSettings"
          },
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [ "player_name" ]
          },
          "content": [
            {
              "id": "player_name",
              "type": "text",
              "element": "input",
              "doc": "TRANSLATE.SYSTEM.PLAYER_NAME_DOC",
              "label": "TRANSLATE.SYSTEM.PLAYER_NAME",
              "value": settings.device_name
            }
          ]
        },
        {
          "id": "section_locale_region",
          "element": "section",
          "label": "TRANSLATE.SYSTEM.LOCALE_REGION",
          "icon": "fa-globe",
          "onSave": {
            "type": "controller",
            "endpoint": "system_controller/system",
            "method": "saveLocaleSettings"
          },
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [ "language", "country", "timezone" ]
          },
          "content": [
            {
              "id": "language",
              "element": "select",
              "doc": "TRANSLATE.APPEARANCE.UI_CONFIG_SELECT_LANGUAGE_DOC",
              "label": "TRANSLATE.APPEARANCE.UI_CONFIG_LANGUAGE",
              "value": language_value_label(&settings.language_code),
              "options": language_select_options()
            },
            {
              "id": "country",
              "element": "select",
              "doc": "TRANSLATE.SYSTEM.COUNTRY_DOC",
              "label": "TRANSLATE.MYVOLUMIO.COUNTRY",
              "value": country_value_label(&settings.country_code),
              "options": country_select_options()
            },
            {
              "id": "timezone",
              "element": "select",
              "doc": "TRANSLATE.APPEARANCE.UI_CONFIG_SELECT_TIMEZONE_DOC",
              "label": "TRANSLATE.APPEARANCE.UI_CONFIG_SELECT_TIMEZONE",
              "value": timezone_value_label(&settings.timezone),
              "options": timezone_select_options(zones)
            }
          ]
        },
        {
          "id": "section_wpe_kiosk",
          "element": "section",
          "label": "TRANSLATE.SYSTEM.KIOSK_WPE",
          "icon": "fa-desktop",
          "onSave": {
            "type": "controller",
            "endpoint": "system_controller/system",
            "method": "saveKioskSettings"
          },
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [ "kiosk_enabled", "primary_display" ]
          },
          "content": [
            {
              "id": "kiosk_enabled",
              "element": "switch",
              "doc": "TRANSLATE.SYSTEM.KIOSK_WPE_DOC",
              "label": "TRANSLATE.SYSTEM.KIOSK_ENABLE",
              "value": settings.kiosk_enabled
            },
            {
              "id": "primary_display",
              "element": "select",
              "label": "TRANSLATE.SYSTEM.PRIMARY_DISPLAY",
              "doc": "TRANSLATE.SYSTEM.PRIMARY_DISPLAY_DOC",
              "value": {
                "value": settings.primary_display,
                "label": settings.primary_display
              },
              "options": [
                { "value": "auto", "label": "auto" },
                { "value": "hdmi", "label": "HDMI" },
                { "value": "dsi", "label": "DSI / touchscreen" },
                { "value": "wayland-default", "label": "Wayland default" }
              ]
            }
          ]
        },
        {
          "id": "section_boot_branding",
          "element": "section",
          "label": "TRANSLATE.SYSTEM.BOOT_BRANDING",
          "icon": "fa-picture-o",
          "onSave": {
            "type": "controller",
            "endpoint": "system_controller/system",
            "method": "installBootBranding"
          },
          "saveButton": {
            "label": "TRANSLATE.SYSTEM.INSTALL_BOOT_BRANDING",
            "data": [ "boot_branding_rotation" ]
          },
          "content": [
            {
              "id": "boot_branding_rotation",
              "element": "select",
              "doc": "TRANSLATE.SYSTEM.BOOT_BRANDING_ROTATION_DOC",
              "label": "TRANSLATE.SYSTEM.BOOT_BRANDING_ROTATION",
              "value": boot_branding_rotation_value_label(settings.boot_branding_plymouth_rotation),
              "options": [
                { "value": 0, "label": "0°" },
                { "value": 90, "label": "90°" },
                { "value": 180, "label": "180°" },
                { "value": 270, "label": "270°" }
              ]
            }
          ]
        },
        { "coreSection": "system-version" },
        {
          "id": "section_updates",
          "type": "section",
          "label": "TRANSLATE.SYSTEM.SYSTEM_UPDATES",
          "icon": "fa-refresh",
          "onSave": {
            "type": "controller",
            "endpoint": "system_controller/system",
            "method": "saveUpdateSettings"
          },
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [
              "automatic_updates",
              "automatic_updates_start_time",
              "automatic_updates_stop_time"
            ]
          },
          "content": [
            {
              "id": "update",
              "element": "button",
              "label": "TRANSLATE.SYSTEM.CHECK_UPDATES",
              "description": "TRANSLATE.SYSTEM.CHECK_UPDATES_DESCR",
              "onClick": {
                "type": "emit",
                "message": "updateCheck",
                "data": "search-for-upgrade"
              }
            },
            {
              "id": "factory",
              "element": "button",
              "label": "TRANSLATE.SYSTEM.FACTORY_RESET",
              "description": "TRANSLATE.SYSTEM.FACTORY_RESET_DESCR",
              "onClick": {
                "type": "emit",
                "message": "deleteUserData",
                "data": " ",
                "askForConfirm": {
                  "title": "TRANSLATE.SYSTEM.FACTORY_RESET_TITLE",
                  "message": "TRANSLATE.SYSTEM.FACTORY_RESET_MESSAGE"
                }
              }
            },
            {
              "id": "automatic_updates",
              "element": "switch",
              "description": "TRANSLATE.SYSTEM.AUTOMATIC_UPDATES_DOC",
              "label": "TRANSLATE.SYSTEM.AUTOMATIC_UPDATES",
              "value": settings.automatic_updates
            },
            {
              "id": "automatic_updates_start_time",
              "element": "select",
              "description": "TRANSLATE.SYSTEM.AUTOMATIC_UPDATES_START_TIME_DOC",
              "label": "TRANSLATE.SYSTEM.AUTOMATIC_UPDATES_START_TIME",
              "value": {
                "value": settings.automatic_updates_start_hour,
                "label": format!("{}", settings.automatic_updates_start_hour)
              },
              "options": hour_opts.clone()
            },
            {
              "id": "automatic_updates_stop_time",
              "element": "select",
              "description": "TRANSLATE.SYSTEM.AUTOMATIC_UPDATES_STOP_TIME_DOC",
              "label": "TRANSLATE.SYSTEM.AUTOMATIC_UPDATES_STOP_TIME",
              "value": {
                "value": settings.automatic_updates_stop_hour,
                "label": format!("{}", settings.automatic_updates_stop_hour)
              },
              "options": hour_opts
            }
          ]
        },
        {
          "id": "section_foss",
          "type": "section",
          "label": "TRANSLATE.SYSTEM.CREDITS_OPEN_SOURCE_LICENSES",
          "icon": "fa-user-circle",
          "content": [
            {
              "id": "credits_foss",
              "element": "button",
              "hidden": false,
              "label": "TRANSLATE.SYSTEM.CREDITS_OPEN_SOURCE_LICENSES",
              "onClick": {
                "type": "goto",
                "pageName": "credits"
              }
            }
          ]
        },
        {
          "id": "section_privacy_settings",
          "element": "section",
          "hidden": false,
          "label": "TRANSLATE.SYSTEM.PRIVACY_SETTINGS",
          "icon": "fa-shield",
          "onSave": {
            "type": "controller",
            "endpoint": "system_controller/system",
            "method": "savePrivacySettings"
          },
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [ "allow_ui_statistics" ]
          },
          "content": [
            {
              "id": "allow_ui_statistics",
              "element": "switch",
              "doc": "TRANSLATE.SYSTEM.ALLOW_UI_STATISTICS_DOC",
              "label": "TRANSLATE.SYSTEM.ALLOW_UI_STATISTICS",
              "value": settings.allow_ui_statistics
            }
          ]
        },
        {
          "id": "section_ui_layout_footer",
          "element": "section",
          "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_LAYOUT_DESIGN",
          "icon": "fa-th-large",
          "onSave": {
            "type": "controller",
            "endpoint": "miscellanea/appearance",
            "method": "setVolumio3UI"
          },
          "hidden": false,
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [ "volumio3_ui" ]
          },
          "content": [
            {
              "id": "volumio3_ui",
              "element": "select",
              "doc": "TRANSLATE.APPEARANCE.USER_INTERFACE_LAYOUT_DESIGN_DOC",
              "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_LAYOUT_DESIGN",
              "value": v3_val,
              "options": v3_opts
            }
          ]
        }
      ]
    });

    resolve_translate_tokens(&mut out);
    out
}

/// **Settings → Appearance** — UI layout (manifest / contemporary / classic), then theme + wallpapers +
/// colours (`coreSection` **ui-settings**). Language only under **Settings → System**.
pub fn miscellanea_appearance_ui_config(active_layout: &str) -> Value {
    let v3_val = volumio3_ui_current_value(active_layout);
    let v3_opts = volumio3_ui_options_array();
    let mut out = json!({
      "page": { "label": "TRANSLATE.APPEARANCE.APPEARANCE" },
      "sections": [
        {
          "id": "volumio3_ui_section",
          "element": "section",
          "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_LAYOUT_DESIGN",
          "icon": "fa-th-large",
          "onSave": {
            "type": "controller",
            "endpoint": "miscellanea/appearance",
            "method": "setVolumio3UI"
          },
          "hidden": false,
          "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": [ "volumio3_ui" ]
          },
          "content": [
            {
              "id": "volumio3_ui",
              "element": "select",
              "doc": "TRANSLATE.APPEARANCE.USER_INTERFACE_LAYOUT_DESIGN_DOC",
              "label": "TRANSLATE.APPEARANCE.USER_INTERFACE_LAYOUT_DESIGN",
              "value": v3_val,
              "options": v3_opts
            }
          ]
        },
        {
          "id": "section_theme_background",
          "element": "section",
          "hidden": false,
          "coreSection": "ui-settings"
        }
      ]
    });
    resolve_translate_tokens(&mut out);
    out
}
