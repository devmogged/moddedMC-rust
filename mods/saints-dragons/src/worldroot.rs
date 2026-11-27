use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    content::World,
    entities::Entity,
    items::{Item, ItemDefaults},
};

struct Tool {
    kind: &'static str,
    path: &'static str,
    name: &'static str,
    damage: f64,
    speed: f64,
    weapon: Option<DragonheartWeapon>,
}

struct DragonheartWeapon {
    durability: i32,
    enchantability: i32,
    repair: &'static str,
    reach: f64,
    rarity: &'static str,
    fire_resistant: bool,
}

// Defaults from 0.9.88 ToolsArmorConfig.
const TOOLS: &[Tool] = &[
    Tool {
        kind: "sword",
        path: "worldroot_sword",
        name: "Worldroot Sword",
        damage: 7.8,
        speed: 1.6,
        weapon: None,
    },
    Tool {
        kind: "pickaxe",
        path: "worldroot_pickaxe",
        name: "Worldroot Pickaxe",
        damage: 5.8,
        speed: 1.2,
        weapon: None,
    },
    Tool {
        kind: "axe",
        path: "worldroot_axe",
        name: "Worldroot Axe",
        damage: 9.8,
        speed: 1.0,
        weapon: None,
    },
    Tool {
        kind: "shovel",
        path: "worldroot_shovel",
        name: "Worldroot Shovel",
        damage: 6.3,
        speed: 1.0,
        weapon: None,
    },
    Tool {
        kind: "hoe",
        path: "worldroot_hoe",
        name: "Worldroot Hoe",
        damage: 0.8,
        speed: 4.0,
        weapon: None,
    },
    Tool {
        kind: "sword",
        path: "blood_tempest_katana",
        name: "Blood Tempest Katana",
        damage: 9.0,
        speed: 3.0,
        weapon: Some(DragonheartWeapon {
            durability: 2600,
            enchantability: 18,
            repair: "#saintsdragons:dragonheart_chunk_repair",
            reach: 5.0,
            rarity: "rare",
            fire_resistant: false,
        }),
    },
    Tool {
        kind: "sword",
        path: "dragonlord_sword",
        name: "Dragonlord Sword",
        damage: 13.0,
        speed: 1.4,
        weapon: Some(DragonheartWeapon {
            durability: 3400,
            enchantability: 20,
            repair: "#saintsdragons:dragonheart_alloy_repair",
            reach: 7.0,
            rarity: "epic",
            fire_resistant: true,
        }),
    },
];

pub fn melee_damage(item: &str, target: &str, damage: f32) -> f32 {
    let Some(kind) = target.strip_prefix("saintsdragons:") else {
        return damage;
    };
    let multiplier = match (item, kind) {
        ("saintsdragons:dragonlord_sword", "ignivorus") => 5.0,
        ("saintsdragons:blood_tempest_katana", "raevyx") => 3.0,
        (
            "saintsdragons:worldroot_sword"
            | "saintsdragons:worldroot_pickaxe"
            | "saintsdragons:worldroot_axe"
            | "saintsdragons:worldroot_shovel"
            | "saintsdragons:worldroot_hoe",
            "cindervane" | "raevyx" | "stegonaut" | "varasuchus" | "ignivorus" | "volitans"
            | "nulljaw" | "atroxiia",
        ) => 1.5,
        _ => return damage,
    };
    damage * multiplier
}

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    TOOLS.iter().map(|tool| (tool.path, tool.name))
}

pub(crate) fn compound<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Compound(HashMap::from_iter(
        fields.map(|(key, value)| (key.to_owned(), value)),
    ))
}

fn modifier(attribute: &str, amount: f64) -> Value {
    compound([
        ("type", Value::String(format!("minecraft:{attribute}"))),
        ("id", Value::String(format!("minecraft:base_{attribute}"))),
        ("amount", Value::Double(amount)),
        ("operation", Value::String("add_value".to_owned())),
        ("slot", Value::String("mainhand".to_owned())),
    ])
}

