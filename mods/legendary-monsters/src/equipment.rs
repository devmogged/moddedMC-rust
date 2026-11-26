//! Vanilla-class Enderitium equipment from ModItems and ModToolTiers 2.2.4.
//! Sword and pickaxe have no bespoke Java use callbacks. Other tools need
//! carrier-aware block interactions before being registered.
use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    items::{Item, ItemDefaults},
};
use std::collections::HashMap;

const DURABILITY: i32 = 2450;
const ENCHANTABILITY: i32 = 15;
const MINING_SPEED: f32 = 9.0;
const TIER_DAMAGE: f64 = 3.5;
// Item IDs are not allocated until registration is committed. Resolve the
// single repair ingredient through a datapack tag once the registry is live.
const INGOT: &str = "#legendary_monsters:enderitium_repair";

fn compound(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Compound(fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}
fn string(s: &str) -> Value {
    Value::String(s.to_owned())
}
fn modifier(attribute: &str, amount: f64) -> Value {
    compound([
        ("type", string(&format!("minecraft:{attribute}"))),
        ("id", string(&format!("minecraft:base_{attribute}"))),
        ("amount", Value::Double(amount)),
        ("operation", string("add_value")),
        ("slot", string("mainhand")),
    ])
}
fn tool_components(kind: &str, damage: f64, speed: f64) -> Value {
    let rules = if kind == "sword" {
        vec![
            compound([
                ("blocks", string("minecraft:cobweb")),
                ("speed", Value::Float(15.0)),
                ("correct_for_drops", Value::Byte(1)),
            ]),
            compound([
                ("blocks", string("#minecraft:sword_efficient")),
                ("speed", Value::Float(1.5)),
            ]),
        ]
    } else {
        // The source tier's needs_dinosaur_bone_tool tag is absent in the release.
        // Its incorrect-block set is therefore empty.
        vec![compound([
            ("blocks", string("#minecraft:mineable/pickaxe")),
            ("speed", Value::Float(MINING_SPEED)),
            ("correct_for_drops", Value::Byte(1)),
        ])]
    };
    compound([
        ("minecraft:max_damage", Value::Int(DURABILITY)),
        (
            "minecraft:enchantable",
            compound([("value", Value::Int(ENCHANTABILITY))]),
        ),
        ("minecraft:repairable", compound([("items", string(INGOT))])),
        (
            "minecraft:tool",
            compound([
                ("rules", Value::List(rules)),
                (
                    "damage_per_block",
                    Value::Int(if kind == "sword" { 2 } else { 1 }),
                ),
                (
                    "can_destroy_blocks_in_creative",
                    Value::Byte(i8::from(kind != "sword")),
                ),
            ]),
        ),
        (
            "minecraft:attribute_modifiers",
            Value::List(vec![
                modifier("attack_damage", TIER_DAMAGE + damage),
                modifier("attack_speed", speed),
            ]),
        ),
    ])
}

fn register_item(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
    path: &str,
    name: &str,
    carrier: &str,
    components: Value,
) -> Result<(), RegistrationError> {
    let bytes = fastnbt::to_bytes(&HashMap::from([
        ("id", string(carrier)),
        ("components", components),
    ]))
    .map_err(|_| RegistrationError::InvalidDefinition)?;
    defaults.add_item(
        registry,
        &Item {
            id: &format!("legendary_monsters:{path}"),
            display_name: name,
            carrier,
            model: &format!("legendary_monsters:item/{path}"),
        },
        &bytes,
    )
}

pub fn register(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    register_item(
        defaults,
        registry,
        "enderitium_sword",
        "Enderitium Sword",
        "minecraft:diamond_sword",
        tool_components("sword", 3.0, -2.4),
    )?;
    register_item(
        defaults,
        registry,
        "enderitium_pickaxe",
        "Enderitium Pickaxe",
        "minecraft:diamond_pickaxe",
        tool_components("pickaxe", 1.0, -2.8),
    )?;
    register_item(
        defaults,
        registry,
        "enderitium_upgrade_smithing_template",
        "Smithing Template",
        "minecraft:netherite_upgrade_smithing_template",
        compound([
            ("minecraft:rarity", string("uncommon")),
            (
                "minecraft:lore",
                Value::List(
                    [
                        "Enderitium Upgrade",
                        " ",
                        "Applies to:",
                        " Diamond Equipment",
                        "Ingredients:",
                        " Enderitium Ingot",
                    ]
                    .into_iter()
                    .map(string)
                    .collect(),
                ),
            ),
        ]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tier_and_tool_properties_are_encoded() {
        let Value::Compound(sword) = tool_components("sword", 3.0, -2.4) else {
            panic!("components")
        };
        let Value::Compound(pick) = tool_components("pickaxe", 1.0, -2.8) else {
            panic!("components")
        };
        assert_eq!(sword["minecraft:max_damage"], Value::Int(2450));
        assert_eq!(
            pick["minecraft:repairable"],
            compound([("items", string(INGOT))])
        );
        let Value::List(attrs) = &sword["minecraft:attribute_modifiers"] else {
            panic!("attributes")
        };
        assert_eq!(attrs[0], modifier("attack_damage", 6.5));
        let Value::Compound(tool) = &pick["minecraft:tool"] else {
            panic!("tool")
        };
        let Value::List(rules) = &tool["rules"] else {
            panic!("rules")
        };
        assert_eq!(
            rules[0],
            compound([
                ("blocks", string("#minecraft:mineable/pickaxe")),
                ("speed", Value::Float(9.0)),
                ("correct_for_drops", Value::Byte(1))
            ])
        );
    }
}
