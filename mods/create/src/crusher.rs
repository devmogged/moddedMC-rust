//! Create 6.0.10's CrushingWheelBlock and CrushingWheelControllerBlockEntity.

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, ShapeBox, State, World},
    entities::{ENTITY_ALIVE, ENTITY_ITEM, Entity, EntityChange, SET_MOTION},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    AXES, DIRECTIONS, ENTITIES, ITEMS, STACKS, STONE, StateIds,
    belt_inventory::{self, TransportedItemStack, count, with_count},
    kinetics::{FACES, Kind, Kinetics, face_axis, offset, positive},
    millstone::InventoryData,
    processing_recipes::{Process, Recipe, RecipeBook},
};

pub const ID: &str = "create:crushing_wheel";
const CONTROLLER: &str = "create:crushing_wheel_controller";
const DOWN: u8 = 0;
const UP: u8 = 1;
const SLOTS: usize = 32;
const NO_RECIPE_TIME: f32 = 100.0;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let props = AXES.map(|axis| format!("axis={axis}"));
    let states = std::array::from_fn::<_, 3, _>(|axis| State {
        properties: &props[axis],
        physical: STONE,
        // shortcut: stock clients cannot load the original NeoForge OBJ model, replace when C5 can read it.
        model: "minecraft:block/polished_deepslate",
        rotating_model: "",
        x: 0,
        y: 0,
        orientation: crate::relay_orientation(axis as u8),
        shape: &[],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Crushing Wheel",
            entity: ID,
            item_model: "minecraft:block/polished_deepslate",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    let rows: [(String, [ShapeBox; 1]); 12] = std::array::from_fn(|i| {
        let face = (i / 2) as u8;
        let mut min = [0; 3];
        let mut max = [16; 3];
        if positive(face) {
            min[usize::from(face_axis(face))] = 3;
        } else {
            max[usize::from(face_axis(face))] = 13;
        }
        (
            format!(
                "facing={},valid={}",
                DIRECTIONS[usize::from(face)],
                i % 2 != 0
            ),
            [ShapeBox { min, max }],
        )
    });
    let states = rows.each_ref().map(|(properties, shape)| State {
        properties,
        physical: "minecraft:structure_void",
        model: "moddedmc:block/empty",
        rotating_model: "",
        x: 0,
        y: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
        shape,
    });
    content.add_block(
        registry,
        &Block {
            id: CONTROLLER,
            display_name: "Crushing Wheel Controller",
            entity: CONTROLLER,
            item_model: "",
            states: &states,
        },
        None,
    )?;
    let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (path, name) in [
        ("cinder_flour", "Cinder Flour"),
        ("copper_nugget", "Copper Nugget"),
        ("crushed_raw_aluminum", "Crushed Raw Aluminum"),
        ("crushed_raw_copper", "Crushed Raw Copper"),
        ("crushed_raw_gold", "Crushed Raw Gold"),
        ("crushed_raw_iron", "Crushed Raw Iron"),
        ("crushed_raw_lead", "Crushed Raw Lead"),
        ("crushed_raw_nickel", "Crushed Raw Nickel"),
        ("crushed_raw_osmium", "Crushed Raw Osmium"),
        ("crushed_raw_platinum", "Crushed Raw Platinum"),
        ("crushed_raw_quicksilver", "Crushed Raw Quicksilver"),
        ("crushed_raw_silver", "Crushed Raw Silver"),
        ("crushed_raw_tin", "Crushed Raw Tin"),
        ("crushed_raw_uranium", "Crushed Raw Uranium"),
        ("crushed_raw_zinc", "Crushed Raw Zinc"),
        ("experience_nugget", "Nugget of Experience"),
        ("powdered_obsidian", "Powdered Obsidian"),
        ("zinc_nugget", "Zinc Nugget"),
        ("dough", "Dough"),
    ] {
        items.add_item(
            registry,
            &Item {
                id: &format!("create:{path}"),
                display_name: name,
                carrier: "minecraft:paper",
                model: &format!("create:item/{path}"),
            },
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (axis, name) in AXES.iter().enumerate() {
        states.add(
            content,
            ID,
            &format!("axis={name}"),
            Some(Kind::Wheel(axis as u8)),
        );
    }
    for (face, name) in DIRECTIONS.iter().enumerate() {
        for valid in [false, true] {
            let properties = format!("facing={name},valid={valid}");
            states.add(content, CONTROLLER, &properties, None);
            if let Some(state) = states.lookup(&format!("{CONTROLLER}[{properties}]")) {
                states
                    .crusher_controllers
                    .insert(state, (face as u8, valid));
            }
        }
    }
}

#[derive(Debug)]
pub struct Controller {
    facing: u8,
    speed: f32,
    contents: [Option<Value>; SLOTS],
    remaining_time: f32,
    applied_recipe: bool,
    processing_entity: Option<i32>,
    dirty: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ControllerData {
    speed: f32,
    inventory: ProcessingInventory,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct ProcessingInventory {
    #[serde(flatten)]
    slots: InventoryData,
    processing_time: f32,
    recipe_time: f32,
    applied_recipe: bool,
}

impl Controller {
    fn load(world: World<'_>, pos: Pos, facing: u8) -> Self {
        let mut controller = Self {
            facing,
            speed: 0.0,
            contents: std::array::from_fn(|_| None),
            remaining_time: 0.0,
            applied_recipe: false,
            processing_entity: None,
            dirty: true,
        };
        if let Some(saved) = world
            .entity(pos)
            .and_then(|bytes| fastnbt::from_bytes::<ControllerData>(&bytes).ok())
        {
            controller.speed = saved.speed;
            controller.contents = saved.inventory.slots.load();
            controller.remaining_time = saved.inventory.processing_time;
            controller.applied_recipe = saved.inventory.applied_recipe;
        }
        controller
    }

    fn save(&self, world: World<'_>, pos: Pos) -> bool {
        fastnbt::to_bytes(&ControllerData {
            speed: self.speed,
            inventory: ProcessingInventory {
                slots: InventoryData::save(&self.contents),
                processing_time: self.remaining_time,
                recipe_time: 0.0,
                applied_recipe: self.applied_recipe,
            },
        })
        .is_ok_and(|bytes| world.set_entity(pos, &bytes))
    }

    fn occupied(&self) -> bool {
        self.processing_entity.is_some() || self.contents.iter().any(Option::is_some)
    }

    fn item_inserted(&mut self, item: Value, recipes: &RecipeBook) {
        self.remaining_time =
            find_recipe(recipes, &item).map_or(NO_RECIPE_TIME, |recipe| recipe.duration as f32);
        self.applied_recipe = false;
        self.contents[0] = Some(item);
        self.dirty = true;
    }

    fn apply_recipe(&mut self, recipes: &RecipeBook) {
        if let Some(input) = self.contents[0].take()
            && let Some(recipe) = find_recipe(recipes, &input)
        {
            // ProcessingInventory.clear() also clears the timer before the output slots are filled.
            self.remaining_time = 0.0;
            for (slot, item) in self.contents[1..]
                .iter_mut()
                .zip(recipe.roll_results(count(&input)))
            {
                *slot = Some(item);
            }
        }
        self.applied_recipe = true;
        self.dirty = true;
    }

    fn tick_inventory(
        &mut self,
        world: World<'_>,
        kinetics: &mut Kinetics,
        pos: Pos,
        recipes: &RecipeBook,
    ) {
        let input = self.contents[0].as_ref().map_or(1, count);
        let divisor = if self.applied_recipe {
            1.0
        } else {
            (input as f32).log2()
        };
        self.remaining_time -= (self.speed * 4.0 / divisor).clamp(0.25, 20.0);
        self.dirty = true;
        if self.remaining_time < 20.0 && !self.applied_recipe {
            self.apply_recipe(recipes);
            return;
        }
        if self.remaining_time > 0.0 {
            return;
        }
        self.remaining_time = 0.0;
        let below = offset(pos, FACES[usize::from(DOWN)]);
        let target = if self.facing < 2 {
            below
        } else {
            offset(below, FACES[usize::from(self.facing)])
        };
        for slot in self.contents.iter_mut().filter(|slot| slot.is_some()) {
            let Some(item) = slot.as_ref() else { continue };
            let transported = TransportedItemStack::new(item.clone());
            if self.facing != UP
                && (kinetics.depots.contains_key(&target)
                    || kinetics.chutes.entries.contains_key(&target)
                    || matches!(kinetics.kind(target), Some(Kind::Belt(_) | Kind::Millstone)))
            {
                let accepted = if matches!(kinetics.kind(target), Some(Kind::Belt(_))) {
                    if belt_inventory::try_inserting_from_side(
                        kinetics,
                        None,
                        target,
                        &transported,
                        self.facing,
                        false,
                    ) {
                        count(item)
                    } else {
                        0
                    }
                } else {
                    crate::funnel::insert(world, kinetics, target, self.facing, &transported, false)
                };
                let remaining = count(item) - accepted;
                *slot = (remaining > 0).then(|| with_count(item, remaining));
            } else if eject(world, pos, self.facing, item, true) {
                *slot = None;
            }
        }
    }

    fn tick_entity(&mut self, world: World<'_>, pos: Pos, id: i32, recipes: &RecipeBook) {
        let (Some(&entities), Some(&stacks)) = (ENTITIES.get(), STACKS.get()) else {
            return;
        };
        let midpoint = center(pos);
        let axis = usize::from(face_axis(self.facing));
        let sign = f64::from(FACES[usize::from(self.facing)][axis]);
        let active = entities.with_entity(world, id, |entity| {
            let Some(view) = entity.view() else {
                return false;
            };
            if entity.flags() & ENTITY_ALIVE == 0
                || (0..3).any(|i| (view.position[i] - midpoint[i]).abs() > 1.0)
            {
                return false;
            }
            if (view.position[axis] - (midpoint[axis] - 0.25)) * sign <= 0.0 {
                let Some(item) = stacks
                    .entity_stack(entity)
                    .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
                else {
                    return false;
                };
                if stacks.set_entity_stack(entity, &[]) {
                    self.item_inserted(item, recipes);
                }
                return false;
            }
            // shortcut: move only the captured item through the controller until collision can exclude that entity.
            let mut step = view.position;
            for i in [0, 2] {
                step[i] += (midpoint[i] - step[i]) * 0.125;
            }
            step[axis] = view.position[axis] - sign * f64::from(self.speed.min(0.5)) * 0.05;
            entity.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0; 3],
                ..EntityChange::default()
            });
            entity.set_position(step);
            true
        });
        if active != Some(true) {
            self.processing_entity = None;
        }
    }
}

fn find_recipe<'a>(recipes: &'a RecipeBook, item: &Value) -> Option<&'a Recipe> {
    recipes
        .find(Process::Crushing, item)
        .or_else(|| recipes.find(Process::Milling, item))
}

