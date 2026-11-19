//! Biomes mods add. Their order is kept in `config/moddedmc/biomes.txt` (`name look-alike json`
//! per line, append-only), so ids stored in chunks stay valid; a biome whose mod is gone stays
//! defined from that file, so its chunks keep it until the mod is back.

use std::{
    collections::HashSet,
    fs,
    path::Path,
    sync::{Arc, Mutex},
};

use pumpkin_core::{
    data::datapack::MODDED_REGISTRY_ENTRIES, net::java::CChunkData, server::Server,
};
use pumpkin_data::{biome::Biome, modded_biome};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::chunk::io::Dirtiable;

use crate::lock;

const KNOWN: &str = "config/moddedmc/biomes.txt";
const REGISTRY: &str = "minecraft:worldgen/biome";

/// A biome a mod defines: namespaced name, vanilla look-alike, client definition (JSON).
pub struct Def {
    pub name: String,
    pub look_alike: String,
    pub json: String,
}

/// Chunks whose biomes changed, resent once at the next tick.
static RESEND: Mutex<Vec<(String, Vector2<i32>)>> = Mutex::new(Vec::new());

/// Checks a definition: a namespaced, non-vanilla name, a vanilla look-alike, a JSON object.
pub fn valid(name: &str, look_alike: &str, json: &str) -> bool {
    let named = name.split_once(':').is_some_and(|(ns, path)| {
        ns != "minecraft"
            && !ns.is_empty()
            && !path.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./:".contains(c))
    });
    named
        && vanilla(look_alike).is_some()
        && serde_json::from_str::<serde_json::Value>(json).is_ok_and(|v| v.is_object())
}

fn vanilla(name: &str) -> Option<&'static Biome> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    Biome::ALL.iter().copied().find(|b| b.registry_id == name)
}

fn encode(d: &Def) -> String {
    // serde_json writes a JSON value on one line.
    let json = serde_json::from_str::<serde_json::Value>(&d.json)
        .map_or_else(|_| d.json.clone(), |v| v.to_string());
    format!("{} {} {json}", d.name, d.look_alike)
}

fn decode(line: &str) -> Option<Def> {
    let (name, rest) = line.split_once(' ')?;
    let (look_alike, json) = rest.split_once(' ')?;
    valid(name, look_alike, json).then(|| Def {
        name: name.to_owned(),
        look_alike: look_alike.to_owned(),
        json: json.to_owned(),
    })
}

/// Installs the mods' biomes after those already known. Call once, before any world loads.
pub fn install(defined: Vec<Def>) {
    let known: Vec<Def> = fs::read_to_string(KNOWN)
        .unwrap_or_default()
        .lines()
        .filter_map(decode)
        .collect();
    let had_known = !known.is_empty();
    let mut all = known;
    for def in defined {
        match all.iter_mut().find(|d| d.name == def.name) {
            Some(old) => *old = def,
            None => all.push(def),
        }
    }
    if all.is_empty() {
        return;
    }
    let text: String = all.iter().map(|d| encode(d) + "\n").collect();
    let saved = Path::new(KNOWN)
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(KNOWN, text));
    if let Err(e) = saved {
        tracing::warn!("moddedmc: could not write {KNOWN}: {e}");
    }
    let pairs: Vec<(String, &'static Biome)> = all
        .iter()
        .filter_map(|d| Some((d.name.clone(), vanilla(&d.look_alike)?)))
        .collect();
    if let Err(e) = modded_biome::install(&pairs) {
        if had_known {
            // Without the table, saved modded biomes would load as another biome.
            tracing::error!(
                "moddedmc: could not install modded biomes ({e}); refusing to start, because \
                 the worlds contain modded biomes that would be lost"
            );
            std::process::exit(1);
        }
        tracing::error!("moddedmc: could not install modded biomes: {e}");
        return;
    }
    let entries = all
        .iter()
        .filter_map(|d| {
            let json = serde_json::from_str(&d.json).ok()?;
            Some((REGISTRY, d.name.clone(), json))
        })
        .collect();
    let _ = MODDED_REGISTRY_ENTRIES.set(entries);
    for d in &all {
        tracing::info!("moddedmc: defined biome {} (like {})", d.name, d.look_alike);
    }
}

