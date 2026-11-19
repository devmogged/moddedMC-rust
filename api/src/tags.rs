//! Vanilla and datapack tags by id string, so a mod's own blocks and items count too.

use crate::{InterfaceUnavailable, abi, require_interface};

#[derive(Clone, Copy)]
pub struct Tags(&'static abi::TagsV1);

pub fn require() -> Result<Tags, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:tags v1.
    unsafe { require_interface(abi::TAGS, 1) }.map(Tags)
}

unsafe extern "C" fn id(ctx: *mut std::ffi::c_void, _key: u64, value: abi::Str) {
    // SAFETY: ctx is the Vec the caller passed with this sink, and value is live until we return.
    let ids = unsafe { &mut *ctx.cast::<Vec<String>>() };
    // SAFETY: the host keeps value live until this sink returns.
    if let Ok(id) = std::str::from_utf8(unsafe { value.bytes() }) {
        ids.push(id.to_owned());
    }
}

impl Tags {
    /// The tag's entry ids, sorted, nested tags flattened. `registry` is a name such as `item`;
    /// `tag` is `namespace:path`, `#` optional. `None` when no pack defines the tag.
    pub fn values(self, registry: &str, tag: &str) -> Option<Vec<String>> {
        let mut ids = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut ids).cast(),
            put: id,
        };
        // SAFETY: the strings are borrowed for the call and the sink fills the local Vec.
        unsafe {
            (self.0.values)(
                abi::Str::new(registry.as_bytes()),
                abi::Str::new(tag.as_bytes()),
                sink,
            )
        }
        .then_some(ids)
    }

    /// Whether `id` is in the tag.
    pub fn contains(self, registry: &str, tag: &str, id: &str) -> bool {
        // SAFETY: the strings are borrowed for the call.
        unsafe {
            (self.0.contains)(
                abi::Str::new(registry.as_bytes()),
                abi::Str::new(tag.as_bytes()),
                abi::Str::new(id.as_bytes()),
            )
        }
    }
}
