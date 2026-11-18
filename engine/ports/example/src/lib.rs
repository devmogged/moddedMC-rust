//! The smallest compiled-in port: it loads with the server and says so. Copy this crate to
//! start a port, then add it to a pack with `<id> = { path = "..." }`.

use std::sync::Arc;

use pumpkin_core::plugin::{Context, Plugin, PluginFuture, PluginMetadata};

struct Example;

impl Plugin for Example {
    fn on_load(&self, _context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async {
            tracing::info!("port-example loaded");
            Ok(())
        })
    }
}

/// What the generated pack server registers: `launch.builtin_plugins.push(port_example::builtin())`.
pub fn builtin() -> (PluginMetadata, Arc<dyn Plugin>) {
    let metadata = PluginMetadata {
        name: "port-example".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        authors: vec!["devmogged".into()],
        description: "Template for compiled-in ports.".into(),
        dependencies: Vec::new(),
        permissions: Vec::new(),
    };
    (metadata, Arc::new(Example))
}
