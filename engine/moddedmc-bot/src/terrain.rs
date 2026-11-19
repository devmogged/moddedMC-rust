use std::collections::HashMap;

use pumpkin_data::{
    BlockStateId,
    packet::{CURRENT_MC_VERSION, PacketId, clientbound::play as p},
};
use pumpkin_protocol::{
    ServerPacket,
    java::client::play::{CMultiBlockUpdate, CUnloadChunk},
    ser::{NetworkReadExt as _, NetworkReadSliceExt as _},
};
use pumpkin_util::math::{boundingbox::BoundingBox, position::BlockPos, vector3::Vector3};

const SECTION_VOLUME: usize = 16 * 16 * 16;

enum Section {
    Uniform(BlockStateId),
    Dense(Box<[BlockStateId]>),
}

impl Section {
    fn get(&self, index: usize) -> BlockStateId {
        match self {
            Self::Uniform(state) => *state,
            Self::Dense(states) => states[index],
        }
    }

    fn set(&mut self, index: usize, state: BlockStateId) {
        if let Self::Uniform(old) = self {
            if *old == state {
                return;
            }
            *self = Self::Dense(vec![*old; SECTION_VOLUME].into_boxed_slice());
        }
        if let Self::Dense(states) = self {
            states[index] = state;
        }
    }
}

struct Chunk {
    min_section: i32,
    sections: Vec<Section>,
}

impl Chunk {
    const fn section(&self, y: i32) -> usize {
        (y.div_euclid(16) - self.min_section) as usize
    }
}

#[derive(Default)]
pub struct World {
    chunks: HashMap<(i32, i32), Chunk>,
}