fn mining_rules(kind: &str) -> Value {
    let rules = if kind == "sword" {
        vec![
            compound([
                ("blocks", Value::String("minecraft:cobweb".to_owned())),
                ("speed", Value::Float(15.0)),
                ("correct_for_drops", Value::Byte(1)),
            ]),
            compound([
                (
                    "blocks",
                    Value::String("#minecraft:sword_efficient".to_owned()),
                ),
                ("speed", Value::Float(1.5)),
            ]),
        ]
    } else {
        vec![
            compound([
                (
                    "blocks",
                    Value::String("#minecraft:incorrect_for_diamond_tool".to_owned()),
                ),
                ("correct_for_drops", Value::Byte(0)),
            ]),
            compound([
                (
                    "blocks",
                    Value::String(format!("#minecraft:mineable/{kind}")),
                ),
                ("speed", Value::Float(8.8)),
                ("correct_for_drops", Value::Byte(1)),
            ]),
        ]
    };
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
    ])
}

pub fn register(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for tool in TOOLS {
        let carrier = format!("minecraft:diamond_{}", tool.kind);
        let model = if cfg!(feature = "original-assets") {
            format!("saintsdragons:item/{}", tool.path)
        } else {
            format!("minecraft:item/diamond_{}", tool.kind)
        };
        let durability = tool
            .weapon
            .as_ref()
            .map_or(1950, |weapon| weapon.durability);
        let enchantability = tool
            .weapon
            .as_ref()
            .map_or(14, |weapon| weapon.enchantability);
        let repair = tool
            .weapon
            .as_ref()
            .map_or("#saintsdragons:worldroot_repair", |weapon| weapon.repair);
        let mut attributes = vec![
            modifier("attack_damage", tool.damage - 1.0),
            modifier("attack_speed", tool.speed - 4.0),
        ];
        let mut components = HashMap::from([
            ("minecraft:max_damage".to_owned(), Value::Int(durability)),
            (
                "minecraft:enchantable".to_owned(),
                compound([("value", Value::Int(enchantability))]),
            ),
            (
                "minecraft:repairable".to_owned(),
                compound([("items", Value::String(repair.to_owned()))]),
            ),
            ("minecraft:tool".to_owned(), mining_rules(tool.kind)),
        ]);
        if let Some(weapon) = &tool.weapon {
            attributes.push(modifier("entity_interaction_range", weapon.reach - 3.0));
            components.insert(
                "minecraft:rarity".into(),
                Value::String(weapon.rarity.into()),
            );
            if weapon.fire_resistant {
                components.insert(
                    "minecraft:damage_resistant".into(),
                    compound([("types", Value::String("#minecraft:is_fire".into()))]),
                );
            }
        }
        components.insert(
            "minecraft:attribute_modifiers".into(),
            Value::List(attributes),
        );
        let nbt = fastnbt::to_bytes(&compound([
            ("id", Value::String(carrier.clone())),
            ("components", Value::Compound(components)),
        ]))
        .map_err(|_| RegistrationError::InvalidDefinition)?;
        defaults.add_item(
            registry,
            &Item {
                id: &format!("saintsdragons:{}", tool.path),
                display_name: tool.name,
                carrier: &carrier,
                model: &model,
            },
            &nbt,
        )?;
    }
    Ok(())
}

pub const DRAGONLORD_SWORD: &str = "saintsdragons:dragonlord_sword";
/// `DragonheartSwordItem.DRAGONLORD_FIRE_ASPECT_SECONDS`.
const FIRE_TICKS: u32 = 8 * 20;

/// `DragonheartSwordItem.hurtEnemy`: the Dragonlord sword sets what it hits on fire.
pub fn weapon_hit(
    _world: World<'_>,
    item: &str,
    _attacker: Entity<'_>,
    target: Entity<'_>,
    _damage_type: &str,
    amount: f32,
) -> f32 {
    if item == DRAGONLORD_SWORD
        && let Some(mobs) = crate::MOBS.get()
    {
        mobs.set_on_fire(target, FIRE_TICKS);
    }
    amount
}
