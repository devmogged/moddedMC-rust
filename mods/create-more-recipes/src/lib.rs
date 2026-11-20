//! Create More Recipes 1.2.1-fix1 (NeoForge 1.21.1), by iglee42.
//! The snowman cooler state machine is ported from its block and block entity.
//! Basin processing remains gated on the Create processing service (WS-4-02).

pub mod block_spout;
pub mod cooler;

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use cooler::{Cooler, Fuel, Heat};
use fastnbt::Value;
use moddedmc_api::{
    InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    interaction::{self, BlockUseResult, Interaction, InteractionExtension, UseBlock},
    items::{self, Item, Items},
    tags::{self, Tags},
};
use serde::{Deserialize, Serialize};

const COOLER: &str = "cmr:snowman_cooler";
const EMPTY_COOLER: &str = "cmr:empty_snowman_cooler";
const ENTITY: &str = "cmr:blaze_heater";
const FACES: [&str; 4] = ["north", "east", "south", "west"];
const HEATS: [&str; 4] = ["idle", "fading", "cooling", "freezing"];
const FROZEN_CAKE: &str = "cmr:frozen_cake";
const CREATIVE_BLAZE_CAKE: &str = "create:creative_blaze_cake";

static CONTENT: OnceLock<Content> = OnceLock::new();
static ITEMS: OnceLock<Items> = OnceLock::new();
static INTERACTION: OnceLock<Interaction> = OnceLock::new();
static TAGS: OnceLock<Tags> = OnceLock::new();
static STATES: OnceLock<CoolerStates> = OnceLock::new();
static WORLDS: Mutex<Option<HashMap<usize, HashMap<Pos, CoolerEntry>>>> = Mutex::new(None);

struct CreateMoreRecipes;

