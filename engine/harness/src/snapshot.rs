use std::{collections::BTreeMap, fs::File, path::Path};

use pumpkin_data::Block;
use pumpkin_nbt::{Nbt, NbtCompound, deserializer::NbtReadHelperJava, nbt_compress, tag::NbtTag};
use pumpkin_util::math::vector2::Vector2;
use pumpkin_world::chunk::{
    format::anvil::AnvilRegion,
    io::{RegionFile, region_chunk_index, region_of},
    palette::PalettedContainer,
};
use uuid::Uuid;

use crate::scenario::{Area, Position};

const PLAYER_FIELDS: &[&str] = &[
    "Pos",
    "Rotation",
    "Dimension",
    "Health",
    "foodLevel",
    "foodTickTimer",
    "foodSaturationLevel",
    "foodExhaustionLevel",
    "XpLevel",
    "XpTotal",
    "XpP",
    "active_effects",
    "ActiveEffects",
    "Inventory",
    "EnderItems",
    "equipment",
    "SelectedItemSlot",
];

#[derive(Default)]
pub struct World {
    pub blocks: BTreeMap<Position, String>,
    pub block_entities: BTreeMap<Position, NbtCompound>,
    pub entities: Vec<Entity>,
}

#[derive(Clone)]
pub struct Entity {
    pub kind: String,
    pub position: [f64; 3],
    pub nbt: NbtCompound,
}

pub fn world(dir: &Path, area: Area) -> Result<World, String> {
    let dimension = dir.join("world/dimensions/minecraft/overworld");
    let terrain_dir = dimension.join("region");
    let entity_dir = dimension.join("entities");
    let mut terrain_regions = BTreeMap::new();
    let mut entity_regions = BTreeMap::new();
    let mut world = World::default();
    for (x, z) in area.chunks() {
        let chunk = Vector2::new(x, z);
        let data = read_region(&terrain_dir, &mut terrain_regions, chunk)?.ok_or_else(|| {
            format!(
                "missing saved terrain chunk {x} {z} in {}",
                terrain_dir.display()
            )
        })?;
        collect_blocks(&data, &mut world, area, x, z)
            .map_err(|e| format!("terrain chunk {x} {z} in {}: {e}", terrain_dir.display()))?;
        if let Some(entities) = read_region(&entity_dir, &mut entity_regions, chunk)? {
            collect_entities(&entities, &mut world, area)
                .map_err(|e| format!("entity chunk {x} {z} in {}: {e}", entity_dir.display()))?;
        }
    }
    if world.entities.len() > 4096 {
        return Err("snapshot exceeds 4096 entities".into());
    }
    Ok(world)
}

pub fn block_states(dir: &Path, area: Area) -> Result<BTreeMap<Position, String>, String> {
    let mut regions = BTreeMap::new();
    let mut blocks = BTreeMap::new();
    for (x, z) in area.chunks() {
        let data = read_region(dir, &mut regions, Vector2::new(x, z))?
            .ok_or_else(|| format!("missing terrain chunk {x} {z} in {}", dir.display()))?;
        collect_block_states(&data, &mut blocks, area, x, z)
            .map_err(|error| format!("terrain chunk {x} {z} in {}: {error}", dir.display()))?;
    }
    Ok(blocks)
}

/// Reads each quart intersecting the block area, retaining the names from the save.
pub fn biomes(dir: &Path, area: Area) -> Result<BTreeMap<Position, String>, String> {
    let terrain = dir.join("world/dimensions/minecraft/overworld/region");
    let quart_area = Area {
        min: area.min.map(|coordinate| coordinate >> 2),
        max: area.max.map(|coordinate| coordinate >> 2),
    };
    let mut regions = BTreeMap::new();
    let mut biomes = BTreeMap::new();
    for (x, z) in area.chunks() {
        let data = read_region(&terrain, &mut regions, Vector2::new(x, z))?
            .ok_or_else(|| format!("missing terrain chunk {x} {z} in {}", terrain.display()))?;
        collect_biomes(&data, &mut biomes, quart_area, x, z)
            .map_err(|error| format!("terrain chunk {x} {z} in {}: {error}", terrain.display()))?;
    }
    let count: usize = (0..3)
        .map(|axis| (quart_area.max[axis] - quart_area.min[axis] + 1) as usize)
        .product();
    if biomes.len() != count {
        return Err(format!(
            "expected {count} biome quarts, read {} in {}",
            biomes.len(),
            terrain.display()
        ));
    }
    Ok(biomes)
}

