use std::collections::HashMap;

use fastnbt::Value;

use moddedmc_api::{
    Command, Menu, MenuButton, Player, RegistrationError, Registry,
    items::{Item, ItemDefaults, Items},
};

use crate::{
    appearance::{GENDER_WAND, VARIANT_BRUSH},
    armor, blocks, consumables, crucible, discs, roster,
    taming::{BRUSH, CREATIVE_MEAL, GOLDEN_BRUSH, PLUCKER},
    venom_arrow, worldroot,
};

pub const OPEN: u64 = 3;
pub const MOOP_BUCKET: &str = "saintsdragons:bucket_of_moop";
const ITEM_ACTION: &str = "saintsdragons:item/";

const GOODS: &[Item<'static>] = &[
    Item {
        id: venom_arrow::ID,
        display_name: "Arrow of Venom",
        carrier: "minecraft:arrow",
        model: "minecraft:item/arrow",
    },
    Item {
        id: MOOP_BUCKET,
        display_name: "Bucket of Moop",
        carrier: "minecraft:cod_bucket",
        model: "minecraft:item/cod_bucket",
    },
    Item {
        id: BRUSH,
        display_name: "Dragon Brush",
        carrier: "minecraft:enchanted_book",
        model: "minecraft:item/brush",
    },
    Item {
        id: GOLDEN_BRUSH,
        display_name: "Golden Dragon Brush",
        carrier: "minecraft:enchanted_book",
        model: "minecraft:item/brush",
    },
    Item {
        id: PLUCKER,
        display_name: "Scale Plucker",
        carrier: "minecraft:enchanted_book",
        model: "minecraft:item/shears",
    },
    Item {
        id: CREATIVE_MEAL,
        display_name: "Creative Dragon Taming Meal",
        carrier: "minecraft:enchanted_book",
        model: "minecraft:item/golden_carrot",
    },
    Item {
        id: GENDER_WAND,
        display_name: "Creative Dragon Gender Wand",
        carrier: "minecraft:enchanted_book",
        model: "minecraft:item/blaze_rod",
    },
    Item {
        id: VARIANT_BRUSH,
        display_name: "Creative Dragon Variant Brush",
        carrier: "minecraft:enchanted_book",
        model: "minecraft:item/brush",
    },
    Item {
        id: "saintsdragons:ignivorus_heart",
        display_name: "Ignivorus Heart",
        carrier: "minecraft:paper",
        model: "minecraft:item/magma_cream",
    },
    Item {
        id: "saintsdragons:ignivorus_tooth",
        display_name: "Ignivorus Tooth",
        carrier: "minecraft:paper",
        model: "minecraft:item/ghast_tear",
    },
    Item {
        id: "saintsdragons:ignivorus_wing_hide",
        display_name: "Ignivorus Wing Hide",
        carrier: "minecraft:paper",
        model: "minecraft:item/leather",
    },
    Item {
        id: "saintsdragons:raevyx_wing_hide",
        display_name: "Raevyx Wing Hide",
        carrier: "minecraft:paper",
        model: "minecraft:item/phantom_membrane",
    },
    Item {
        id: "saintsdragons:raevyx_wingtalon",
        display_name: "Raevyx Wingtalon",
        carrier: "minecraft:paper",
        model: "minecraft:item/flint",
    },
    Item {
        id: "saintsdragons:raevyx_scale",
        display_name: "Raevyx Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:cindervane_scale",
        display_name: "Cindervane Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:stegonaut_scale",
        display_name: "Stegonaut Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:varasuchus_scale",
        display_name: "Varasuchus Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:ignivorus_scale",
        display_name: "Ignivorus Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:volitans_scale",
        display_name: "Volitans Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:volitans_spine",
        display_name: "Volitans Spine",
        carrier: "minecraft:paper",
        model: "minecraft:item/bone",
    },
    Item {
        id: "saintsdragons:atroxiia_scale",
        display_name: "Atroxiia Scale",
        carrier: "minecraft:paper",
        model: "minecraft:item/armadillo_scute",
    },
    Item {
        id: "saintsdragons:draconian_flesh",
        display_name: "Draconian Flesh",
        carrier: "minecraft:paper",
        model: "minecraft:item/rotten_flesh",
    },
    Item {
        id: "saintsdragons:searing_coal",
        display_name: "Searing Coal",
        carrier: "minecraft:paper",
        model: "minecraft:item/charcoal",
    },
    Item {
        id: "saintsdragons:ancient_dragonite_fragment",
        display_name: "Ancient Dragonite Fragment",
        carrier: "minecraft:paper",
        model: "minecraft:item/amethyst_shard",
    },
    Item {
        id: "saintsdragons:dragon_seal_stone",
        display_name: "Dragon Seal Stone",
        carrier: "minecraft:paper",
        model: "minecraft:item/prismarine_shard",
    },
    Item {
        id: "saintsdragons:dragon_binder_core",
        display_name: "Dragon Binder Core",
        carrier: "minecraft:paper",
        model: "minecraft:item/echo_shard",
    },
    Item {
        id: "saintsdragons:raw_worldroot",
        display_name: "Raw Worldroot",
        carrier: "minecraft:paper",
        model: "minecraft:item/raw_copper",
    },
    Item {
        id: "saintsdragons:worldroot_ingot",
        display_name: "Worldroot Ingot",
        carrier: "minecraft:paper",
        model: "minecraft:item/copper_ingot",
    },
    Item {
        id: "saintsdragons:dragonheart_chunk",
        display_name: "Dragonheart Chunk",
        carrier: "minecraft:paper",
        model: "minecraft:item/quartz",
    },
    Item {
        id: "saintsdragons:dragonheart_alloy",
        display_name: "Dragonheart Alloy",
        carrier: "minecraft:paper",
        model: "minecraft:item/netherite_ingot",
    },
];

