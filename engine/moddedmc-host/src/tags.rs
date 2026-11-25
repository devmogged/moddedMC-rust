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
/// tags expanded. A pack's `replace` drops the generated entries.
fn collect(
    manager: &DatapackManager,
    (key, registry): (Option<RegistryKey>, &str),
    tag: &str,
    depth: u8,
    out: &mut BTreeSet<String>,
) -> bool {
    if depth > MAX_TAG_DEPTH {
        return false;
    }
    let loaded = manager.get_tag(registry, tag);
    let generated = key.and_then(|key| get_tag_values(key, tag));
    if let Some(values) = generated
        && !loaded.as_ref().is_some_and(|entry| entry.replace)
    {
        out.extend(values.iter().map(|value| full_id(value)));
    }
    for value in loaded.iter().flat_map(|entry| &entry.values) {
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
    let registry = registry.strip_prefix("minecraft:").unwrap_or(registry);
    let key = RegistryKey::from_string(registry);
    if key.is_none() && registry != "recipe_serializer" {
        return None;
    }
    let manager = &SERVER.get()?.datapack_manager;
    let tag = full_id(tag.strip_prefix('#').unwrap_or(tag));
    let mut out = BTreeSet::new();
    collect(
        manager,
        (
            key,
            key.as_ref()
                .map_or(registry, RegistryKey::identifier_string),
        ),
        &tag,
        0,
        &mut out,
    )
    .then_some(out)
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

#[cfg(test)]
mod tests {
    use std::fs;

    use pumpkin_core::server::recipe::RecipeManager;

    use super::*;

    #[test]
    fn replace_drops_generated_entries() -> std::io::Result<()> {
        let world = std::env::temp_dir().join(format!("moddedmc-tags-{}", std::process::id()));
        let pack = world.join("datapacks/replace");
        let tags = pack.join("data/minecraft/tags/block");
        fs::create_dir_all(&tags)?;
        fs::write(
            pack.join("pack.mcmeta"),
            r#"{"pack": {"pack_format": 80, "description": ""}}"#,
        )?;
        fs::write(
            tags.join("wool.json"),
            r#"{"replace": true, "values": ["dirt"]}"#,
        )?;
        fs::write(tags.join("planks.json"), r#"{"values": ["dirt"]}"#)?;

        let manager = DatapackManager::new();
        manager.load_all(&world, &["file/replace".to_owned()], &RecipeManager::new());
        fs::remove_dir_all(&world)?;

        let flatten = |tag| {
            let mut out = BTreeSet::new();
            collect(
                &manager,
                (Some(RegistryKey::Block), "block"),
                tag,
                0,
                &mut out,
            );
            out
        };
        assert_eq!(
            flatten("minecraft:wool"),
            BTreeSet::from(["minecraft:dirt".to_owned()])
        );
        let planks = flatten("minecraft:planks");
        assert!(planks.contains("minecraft:dirt") && planks.contains("minecraft:oak_planks"));
        Ok(())
    }
}
