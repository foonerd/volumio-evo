//! Build-time **volumio-evo** version (the `volumio-evo` daemon crate).

/// Semver from `volumio-evo-core`’s `Cargo.toml` — this is the **volumio-evo** software version
/// (e.g. Settings → System, `getSystemVersion` / `pushSystemVersion` **`systemversion`** field).
pub const VOLUMIO_EVO_VERSION: &str = env!("CARGO_PKG_VERSION");
