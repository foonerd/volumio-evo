//! **Settings → Network** (`system_controller/network`) UI config for stock Volumio2-UI.
//! Mirrors `volumio3-backend/app/plugins/system_controller/network/UIConfig.json`; `TRANSLATE.*`
//! is resolved like [`super::sources_ui::my_music_ui_config`].

use serde_json::Value;
use std::sync::OnceLock;

use crate::config::Config;
use crate::network_config::{
    default_hotspot_ssid_from_mac, wifi_ap_psk_configured, NetworkIntent, Ipv4Mode, WifiRole,
};

static NETWORK_UI_I18N: OnceLock<Value> = OnceLock::new();

/// Stock Network page: `pushUiConfig` for `getUiConfig` with `page: system_controller/network`.
pub fn network_settings_ui_config() -> Value {
    NETWORK_UI_I18N
        .get_or_init(|| {
            let mut v: Value = serde_json::from_str(include_str!("network_ui_config.json"))
                .expect("embedded network_ui_config.json must be valid JSON");
            super::sources_ui::resolve_translate_tokens(&mut v);
            v
        })
        .clone()
}

/// Same as [`network_settings_ui_config`], with **field `value`s** filled from persisted
/// [`NetworkIntent::load`] so the form is not empty on first open.
pub fn network_settings_ui_config_merged() -> Value {
    let mut v = network_settings_ui_config();
    let intent = NetworkIntent::load();
    merge_network_intent_into_ui(&mut v, &intent);
    v
}

/// Like [`network_settings_ui_config_merged`], plus a **Preferred Wi-Fi interface** section when
/// NetworkManager reports more than one `wifi` device (excludes `p2p-dev-*`). Second return is
/// **`true`** when the UI should show a one-time info modal (multi-radio and no preference file yet).
pub async fn network_settings_ui_config_merged_enriched(config: &Config) -> (Value, bool) {
    let mut v = network_settings_ui_config_merged();
    let Ok(rows) = crate::nm_network::nm_device_table().await else {
        return (v, false);
    };
    let mut wifi_ifaces: Vec<String> = rows
        .into_iter()
        .filter(|r| {
            r.kind.eq_ignore_ascii_case("wifi")
                && !r.device.trim().starts_with("p2p-dev-")
                && !r.device.trim().is_empty()
        })
        .map(|r| r.device.trim().to_string())
        .collect();
    // Filter: only offer **STA-capable** radios as "Preferred Wi-Fi interface" — exclude virtual
    // AP vifs we create on the same PHY (e.g. `ap0` via `iw dev wlan0 interface add ap0 type __ap`).
    // Uses `iw dev`/`iw phy` capability probe (see `crate::wifi_phy::is_sta_capable`).
    let mut sta_only: Vec<String> = Vec::with_capacity(wifi_ifaces.len());
    for ifn in wifi_ifaces.drain(..) {
        if crate::wifi_phy::is_sta_capable(&ifn).await {
            sta_only.push(ifn);
        }
    }
    if sta_only.len() <= 1 {
        // Only one real STA radio (plus any AP vif) — no need for the preferred-iface picker.
        return (v, false);
    }
    let prompt_modal = crate::network_config::read_wifi_iface_preferred().is_none();
    let current = crate::nm_network::resolve_effective_wifi_iface(config).await;
    merge_preferred_wifi_iface_section(&mut v, &sta_only, &current);
    // Static sections are translated once at load-time via `OnceLock` (see `network_settings_ui_config`).
    // The dynamic preferred-iface section we just inserted contains fresh `TRANSLATE.*` tokens —
    // resolve them now so the UI template (`{{section.saveButton.label}}`, no `| translate`) renders
    // localised text, matching what Node's `CoreCommandRouter.translateKeys` does before `pushUiConfig`.
    super::sources_ui::resolve_translate_tokens(&mut v);
    (v, prompt_modal)
}

/// Stock-style **`openModal`** payload: explains preferred-iface section when multiple radios exist.
pub fn preferred_wifi_iface_info_modal_payload() -> Value {
    serde_json::json!({
        "title": "Preferred Wi-Fi interface",
        "message": "This device has more than one Wi-Fi radio. Choose the preferred interface in the new section above and save. It is stored under settings and merged into /etc/volumio-evo/config.toml when allowed.",
        "size": "md",
        "buttons": [
            {
                "name": "OK",
                "class": "btn btn-info",
                "emit": "closeModals",
                "payload": ""
            }
        ]
    })
}

