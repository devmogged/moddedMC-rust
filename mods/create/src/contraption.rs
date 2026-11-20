//! Create 6.0.10's Contraption.searchMovedStructure and StructureTransform.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use fastnbt::{IntArray, Value};
use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, Content, UNLOADED_STATE, World},
};
use serde::{Deserialize, Serialize};

use crate::{
    AXES, BLOCK_DISPLAYS, CONTENT, DIRECTIONS, STACKS, TAGS,
    kinetics::{FACES, face_axis, offset, packed, positive},
};

/// CKinetics.maxBlocksMoved default.
const MAX_BLOCKS_MOVED: usize = 2048;
// The host caps the whole kinetic save at 64 KiB, including the controller fields.
pub const MAX_ASSEMBLY_DATA_BYTES: usize = (64 << 10) - 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BlockMovement {
    Air,
    Immovable,
    Movable(PaletteEntry),
}

#[derive(Default)]
pub struct BlockMovementChecks {
    states: HashMap<u32, BlockMovement>,
    names: HashMap<&'static str, u32>,
}

impl BlockMovementChecks {
    pub fn register_states(&mut self, content: Content) {
        // shortcut: states with unported movement rules must not be silently skipped.
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
        .map(|name| (name, BlockMovement::Movable(PaletteEntry::parse(name))));
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
        let mut contraption =
            Contraption::search_moved_structure(bearing, facing, |pos| self.classify(world, pos))?;
        let anchor = contraption.anchor()?;
        contraption.blocks.capture_entities(world, anchor)?;
        if let Some(sails) = TAGS.get()?.values("block", "create:windmill_sails") {
            contraption.sails = contraption
                .blocks
                .block_list
                .iter()
                .filter(|block| {
                    sails.contains(&contraption.blocks.palette[block.palette_index()].name)
                })
                .count() as i32;
        }
        Some(contraption)
    }