impl Mod for CreateMoreRecipes {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        let _ = ITEMS.set(items::require()?);
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        let _ = INTERACTION.set(interaction);
        let _ = TAGS.set(tags::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let Some(&content) = CONTENT.get() else {
            return Err(RegistrationError::InvalidDefinition);
        };
        let Some(&items) = ITEMS.get() else {
            return Err(RegistrationError::InvalidDefinition);
        };
        // The two cake items are real content in the source, not stand-ins for the fuel
        // logic. Their recipes still need the addon-specific cooler/basin mechanics.
        for (id, name) in [
            ("cmr:frozen_cake_base", "Frozen Cake Base"),
            (FROZEN_CAKE, "Frozen Cake"),
        ] {
            items.add_item(
                registry,
                &Item {
                    id,
                    display_name: name,
                    // Original Item::new has no food or placement behavior.
                    carrier: "minecraft:paper",
                    model: if id == FROZEN_CAKE {
                        "cmr:item/frozen_cake"
                    } else {
                        "cmr:item/frozen_cake_base"
                    },
                },
            )?;
        }
        let properties: Vec<_> = FACES
            .iter()
            .flat_map(|face| HEATS.map(|heat| format!("facing={face},snowman={heat}")))
            .collect();
        let states: Vec<_> = properties
            .iter()
            .map(|properties| State {
                properties,
                physical: "minecraft:iron_block",
                model: "cmr:block/snowman_cooler/block",
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                // shortcut: stock-client collision uses a simple 14/16-high box;
                // replace with AllShapes.HEATER_BLOCK_SPECIAL_COLLISION_SHAPE
                // when the host can represent the context-dependent shape.
                shape: &[ShapeBox {
                    min: [0, 0, 0],
                    max: [16, 14, 16],
                }],
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id: COOLER,
                display_name: "Snowman Cooler",
                entity: ENTITY,
                item_model: "cmr:item/snowman_cooler",
                states: &states,
            },
            Some(placed),
        )?;
        content.add_block(
            registry,
            &Block {
                id: EMPTY_COOLER,
                display_name: "Empty Snowman Cooler",
                entity: "",
                item_model: "cmr:item/empty_snowman_cooler",
                states: &[State {
                    properties: "",
                    physical: "minecraft:iron_block",
                    model: "cmr:block/snowman_cooler/block",
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )?;
        INTERACTION
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .on_use_block(registry, use_block)?;
        content.on_tick(registry, tick)
    }
}

#[derive(Default)]
struct CoolerStates {
    by_id: HashMap<u32, (usize, Heat)>,
    by_value: [[Option<u32>; 4]; 4],
}

impl CoolerStates {
    fn resolve() -> Self {
        let mut ids = Self::default();
        let Some(content) = CONTENT.get() else {
            return ids;
        };
        for (facing, face) in FACES.iter().enumerate() {
            for (heat_idx, heat) in HEATS.iter().enumerate() {
                let selector = format!("{COOLER}[facing={face},snowman={heat}]");
                if let Some(id) = content.state(&selector) {
                    let level = match heat_idx {
                        0 => Heat::Idle,
                        1 => Heat::Fading,
                        2 => Heat::Cooling,
                        _ => Heat::Freezing,
                    };
                    ids.by_id.insert(id, (facing, level));
                    ids.by_value[facing][heat_idx] = Some(id);
                }
            }
        }
        ids
    }

    fn lookup(&self, facing: usize, heat: Heat) -> Option<u32> {
        self.by_value
            .get(facing)?
            .get(heat as usize)
            .copied()
            .flatten()
    }
}

fn states() -> &'static CoolerStates {
    STATES.get_or_init(CoolerStates::resolve)
}

fn facing_for_yaw(yaw: f32) -> usize {
    // BlockPlaceContext#getHorizontalDirection().getOpposite:
    // yaw 0 (player faces south) places the cooler facing north.
    (((yaw / 90.0) + 0.5).floor() as i32).rem_euclid(4) as usize
}

fn placed(context: Placement<'_>) -> Option<u32> {
    states().lookup(facing_for_yaw(context.yaw), Heat::Idle)
}

#[derive(Clone, Copy)]
struct CoolerEntry {
    facing: usize,
    state_heat: Heat,
    cooler: Cooler,
    goggles: bool,
    train_hat: bool,
}

#[derive(Default, Clone, Copy, Deserialize, Serialize)]
struct CoolerSaved {
    #[serde(default, rename = "fuelLevel", skip_serializing_if = "Option::is_none")]
    fuel_level: Option<i32>,
    #[serde(
        default,
        rename = "burnTimeRemaining",
        skip_serializing_if = "Option::is_none"
    )]
    burn_time_remaining: Option<i32>,
    #[serde(default, rename = "isCreative", skip_serializing_if = "is_false")]
    is_creative: bool,
    #[serde(default, rename = "Goggles", skip_serializing_if = "is_false")]
    goggles: bool,
    #[serde(default, rename = "TrainHat", skip_serializing_if = "is_false")]
    train_hat: bool,
}

fn is_false(value: &bool) -> bool {
    !value
}

impl CoolerSaved {
    fn from_cooler(cooler: Cooler) -> Self {
        Self {
            fuel_level: (!cooler.creative).then_some(cooler.fuel as i32),
            burn_time_remaining: (!cooler.creative)
                .then_some(i32::try_from(cooler.remaining).unwrap_or(i32::MAX)),
            is_creative: cooler.creative,
            goggles: false,
            train_hat: false,
        }
    }

    fn restore(self, heat: Heat) -> Cooler {
        Cooler::restore(
            self.fuel_level.unwrap_or_default(),
            self.burn_time_remaining.unwrap_or_default(),
            self.is_creative,
            heat,
        )
    }
}

impl CoolerEntry {
    fn load(world: World<'_>, pos: Pos, facing: usize, heat: Heat) -> Self {
        let saved = world
            .entity(pos)
            .and_then(|bytes| fastnbt::from_bytes::<CoolerSaved>(&bytes).ok())
            .unwrap_or_default();
        Self {
            facing,
            state_heat: heat,
            cooler: saved.restore(heat),
            goggles: saved.goggles,
            train_hat: saved.train_hat,
        }
    }