pub fn register(
    items: Items,
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for item in GOODS {
        let original_model;
        let model = if cfg!(feature = "original-assets") {
            let name = item.id.trim_start_matches("saintsdragons:");
            original_model = format!("saintsdragons:item/{name}");
            &original_model
        } else {
            item.model
        };
        let item = Item { model, ..*item };
        let max_damage = match item.id {
            BRUSH | GOLDEN_BRUSH => Some(256),
            PLUCKER => Some(64),
            _ => None,
        };
        let mut components = HashMap::new();
        if let Some(max_damage) = max_damage {
            components.insert("minecraft:max_damage".to_owned(), Value::Int(max_damage));
            components.insert("minecraft:damage".to_owned(), Value::Int(0));
        } else if matches!(item.id, CREATIVE_MEAL | GENDER_WAND | VARIANT_BRUSH) {
            components.insert("minecraft:max_stack_size".to_owned(), Value::Int(1));
        }
        if !components.is_empty() {
            let stack = fastnbt::to_bytes(&HashMap::from([
                ("id", Value::String(item.carrier.to_owned())),
                ("components", Value::Compound(components)),
            ]))
            .map_err(|_| RegistrationError::InvalidDefinition)?;
            defaults.add_item(registry, &item, &stack)?;
        } else {
            items.add_item(registry, &item)?;
        }
    }
    for (name, display) in roster::egg_names() {
        items.add_item(
            registry,
            &Item {
                id: &format!("saintsdragons:{name}_spawn_egg"),
                display_name: &format!("{display} Spawn Egg"),
                carrier: "minecraft:paper",
                model: match name {
                    "latcher" | "winged" | "whettled" | roster::SWARM_EGG => {
                        "minecraft:item/skeleton_spawn_egg"
                    }
                    crate::ivy::EGG => "minecraft:item/villager_spawn_egg",
                    _ => "minecraft:item/chicken_spawn_egg",
                },
            },
        )?;
    }
    Ok(())
}

fn button(path: &str, label: String) -> MenuButton {
    MenuButton {
        label,
        action: format!("{ITEM_ACTION}{path}"),
        tooltip: format!("/give @s saintsdragons:{path}"),
    }
}

pub fn open(command: Command<'_>) -> i32 {
    let Some(player) = command.player.as_ref() else {
        command.reply("Run /saintsdragons items in game to open the item menu.");
        return 0;
    };
    let buttons = GOODS
        .iter()
        .filter_map(|item| {
            Some(button(
                item.id.strip_prefix("saintsdragons:")?,
                item.display_name.to_owned(),
            ))
        })
        .chain(crate::binders::names().map(|(name, display)| button(name, display)))
        .chain(roster::egg_names().map(|(name, display)| {
            button(&format!("{name}_spawn_egg"), format!("{display} Spawn Egg"))
        }))
        .chain(
            blocks::names()
                .chain([
                    ("draconic_crucible", crucible::NAME),
                    ("draconian_nucleus", crate::nucleus::BLOCK_NAME),
                    ("draconian_controller", crate::nucleus::CONTROLLER_NAME),
                    ("draconic_codex", crate::codex::NAME),
                    ("ivy_octopus_plushie", crate::ivy::PLUSHIE_NAME),
                ])
                .chain(worldroot::names())
                .chain(armor::names())
                .chain(consumables::names())
                .chain(discs::names())
                .chain(crate::eggs::names())
                .map(|(name, display)| button(name, display.to_owned())),
        )
        .collect();
    player.show_menu(&Menu {
        title: "Saint's Dragons Items".to_owned(),
        body: "Select an item to see its /give command.".to_owned(),
        buttons,
    });
    1
}

pub fn click(player: &Player<'_>, action: &str) {
    let Some(path) = action.strip_prefix(ITEM_ACTION) else {
        return;
    };
    let registered = GOODS
        .iter()
        .any(|item| item.id.strip_prefix("saintsdragons:") == Some(path))
        || matches!(
            path,
            "draconic_crucible"
                | "draconian_nucleus"
                | "draconian_controller"
                | "draconic_codex"
                | "ivy_octopus_plushie"
        )
        || crate::binders::names().any(|(name, _)| name == path)
        || blocks::names()
            .chain(worldroot::names())
            .chain(armor::names())
            .chain(consumables::names())
            .chain(discs::names())
            .chain(crate::eggs::names())
            .any(|(name, _)| name == path)
        || path
            .strip_suffix("_spawn_egg")
            .is_some_and(|egg| roster::egg_names().any(|(name, _)| name == egg));
    if registered {
        player.send_message(&format!("/give @s saintsdragons:{path}"));
    }
}
