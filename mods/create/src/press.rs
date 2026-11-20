//! Create 6.0.10's MechanicalPressBlock, PressingBehaviour and BeltPressingCallbacks.

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, Placement, ShapeBox, State, World},
    effects::SOUND_BLOCKS,
    entities::{ENTITY_ALIVE, ENTITY_ITEM, Entity},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    DIRECTIONS, EFFECTS, ENTITIES, ITEMS, STACKS, STONE, StateIds,
    belt_inventory::{TransportedItemStack, count, next_int, with_count},
    kinetics::{FACES, Kind, Kinetics, face_axis, offset},
    processing_recipes::{Process, RecipeBook},
};

pub const ID: &str = "create:mechanical_press";
const CYCLE: i32 = 240;
const STROKE: i32 = CYCLE / 2;
const ENTITY_SCAN: u8 = 10;
const WORLD: i32 = 0;
const BELT: i32 = 1;
const HORIZONTAL: [u8; 4] = [2, 3, 4, 5];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let rows = HORIZONTAL.map(|face| {
        let rotation = match face {
            3 => 180,
            4 => 270,
            5 => 90,
            _ => 0,
        };
        (
            format!("facing={}", DIRECTIONS[usize::from(face)]),
            rotation,
        )
    });
    let states = rows.each_ref().map(|(properties, yaw)| State {
        properties,
        physical: STONE,
        model: "create:block/mechanical_press/block",
        rotating_model: "create:block/shaft",
        x: 0,
        y: *yaw,
        orientation: crate::relay_orientation(if *yaw % 180 == 0 { 2 } else { 0 }),
        shape: &[
            ShapeBox {
                min: [0, 0, 0],
                max: [4, 16, 16],
            },
            ShapeBox {
                min: [12, 0, 0],
                max: [16, 16, 16],
            },
            ShapeBox {
                min: [4, 0, 0],
                max: [12, 16, 4],
            },
            ShapeBox {
                min: [4, 0, 12],
                max: [12, 16, 16],
            },
        ],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Mechanical Press",
            entity: ID,
            item_model: "create:item/mechanical_press",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (id, name, carrier, model) in [
        (
            "create:iron_sheet",
            "Iron Sheet",
            "minecraft:iron_ingot",
            "create:item/iron_sheet",
        ),
        (
            "create:copper_sheet",
            "Copper Sheet",
            "minecraft:copper_ingot",
            "create:item/copper_sheet",
        ),
        (
            "create:golden_sheet",
            "Golden Sheet",
            "minecraft:gold_ingot",
            "create:item/golden_sheet",
        ),
        (
            "create:brass_sheet",
            "Brass Sheet",
            "minecraft:gold_ingot",
            "create:item/brass_sheet",
        ),
    ] {
        items.add_item(
            registry,
            &Item {
                id,
                display_name: name,
                carrier,
                model,
            },
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for face in HORIZONTAL {
        ids.add(
            content,
            ID,
            &format!("facing={}", DIRECTIONS[usize::from(face)]),
            Some(Kind::Press(face_axis(face))),
        );
    }
}

pub fn placed_state(args: Placement<'_>, states: &StateIds) -> Option<u32> {
    let positions = HORIZONTAL.map(|face| offset(args.position, FACES[usize::from(face)]));
    let mut found = [content::UNLOADED_STATE; 4];
    args.world.read(&positions, &mut found);
    let mut candidates = HORIZONTAL
        .into_iter()
        .zip(found)
        .filter(|(face, block)| {
            states
                .kind(*block)
                .is_some_and(|kind| kind.has_shaft_towards(face ^ 1))
        })
        .map(|(face, _)| face);
    let first = candidates.next();
    let preferred =
        first.filter(|&face| candidates.all(|other| face_axis(face) == face_axis(other)));
    let facing = preferred.unwrap_or_else(|| {
        let horizontal = [3, 4, 2, 5][((args.yaw / 90.0 + 0.5).floor() as i32 & 3) as usize];
        horizontal ^ 1
    });
    states.lookup(&format!("{ID}[facing={}]", DIRECTIONS[usize::from(facing)]))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct PressingData {
    running: bool,
    mode: i32,
    finished: bool,
    ticks: i32,
}

impl PressingData {
    pub const EMPTY: Self = Self {
        running: false,
        mode: WORLD,
        finished: false,
        ticks: 0,
    };
}

#[derive(Clone, Debug)]
pub struct PressingBehaviour {
    pub data: PressingData,
    entity_scan_cooldown: u8,
}

impl PressingBehaviour {
    pub const fn new(data: PressingData) -> Self {
        Self {
            data,
            entity_scan_cooldown: ENTITY_SCAN,
        }
    }

    fn start(&mut self, mode: i32) {
        self.data.mode = mode;
        self.data.running = true;
        self.data.ticks = 0;
    }

    fn advance(&mut self, speed: f32) {
        if self.data.ticks > CYCLE {
            self.data.finished = true;
            self.data.running = false;
            return;
        }
        let previous = self.data.ticks;
        self.data.ticks += running_tick_speed(speed);
        if previous < STROKE && self.data.ticks >= STROKE {
            self.data.ticks = STROKE;
        }
    }
}

fn running_tick_speed(speed: f32) -> i32 {
    if speed == 0.0 {
        return 0;
    }
    (1.0 + 59.0 * (speed.abs() / 512.0).min(1.0)) as i32
}

pub enum ProcessingResult {
    Pass,
    Hold,
    Convert(Vec<Value>),
}

pub fn process_item(
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
) -> ProcessingResult {
    let Some(recipes) = kinetics.recipes.as_ref().cloned() else {
        return ProcessingResult::Pass;
    };
    let Some(be) = kinetics.get_mut(above(pos, 2)) else {
        return ProcessingResult::Pass;
    };
    if be.get_speed() == 0.0 {
        return ProcessingResult::Pass;
    }
    let Some(press) = &mut be.press else {
        return ProcessingResult::Pass;
    };
    if item.locked {
        if !press.data.running {
            return ProcessingResult::Pass;
        }
        if press.data.ticks != STROKE {
            return ProcessingResult::Hold;
        }
        return recipes
            .find(Process::Pressing, &item.item)
            .map_or(ProcessingResult::Pass, |recipe| {
                ProcessingResult::Convert(recipe.outputs())
            });
    }
    if press.data.running {
        return ProcessingResult::Hold;
    }
    if recipes.find(Process::Pressing, &item.item).is_none() {
        return ProcessingResult::Pass;
    }
    press.start(BELT);
    be.dirty = true;
    ProcessingResult::Hold
}

pub fn is_blocked(world: World<'_>, pos: Pos) -> bool {
    let Some(states) = crate::states() else {
        return true;
    };
    let mut block = [content::UNLOADED_STATE];
    world.read(&[above(pos, 1)], &mut block);
    !states.funnels.contains_key(&block[0]) && !states.replaceable.contains(&block[0])
}

fn above(pos: Pos, dist: i32) -> Pos {
    Pos {
        y: pos.y + dist,
        ..pos
    }
}

fn center(pos: Pos) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    ]
}

fn sounding(world: World<'_>, pos: Pos, belt: bool, speed: f32) {
    if let Some(&effects) = EFFECTS.get() {
        let (impact, volume, rate, item_volume, item_rate) = if belt {
            ("minecraft:block.wool.hit", 0.75, 1.0, 0.15, 0.75)
        } else {
            let pitch = 0.75 + speed.abs() / 1024.0;
            ("minecraft:block.anvil.land", 0.0625, pitch, 0.25, pitch)
        };
        effects.play_sound(world, center(pos), impact, SOUND_BLOCKS, volume, rate);
        effects.play_sound(
            world,
            center(pos),
            "minecraft:entity.item.break",
            SOUND_BLOCKS,
            item_volume,
            item_rate,
        );
    }
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    let positions = kinetics.positions(|kind| matches!(kind, Kind::Press(_)));
    if positions.is_empty() {
        return;
    }
    kinetics.recipes = crate::processing_recipes::current();
    let Some(recipes) = kinetics.recipes.as_ref().cloned() else {
        return;
    };
    for pos in positions {
        let below = above(pos, -2);
        let is_belt = matches!(kinetics.kind(below), Some(Kind::Belt(_)));
        let processing_space = is_belt || kinetics.depots.contains_key(&below);
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let speed = be.get_speed();
        let Some(press) = &mut be.press else { continue };
        if !press.data.running {
            if speed != 0.0 {
                press.entity_scan_cooldown = press.entity_scan_cooldown.saturating_sub(1);
                if press.entity_scan_cooldown == 0 {
                    press.entity_scan_cooldown = ENTITY_SCAN;
                    if !processing_space
                        && visit_items(world, above(pos, -1), 0.125, |entity| {
                            try_process_in_world(world, &recipes, entity, true)
                        })
                    {
                        press.start(WORLD);
                        be.dirty = true;
                    }
                }
            }
            continue;
        }
        if press.data.ticks == STROKE && speed != 0.0 {
            if press.data.mode == WORLD {
                visit_items(world, above(pos, -1), 0.0, |entity| {
                    press.entity_scan_cooldown = 0;
                    try_process_in_world(world, &recipes, entity, false);
                    true
                });
            }
            sounding(world, pos, is_belt, speed);
        }
        let previous = press.data;
        press.advance(speed);
        be.dirty |= press.data != previous;
    }
}

fn visit_items(
    world: World<'_>,
    pos: Pos,
    inset: f64,
    mut visit: impl FnMut(Entity<'_>) -> bool,
) -> bool {
    let Some(&entities) = ENTITIES.get() else {
        return false;
    };
    let [x, y, z] = [pos.x, pos.y, pos.z].map(f64::from);
    entities
        .entities_in(
            world,
            [x + inset, y + inset, z + inset],
            [x + 1. - inset, y + 1. - inset, z + 1. - inset],
            0,
        )
        .into_iter()
        .any(|id| {
            entities
                .with_entity(world, id, |entity| {
                    entity.flags() & ENTITY_ALIVE != 0
                        && entity
                            .view()
                            .is_some_and(|view| view.kind & ENTITY_ITEM != 0 && view.on_ground)
                        && visit(entity)
                })
                .unwrap_or(false)
        })
}

fn try_process_in_world(
    world: World<'_>,
    recipes: &RecipeBook,
    entity: Entity<'_>,
    simulate: bool,
) -> bool {
    let Some(&stacks) = STACKS.get() else {
        return false;
    };
    let Some(item) = stacks
        .entity_stack(entity)
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
    else {
        return false;
    };
    let Some(recipe) = recipes.find(Process::Pressing, &item) else {
        return false;
    };
    if simulate {
        return true;
    }
    let Some(view) = entity.view() else {
        return false;
    };
    let outputs = recipe.outputs();
    let mut outputs = outputs
        .iter()
        .filter_map(|item| fastnbt::to_bytes(item).ok());
    if count(&item) == 1 {
        let result = outputs.next().unwrap_or_default();
        if !stacks.set_entity_stack(entity, &result) {
            return false;
        }
    } else {
        let Ok(rest) = fastnbt::to_bytes(&with_count(&item, count(&item) - 1)) else {
            return false;
        };
        if !stacks.set_entity_stack(entity, &rest) {
            return false;
        }
    }
    for bytes in outputs {
        let speed = || (f64::from(next_int(1 << 20)) / f64::from(1 << 20) - 0.5) * 0.1;
        stacks.spawn(world, view.position, [speed(), speed(), speed()], &bytes);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_stops_at_the_stroke_before_continuing() {
        let mut press = PressingBehaviour::new(PressingData::default());
        press.start(BELT);
        for _ in 0..4 {
            press.advance(256.0);
        }
        assert_eq!(press.data.ticks, STROKE);
        press.advance(0.0);
        assert_eq!(press.data.ticks, STROKE);
        press.advance(256.0);
        assert_eq!(press.data.ticks, 150);
    }
}
