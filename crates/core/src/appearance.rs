//! Stock UI **Appearance** port: theme name + wallpaper state in
//! [`crate::paths::backgrounds_data_dir`]/`state.toml`, layout in
//! [`crate::paths::ui_active_layout_overlay_path`]. Does not use Network
//! `config.toml.pending` or `volumio-evo-config-install`.

use serde_json::Value;

/// Result of reading a `theme` key from a `setBackgrounds` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedThemeField {
    /// No `theme` key, or JSON `null`.
    Absent,
    /// Sanitized theme id (e.g. `default`, `volumio`).
    Set(String),
    /// Key present but not a valid theme token.
    Invalid,
}

pub fn parse_theme_field(data: &Value) -> ParsedThemeField {
    let Some(raw) = data.get("theme") else {
        return ParsedThemeField::Absent;
    };
    if raw.is_null() {
        return ParsedThemeField::Absent;
    }
    match parse_theme_value(raw) {
        Some(s) => ParsedThemeField::Set(s),
        None => ParsedThemeField::Invalid,
    }
}

fn parse_theme_value(v: &Value) -> Option<String> {
    let token = if let Some(s) = v.as_str() {
        s
    } else if let Some(o) = v.as_object() {
        let inner = o.get("value").or_else(|| o.get("label"))?;
        inner.as_str()?
    } else {
        return None;
    };
    sanitize_theme_token(token)
}

fn sanitize_theme_token(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t.len() > 64 {
        return None;
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some(t.to_string())
}
