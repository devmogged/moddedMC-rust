use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    items::{Item, ItemDefaults},
};

use crate::worldroot::compound;

struct Set {
    ids: [&'static str; 4],
    names: [&'static str; 4],
    carrier: &'static str,
    repair: &'static str,
    durability: i32,
    defense: [f64; 4],
    toughness: f64,
    resistance: [f64; 4],
    health: [f64; 4],
    enchantment: i32,
    epic: bool,
}

// Slot order follows ArmorItem.Type in the 0.9.88 stats below.
const SLOTS: [(&str, &str, i32); 4] = [
    ("helmet", "head", 11),
    ("chestplate", "chest", 16),
    ("leggings", "legs", 15),
    ("boots", "feet", 13),
];

const SETS: &[Set] = &[
    Set {
        ids: [
            "draconian_helmet",
            "draconian_chestplate",
            "draconian_leggings",
            "draconian_boots",
        ],
        names: [
            "Draconian Helmet",
            "Draconian Chestplate",
            "Draconian Leggings",
            "Draconian Boots",
        ],
        carrier: "leather",
        repair: "draconian_flesh",
        durability: 25,
        defense: [3.0, 7.0, 5.0, 3.0],
        toughness: 1.0,
        resistance: [0.0; 4],
        health: [0.0; 4],
        enchantment: 18,
        epic: false,
    },
    Set {
        ids: [
            "blood_tempest_helmet",
            "blood_tempest_chestplate",
            "blood_tempest_leggings",
            "blood_tempest_boots",
        ],
        names: [
            "Blood Tempest Helmet",
            "Blood Tempest Chestplate",
            "Blood Tempest Leggings",
            "Blood Tempest Boots",
        ],
        carrier: "netherite",
        repair: "dragonheart_chunk",
        durability: 45,
        defense: [4.0, 9.0, 7.0, 4.0],
        toughness: 4.0,
        resistance: [0.15; 4],
        health: [0.0; 4],
        enchantment: 18,
        epic: true,
    },
    Set {
        ids: [
            "dragonlord_helmet",
            "dragonlord_chestplate",
            "dragonlord_leggings",
            "dragonlord_boots",
        ],
        names: [
            "Dragonlord Helmet",
            "Dragonlord Chestplate",
            "Dragonlord Leggings",
            "Dragonlord Boots",
        ],
        carrier: "netherite",
        repair: "dragonheart_alloy",
        durability: 55,
        defense: [5.0, 10.0, 8.0, 5.0],
        toughness: 5.0,
        resistance: [0.15, 0.5, 0.25, 0.1],
        health: [0.1, 0.15, 0.15, 0.1],
        enchantment: 20,
        epic: true,
    },
];

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    SETS.iter()
        .flat_map(|set| set.ids.into_iter().zip(set.names))
}

fn attributes(set: &Set, index: usize, slot: &str) -> Value {
    Value::List(
        [
            ("armor", set.defense[index], "add_value"),
            ("armor_toughness", set.toughness, "add_value"),
            ("knockback_resistance", set.resistance[index], "add_value"),
            ("max_health", set.health[index], "add_multiplied_base"),
        ]
        .into_iter()
        .filter(|(_, amount, _)| *amount != 0.0)
        .map(|(name, amount, operation)| {
            compound([
                ("type", Value::String(format!("minecraft:{name}"))),
                (
                    "id",
                    Value::String(format!("saintsdragons:armor/{slot}/{name}")),
                ),
                ("amount", Value::Double(amount)),
                ("operation", Value::String(operation.to_owned())),
                ("slot", Value::String(slot.to_owned())),
            ])
        })
        .collect(),
    )
}

pub fn register(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for set in SETS {
        for (index, (kind, slot, wear)) in SLOTS.into_iter().enumerate() {
            let path = set.ids[index];
            let carrier = format!("minecraft:{}_{kind}", set.carrier);
            let model = if cfg!(feature = "original-assets") {
                format!("saintsdragons:item/{path}")
            } else {
                format!("minecraft:item/{}_{kind}", set.carrier)
            };
            let mut components = HashMap::from([
                ("minecraft:max_damage", Value::Int(wear * set.durability)),
                (
                    "minecraft:enchantable",
                    compound([("value", Value::Int(set.enchantment))]),
                ),
                (
                    "minecraft:repairable",
                    compound([(
                        "items",
                        Value::String(format!("#saintsdragons:{}_repair", set.repair)),
                    )]),
                ),
                (
                    "minecraft:attribute_modifiers",
                    attributes(set, index, slot),
                ),
            ]);
            if set.epic {
                components.insert("minecraft:rarity", Value::String("epic".to_owned()));
            }
            let nbt = fastnbt::to_bytes(&HashMap::from([
                ("id", Value::String(carrier.clone())),
                (
                    "components",
                    Value::Compound(
                        components
                            .into_iter()
                            .map(|(id, value)| (id.to_owned(), value))
                            .collect(),
                    ),
                ),
            ]))
            .map_err(|_| RegistrationError::InvalidDefinition)?;
            defaults.add_item(
                registry,
                &Item {
                    id: &format!("saintsdragons:{path}"),
                    display_name: set.names[index],
                    carrier: &carrier,
                    model: &model,
                },
                &nbt,
            )?;
        }
    }
    Ok(())
}
