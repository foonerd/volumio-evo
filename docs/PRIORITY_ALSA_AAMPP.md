# Audio processing rack - ALSA pipeline composition

**Fabric role:** Audio processing rack ([CONCEPT.md](CONCEPT.md) section 3, Domain / Transformer). Processor plugins contribute ALSA fragments; the steward composes them into a single pipeline projection that the audio rack's delivery warden consumes.

**Status:** composition not yet implemented end-to-end. Without it, processor plugins (DSP, effects, correction) cannot be stocked in a way the audio rack can actually use.

## Why this matters

- The audio processing rack exists to admit multiple processor plugins and emit a composed pipeline. Today the ingestion side (one WASM transport) exists; the steward-side composition and delivery-warden reconfiguration do not.
- Deferring composition does not reduce complexity. Processor plugin authors will target a composed graph regardless; getting the fabric shape wrong first means rework.
- Processor plugins (DSP Fusion and similar) are blocked on this composition path, not on their own implementations.
- Volumio 3+ routed ALSA changes through [contribution snippets](https://developers.volumio.com/AAMPP/using-aampp) and a central rebuild, not direct edits to `/etc/asound.conf`. The fabric keeps the same principle: plugins contribute declaratively, the steward composes, nothing bypasses.

## What exists today

- **WASM transport:** guests can declare `has_alsa_contribution` and produce JSON `AlsaContribution` fragments (see [PLUGIN_ABI.md](PLUGIN_ABI.md)). This is one transport; native and out-of-process plugin transports are equally admissible under the contract ([CONCEPT.md](CONCEPT.md) section 5).
- **Probe only:** fragments are read at load time and logged. There is no steward-side composition, no pipeline rebuild, no delivery-warden reconfiguration on change. The ingestion plumbing works; the orchestration plumbing does not.

## What must be built (minimal bar for "ready")

1. **Collect** all active processor plugin ALSA fragments from the audio processing rack's processor shelf, honouring the shelf's declared ordering rules.
2. **Compose** into the effective ALSA config the image uses (same contract as stock: snippets, not ad-hoc file stomping). Composition is the steward's work.
3. **Reproject** the audio rack's delivery pipeline when the output device changes or when the set of active processor plugins changes. The delivery warden consumes the new projection and reconfigures.
4. **Wire** MPD (or its successor warden) so it targets the composed PCM graph when modular mode is active.

Until these exist, the answer to "can we port DSP or full AAMPP plugins now?" is **no** - not because of missing UI, but because the **composition path from processor plugins to delivery warden** is missing.

## References

- [Using AAMPP in a plugin](https://developers.volumio.com/AAMPP/using-aampp) (snippets, `has_alsa_contribution`, rebuild).
- [AAMPP overview](https://developers.volumio.com/AAMPP/aampp-overview) (framework context).
- [CONCEPT.md](CONCEPT.md) section 3 (audio processing rack charter), section 4 (plugin model), section 6 (existing assets mapping for ALSA).
