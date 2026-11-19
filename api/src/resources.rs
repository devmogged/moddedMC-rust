//! The server's merged datapack files, for mods that load their own data such as recipe types.

use crate::{InterfaceUnavailable, abi, require_interface};

pub use abi::MAX_RESOURCE_BYTES;

#[derive(Clone, Copy)]
pub struct Resources(&'static abi::ResourcesV1);

pub fn require() -> Result<Resources, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:resources v1.
    unsafe { require_interface(abi::RESOURCES, 1) }.map(Resources)
}

unsafe extern "C" fn id(ctx: *mut std::ffi::c_void, _key: u64, value: abi::Str) {
    // SAFETY: ctx is the Vec the caller passed with this sink, and value is live until we return.
    let ids = unsafe { &mut *ctx.cast::<Vec<String>>() };
    // SAFETY: the host keeps value live until this sink returns.
    if let Ok(id) = std::str::from_utf8(unsafe { value.bytes() }) {
        ids.push(id.to_owned());
    }
}

impl Resources {
    /// Vanilla `ResourceManager.listResources`: the sorted `namespace:directory/...` ids of every
    /// file under `data/<namespace>/<directory>/`. `None` before datapacks load.
    pub fn list(self, directory: &str) -> Option<Vec<String>> {
        let mut ids = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut ids).cast(),
            put: id,
        };
        // SAFETY: the directory is borrowed for the call and the sink fills the local Vec.
        unsafe { (self.0.list)(abi::Str::new(directory.as_bytes()), sink) }.then_some(ids)
    }

    /// The highest-priority pack's bytes for a `list` id.
    pub fn read(self, id: &str) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the id is borrowed for the call and the sink fills the local Vec.
        unsafe { (self.0.read)(abi::Str::new(id.as_bytes()), sink) }.then_some(data)
    }

    /// 0 before datapacks load, then a new value after every load, `/reload` included.
    pub fn generation(self) -> u64 {
        // SAFETY: the table is live for the program.
        unsafe { (self.0.generation)() }
    }
}