fn collect_biomes(
    data: &NbtCompound,
    biomes: &mut BTreeMap<Position, String>,
    quart_area: Area,
    x: i32,
    z: i32,
) -> Result<(), String> {
    validate_terrain_chunk(data, x, z)?;
    for entry in data.get_list("sections").ok_or("missing sections list")? {
        let section = compound(entry)?;
        let y = i32::from(section.get_byte("Y").ok_or("section with no Y")?) * 4;
        if y + 3 < quart_area.min[1] || y > quart_area.max[1] {
            continue;
        }
        let palette = section
            .get_compound("biomes")
            .ok_or("section with no biomes")?;
        let (names, indices) = decode_named_palette::<4>(palette, 0, |tag| {
            let NbtTag::String(name) = tag else {
                return Err("biome palette entries must be registry names".into());
            };
            let name = name.to_string();
            Ok(if name.contains(':') {
                name
            } else {
                format!("minecraft:{name}")
            })
        })?;
        for (index, id) in (&indices).into_iter().enumerate() {
            let quart = [
                x * 4 + (index & 3) as i32,
                y + (index >> 4) as i32,
                z * 4 + (index >> 2 & 3) as i32,
            ];
            if quart_area.contains(quart)
                && biomes.insert(quart, names[id as usize].clone()).is_some()
            {
                return Err(format!("duplicate biome at quart {quart:?}"));
            }
        }
    }
    Ok(())
}

fn read_region(
    dir: &Path,
    regions: &mut BTreeMap<(i32, i32), AnvilRegion>,
    chunk: Vector2<i32>,
) -> Result<Option<NbtCompound>, String> {
    use std::collections::btree_map::Entry;
    let region_pos = region_of(chunk);
    let path = dir.join(AnvilRegion::file_name(region_pos));
    let error = |e: &dyn std::fmt::Display| {
        format!("{} chunk {} {}: {e}", path.display(), chunk.x, chunk.y)
    };
    let region = match regions.entry((region_pos.x, region_pos.y)) {
        Entry::Occupied(entry) => entry.into_mut(),
        Entry::Vacant(entry) => {
            entry.insert(AnvilRegion::open(&path, region_pos).map_err(|e| error(&e))?)
        }
    };
    let Some(raw) = region
        .read_chunk(region_chunk_index(chunk))
        .map_err(|e| error(&e))?
    else {
        return Ok(None);
    };
    let nbt = AnvilRegion::decode_chunk(raw).map_err(|e| error(&e))?;
    let mut input = std::io::Cursor::new(&nbt[..]);
    let data = Nbt::read(&mut NbtReadHelperJava::new(&mut input)).map_err(|e| error(&e))?;
    if input.position() != nbt.len() as u64 {
        return Err(error(&"trailing bytes after chunk NBT"));
    }
    Ok(Some(data.root_tag))
}

fn validate_terrain_chunk(data: &NbtCompound, x: i32, z: i32) -> Result<(), String> {
    if data.get_int("xPos") != Some(x) || data.get_int("zPos") != Some(z) {
        return Err("stored coordinates do not match the region slot".into());
    }
    if !data
        .get_string("Status")
        .is_some_and(|state| matches!(state, "minecraft:full" | "full"))
    {
        return Err(format!(
            "expected a full chunk, got {:?}",
            data.get("Status")
        ));
    }
    Ok(())
}

fn collect_blocks(
    data: &NbtCompound,
    world: &mut World,
    area: Area,
    x: i32,
    z: i32,
) -> Result<(), String> {
    collect_block_states(data, &mut world.blocks, area, x, z)?;
    for entity in optional_list(data, "block_entities")? {
        let entity = compound(entity)?;
        let pos = ["x", "y", "z"].map(|axis| entity.get_int(axis));
        let [Some(x), Some(y), Some(z)] = pos else {
            return Err("block entity with missing integer coordinates".into());
        };
        if area.contains([x, y, z])
            && world
                .block_entities
                .insert([x, y, z], entity.clone())
                .is_some()
        {
            return Err(format!("duplicate block entity at {x} {y} {z}"));
        }
    }
    Ok(())
}

