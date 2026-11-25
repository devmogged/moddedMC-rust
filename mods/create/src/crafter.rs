//! The Mechanical Crafter's block state and directed item grid.
use std::{
    collections::{HashSet, VecDeque},
    sync::LazyLock,
};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, State, World},
    interaction::{BlockUseResult, UseBlock},
    items::Item,
    stacks::ItemDisplay,
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTAINERS, CRAFTING, DIRECTIONS, DISPLAYS, Kinetics, STACKS, StateIds, TAGS, WRENCH,
    belt_inventory::{TransportedItemStack, count, with_count},
    kinetics::{FACES, Kind, face_axis, positive},
    offset, processing_recipes,
};

pub(crate) const ID: &str = "create:mechanical_crafter";
pub(crate) const SLOT_COVER: &str = "create:crafter_slot_cover";

// Catnip's Pointing order is the order used by the wrench.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Pointing {
    Up,
    Left,
    Down,
    Right,
}

impl Pointing {
    const ALL: [Self; 4] = [Self::Up, Self::Left, Self::Down, Self::Right];

    const fn name(self) -> &'static str {
        ["up", "left", "down", "right"][self as usize]
    }

    const fn cycle(self) -> Self {
        Self::ALL[(self as usize + 1) % Self::ALL.len()]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CrafterBlock {
    pub facing: u8,
    pointing: Pointing,
}

impl CrafterBlock {
    const fn right(self) -> u8 {
        [4, 5, 3, 2][(self.facing - 2) as usize]
    }

    const fn target_face(self) -> u8 {
        match self.pointing {
            Pointing::Up => 1,
            Pointing::Left => self.right() ^ 1,
            Pointing::Down => 0,
            Pointing::Right => self.right(),
        }
    }

    fn target(self, position: Pos) -> Pos {
        offset(position, FACES[usize::from(self.target_face())])
    }

    fn properties(self) -> String {
        format!(
            "facing={},pointing={}",
            DIRECTIONS[usize::from(self.facing)],
            self.pointing.name()
        )
    }

    fn state(self, states: &StateIds) -> Option<u32> {
        states.lookup(&format!("{ID}[{}]", self.properties()))
    }

    fn placement(facing: u8, clicked: u8, inherited: bool) -> Self {
        let positive = positive(facing);
        let pointing = match clicked {
            0 => Pointing::Up,
            1 => Pointing::Down,
            side if !inherited && side == facing => Pointing::Up,
            2 | 5 if positive => Pointing::Left,
            3 | 4 if !positive => Pointing::Left,
            _ => Pointing::Right,
        };
        Self { facing, pointing }
    }

    fn arrow_rotation(self) -> [f32; 4] {
        let [yaw, pitch] = [
            [270_f32, 90.0, 180.0, 0.0][usize::from(self.facing - 2)],
            [0_f32, 270.0, 180.0, 90.0][self.pointing as usize],
        ];
        let (sy, cy) = (yaw.to_radians() / 2.0).sin_cos();
        let (sx, cx) = (pitch.to_radians() / 2.0).sin_cos();
        [cy * sx, sy * cx, -sy * sx, cy * cx]
    }

    fn grid_position(self, pos: Pos) -> [i32; 2] {
        let [x, _, z] = FACES[usize::from(self.right())];
        [x * pos.x + z * pos.z, pos.y]
    }

    fn front(self, pos: Pos, distance: f64) -> [f64; 3] {
        let world = [pos.x, pos.y, pos.z];
        let facing = FACES[usize::from(self.facing)];
        std::array::from_fn(|i| f64::from(world[i]) + 0.5 + f64::from(facing[i]) * distance)
    }
}

fn block_states() -> impl Iterator<Item = CrafterBlock> {
    (2..6u8).flat_map(|facing| {
        Pointing::ALL
            .into_iter()
            .map(move |pointing| CrafterBlock { facing, pointing })
    })
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows: Vec<_> = block_states()
        .map(|block| (block, block.properties()))
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(block, properties)| State {
            properties,
            physical: crate::STONE,
            model: "create:block/mechanical_crafter/block",
            rotating_model: "create:block/mechanical_crafter/arrow",
            x: 0,
            y: [0, 180, 270, 90][usize::from(block.facing - 2)],
            orientation: block.arrow_rotation(),
            shape: &[],
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Mechanical Crafter",
            entity: ID,
            item_model: "create:item/mechanical_crafter",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    crate::ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: SLOT_COVER,
                display_name: "Crafter Slot Cover",
                carrier: "minecraft:iron_nugget",
                model: "create:item/crafter_slot_cover",
            },
        )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for block in block_states() {
        states.add(content, ID, &block.properties(), Some(Kind::Crafter(block)));
    }
}

