# Logs, journald, and filtering

Evo uses **`tracing`** with **`tracing-subscriber`** (env filter + stderr). This page describes what you see in **`journalctl`** and how to grep it reliably.

## Two layers of markers

1. **Process-wide line prefix** — Every formatted line written by the subscriber starts with **`[EVO] `** (see `crates/core/src/evo_log_fmt.rs`, constant `log_tags::EVO_LINE`). That includes dependency crates (**`tower_http`**, **`mpd_protocol`**, …), not only Evo’s own modules.
2. **Domain tags inside the message** — Evo code prefixes many messages with stable anchors such as **`EVO VOLUME -->`**, **`EVO QUEUE -->`**, **`EVO BOOT -->`**, defined in **`crates/core/src/log_tags.rs`**. Use these to narrow by subsystem.

## Configuration precedence

1. **`RUST_LOG`** — If set in the environment (e.g. systemd `Environment=`), it is the full **`tracing_subscriber::EnvFilter`** directive used for **tracing** and overrides the filter derived from config (see `main.rs`: `EnvFilter::try_from_default_env()`).

2. **`VOLUMIO_EVO_LOG_LEVEL`** — Read in **`config::load()`**. If set and valid, it **replaces** **`log_level`** from **`config.toml`** in the in-memory **`Config`** (values: `error`, `warn`, `info`, `verbose`, `debug`, `trace`). This affects the boot log line and any code that reads **`config.log_level`**. It does **not** override **`RUST_LOG`** for the tracing filter.

3. **`log_level`** in **`config.toml`** — Used when **`VOLUMIO_EVO_LOG_LEVEL`** is unset, and feeds the default tracing filter when **`RUST_LOG`** is unset: **`config.log_level.env_filter_directive()`**.

The example config documents this in **`layer/config/volumio-evo.toml.example`**.

## Default filter (when `RUST_LOG` is unset)

[`LogLevel::env_filter_directive`](../crates/core/src/config.rs) adds dependency-specific clamps so **`log_level = "info"`** stays readable. In particular **`mpd_protocol=warn`** hides per-connection **INFO** lines (`connected successfully`) from the MPD client stack, which would otherwise appear on every MPD connect opened by the **`pushState`** poll (default **2s** interval). Override explicitly if needed, e.g. **`RUST_LOG=info,mpd_protocol=info`**.

## Examples

```bash
# All Evo process lines (fixed string; safe with grep -F)
journalctl -u volumio-evo -n 200 --no-pager | grep -F '[EVO]'

# Only volume-related Evo messages (domain tag)
journalctl -u volumio-evo -n 200 --no-pager | grep -F 'EVO VOLUME -->'

# Follow live
journalctl -u volumio-evo -f | grep -F '[EVO]'
```

**Note:** Dependency crates log under their own targets (e.g. `mpd_protocol::connection`). Use **`RUST_LOG`** to tune targets; setting **`RUST_LOG=info`** alone replaces the whole default directive (including the **`mpd_protocol=warn`** clamp), so connection spam can return unless you add **`mpd_protocol=warn`** again.

## Playback state (`pushState`) — facts, not guesses

Phased product requirements (elapsed UI, progress sync, traffic limits) are in **`PLAYBACK_STATE_REQUIREMENTS.md`**.

At **`DEBUG`**, Evo logs **exactly** what it built from MPD and whether delivery succeeded:

| Anchor | What you see |
|--------|----------------|
| **`EVO PUSHSTATE -->`** | `status`, **`seek_ms`**, **`duration_s`**, `position`, `volume`, and for Socket.IO **`emit=ok`** or **`emit=err`**. |

Sources include: **broadcast** poll (`io.emit` to all clients), **Socket.IO** `getState` / `getQueue` handlers, and **REST** `GET /api/v1/getState` / `getQueue` (body snapshot / `queue_len`).

**`WARN`** lines mean a hard failure: MPD fetch failed, or **`emit`** failed (encode / socket closed / buffer full — error text is logged).

```bash
journalctl -u volumio-evo -f --no-pager | grep -F 'EVO PUSHSTATE -->'
```

Enable with **`log_level = "verbose"`** or **`debug`**, or e.g. **`RUST_LOG=volumio_evo_core=debug`** when **`RUST_LOG`** overrides the file.

## Volume changes in the journal

Successful UI/socket volume applies log a line containing **`EVO VOLUME -->`** and **`volume applied`** at **`DEBUG`** (to avoid journal noise when dragging the slider). At default **`log_level = "info"`** they do not appear; use **`log_level = "verbose"`** (Evo crates at **debug**), **`log_level = "debug"`**, or a targeted **`RUST_LOG`** (e.g. `RUST_LOG=volumio_evo_core=debug`) to see them. Failures (ALSA / MPD **`setvol`**) remain **`WARN`**.

Startup / bootstrap volume messages (e.g. default startup volume from Playback options) stay **`INFO`** where they are one-shot events.

## Assumptions

- Service runs under **systemd** (see **`layer/systemd/volumio-evo.service`**). By default, systemd captures the process **stdout/stderr** into the journal for **`Type=simple`** units.
- Older binaries without the `[EVO]` writer will not show the process-wide prefix; domain tags in messages may still appear where implemented.
