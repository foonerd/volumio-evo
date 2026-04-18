# Alarm clock: RTC wake from suspend

This document describes how Volumio Evo uses **`rtcwake`** (package **util-linux**) to schedule **wake-from-suspend** so an alarm can resume playback after the system returns from **suspend-to-RAM** (`mem`), **freeze**, or similar — **without** pretending the Pi can cold-boot from full wall power loss (that needs different hardware/policy).

Product stance: ship this path **early**, measure on **Pi 5**, **CM4/CM5**, **some Pi 4**, and **amd64** installs, then refine (RTC device selection, timezone edge cases, max alarm horizon).

## Why `rtcwake`

Linux exposes RTC alarms through the generic RTC framework; **`rtcwake`** is the portable CLI:

| Mode | Behaviour |
|------|-----------|
| **`-m no`** | Program the RTC alarm **only** — **no** sleep (used by Evo when setting “wake at” without suspending immediately). |
| **`-m disable`** | Clear a programmed alarm. |
| **`-m show`** | Print current alarm status (good for diagnostics). |
| **`-m mem`** | Suspend to RAM until RTC fires or interrupt. |
| **`-t`** | Absolute wakeup as **Unix `time_t`** (**UTC**). |

See **`man rtcwake`**. Some boards cap how far ahead the RTC can alarm (often **≤ 24 hours**); behaviour varies by driver.

## Evo implementation

- **Rust:** [`crates/core/src/rtc_wake.rs`](../crates/core/src/rtc_wake.rs) — **`program_wake_utc_epoch`**, **`clear_wake`**, **`wake_show_text`**, **`log_startup_probe`**.
- **Caller responsibility:** Convert “alarm at **local** wall time” → **UTC epoch seconds** before calling **`program_wake_utc_epoch`** (use system timezone / **`timedatectl`** / persisted Evo settings consistently).
- **Suspend:** Evo does **not** auto-invoke **`systemctl suspend`** or **`rtcwake -m mem`** from this module alone — product code must decide when to enter sleep (user gesture, idle policy, etc.). Typical pattern: program RTC with **`-m no`**, then **`systemctl suspend`**, or use **`rtcwake -m mem -t …`** in one shot for manual tests.

## Privileges (non-root service)

Programming the RTC requires **root**. When **`volumio-evo`** runs as a normal user, Evo uses **`sudo -n`** with **`VOLUMIO_EVO_RTCWAKE`** (path to **`rtcwake`**), matching **`/etc/sudoers.d/volumio-evo-rtcwake`** installed by **`scripts/bootstrap-volumio-evo-player.sh`** when **`EVO_INSTALL_RTCWAKE_SUDOERS=1`** (default). Same pattern as **`nmcli`** / **`iw`**.

See **[OS_PRIVILEGE_MODEL.md](OS_PRIVILEGE_MODEL.md)** and **[RUNTIME_USER.md](RUNTIME_USER.md)**.

## Environment variables

| Variable | Meaning |
|----------|---------|
| **`VOLUMIO_EVO_RTCWAKE`** | Path to **`rtcwake`** (bootstrap sets from **`command -v`**). |
| **`VOLUMIO_EVO_RTC_DEVICE`** | Optional RTC name for **`-d`** (e.g. **`rtc0`**) when the platform exposes more than one RTC. |
| **`VOLUMIO_EVO_PROBE_RTC_WAKE`** | If **`1`** or **`true`**, Evo logs one startup probe (**`rtcwake`** path + **`rtcwake -m show`** when permitted). |

## Manual testing on device

1. Ensure **util-linux** provides **`/usr/sbin/rtcwake`**.
2. Re-run bootstrap so sudoers + **`Environment=VOLUMIO_EVO_RTCWAKE`** exist.
3. As root, sanity-check RTC list: **`rtcwake --list-modes`**, **`rtcwake -m show`**.
4. Program + suspend + wake (example: wake in ~2 minutes — adjust epoch):

   ```bash
   sudo rtcwake -m no -t "$(($(date +%s) + 120))"
   sudo systemctl suspend
   ```

   Or relative: **`sudo rtcwake -m mem -s 120`** (sleeps **and** sets alarm).

5. Run Evo with **`VOLUMIO_EVO_PROBE_RTC_WAKE=1`** and inspect **`journalctl -u volumio-evo`** for **`EVO RTC`** lines.

## Limitations (expected to refine)

- **Powered-off (S5)** is **not** the same as suspend; RTC wake usually applies to **suspend/hibernate** paths the firmware/kernel support.
- **Hardware**: some boards lack a wake-capable RTC or expose **`rtc1`** only — use **`VOLUMIO_EVO_RTC_DEVICE`** after **`ls /sys/class/rtc/`**.
- **Timezone**: **`rtcwake -t`** uses **UTC**; mismatches between UI local time and conversion will show up as “alarm offset” bugs — fix in the alarm scheduler layer, not inside **`rtc_wake`**.

## Related

- Persisted alarm state will live under **`settings/`** when the alarm feature is wired end-to-end — see **[SETTINGS_LAYOUT.md](SETTINGS_LAYOUT.md)** (updated when that lands).