pub(crate) fn placed(
    states: &StateIds,
    support: u32,
    clicked: u8,
    look: u8,
    sneak: bool,
) -> Option<u32> {
    let (facing, inherited) = match states.kind(support) {
        Some(Kind::Crafter(block)) if !sneak => (block.facing, true),
        _ => (look ^ 1, false),
    };
    CrafterBlock::placement(facing, clicked, inherited).state(states)
}

fn plane_block(kinetics: &crate::Kinetics, position: Pos, facing: u8) -> Option<CrafterBlock> {
    match kinetics.kind(position) {
        Some(Kind::Crafter(block)) if block.facing == facing => Some(block),
        _ => None,
    }
}

fn plane(facing: u8) -> [u8; 4] {
    if face_axis(facing) == 0 {
        [0, 1, 2, 3]
    } else {
        [0, 1, 4, 5]
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
enum Phase {
    #[default]
    Idle,
    Accepting,
    Assembling,
    Exporting,
    Waiting,
    Crafting,
    Inserting,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct GridItem {
    position: [i32; 2],
    stack: Value,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub(crate) struct CrafterData {
    #[serde(skip_serializing_if = "Option::is_none")]
    slot: Option<Value>,
    covered: bool,
    phase: Phase,
    countdown: i32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    grouped_items: Vec<GridItem>,
    was_powered: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Crafter {
    pub(crate) data: CrafterData,
    display: Option<(i32, Value)>,
}

impl Crafter {
    pub(crate) fn hide(&mut self, world: World<'_>) {
        if let Some((id, _)) = self.display.take()
            && let Some(displays) = DISPLAYS.get()
        {
            displays.remove(world, id);
        }
    }

    fn render(&mut self, world: World<'_>, pos: Pos, block: CrafterBlock) {
        let Some(displays) = DISPLAYS.get() else {
            return;
        };
        let shown = self
            .data
            .slot
            .as_ref()
            .or_else(|| self.data.grouped_items.first().map(|item| &item.stack))
            .or_else(|| self.data.covered.then(cover));
        let Some(shown) = shown else {
            self.hide(world);
            return;
        };
        if self
            .display
            .as_ref()
            .is_some_and(|(_, stack)| stack == shown)
        {
            return;
        }
        let Ok(bytes) = fastnbt::to_bytes(shown) else {
            return;
        };
        if let Some((id, item)) = &mut self.display
            && displays.set_item(world, *id, &bytes)
        {
            *item = shown.clone();
            return;
        }
        let angle = [180_f32, 0.0, 270.0, 90.0][usize::from(block.facing - 2)];
        let (y, w) = (angle.to_radians() / 2.0).sin_cos();
        let transform = ItemDisplay {
            position: block.front(pos, 0.52),
            rotation: [0.0, y, 0.0, w],
            scale: 0.47,
            context: 8,
            interpolation: 0,
        };
        self.display = displays
            .spawn_item(world, &bytes, &transform)
            .map(|id| (id, shown.clone()));
    }
}

struct Layout {
    nodes: Vec<(Pos, CrafterBlock)>,
    root: Pos,
}

fn preceding(kinetics: &Kinetics, pos: Pos, block: CrafterBlock) -> impl Iterator<Item = Pos> {
    plane(block.facing).into_iter().filter_map(move |face| {
        if face == block.target_face() {
            return None;
        }
        let other = offset(pos, FACES[usize::from(face)]);
        (plane_block(kinetics, other, block.facing)?.target_face() == face ^ 1).then_some(other)
    })
}

impl Layout {
    fn discover(
        world: World<'_>,
        kinetics: &Kinetics,
        pos: Pos,
        block: CrafterBlock,
    ) -> Option<Self> {
        let mut nodes = Vec::new();
        let mut root = None;
        let mut seen = HashSet::new();
        let mut next = VecDeque::from([(pos, None)]);
        while let Some((pos, source)) = next.pop_front() {
            if !seen.insert(pos) {
                return None;
            }
            let block = plane_block(kinetics, pos, block.facing)?;
            nodes.push((pos, block));
            let neighbors = plane(block.facing).map(|face| offset(pos, FACES[usize::from(face)]));
            let mut loaded = [content::UNLOADED_STATE; 4];
            if !world.read(&neighbors, &mut loaded) || loaded.contains(&content::UNLOADED_STATE) {
                return None;
            }
            let target = block.target(pos);
            if plane_block(kinetics, target, block.facing).is_some() {
                if Some(target) != source {
                    next.push_back((target, Some(pos)));
                }
            } else if root.replace(pos).is_some() {
                return None;
            }
            next.extend(
                preceding(kinetics, pos, block)
                    .filter(|&other| Some(other) != source)
                    .map(|other| (other, Some(pos))),
            );
        }
        Some(Self { nodes, root: root? })
    }

    fn eject(&self, world: World<'_>, kinetics: &mut Kinetics) {
        for &(pos, block) in &self.nodes {
            edit(kinetics, pos, |data| eject(world, pos, block, data));
        }
    }
}

fn edit(kinetics: &mut Kinetics, pos: Pos, change: impl FnOnce(&mut CrafterData)) {
    if let Some(be) = kinetics.get_mut(pos)
        && let Some(crafter) = &mut be.crafter
    {
        change(&mut crafter.data);
        be.dirty = true;
    }
}

fn data(kinetics: &Kinetics, pos: Pos) -> Option<&CrafterData> {
    kinetics
        .get(pos)?
        .crafter
        .as_ref()
        .map(|crafter| &crafter.data)
}

fn drop_stack(world: World<'_>, position: [f64; 3], value: &Value) {
    if let Some(stacks) = STACKS.get()
        && let Ok(stack) = fastnbt::to_bytes(value)
    {
        stacks.spawn(world, position, [0.0, 0.08, 0.0], &stack);
    }
}

fn cover() -> &'static Value {
    static STACK: LazyLock<Value> = LazyLock::new(|| {
        Value::Compound(std::collections::HashMap::from([
            ("id".into(), Value::String(SLOT_COVER.into())),
            ("count".into(), Value::Int(1)),
        ]))
    });
    &STACK
}

fn eject(world: World<'_>, pos: Pos, block: CrafterBlock, data: &mut CrafterData) {
    let front = block.front(pos, 0.75);
    if let Some(stack) = data.slot.take() {
        drop_stack(world, front, &stack);
    }
    for item in data.grouped_items.drain(..) {
        drop_stack(world, front, &item.stack);
    }
    data.phase = Phase::Idle;
    data.countdown = 0;
}

pub(crate) fn remove(world: World<'_>, kinetics: &mut Kinetics, pos: Pos, block: CrafterBlock) {
    if let Some(layout) = Layout::discover(world, kinetics, pos, block) {
        layout.eject(world, kinetics);
    } else {
        edit(kinetics, pos, |data| eject(world, pos, block, data));
    }
    if let Some(be) = kinetics.get_mut(pos)
        && let Some(crafter) = &mut be.crafter
    {
        if crafter.data.covered {
            drop_stack(world, block.front(pos, 0.5), cover());
        }
        crafter.hide(world);
    }
}

pub(crate) fn use_item_on(
    kinetics: &mut Kinetics,
    states: &StateIds,
    block: CrafterBlock,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    if context.face != block.facing || context.sneaking || !context.main_hand {
        return BlockUseResult::TryEmptyHand;
    }
    let pos = context.position;
    let world = context.world;
    let item = context
        .stack
        .read()
        .and_then(|stack| fastnbt::from_bytes::<Value>(&stack).ok());
    let id = item.as_ref().and_then(processing_recipes::item_id);
    let Some(data) = data(kinetics, pos) else {
        return BlockUseResult::Pass;
    };
    if id == Some(WRENCH) {
        let updated = CrafterBlock {
            pointing: block.pointing.cycle(),
            ..block
        };
        let Some(state) = updated.state(states) else {
            return BlockUseResult::Fail;
        };
        if data.phase != Phase::Idle
            && let Some(layout) = Layout::discover(world, kinetics, pos, block)
        {
            layout.eject(world, kinetics);
        }
        return if world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: content::UPDATE_ALL,
        }]) == 1
        {
            BlockUseResult::Success
        } else {
            BlockUseResult::Fail
        };
    }
    if data.phase != Phase::Idle {
        if let Some(layout) = Layout::discover(world, kinetics, pos, block) {
            layout.eject(world, kinetics);
            return BlockUseResult::Success;
        }
        return BlockUseResult::Fail;
    }
    if let Some(item) = item {
        if !data.covered && data.slot.is_none() {
            edit(kinetics, pos, |data| {
                if processing_recipes::item_id(&item) == Some(SLOT_COVER) {
                    data.covered = true;
                } else {
                    data.slot = Some(with_count(&item, 1));
                }
            });
            if !context.creative {
                context.stack.shrink(1);
            }
        }
        return BlockUseResult::Success;
    }
    let covered = data.covered;
    if covered && context.creative {
        edit(kinetics, pos, |data| data.covered = false);
        return BlockUseResult::Success;
    }
    let returned = if covered {
        Some(cover())
    } else {
        data.slot.as_ref()
    };
    if let Some(returned) = returned
        && let Ok(bytes) = fastnbt::to_bytes(returned)
        && context.give(&bytes)
    {
        edit(kinetics, pos, |data| {
            data.covered = false;
            data.slot = None;
        });
        return BlockUseResult::Success;
    }
    BlockUseResult::TryEmptyHand
}

pub(crate) fn insert(
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &Value,
    simulate: bool,
) -> Option<i32> {
    let data = data(kinetics, pos)?;
    if data.phase != Phase::Idle || data.covered || data.slot.is_some() || count(item) == 0 {
        return Some(0);
    }
    if !simulate {
        edit(kinetics, pos, |data| data.slot = Some(with_count(item, 1)));
    }
    Some(1)
}

const EXPORT_TIME: i32 = 1000;
const CRAFT_TIME: i32 = 2000;
const INSERT_INTERVAL: u64 = 10;

// Create's CRecipes.maxFireworkIngredientsInCrafter default.
const MAX_FIREWORK_INGREDIENTS: usize = 9;

fn result(world: World<'_>, items: &[GridItem]) -> Option<Vec<Value>> {
    let [mut left, mut bottom] = items.first()?.position;
    let (mut right, mut top) = (left, bottom);
    for item in items {
        let [x, y] = item.position;
        left = left.min(x);
        right = right.max(x);
        bottom = bottom.min(y);
        top = top.max(y);
    }
    let width = usize::try_from(right.checked_sub(left)?.checked_add(1)?).ok()?;
    let height = usize::try_from(top.checked_sub(bottom)?.checked_add(1)?).ok()?;
    if width > processing_recipes::MAX_CRAFTER_SIDE || height > processing_recipes::MAX_CRAFTER_SIDE
    {
        return None;
    }
    let mut grid = vec![None; width * height];
    for item in items {
        let [x, y] = item.position;
        let cell =
            &mut grid[usize::try_from(top - y).ok()? * width + usize::try_from(x - left).ok()?];
        if cell.replace(&item.stack).is_some() {
            return None;
        }
    }
    let input = grid
        .iter()
        .map(|stack| stack.map_or_else(|| Ok(Vec::new()), fastnbt::to_bytes))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    let crafting = *CRAFTING.get()?;
    let ordinary = crafting
        .match_recipe(
            world,
            u8::try_from(width).ok()?,
            u8::try_from(height).ok()?,
            &input,
        )
        .filter(|recipe| {
            !recipe
                .id
                .as_ref()
                .is_some_and(|id| id.ends_with("_manual_only"))
                && (recipe.serializer != "minecraft:crafting_special_firework_rocket"
                    || items.len() <= MAX_FIREWORK_INGREDIENTS)
                && !TAGS.get().is_some_and(|tags| {
                    tags.contains(
                        "recipe_serializer",
                        "create:automation_ignore",
                        &recipe.serializer,
                    )
                })
        });
    let product = if let Some(recipe) = ordinary {
        fastnbt::from_bytes(&recipe.result).ok()?
    } else {
        let ids: Vec<_> = grid
            .chunks_exact(width)
            .map(|row| {
                row.iter()
                    .map(|slot| slot.and_then(processing_recipes::item_id))
                    .collect()
            })
            .collect();
        let product = processing_recipes::current()?
            .mechanical_craft(&ids)?
            .clone();
        STACKS.get()?.info(&fastnbt::to_bytes(&product).ok()?)?;
        product
    };
    let remainders = input
        .iter()
        .filter(|slot| !slot.is_empty())
        .filter_map(|slot| {
            let bytes = crafting.remainder(slot)?;
            fastnbt::from_bytes(&bytes).ok()
        });
    Some(std::iter::once(product).chain(remainders).collect())
}

impl Layout {
    fn start_if_ready(&self, world: World<'_>, kinetics: &mut Kinetics) {
        let mut all_idle = true;
        let mut ready = true;
        let mut has_item = false;
        let mut moving = false;
        let mut triggered = false;
        for &(pos, _) in &self.nodes {
            let Some(be) = kinetics.get_mut(pos) else {
                return;
            };
            let speed = be.get_speed();
            let Some(crafter) = &mut be.crafter else {
                return;
            };
            let data = &mut crafter.data;
            all_idle &= data.phase == Phase::Idle;
            ready &= data.covered || data.slot.is_some();
            has_item |= data.slot.is_some();
            moving |= speed != 0.0;
            if data.phase != Phase::Accepting {
                let powered = world.has_neighbor_signal(pos);
                if data.was_powered != powered {
                    triggered |= powered && speed != 0.0;
                    data.was_powered = powered;
                    be.dirty = true;
                }
            }
        }
        if !all_idle || !has_item || !moving || !(ready || triggered) {
            return;
        }
        for &(pos, block) in &self.nodes {
            let leaf = preceding(kinetics, pos, block).next().is_none();
            edit(kinetics, pos, |data| {
                if let Some(stack) = data.slot.take() {
                    data.grouped_items.push(GridItem {
                        position: block.grid_position(pos),
                        stack,
                    });
                }
                data.phase = if leaf {
                    Phase::Assembling
                } else {
                    Phase::Accepting
                };
                data.countdown = i32::from(leaf);
            });
        }
    }

    fn output(&self, world: World<'_>, kinetics: &mut Kinetics, pos: Pos, block: CrafterBlock) {
        let target = block.target(pos);
        let side = block.target_face() ^ 1;
        let target_accepts = matches!(
            kinetics.kind(target),
            Some(Kind::Belt(_) | Kind::Crafter(_))
        ) || kinetics.depots.contains_key(&target)
            || CONTAINERS
                .get()
                .and_then(|containers| containers.at(world, target, side))
                .is_some();
        if !target_accepts {
            self.eject(world, kinetics);
            return;
        }

        let mut items = Vec::new();
        edit(kinetics, pos, |data| {
            items = std::mem::take(&mut data.grouped_items)
        });
        items.retain_mut(|item| {
            let remaining = count(&item.stack);
            let stack = TransportedItemStack::new(item.stack.clone());
            let inserted = crate::funnel::insert(world, kinetics, target, side, &stack, false);
            if inserted >= remaining {
                false
            } else {
                item.stack = with_count(&item.stack, remaining - inserted);
                true
            }
        });
        if items.is_empty() {
            self.eject(world, kinetics);
        } else {
            edit(kinetics, pos, |data| {
                data.grouped_items = items;
                data.phase = Phase::Inserting;
            });
        }
    }

    fn tick(&self, world: World<'_>, kinetics: &mut Kinetics, time: u64) {
        self.start_if_ready(world, kinetics);
        for &(pos, block) in &self.nodes {
            let Some(be) = kinetics.get(pos) else {
                continue;
            };
            let Some(snapshot) = be.crafter.as_ref().map(|crafter| &crafter.data) else {
                continue;
            };
            let phase = snapshot.phase;
            if phase == Phase::Inserting {
                if time.is_multiple_of(INSERT_INTERVAL)
                    || matches!(kinetics.kind(block.target(pos)), Some(Kind::Belt(_)))
                {
                    self.output(world, kinetics, pos, block);
                }
                continue;
            }
            if !matches!(
                phase,
                Phase::Assembling | Phase::Exporting | Phase::Crafting
            ) {
                continue;
            }
            let speed = be.get_speed().abs();
            if speed == 0.0 {
                continue;
            }
            let countdown = snapshot.countdown - (speed as i32).clamp(4, 250);
            if countdown >= 0 {
                edit(kinetics, pos, |data| data.countdown = countdown);
                continue;
            }
            match phase {
                Phase::Assembling if pos != self.root => {
                    edit(kinetics, pos, |data| {
                        data.phase = Phase::Exporting;
                        data.countdown = if data.grouped_items.is_empty() {
                            0
                        } else {
                            EXPORT_TIME
                        };
                    });
                }
                Phase::Assembling => {
                    let Some(products) = result(world, &snapshot.grouped_items) else {
                        self.eject(world, kinetics);
                        break;
                    };
                    edit(kinetics, pos, |data| {
                        data.phase = Phase::Crafting;
                        data.countdown = CRAFT_TIME;
                        let [x, y] = block.grid_position(pos);
                        data.grouped_items = (x..)
                            .zip(products)
                            .map(|(x, stack)| GridItem {
                                position: [x, y],
                                stack,
                            })
                            .collect();
                    });
                }
                Phase::Exporting => {
                    let mut moved = Vec::new();
                    edit(kinetics, pos, |data| {
                        moved = std::mem::take(&mut data.grouped_items);
                        data.phase = Phase::Waiting;
                        data.countdown = 0;
                    });
                    let target = block.target(pos);
                    let Some(target_block) = plane_block(kinetics, target, block.facing) else {
                        edit(kinetics, pos, |data| data.grouped_items = moved);
                        self.eject(world, kinetics);
                        break;
                    };
                    let ready = preceding(kinetics, target, target_block).all(|other| {
                        data(kinetics, other).is_some_and(|data| data.phase == Phase::Waiting)
                    });
                    edit(kinetics, target, |data| {
                        data.grouped_items.extend(moved);
                        if ready {
                            data.phase = Phase::Assembling;
                            data.countdown = 1;
                        }
                    });
                }
                Phase::Crafting => {
                    self.output(world, kinetics, pos, block);
                }
                _ => {}
            }
        }
    }
}

pub(crate) fn tick(world: World<'_>, kinetics: &mut Kinetics, time: u64) {
    let mut seen = HashSet::new();
    for pos in kinetics.positions(|kind| matches!(kind, Kind::Crafter(_))) {
        let Some(Kind::Crafter(block)) = kinetics.kind(pos) else {
            continue;
        };
        if !seen.contains(&pos)
            && let Some(layout) = Layout::discover(world, kinetics, pos, block)
        {
            seen.extend(layout.nodes.iter().map(|&(pos, _)| pos));
            layout.tick(world, kinetics, time);
        }
        if let Some(be) = kinetics.get_mut(pos)
            && let Some(crafter) = &mut be.crafter
        {
            crafter.render(world, pos, block);
        }
    }
}