fn merge_preferred_wifi_iface_section(v: &mut Value, ifaces: &[String], current: &str) {
    let Some(sections) = v.get_mut("sections").and_then(|s| s.as_array_mut()) else {
        return;
    };
    let options: Vec<Value> = ifaces
        .iter()
        .map(|name| serde_json::json!({"value": name, "label": name}))
        .collect();
    let mut pick = current.to_string();
    if !ifaces.iter().any(|i| i == &pick) {
        pick = ifaces.first().cloned().unwrap_or_default();
    }
    let section = serde_json::json!({
        "id": "section_preferred_wifi_iface",
        "element": "section",
        "label": "Preferred Wi-Fi interface",
        "doc": "Multiple wireless interfaces detected. Select which radio to use for client (STA) scan and connection.",
        "icon": "fa-wifi",
        "onSave": {"type": "controller", "endpoint": "system_controller/network", "method": "savePreferredWifiIface"},
        "saveButton": {
            "label": "TRANSLATE.COMMON.SAVE",
            "data": ["preferred_wifi_iface"]
        },
        "content": [{
            "id": "preferred_wifi_iface",
            "element": "select",
            "label": "Interface",
            "value": {"value": pick, "label": pick},
            "options": options
        }]
    });
    if let Some(idx) = sections.iter().position(|s| {
        s.get("id").and_then(|x| x.as_str()) == Some("section_wireless_network")
    }) {
        sections.insert(idx, section);
    } else {
        sections.push(section);
    }
}

/// Extract `preferred_wifi_iface` from **`callMethod.data`** (string or select `{ value, label }`).
pub fn parse_preferred_wifi_iface_field(data: &Value) -> Option<String> {
    let v = data.get("preferred_wifi_iface")?;
    if let Some(s) = v.as_str() {
        let t = s.trim();
        return (!t.is_empty()).then(|| t.to_string());
    }
    if let Some(s) = v.get("value").and_then(|x| x.as_str()) {
        let t = s.trim();
        return (!t.is_empty()).then(|| t.to_string());
    }
    None
}

fn merge_network_intent_into_ui(v: &mut Value, intent: &NetworkIntent) {
    let Some(sections) = v.get_mut("sections").and_then(|s| s.as_array_mut()) else {
        return;
    };
    for section in sections.iter_mut() {
        let id = section.get("id").and_then(|x| x.as_str()).unwrap_or("");
        match id {
            "section_wired_network" => merge_wired_section(section, intent),
            "section_wireless_network" => merge_wireless_section(section, intent),
            "section_hotspot" => merge_hotspot_section(section, intent),
            _ => {}
        }
    }
}

fn set_field_value(section: &mut Value, field_id: &str, value: Value) {
    let Some(content) = section.get_mut("content").and_then(|c| c.as_array_mut()) else {
        return;
    };
    for item in content.iter_mut() {
        if item.get("id").and_then(|x| x.as_str()) == Some(field_id) {
            if let Some(obj) = item.as_object_mut() {
                obj.insert("value".to_string(), value);
            }
            break;
        }
    }
}

fn merge_wired_section(section: &mut Value, intent: &NetworkIntent) {
    let eth = &intent.ethernet;
    set_field_value(section, "ethernet_enabled", Value::Bool(eth.enabled));
    let dhcp = matches!(eth.ipv4_mode, Ipv4Mode::Dhcp);
    set_field_value(section, "dhcp", Value::Bool(dhcp));
    if !dhcp {
        let addr = eth.ipv4_address.trim();
        if let Some((ip, mask)) = split_ipv4_cidr_for_ui(addr) {
            set_field_value(section, "static_ip", Value::String(ip));
            set_field_value(section, "static_netmask", Value::String(mask));
        } else if !addr.is_empty() {
            set_field_value(section, "static_ip", Value::String(addr.to_string()));
        }
        let gw = eth.ipv4_gateway.trim();
        if !gw.is_empty() {
            set_field_value(section, "static_gateway", Value::String(gw.to_string()));
        }
    }
}

