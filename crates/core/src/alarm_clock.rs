//! Persisted alarm clock + sleep timer (Socket.IO parity with volumio3-backend `alarm-clock`).
//!
//! State file: [`crate::paths::default_alarm_clock_state_path`] (`settings/alarm/state.toml`).
//!
//! ## Product contract: daily alarm **WYSIWYG**
//!
//! What the user sets on the clock (**hour / minute**) is authoritative. Client payloads may carry
//! ISO datetimes with stray **seconds**; Evo **canonicalizes** each alarm’s **`time`** to **`HH:MM`**
//! on **`saveAlarm`** before persisting, normalizes to **`HH:MM:00`** local when scheduling, and fires on
//! that minute boundary — documented in **`docs/SETTINGS_LAYOUT.md`** (product contract section).

use crate::api::{system_power::graceful_power_transition, AppState};
use crate::log_tags::EVO_ALARM;
use crate::mpd::{self, MpdConfig};
use chrono::{
    DateTime, Duration as ChronoDuration, Local, LocalResult, NaiveTime, TimeZone, Timelike, Utc,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmClockState {
    #[serde(default)]
    pub alarms: Vec<AlarmRow>,
    #[serde(default)]
    pub sleep: SleepState,
}

impl Default for AlarmClockState {
    fn default() -> Self {
        Self {
            alarms: Vec::new(),
            sleep: SleepState::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmRow {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub playlist: String,
    /// Daily alarm wall time. After **`saveAlarm`**, Evo persists **canonical `HH:MM`** only (WYSIWYG).
    #[serde(default)]
    pub time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SleepState {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub duration_hours: u32,
    #[serde(default)]
    pub duration_minutes: u32,
    #[serde(default)]
    pub requested_at_rfc3339: Option<String>,
    /// When set, sleep fires at this absolute UTC instant (wall-clock **HH:MM** mode, `hours >= 12`).
    #[serde(default)]
    pub sleep_deadline_rfc3339: Option<String>,
    #[serde(default = "default_sleep_action")]
    pub action: String,
}

fn default_sleep_action() -> String {
    "stop".into()
}

impl Default for SleepState {
    fn default() -> Self {
        Self {
            enabled: false,
            duration_hours: 0,
            duration_minutes: 0,
            requested_at_rfc3339: None,
            sleep_deadline_rfc3339: None,
            action: default_sleep_action(),
        }
    }
}

struct AlarmClockInner {
    persisted: AlarmClockState,
    sleep_task: Option<tokio::task::JoinHandle<()>>,
    alarm_task: Option<tokio::task::JoinHandle<()>>,
}

pub struct AlarmClockCoordinator {
    path: PathBuf,
    inner: Mutex<AlarmClockInner>,
    /// Shared with the alarm poll task so “fired today” survives across ticks.
    alarm_last_days: Arc<Mutex<Vec<String>>>,
}

impl AlarmClockCoordinator {
    pub fn new(path: PathBuf) -> Arc<Self> {
        tracing::debug!("{} AlarmClockCoordinator::new enter path={:?}", EVO_ALARM, path);
        let persisted = read_or_default(&path);
        let n = persisted.alarms.len();
        tracing::debug!(
            "{} AlarmClockCoordinator::new loaded alarms={} sleep_enabled={}",
            EVO_ALARM,
            n,
            persisted.sleep.enabled
        );
        Arc::new(Self {
            path,
            alarm_last_days: Arc::new(Mutex::new(vec![String::new(); n])),
            inner: Mutex::new(AlarmClockInner {
                persisted,
                sleep_task: None,
                alarm_task: None,
            }),
        })
    }

    /// Load disk state again (used sparingly); normally handlers mutate through [`Self::apply_*`].
    #[allow(dead_code)]
    pub async fn reload_from_disk(&self) {
        tracing::debug!("{} AlarmClockCoordinator::reload_from_disk enter", EVO_ALARM);
        let disk = read_or_default(&self.path);
        let n = disk.alarms.len();
        let mut g = self.inner.lock().await;
        g.persisted = disk;
        drop(g);
        let mut d = self.alarm_last_days.lock().await;
        sync_last_days(&mut *d, n);
        tracing::debug!(
            "{} AlarmClockCoordinator::reload_from_disk done alarms={}",
            EVO_ALARM,
            n
        );
    }

    fn mpd_config(state: &AppState) -> MpdConfig {
        MpdConfig {
            host: state.config.mpd_host.clone(),
            port: state.config.mpd_port,
        }
    }

    /// Cancel running timers and start new ones from persisted state.
    pub async fn reschedule_all(self: &Arc<Self>, app: AppState) {
        tracing::debug!("{} AlarmClockCoordinator::reschedule_all enter", EVO_ALARM);
        let mut g = self.inner.lock().await;
        if let Some(h) = g.sleep_task.take() {
            h.abort();
        }
        if let Some(h) = g.alarm_task.take() {
            h.abort();
        }

        self.cleanup_expired_sleep_locked(&mut g).await;

        let sleep_snapshot = g.persisted.sleep.clone();
        let alarms_snapshot = g.persisted.alarms.clone();
        drop(g);

        let coord = Arc::clone(self);
        let app_sleep = app.clone();
        let sleep_handle = Self::spawn_sleep_task_if_needed(coord.clone(), app_sleep, sleep_snapshot);

        let last_days = Arc::clone(&self.alarm_last_days);
        let app_alarm = app.clone();
        let alarm_handle = Self::spawn_alarm_poll_task(app_alarm, alarms_snapshot, last_days);

        let mut g = self.inner.lock().await;
        g.sleep_task = sleep_handle;
        g.alarm_task = alarm_handle;
        tracing::debug!(
            "{} AlarmClockCoordinator::reschedule_all done sleep_task={} alarm_task={}",
            EVO_ALARM,
            g.sleep_task.is_some(),
            g.alarm_task.is_some()
        );
    }

    async fn cleanup_expired_sleep_locked(&self, g: &mut AlarmClockInner) {
        tracing::debug!(
            "{} cleanup_expired_sleep_locked sleep.enabled={}",
            EVO_ALARM,
            g.persisted.sleep.enabled
        );
        if !g.persisted.sleep.enabled {
            return;
        }
        let Some(deadline) = sleep_deadline_utc(&g.persisted.sleep) else {
            g.persisted.sleep.enabled = false;
            g.persisted.sleep.requested_at_rfc3339 = None;
            g.persisted.sleep.sleep_deadline_rfc3339 = None;
            let _ = save_to_path(&self.path, &g.persisted);
            return;
        };
        if deadline > Utc::now() {
            return;
        }
        tracing::info!(
            "{} sleep timer expired while offline or overdue; disabling",
            EVO_ALARM
        );
        g.persisted.sleep.enabled = false;
        g.persisted.sleep.requested_at_rfc3339 = None;
        g.persisted.sleep.sleep_deadline_rfc3339 = None;
        let _ = save_to_path(&self.path, &g.persisted);
    }

    fn spawn_sleep_task_if_needed(
        coord: Arc<Self>,
        app: AppState,
        sleep: SleepState,
    ) -> Option<tokio::task::JoinHandle<()>> {
        tracing::debug!(
            "{} spawn_sleep_task_if_needed enabled={} action={}",
            EVO_ALARM,
            sleep.enabled,
            sleep.action
        );
        if !sleep.enabled {
            tracing::debug!("{} spawn_sleep_task_if_needed skip (disabled)", EVO_ALARM);
            return None;
        }
        let deadline = sleep_deadline_utc(&sleep)?;
        let left = deadline - Utc::now();
        let local_fire = deadline.with_timezone(&Local);
        let mode = if sleep.sleep_deadline_rfc3339.is_some() {
            "wall_clock_local_hm"
        } else {
            "duration_from_save"
        };
        tracing::info!(
            "{} sleep timer armed ({}): fires at {} local ({} UTC), in {:?}; UI fields {}h {}m",
            EVO_ALARM,
            mode,
            local_fire.format("%Y-%m-%d %H:%M:%S"),
            deadline.format("%H:%M:%SZ"),
            left,
            sleep.duration_hours,
            sleep.duration_minutes
        );
        if left <= ChronoDuration::zero() {
            tracing::info!("{} sleep deadline already passed; clearing", EVO_ALARM);
            let c = coord.clone();
            let a = app.clone();
            return Some(tokio::spawn(async move {
                c.disable_sleep_after_expired(a).await;
            }));
        }
        tracing::debug!(
            "{} spawn_sleep_task_if_needed until_deadline_s={}",
            EVO_ALARM,
            left.num_seconds()
        );
        Some(tokio::spawn(async move {
            tracing::debug!("{} sleep timer task: precise wait until deadline", EVO_ALARM);
            sleep_until_utc_deadline_precise(deadline).await;
            // Match Node `alarm-clock`: schedule fires, then 5s grace before stop/shutdown.
            tokio::time::sleep(Duration::from_secs(5)).await;
            coord.run_sleep_fire(app).await;
        }))
    }

    fn spawn_alarm_poll_task(
        app: AppState,
        alarms: Vec<AlarmRow>,
        last_days: Arc<Mutex<Vec<String>>>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let enabled_ct = alarms.iter().filter(|a| a.enabled).count();
        tracing::debug!(
            "{} spawn_alarm_poll_task rows={} enabled={}",
            EVO_ALARM,
            alarms.len(),
            enabled_ct
        );
        if alarms.iter().all(|a| !a.enabled) {
            tracing::debug!("{} spawn_alarm_poll_task skip (no enabled alarms)", EVO_ALARM);
            return None;
        }
        Some(tokio::spawn(async move {
            tracing::debug!(
                "{} alarm scheduler: wall-clock via chrono::Local (system TZ / DST)",
                EVO_ALARM
            );
            loop {
                let now_local = Local::now();
                let mut min_deadline: Option<DateTime<Utc>> = None;
                let mut fire_indices: Vec<usize> = Vec::new();

                for (i, alarm) in alarms.iter().enumerate() {
                    if !alarm.enabled {
                        continue;
                    }
                    let Some(wall_raw) = parse_alarm_naive_time(&alarm.time) else {
                        continue;
                    };
                    let wall = normalize_daily_alarm_wall_time(wall_raw);
                    let Some(t_utc) =
                        next_local_wall_time_occurrence_after(now_local, wall)
                    else {
                        continue;
                    };
                    match min_deadline {
                        None => {
                            min_deadline = Some(t_utc);
                            fire_indices = vec![i];
                        }
                        Some(m) if t_utc < m => {
                            min_deadline = Some(t_utc);
                            fire_indices = vec![i];
                        }
                        Some(m) if t_utc == m => fire_indices.push(i),
                        _ => {}
                    }
                }

                let Some(deadline) = min_deadline else {
                    tokio::time::sleep(Duration::from_secs(600)).await;
                    continue;
                };

                sleep_until_utc_deadline_precise(deadline).await;

                let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
                let mut pending: Vec<String> = Vec::new();
                {
                    let mut track = last_days.lock().await;
                    sync_last_days(&mut track, alarms.len());
                    for &i in &fire_indices {
                        if track.get(i).map(|s| s.as_str()) == Some(today.as_str()) {
                            continue;
                        }
                        let alarm = &alarms[i];
                        let actual = Utc::now();
                        let skew_ms = (actual - deadline).num_milliseconds();
                        tracing::info!(
                            "{} firing alarm playlist={:?} target={} actual={} skew_ms={}",
                            EVO_ALARM,
                            alarm.playlist,
                            deadline.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                            actual.format("%Y-%m-%dT%H:%M:%S%.3fZ"),
                            skew_ms
                        );
                        if let Some(slot) = track.get_mut(i) {
                            *slot = today.clone();
                        }
                        pending.push(alarm.playlist.clone());
                    }
                }

                for name in pending {
                    let cfg = AlarmClockCoordinator::mpd_config(&app);
                    if let Err(e) = mpd::play_playlist_by_name(&cfg, &name).await {
                        tracing::warn!("{} alarm playPlaylist: {}", EVO_ALARM, e);
                    }
                    app.notify_push_state();
                }
            }
        }))
    }

    async fn disable_sleep_after_expired(self: Arc<Self>, app: AppState) {
        tracing::debug!("{} disable_sleep_after_expired enter", EVO_ALARM);
        {
            let mut g = self.inner.lock().await;
            g.persisted.sleep.enabled = false;
            g.persisted.sleep.requested_at_rfc3339 = None;
            g.persisted.sleep.sleep_deadline_rfc3339 = None;
            let _ = save_to_path(&self.path, &g.persisted);
        }
        let v = self.push_sleep_payload().await;
        broadcast_push_sleep(&app, &v).await;
        self.reschedule_all(app).await;
        tracing::debug!("{} disable_sleep_after_expired done", EVO_ALARM);
    }

    async fn run_sleep_fire(self: Arc<Self>, app: AppState) {
        tracing::debug!("{} run_sleep_fire enter", EVO_ALARM);
        let action = {
            let g = self.inner.lock().await;
            g.persisted.sleep.action.clone()
        };

        tracing::info!("{} sleep timer fired (action={})", EVO_ALARM, action);

        {
            let mut g = self.inner.lock().await;
            g.persisted.sleep.enabled = false;
            g.persisted.sleep.requested_at_rfc3339 = None;
            g.persisted.sleep.sleep_deadline_rfc3339 = None;
            let _ = save_to_path(&self.path, &g.persisted);
        }

        let v = self.push_sleep_payload().await;
        broadcast_push_sleep(&app, &v).await;

        if action == "poweroff" {
            let st = app.clone();
            tokio::spawn(async move {
                graceful_power_transition(st, false).await;
            });
        } else {
            let cfg = Self::mpd_config(&app);
            if let Err(e) = mpd::run_command_connected(&cfg, "stop", None, None, None, None).await {
                tracing::warn!("{} sleep stop: {}", EVO_ALARM, e);
            }
            app.notify_push_state();
        }

        self.reschedule_all(app).await;
        tracing::debug!("{} run_sleep_fire done action={}", EVO_ALARM, action);
    }

    /// Apply `setSleep` payload and persist.
    pub async fn apply_set_sleep(&self, app: &AppState, data: Option<serde_json::Value>) {
        tracing::debug!(
            "{} apply_set_sleep enter has_payload={}",
            EVO_ALARM,
            data.is_some()
        );
        let Some(v) = data else {
            tracing::debug!("{} apply_set_sleep exit (no payload)", EVO_ALARM);
            return;
        };
        #[derive(Deserialize)]
        struct SetSleepIn {
            enabled: bool,
            time: String,
            #[serde(default)]
            action: Option<String>,
        }
        let Ok(parsed) = serde_json::from_value::<SetSleepIn>(v) else {
            tracing::warn!("{} setSleep: invalid payload", EVO_ALARM);
            return;
        };

        let Some((dh, dm)) = parse_hm_duration(&parsed.time) else {
            tracing::warn!("{} setSleep: bad time {:?}", EVO_ALARM, parsed.time);
            return;
        };
        tracing::debug!(
            "{} apply_set_sleep parsed enabled={} duration={}:{} action={:?}",
            EVO_ALARM,
            parsed.enabled,
            dh,
            dm,
            parsed.action
        );

        let mut g = self.inner.lock().await;
        if parsed.enabled {
            let now = Utc::now();
            g.persisted.sleep.enabled = true;
            g.persisted.sleep.duration_hours = dh;
            g.persisted.sleep.duration_minutes = dm;
            g.persisted.sleep.action =
                normalize_sleep_action(parsed.action.as_deref().unwrap_or("stop"));
            g.persisted.sleep.requested_at_rfc3339 = Some(now.to_rfc3339());
            // Stock UI: hour < 12 → countdown from save (presets 0:15 … 4:0). hour >= 12 → local wall HH:MM:00 (today or tomorrow).
            if dh >= 12 {
                let Some(abs) = wall_clock_deadline_utc(dh, dm) else {
                    tracing::warn!(
                        "{} setSleep: invalid wall-clock {}:{}",
                        EVO_ALARM,
                        dh,
                        dm
                    );
                    g.persisted.sleep.enabled = false;
                    g.persisted.sleep.sleep_deadline_rfc3339 = None;
                    g.persisted.sleep.requested_at_rfc3339 = None;
                    drop(g);
                    return;
                };
                g.persisted.sleep.sleep_deadline_rfc3339 = Some(abs.to_rfc3339());
            } else {
                g.persisted.sleep.sleep_deadline_rfc3339 = None;
            }
        } else {
            g.persisted.sleep.enabled = false;
            g.persisted.sleep.duration_hours = dh;
            g.persisted.sleep.duration_minutes = dm;
            g.persisted.sleep.action =
                normalize_sleep_action(parsed.action.as_deref().unwrap_or("stop"));
            g.persisted.sleep.requested_at_rfc3339 = None;
            g.persisted.sleep.sleep_deadline_rfc3339 = None;
        }
        let _ = save_to_path(&self.path, &g.persisted);
        drop(g);

        app.alarm_clock.clone().reschedule_all(app.clone()).await;
        tracing::debug!("{} apply_set_sleep done", EVO_ALARM);
    }

    /// Replace alarms from `saveAlarm` array and persist.
    pub async fn apply_save_alarms(&self, app: &AppState, data: Option<serde_json::Value>) {
        tracing::debug!(
            "{} apply_save_alarms enter has_payload={}",
            EVO_ALARM,
            data.is_some()
        );
        let Some(v) = data else {
            tracing::debug!("{} apply_save_alarms exit (no payload)", EVO_ALARM);
            return;
        };
        let Ok(mut rows) = serde_json::from_value::<Vec<AlarmRow>>(v) else {
            tracing::warn!("{} saveAlarm: expected array", EVO_ALARM);
            return;
        };

        for row in &mut rows {
            row.playlist = row.playlist.trim().to_string();
            row.time = row.time.trim().to_string();
            // WYSIWYG: only HH:MM matter for daily alarms — ignore stray ISO seconds from the client.
            if let Some(t) = parse_alarm_naive_time(&row.time) {
                let wall = normalize_daily_alarm_wall_time(t);
                row.time = format!("{:02}:{:02}", wall.hour(), wall.minute());
            }
        }

        let mut g = self.inner.lock().await;
        g.persisted.alarms = rows;
        let n = g.persisted.alarms.len();
        let _ = save_to_path(&self.path, &g.persisted);
        drop(g);

        {
            let mut d = self.alarm_last_days.lock().await;
            sync_last_days(&mut *d, n);
        }

        app.alarm_clock.clone().reschedule_all(app.clone()).await;
        tracing::debug!("{} apply_save_alarms done rows={}", EVO_ALARM, n);
    }

    pub async fn push_sleep_payload(&self) -> serde_json::Value {
        tracing::debug!("{} push_sleep_payload enter", EVO_ALARM);
        let mut g = self.inner.lock().await;
        self.cleanup_expired_sleep_locked(&mut g).await;
        let sleep = g.persisted.sleep.clone();
        drop(g);
        let j = sleep_to_json(&sleep);
        tracing::debug!("{} push_sleep_payload done enabled={}", EVO_ALARM, sleep.enabled);
        j
    }

    pub async fn push_alarm_payload(&self) -> serde_json::Value {
        tracing::debug!("{} push_alarm_payload enter", EVO_ALARM);
        let g = self.inner.lock().await;
        let n = g.persisted.alarms.len();
        let v = serde_json::to_value(&g.persisted.alarms).unwrap_or_else(|_| json!([]));
        tracing::debug!("{} push_alarm_payload done rows={}", EVO_ALARM, n);
        v
    }
}

fn sync_last_days(days: &mut Vec<String>, len: usize) {
    let before = days.len();
    days.resize(len, String::new());
    if before != len {
        tracing::debug!(
            "{} sync_last_days resize {} -> {}",
            EVO_ALARM,
            before,
            len
        );
    }
}

fn normalize_sleep_action(a: &str) -> String {
    match a.trim() {
        "poweroff" => "poweroff".into(),
        _ => "stop".into(),
    }
}

fn sleep_deadline_utc(sleep: &SleepState) -> Option<DateTime<Utc>> {
    if let Some(ref s) = sleep.sleep_deadline_rfc3339 {
        return DateTime::parse_from_rfc3339(s.trim())
            .ok()
            .map(|d| d.with_timezone(&Utc));
    }
    let start_s = sleep.requested_at_rfc3339.as_ref()?;
    let start = DateTime::parse_from_rfc3339(start_s.trim()).ok()?.with_timezone(&Utc);
    let dur = ChronoDuration::hours(sleep.duration_hours as i64)
        + ChronoDuration::minutes(sleep.duration_minutes as i64);
    Some(start + dur)
}

/// Sleep wall-clock: next **`hour`:`minute`**:00 local (system timezone / DST via **`chrono::Local`**).
fn wall_clock_deadline_utc(hour: u32, minute: u32) -> Option<DateTime<Utc>> {
    let t = NaiveTime::from_hms_opt(hour, minute, 0)?;
    next_local_wall_time_occurrence_after(Local::now(), t)
}

/// Next time **`target`** local civil time occurs strictly after **`now`** (may be today or within the next year).
/// Source of truth is the OS zone database / libc as used by **`chrono::Local`** (not browser session time).
fn next_local_wall_time_occurrence_after(
    now: DateTime<Local>,
    target: NaiveTime,
) -> Option<DateTime<Utc>> {
    for day_offset in 0..366i64 {
        let date = now
            .date_naive()
            .checked_add_signed(ChronoDuration::days(day_offset))?;
        let naive = date.and_time(target);
        match Local.from_local_datetime(&naive) {
            LocalResult::Single(dt) => {
                if dt > now {
                    return Some(dt.with_timezone(&Utc));
                }
            }
            LocalResult::Ambiguous(dt1, dt2) => {
                for cand in [dt1.min(dt2), dt1.max(dt2)] {
                    if cand > now {
                        return Some(cand.with_timezone(&Utc));
                    }
                }
            }
            LocalResult::None => {}
        }
    }
    None
}

/// Wait until **`deadline`** (UTC from OS clock). Coarse sleeps first, then fine steps, then short yield/spin so playback lines up with **local wall** targets (alarms at 20:00:00.000, etc.).
async fn sleep_until_utc_deadline_precise(deadline: DateTime<Utc>) {
    loop {
        let now = Utc::now();
        if now >= deadline {
            return;
        }
        let rem = deadline - now;
        let Ok(std_rem) = rem.to_std() else {
            return;
        };
        const COARSE: Duration = Duration::from_millis(250);
        const MID: Duration = Duration::from_millis(10);
        const FINE: Duration = Duration::from_millis(2);
        const MICRO: Duration = Duration::from_micros(150);

        if std_rem > MID + COARSE {
            tokio::time::sleep(std_rem - MID).await;
        } else if std_rem > FINE + MID {
            tokio::time::sleep(std_rem - FINE).await;
        } else if std_rem > MICRO + FINE {
            tokio::time::sleep(std_rem - MICRO).await;
        } else if std_rem > MICRO {
            tokio::time::sleep(MICRO.min(std_rem / 2).max(Duration::from_nanos(1))).await;
        } else {
            while Utc::now() < deadline {
                tokio::task::yield_now().await;
                std::hint::spin_loop();
            }
            return;
        }
    }
}

/// Daily alarms fire on **minute** boundaries (**HH:MM:00.000** local). **WYSIWYG:** what the user
/// sets (**18** / **23**) is authoritative; ISO noise from the browser is ignored on save and here.
fn normalize_daily_alarm_wall_time(t: NaiveTime) -> NaiveTime {
    NaiveTime::from_hms_nano_opt(t.hour(), t.minute(), 0, 0).unwrap_or(t)
}

fn parse_alarm_naive_time(s: &str) -> Option<NaiveTime> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    for fmt in ["%H:%M:%S%.f", "%H:%M:%S", "%H:%M"] {
        if let Ok(t) = NaiveTime::parse_from_str(s, fmt) {
            return Some(t);
        }
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        let local = dt.with_timezone(&Local);
        return Some(local.time());
    }
    None
}

fn parse_hm_duration(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.trim().split(':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    Some((h, m))
}

fn action_display(action: &str) -> &'static str {
    match action {
        "poweroff" => "Turn off",
        _ => "Stop music",
    }
}

fn sleep_to_json(sleep: &SleepState) -> serde_json::Value {
    let action = normalize_sleep_action(&sleep.action);
    let text = action_display(&action);
    let (rh, rm) = remaining_sleep_display(sleep);
    json!({
        "enabled": sleep.enabled,
        "time": format!("{}:{}", rh, rm),
        "action": { "val": action, "text": text }
    })
}

fn remaining_sleep_display(sleep: &SleepState) -> (u32, u32) {
    if !sleep.enabled {
        return (0, 0);
    }
    let Some(deadline) = sleep_deadline_utc(sleep) else {
        return (0, 0);
    };
    let now = Utc::now();
    if deadline <= now {
        return (0, 0);
    }
    let diff = deadline - now;
    let mins = diff.num_minutes().max(0) as u64;
    ((mins / 60) as u32, (mins % 60) as u32)
}

fn read_or_default(path: &Path) -> AlarmClockState {
    tracing::debug!("{} read_or_default {:?}", EVO_ALARM, path);
    match std::fs::read_to_string(path) {
        Ok(s) => match toml::from_str::<AlarmClockState>(&s) {
            Ok(mut st) => {
                st.sleep.action = normalize_sleep_action(&st.sleep.action);
                tracing::debug!("{} read_or_default parsed alarms={}", EVO_ALARM, st.alarms.len());
                st
            }
            Err(e) => {
                tracing::warn!("{} corrupt {:?}: {}; using defaults", EVO_ALARM, path, e);
                AlarmClockState::default()
            }
        },
        Err(_) => {
            tracing::debug!("{} read_or_default missing or unreadable -> defaults", EVO_ALARM);
            AlarmClockState::default()
        }
    }
}

fn save_to_path(path: &Path, state: &AlarmClockState) -> std::io::Result<()> {
    tracing::debug!(
        "{} save_to_path {:?} alarms={}",
        EVO_ALARM,
        path,
        state.alarms.len()
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let ser = toml::to_string_pretty(state).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(path, ser)?;
    tracing::debug!("{} save_to_path ok", EVO_ALARM);
    Ok(())
}

pub(crate) async fn broadcast_push_sleep(app: &AppState, payload: &serde_json::Value) {
    tracing::debug!("{} broadcast_push_sleep enter", EVO_ALARM);
    let maybe_io = match app.socket_io_broadcast.lock() {
        Ok(g) => g.as_ref().cloned(),
        Err(_) => None,
    };
    if let Some(io) = maybe_io {
        let _ = io.emit("pushSleep", payload).await;
        tracing::debug!("{} broadcast_push_sleep emitted", EVO_ALARM);
    } else {
        tracing::debug!("{} broadcast_push_sleep no socket io", EVO_ALARM);
    }
}

pub(crate) async fn broadcast_push_alarm(app: &AppState, payload: &serde_json::Value) {
    tracing::debug!("{} broadcast_push_alarm enter", EVO_ALARM);
    let maybe_io = match app.socket_io_broadcast.lock() {
        Ok(g) => g.as_ref().cloned(),
        Err(_) => None,
    };
    if let Some(io) = maybe_io {
        let _ = io.emit("pushAlarm", payload).await;
        tracing::debug!("{} broadcast_push_alarm emitted", EVO_ALARM);
    } else {
        tracing::debug!("{} broadcast_push_alarm no socket io", EVO_ALARM);
    }
}
