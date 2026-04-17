//! **Settings → Sources** (`miscellanea/my_music`) UI config for the stock Volumio2-UI plugin page.
//! Mirrors `volumio3-backend/app/plugins/miscellanea/my_music/UIConfig.json` so `getUiConfig` can
//! render the same sections as Node (core `my-music`, `network-drives`, declarative album-art / library rows).
//!
//! **Translations:** Stock `UIConfig.json` uses values like `TRANSLATE.APPEARANCE.ALBUMART_SETTINGS`.
//! The Angular plugin template prints `{{ item.label }}` **without** `| translate`, so the **browser
//! never resolves** those tokens. Node fixes this by running
//! [`CoreCommandRouter.prototype.i18nJson`](https://github.com/volumio/volumio3-backend/blob/master/app/index.js)
//! (`translateKeys`) **before** `pushUiConfig`. Evo does the same using embedded `strings_en.json`.
//!
//! **Which `strings_en.json`?** Classic, contemporary, and manifest each ship
//! `layer/web/<theme>/app/i18n/strings_*.json`. For `COMMON` and `APPEARANCE`, those files are kept
//! in sync across themes, so embedding **any one** theme’s `strings_en.json` is equivalent for
//! `TRANSLATE.*` resolution. (Manifest’s slimmer `locale-*.json` bundles are **only** for client-side
//! Angular `translate`; they are not the dictionary for this server-side step.)

use serde_json::Value;
use std::sync::OnceLock;

static STRINGS_EN: OnceLock<Value> = OnceLock::new();
static MY_MUSIC_UI_I18N: OnceLock<Value> = OnceLock::new();

fn strings_en_parsed() -> &'static Value {
    STRINGS_EN.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../../../../layer/web/classic/app/i18n/strings_en.json"
        ))
        .expect("layer/web/classic/app/i18n/strings_en.json must be valid JSON")
    })
}

/// Same algorithm as volumio3-backend `translateKeys`: replace `TRANSLATE.Category.key` using
/// `dictionary[Category][key]`, falling back to `defaultDictionary`, then the original string.
fn translate_keys_in_value(v: &mut Value, dictionary: &Value, default_dictionary: &Value) {
    match v {
        Value::Object(map) => {
            for (_, child) in map.iter_mut() {
                translate_keys_in_value(child, dictionary, default_dictionary);
            }
        }
        Value::Array(arr) => {
            for child in arr.iter_mut() {
                translate_keys_in_value(child, dictionary, default_dictionary);
            }
        }
        Value::String(s) => {
            if let Some(replaced) = translate_one_translate_token(s, dictionary, default_dictionary) {
                *v = Value::String(replaced);
            }
        }
        _ => {}
    }
}

fn translate_one_translate_token(
    s: &str,
    dictionary: &Value,
    default_dictionary: &Value,
) -> Option<String> {
    const PREFIX: &str = "TRANSLATE.";
    if !s.starts_with(PREFIX) {
        return None;
    }
    let path = s.strip_prefix(PREFIX)?;
    let resolved = if let Some(dot) = path.find('.') {
        let category = &path[..dot];
        let key = &path[dot + 1..];
        lookup_category_key(dictionary, category, key)
            .filter(|t| !t.is_empty())
            .or_else(|| lookup_category_key(default_dictionary, category, key))
    } else {
        dictionary
            .get(path)
            .and_then(|x| x.as_str())
            .filter(|t| !t.is_empty())
            .or_else(|| default_dictionary.get(path).and_then(|x| x.as_str()))
    };
    Some(resolved.unwrap_or(s).to_string())
}

fn lookup_category_key<'a>(d: &'a Value, category: &str, key: &str) -> Option<&'a str> {
    d.get(category)?.get(key)?.as_str()
}

/// Stock Sources page: `pushUiConfig` payload matching Node `getUIConfig` for `miscellanea/my_music`,
/// with **`TRANSLATE.*` resolved** to English (same as Node when language is `en`).
pub fn my_music_ui_config() -> serde_json::Value {
    MY_MUSIC_UI_I18N
        .get_or_init(|| {
            let mut v: Value = serde_json::from_str(include_str!("my_music_ui_config.json"))
                .expect("embedded my_music_ui_config.json must be valid JSON");
            let en = strings_en_parsed();
            translate_keys_in_value(&mut v, en, en);
            v
        })
        .clone()
}