fn collect_block_states(
    data: &NbtCompound,
    blocks: &mut BTreeMap<Position, String>,
    area: Area,
    x: i32,
    z: i32,
) -> Result<(), String> {
    validate_terrain_chunk(data, x, z)?;
    let mut section_ids = std::collections::BTreeSet::new();
    let sections = data.get_list("sections").ok_or("missing sections list")?;
    for section in sections {
        let section = compound(section)?;
        let base_y = i32::from(section.get_byte("Y").ok_or("section with no Y")?) * 16;
        if !section_ids.insert(base_y) {
            return Err(format!("duplicate section at Y={base_y}"));
        }
        if base_y + 15 < area.min[1] || base_y > area.max[1] {
            continue;
        }
        let Some(states) = section.get("block_states") else {
            continue;
        };
        let (names, indices) = decode_palette(compound(states)?)?;
        for (index, id) in (&indices).into_iter().enumerate() {
            let pos = [
                x * 16 + (index & 15) as i32,
                base_y + (index >> 8) as i32,
                z * 16 + ((index >> 4) & 15) as i32,
            ];
            if names[id as usize] != "minecraft:air" && area.contains(pos) {
                blocks.insert(pos, names[id as usize].clone());
            }
        }
    }
    Ok(())
}

fn decode_palette(
    states: &NbtCompound,
) -> Result<(Vec<String>, PalettedContainer<u16, 16>), String> {
    decode_named_palette::<16>(states, 4, state_name)
}

fn decode_named_palette<const DIM: usize>(
    states: &NbtCompound,
    min_bits: u8,
    name: impl Fn(&NbtTag) -> Result<String, String>,
) -> Result<(Vec<String>, PalettedContainer<u16, DIM>), String> {
    let volume = PalettedContainer::<u16, DIM>::VOLUME;
    let palette = states.get_list("palette").ok_or("missing palette")?;
    if !(1..=volume).contains(&palette.len()) {
        return Err(format!("invalid palette length {}", palette.len()));
    }
    let names = palette.iter().map(name).collect::<Result<Vec<_>, _>>()?;
    let data = match states.get("data") {
        Some(NbtTag::LongArray(values)) => values.as_slice(),
        None if palette.len() == 1 => &[],
        _ => return Err("palette is missing its packed long array".into()),
    };
    let bits = (palette.len() - 1)
        .checked_ilog2()
        .map_or(0, |bits| bits + 1)
        .max(u32::from(min_bits));
    if palette.len() > 1 {
        let per_word = 64 / bits as usize;
        let mask = (1 << bits) - 1;
        if data.len() != volume.div_ceil(per_word)
            || (0..volume).any(|i| {
                let word = data[i / per_word] as u64;
                ((word >> ((i % per_word) * bits as usize)) & mask) as usize >= palette.len()
            })
        {
            return Err("packed palette contains an invalid length or index".into());
        }
    } else if !data.is_empty() {
        return Err("homogeneous palette unexpectedly contains packed data".into());
    }
    let values = (0..palette.len() as u16).collect::<Vec<_>>();
    Ok((
        names,
        PalettedContainer::from_palette_and_packed_data(&values, data, min_bits),
    ))
}

fn state_name(state: &NbtTag) -> Result<String, String> {
    // Vanilla 26.3 emits a registry string for default states and an id/properties map otherwise.
    let (name, properties) = match state {
        NbtTag::String(name) => (name.as_ref(), None),
        NbtTag::Compound(state) => {
            if let Some(name) = state.get_string("id") {
                (name, state.get("properties"))
            } else {
                (
                    state
                        .get_string("Name")
                        .ok_or("palette entry without a block ID")?,
                    state.get("Properties"),
                )
            }
        }
        _ => return Err("palette entry must be a registry string or a state compound".into()),
    };
    let mut fields = BTreeMap::new();
    if let Some(block) = Block::from_name(name)
        && let Some(defaults) = block.properties(block.default_state.id)
    {
        fields.extend(
            defaults
                .to_props()
                .into_iter()
                .map(|value| (value.key_str(), value.value_str())),
        );
    }
    if let Some(properties) = properties {
        for (key, value) in &compound(properties)?.child_tags {
            let NbtTag::String(value) = value else {
                return Err(format!("property {key} of {name} is not a string"));
            };
            fields.insert(key.as_ref(), value.as_ref());
        }
    }
    if fields.is_empty() {
        return Ok(name.into());
    }
    let values = fields
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>();
    Ok(format!("{name}[{}]", values.join(",")))
}

fn collect_entities(data: &NbtCompound, world: &mut World, area: Area) -> Result<(), String> {
    for entity in data
        .get_list("Entities")
        .ok_or("missing Entities list in entity chunk")?
    {
        let entity = compound(entity)?;
        let kind = entity.get_string("id").ok_or("entity without an id")?;
        let position = match entity.get_list("Pos") {
            Some([NbtTag::Double(x), NbtTag::Double(y), NbtTag::Double(z)])
                if [x, y, z].iter().all(|n| n.is_finite()) =>
            {
                [*x, *y, *z]
            }
            _ => return Err(format!("entity {kind} does not have a finite double Pos")),
        };
        if area.contains_entity(position) {
            world.entities.push(Entity {
                kind: kind.into(),
                position,
                nbt: entity.clone(),
            });
        }
    }
    Ok(())
}

