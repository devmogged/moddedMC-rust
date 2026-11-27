//! `moddedmc:tags` v1: vanilla and datapack tags by id string.

use std::collections::BTreeSet;

use moddedmc_api::abi::{self, Sink, Str};
use pumpkin_core::data::datapack::DatapackManager;
use pumpkin_data::tag::{RegistryKey, get_latest_map, get_tag_references, get_tag_values};

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
        if let Some(key) = key {
            for nested in get_tag_references(key, tag) {
                collect(manager, (Some(key), registry), nested, depth + 1, out);
            }
        }
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

fn tags_containing(manager: &DatapackManager, key: RegistryKey, entry: &str) -> BTreeSet<String> {
    let registry = key.identifier_string();
    let names: BTreeSet<_> = manager
        .get_tags_for_category(registry)
        .into_keys()
        .chain(get_latest_map(key).keys().map(|name| full_id(name)))
        .collect();
    names
        .into_iter()
        .filter(|tag| {
            let mut members = BTreeSet::new();
            collect(manager, (Some(key), registry), tag, 0, &mut members);
            members.contains(entry)
        })
        .collect()
}

unsafe extern "C" fn memberships(registry: Str, entry: Str, out: Sink) -> bool {
    // SAFETY: the caller borrows both strings for this call.
    let (registry, entry) = unsafe { (registry.str(), entry.str()) };
    let Some(key) =
        RegistryKey::from_string(registry.strip_prefix("minecraft:").unwrap_or(registry))
    else {
        return false;
    };
    let Some(server) = SERVER.get() else {
        return false;
    };
    for tag in tags_containing(&server.datapack_manager, key, &full_id(entry)) {
        // SAFETY: the tag is live until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, 0, Str::new(tag.as_bytes())) };
    }
    true
}

pub static TAGS: abi::TagsV1Memberships = abi::TagsV1Memberships {
    base: abi::TagsV1 {
        size: size_of::<abi::TagsV1Memberships>(),
        values,
        contains,
    },
    memberships,
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
        let item_tags = pack.join("data/minecraft/tags/item");
        fs::create_dir_all(&item_tags)?;
        fs::write(item_tags.join("oak_logs.json"), r#"{"values": ["dirt"]}"#)?;

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
        let item_tags = tags_containing(&manager, RegistryKey::Item, "minecraft:dirt");
        assert!(item_tags.contains("minecraft:oak_logs"));
        assert!(item_tags.contains("minecraft:logs"));
        assert!(!item_tags.contains("minecraft:wool"));
        Ok(())
    }
}
