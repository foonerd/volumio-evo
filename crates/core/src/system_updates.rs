//! **System software updates** (OTA / packages). Stock UI emits **`updateCheck`** over Socket.IO.
//!
//! ## Planned editions (two paths)
//!
//! - **Open source** — community / self-build channel (mechanism TBD: e.g. apt, image, or signed bundle).
//! - **Paid / commercial** — separate entitlement or subscription-backed channel (same placeholder hook,
//!   different backend when implemented).
//!
//! Until those exist, [`placeholder_update_ready_json`] returns **`updateavailable: false`** and a short
//! explanation. Edition is selected by **`VOLUMIO_EVO_UPDATE_EDITION`** (`opensource` \| `paid`, default
//! open source).

use serde::Serialize;

/// Product line for future update routing (apt/image URLs, auth, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UpdateEdition {
    #[default]
    OpenSource,
    Paid,
}

impl UpdateEdition {
    /// `VOLUMIO_EVO_UPDATE_EDITION`: `opensource` (default) or `paid` (case-insensitive).
    pub fn from_env() -> Self {
        match std::env::var("VOLUMIO_EVO_UPDATE_EDITION")
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default()
            .as_str()
        {
            "paid" | "commercial" => Self::Paid,
            _ => Self::OpenSource,
        }
    }
}

/// Payload for **`updateReady`** / **`updateWaitMsg`**: matches stock Volumio2-UI updater modal shape
/// (`title`, `description`, `updateavailable`, optional `changeLogLink`).
#[derive(Debug, Serialize)]
pub struct UpdateReadyMessage {
    pub title: &'static str,
    pub description: &'static str,
    #[serde(rename = "updateavailable")]
    pub update_available: bool,
    #[serde(rename = "changeLogLink")]
    pub change_log_link: &'static str,
}

impl UpdateReadyMessage {
    pub fn placeholder(edition: UpdateEdition) -> Self {
        let description = match edition {
            UpdateEdition::OpenSource => {
                "Software updates are not wired up yet. Open-source builds will use one update path; \
                 commercial builds will use another. This device shows a placeholder until that lands."
            }
            UpdateEdition::Paid => {
                "Software updates are not wired up yet. Commercial builds will use the paid update path; \
                 open-source builds will use a separate channel. This is a placeholder until delivery is implemented."
            }
        };
        Self {
            title: "Software updates",
            description,
            update_available: false,
            change_log_link: "",
        }
    }

    pub fn checking() -> Self {
        Self {
            title: "Checking for updates",
            description: "Please wait…",
            update_available: false,
            change_log_link: "",
        }
    }
}

/// JSON object for **`emit('updateReady', …)`** (Angular updater modal).
pub fn placeholder_update_ready_json(edition: UpdateEdition) -> serde_json::Value {
    serde_json::to_value(UpdateReadyMessage::placeholder(edition)).unwrap_or_else(|_| {
        serde_json::json!({
            "title": "Software updates",
            "description": "Updates not available yet.",
            "updateavailable": false,
            "changeLogLink": ""
        })
    })
}
