//! Create 6.0.10's Contraption.searchMovedStructure and StructureTransform.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use fastnbt::{IntArray, Value};
use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, Content, UNLOADED_STATE, World},
    stacks::ItemDisplay,
};
use serde::{Deserialize, Serialize};

use crate::{
    AXES, BLOCK_DISPLAYS, BRASS_ENCASED_SHAFT, CONTENT, DIRECTIONS, DISPLAYS, ENCASED_SHAFT, SHAFT,
    STACKS, TAGS, WorldLevel,
    belt::BELT,
    belt_inventory::{self, BeltInventory, Chain},
    belt_movement::can_transport_objects,
    chassis,
    collider::{Collider, Frame},
    drill::{self, DrillMovementBehaviour},
    glue::{self, GlueBox, GlueScan},
    harvester,
    kinetics::{FACES, Kind, Kinetics, Part, face_axis, offset, packed, positive},
    seat, states, sticker,
    trains::{bogey, controls},
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
            "minecraft:obsidian",
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
        if let Some(water) = content.state("minecraft:water") {
            self.names.insert("minecraft:water", water);
        }
    }

    pub fn scan(
        &self,
        world: World<'_>,
        bearing: Pos,
        facing: u8,
        kinetics: &Kinetics,
    ) -> Option<Contraption> {
        self.scan_ignoring(world, bearing, facing, kinetics, &HashSet::new())
    }

    pub fn scan_ignoring(
        &self,
        world: World<'_>,
        bearing: Pos,
        facing: u8,
        kinetics: &Kinetics,
        ignored: &HashSet<Pos>,
    ) -> Option<Contraption> {
        let mut contraption = Contraption::search_moved_structure(
            bearing,
            facing,
            Some(world),
            |pos| {
                if ignored.contains(&pos) {
                    Some(BlockMovement::Air)
                } else {
                    self.classify(world, pos)
                }
            },
            |pos, entry| self.attachments(world, pos, entry, None),
        )?;
        let anchor = contraption.anchor()?;
        // shortcut: rotating a diagonal around its run axis can leave disconnected parts.
        // Keep them intact until disassembly can reproduce Create's invalid-chain drops.
        if contraption.blocks.palette.iter().any(|entry| {
            entry.name == BELT
                && entry.state().and_then(|state| states()?.kind(state)).is_some_and(|kind| {
                    matches!(kind, Kind::Belt(belt) if belt.slope.diagonal() && face_axis(belt.facing) == face_axis(facing))
                })
        }) {
            return None;
        }
        contraption
            .blocks
            .capture_entities(world, anchor, kinetics)?;
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

    pub(crate) fn classify(&self, world: World<'_>, pos: Pos) -> Option<BlockMovement> {
        let mut state = [UNLOADED_STATE];
        world.read(&[pos], &mut state);
        if let Some(known) = self.states.get(&state[0]) {
            return Some(known.clone());
        }
        let entry = PaletteEntry::parse(&CONTENT.get()?.describe_state(state[0])?);
        if entry.name == "minecraft:water" {
            return Some(BlockMovement::Air);
        }
        if matches!(
            entry.name.as_str(),
            "create:track"
                | "create:track_station"
                | "create:track_signal"
                | crate::large_water_wheel::ID
                | crate::large_water_wheel::STRUCTURE
        ) {
            return Some(BlockMovement::Immovable);
        }
        if chassis::is_chassis(&entry.name)
            || seat::is_seat(&entry.name)
            || bogey::is_bogey(&entry.name)
            || crate::sail::is_sail(&entry.name)
            || matches!(
                entry.name.as_str(),
                BELT | sticker::ID
                    | crate::elevator::CONTACT
                    | drill::ID
                    | harvester::ID
                    | controls::ID
            )
        {
            return Some(BlockMovement::Movable(entry));
        }
        let movable = matches!(
            entry.name.as_str(),
            SHAFT | ENCASED_SHAFT | BRASS_ENCASED_SHAFT
        ) || entry.name.strip_prefix("minecraft:").is_some_and(|id| {
            id.ends_with("_log")
                || id.ends_with("_stairs")
                || id.ends_with("_wool")
                || entry.has_storage()
        });
        (movable
            && entry
                .properties
                .get("waterlogged")
                .is_none_or(|value| value == "false"))
        .then_some(BlockMovement::Movable(entry))
    }

    pub fn state(&self, name: &str) -> Option<u32> {
        self.names.get(name).copied()
    }

    pub fn is_air(&self, state: u32) -> bool {
        self.states.get(&state) == Some(&BlockMovement::Air)
    }

    pub fn collect_attached(
        &self,
        world: World<'_>,
        anchor: Pos,
        frontier: VecDeque<(Pos, PaletteEntry)>,
        visited: HashSet<Pos>,
        forced: Option<u8>,
    ) -> Option<(Blocks, HashMap<PaletteEntry, i32>)> {
        Blocks::collect_attached(
            anchor,
            Some(world),
            frontier,
            visited,
            |pos| self.classify(world, pos),
            |pos, entry| self.attachments(world, pos, entry, forced),
        )
    }

    fn attachments(
        &self,
        world: World<'_>,
        pos: Pos,
        entry: &PaletteEntry,
        forced: Option<u8>,
    ) -> Option<Vec<Pos>> {
        let mut attached = chassis::attachments(world, self, pos, entry, forced)?;
        attached.extend(sticker::attachment(entry, pos));
        if let Some(axis) = entry.bogey_axis() {
            attached.extend(
                FACES
                    .into_iter()
                    .enumerate()
                    .filter(|(face, _)| face_axis(*face as u8) == axis)
                    .map(|(_, step)| offset(pos, step)),
            );
        }
        if let Some(face) = harvester::facing(entry) {
            attached.push(offset(pos, FACES[usize::from(face ^ 1)]));
        }
        if entry.name == BELT {
            // `Contraption.moveBelt` follows both ends even when only one pulley is attached.
            let Some(Kind::Belt(belt)) = states()?.kind(entry.state()?) else {
                return None;
            };
            for forward in [true, false] {
                let Some(next) = belt.next_segment(pos, forward) else {
                    continue;
                };
                let BlockMovement::Movable(next_entry) = self.classify(world, next)? else {
                    return None;
                };
                let Some(Kind::Belt(neighbor)) = states()?.kind(next_entry.state()?) else {
                    return None;
                };
                if neighbor.facing != belt.facing
                    || neighbor.next_segment(next, !forward) != Some(pos)
                {
                    return None;
                }
                attached.push(next);
            }
        }
        Some(attached)
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
    #[serde(default, rename = "Superglue", skip_serializing_if = "Vec::is_empty")]
    superglue: Vec<GlueBox>,
    #[serde(skip)]
    glue_to_remove: Vec<i32>,
    #[serde(default, rename = "Actors", skip_serializing_if = "Vec::is_empty")]
    drills: Vec<DrillMovementBehaviour>,
}

impl Blocks {
    pub fn harvest(&mut self, world: World<'_>, before: Frame, after: Frame) {
        let mut harvests = Vec::new();
        for block in &self.block_list {
            let entry = &self.palette[block.palette_index()];
            if let Some(face) = harvester::facing(entry)
                && let Some(pos) = harvester::newly_visited(block.local_pos(), face, before, after)
                && let Some(drops) = harvester::visit_new_position(world, pos)
            {
                harvests.push((pos, drops));
            }
        }
        for (pos, drops) in harvests {
            self.collect_or_drop_items(world, pos, drops);
        }
    }

    pub fn tick_drills(&mut self, world: World<'_>, before: Frame, after: Frame) -> bool {
        let mut drills = std::mem::take(&mut self.drills);
        let mut stalled = false;
        for actor in &mut drills {
            match actor.tick(world, before, after) {
                drill::Tick::Stalled => stalled = true,
                drill::Tick::Broke(pos, drops) => self.collect_or_drop_items(world, pos, drops),
                drill::Tick::Travel => {}
            }
        }
        self.drills = drills;
        stalled
    }

    fn collect_or_drop_items(&mut self, world: World<'_>, position: Pos, drops: Vec<Vec<u8>>) {
        let Some(stacks) = STACKS.get() else { return };
        let at = [position.x, position.y, position.z].map(|n| f64::from(n) + 0.5);
        for drop in drops {
            let Some(info) = stacks.info(&drop) else {
                continue;
            };
            let Ok(item) = fastnbt::from_bytes::<Value>(&drop) else {
                continue;
            };
            let remaining = self.insert_into_chests(&item, info.max_count as i32);
            if remaining == info.count as i32 {
                stacks.spawn(world, at, [0.0; 3], &drop);
            } else if remaining > 0
                && let Ok(rest) = fastnbt::to_bytes(&belt_inventory::with_count(&item, remaining))
            {
                stacks.spawn(world, at, [0.0; 3], &rest);
            }
        }
    }

    fn insert_into_chests(&mut self, item: &Value, max_count: i32) -> i32 {
        const CHEST_SIZE: u8 = 27;
        let mut count = belt_inventory::count(item);
        let Value::Compound(offered) = item else {
            return count;
        };
        for merge in [true, false] {
            for block in &mut self.block_list {
                if self.palette[block.palette_index()].name != "minecraft:chest" {
                    continue;
                }
                let Some(nbt) = &mut block.nbt else { continue };
                let Value::List(items) = nbt
                    .entry("Items".into())
                    .or_insert_with(|| Value::List(Vec::new()))
                else {
                    continue;
                };
                if merge {
                    for existing in &mut *items {
                        let stackable = matches!(existing, Value::Compound(nbt) if
                            nbt.get("id") == offered.get("id") && nbt.get("components") == offered.get("components"));
                        if !stackable {
                            continue;
                        }
                        let existing_count = belt_inventory::count(existing);
                        let inserted = count.min((max_count - existing_count).max(0));
                        if let Value::Compound(stack) = existing
                            && inserted > 0
                        {
                            stack.insert("count".into(), Value::Int(existing_count + inserted));
                        }
                        count -= inserted;
                        if count == 0 {
                            return 0;
                        }
                    }
                } else {
                    let occupied: HashSet<_> = items
                        .iter()
                        .filter_map(|stored| {
                            let Value::Compound(stored) = stored else {
                                return None;
                            };
                            match stored.get("Slot")? {
                                Value::Byte(slot) => u8::try_from(*slot).ok(),
                                Value::Int(slot) => u8::try_from(*slot).ok(),
                                _ => None,
                            }
                        })
                        .collect();
                    for slot in (0..CHEST_SIZE).filter(|slot| !occupied.contains(slot)) {
                        let mut stack = offered.clone();
                        let inserted = count.min(max_count);
                        stack.insert("Slot".into(), Value::Byte(slot as i8));
                        stack.insert("count".into(), Value::Int(inserted));
                        items.push(Value::Compound(stack));
                        count -= inserted;
                        if count == 0 {
                            return 0;
                        }
                    }
                }
            }
        }
        count
    }

    pub fn seats(&self) -> impl Iterator<Item = Pos> + '_ {
        self.block_list.iter().filter_map(|block| {
            let [x, y, z] = block.local_pos();
            seat::is_seat(&self.palette.get(block.palette_index())?.name).then_some(Pos { x, y, z })
        })
    }

    pub fn append_at(&mut self, anchor: Pos, other_anchor: Pos, other: &Self) {
        let origin = Pos {
            x: anchor.x - other_anchor.x,
            y: anchor.y - other_anchor.y,
            z: anchor.z - other_anchor.z,
        };
        self.superglue
            .extend(other.superglue.iter().map(|glue| glue.local(origin)));
        let mut palette = self
            .palette
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, entry)| (entry, index as i32))
            .collect();
        for block in &other.block_list {
            let pos = offset(other_anchor, block.local_pos());
            let local = Pos {
                x: pos.x - anchor.x,
                y: pos.y - anchor.y,
                z: pos.z - anchor.z,
            };
            let entry = other.palette[block.palette_index()].clone();
            let inserted = self.push(local, entry, &mut palette);
            inserted.nbt.clone_from(&block.nbt);
            inserted.shapes.clone_from(&block.shapes);
        }
    }

    pub fn colliders(&self) -> impl Iterator<Item = Collider> + '_ {
        self.block_list.iter().flat_map(|block| {
            let local = block.local_pos().map(f64::from);
            block
                .shapes
                .as_deref()
                .unwrap_or(&[Collider::BLOCK])
                .iter()
                .map(move |shape| shape.translate(local))
        })
    }

    pub fn positions(&self, anchor: Pos) -> impl Iterator<Item = Pos> + Clone {
        self.block_list
            .iter()
            .map(move |block| offset(anchor, block.local_pos()))
    }

    pub fn removals(
        &self,
        anchor: Pos,
        checks: &BlockMovementChecks,
    ) -> Option<impl Iterator<Item = BlockUpdate>> {
        let air = checks.state("minecraft:air")?;
        let water = checks.state("minecraft:water")?;
        Some(self.block_list.iter().map(move |block| {
            let entry = &self.palette[block.palette_index()];
            let wet = entry.can_waterlog_on_placement()
                && entry
                    .properties
                    .get("waterlogged")
                    .is_some_and(|wet| wet == "true");
            BlockUpdate {
                position: offset(anchor, block.local_pos()),
                state: if wet { water } else { air },
                flags: content::UPDATE_MOVED | content::UPDATE_CLIENTS,
            }
        }))
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
        self.block_list
            .iter()
            .zip(self.positions(anchor))
            .any(|(block, pos)| {
                Some(pos) != controller
                    && !sources.contains(&pos)
                    && !destinations.contains(&offset(pos, FACES[usize::from(direction)]))
                    && world
                        .collision_shape(pos)
                        .is_none_or(|shape| !shape.is_empty())
                    && match self.palette[block.palette_index()].name.as_str() {
                        drill::ID => drill::breakable(world, pos).is_none(),
                        harvester::ID => !harvester::can_harvest(world, pos),
                        _ => true,
                    }
            })
    }

    pub fn capture_entities(
        &mut self,
        world: World<'_>,
        anchor: Pos,
        kinetics: &Kinetics,
    ) -> Option<()> {
        for block in &mut self.block_list {
            let entry = &self.palette[block.palette_index()];
            let position = offset(anchor, block.local_pos());
            let shapes: Vec<_> = world
                .collision_shape(position)?
                .into_iter()
                .map(Collider::from_shape)
                .collect();
            if shapes.as_slice() != [Collider::BLOCK] {
                block.shapes = Some(shapes);
            }
            if entry.name == drill::ID {
                let Some(Kind::Drill(facing)) = states()?.kind(entry.state()?) else {
                    return None;
                };
                self.drills
                    .push(DrillMovementBehaviour::new(block.local_pos(), facing));
            }
            if entry.name == BELT {
                let be = kinetics.get(position)?;
                let Kind::Belt(belt) = be.kind else {
                    return None;
                };
                if be.belt_length < 2
                    || (belt.part == Part::Start) != (be.controller == Some(position))
                {
                    return None;
                }
                let data = crate::belt_data(position, be);
                if !can_transport_objects(belt)
                    && data
                        .inventory
                        .as_ref()
                        .is_some_and(|inventory| !inventory.items.is_empty())
                {
                    return None;
                }
                block.nbt = Some(fastnbt::from_bytes(&fastnbt::to_bytes(&data).ok()?).ok()?);
            } else if entry.has_storage()
                || chassis::is_chassis(&entry.name)
                || bogey::is_bogey(&entry.name)
            {
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
    ) -> &mut StructureBlockInfo {
        let state = *palette.entry(entry.clone()).or_insert_with(|| {
            let state = self.palette.len() as i32;
            self.palette.push(entry);
            state
        });
        let index = self.block_list.len();
        self.block_list.push(StructureBlockInfo {
            pos: packed(pos),
            state,
            nbt: None,
            shapes: None,
        });
        &mut self.block_list[index]
    }

    fn collect_attached(
        anchor: Pos,
        world: Option<World<'_>>,
        mut frontier: VecDeque<(Pos, PaletteEntry)>,
        mut visited: HashSet<Pos>,
        mut get: impl FnMut(Pos) -> Option<BlockMovement>,
        mut attachments: impl FnMut(Pos, &PaletteEntry) -> Option<Vec<Pos>>,
    ) -> Option<(Self, HashMap<PaletteEntry, i32>)> {
        let excluded = visited.clone();
        let mut glue = GlueScan::default();
        visited.extend(frontier.iter().map(|(pos, _)| *pos));
        let mut blocks = Self::default();
        let mut palette = HashMap::new();
        while let Some((pos, entry)) = frontier.pop_front() {
            for attached in attachments(pos, &entry)? {
                if visited.insert(attached) {
                    match get(attached)? {
                        BlockMovement::Movable(next) => frontier.push_back((attached, next)),
                        BlockMovement::Air => {}
                        BlockMovement::Immovable => return None,
                    }
                }
            }
            for (face, step) in FACES.into_iter().enumerate() {
                let adjacent = offset(pos, step);
                if excluded.contains(&adjacent) {
                    continue;
                }
                if visited.contains(&adjacent) {
                    glue.is_glued(world, pos, adjacent);
                    continue;
                }
                // Leave the world intact if an unknown neighbour might attach to this face.
                if let BlockMovement::Movable(next) = get(adjacent)?
                    && (glue.is_glued(world, pos, adjacent)
                        || sticker::attachment(&next, adjacent) == Some(pos)
                        || harvester::facing(&next) == Some(face as u8)
                        || next.bogey_axis() == Some(face_axis(face as u8))
                        || crate::sail::plane(&next)
                            .is_some_and(|axis| axis != face_axis(face as u8))
                        || can_stick(&entry.name, &next.name)
                            && !entry.is_not_supportive(face as u8)
                            && !next.is_not_supportive(face as u8 ^ 1))
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
        (blocks.superglue, blocks.glue_to_remove) = glue.capture(anchor, blocks.positions(anchor));
        Some((blocks, palette))
    }

    pub fn detach_glue(&mut self, world: World<'_>) {
        for id in self.glue_to_remove.drain(..) {
            glue::discard(world, id);
        }
    }

    pub fn place(
        &self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        transform: StructureTransform,
        controller: Option<Pos>,
        previous: Frame,
    ) -> bool {
        if !self.place_structure(world, checks, transform, controller) {
            return false;
        }
        self.settle_seats(world, transform, previous);
        true
    }

    pub fn place_structure(
        &self,
        world: World<'_>,
        checks: &BlockMovementChecks,
        transform: StructureTransform,
        controller: Option<Pos>,
    ) -> bool {
        let targets: Vec<_> = self
            .block_list
            .iter()
            .filter_map(|block| {
                let pos = transform.apply(block.local_pos());
                (Some(pos) != controller).then_some((pos, block))
            })
            .collect();
        if targets.is_empty() {
            return true;
        }
        let positions: Vec<_> = targets.iter().map(|&(position, _)| position).collect();
        let mut destination = vec![UNLOADED_STATE; positions.len()];
        world.read(&positions, &mut destination);
        let Some((place, saved_entities)) = targets
            .into_iter()
            .zip(&destination)
            .map(|((position, block), &previous)| {
                let entry = self.palette.get(block.palette_index())?;
                let belt = entry.name == BELT;
                let waterlog = entry.can_waterlog_on_placement();
                let air = checks.is_air(previous);
                if !air
                    && (!waterlog
                        || CONTENT.get()?.describe_state(previous)?.split('[').next()
                            != Some("minecraft:water"))
                {
                    return None;
                }
                let mut state = transform.block_state(entry);
                if state.name == controls::ID {
                    state.properties.insert("open".into(), "false".into());
                }
                if waterlog {
                    let wet = !air && world.fluid_id(position)? == "minecraft:water";
                    state
                        .properties
                        .insert("waterlogged".into(), wet.to_string());
                }
                Some((
                    BlockUpdate {
                        position,
                        state: state.state()?,
                        flags: content::UPDATE_ALL,
                    },
                    (
                        belt,
                        block.nbt.as_ref().map(fastnbt::to_bytes).transpose().ok()?,
                    ),
                ))
            })
            .collect::<Option<(Vec<_>, Vec<_>)>>()
        else {
            return false;
        };
        let mut glue_placed = Vec::new();
        for &glue in &self.superglue {
            let Some(id) = glue.place(world, transform) else {
                for id in glue_placed {
                    glue::discard(world, id);
                }
                return false;
            };
            glue_placed.push(id);
        }
        if world.write(&place) != place.len() {
            for id in glue_placed {
                glue::discard(world, id);
            }
            return false;
        }
        for (update, (belt, saved)) in place.iter().zip(saved_entities) {
            if let Some(saved) = saved
                && !(if belt {
                    crate::restore_moved_belt(world, update.position, &saved)
                } else {
                    world.restore_block_entity(update.position, &saved)
                })
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
                for id in glue_placed {
                    glue::discard(world, id);
                }
                return false;
            }
        }
        true
    }

    pub fn settle_seats(&self, world: World<'_>, transform: StructureTransform, previous: Frame) {
        for local in self.seats() {
            let destination = transform.apply([local.x, local.y, local.z]);
            seat::move_carrier(
                world,
                previous.to_world(seat::position(local)),
                seat::position(destination),
            );
        }
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
                if entry.name == BELT
                    && let Some(inventory) = block.belt_inventory()
                {
                    for item in inventory.items {
                        if let Some(bytes) = item.bytes() {
                            stacks.spawn(world, position, [0.0, 0.1, 0.0], &bytes);
                        }
                    }
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

pub fn detach_moved_belts(world: World<'_>, remove: &[BlockUpdate], kinetics: &mut Kinetics) {
    let Some(states) = states() else { return };
    let mut level = WorldLevel { world, states };
    // Content events arrive next tick. Stop the captured belt before the current transport tick.
    for update in remove {
        if matches!(kinetics.kind(update.position), Some(Kind::Belt(_))) {
            kinetics.remove_moved(&mut level, update.position);
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ContraptionDisplays {
    blocks: Vec<i32>,
    belts: Option<Vec<(Chain, BeltInventory)>>,
}

impl ContraptionDisplays {
    pub fn clear(&mut self, world: World<'_>) {
        if let Some(displays) = BLOCK_DISPLAYS.get() {
            for id in self.blocks.drain(..).filter(|&id| id != 0) {
                displays.remove(world, id);
            }
        }
        if let Some(belts) = self.belts.take() {
            for (_, inventory) in belts {
                belt_inventory::remove_displays(world, &inventory);
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
        self.render(world, blocks, transform, translation, false);
    }

    pub(crate) fn show_moving(
        &mut self,
        world: World<'_>,
        blocks: &Blocks,
        transform: StructureTransform,
        translation: [f32; 3],
    ) {
        self.render(world, blocks, transform, translation, true);
    }

    fn render(
        &mut self,
        world: World<'_>,
        blocks: &Blocks,
        transform: StructureTransform,
        translation: [f32; 3],
        moving: bool,
    ) {
        let Some(displays) = BLOCK_DISPLAYS.get() else {
            return;
        };
        self.blocks.resize(blocks.block_list.len(), 0);
        for (block, id) in blocks.block_list.iter().zip(&mut self.blocks) {
            let mut shown = transform.block_display(block.local_pos());
            if moving {
                for (position, shift) in shown.position.iter_mut().zip(translation) {
                    *position += f64::from(shift);
                }
                shown.position_interpolation = 1;
            } else {
                for (offset, shift) in shown.translation.iter_mut().zip(translation) {
                    *offset += shift;
                }
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
        let (Some(&stacks), Some(&displays)) = (STACKS.get(), DISPLAYS.get()) else {
            return;
        };
        let belts = self.belts.get_or_insert_with(|| {
            blocks
                .block_list
                .iter()
                .filter_map(|block| {
                    let entry = blocks.palette.get(block.palette_index())?;
                    if entry.name != BELT {
                        return None;
                    }
                    let Some(Kind::Belt(belt)) = states()?.kind(entry.state()?) else {
                        return None;
                    };
                    let inventory = block.belt_inventory()?;
                    let [x, y, z] = block.local_pos();
                    let Value::Int(length) = *block.nbt.as_ref()?.get("Length")? else {
                        return None;
                    };
                    Some((
                        Chain {
                            pos: Pos { x, y, z },
                            belt,
                            length,
                            speed: 0.0,
                        },
                        inventory,
                    ))
                })
                .collect()
        });
        for (chain, inventory) in belts {
            belt_inventory::render(world, stacks, displays, *chain, inventory, |shown| {
                transform.item_display(shown, translation)
            });
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
    fn can_waterlog_on_placement(&self) -> bool {
        matches!(
            self.name.as_str(),
            BELT | drill::ID | harvester::ID | controls::ID
        ) || seat::is_seat(&self.name)
            || bogey::is_bogey(&self.name)
    }

    fn bogey_axis(&self) -> Option<u8> {
        if !bogey::is_bogey(&self.name) {
            return None;
        }
        match self.properties.get("axis")?.as_str() {
            "x" => Some(0),
            "z" => Some(2),
            _ => None,
        }
    }

    pub(crate) fn is_not_supportive(&self, face: u8) -> bool {
        sticker::blocks_attachment(self, face)
            || crate::sail::plane(self) == Some(face_axis(face))
            || matches!(self.name.as_str(), drill::ID | harvester::ID)
                && self
                    .properties
                    .get("facing")
                    .is_some_and(|facing| facing == DIRECTIONS[usize::from(face)])
    }

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shapes: Option<Vec<Collider>>,
}

impl StructureBlockInfo {
    fn belt_inventory(&self) -> Option<BeltInventory> {
        fastnbt::from_value(self.nbt.as_ref()?.get("Inventory")?).ok()
    }

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
        world: Option<World<'_>>,
        mut get: impl FnMut(Pos) -> Option<BlockMovement>,
        attachments: impl FnMut(Pos, &PaletteEntry) -> Option<Vec<Pos>>,
    ) -> Option<Self> {
        let anchor = offset(bearing, FACES[usize::from(facing)]);
        let BlockMovement::Movable(entry) = get(anchor)? else {
            return None;
        };
        let (blocks, _) = Blocks::collect_attached(
            anchor,
            world,
            VecDeque::from([(anchor, entry)]),
            HashSet::from([bearing]),
            get,
            attachments,
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
    pub(crate) fn item_display(self, mut shown: ItemDisplay, translation: [f32; 3]) -> ItemDisplay {
        let axis = usize::from(self.axis);
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        let angle = self.angle.to_radians();
        let (sin, cos) = f64::from(angle).sin_cos();
        let mut position = shown.position.map(|n| n - 0.5);
        let (a, b) = (position[u], position[v]);
        position[u] = a * cos - b * sin;
        position[v] = a * sin + b * cos;
        let anchor = [self.anchor.x, self.anchor.y, self.anchor.z];
        shown.position = std::array::from_fn(|i| {
            f64::from(anchor[i]) + 0.5 + position[i] + f64::from(translation[i])
        });
        let (s, c) = (angle * 0.5).sin_cos();
        let rotation = shown.rotation;
        shown.rotation[axis] = c * rotation[axis] + s * rotation[3];
        shown.rotation[u] = c * rotation[u] - s * rotation[v];
        shown.rotation[v] = c * rotation[v] + s * rotation[u];
        shown.rotation[3] = c * rotation[3] - s * rotation[axis];
        shown
    }

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

    pub fn rotate_face(self, face: u8) -> u8 {
        let direction = self.rotate(FACES[usize::from(face)]);
        FACES
            .iter()
            .position(|&step| step == direction)
            .map_or(face, |index| index as u8)
    }

    pub fn block_state(self, entry: &PaletteEntry) -> PaletteEntry {
        let mut rotated = entry.clone();
        let turns = self.quarter_turns();
        if turns == 0 {
            return rotated;
        }
        if chassis::is_chassis(&entry.name) {
            return chassis::apply_rotation(entry, self).unwrap_or(rotated);
        }
        if entry.name == sticker::ID {
            return sticker::apply_rotation(entry, self).unwrap_or(rotated);
        }
        if entry.name == BELT {
            return crate::belt::transform(rotated, self, turns).unwrap_or_else(|| entry.clone());
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
        let contraption = Contraption::search_moved_structure(
            p(0, 0, 0),
            1,
            None,
            |pos| {
                assert_ne!(pos, p(0, 0, 0));
                Some(tiles.get(&pos).cloned().unwrap_or(Air))
            },
            |_, _| Some(Vec::new()),
        )
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