    fn save(self, world: World<'_>, pos: Pos) {
        let mut saved = CoolerSaved::from_cooler(self.cooler);
        saved.goggles = self.goggles;
        saved.train_hat = self.train_hat;
        if let Ok(bytes) = fastnbt::to_bytes(&saved) {
            world.set_entity(pos, &bytes);
        }
    }

    fn update_state(&mut self, world: World<'_>, pos: Pos) {
        let wanted = self.cooler.heat();
        if self.state_heat != wanted
            && let Some(id) = states().lookup(self.facing, wanted)
            && world.write(&[BlockUpdate {
                position: pos,
                state: id,
                flags: content::UPDATE_CLIENTS,
            }]) == 1
        {
            self.state_heat = wanted;
        }
    }
}

fn tick(world: World<'_>, _time: u64, events: &[ContentEvent]) {
    let map = states();
    let mut locked = WORLDS.lock().unwrap_or_else(PoisonError::into_inner);
    let worlds = locked.get_or_insert_with(HashMap::new);
    let coolers = worlds.entry(world.id()).or_default();
    for event in events {
        let after = map.by_id.get(&event.state).copied();
        if event.reason == content::UNLOADED || after.is_none() {
            coolers.remove(&event.position);
        } else if let Some((facing, heat)) = after {
            let entry = coolers
                .entry(event.position)
                .or_insert_with(|| CoolerEntry::load(world, event.position, facing, heat));
            entry.facing = facing;
            entry.state_heat = heat;
        }
    }
    for (&pos, entry) in coolers.iter_mut() {
        let before = entry.cooler;
        entry.cooler.tick();
        if before != entry.cooler {
            entry.save(world, pos);
            entry.update_state(world, pos);
        }
    }
}

fn held_id(context: &UseBlock<'_>) -> Option<String> {
    let bytes = context.stack.read()?;
    let value: Value = fastnbt::from_bytes(&bytes).ok()?;
    let Value::Compound(compound) = value else {
        return None;
    };
    let Value::String(id) = compound.get("id")? else {
        return None;
    };
    Some(id.clone())
}

fn use_block(context: &mut UseBlock<'_>) -> BlockUseResult {
    if !context.with_item {
        return BlockUseResult::Pass;
    }
    let Some(&(facing, heat)) = states().by_id.get(&context.state) else {
        return BlockUseResult::Pass;
    };
    let Some(id) = held_id(context) else {
        return BlockUseResult::TryEmptyHand;
    };
    let fuel = if id == CREATIVE_BLAZE_CAKE {
        None
    } else if TAGS
        .get()
        .is_some_and(|tags| tags.contains("item", "cmr:snowman_cooler_fuel/special", &id))
    {
        Some(Fuel::Special)
    } else if TAGS
        .get()
        .is_some_and(|tags| tags.contains("item", "cmr:snowman_cooler_fuel/regular", &id))
    {
        Some(Fuel::Normal)
    } else {
        return BlockUseResult::TryEmptyHand;
    };

    let mut locked = WORLDS.lock().unwrap_or_else(PoisonError::into_inner);
    let worlds = locked.get_or_insert_with(HashMap::new);
    let coolers = worlds.entry(context.world.id()).or_default();
    let entry = coolers
        .entry(context.position)
        .or_insert_with(|| CoolerEntry::load(context.world, context.position, facing, heat));
    let accepted = match fuel {
        None => {
            entry.cooler.apply_creative_fuel();
            true
        }
        Some(fuel) => entry.cooler.refuel(fuel, true, false),
    };
    if !accepted {
        return BlockUseResult::TryEmptyHand;
    }

    if fuel.is_some() && !context.creative {
        context.stack.shrink(1);
        if id == "minecraft:powder_snow_bucket" {
            let mut bucket = std::collections::HashMap::new();
            bucket.insert("id".to_owned(), Value::String("minecraft:bucket".into()));
            bucket.insert("count".to_owned(), Value::Int(1));
            if let Ok(bytes) = fastnbt::to_bytes(&Value::Compound(bucket)) {
                context.give(&bytes);
            }
        }
    }
    entry.save(context.world, context.position);
    entry.update_state(context.world, context.position);
    BlockUseResult::Success
}

