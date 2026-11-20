//! Create 6.0.10's Contraption.searchMovedStructure and StructureTransform.

use std::collections::{HashMap, HashSet, VecDeque};

use fastnbt::IntArray;
use moddedmc_api::{
    Pos,
    content::{Content, UNLOADED_STATE, World},
};
use serde::{Deserialize, Serialize};

use crate::kinetics::{FACES, face_axis, offset, packed};

/// CKinetics.maxBlocksMoved default.
const MAX_BLOCKS_MOVED: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockMovement {
    Air,
    Immovable,
    Simple(&'static str),
}

#[derive(Default)]
pub struct BlockMovementChecks {
    states: HashMap<u32, BlockMovement>,
    names: HashMap<&'static str, u32>,
}

impl BlockMovementChecks {
    pub fn register_states(&mut self, content: Content) {
        // shortcut: only isotropic blocks proven free of block entities, until state transforms
        // and block entity capture are available.
        let simple = [
            "minecraft:stone",
            "minecraft:cobblestone",
            "minecraft:smooth_stone",
            "minecraft:andesite",
            "minecraft:granite",
            "minecraft:diorite",
            "minecraft:oak_planks",
            "minecraft:spruce_planks",
            "minecraft:glass",
            "minecraft:slime_block",
            "minecraft:honey_block",
            "minecraft:iron_block",
        ]
        .into_iter()
        .map(|name| (name, BlockMovement::Simple(name)));
        let air = ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"]
            .into_iter()
            .map(|name| (name, BlockMovement::Air));
        let immovable = [
            "minecraft:bedrock",
            "minecraft:barrier",
            "minecraft:reinforced_deepslate",
        ]
        .into_iter()
        .map(|name| (name, BlockMovement::Immovable));
        for (name, kind) in simple.chain(air).chain(immovable) {
            if let Some(state) = content.state(name) {
                self.states.insert(state, kind);
                self.names.insert(name, state);
            }
        }
    }

    pub fn scan(&self, world: World<'_>, bearing: Pos, facing: u8) -> Option<Contraption> {
        Contraption::search_moved_structure(bearing, facing, |pos| {
            let mut state = [UNLOADED_STATE];
            world.read(&[pos], &mut state);
            self.states.get(&state[0]).copied()
        })
    }

    pub fn state(&self, name: &str) -> Option<u32> {
        self.names.get(name).copied()
    }

    pub fn is_air(&self, state: u32) -> bool {
        self.states.get(&state) == Some(&BlockMovement::Air)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Contraption {
    anchor: IntArray,
    pub blocks: Blocks,
    bounds_front: Vec<f64>,
    facing: u8,
    sails: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Blocks {
    pub palette: Vec<PaletteEntry>,
    pub block_list: Vec<StructureBlockInfo>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct PaletteEntry {
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct StructureBlockInfo {
    pos: i64,
    state: i32,
}

impl StructureBlockInfo {
    pub const fn palette_index(&self) -> usize {
        self.state as usize
    }

    pub const fn local_pos(&self) -> [i32; 3] {
        [
            (self.pos >> 38) as i32,
            (self.pos << 52 >> 52) as i32,
            (self.pos << 26 >> 38) as i32,
        ]
    }
}

impl Contraption {
    fn search_moved_structure(
        bearing: Pos,
        facing: u8,
        mut get: impl FnMut(Pos) -> Option<BlockMovement>,
    ) -> Option<Self> {
        let anchor = offset(bearing, FACES[usize::from(facing)]);
        let BlockMovement::Simple(name) = get(anchor)? else {
            return None;
        };
        let mut frontier = VecDeque::from([(anchor, name)]);
        let mut visited = HashSet::from([anchor, bearing]);
        let mut blocks = Blocks::default();
        let mut palette = HashMap::new();
        while let Some((pos, name)) = frontier.pop_front() {
            for step in FACES {
                let adjacent = offset(pos, step);
                if visited.contains(&adjacent) {
                    continue;
                }
                // An unknown neighbour may be attached to this face. Leave the world intact.
                if let BlockMovement::Simple(next) = get(adjacent)?
                    && can_stick(name, next)
                {
                    visited.insert(adjacent);
                    frontier.push_back((adjacent, next));
                }
            }
            let state = *palette.entry(name).or_insert_with(|| {
                let state = blocks.palette.len() as i32;
                blocks.palette.push(PaletteEntry {
                    name: name.to_owned(),
                });
                state
            });
            let relative = Pos {
                x: pos.x - anchor.x,
                y: pos.y - anchor.y,
                z: pos.z - anchor.z,
            };
            blocks.block_list.push(StructureBlockInfo {
                pos: packed(relative),
                state,
            });
            if blocks.block_list.len() > MAX_BLOCKS_MOVED {
                return None;
            }
        }
        let axis = usize::from(face_axis(facing));
        let mut along = [0, 0];
        let mut radius_squared = 0;
        for block in &blocks.block_list {
            let local = block.local_pos();
            along[0] = along[0].min(local[axis]);
            along[1] = along[1].max(local[axis] + 1);
            let u = local[(axis + 1) % 3];
            let v = local[(axis + 2) % 3];
            radius_squared = radius_squared.max(u * u + v * v);
        }
        let radius = f64::from(radius_squared).sqrt().ceil();
        let mut bounds = vec![-radius - 1.0; 3];
        bounds.extend([radius + 2.0; 3]);
        bounds[axis] = f64::from(along[0]);
        bounds[axis + 3] = f64::from(along[1]);
        Some(Self {
            anchor: IntArray::new(vec![anchor.x, anchor.y, anchor.z]),
            blocks,
            bounds_front: bounds,
            facing,
            sails: 0,
        })
    }

    pub fn anchor(&self) -> Option<Pos> {
        let [x, y, z] = *self.anchor else {
            return None;
        };
        Some(Pos { x, y, z })
    }
}

fn can_stick(first: &str, second: &str) -> bool {
    const SLIME: &str = "minecraft:slime_block";
    const HONEY: &str = "minecraft:honey_block";
    match (first, second) {
        (SLIME, HONEY) | (HONEY, SLIME) => false,
        (SLIME | HONEY, _) | (_, SLIME | HONEY) => true,
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub struct StructureTransform {
    pub anchor: Pos,
    pub axis: u8,
    pub angle: f32,
}

impl StructureTransform {
    /// Create snaps the current angle to the nearest quarter-turn on disassembly.
    pub fn apply(self, mut local: [i32; 3]) -> Pos {
        let u = usize::from((self.axis + 1) % 3);
        let v = usize::from((self.axis + 2) % 3);
        for _ in 0..((self.angle / 90.0 + 0.5).floor() as i32).rem_euclid(4) {
            (local[u], local[v]) = (-local[v], local[u]);
        }
        offset(self.anchor, local)
    }

    pub fn block_display(self, local: [i32; 3]) -> moddedmc_api::block_displays::BlockDisplay {
        let axis = usize::from(self.axis);
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let radians = self.angle.to_radians();
        let (sin, cos) = radians.sin_cos();
        let (half_sin, half_cos) = (radians * 0.5).sin_cos();
        let mut rotation = [0.0, 0.0, 0.0, half_cos];
        rotation[axis] = half_sin;
        let mut translation = local.map(|c| c as f32 - 0.5);
        let [a, b] = [translation[u], translation[v]];
        translation[u] = a * cos - b * sin;
        translation[v] = a * sin + b * cos;
        moddedmc_api::block_displays::BlockDisplay {
            position: [self.anchor.x, self.anchor.y, self.anchor.z].map(|n| f64::from(n) + 0.5),
            translation,
            rotation,
            scale: [1.0; 3],
            transform_interpolation: 1,
            position_interpolation: 0,
            brightness: -1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }

    #[test]
    fn scan_traverses_slime_and_skips_incompatible_honey_and_the_anchor() {
        use BlockMovement::{Air, Simple};
        let tiles = HashMap::from([
            (p(0, 1, 0), Simple("minecraft:slime_block")),
            (p(-1, 1, 0), Simple("minecraft:stone")),
            (p(-2, 1, 0), Simple("minecraft:stone")),
            (p(1, 1, 0), Simple("minecraft:honey_block")),
        ]);
        let contraption = Contraption::search_moved_structure(p(0, 0, 0), 1, |pos| {
            assert_ne!(pos, p(0, 0, 0));
            Some(tiles.get(&pos).copied().unwrap_or(Air))
        })
        .expect("assembled");
        let bytes = fastnbt::to_bytes(&contraption).expect("saved");
        let loaded: Contraption = fastnbt::from_bytes(&bytes).expect("loaded");
        assert_eq!(loaded.anchor(), Some(p(0, 1, 0)));
        assert_eq!(
            loaded
                .blocks
                .block_list
                .iter()
                .map(StructureBlockInfo::local_pos)
                .collect::<Vec<_>>(),
            vec![[0, 0, 0], [-1, 0, 0]]
        );
        assert_eq!(loaded.blocks.palette[1].name, "minecraft:stone");
    }

    #[test]
    fn transform_uses_create_quarter_turns_for_all_axes() {
        let mut transform = StructureTransform {
            anchor: p(10, 64, 10),
            axis: 0,
            angle: 93.0,
        };
        assert_eq!(transform.apply([0, 2, 0]), p(10, 64, 12));
        transform.axis = 1;
        assert_eq!(transform.apply([2, 0, 0]), p(10, 64, 8));
        transform.axis = 2;
        assert_eq!(transform.apply([2, 0, 0]), p(10, 66, 10));
        transform.angle = -45.0;
        assert_eq!(transform.apply([2, 0, 0]), p(12, 64, 10));
    }
}