fn merge_wireless_section(section: &mut Value, intent: &NetworkIntent) {
    let wifi = &intent.wifi;
    let enabled = !matches!(wifi.role, WifiRole::Disabled);
    set_field_value(section, "wireless_enabled", Value::Bool(enabled));
    let dhcp = matches!(wifi.sta_ipv4_mode, Ipv4Mode::Dhcp);
    set_field_value(section, "wireless_dhcp", Value::Bool(dhcp));
    if !dhcp {
        let addr = wifi.sta_ipv4_address.trim();
        if let Some((ip, mask)) = split_ipv4_cidr_for_ui(addr) {
            set_field_value(section, "wireless_static_ip", Value::String(ip));
            set_field_value(section, "wireless_static_netmask", Value::String(mask));
        } else if !addr.is_empty() {
            set_field_value(section, "wireless_static_ip", Value::String(addr.to_string()));
        }
        let gw = wifi.sta_ipv4_gateway.trim();
        if !gw.is_empty() {
            set_field_value(section, "wireless_static_gateway", Value::String(gw.to_string()));
        }
    }
}

fn merge_hotspot_section(section: &mut Value, intent: &NetworkIntent) {
    let fb = &intent.fallback;
    let wifi = &intent.wifi;
    set_field_value(section, "enable_hotspot", Value::Bool(fb.hotspot_enabled));
    set_field_value(section, "hotspot_fallback", Value::Bool(fb.hotspot_fallback));
    let name = wifi.ap_ssid.trim();
    let name = if name.is_empty() {
        default_hotspot_ssid_from_mac()
    } else {
        name.to_string()
    };
    set_field_value(section, "hotspot_name", Value::String(name));
    set_field_value(
        section,
        "hotspot_protection",
        Value::Bool(wifi_ap_psk_configured()),
    );
    set_field_value(section, "hotspot_password", Value::String(String::new()));
    let ch = wifi.ap_channel.clamp(1, 11);
    set_field_value(
        section,
        "hotspot_channel",
        serde_json::json!({ "value": ch, "label": ch.to_string() }),
    );
}

/// Split `addr` like `192.168.1.10/24` into IP and dotted netmask for the legacy UI fields.
fn split_ipv4_cidr_for_ui(addr: &str) -> Option<(String, String)> {
    let addr = addr.trim();
    let (ip, pref_s) = addr.split_once('/')?;
    let ip = ip.trim();
    if ip.is_empty() {
        return None;
    }
    let p: u8 = pref_s.trim().parse().ok()?;
    let netmask = prefix_to_dotted_netmask(p)?;
    Some((ip.to_string(), netmask))
}

/// Dotted netmask → prefix length (e.g. `255.255.255.0` → 24).
fn netmask_dotted_to_prefix(netmask: &str) -> Option<u8> {
    let mut parts = netmask.trim().split('.');
    let a: u8 = parts.next()?.parse().ok()?;
    let b: u8 = parts.next()?.parse().ok()?;
    let c: u8 = parts.next()?.parse().ok()?;
    let d: u8 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let m = u32::from_be_bytes([a, b, c, d]);
    if m == 0 {
        return Some(0);
    }
    Some(m.leading_ones() as u8)
}

fn prefix_to_dotted_netmask(prefix: u8) -> Option<String> {
    if prefix > 32 {
        return None;
    }
    let mask: u32 = if prefix == 0 {
        0
    } else {
        !((1u32 << (32 - u32::from(prefix))) - 1)
    };
    let o = [
        ((mask >> 24) & 0xff) as u8,
        ((mask >> 16) & 0xff) as u8,
        ((mask >> 8) & 0xff) as u8,
        (mask & 0xff) as u8,
    ];
    Some(format!("{}.{}.{}.{}", o[0], o[1], o[2], o[3]))
}

/// Stock **`saveWiredNet`** (`callMethod.data`): maps **`ethernet_enabled`** → [`EthernetIntent::enabled`].
#[derive(Debug)]
pub enum ApplyWiredNetError {
    StaticIpOrNetmaskMissing,
    InvalidNetmask,
}