fn center(pos: Pos) -> [f64; 3] {
    [pos.x, pos.y, pos.z].map(|n| f64::from(n) + 0.5)
}

fn eject(world: World<'_>, pos: Pos, facing: u8, item: &Value, bypass: bool) -> bool {
    let (Some(&stacks), Ok(bytes)) = (STACKS.get(), fastnbt::to_bytes(item)) else {
        return false;
    };
    let direction = FACES[usize::from(facing)].map(f64::from);
    let mid = center(pos);
    let position = std::array::from_fn(|axis| mid[axis] + f64::from(0.55f32) * direction[axis]);
    let motion = [
        direction[0] * 0.25,
        direction[1].max(0.0) * 0.5,
        direction[2] * 0.25,
    ];
    let Some(id) = stacks.spawn(world, position, motion, &bytes) else {
        return false;
    };
    if bypass
        && let Some(&entities) = ENTITIES.get()
        && let Ok(data) = fastnbt::to_bytes(&BypassCrushingWheel::new(pos))
    {
        entities.with_entity(world, id, |entity| {
            entity.set_data("create_crushing_wheel", &data)
        });
    }
    true
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct BypassCrushingWheel {
    #[serde(rename = "BypassCrushingWheel")]
    position: [i32; 3],
}

impl BypassCrushingWheel {
    const fn new(pos: Pos) -> Self {
        Self {
            position: [pos.x, pos.y, pos.z],
        }
    }
}

pub fn entity_inside(kinetics: &mut Kinetics, pos: Pos, entity: Entity<'_>) {
    let Some(controller) = kinetics.crushers.get_mut(&pos) else {
        return;
    };
    if controller.speed == 0.0
        || controller.occupied()
        || entity
            .view()
            .is_none_or(|view| view.kind & ENTITY_ITEM == 0)
        || entity.flags() & ENTITY_ALIVE == 0
    {
        return;
    }
    if entity
        .data("create_crushing_wheel")
        .and_then(|bytes| fastnbt::from_bytes::<BypassCrushingWheel>(&bytes).ok())
        .is_some_and(|data| data == BypassCrushingWheel::new(pos))
    {
        return;
    }
    controller.processing_entity = Some(entity.id());
}

pub fn is_controller(kinetics: &Kinetics, pos: Pos, side: u8) -> bool {
    kinetics
        .crushers
        .get(&pos)
        .is_some_and(|controller| controller.facing == DOWN || controller.facing == side)
}

pub fn insert(kinetics: &mut Kinetics, pos: Pos, item: &Value, simulate: bool) -> i32 {
    let Some(controller) = kinetics.crushers.get_mut(&pos) else {
        return 0;
    };
    if controller.occupied() {
        return 0;
    }
    let Some(recipes) = crate::processing_recipes::current() else {
        return 0;
    };
    let amount = count(item).min(crate::millstone::max_count(item));
    if !simulate && amount > 0 {
        controller.item_inserted(with_count(item, amount), &recipes);
    }
    amount
}

pub fn event(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics, event: &ContentEvent) {
    let pos = event.position;
    match event.reason {
        content::STATE_CHANGED
            if let Some(Kind::Wheel(axis)) = states.kind(event.previous)
                && states.kind(event.state) != Some(Kind::Wheel(axis)) =>
        {
            if let Some(&air) = states.vanilla.get("minecraft:air") {
                for (face, step) in FACES.into_iter().enumerate() {
                    if face_axis(face as u8) == axis {
                        continue;
                    }
                    let position = offset(pos, step);
                    let mut block = [content::UNLOADED_STATE];
                    world.read(&[position], &mut block);
                    if states.crusher_controllers.contains_key(&block[0]) {
                        world.write(&[BlockUpdate {
                            position,
                            state: air,
                            flags: content::UPDATE_ALL,
                        }]);
                    }
                }
            }
        }
        content::UNLOADED => {
            kinetics.crushers.remove(&pos);
        }
        content::LOADED if let Some(&(face, _)) = states.crusher_controllers.get(&event.state) => {
            kinetics
                .crushers
                .insert(pos, Controller::load(world, pos, face));
        }
        content::STATE_CHANGED
            if !states.crusher_controllers.contains_key(&event.state)
                && let Some(controller) = kinetics.crushers.remove(&pos) =>
        {
            for item in controller.contents.into_iter().flatten() {
                eject(world, pos, UP, &item, false);
            }
        }
        _ => {}
    }
}

fn flow_direction(axis: u8, side: u8, speed: f32) -> u8 {
    let mut a = [0; 3];
    let mut b = [0; 3];
    a[usize::from(axis)] = if speed > 0.0 { 1 } else { -1 };
    b[usize::from(face_axis(side))] = if positive(side) { 1 } else { -1 };
    let cross =
        std::array::from_fn(|i| a[(i + 1) % 3] * b[(i + 2) % 3] - a[(i + 2) % 3] * b[(i + 1) % 3]);
    FACES
        .iter()
        .position(|&f| f == cross)
        .map_or(DOWN, |face| face as u8)
}

fn sync_controllers(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    let wheels = kinetics.positions(|kind| matches!(kind, Kind::Wheel(_)));
    let mut seen = std::collections::HashSet::new();
    for wheel in wheels {
        let Some(be) = kinetics.get(wheel) else {
            continue;
        };
        let Kind::Wheel(axis) = be.kind else { continue };
        let speed = be.get_speed();
        for (side, step) in FACES.into_iter().enumerate() {
            let side = side as u8;
            if face_axis(side) == axis {
                continue;
            }
            let pos = offset(wheel, step);
            let other = offset(pos, step);
            let pair = kinetics
                .get(other)
                .filter(|be| be.kind == Kind::Wheel(axis));
            let Some(pair) = pair else { continue };
            if !seen.insert(pos) {
                continue;
            }
            let mut blocks = [content::UNLOADED_STATE];
            world.read(&[pos], &mut blocks);
            if !states.crusher_controllers.contains_key(&blocks[0])
                && !states.replaceable.contains(&blocks[0])
            {
                continue;
            }
            let other_speed = pair.get_speed();
            let valid = speed * other_speed < 0.0;
            let face = if valid {
                flow_direction(axis, side, speed)
            } else {
                DOWN
            };
            let crush_speed = if valid {
                (if positive(side) {
                    speed.abs()
                } else {
                    other_speed.abs()
                }) / 50.0
            } else {
                0.0
            };
            let Some(state) = states.lookup(&format!(
                "{CONTROLLER}[facing={},valid={valid}]",
                DIRECTIONS[usize::from(face)]
            )) else {
                continue;
            };
            if state != blocks[0]
                && world.write(&[BlockUpdate {
                    position: pos,
                    state,
                    flags: content::UPDATE_ALL,
                }]) == 0
            {
                continue;
            }
            let controller = kinetics
                .crushers
                .entry(pos)
                .or_insert_with(|| Controller::load(world, pos, face));
            controller.dirty |= controller.facing != face || controller.speed != crush_speed;
            controller.facing = face;
            controller.speed = crush_speed;
        }
    }
}

pub fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    sync_controllers(world, states, kinetics);
    if kinetics.crushers.is_empty() {
        return;
    }
    let Some(recipes) = crate::processing_recipes::current() else {
        return;
    };
    for pos in kinetics.crushers.keys().copied().collect::<Vec<_>>() {
        let Some(mut controller) = kinetics.crushers.remove(&pos) else {
            continue;
        };
        if controller.speed != 0.0 {
            if let Some(id) = controller.processing_entity {
                controller.tick_entity(world, pos, id, &recipes);
            } else if controller.occupied() {
                controller.tick_inventory(world, kinetics, pos, &recipes);
            }
        }
        if controller.dirty {
            controller.dirty = !controller.save(world, pos);
        }
        kinetics.crushers.insert(pos, controller);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_wheels_agree_on_crushing_direction() {
        assert_eq!(flow_direction(2, 5, -128.0), DOWN);
        assert_eq!(flow_direction(2, 4, 64.0), DOWN);
        assert_eq!(flow_direction(2, 4, -64.0), UP);
        assert_eq!(flow_direction(2, 1, 64.0), 4);
    }
}
