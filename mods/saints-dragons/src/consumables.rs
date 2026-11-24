use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    items::{Item, ItemDefaults},
};

use crate::worldroot::compound;

struct Food {
    name: &'static str,
    display: &'static str,
    carrier: &'static str,
    nutrition: i32,
    saturation_modifier: f32,
    max_stack_size: i32,
    effects: &'static [Effect],
}

struct Potion {
    name: &'static str,
    display: &'static str,
    effects: &'static [Effect],
}

struct Effect(&'static str, i32, i32);

// ModItems and ModPotions in the 0.9.88 release.
const FOODS: &[Food] = &[
    Food {
        name: "raw_moop",
        display: "Raw Moop",
        carrier: "minecraft:cod",
        nutrition: 2,
        saturation_modifier: 0.1,
        max_stack_size: 64,
        effects: &[],
    },
    Food {
        name: "cooked_moop",
        display: "Cooked Moop",
        carrier: "minecraft:cooked_cod",
        nutrition: 5,
        saturation_modifier: 0.6,
        max_stack_size: 64,
        effects: &[],
    },
    Food {
        name: "raw_mossback",
        display: "Raw Mossback",
        carrier: "minecraft:rabbit",
        nutrition: 4,
        saturation_modifier: 0.3,
        max_stack_size: 64,
        effects: &[
            Effect("minecraft:poison", 200, 0),
            Effect("minecraft:nausea", 200, 0),
            Effect("minecraft:blindness", 200, 0),
        ],
    },
    Food {
        name: "cooked_mossback",
        display: "Cooked Mossback",
        carrier: "minecraft:cooked_rabbit",
        nutrition: 8,
        saturation_modifier: 0.5,
        max_stack_size: 64,
        effects: &[],
    },
    Food {
        name: "hearty_dragon_meal",
        display: "Hearty Dragon Meal",
        carrier: "minecraft:cooked_chicken",
        nutrition: 10,
        saturation_modifier: 1.2,
        max_stack_size: 16,
        effects: &[],
    },
];

const POTIONS: &[Potion] = &[
    Potion {
        name: "potion_of_tideguard",
        display: "Potion of Tideguard",
        effects: &[
            Effect("minecraft:water_breathing", 1200, 0),
            Effect("minecraft:dolphins_grace", 1200, 0),
        ],
    },
    Potion {
        name: "potion_of_searing",
        display: "Potion of Searing",
        effects: &[
            Effect("minecraft:strength", 9600, 2),
            Effect("minecraft:fire_resistance", 9600, 0),
        ],
    },
];

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    FOODS
        .iter()
        .map(|food| (food.name, food.display))
        .chain(POTIONS.iter().map(|potion| (potion.name, potion.display)))
}

fn effects(values: &[Effect]) -> Value {
    Value::List(
        values
            .iter()
            .map(|&Effect(id, duration, amplifier)| {
                compound([
                    ("id", Value::String(id.into())),
                    ("duration", Value::Int(duration)),
                    ("amplifier", Value::Int(amplifier)),
                    ("ambient", Value::Byte(0)),
                    ("show_particles", Value::Byte(1)),
                    ("show_icon", Value::Byte(1)),
                ])
            })
            .collect(),
    )
}

pub fn register(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for food in FOODS {
        let consumable_effects = if food.effects.is_empty() {
            Vec::new()
        } else {
            vec![compound([
                ("type", Value::String("apply_effects".into())),
                ("probability", Value::Float(1.0)),
                ("effects", effects(food.effects)),
            ])]
        };
        let components = compound([
            ("minecraft:max_stack_size", Value::Int(food.max_stack_size)),
            (
                "minecraft:food",
                compound([
                    ("nutrition", Value::Int(food.nutrition)),
                    (
                        "saturation",
                        Value::Float(food.nutrition as f32 * food.saturation_modifier * 2.0),
                    ),
                ]),
            ),
            (
                "minecraft:consumable",
                compound([
                    ("consume_seconds", Value::Float(1.6)),
                    ("animation", Value::String("eat".into())),
                    ("has_consume_particles", Value::Byte(1)),
                    ("on_consume_effects", Value::List(consumable_effects)),
                ]),
            ),
        ]);
        add(
            defaults,
            registry,
            food.name,
            food.display,
            food.carrier,
            food.carrier,
            components,
        )?;
    }
    for potion in POTIONS {
        let components = compound([
            ("minecraft:max_stack_size", Value::Int(1)),
            (
                "minecraft:consumable",
                compound([
                    ("consume_seconds", Value::Float(1.6)),
                    ("animation", Value::String("drink".into())),
                    (
                        "sound",
                        Value::String("minecraft:entity.generic.drink".into()),
                    ),
                ]),
            ),
            (
                "minecraft:potion_contents",
                compound([("custom_effects", effects(potion.effects))]),
            ),
        ]);
        // FixedPotionItem in 0.9.88 consumes the item without leaving a bottle.
        add(
            defaults,
            registry,
            potion.name,
            potion.display,
            "minecraft:paper",
            "minecraft:potion",
            components,
        )?;
    }
    Ok(())
}

fn add(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
    name: &str,
    display_name: &str,
    carrier: &str,
    fallback: &str,
    components: Value,
) -> Result<(), RegistrationError> {
    let model = if cfg!(feature = "original-assets") {
        format!("saintsdragons:item/{name}")
    } else {
        format!(
            "minecraft:item/{}",
            fallback.trim_start_matches("minecraft:")
        )
    };
    let stack = fastnbt::to_bytes(&compound([
        ("id", Value::String(carrier.into())),
        ("components", components),
    ]))
    .map_err(|_| RegistrationError::InvalidDefinition)?;
    defaults.add_item(
        registry,
        &Item {
            id: &format!("saintsdragons:{name}"),
            display_name,
            carrier,
            model: &model,
        },
        &stack,
    )
}