moddedmc_api::export_mod!(CreateMoreRecipes);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facing_matches_player_opposite_direction() {
        assert_eq!(facing_for_yaw(0.0), 0);
        assert_eq!(facing_for_yaw(90.0), 1);
        assert_eq!(facing_for_yaw(180.0), 2);
        assert_eq!(facing_for_yaw(-90.0), 3);
        assert_eq!(facing_for_yaw(44.0), 0);
        assert_eq!(facing_for_yaw(45.0), 1);
        assert_eq!(facing_for_yaw(-45.0), 0);
        assert_eq!(facing_for_yaw(-46.0), 3);
        assert_eq!(facing_for_yaw(46.0), 1);
    }

    #[test]
    fn cooler_nbt_uses_original_java_fields() {
        let mut cooler = Cooler::default();
        assert!(cooler.refuel(Fuel::Special, false, false));
        cooler.tick();
        let bytes = fastnbt::to_bytes(&CoolerSaved::from_cooler(cooler)).expect("serialize");
        let value: Value = fastnbt::from_bytes(&bytes).expect("parse");
        let Value::Compound(fields) = value else {
            panic!("expected compound");
        };
        assert_eq!(fields.get("fuelLevel"), Some(&Value::Int(2)));
        assert_eq!(fields.get("burnTimeRemaining"), Some(&Value::Int(3_199)));
        assert!(!fields.contains_key("isCreative"));
        let loaded = fastnbt::from_bytes::<CoolerSaved>(&bytes)
            .expect("parse saved")
            .restore(Heat::Idle);
        assert_eq!(loaded, cooler);
    }

    #[test]
    fn creative_cooler_saves_only_the_original_boolean_field() {
        let mut cooler = Cooler::default();
        cooler.apply_creative_fuel();
        cooler.apply_creative_fuel();
        assert_eq!(cooler.heat(), Heat::Freezing);
        let bytes = fastnbt::to_bytes(&CoolerSaved::from_cooler(cooler)).expect("serialize");
        let Value::Compound(fields) = fastnbt::from_bytes::<Value>(&bytes).expect("parse") else {
            panic!("expected compound");
        };
        assert_eq!(fields.get("isCreative"), Some(&Value::Byte(1)));
        assert!(!fields.contains_key("fuelLevel"));
        assert!(!fields.contains_key("burnTimeRemaining"));
        let restored = fastnbt::from_bytes::<CoolerSaved>(&bytes)
            .expect("parse saved")
            .restore(Heat::Freezing);
        assert_eq!(restored, cooler);
    }

    #[test]
    fn goggles_and_train_hat_nbt_fields_remain_compatible() {
        let mut saved = CoolerSaved::from_cooler(Cooler::default());
        saved.goggles = true;
        saved.train_hat = true;
        let bytes = fastnbt::to_bytes(&saved).expect("serialize");
        let Value::Compound(fields) = fastnbt::from_bytes::<Value>(&bytes).expect("parse") else {
            panic!("expected compound");
        };
        assert_eq!(fields.get("Goggles"), Some(&Value::Byte(1)));
        assert_eq!(fields.get("TrainHat"), Some(&Value::Byte(1)));
        let decoded: CoolerSaved = fastnbt::from_bytes(&bytes).expect("parse saved");
        assert!(decoded.goggles && decoded.train_hat);
    }
}