/// Apply **Wired** section fields to `intent` (caller saves + [`crate::nm_network::apply_network_intent_exclusive`]).
pub fn apply_wired_net_form_to_intent(
    intent: &mut NetworkIntent,
    data: &Value,
) -> Result<(), ApplyWiredNetError> {
    let enabled = match data.get("ethernet_enabled") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };
    intent.ethernet.enabled = enabled;
    if !enabled {
        return Ok(());
    }

    let dhcp = match data.get("dhcp") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };
    if dhcp {
        intent.ethernet.ipv4_mode = Ipv4Mode::Dhcp;
    } else {
        intent.ethernet.ipv4_mode = Ipv4Mode::Static;
        let ip = data
            .get("static_ip")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let mask = data
            .get("static_netmask")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let gw = data
            .get("static_gateway")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if ip.is_empty() || mask.is_empty() {
            return Err(ApplyWiredNetError::StaticIpOrNetmaskMissing);
        }
        let prefix = netmask_dotted_to_prefix(mask).ok_or(ApplyWiredNetError::InvalidNetmask)?;
        intent.ethernet.ipv4_address = format!("{ip}/{prefix}");
        intent.ethernet.ipv4_gateway = gw.to_string();
    }
    Ok(())
}

/// `true` when the UI should show the static-IP warning modal (wired: DHCP off, not yet confirmed).
pub fn wired_net_needs_static_confirm(data: &Value) -> bool {
    let eth_on = match data.get("ethernet_enabled") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };
    if !eth_on {
        return false;
    }
    let dhcp = match data.get("dhcp") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };
    let confirm = json_truthy(data.get("confirm"));
    !dhcp && !confirm
}

/// Payload for **`openModal`** matching stock `saveWiredNet` static-IP confirmation.
pub fn wired_static_confirm_modal_payload(data: &Value) -> Value {
    let mut confirm_data = data.clone();
    if let Some(o) = confirm_data.as_object_mut() {
        o.insert("confirm".to_string(), Value::Bool(true));
    }
    serde_json::json!({
        "title": "Static IP",
        "message": "Using a static IP may disconnect this device if the values are wrong for your network. Continue?",
        "size": "lg",
        "buttons": [
            {
                "name": "Cancel",
                "class": "btn btn-cancel",
                "emit": "closeModals",
                "payload": ""
            },
            {
                "name": "Continue",
                "class": "btn btn-info",
                "emit": "callMethod",
                "payload": {
                    "endpoint": "system_controller/network",
                    "method": "saveWiredNet",
                    "data": confirm_data
                }
            }
        ]
    })
}

/// Stock **`saveWirelessNet`** (`callMethod.data`): maps **`wireless_enabled`** to [`WifiRole`] — single
/// source of truth for **Wi‑Fi as client (STA)** vs off.
#[derive(Debug)]
pub enum ApplyWirelessNetError {
    StaticIpOrNetmaskMissing,
    InvalidNetmask,
}

/// `true` when the UI should show the static-IP warning modal (stock: DHCP off, not yet confirmed).
pub fn wireless_net_needs_static_confirm(data: &Value) -> bool {
    let dhcp = match data.get("wireless_dhcp") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };
    let confirm = json_truthy(data.get("confirm"));
    !dhcp && !confirm
}

/// Payload for **`openModal`** matching stock `saveWirelessNet` static-IP confirmation.
pub fn wireless_static_confirm_modal_payload(data: &Value) -> Value {
    let mut confirm_data = data.clone();
    if let Some(o) = confirm_data.as_object_mut() {
        o.insert("confirm".to_string(), Value::Bool(true));
    }
    serde_json::json!({
        "title": "Static IP",
        "message": "Using a static IP may disconnect this device if the values are wrong for your network. Continue?",
        "size": "lg",
        "buttons": [
            {
                "name": "Cancel",
                "class": "btn btn-cancel",
                "emit": "closeModals",
                "payload": ""
            },
            {
                "name": "Continue",
                "class": "btn btn-info",
                "emit": "callMethod",
                "payload": {
                    "endpoint": "system_controller/network",
                    "method": "saveWirelessNet",
                    "data": confirm_data
                }
            }
        ]
    })
}

/// Stock Socket.IO **`saveWirelessNetworkSettings`** (and wizard Wi‑Fi join): `ssid`, `password`, `security` or `encryption`.
/// Sets STA mode and SSID; use [`wifi_join_security_is_open`] + PSK sidecar for auth.
pub fn apply_wireless_sta_join_payload(
    intent: &mut NetworkIntent,
    data: &Value,
) -> Result<(), &'static str> {
    let ssid = data.get("ssid").and_then(|v| v.as_str()).unwrap_or("").trim();
    if ssid.is_empty() {
        return Err("missing_ssid");
    }
    let security = data
        .get("security")
        .or_else(|| data.get("encryption"))
        .and_then(|v| v.as_str())
        .unwrap_or("open");

    intent.wifi.role = WifiRole::Sta;
    intent.wifi.sta_ssid = ssid.to_string();
    intent.wifi.sta_open = wifi_join_security_is_open(security);
    Ok(())
}

