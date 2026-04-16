# Logs, journald, and filtering

Evo uses **`tracing`** with **`tracing-subscriber`** (env filter + stderr). This page describes what you see in **`journalctl`** and how to grep it reliably.

## Two layers of markers

1. **Process-wide line prefix** — Every formatted line written by the subscriber starts with **`[EVO] `** (see `crates/core/src/evo_log_fmt.rs`, constant `log_tags::EVO_LINE`). That includes dependency crates (**`tower_http`**, **`mpd_protocol`**, …), not only Evo’s own modules.
2. **Domain tags inside the message** — Evo code prefixes many messages with stable anchors such as **`EVO VOLUME -->`**, **`EVO QUEUE -->`**, **`EVO BOOT -->`**, defined in **`crates/core/src/log_tags.rs`**. Use these to narrow by subsystem.

## Configuration precedence

1. **`RUST_LOG`** — If set in the environment (e.g. systemd `Environment=`), it is the full **`tracing_subscriber::EnvFilter`** directive and **wins** over the config file.
2. **`VOLUMIO_EVO_LOG_LEVEL`** — Overrides **`log_level`** from **`/etc/volumio-evo/config.toml`** when **`RUST_LOG`** is unset (values: `error`, `warn`, `info`, `verbose`, `debug`, `trace`).
3. **`log_level`** in **`config.toml`** — Default when neither of the above is set.

The example config documents this in **`layer/config/volumio-evo.toml.example`**.

## Examples

```bash
# All Evo process lines (fixed string; safe with grep -F)
journalctl -u volumio-evo -n 200 --no-pager | grep -F '[EVO]'

# Only volume-related Evo messages (domain tag)
journalctl -u volumio-evo -n 200 --no-pager | grep -F 'EVO VOLUME -->'

# Follow live
journalctl -u volumio-evo -f | grep -F '[EVO]'
```

**Note:** Dependency crates log under their own targets (e.g. `mpd_protocol::connection`). At **`debug`** / **`trace`**, wire-level MPD traffic can be noisy; use domain tags or filter targets via **`RUST_LOG`** (e.g. `RUST_LOG=info` to suppress dependency debug).

## Volume changes in the journal

Successful UI/socket volume applies emit an **`INFO`** line containing **`EVO VOLUME -->`** and **`volume applied`**. Low-level **`setvol`** also appears in **`mpd_protocol`** debug lines when the log level allows.

## Assumptions

- Service runs under **systemd** (see **`layer/systemd/volumio-evo.service`**). By default, systemd captures the process **stdout/stderr** into the journal for **`Type=simple`** units.
- Older binaries without the `[EVO]` writer will not show the process-wide prefix; domain tags in messages may still appear where implemented.
