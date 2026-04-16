# Playback state: phased requirements (Evo ↔ Volumio2-UI)

Work is split so transport, UI contract, and efficiency can be reasoned about separately.

## Phase 1 — Observability (DEBUG pushState)

**Goal:** Operators can see **facts** in the journal: what snapshot was built, whether `io.emit(pushState)` / `pushQueue` succeeded.

**Implementation:** `EVO PUSHSTATE` DEBUG lines for:

- Broadcast loop: `pushState` / `pushQueue` after emit (include `seek_ms`, `duration_s`, `position`, `volume`, `emit=ok|err`).
- Socket handlers: `getState` / `getQueue` after emit.
- REST: `GET /api/v1/getState` (response snapshot); optional queue length for `getQueue`.

**Grep:** `journalctl -u volumio-evo | grep -F 'EVO PUSHSTATE'`

---

## Phase 2 — Elapsed time: 1 s UI steps

**Goal:** The stock UI’s **elapsed display** advances in **one-second steps** (no stuck `0:00`, no random skips of a second).

**Constraint (Volumio2-UI):** The volumio3 playback view uses **`elapsedTimeString`**, updated from Angular’s `$interval(..., 1000)` via `startSeek()` → `updateSeek()` → `calculateElapsedTimeString()`. Each `pushState` while playing calls `startSeek()` and **cancels** that interval. Server **`pushState` cadence** must allow that timer to run (see Phase 4).

**Out of scope for Evo alone:** Perfect second ticks may still need small UI fixes; Evo must not violate the timer contract.

---

## Phase 3 — Progress vs source

**Goal:** Progress ring / seek percent stay **consistent** with either **MPD truth** on resync or **elapsed time** between samples (same direction, bounded drift).

**Approach options:** Align broadcast timing with Phase 2; optionally smooth corrections so resync does not jump more than ~1 displayed second when possible.

---

## Phase 4 — Optimize queue + state traffic

**Goal:** Avoid excessive TCP and Socket.IO work (e.g. **not** tens or hundreds of events per second).

**Examples:**

- One MPD connection per poll: `get_state` + `get_queue` on the same client.
- **Decouple** queue broadcasts from state if queue rarely changes (hash or version).
- **Configurable** minimum interval for full broadcast (state + queue), tuned with Phases 2–3.

**Anti-pattern:** High-frequency `pushState` that resets the UI timer every tick (breaks Phase 2).

---

## Implementation note (Evo, Node-like)

RAM **`PlaybackClock`** (`crates/core/src/api/playback_clock.rs`) advances `seek` between MPD samples when playing; **sparse** `pushState` (~2.2s) + single **`get_state_and_queue_connected`** per tick. **Do not** emit `pushState` at high frequency — the stock UI fills gaps with its own 1s timer.