/// `open` / empty → no WPA; anything else (e.g. `wpa2`, `wpa3`, `wep`) → use PSK sidecar unless open network.
pub fn wifi_join_security_is_open(security: &str) -> bool {
    let s = security.trim().to_ascii_lowercase();
    s.is_empty() || s == "open"
}

/// Apply **Wireless** section fields to `intent` (caller saves + [`crate::nm_network::apply_network_intent_exclusive`]).
pub fn apply_wireless_net_form_to_intent(
    intent: &mut NetworkIntent,
    data: &Value,
) -> Result<(), ApplyWirelessNetError> {
    let enabled = match data.get("wireless_enabled") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };
    let dhcp = match data.get("wireless_dhcp") {
        None => true,
        Some(v) => json_truthy(Some(v)),
    };

    intent.wifi.role = if enabled {
        WifiRole::Sta
    } else {
        WifiRole::Disabled
    };

    if dhcp {
        intent.wifi.sta_ipv4_mode = Ipv4Mode::Dhcp;
    } else {
        intent.wifi.sta_ipv4_mode = Ipv4Mode::Static;
        let ip = data
            .get("wireless_static_ip")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let mask = data
            .get("wireless_static_netmask")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let gw = data
            .get("wireless_static_gateway")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if ip.is_empty() || mask.is_empty() {
            return Err(ApplyWirelessNetError::StaticIpOrNetmaskMissing);
        }
        let prefix = netmask_dotted_to_prefix(mask).ok_or(ApplyWirelessNetError::InvalidNetmask)?;
        intent.wifi.sta_ipv4_address = format!("{ip}/{prefix}");
        intent.wifi.sta_ipv4_gateway = gw.to_string();
    }
    Ok(())
}

/// Stock **saveHotspotSettings** payload (`callMethod.data`).
#[derive(Debug)]
pub enum ApplyHotspotFormError {
    PasswordTooShort,
}

/// Update `intent` fields from the UI save payload. Caller writes AP PSK sidecar and calls
/// [`crate::nm_network::apply_network_intent_exclusive`].
pub fn apply_hotspot_form_to_intent(
    intent: &mut NetworkIntent,
    data: &Value,
) -> Result<(), ApplyHotspotFormError> {
    let enable = json_truthy(data.get("enable_hotspot"));
    let fallback = json_truthy(data.get("hotspot_fallback"));
    let name = data
        .get("hotspot_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let protection = json_truthy(data.get("hotspot_protection"));
    let password = data
        .get("hotspot_password")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if protection && password.len() < 8 {
        return Err(ApplyHotspotFormError::PasswordTooShort);
    }

    intent.fallback.hotspot_enabled = enable;
    intent.fallback.hotspot_fallback = fallback;
    intent.wifi.ap_ssid = if name.is_empty() {
        default_hotspot_ssid_from_mac()
    } else {
        name
    };
    let ch = parse_hotspot_channel(data.get("hotspot_channel"));
    intent.wifi.ap_channel = ch.clamp(1, 11);
    Ok(())
}

fn json_truthy(v: Option<&Value>) -> bool {
    match v {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true") || s == "1",
        Some(Value::Number(n)) => n.as_i64().map(|i| i != 0).unwrap_or(false),
        _ => false,
    }
}

fn parse_hotspot_channel(v: Option<&Value>) -> u32 {
    let Some(v) = v else {
        return 4;
    };
    if let Some(n) = v.as_u64() {
        return n as u32;
    }
    if let Some(s) = v.as_str() {
        return s.parse().unwrap_or(4);
    }
    if let Some(obj) = v.as_object() {
        if let Some(l) = obj.get("label").and_then(|x| x.as_str()) {
            return l.parse().unwrap_or(4);
        }
        if let Some(n) = obj.get("value").and_then(|x| x.as_u64()) {
            return n as u32;
        }
    }
    4
}
