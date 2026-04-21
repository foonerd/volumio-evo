// volumio-evo-kiosk-browser - purpose-built WebKit kiosk shell for Volumio Evo.
//
// One maximized, undecorated GTK 4 ApplicationWindow containing one
// WebKit.WebView, loading one URL. Blocks every new-window request
// (window.open, target=_blank, middle-click), every context menu, every
// popup. WebProcess crashes trigger a reload.
//
// Replaces the previous Python shell; Evo is a Rust+WASM project and the
// Python dependency was a footgun across installers. This binary is built
// by `cargo build -p volumio-evo-kiosk-browser --release` on the target
// during `layer/kiosk-wpe/install.sh`, or from a prebuilt in
// `layer/binaries/<triple>/` when present.
//
// Design notes mirror the WPE/cog rationale in docs/KIOSK.md:
//
// 1. Why webkit2gtk 6.0 and not WPE:
//    WPE WebKit 2.48.3 on Debian Trixie does not dispatch pointer button
//    or keyboard events to the DOM on Pi 5 class hardware. libinput
//    delivers events at the kernel layer but WPE's wl/fdo platform
//    plugins drop them before WebCore. webkit2gtk (same engine family,
//    GTK platform path) delivers every event correctly on the same
//    hardware.
//
// 2. Why maximize() and not fullscreen():
//    Squeekboard's layer-shell surface lives on LAYER_TOP. wlroots'
//    layer ordering places "fullscreen windows" ABOVE LAYER_TOP, so a
//    true-fullscreen kiosk client covers the OSK entirely. Under labwc
//    the recommended workaround is to maximize instead. Combined with
//    set_decorated(false) and the labwc <decoration>client</decoration>
//    policy in rc.xml, maximize is visually identical to fullscreen.
//    See labwc issue #2926 and raspberrypi-ui/squeekboard #13.
//
// Environment contract with the launcher:
//
//   KIOSK_URL    URL to load (default http://127.0.0.1/ if argv missing)
//   KIOSK_ZOOM   WebKit zoom level as float string, e.g. "1.2". Default
//                1.0 if unset or unparseable. Maps to
//                WebKitWebView::set_zoom_level(), which affects the CSS
//                viewport reported to the page (so Bootstrap media
//                queries shift, matching Chromium
//                --force-device-scale-factor behaviour).
//
// CLI:
//   volumio-evo-kiosk-browser [URL]
//     If URL is omitted, falls back to $KIOSK_URL, else http://127.0.0.1/.

use std::env;

use gio::prelude::*;
use gio::ApplicationFlags;
use glib::ControlFlow;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow};
use webkit6::prelude::*;
use webkit6::{PolicyDecisionType, WebView};

const APP_ID: &str = "io.volumio.evo.kiosk";
const DEFAULT_URL: &str = "http://127.0.0.1/";
const FALLBACK_WIDTH: i32 = 1280;
const FALLBACK_HEIGHT: i32 = 720;
const RELOAD_DELAY_MS: u32 = 1000;
const MAXIMIZE_RETRY_MS: u32 = 200;

fn log_line(msg: &str) {
    // systemd journal: keep the prefix the launcher + session use so grep
    // across [kiosk-launch] / [kiosk-session] / [kiosk-browser] lines up.
    println!("[kiosk-browser] {msg}");
}

