//! `moddedmc:tags` v1: vanilla and datapack tags by id string.

use std::collections::BTreeSet;

use moddedmc_api::abi::{self, Sink, Str};
use pumpkin_core::data::datapack::DatapackManager;
use pumpkin_data::tag::{RegistryKey, get_tag_values};

use crate::native::SERVER;

/// Vanilla rejects deeper or cyclic references; this only has to stop.
const MAX_TAG_DEPTH: u8 = 32;

/// Vanilla `ResourceLocation.parse`: a bare path is in `minecraft`.
fn full_id(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// Vanilla `TagLoader.build` flattening: generated vanilla entries, then every pack's, nested
/// tags expanded.
fn collect(
    manager: &DatapackManager,
    (key, registry): (RegistryKey, &str),
    tag: &str,
    depth: u8,
    out: &mut BTreeSet<String>,
) -> bool {
    if depth > MAX_TAG_DEPTH {
        return false;
    }
    let generated = get_tag_values(key, tag);
    if let Some(values) = generated {
        out.extend(values.iter().map(|value| full_id(value)));
    }
    let loaded = manager.get_tag_values(registry, tag);
    for value in loaded.iter().flatten() {
        match value.strip_prefix('#') {
            Some(nested) => {
                collect(manager, (key, registry), &full_id(nested), depth + 1, out);
            }
            None => {
                out.insert(full_id(value));
            }
        }
    }
    generated.is_some() || loaded.is_some()
}

/// The flattened tag, `None` for an unknown registry or tag.
fn resolve(registry: Str, tag: Str) -> Option<BTreeSet<String>> {
    // SAFETY: the caller borrows both strings for this call.
    let (registry, tag) = unsafe { (registry.str(), tag.str()) };
    let key = RegistryKey::from_string(registry.strip_prefix("minecraft:").unwrap_or(registry))?;
    let manager = &SERVER.get()?.datapack_manager;
    let tag = full_id(tag.strip_prefix('#').unwrap_or(tag));
    let mut out = BTreeSet::new();
    collect(manager, (key, key.identifier_string()), &tag, 0, &mut out).then_some(out)
}

unsafe extern "C" fn values(registry: Str, tag: Str, out: Sink) -> bool {
    let Some(values) = resolve(registry, tag) else {
        return false;
    };
    for value in &values {
        // SAFETY: the id is live until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, 0, Str::new(value.as_bytes())) };
    }
    true
}

unsafe extern "C" fn contains(registry: Str, tag: Str, id: Str) -> bool {
    // SAFETY: the caller borrows the id for this call.
    let id = full_id(unsafe { id.str() });
    resolve(registry, tag).is_some_and(|values| values.contains(&id))
}

pub static TAGS: abi::TagsV1 = abi::TagsV1 {
    size: size_of::<abi::TagsV1>(),
    values,
    contains,
};
