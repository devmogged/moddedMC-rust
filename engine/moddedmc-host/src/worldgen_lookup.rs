//! Read the active generator, including datapack biome sources, without chunk tickets.
use moddedmc_api::abi::{self, Pos, Sink, Str, WorldRef};
use pumpkin_data::{
    biome::Biome,
    structures::{StructureKeys, StructurePlacementType, StructureSet},
};
use pumpkin_world::{
    biome::BiomeSupplier,
    generation::{
        generator::{
            VanillaGenerator, WorldGenerator,
            structure_finder::{locate_pos, structure_start_at},
        },
        noise::router::multi_noise_sampler::MultiNoiseSampler,
        structure::placement::{RingLayout, get_structure_chunk_in_region},
    },
};

pub static LOOKUP: abi::WorldgenLookupV1 = abi::WorldgenLookupV1 {
    size: size_of::<abi::WorldgenLookupV1>(),
    biomes,
    noise_biomes,
    height_bounds,
};

pub static STRUCTURES: abi::WorldgenStructuresV1 = abi::WorldgenStructuresV1 {
    size: size_of::<abi::WorldgenStructuresV1>(),
    structures,
    placements,
    spread_candidate,
    ring_chunks,
    structure_start,
};

fn emit(out: &Sink, key: u64, name: &str) {
    let id = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    // SAFETY: identifier bytes remain live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, key, Str::from(id.as_str())) };
}

unsafe extern "C" fn biomes(raw: WorldRef, out: Sink) -> bool {
    // SAFETY: the world is borrowed by the caller for this callback.
    let Some(world) = (unsafe { crate::content::world(raw) }) else {
        return false;
    };
    let generator = world.level.world_gen.load_full();
    match generator.as_ref() {
        WorldGenerator::Noise(generator) => {
            let mut ids: Vec<_> = generator.possible_biomes.iter().copied().collect();
            ids.sort_unstable();
            for id in ids {
                let Some(biome) = u8::try_from(id).ok().and_then(Biome::from_id) else {
                    return false;
                };
                emit(&out, u64::from(id), biome.registry_id);
            }
        }
        WorldGenerator::Flat(generator) => emit(&out, 0, &generator.biome),
        WorldGenerator::Custom(_) => return false,
    }
    true
}

unsafe extern "C" fn height_bounds(raw: WorldRef, bottom: *mut i32, top: *mut i32) -> bool {
    // SAFETY: the caller borrows the world and writable outputs.
    let (Some(world), Some(bottom), Some(top)) = (
        unsafe { crate::content::world(raw) },
        unsafe { bottom.as_mut() },
        unsafe { top.as_mut() },
    ) else {
        return false;
    };
    let generator = world.level.world_gen.load_full();
    let dimension = generator.dimension();
    *bottom = dimension.min_y;
    *top = dimension.min_y.saturating_add(dimension.height);
    *top > *bottom
}

fn enabled_set(generator: &VanillaGenerator, index: u32) -> Option<&'static StructureSet> {
    let index = usize::try_from(index).ok()?;
    generator
        .enabled_structure_sets
        .contains(&index)
        .then_some(())?;
    StructureSet::all().get(index).copied()
}

unsafe extern "C" fn structures(raw: WorldRef, out: Sink) -> bool {
    // SAFETY: the caller borrows the world.
    let Some(world) = (unsafe { crate::content::world(raw) }) else {
        return false;
    };
    let generator = world.level.world_gen.load_full();
    match generator.as_ref() {
        WorldGenerator::Flat(_) => true,
        WorldGenerator::Custom(_) => false,
        WorldGenerator::Noise(_) => {
            for name in StructureKeys::all_names() {
                let Some(key) = StructureKeys::from_name(name) else {
                    continue;
                };
                if StructureSet::all().iter().enumerate().any(|(index, set)| {
                    set.structures.iter().any(|e| e.structure == key)
                        && generator.can_locate_structure(index, key)
                }) {
                    emit(&out, 0, name);
                }
            }
            true
        }
    }
}

unsafe extern "C" fn placements(raw: WorldRef, target: Str, out: Sink) -> bool {
    // SAFETY: the caller borrows the world and target.
    let (Some(world), Some(key)) = (
        unsafe { crate::content::world(raw) },
        StructureKeys::from_name(unsafe { target.str() }),
    ) else {
        return false;
    };
    let generator = world.level.world_gen.load_full();
    if !matches!(generator.as_ref(), WorldGenerator::Noise(_)) {
        return matches!(generator.as_ref(), WorldGenerator::Flat(_));
    }
    for (index, set) in StructureSet::all().iter().enumerate() {
        if !set.structures.iter().any(|e| e.structure == key)
            || !generator.can_locate_structure(index, key)
        {
            continue;
        }
        let spacing = match &set.placement.placement_type {
            StructurePlacementType::RandomSpread(p) => p.spacing,
            StructurePlacementType::ConcentricRings(_) => 0,
        };
        let bytes = spacing.to_le_bytes();
        // SAFETY: the sink copies bytes synchronously.
        unsafe { (out.put)(out.ctx, index as u64, Str::new(&bytes)) };
    }
    true
}

const CHUNK_LIMIT: u32 = 1_875_000;
fn valid_chunk(x: i32, z: i32) -> bool {
    x.unsigned_abs() <= CHUNK_LIMIT && z.unsigned_abs() <= CHUNK_LIMIT
}