impl World {
    pub fn observe(&mut self, kind: i32, mut bytes: &[u8]) -> Result<(), String> {
        match PacketId(kind) {
            p::LEVEL_CHUNK_WITH_LIGHT => {
                let x = bytes.get_i32_be().map_err(|error| error.to_string())?;
                let z = bytes.get_i32_be().map_err(|error| error.to_string())?;
                for _ in 0..count(&mut bytes, 16)? {
                    var(&mut bytes)?;
                    let size = count(&mut bytes, 64)?;
                    bytes
                        .read_slice_borrowed(size * 8)
                        .map_err(|error| error.to_string())?;
                }
                let length = count(&mut bytes, 2 * 1024 * 1024)?;
                let chunk = read_sections(
                    bytes
                        .read_slice_borrowed(length)
                        .map_err(|error| error.to_string())?,
                )?;
                self.chunks.insert((x, z), chunk);
            }
            p::FORGET_LEVEL_CHUNK => {
                let chunk = CUnloadChunk::read(&mut bytes, &CURRENT_MC_VERSION)
                    .map_err(|error| error.to_string())?;
                self.chunks.remove(&(chunk.x, chunk.z));
            }
            p::RESPAWN => self.chunks.clear(),
            p::BLOCK_UPDATE => {
                let pos = bytes
                    .get_block_pos(&CURRENT_MC_VERSION)
                    .map_err(|error| error.to_string())?;
                self.set(pos, var(&mut bytes)?)?;
            }
            p::SECTION_BLOCKS_UPDATE => {
                let mut header = bytes;
                header.get_i64_be().map_err(|error| error.to_string())?;
                count(&mut header, SECTION_VOLUME)?;
                let updates = CMultiBlockUpdate::read(&mut bytes, &CURRENT_MC_VERSION)
                    .map_err(|error| error.to_string())?;
                for (pos, state) in updates.updates {
                    self.set(pos, i32::from(state.as_u16()))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn block(&self, pos: Vector3<i32>) -> Option<BlockStateId> {
        let chunk = self
            .chunks
            .get(&(pos.x.div_euclid(16), pos.z.div_euclid(16)))?;
        Some(
            chunk
                .sections
                .get(chunk.section(pos.y))
                .map_or(BlockStateId::AIR, |section| section.get(index(pos))),
        )
    }

    pub fn shapes(&self, bounds: BoundingBox) -> Vec<BoundingBox> {
        let mut shapes = Vec::new();
        self.visit_shapes(bounds, |_, shape| shapes.push(shape));
        shapes
    }

    pub fn supporting_block(&self, feet: BoundingBox, position: Vector3<f64>) -> Option<BlockPos> {
        let below = BoundingBox::new(
            feet.min - Vector3::new(0.0, 1.0e-6, 0.0),
            Vector3::new(feet.max.x, feet.min.y, feet.max.z),
        );
        let mut support = None;
        self.visit_shapes(below, |pos, _| {
            let distance = pos.dist_to_center_sqr(position.x, position.y, position.z);
            if support.is_none_or(|(current, current_distance): (BlockPos, f64)| {
                let (a, b) = (pos.0, current.0);
                distance < current_distance
                    || distance == current_distance && (a.y, a.z, a.x) > (b.y, b.z, b.x)
            }) {
                support = Some((pos, distance));
            }
        });
        support.map(|(pos, _)| pos)
    }

    fn visit_shapes(&self, bounds: BoundingBox, mut visit: impl FnMut(BlockPos, BoundingBox)) {
        let min = bounds.min.floor_to_i32();
        let max = bounds.max.floor_to_i32();
        // Fences and walls extend beyond their own block.
        for x in (min.x - 1)..=(max.x + 1) {
            for z in (min.z - 1)..=(max.z + 1) {
                for y in (min.y - 1)..=(max.y + 1) {
                    let pos = BlockPos::new(x, y, z);
                    if let Some(state) = self.block(pos.0) {
                        let origin = Vector3::new(f64::from(x), f64::from(y), f64::from(z));
                        for shape in state.to_state().get_block_collision_shapes_at(&pos) {
                            let shape = shape.shift(origin);
                            if shape.intersects(&bounds) {
                                visit(pos, shape);
                            }
                        }
                    }
                }
            }
        }
    }

    fn set(&mut self, pos: BlockPos, id: i32) -> Result<(), String> {
        if let Some(chunk) = self
            .chunks
            .get_mut(&(pos.0.x.div_euclid(16), pos.0.z.div_euclid(16)))
        {
            let section = chunk.section(pos.0.y);
            if let Some(section) = chunk.sections.get_mut(section) {
                section.set(index(pos.0), state(id)?);
            }
        }
        Ok(())
    }
}

const fn index(pos: Vector3<i32>) -> usize {
    ((pos.y & 15) << 8 | (pos.z & 15) << 4 | (pos.x & 15)) as usize
}

fn read_sections(mut bytes: &[u8]) -> Result<Chunk, String> {
    let mut sections = Vec::new();
    while !bytes.is_empty() {
        if sections.len() == 24 {
            return Err("chunk has more than 24 sections".into());
        }
        bytes
            .read_slice_borrowed(4)
            .map_err(|error| error.to_string())?; // non-air and fluid counts
        sections.push(read_blocks(&mut bytes)?);
        skip_biomes(&mut bytes)?;
    }
    // shortcut: standard dimension bounds only, extend when custom dimensions need physics.
    let min_section = match sections.len() {
        24 => -4,
        16 => 0,
        size => return Err(format!("unsupported chunk height with {size} sections")),
    };
    Ok(Chunk {
        min_section,
        sections,
    })
}

fn read_blocks(bytes: &mut &[u8]) -> Result<Section, String> {
    let bits = usize::from(bytes.get_u8().map_err(|error| error.to_string())?);
    if bits == 0 {
        return Ok(Section::Uniform(state(var(bytes)?)?));
    }
    if !(4..=16).contains(&bits) {
        return Err(format!("invalid block palette width {bits}"));
    }
    let palette = if bits <= 8 {
        (0..count(bytes, 1 << bits)?)
            .map(|_| state(var(bytes)?))
            .collect::<Result<Vec<_>, _>>()?
    } else {
        Vec::new()
    };
    let per_word = 64 / bits;
    let mask = (1u64 << bits) - 1;
    let mut states = Vec::with_capacity(SECTION_VOLUME);
    for _ in 0..SECTION_VOLUME.div_ceil(per_word) {
        let word = bytes.get_u64_be().map_err(|error| error.to_string())?;
        for offset in 0..per_word.min(SECTION_VOLUME - states.len()) {
            let id = ((word >> (offset * bits)) & mask) as usize;
            states.push(if bits <= 8 {
                *palette.get(id).ok_or("invalid block palette index")?
            } else {
                state(id as i32)?
            });
        }
    }
    Ok(Section::Dense(states.into_boxed_slice()))
}

fn skip_biomes(bytes: &mut &[u8]) -> Result<(), String> {
    let bits = usize::from(bytes.get_u8().map_err(|error| error.to_string())?);
    match bits {
        0 => {
            var(bytes)?;
        }
        1..=3 => {
            for _ in 0..count(bytes, 1 << bits)? {
                var(bytes)?;
            }
        }
        4..=16 => {}
        _ => return Err(format!("invalid biome palette width {bits}")),
    }
    if let Some(per_word) = 64usize.checked_div(bits) {
        bytes
            .read_slice_borrowed(64usize.div_ceil(per_word) * 8)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn state(id: i32) -> Result<BlockStateId, String> {
    u16::try_from(id)
        .ok()
        .and_then(BlockStateId::new)
        .ok_or_else(|| format!("invalid block state {id}"))
}

fn count(bytes: &mut &[u8], maximum: usize) -> Result<usize, String> {
    let n = var(bytes)? as usize;
    if n > maximum {
        return Err(format!("count {n} exceeds {maximum}"));
    }
    Ok(n)
}

fn var(bytes: &mut &[u8]) -> Result<i32, String> {
    bytes
        .get_var_int()
        .map(|value| value.0)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    impl World {
        pub fn test_column(blocks: &[(Vector3<i32>, BlockStateId)]) -> Self {
            let mut world = Self::default();
            for &(pos, state) in blocks {
                let chunk = world
                    .chunks
                    .entry((pos.x.div_euclid(16), pos.z.div_euclid(16)))
                    .or_insert_with(|| Chunk {
                        min_section: -4,
                        sections: (0..24)
                            .map(|_| Section::Uniform(BlockStateId::AIR))
                            .collect(),
                    });
                let section = chunk.section(pos.y);
                chunk.sections[section].set(index(pos), state);
            }
            world
        }
    }

    #[test]
    fn decodes_26_3_single_state_sections_and_signed_chunk_coordinates() -> Result<(), String> {
        let mut packet = vec![0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0];
        packet.extend([16 * 8]);
        for _ in 0..16 {
            packet.extend([0, 0, 0, 0, 0, 1, 0, 0]);
        }
        let mut world = World::default();
        // A 128-byte VarInt needs a continuation byte and a second byte.
        packet[9] |= 0x80;
        packet.insert(10, 1);
        world.observe(p::LEVEL_CHUNK_WITH_LIGHT.0, &packet)?;
        assert_eq!(world.block(Vector3::new(-1, 14, 0)), BlockStateId::new(1));
        Ok(())
    }
}
