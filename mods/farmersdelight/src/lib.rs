//! Farmer's Delight 1.3.4 crops, raw food, ingredients and storage crates.
//! Source: ModBlocks and ModItems in vectorwing/FarmersDelight branch 1.21.

mod crops;
pub mod cutting;
pub mod food_values;

use std::{collections::HashMap, sync::OnceLock};

use fastnbt::Value;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, State},
    items::{self, Item, ItemDefaults, Items},
};

static CONTENT: OnceLock<Content> = OnceLock::new();
static ITEMS: OnceLock<Items> = OnceLock::new();
static ITEM_DEFAULTS: OnceLock<ItemDefaults> = OnceLock::new();

const STORAGE_BLOCKS: [(&str, &str, &str); 7] = [
    ("carrot_crate", "Carrot Crate", "minecraft:oak_planks"),
    ("potato_crate", "Potato Crate", "minecraft:oak_planks"),
    ("beetroot_crate", "Beetroot Crate", "minecraft:oak_planks"),
    ("cabbage_crate", "Cabbage Crate", "minecraft:oak_planks"),
    ("tomato_crate", "Tomato Crate", "minecraft:oak_planks"),
    ("onion_crate", "Onion Crate", "minecraft:oak_planks"),
    ("rice_bag", "Rice Bag", "minecraft:white_wool"),
];

const INGREDIENTS: [(&str, &str); 7] = [
    ("straw", "Straw"),
    ("canvas", "Canvas"),
    ("tree_bark", "Tree Bark"),
    ("rice_panicle", "Rice Panicle"),
    ("cabbage_seeds", "Cabbage Seeds"),
    ("tomato_seeds", "Tomato Seeds"),
    ("rotten_tomato", "Rotten Tomato"),
];

fn register_raw_food(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
    name: &str,
    display_name: &str,
    profile_name: &str,
) -> Result<(), RegistrationError> {
    let profile = food_values::profile(profile_name).ok_or(RegistrationError::InvalidDefinition)?;
    let nutrition = profile
        .nutrition
        .ok_or(RegistrationError::InvalidDefinition)?;
    let modifier = profile
        .saturation_modifier
        .ok_or(RegistrationError::InvalidDefinition)?;
    // The 1.21.1 builder stores saturation as twice nutrition times its modifier.
    let food = Value::Compound(HashMap::from([
        ("nutrition".into(), Value::Int(nutrition)),
        (
            "saturation".into(),
            Value::Float(nutrition as f32 * modifier * 2.0),
        ),
    ]));
    let carrier = "minecraft:paper";
    let stack = Value::Compound(HashMap::from([
        ("id".into(), Value::String(carrier.into())),
        (
            "components".into(),
            Value::Compound(HashMap::from([
                ("minecraft:food".into(), food),
                (
                    "minecraft:consumable".into(),
                    Value::Compound(HashMap::from([
                        ("consume_seconds".into(), Value::Float(1.6)),
                        ("animation".into(), Value::String("eat".into())),
                        ("has_consume_particles".into(), Value::Byte(1)),
                    ])),
                ),
            ])),
        ),
    ]));
    let nbt = fastnbt::to_bytes(&stack).map_err(|_| RegistrationError::InvalidDefinition)?;
    defaults.add_item(
        registry,
        &Item {
            id: &format!("farmersdelight:{name}"),
            display_name,
            carrier,
            model: &format!("farmersdelight:item/{name}"),
        },
        &nbt,
    )
}

struct FarmersDelight;

impl Mod for FarmersDelight {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        let _ = CONTENT.set(content);
        let _ = ITEMS.set(items::require()?);
        let _ = ITEM_DEFAULTS.set(items::require_defaults()?);
        crops::start(content)?;
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let content = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
        for (name, display_name) in INGREDIENTS {
            let id = format!("farmersdelight:{name}");
            let model = format!("farmersdelight:item/{name}");
            items.add_item(
                registry,
                &Item {
                    id: &id,
                    display_name,
                    carrier: "minecraft:iron_ingot",
                    model: &model,
                },
            )?;
        }
        let defaults = *ITEM_DEFAULTS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?;
        for (name, display_name, profile) in [
            ("cabbage", "Cabbage", "CABBAGE"),
            ("tomato", "Tomato", "TOMATO"),
            ("onion", "Onion", "ONION"),
        ] {
            register_raw_food(defaults, registry, name, display_name, profile)?;
        }
        for (name, display_name, physical) in STORAGE_BLOCKS {
            let id = format!("farmersdelight:{name}");
            let model = format!("farmersdelight:block/{name}");
            let item_model = format!("farmersdelight:item/{name}");
            content.add_block(
                registry,
                &Block {
                    id: &id,
                    display_name,
                    entity: "",
                    item_model: &item_model,
                    states: &[State {
                        properties: "",
                        physical,
                        model: &model,
                        rotating_model: "",
                        x: 0,
                        y: 0,
                        orientation: [0.0, 0.0, 0.0, 1.0],
                        shape: &[],
                    }],
                },
                None,
            )?;
        }
        crops::register(*content, registry)
    }
}

moddedmc_api::export_mod!(FarmersDelight);
