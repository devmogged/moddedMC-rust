//! Create 6.0.10 blaze burner placement, capture, and fuel.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    interaction::{BlockUseResult, UseBlock, UseOnBlock, UseResult},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTENT, DIRECTIONS, EFFECTS, ITEMS, RESOURCES, STACKS, TAGS, nearest_looking_direction,
};

const ID: &str = "create:blaze_burner";
pub const EMPTY: &str = "create:empty_blaze_burner";
const CREATIVE_FUEL: &str = "create:creative_blaze_cake";
const MAX_HEAT_CAPACITY: u32 = 10_000;
const INSERTION_THRESHOLD: u32 = 500;
const FADING_TICKS: u32 = MAX_HEAT_CAPACITY / 80;
const FACINGS: [u8; 4] = [3, 4, 2, 5];

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum Heat {
    None,
    Smouldering,
    Fading,
    Kindled,
    Seething,
}

impl Heat {
    const VALUES: [Self; 5] = [
        Self::Smouldering,
        Self::None,
        Self::Fading,
        Self::Kindled,
        Self::Seething,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Smouldering => "smouldering",
            Self::Fading => "fading",
            Self::Kindled => "kindled",
            Self::Seething => "seething",
        }
    }

    const fn model(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Smouldering => "create:block/blaze_burner/blaze/idle",
            Self::Fading | Self::Kindled => "create:block/blaze_burner/blaze/active",
            Self::Seething => "create:block/blaze_burner/blaze/super_active",
        }
    }

    const fn cycle(self) -> Self {
        match self {
            Self::None | Self::Seething => Self::Smouldering,
            Self::Smouldering | Self::Fading => Self::Kindled,
            Self::Kindled => Self::Seething,
        }
    }
}

fn properties(heat: Heat, facing: u8) -> String {
    format!(
        "blaze={},facing={}",
        heat.name(),
        DIRECTIONS[usize::from(facing)]
    )
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let props = Heat::VALUES.map(|heat| FACINGS.map(|facing| properties(heat, facing)));
    let states: [State<'_>; 20] = std::array::from_fn(|index| {
        let row = index / FACINGS.len();
        let facing = FACINGS[index % FACINGS.len()];
        State {
            properties: &props[row][index % FACINGS.len()],
            physical: "minecraft:copper_block",
            // shortcut: use a vanilla cage model until the pack can resolve NeoForge OBJ geometry.
            model: "minecraft:block/cauldron",
            rotating_model: Heat::VALUES[row].model(),
            x: 0,
            y: 0,
            orientation: crate::motor_orientation(facing),
            shape: &[ShapeBox {
                min: [1, 0, 1],
                max: [15, 14, 15],
            }],
        }
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Blaze Burner",
            entity: ID,
            item_model: "minecraft:item/blaze_spawn_egg",
            states: &states,
        },
        Some(crate::placed_state),
    )?;
    let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (id, display_name, model) in [
        (EMPTY, "Empty Blaze Burner", "minecraft:item/cauldron"),
        ("create:blaze_cake", "Blaze Cake", "create:item/blaze_cake"),
        (
            CREATIVE_FUEL,
            "Creative Blaze Cake",
            "create:item/creative_blaze_cake",
        ),
    ] {
        items.add_item(
            registry,
            &Item {
                id,
                display_name,
                carrier: "minecraft:paper",
                model,
            },
        )?;
    }
    Ok(())
}

type States = HashMap<u32, (Heat, u8)>;
static STATES: std::sync::OnceLock<States> = std::sync::OnceLock::new();

fn states() -> Option<&'static States> {
    let content = CONTENT.get()?;
    Some(STATES.get_or_init(|| {
        Heat::VALUES
            .into_iter()
            .flat_map(|heat| {
                FACINGS.into_iter().filter_map(move |facing| {
                    content
                        .state(&format!("{ID}[{}]", properties(heat, facing)))
                        .map(|state| (state, (heat, facing)))
                })
            })
            .collect()
    }))
}

pub fn placed(args: &Placement<'_>) -> Option<u32> {
    states()?.get(&args.state)?;
    let facing = nearest_looking_direction(args.yaw, 0.0) ^ 1;
    CONTENT
        .get()?
        .state(&format!("{ID}[{}]", properties(Heat::Smouldering, facing)))
}

pub fn heat_at(world: World<'_>, pos: Pos) -> Heat {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[pos], &mut state);
    states()
        .and_then(|states| states.get(&state[0]))
        .map_or(Heat::None, |state| state.0)
}

pub fn is_burner(state: u32) -> bool {
    states().is_some_and(|states| states.contains_key(&state))
}

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
pub struct Burner {
    #[serde(default, rename = "fuelLevel")]
    fuel: u8,
    #[serde(default, rename = "burnTimeRemaining")]
    remaining: u32,
    #[serde(default, rename = "isCreative")]
    creative: bool,
}