/// Maps modded biomes to their ids in the registry clients receive. Call once datapacks loaded.
pub fn map_network(server: &Server) {
    let registries = &server.datapack_manager.dynamic_registry_manager;
    let first = Biome::ALL.len();
    let ids: Vec<u8> = (first..modded_biome::MAX_TOTAL)
        .map_while(|id| modded_biome::by_id(u8::try_from(id).ok()?))
        .map(|b| {
            registries
                .get_raw_id(REGISTRY, b.registry_id)
                .and_then(|raw| u8::try_from(raw).ok())
                .map_or_else(
                    || {
                        tracing::warn!("moddedmc: biome {} is not in the registry", b.registry_id);
                        b.id
                    },
                    |network| {
                        tracing::info!("moddedmc: biome {} is network id {network}", b.registry_id);
                        network
                    },
                )
        })
        .collect();
    if !ids.is_empty() {
        modded_biome::set_network(ids);
    }
}

/// Quart (4-block) coordinates inside a chunk: x, y from the bottom, z.
fn quart(
    world: &pumpkin_core::world::World,
    x: i32,
    y: i32,
    z: i32,
) -> Option<(usize, usize, usize)> {
    let rel_y = (y >> 2) - (world.min_y >> 2);
    Some((
        usize::try_from((x >> 2) & 3).ok()?,
        usize::try_from(rel_y).ok()?,
        usize::try_from((z >> 2) & 3).ok()?,
    ))
}

/// Sets the biome at a block's 4x4x4 cell; false if unknown or the chunk is not loaded.
pub fn set(dimension: &str, x: i32, y: i32, z: i32, name: &str) -> bool {
    let (Some(world), Some(biome)) = (
        crate::native::world(dimension),
        Biome::from_name(name.strip_prefix("minecraft:").unwrap_or(name)),
    ) else {
        return false;
    };
    let Some((rx, ry, rz)) = quart(&world, x, y, z) else {
        return false;
    };
    let pos = Vector2::new(x >> 4, z >> 4);
    let set = world.level.read_chunk_sync(&pos, |chunk| {
        if chunk
            .section
            .get_noise_biome(ry / 4, rx, ry % 4, rz)
            .is_none()
        {
            return false;
        }
        chunk.section.set_relative_biome(rx, ry, rz, biome.id);
        chunk.mark_dirty(true);
        true
    });
    if set != Some(true) {
        return false;
    }
    let mut resend = lock(&RESEND);
    if !resend.iter().any(|(d, p)| d == dimension && *p == pos) {
        resend.push((dimension.to_owned(), pos));
    }
    true
}

/// The biome's name at a block; `None` if the chunk is not loaded.
pub fn at(dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
    let world = crate::native::world(dimension)?;
    let (rx, ry, rz) = quart(&world, x, y, z)?;
    let id = world
        .level
        .read_chunk_sync(&Vector2::new(x >> 4, z >> 4), |chunk| {
            chunk.section.get_noise_biome(ry / 4, rx, ry % 4, rz)
        })??;
    let name = Biome::from_id(id)?.registry_id;
    Some(if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    })
}

/// Sends players the chunks whose biomes changed since the last tick.
pub fn flush() {
    let chunks = std::mem::take(&mut *lock(&RESEND));
    let mut sent = HashSet::new();
    for (dimension, pos) in chunks {
        let Some(world) = crate::native::world(&dimension) else {
            continue;
        };
        if !sent.insert((dimension, pos.x, pos.y)) {
            continue;
        }
        let chunk: Option<Arc<_>> = world.level.read_chunk_sync(&pos, Arc::clone);
        if let Some(chunk) = chunk {
            world.broadcast_to_chunk_except(pos, &[], &CChunkData(&chunk));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_round_trip() {
        let def = Def {
            name: "frost:glacier".to_owned(),
            look_alike: "minecraft:snowy_plains".to_owned(),
            json: "{\n \"temperature\": -0.5\n}".to_owned(),
        };
        let line = encode(&def);
        assert!(!line.contains('\n'));
        let back = decode(&line).map(|d| (d.name, d.look_alike));
        assert_eq!(
            back,
            Some((
                "frost:glacier".to_owned(),
                "minecraft:snowy_plains".to_owned()
            ))
        );
        assert!(!valid("minecraft:glacier", "snowy_plains", "{}"));
        assert!(!valid("frost:glacier", "no_such_biome", "{}"));
        assert!(!valid("frost:glacier", "snowy_plains", "[1]"));
        assert!(valid("frost:glacier", "snowy_plains", "{}"));
    }
}