    pub fn classify(&self, world: World<'_>, pos: Pos) -> Option<BlockMovement> {
        let mut state = [UNLOADED_STATE];
        world.read(&[pos], &mut state);
        if let Some(known) = self.states.get(&state[0]) {
            return Some(known.clone());
        }
        let entry = PaletteEntry::parse(&CONTENT.get()?.describe_state(state[0])?);
        let id = entry.name.strip_prefix("minecraft:")?;
        ((id.ends_with("_log")
            || id.ends_with("_stairs")
            || id.ends_with("_wool")
            || entry.has_storage())
            && entry
                .properties
                .get("waterlogged")
                .is_none_or(|v| v == "false"))
        .then_some(BlockMovement::Movable(entry))
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
    pub sails: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Blocks {
    pub palette: Vec<PaletteEntry>,
    pub block_list: Vec<StructureBlockInfo>,
}

impl Blocks {
    pub fn positions(&self, anchor: Pos) -> impl Iterator<Item = Pos> {
        self.block_list
            .iter()
            .map(move |block| offset(anchor, block.local_pos()))
    }

    pub fn is_colliding_with_world(
        &self,
        world: World<'_>,
        anchor: Pos,
        direction: u8,
        sources: &HashSet<Pos>,
        controller: Option<Pos>,
    ) -> bool {
        let destinations: HashSet<_> = self.positions(anchor).collect();
        destinations.iter().any(|&pos| {
            Some(pos) != controller
                && !sources.contains(&pos)
                && !destinations.contains(&offset(pos, FACES[usize::from(direction)]))
                && world
                    .collision_shape(pos)
                    .is_none_or(|shape| !shape.is_empty())
        })
    }

    pub fn capture_entities(&mut self, world: World<'_>, anchor: Pos) -> Option<()> {
        for block in &mut self.block_list {
            if self.palette[block.palette_index()].has_storage() {
                let position = offset(anchor, block.local_pos());
                let bytes = world.block_entity(position)?;
                let mut data: HashMap<String, Value> = fastnbt::from_bytes(&bytes).ok()?;
                for coordinate in ["x", "y", "z"] {
                    data.remove(coordinate);
                }
                block.nbt = Some(data);
            }
        }
        Some(())
    }

    pub fn push(
        &mut self,
        pos: Pos,
        entry: PaletteEntry,
        palette: &mut HashMap<PaletteEntry, i32>,
    ) {
        let state = *palette.entry(entry.clone()).or_insert_with(|| {
            let state = self.palette.len() as i32;
            self.palette.push(entry);
            state
        });
        self.block_list.push(StructureBlockInfo {
            pos: packed(pos),
            state,
            nbt: None,
        });
    }

    pub fn collect_attached(
        anchor: Pos,
        mut frontier: VecDeque<(Pos, PaletteEntry)>,
        mut visited: HashSet<Pos>,
        mut get: impl FnMut(Pos) -> Option<BlockMovement>,
    ) -> Option<(Self, HashMap<PaletteEntry, i32>)> {
        visited.extend(frontier.iter().map(|(pos, _)| *pos));
        let mut blocks = Self::default();
        let mut palette = HashMap::new();
        while let Some((pos, entry)) = frontier.pop_front() {
            for step in FACES {
                let adjacent = offset(pos, step);
                if visited.contains(&adjacent) {
                    continue;
                }
                // Leave the world intact if an unknown neighbour might attach to this face.
                if let BlockMovement::Movable(next) = get(adjacent)?
                    && can_stick(&entry.name, &next.name)
                {
                    visited.insert(adjacent);
                    frontier.push_back((adjacent, next));
                }
            }
            blocks.push(
                Pos {
                    x: pos.x - anchor.x,
                    y: pos.y - anchor.y,
                    z: pos.z - anchor.z,
                },
                entry,
                &mut palette,
            );
            if blocks.block_list.len() > MAX_BLOCKS_MOVED {
                return None;
            }
        }
        Some((blocks, palette))
    }

    pub fn place(
        &self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        transform: StructureTransform,
        controller: Option<Pos>,
    ) -> bool {
        let Some((place, saved_entities)) = self
            .block_list
            .iter()
            .filter(|block| Some(transform.apply(block.local_pos())) != controller)
            .map(|block| {
                let entry = self.palette.get(block.palette_index())?;
                Some((
                    BlockUpdate {
                        position: transform.apply(block.local_pos()),
                        state: transform.block_state(entry).state()?,
                        flags: content::UPDATE_ALL,
                    },
                    block.nbt.as_ref().map(fastnbt::to_bytes).transpose().ok()?,
                ))
            })
            .collect::<Option<(Vec<_>, Vec<_>)>>()
        else {
            return false;
        };
        if place.is_empty() {
            return true;
        }
        let positions: Vec<_> = place.iter().map(|update| update.position).collect();
        let mut destination = vec![UNLOADED_STATE; place.len()];
        world.read(&positions, &mut destination);
        // shortcut: retain the assembly if any destination needs unported replacement rules.
        if !destination.iter().all(|&state| checks.is_air(state))
            || world.write(&place) != place.len()
        {
            return false;
        }
        for (update, saved) in place.iter().zip(saved_entities) {
            if let Some(saved) = saved
                && !world.restore_block_entity(update.position, &saved)
            {
                let rollback: Vec<_> = positions
                    .iter()
                    .zip(destination)
                    .map(|(&position, state)| BlockUpdate {
                        position,
                        state,
                        flags: content::UPDATE_MOVED | content::UPDATE_CLIENTS,
                    })
                    .collect();
                world.write(&rollback);
                return false;
            }
        }
        true
    }

    pub fn drop_contents(self, world: World<'_>, pos: Pos, item_for: impl Fn(&str) -> &str) {
        let Some(stacks) = STACKS.get() else { return };
        let position = [pos.x, pos.y, pos.z].map(|n| f64::from(n) + 0.5);
        for block in self.block_list {
            if let Some(entry) = self.palette.get(block.palette_index()) {
                let item = Value::Compound(HashMap::from([
                    ("id".into(), Value::String(item_for(&entry.name).into())),
                    ("count".into(), Value::Int(1)),
                ]));
                if let Ok(bytes) = fastnbt::to_bytes(&item) {
                    stacks.spawn(world, position, [0.0, 0.1, 0.0], &bytes);
                }
            }
            if let Some(Value::List(items)) = block.nbt.and_then(|mut nbt| nbt.remove("Items")) {
                for mut item in items {
                    if let Value::Compound(fields) = &mut item {
                        fields.remove("Slot");
                        if let Ok(bytes) = fastnbt::to_bytes(&item) {
                            stacks.spawn(world, position, [0.0, 0.1, 0.0], &bytes);
                        }
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ContraptionDisplays(Vec<i32>);

impl ContraptionDisplays {
    pub fn clear(&mut self, world: World<'_>) {
        if let Some(displays) = BLOCK_DISPLAYS.get() {
            for id in self.0.drain(..).filter(|&id| id != 0) {
                displays.remove(world, id);
            }
        }
    }

    pub fn show(
        &mut self,
        world: World<'_>,
        blocks: &Blocks,
        transform: StructureTransform,
        translation: [f32; 3],
    ) {
        let Some(displays) = BLOCK_DISPLAYS.get() else {
            return;
        };
        self.0.resize(blocks.block_list.len(), 0);
        for (block, id) in blocks.block_list.iter().zip(&mut self.0) {
            let mut shown = transform.block_display(block.local_pos());
            for (shown, shift) in shown.translation.iter_mut().zip(translation) {
                *shown += shift;
            }
            if (*id == 0 || !displays.update(world, *id, &shown))
                && let Some(state) = blocks
                    .palette
                    .get(block.palette_index())
                    .and_then(PaletteEntry::state)
            {
                *id = displays.spawn(world, state, &shown).unwrap_or(0);
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "PascalCase")]
pub struct PaletteEntry {
    pub name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) properties: BTreeMap<String, String>,
}

impl PaletteEntry {
    pub(crate) fn parse(selector: &str) -> Self {
        let (name, properties) = selector
            .strip_suffix(']')
            .and_then(|selector| selector.split_once('['))
            .unwrap_or((selector, ""));
        Self {
            name: name.to_owned(),
            properties: properties
                .split(',')
                .filter_map(|pair| pair.split_once('='))
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .collect(),
        }
    }

    pub fn state(&self) -> Option<u32> {
        let content = CONTENT.get()?;
        if self.properties.is_empty() {
            return content.state(&self.name);
        }
        let mut selector = self.name.clone();
        selector.push('[');
        for (index, (key, value)) in self.properties.iter().enumerate() {
            if index > 0 {
                selector.push(',');
            }
            selector.push_str(key);
            selector.push('=');
            selector.push_str(value);
        }
        selector.push(']');
        content.state(&selector)
    }
    fn has_storage(&self) -> bool {
        self.name == "minecraft:furnace"
            || self.name == "minecraft:chest"
                && self
                    .properties
                    .get("type")
                    .is_some_and(|kind| kind == "single")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct StructureBlockInfo {
    pos: i64,
    state: i32,
    #[serde(rename = "Data", default, skip_serializing_if = "Option::is_none")]
    pub nbt: Option<HashMap<String, Value>>,
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
        let BlockMovement::Movable(entry) = get(anchor)? else {
            return None;
        };
        let (blocks, _) = Blocks::collect_attached(
            anchor,
            VecDeque::from([(anchor, entry)]),
            HashSet::from([bearing]),
            get,
        )?;
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
    fn quarter_turns(self) -> i32 {
        ((self.angle / 90.0 + 0.5).floor() as i32).rem_euclid(4)
    }

    /// Create snaps the current angle to the nearest quarter-turn on disassembly.
    pub fn apply(self, local: [i32; 3]) -> Pos {
        offset(self.anchor, self.rotate(local))
    }

    fn rotate(self, mut local: [i32; 3]) -> [i32; 3] {
        let u = usize::from((self.axis + 1) % 3);
        let v = usize::from((self.axis + 2) % 3);
        for _ in 0..self.quarter_turns() {
            (local[u], local[v]) = (-local[v], local[u]);
        }
        local
    }

    pub fn block_state(self, entry: &PaletteEntry) -> PaletteEntry {
        let mut rotated = entry.clone();
        let turns = self.quarter_turns();
        if turns == 0 {
            return rotated;
        }
        if let Some(axis) = rotated.properties.get_mut("axis")
            && let Some(index) = AXES.iter().position(|name| *name == axis)
        {
            let mut step = [0; 3];
            step[index] = 1;
            if let Some(index) = self.rotate(step).iter().position(|&step| step != 0) {
                *axis = AXES[index].to_owned();
            }
        }
        let Some(mut face) = rotated
            .properties
            .get("facing")
            .and_then(|value| DIRECTIONS.iter().position(|name| *name == value))
            .map(|face| face as u8)
        else {
            return rotated;
        };
        let stair = entry.name.ends_with("_stairs");
        if self.axis == 1 || (turns == 2 && !stair) {
            if self.axis == 1 || face_axis(face) != self.axis {
                let rotated_face =
                    StructureTransform { axis: 1, ..self }.rotate(FACES[usize::from(face)]);
                if let Some(index) = FACES.iter().position(|&step| step == rotated_face) {
                    face = index as u8;
                }
            }
        } else if stair && let Some(half) = rotated.properties.get_mut("half") {
            let mut bottom = half == "bottom";
            if face_axis(face) != self.axis {
                // StructureTransform.transformStairs iterates Rotation.ordinal() clockwise steps.
                for _ in 0..(4 - turns) {
                    if positive(face) ^ bottom ^ (face_axis(face) == 2) {
                        bottom = !bottom;
                    } else {
                        face ^= 1;
                    }
                }
            } else if turns == 2 {
                bottom = !bottom;
            }
            *half = if bottom { "bottom" } else { "top" }.to_owned();
        }
        rotated
            .properties
            .insert("facing".into(), DIRECTIONS[usize::from(face)].into());
        rotated
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
        use BlockMovement::{Air, Movable};
        let simple = |name| Movable(PaletteEntry::parse(name));
        let tiles = HashMap::from([
            (p(0, 1, 0), simple("minecraft:slime_block")),
            (p(-1, 1, 0), simple("minecraft:stone")),
            (p(-2, 1, 0), simple("minecraft:stone")),
            (p(1, 1, 0), simple("minecraft:honey_block")),
        ]);
        let contraption = Contraption::search_moved_structure(p(0, 0, 0), 1, |pos| {
            assert_ne!(pos, p(0, 0, 0));
            Some(tiles.get(&pos).cloned().unwrap_or(Air))
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

    #[test]
    fn state_rotation_preserves_properties_and_handles_horizontal_stairs() {
        let mut transform = StructureTransform {
            anchor: p(0, 0, 0),
            axis: 1,
            angle: 90.0,
        };
        let log = PaletteEntry::parse("minecraft:oak_log[axis=x]");
        assert_eq!(
            transform.block_state(&log),
            PaletteEntry::parse("minecraft:oak_log[axis=z]")
        );
        let stairs = PaletteEntry::parse(
            "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]",
        );
        assert_eq!(
            transform.block_state(&stairs),
            PaletteEntry::parse(
                "minecraft:oak_stairs[facing=west,half=bottom,shape=straight,waterlogged=false]"
            )
        );
        transform.axis = 0;
        assert_eq!(
            transform.block_state(&stairs),
            PaletteEntry::parse(
                "minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=false]"
            )
        );
    }
}