const NORMAL: u8 = 1;
const SPECIAL: u8 = 2;

impl Burner {
    fn load(world: World<'_>, pos: Pos) -> Self {
        world
            .entity(pos)
            .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
            .unwrap_or_default()
    }

    fn heat(self) -> Heat {
        match self.fuel {
            SPECIAL => Heat::Seething,
            NORMAL if self.remaining >= FADING_TICKS => Heat::Kindled,
            NORMAL => Heat::Fading,
            _ => Heat::Smouldering,
        }
    }

    fn save(self, world: World<'_>, pos: Pos) -> bool {
        fastnbt::to_bytes(&self).is_ok_and(|bytes| world.set_entity(pos, &bytes))
    }

    fn consume_tick(mut self) -> Self {
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            if self.fuel == SPECIAL {
                self.fuel = NORMAL;
                self.remaining = MAX_HEAT_CAPACITY / 2;
            } else {
                self.fuel = 0;
            }
        }
        self
    }
}

fn set_heat(world: World<'_>, position: Pos, facing: u8, heat: Heat) {
    if let Some(state) = CONTENT
        .get()
        .and_then(|content| content.state(&format!("{ID}[{}]", properties(heat, facing))))
    {
        world.write(&[BlockUpdate {
            position,
            state,
            flags: content::UPDATE_ALL,
        }]);
    }
}

pub fn event(world: World<'_>, burners: &mut HashMap<Pos, Burner>, event: &ContentEvent) {
    let Some(states) = states() else { return };
    if event.reason != content::UNLOADED
        && states
            .get(&event.state)
            .is_some_and(|(heat, _)| *heat != Heat::None)
    {
        burners
            .entry(event.position)
            .or_insert_with(|| Burner::load(world, event.position));
    } else if states.contains_key(&event.previous) {
        burners.remove(&event.position);
    }
}

pub fn tick(world: World<'_>, burners: &mut HashMap<Pos, Burner>) {
    let Some(states) = states() else { return };
    for (&pos, burner) in burners {
        if burner.creative || burner.fuel == 0 {
            continue;
        }
        let mut state = [content::UNLOADED_STATE];
        world.read(&[pos], &mut state);
        let Some(&(current, facing)) = states.get(&state[0]) else {
            continue;
        };
        let next = burner.consume_tick();
        if next.save(world, pos) {
            *burner = next;
            let heat = next.heat();
            if heat != current {
                set_heat(world, pos, facing, heat);
            }
        }
    }
}

#[derive(Deserialize)]
struct FuelMap {
    values: HashMap<String, FuelDuration>,
}

#[derive(Deserialize)]
struct FuelDuration {
    burn_time: u32,
}

type Fuels = HashMap<String, (u8, u32)>;
static FUEL_MAP: Mutex<Option<(u64, Fuels)>> = Mutex::new(None);

fn mapped_fuel(id: &str) -> Option<(u8, u32)> {
    let resources = RESOURCES.get()?;
    let generation = resources.generation();
    let mut cached = FUEL_MAP.lock().unwrap_or_else(PoisonError::into_inner);
    if cached
        .as_ref()
        .is_none_or(|(previous, _)| *previous != generation)
    {
        let mut fuels = HashMap::new();
        for (file, fuel) in [("regular", NORMAL), ("superheated", SPECIAL)] {
            if let Some(data) = resources.read(&format!(
                "create:data_maps/item/{file}_blaze_burner_fuels.json"
            )) && let Ok(map) = serde_json::from_slice::<FuelMap>(&data)
            {
                fuels.extend(
                    map.values
                        .into_iter()
                        .filter(|(_, duration)| duration.burn_time > 0)
                        .map(|(id, duration)| (id, (fuel, duration.burn_time))),
                );
            }
        }
        *cached = Some((generation, fuels));
    }
    cached.as_ref()?.1.get(id).copied()
}

fn fuel(id: &str, bytes: &[u8]) -> Option<(u8, u32, Vec<u8>)> {
    if let Some((fuel, duration)) = mapped_fuel(id) {
        return Some((fuel, duration, Vec::new()));
    }
    let tags = TAGS.get()?;
    if tags.contains("item", "create:blaze_burner_fuel/special", id) {
        return Some((SPECIAL, 3_200, Vec::new()));
    }
    if let Some((duration, remainder)) = STACKS.get()?.fuel(bytes) {
        return Some((NORMAL, duration, remainder));
    }
    tags.contains("item", "create:blaze_burner_fuel/regular", id)
        .then_some((NORMAL, 1_600, Vec::new()))
}

