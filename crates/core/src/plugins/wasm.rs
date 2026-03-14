//! Load and run WASM plugins via Wasmtime.

use std::path::Path;

use anyhow::Result;
use wasmtime::*;

/// Handle to a loaded plugin (engine + module + store). Lifecycle TBD.
#[allow(dead_code)]
pub struct PluginHandle {
    _engine: Engine,
    _module: Module,
    _store: Store<()>,
}

/// Load a plugin from a `.wasm` file. Stub: instantiates and calls no exports yet.
#[allow(dead_code)]
pub fn load_plugin(path: &Path) -> Result<PluginHandle> {
    let engine = Engine::default();
    let module = Module::from_file(&engine, path)?;
    let mut store = Store::new(&engine, ());

    let mut linker = Linker::new(&engine);
    linker.func_wrap("env", "log", |_caller: Caller<'_, ()>, _ptr: i32, _len: i32| {
        tracing::debug!("plugin log (stub)");
    })?;

    let _instance = linker.instantiate(&mut store, &module)?;

    // Call plugin_init if present.
    if let Some(init) = _instance.get_func(&mut store, "plugin_init") {
        let init = init.typed::<(), ()>(&store)?;
        init.call(&mut store, ())?;
    }

    Ok(PluginHandle {
        _engine: engine,
        _module: module,
        _store: store,
    })
}
