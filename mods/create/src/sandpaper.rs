//! SandPaperItem's timed use with a held material or an item at the ray hit.
use crate::{
    ENTITIES, INTERACTION, ITEMS, STACKS,
    belt_inventory::{count, with_count},
    processing_recipes::{self, Process},
};
use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    interaction::{ItemUse, ItemUsePhase, ItemUseResult},
    items::Item,
};
use serde::{Deserialize, Serialize};

const SAND_PAPER: &str = "create:sand_paper";
const RED_SAND_PAPER: &str = "create:red_sand_paper";
const DURABILITY: u16 = 8;
const USE_TICKS: u16 = 32;

#[derive(Deserialize, Serialize)]
struct Polishing {
    item: Value,
}

pub(crate) fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (id, display_name, carrier, model) in [
        (
            SAND_PAPER,
            "Sand Paper",
            "minecraft:paper",
            "create:item/sand_paper",
        ),
        (
            RED_SAND_PAPER,
            "Red Sand Paper",
            "minecraft:paper",
            "create:item/red_sand_paper",
        ),
        (
            "create:rose_quartz",
            "Rose Quartz",
            "minecraft:quartz",
            "create:item/rose_quartz",
        ),
        (
            "create:polished_rose_quartz",
            "Polished Rose Quartz",
            "minecraft:quartz",
            "create:item/polished_rose_quartz",
        ),
        (
            "create:electron_tube",
            "Electron Tube",
            "minecraft:iron_nugget",
            "create:item/electron_tube",
        ),
    ] {
        items.add_item(
            registry,
            &Item {
                id,
                display_name,
                carrier,
                model,
            },
        )?;
    }
    let interaction = INTERACTION
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?;
    for id in [SAND_PAPER, RED_SAND_PAPER] {
        items.item_durability(registry, id, DURABILITY)?;
        interaction.item_use_animation(registry, id, USE_TICKS)?;
    }
    Ok(())
}

pub(crate) fn item_use(context: &mut ItemUse<'_>) -> ItemUseResult {
    if !matches!(context.item, SAND_PAPER | RED_SAND_PAPER) {
        return ItemUseResult::Pass;
    }
    let saved = context
        .stack
        .data()
        .and_then(|bytes| fastnbt::from_bytes::<Polishing>(&bytes).ok());
    match context.phase {
        ItemUsePhase::Start => {
            if saved.is_some() || select(context) {
                ItemUseResult::Start(USE_TICKS)
            } else {
                ItemUseResult::Handled
            }
        }
        ItemUsePhase::Finish | ItemUsePhase::Release => {
            if let Some(saved) = saved {
                complete(context, saved);
            }
            ItemUseResult::Handled
        }
    }
}

fn select(context: &mut ItemUse<'_>) -> bool {
    let Some(recipes) = processing_recipes::current() else {
        return false;
    };
    let select = |bytes: &[u8]| {
        let item = fastnbt::from_bytes::<Value>(bytes).ok()?;
        if count(&item) < 1 || recipes.find(Process::Polishing, &item).is_none() {
            return None;
        }
        let saved = fastnbt::to_bytes(&Polishing {
            item: with_count(&item, 1),
        })
        .ok()?;
        Some((item, saved))
    };

    if let Some((_, saved)) = context.other_hand.read().as_deref().and_then(select) {
        if !context.stack.set_data(&saved) {
            return false;
        }
        context.other_hand.shrink(1);
        return true;
    }

    let (Some(&entities), Some(&stacks), Some(view)) = (
        ENTITIES.get(),
        STACKS.get(),
        context.player.and_then(|player| player.view()),
    ) else {
        return false;
    };
    let world = context.world;
    let min = context.hit.map(|coordinate| coordinate - 1.0);
    let max = context.hit.map(|coordinate| coordinate + 1.0);
    for id in entities.entities_matching(world, min, max, 0, 0) {
        let changed = entities.with_entity(world, id, |entity| {
            let Some(position) = entity.view().map(|view| view.position) else {
                return false;
            };
            let distance = position
                .iter()
                .zip(view.position)
                .map(|(&a, b)| (a - b) * (a - b))
                .sum::<f64>();
            if distance > 9.0 {
                return false;
            }
            let Some((item, saved)) = stacks.entity_stack(entity).as_deref().and_then(select)
            else {
                return false;
            };
            let remaining = if count(&item) == 1 {
                Vec::new()
            } else {
                let Ok(bytes) = fastnbt::to_bytes(&with_count(&item, count(&item) - 1)) else {
                    return false;
                };
                bytes
            };
            if !context.stack.set_data(&saved) {
                return false;
            }
            if !stacks.set_entity_stack(entity, &remaining) {
                context.stack.set_data(&[]);
                return false;
            }
            true
        });
        if changed == Some(true) {
            return true;
        }
    }
    false
}

fn complete(context: &mut ItemUse<'_>, saved: Polishing) {
    let output = (context.phase == ItemUsePhase::Finish)
        .then(|| {
            processing_recipes::current()?
                .find(Process::Polishing, &saved.item)?
                .outputs()
                .into_iter()
                .next()
        })
        .flatten();
    let wear = output.is_some() && !context.creative;
    let Some(original) = context.stack.data() else {
        return;
    };
    let Ok(bytes) = fastnbt::to_bytes(output.as_ref().unwrap_or(&saved.item)) else {
        return;
    };
    if !context.stack.set_data(&[]) {
        return;
    }
    if !context.give(&bytes) {
        context.stack.set_data(&original);
        return;
    }
    if wear && let Some(updated) = crate::stack_after_damage(&context.stack, i32::from(DURABILITY))
    {
        context.stack.write(&updated);
    }
}