pub fn use_item(
    burners: &mut HashMap<Pos, Burner>,
    context: &mut UseBlock<'_>,
) -> Option<BlockUseResult> {
    let &(heat, facing) = states()?.get(&context.state)?;
    if heat == Heat::None {
        return None;
    }
    let bytes = context.stack.read()?;
    let item: Value = fastnbt::from_bytes(&bytes).ok()?;
    let id = crate::processing_recipes::item_id(&item)?;
    let (world, pos) = (context.world, context.position);
    let burner = burners
        .entry(pos)
        .or_insert_with(|| Burner::load(world, pos));
    let next_heat = if id == CREATIVE_FUEL {
        let next = Burner {
            creative: true,
            ..Burner::default()
        };
        if !next.save(world, pos) {
            return Some(BlockUseResult::Fail);
        }
        *burner = next;
        heat.cycle()
    } else {
        let (kind, mut duration, remainder) = fuel(id, &bytes)?;
        if burner.creative || kind < burner.fuel {
            return None;
        }
        if kind == burner.fuel {
            if burner.remaining <= INSERTION_THRESHOLD {
                duration = duration.saturating_add(burner.remaining);
            } else if kind == NORMAL {
                duration = burner
                    .remaining
                    .saturating_add(duration)
                    .min(MAX_HEAT_CAPACITY.max(burner.remaining));
            } else {
                return None;
            }
        }
        let next = Burner {
            fuel: kind,
            remaining: duration,
            creative: false,
        };
        if !next.save(world, pos) {
            return Some(BlockUseResult::Fail);
        }
        if !context.creative {
            if remainder.is_empty() {
                context.stack.shrink(1);
            } else if context.stack.count() == 1 {
                if !context.stack.write(&remainder) {
                    burner.save(world, pos);
                    return Some(BlockUseResult::Fail);
                }
            } else if context.give(&remainder) {
                context.stack.shrink(1);
            } else {
                burner.save(world, pos);
                return Some(BlockUseResult::Fail);
            }
        }
        *burner = next;
        next.heat()
    };
    set_heat(world, pos, facing, next_heat);
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            center(pos),
            "minecraft:entity.blaze.shoot",
            moddedmc_api::effects::SOUND_BLOCKS,
            0.2,
            0.7,
        );
    }
    Some(BlockUseResult::Success)
}

pub fn capture(context: &mut UseOnBlock<'_>) -> UseResult {
    let Some(data) = context
        .world
        .block_entity(context.position)
        .and_then(|bytes| fastnbt::from_bytes::<HashMap<String, Value>>(&bytes).ok())
    else {
        return UseResult::Pass;
    };
    if !matches!(data.get("id"), Some(Value::String(id)) if id == "minecraft:mob_spawner") {
        return UseResult::Pass;
    }
    let capturable = |data: &Value| {
        let Value::Compound(fields) = data else {
            return false;
        };
        let Some(Value::Compound(entity)) = fields.get("entity") else {
            return false;
        };
        matches!(entity.get("id"), Some(Value::String(id)) if TAGS.get().is_some_and(|tags|
            tags.contains("entity_type", "create:blaze_burner_capturable", id)))
    };
    let allowed = match data.get("SpawnPotentials") {
        Some(Value::List(list)) if !list.is_empty() => list.iter().any(|entry| {
            matches!(entry, Value::Compound(entry) if entry.get("data").is_some_and(capturable))
        }),
        _ => data.get("SpawnData").is_some_and(capturable),
    };
    if !allowed {
        return UseResult::Pass;
    }
    let saved = Value::Compound(HashMap::from([
        ("id".to_owned(), Value::String(ID.to_owned())),
        ("count".to_owned(), Value::Int(1)),
    ]));
    let Ok(bytes) = fastnbt::to_bytes(&saved) else {
        return UseResult::Fail;
    };
    if !context.creative && context.stack.count() == 1 {
        if !context.stack.write(&bytes) {
            return UseResult::Fail;
        }
    } else if !context.give(&bytes) {
        return UseResult::Fail;
    } else if !context.creative {
        context.stack.shrink(1);
    }
    UseResult::Success
}

fn center(pos: Pos) -> [f64; 3] {
    [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spent_special_fuel_leaves_5000_normal_ticks_before_fading() {
        let burner = Burner {
            fuel: SPECIAL,
            remaining: 1,
            creative: false,
        };
        let mut burner = burner.consume_tick();
        assert_eq!(burner.heat(), Heat::Kindled);
        assert_eq!(burner.remaining, 5_000);
        burner.remaining = 125;
        assert_eq!(burner.heat(), Heat::Kindled);
        assert_eq!(burner.consume_tick().heat(), Heat::Fading);
        burner.remaining = 1;
        assert_eq!(burner.consume_tick().heat(), Heat::Smouldering);
    }
}
