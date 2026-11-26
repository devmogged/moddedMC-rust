//! Legendary Monsters 2.2.4 material port. Passive items are translated from ModItems.java
//! in the official 2026-10-23 NeoForge 1.21.1 release JAR.
//! Restricted assets stay in the user's local original JAR, never in this library.

use std::sync::OnceLock;
mod blocks;
mod equipment;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    items::{self, Item, ItemDefaults, Items},
};

static ITEMS: OnceLock<Items> = OnceLock::new();
static DEFAULTS: OnceLock<ItemDefaults> = OnceLock::new();
static CONTENT: OnceLock<moddedmc_api::content::Content> = OnceLock::new();
static PHYSICS: OnceLock<moddedmc_api::block_physics::BlockPhysics> = OnceLock::new();

struct LegendaryMonsters;

/// Items registered as new Item(new Properties()), plus those with only fireResistant().
/// Netherite ingot carriers provide vanilla fire resistance; the other carriers are plain
/// stack-64 items.
const PASSIVE_MATERIALS: &[(&str, &str, &str)] = &[
    (
        "dinosaur_bone",
        "Dinosaur Bone",
        "minecraft:netherite_ingot",
    ),
    (
        "withered_bone",
        "Withered Bone",
        "minecraft:netherite_ingot",
    ),
    (
        "chorus_crystal",
        "Chorus Crystal",
        "minecraft:amethyst_shard",
    ),
    ("frozen_rune", "Frozen Rune", "minecraft:amethyst_shard"),
    (
        "large_shulker_shell",
        "Large Shulker Shell",
        "minecraft:shulker_shell",
    ),
    (
        "nature_crystal",
        "Nature Crystal",
        "minecraft:amethyst_shard",
    ),
    ("void_gem", "Void Gem", "minecraft:amethyst_shard"),
    (
        "warped_mushroom_cap_part",
        "Warped Mushroom Cap Part",
        "minecraft:netherite_ingot",
    ),
    ("ancient_spike", "Ancient Spike", "minecraft:iron_ingot"),
    (
        "falling_cloud_projectile",
        "Falling Cloud Projectile",
        "minecraft:iron_ingot",
    ),
    (
        "falling_cloud_small_projectile",
        "Falling Cloud Small Projectile",
        "minecraft:iron_ingot",
    ),
    ("air_rune", "Air Rune", "minecraft:netherite_ingot"),
    (
        "knights_sword_blade_half",
        "Knight's Sword Blade Half",
        "minecraft:iron_ingot",
    ),
    ("long_stick_half", "Long Stick Half", "minecraft:stick"),
    (
        "deactivated_knight_summoner",
        "Deactivated Knight Summoner",
        "minecraft:iron_ingot",
    ),
    (
        "deactivated_guard_summoner",
        "Deactivated Guard Summoner",
        "minecraft:iron_ingot",
    ),
    (
        "primal_ice_shard",
        "Primal Ice Shard",
        "minecraft:amethyst_shard",
    ),
    (
        "withered_horn",
        "Withered Horn",
        "minecraft:netherite_ingot",
    ),
    (
        "chiseled_ancient_dripstone_shard",
        "Chiseled Ancient Dripstone Shard",
        "minecraft:iron_ingot",
    ),
    (
        "crystal_of_sandstorm",
        "Crystal of Sandstorm",
        "minecraft:amethyst_shard",
    ),
    ("cloud_rod", "Cloud Rod", "minecraft:stick"),
    (
        "enderitium_ingot",
        "Enderitium Ingot",
        "minecraft:iron_ingot",
    ),
    (
        "enderitium_gem",
        "Enderitium Gem",
        "minecraft:amethyst_shard",
    ),
    ("metal_debris", "Metal Debris", "minecraft:iron_ingot"),
];

// Named-root ItemStack NBT: amethyst shard carrier with epic rarity.
// Both source items are plain Item instances with stack size 64.
const EPIC_DEFAULTS: &[u8] = b"\x0a\x00\x00\x08\x00\x02id\x00\x18minecraft:amethyst_shard\x0a\x00\x0acomponents\x08\x00\x10minecraft:rarity\x00\x04epic\x00\x00";
const EPIC_MATERIALS: &[(&str, &str, &str)] = &[
    ("portal_shard", "Portal Shard", "minecraft:amethyst_shard"),
    ("eye_crystal", "Eye Crystal", "minecraft:amethyst_shard"),
];

impl Mod for LegendaryMonsters {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = ITEMS.set(items::require()?);
        let _ = DEFAULTS.set(items::require_defaults()?);
        let _ = CONTENT.set(moddedmc_api::content::require()?);
        let _ = PHYSICS.set(moddedmc_api::block_physics::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
        for &(id, display_name, carrier) in PASSIVE_MATERIALS {
            items.add_item(
                registry,
                &Item {
                    id: &format!("legendary_monsters:{id}"),
                    display_name,
                    carrier,
                    model: &format!("legendary_monsters:item/{id}"),
                },
            )?;
        }
        let defaults = DEFAULTS.get().ok_or(RegistrationError::InvalidDefinition)?;
        for &(id, display_name, carrier) in EPIC_MATERIALS {
            defaults.add_item(
                registry,
                &Item {
                    id: &format!("legendary_monsters:{id}"),
                    display_name,
                    carrier,
                    model: &format!("legendary_monsters:item/{id}"),
                },
                EPIC_DEFAULTS,
            )?;
        }
        equipment::register(*defaults, registry)?;
        blocks::register(
            *CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?,
            *PHYSICS.get().ok_or(RegistrationError::InvalidDefinition)?,
            registry,
        )?;
        Ok(())
    }
}

// The .mmc mod id follows the crate; content retains the original namespace.
moddedmc_api::export_mod!(LegendaryMonsters);