fn resolve_url(argv: &[String]) -> String {
    if argv.len() >= 2 && !argv[1].is_empty() {
        return argv[1].clone();
    }
    env::var("KIOSK_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_URL.to_string())
}

fn resolve_zoom() -> f64 {
    // KIOSK_ZOOM may be absent, "auto", or a float. Default 1.0 on
    // absent / "auto" / parse error.
    let raw = match env::var("KIOSK_ZOOM") {
        Ok(v) => v,
        Err(_) => return 1.0,
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return 1.0;
    }
    match trimmed.parse::<f64>() {
        Ok(v) if v.is_finite() && v > 0.0 => v.clamp(0.25, 5.0),
        _ => {
            log_line(&format!("KIOSK_ZOOM='{raw}' not a positive float; using 1.0"));
            1.0
        }
    }
}

fn build_ui(app: &Application, url: &str, zoom: f64) {
    let window = ApplicationWindow::builder()
        .application(app)
        .decorated(false)
        .title("Volumio")
        .default_width(FALLBACK_WIDTH)
        .default_height(FALLBACK_HEIGHT)
        .build();

    let webview = WebView::new();
    // Without expand=true the WebView renders at its minimum-content size
    // and the window ends up as a small rectangle.
    webview.set_hexpand(true);
    webview.set_vexpand(true);
    // GTK focus is required for GtkIMContext -> text-input-v3 -> OSK.
    webview.set_focusable(true);

    // Zoom level. WebKit applies this both to raster (everything bigger)
    // AND to the reported CSS viewport (Bootstrap media queries shift).
    // Equivalent to Chromium's --force-device-scale-factor in terms of
    // what the page sees, which is what the legacy Node kiosk used.
    if (zoom - 1.0).abs() > f64::EPSILON {
        log_line(&format!("set_zoom_level({zoom})"));
        webview.set_zoom_level(zoom);
    }

    webview.load_uri(url);

    // Block every new-window request (window.open, target=_blank). webkit6's
    // connect_create binding requires a GtkWidget; refusing is done via
    // decide-policy on NewWindowAction instead.
    webview.connect_decide_policy(|_webview, decision, dtype| {
        if dtype == PolicyDecisionType::NewWindowAction {
            log_line("blocked new window (popup / target=_blank / window.open)");
            decision.ignore();
            true
        } else {
            false
        }
    });

    // Suppress the browser-default context menu on right-click / long press.
    webview.connect_context_menu(|_webview, _menu, _hit| {
        // Returning true tells WebKit the signal was handled; the menu
        // is suppressed.
        true
    });

    // Reload the page if the WebProcess dies (GPU reset, OOM, etc).
    let webview_for_terminate = webview.clone();
    let url_for_terminate = url.to_string();
    webview.connect_web_process_terminated(move |_webview, reason| {
        log_line(&format!(
            "WebProcess terminated ({reason:?}); reloading in {RELOAD_DELAY_MS}ms"
        ));
        let webview = webview_for_terminate.clone();
        let url = url_for_terminate.clone();
        glib::timeout_add_local(
            std::time::Duration::from_millis(RELOAD_DELAY_MS as u64),
            move || {
                webview.load_uri(&url);
                ControlFlow::Break
            },
        );
    });

    window.set_child(Some(&webview));

    // Re-assert maximize if the compositor ever unmaximizes us (e.g.
    // external display hotplug). Plain clone into the closure is
    // sufficient; GtkWindow holds an internal reference.
    let window_for_notify = window.clone();
    window.connect_maximized_notify(move |w| {
        if !w.is_maximized() {
            log_line("window unmaximized; re-requesting");
            let w2 = window_for_notify.clone();
            glib::timeout_add_local(
                std::time::Duration::from_millis(MAXIMIZE_RETRY_MS as u64),
                move || {
                    w2.maximize();
                    ControlFlow::Break
                },
            );
        }
    });

    window.present();

    // xdg_toplevel.set_maximized only takes effect after the surface is
    // mapped. Defer one idle tick so the compositor has accepted the
    // surface.
    let window_for_maximize = window.clone();
    glib::idle_add_local_once(move || {
        log_line("requesting maximize");
        window_for_maximize.maximize();
    });

    // GTK focus must be on the WebView widget itself so keyboard events
    // and IM state traverse through WebKit's input path.
    let webview_for_focus = webview.clone();
    glib::idle_add_local_once(move || {
        log_line("granting GTK focus to WebView");
        webview_for_focus.grab_focus();
    });
}

fn main() -> glib::ExitCode {
    let argv: Vec<String> = env::args().collect();
    let url = resolve_url(&argv);
    let zoom = resolve_zoom();
    log_line(&format!("starting; url={url} zoom={zoom}"));

    // NON_UNIQUE avoids D-Bus registration of the application id. systemd
    // already guarantees single-instance via the unit; registration can
    // fail in the PAM-login session context where the user session bus is
    // minimal.
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(ApplicationFlags::NON_UNIQUE)
        .build();

    let url_for_activate = url.clone();
    app.connect_activate(move |app| {
        build_ui(app, &url_for_activate, zoom);
    });

    // Pass only argv[0] to GApplication so it does not try to parse our
    // URL as a GApplication command-line option.
    let argv0 = argv
        .first()
        .cloned()
        .unwrap_or_else(|| "volumio-evo-kiosk-browser".to_string());
    app.run_with_args(&[argv0])
}
