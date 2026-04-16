//! Load and run WASM plugins via Wasmtime.

use std::path::Path;

use anyhow::{Context, Result};
use volumio_evo_plugin_sdk::abi::{self, AlsaContribution};
use wasmtime::*;

/// Handle to a loaded plugin (engine + module + store). Lifecycle TBD.
#[allow(dead_code)]
pub struct PluginHandle {
    _engine: Engine,
    _module: Module,
    _store: Store<()>,
}

/// Read optional [`AlsaContribution`] from guest exports + linear memory.
fn read_alsa_contribution(instance: &Instance, store: &mut Store<()>) -> Result<Option<AlsaContribution>> {
    let Some(has_fn) = instance.get_func(&mut *store, abi::alsa_exports::HAS_ALSA_CONTRIBUTION) else {
        return Ok(None);
    };
    let has_fn = has_fn
        .typed::<(), i32>(&*store)
        .context("has_alsa_contribution signature")?;
    let has = has_fn.call(&mut *store, ())?;
    if has == 0 {
        return Ok(None);
    }

    let ptr_fn = instance
        .get_func(&mut *store, abi::alsa_exports::ALSA_JSON_PTR)
        .context("alsa_contribution_json_ptr required when has_alsa_contribution=1")?;
    let len_fn = instance
        .get_func(&mut *store, abi::alsa_exports::ALSA_JSON_LEN)
        .context("alsa_contribution_json_len required when has_alsa_contribution=1")?;

    let ptr_fn = ptr_fn.typed::<(), i32>(&*store)?;
    let len_fn = len_fn.typed::<(), i32>(&*store)?;
    let ptr = ptr_fn.call(&mut *store, ())? as usize;
    let len = len_fn.call(&mut *store, ())? as usize;

    if len == 0 {
        return Ok(None);
    }

    let memory = instance
        .get_memory(&mut *store, "memory")
        .context("WASM plugin must export `memory` when contributing ALSA JSON")?;
    let mem_size = memory.data_size(&*store);
    let end = ptr
        .checked_add(len)
        .filter(|&e| e <= mem_size)
        .context("alsa JSON ptr/len out of bounds")?;
    let data = &memory.data(&*store)[ptr..end];
    let parsed: AlsaContribution =
        serde_json::from_slice(data).context("parse AlsaContribution JSON from plugin")?;
    if parsed.abi_version != abi::ALSA_PLUGIN_ABI_VERSION {
        anyhow::bail!(
            "plugin ALSA abi_version {} != host {}",
            parsed.abi_version,
            abi::ALSA_PLUGIN_ABI_VERSION
        );
    }
    Ok(Some(parsed))
}

/// Load a plugin from a `.wasm` file. Instantiates, calls `plugin_init`, probes optional ALSA ABI.
#[allow(dead_code)]
pub fn load_plugin(path: &Path) -> Result<PluginHandle> {
    let engine = Engine::default();
    let module = Module::from_file(&engine, path)?;
    let mut store = Store::new(&engine, ());

    let mut linker = Linker::new(&engine);
    linker.func_wrap("env", "log", |_caller: Caller<'_, ()>, _ptr: i32, _len: i32| {
        tracing::debug!("{} plugin log (stub)", crate::log_tags::EVO_PLUGIN);
    })?;

    let instance = linker.instantiate(&mut store, &module)?;

    // Call plugin_init if present.
    if let Some(init) = instance.get_func(&mut store, "plugin_init") {
        let init = init.typed::<(), ()>(&store)?;
        init.call(&mut store, ())?;
    }

    match read_alsa_contribution(&instance, &mut store) {
        Ok(Some(c)) => {
            tracing::debug!(
                fragments = c.fragments.len(),
                path = %path.display(),
                "{} WASM plugin ALSA contribution",
                crate::log_tags::EVO_PLUGIN
            );
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(
            error = %e,
            path = %path.display(),
            "{} WASM plugin ALSA probe failed",
            crate::log_tags::EVO_PLUGIN
        ),
    }

    Ok(PluginHandle {
        _engine: engine,
        _module: module,
        _store: store,
    })
}
