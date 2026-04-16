# High priority — ALSA / AAMPP / plugin audio pipeline

**Status: not implemented end-to-end in Evo.** Treat this as **load-bearing infrastructure** for ported plugins (DSP, effects, anything that contributes ALSA).

## Why this is priority

- Volumio 3+ routes plugin ALSA changes through **[ALSA contribution snippets](https://developers.volumio.com/AAMPP/using-aampp)** and a **central rebuild**, not direct edits to `/etc/asound.conf`.
- **Deferring** this layer does **not** reduce complexity when plugins and volume/device code already assume a **graph** exists. It increases rework if wrong shapes are baked in first.
- **DSP Fusion** and similar plugins are **blocked** until the host can **merge contributions** and **rebuild** the live ALSA configuration on demand (output device change, plugin enable/disable, etc.).

## What Evo has today

- **WASM ABI:** `has_alsa_contribution`, JSON `AlsaContribution` with fragments (`crates/plugin-sdk`, `docs/PLUGIN_ABI.md`).
- **Probe:** `crates/core/src/plugins/wasm.rs` reads contributions at load and logs; **no** merge into system ALSA, **no** `rebuildALSAConfiguration`-equivalent. Plugin lifecycle integration is **TBD**.

## What must be built (minimal bar for “ready”)

1. **Collect** all active plugin ALSA fragments (ordering rules per Volumio / ABI).
2. **Merge** into the effective ALSA config the image uses (same contract as stock: snippets, not ad-hoc file stomping).
3. **Rebuild** pipeline when **`alsa.outputdevice`** (or Evo equivalent) changes and when plugins contributing ALSA change.
4. **Wire** MPD / playback device strings so they target the **composed** PCM graph when modular mode expects it.

Until those exist, answer for “can we port DSP / full AAMPP plugins **now**?” is **no** — not because of missing UI, but because the **host integration boundary** is missing.

## References

- [Using AAMPP in a plugin](https://developers.volumio.com/AAMPP/using-aampp) (snippets, `has_alsa_contribution`, rebuild).
- [AAMPP overview](https://developers.volumio.com/AAMPP/aampp-overview) (framework context).