pub fn player(dir: &Path, name: &str, vanilla: bool) -> Result<NbtCompound, String> {
    let uuid = if vanilla {
        use md5::{Digest as _, Md5};
        let mut bytes: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}")).into();
        bytes[6] = (bytes[6] & 0x0f) | 0x30;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Uuid::from_bytes(bytes)
    } else {
        use sha2::{Digest as _, Sha256};
        Uuid::from_slice(&Sha256::digest(name)[..16]).map_err(|e| e.to_string())?
    };
    let path = dir.join("world/players/data").join(format!("{uuid}.dat"));
    let context =
        |e: &dyn std::fmt::Display| format!("saved player {name} at {}: {e}", path.display());
    let file = File::open(&path).map_err(|e| context(&e))?;
    let mut data = nbt_compress::read_gzip_compound_tag(file).map_err(|e| context(&e))?;
    data.child_tags
        .retain(|key, _| PLAYER_FIELDS.contains(&key.as_ref()));
    if !data.child_tags.contains_key("Pos") || !data.child_tags.contains_key("Health") {
        return Err(context(&"missing core player fields"));
    }
    Ok(data)
}

fn optional_list<'a>(data: &'a NbtCompound, field: &str) -> Result<&'a [NbtTag], String> {
    match data.get(field) {
        Some(NbtTag::List(list)) => Ok(list),
        None => Ok(&[]),
        _ => Err(format!("{field} must be an NBT list")),
    }
}

fn compound(tag: &NbtTag) -> Result<&NbtCompound, String> {
    match tag {
        NbtTag::Compound(data) => Ok(data),
        _ => Err("expected an NBT compound".into()),
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_nbt::{NbtCompound, nbt_compound_tag, tag::NbtTag};

    use super::{collect_biomes, decode_palette, state_name};

    #[test]
    fn reads_custom_biome_in_negative_quart_coordinates() -> Result<(), String> {
        let mut palette = NbtCompound::new();
        palette.put_list(
            "palette",
            vec![
                NbtTag::String("minecraft:plains".into()),
                NbtTag::String("pack:new_biome".into()),
            ],
        );
        palette.put("data", NbtTag::LongArray(vec![1 << 21]));
        let mut section = NbtCompound::new();
        section.put_byte("Y", -3);
        section.put_compound("biomes", palette);
        let mut chunk = NbtCompound::new();
        chunk.put_int("xPos", -1);
        chunk.put_int("zPos", 4);
        chunk.put_string("Status", "minecraft:full".into());
        chunk.put_list("sections", vec![NbtTag::Compound(section)]);
        let area = crate::scenario::Area {
            min: [-3, -11, 17],
            max: [-3, -11, 17],
        };
        let mut quarts = std::collections::BTreeMap::new();
        collect_biomes(&chunk, &mut quarts, area, -1, 4)?;
        assert_eq!(quarts[&[-3, -11, 17]], "pack:new_biome");
        Ok(())
    }

    #[test]
    fn compact_default_and_explicit_piston_states_compare_equally() {
        let compact = NbtTag::String("minecraft:piston".into());
        let explicit = nbt_compound_tag!(
            "Name": NbtTag::String("minecraft:piston".into()),
            "Properties": nbt_compound_tag!(
                "extended": NbtTag::String("false".into()),
                "facing": NbtTag::String("north".into())
            )
        );
        assert_eq!(state_name(&compact), state_name(&explicit));
        let changed = nbt_compound_tag!(
            "id": NbtTag::String("minecraft:piston".into()),
            "properties": nbt_compound_tag!("facing": NbtTag::String("east".into()))
        );
        assert_eq!(
            state_name(&changed).as_deref(),
            Ok("minecraft:piston[extended=false,facing=east]")
        );
    }

    #[test]
    fn corrupted_palette_indices_fail_instead_of_falling_back_to_air() {
        let mut section = NbtCompound::new();
        section.put_list(
            "palette",
            vec![
                NbtTag::String("minecraft:air".into()),
                NbtTag::String("minecraft:stone".into()),
            ],
        );
        let mut words = vec![0; 256];
        words[17] = 2;
        section.put("data", NbtTag::LongArray(words));
        assert!(decode_palette(&section).is_err());
    }
}