unsafe extern "C" fn spread_candidate(
    raw: WorldRef,
    placement: u32,
    x: i32,
    z: i32,
    out: *mut Pos,
) -> bool {
    if !valid_chunk(x, z) {
        return false;
    }
    // SAFETY: the caller borrows the world and writable output.
    let (Some(world), Some(out)) = (unsafe { crate::content::world(raw) }, unsafe {
        out.as_mut()
    }) else {
        return false;
    };
    let generator = world.level.world_gen.load_full();
    let WorldGenerator::Noise(generator) = generator.as_ref() else {
        return false;
    };
    let Some(set) = enabled_set(generator, placement) else {
        return false;
    };
    let StructurePlacementType::RandomSpread(p) = &set.placement.placement_type else {
        return false;
    };
    if p.spacing <= 0
        || p.spacing > CHUNK_LIMIT as i32
        || p.separation < 0
        || p.separation >= p.spacing
    {
        return false;
    }
    let (x, z) = get_structure_chunk_in_region(
        p,
        generator.random_config.seed as i64,
        x.div_euclid(p.spacing),
        z.div_euclid(p.spacing),
        set.placement.salt,
    );
    if !valid_chunk(x, z) {
        return false;
    }
    *out = Pos { x, y: 0, z };
    true
}

unsafe extern "C" fn ring_chunks(raw: WorldRef, placement: u32, out: Sink) -> i32 {
    // SAFETY: the caller borrows the world.
    let Some(world) = (unsafe { crate::content::world(raw) }) else {
        return -1;
    };
    let generator = world.level.world_gen.load_full();
    let WorldGenerator::Noise(generator) = generator.as_ref() else {
        return -1;
    };
    let Some(set) = enabled_set(generator, placement) else {
        return -1;
    };
    let StructurePlacementType::ConcentricRings(rings) = &set.placement.placement_type else {
        return -1;
    };
    match generator
        .global_structure_cache
        .try_get_ring_chunks(generator.random_config.seed as i64, rings)
    {
        RingLayout::Pending => 2,
        RingLayout::Failed => -1,
        RingLayout::Ready(chunks) => {
            for (index, &(x, z)) in chunks.iter().enumerate() {
                let mut bytes = [0; 8];
                bytes[..4].copy_from_slice(&x.to_le_bytes());
                bytes[4..].copy_from_slice(&z.to_le_bytes());
                // SAFETY: bytes are live until the sink returns.
                unsafe { (out.put)(out.ctx, index as u64, Str::new(&bytes)) };
            }
            1
        }
    }
}

unsafe extern "C" fn structure_start(
    raw: WorldRef,
    placement: u32,
    target: Str,
    x: i32,
    z: i32,
    out: *mut Pos,
) -> i32 {
    if !valid_chunk(x, z) {
        return -1;
    }
    // SAFETY: the caller borrows the world, target and writable output.
    let (Some(world), Some(key), Some(out)) = (
        unsafe { crate::content::world(raw) },
        StructureKeys::from_name(unsafe { target.str() }),
        unsafe { out.as_mut() },
    ) else {
        return -1;
    };
    let generator = world.level.world_gen.load_full();
    let WorldGenerator::Noise(generator) = generator.as_ref() else {
        return -1;
    };
    let Some(set) = enabled_set(generator, placement) else {
        return -1;
    };
    if !set.structures.iter().any(|entry| entry.structure == key)
        || !generator.can_locate_structure(placement as usize, key)
    {
        return -1;
    }
    if let StructurePlacementType::ConcentricRings(rings) = &set.placement.placement_type {
        match generator
            .global_structure_cache
            .try_get_ring_chunks(generator.random_config.seed as i64, rings)
        {
            RingLayout::Pending => return 2,
            RingLayout::Failed => return -1,
            RingLayout::Ready(_) => {}
        }
    }
    if structure_start_at(generator, set, x, z) != Some(key) {
        return 0;
    }
    let pos = locate_pos(&set.placement, x, z);
    *out = Pos {
        x: pos.0.x,
        y: pos.0.y,
        z: pos.0.z,
    };
    1
}

unsafe extern "C" fn noise_biomes(
    raw: WorldRef,
    positions: *const Pos,
    count: usize,
    out: Sink,
) -> bool {
    if count > 256 || (count != 0 && positions.is_null()) {
        return false;
    }
    // SAFETY: the world is borrowed by the caller for this callback.
    let Some(world) = (unsafe { crate::content::world(raw) }) else {
        return false;
    };
    let generator = world.level.world_gen.load_full();
    // SAFETY: the caller provides count readable positions. No dereference for empty input.
    let positions = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(positions, count) }
    };
    // Avoid overflowing the generator's quart-to-block conversion.
    if positions
        .iter()
        .any(|p| [p.x, p.y, p.z].iter().any(|v| v.unsigned_abs() > 7_500_000))
    {
        return false;
    }
    match generator.as_ref() {
        WorldGenerator::Noise(generator) => {
            let mut sampler = MultiNoiseSampler::generate(&generator.base_router.multi_noise);
            for (index, pos) in positions.iter().enumerate() {
                let biome = generator
                    .biome_supplier
                    .biome(pos.x, pos.y, pos.z, &mut sampler);
                emit(&out, index as u64, biome.registry_id);
            }
        }
        WorldGenerator::Flat(generator) => {
            for index in 0..count {
                emit(&out, index as u64, &generator.biome);
            }
        }
        WorldGenerator::Custom(_) => return false,
    }
    true
}
