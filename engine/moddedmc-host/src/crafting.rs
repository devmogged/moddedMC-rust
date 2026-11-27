//! The engine crafting matcher exposed to mods.

use std::sync::RwLock;

use moddedmc_api::abi::{self, PlayerRef, Sink, Str, WorldRef};
use pumpkin_data::{
    item_stack::ItemStack,
    recipes::{CraftingRecipeTypes, SpecialCraftingRecipe},
};
use pumpkin_inventory::crafter_screen_handler::CrafterScreenHandler;
use pumpkin_inventory::crafting::crafting_screen_handler::CraftingTableScreenHandler;
use pumpkin_inventory::crafting::{
    crafting_inventory::CraftingInventory,
    crafting_screen_handler::{
        crafting_recipe_identity, for_each_matching_crafting_recipe, match_crafting_recipe_with,
    },
    recipe_provider::GenericRecipe,
};
use pumpkin_inventory::{
    crafting::recipe_provider::RecipeProvider, player::player_screen_handler::PlayerScreenHandler,
};
use pumpkin_nbt::Nbt;
use pumpkin_protocol::codec::recipe::OwnedCraftingRecipe;

use crate::{content::world, stacks::read_stack};
use pumpkin_core::block::entities::{BlockEntity, crafter::CrafterBlockEntity};
use pumpkin_inventory::Inventory;

fn recipe_definition(recipe: GenericRecipe<'_>) -> (Option<String>, &'static str) {
    use CraftingRecipeTypes as Recipe;
    use SpecialCraftingRecipe as Special;

    let serializer = match recipe {
        GenericRecipe::Vanilla(recipe) => match recipe {
            Recipe::CraftingShaped { .. } => "minecraft:crafting_shaped",
            Recipe::CraftingShapeless { .. } => "minecraft:crafting_shapeless",
            Recipe::CraftingTransmute { .. } => "minecraft:crafting_transmute",
            Recipe::CraftingDecoratedPot { .. } => "minecraft:crafting_decorated_pot",
            Recipe::CraftingSpecial(special) => match special {
                Special::Dye { .. } => "minecraft:crafting_dye",
                Special::Imbue { .. } => "minecraft:crafting_imbue",
                Special::ShieldDecoration { .. } => "minecraft:crafting_special_shielddecoration",
                Special::BannerDuplicate { .. } => "minecraft:crafting_special_bannerduplicate",
                Special::BookCloning { .. } => "minecraft:crafting_special_bookcloning",
                Special::FireworkStar { .. } => "minecraft:crafting_special_firework_star",
                Special::FireworkStarFade { .. } => "minecraft:crafting_special_firework_star_fade",
                Special::FireworkRocket { .. } => "minecraft:crafting_special_firework_rocket",
                Special::MapExtending { .. } => "minecraft:crafting_special_mapextending",
                Special::RepairItem => "minecraft:crafting_special_repairitem",
            },
        },
        GenericRecipe::Dynamic(OwnedCraftingRecipe::Shaped { recipe_id, .. }) => {
            return (recipe_id.clone(), "minecraft:crafting_shaped");
        }
        GenericRecipe::Dynamic(OwnedCraftingRecipe::Shapeless { recipe_id, .. }) => {
            return (recipe_id.clone(), "minecraft:crafting_shapeless");
        }
    };
    (crafting_recipe_identity(recipe), serializer)
}

/// Decode all untrusted grid stacks before visiting recipes. A malformed
/// ingredient must not cause a partial list of seemingly valid alternatives.
unsafe fn read_grid(width: u8, height: u8, grid: *const Str) -> Option<CraftingInventory> {
    if grid.is_null()
        || !(1..=abi::MAX_CRAFTING_SIDE).contains(&width)
        || !(1..=abi::MAX_CRAFTING_SIDE).contains(&height)
    {
        return None;
    }
    // SAFETY: the caller provides exactly width * height live stack slices.
    let grid =
        unsafe { std::slice::from_raw_parts(grid, usize::from(width) * usize::from(height)) };
    let items = grid
        .iter()
        .map(|&stack| {
            if stack.len == 0 {
                Some(ItemStack::EMPTY.clone())
            } else {
                read_stack(stack)
            }
        })
        .collect::<Option<Vec<_>>>()?;
    Some(CraftingInventory {
        width,
        height,
        items: RwLock::new(items),
    })
}

unsafe extern "C" fn match_recipe(
    raw: WorldRef,
    width: u8,
    height: u8,
    grid: *const Str,
    out: Sink,
) -> bool {
    // SAFETY: the caller borrows a world and width * height initialized slices for this call.
    let (Some(server), Some(input)) = (
        unsafe { world(raw) }.and_then(|world| world.server.upgrade()),
        unsafe { read_grid(width, height, grid) },
    ) else {
        return false;
    };
    match_crafting_recipe_with(
        &input,
        Some(server.recipe_manager.as_ref()),
        |recipe, result| {
            let Some(saved) = result.to_item_stack().to_nbt() else {
                return false;
            };
            let data = Nbt::from(saved).write();
            if data.len() > abi::MAX_STACK_BYTES {
                return false;
            }
            let (id, serializer) = recipe_definition(recipe);
            let emit = |key, bytes: &[u8]| {
                // SAFETY: the sink is synchronous and the bytes are borrowed for each call.
                unsafe { (out.put)(out.ctx, key, Str::new(bytes)) };
            };
            emit(0, &data);
            if let Some(id) = &id {
                emit(1, id.as_bytes());
            }
            emit(2, serializer.as_bytes());
            true
        },
    )
    .unwrap_or(false)
}

unsafe extern "C" fn enumerate_recipes(
    raw: WorldRef,
    width: u8,
    height: u8,
    grid: *const Str,
    limit: u16,
    out: Sink,
) -> u16 {
    if limit == 0 || limit > abi::MAX_CRAFTING_CHOICES {
        return 0;
    }
    // SAFETY: the world and every grid element are live for the call.
    let (Some(server), Some(input)) = (
        unsafe { world(raw) }.and_then(|world| world.server.upgrade()),
        unsafe { read_grid(width, height, grid) },
    ) else {
        return 0;
    };
    emit_choices(&input, server.recipe_manager.as_ref(), limit, out)
}

fn emit_choices(
    input: &CraftingInventory,
    provider: &dyn RecipeProvider,
    limit: u16,
    out: Sink,
) -> u16 {
    let mut emitted: u16 = 0;
    let mut truncated = false;
    let mut invalid = false;
    for_each_matching_crafting_recipe(input, Some(provider), |recipe, result| {
        if emitted == limit {
            truncated = true;
            return false;
        }
        let Some(saved) = result.to_item_stack().to_nbt() else {
            invalid = true;
            return false;
        };
        let data = Nbt::from(saved).write();
        if data.len() > abi::MAX_STACK_BYTES {
            invalid = true;
            return false;
        }
        let (id, serializer) = recipe_definition(recipe);
        let base = u64::from(emitted) * 3;
        // SAFETY: `put` is synchronous and borrows all arguments for this call.
        unsafe { (out.put)(out.ctx, base, Str::new(&data)) };
        if let Some(id) = &id {
            unsafe { (out.put)(out.ctx, base + 1, Str::from(id.as_str())) };
        }
        unsafe { (out.put)(out.ctx, base + 2, Str::from(serializer)) };
        emitted += 1;
        true
    });
    if invalid {
        0
    } else {
        emitted | if truncated { 0x8000 } else { 0 }
    }
}

fn open_crafting_grid(player: &pumpkin_core::entity::player::Player) -> Option<CraftingInventory> {
    let screen = crate::lock(&player.current_screen_handler).clone();
    let screen = crate::lock(&screen);
    let (width, height, grid) =
        if let Some(table) = screen.as_any().downcast_ref::<CraftingTableScreenHandler>() {
            (3, 3, table.crafting_grid())
        } else if let Some(inventory) = screen.as_any().downcast_ref::<PlayerScreenHandler>() {
            (2, 2, inventory.crafting_grid())
        } else {
            return None;
        };
    if grid.len() != width * height {
        return None;
    }
    Some(CraftingInventory {
        width: width as u8,
        height: height as u8,
        items: RwLock::new(grid),
    })
}

unsafe extern "C" fn current_choices(player: PlayerRef, limit: u16, out: Sink) -> u16 {
    if !(1..=abi::MAX_CRAFTING_CHOICES).contains(&limit) {
        return 0;
    }
    // SAFETY: player belongs to the active command callback; no handle escapes.
    let Some(player) = (unsafe { crate::native::player(player) }) else {
        return 0;
    };
    let Some(input) = open_crafting_grid(player) else {
        return 0;
    };
    let world = player.world();
    let Some(server) = world.server.upgrade() else {
        return 0;
    };
    emit_choices(&input, server.recipe_manager.as_ref(), limit, out) | 0x4000
}

unsafe extern "C" fn select_recipe(player: PlayerRef, index: u16) -> bool {
    if index >= abi::MAX_CRAFTING_CHOICES {
        return false;
    }
    // SAFETY: the handle is live for this callback.
    let Some(player) = (unsafe { crate::native::player(player) }) else {
        return false;
    };
    let screen = crate::lock(&player.current_screen_handler).clone();
    let mut screen = crate::lock(&screen);
    let changed = if let Some(table) = screen
        .as_any_mut()
        .downcast_mut::<CraftingTableScreenHandler>()
    {
        table.select_crafting_recipe(usize::from(index))
    } else if let Some(inventory) = screen.as_any_mut().downcast_mut::<PlayerScreenHandler>() {
        inventory.select_crafting_recipe(usize::from(index))
    } else {
        false
    };
    if changed {
        // Send the result produced by the exact selected recipe, without
        // reopening the menu or modifying any player inventory slots.
        screen.send_content_updates();
    }
    changed
}

fn opened_machine(
    player: &pumpkin_core::entity::player::Player,
) -> Option<std::sync::Arc<dyn pumpkin_inventory::crafting::recipes::RecipeInputInventory>> {
    let screen = crate::lock(&player.current_screen_handler).clone();
    let screen = crate::lock(&screen);
    let crafter = screen.as_any().downcast_ref::<CrafterScreenHandler>()?;
    (!crafter.is_spectator).then(|| crafter.container.clone())
}

unsafe extern "C" fn current_machine_choices(player: PlayerRef, limit: u16, out: Sink) -> u16 {
    if !(1..=abi::MAX_CRAFTING_CHOICES).contains(&limit) {
        return 0;
    }
    let Some(player) = (unsafe { crate::native::player(player) }) else {
        return 0;
    };
    let Some(container) = opened_machine(player) else {
        return 0;
    };
    let Some(crafter) = container.as_any().downcast_ref::<CrafterBlockEntity>() else {
        return 0;
    };
    let world = player.world();
    let Some(server) = world.server.upgrade() else {
        return 0;
    };
    let inventory = CraftingInventory::new(3, 3);
    for (slot, ingredient) in crafter.ingredient_snapshot().iter().enumerate() {
        inventory.set_stack(slot, ingredient.clone());
    }
    emit_choices(&inventory, server.recipe_manager.as_ref(), limit, out) | 0x4000
}

unsafe extern "C" fn select_machine_recipe(player: PlayerRef, index: u16) -> bool {
    if index >= abi::MAX_CRAFTING_CHOICES {
        return false;
    }
    let Some(player) = (unsafe { crate::native::player(player) }) else {
        return false;
    };
    let Some(container) = opened_machine(player) else {
        return false;
    };
    let Some(crafter) = container.as_any().downcast_ref::<CrafterBlockEntity>() else {
        return false;
    };
    let world = player.world();
    let Some(server) = world.server.upgrade() else {
        return false;
    };
    let machine_position = crafter.get_position();
    if world.get_block(&machine_position) != &pumpkin_data::Block::CRAFTER {
        tracing::warn!(
            "polymorph machine selection rejected: not crafter block at {machine_position:?}"
        );
        return false;
    }
    let Some(live_machine) = world.get_block_entity(&machine_position) else {
        tracing::warn!(
            "polymorph machine selection rejected: block entity missing at {machine_position:?}"
        );
        return false;
    };
    if !std::ptr::eq(
        live_machine.as_any() as *const dyn std::any::Any as *const (),
        crafter as *const CrafterBlockEntity as *const (),
    ) {
        tracing::warn!(
            "polymorph machine selection rejected: stale block entity at {machine_position:?}"
        );
        return false;
    }
    // Keep the current menu fixed throughout validation and application.
    // A disconnect, switch to another Crafter, or menu replacement must not
    // make an old viewer's command mutate an unrelated machine.
    let screen = crate::lock(&player.current_screen_handler).clone();
    let mut screen = crate::lock(&screen);
    let Some(menu) = screen.as_any_mut().downcast_mut::<CrafterScreenHandler>() else {
        tracing::warn!("polymorph machine selection rejected: menu not Crafter");
        return false;
    };
    if menu.is_spectator || !std::sync::Arc::ptr_eq(&menu.container, &container) {
        tracing::warn!("polymorph machine selection rejected: menu container changed or spectator");
        return false;
    }
    let _craft_guard = crafter
        .craft_transaction
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Do not update a detached incarnation after removal/conversion while a
    // stale menu stays open. The position alone is not the machine identity.
    let Some(still_live) = world.get_block_entity(&machine_position) else {
        tracing::warn!("polymorph machine selection rejected: entity removed during selection");
        return false;
    };
    if !std::ptr::eq(
        still_live.as_any() as *const dyn std::any::Any as *const (),
        crafter as *const CrafterBlockEntity as *const (),
    ) {
        tracing::warn!("polymorph machine selection rejected: entity replaced during selection");
        return false;
    }
    let frozen = crafter.ingredient_snapshot();
    let inventory = CraftingInventory::new(3, 3);
    for (slot, ingredient) in frozen.iter().enumerate() {
        inventory.set_stack(slot, ingredient.clone());
    }
    let revision_before = server.recipe_manager.recipe_revision();
    let mut chosen = None;
    let mut ordinal = 0;
    for_each_matching_crafting_recipe(
        &inventory,
        Some(server.recipe_manager.as_ref()),
        |definition, _| {
            if ordinal == usize::from(index) {
                chosen = crafting_recipe_identity(definition);
                return false;
            }
            ordinal += 1;
            ordinal < usize::from(abi::MAX_CRAFTING_CHOICES)
        },
    );
    let current_revision = server.recipe_manager.recipe_revision();
    let input_changed = crafter
        .ingredient_snapshot()
        .iter()
        .zip(frozen.iter())
        .any(|(a, b)| a.item_count != b.item_count || !a.are_items_and_components_equal(b));
    if chosen.is_none() || revision_before != current_revision || input_changed {
        tracing::warn!(
            ?index,
            ordinal,
            ?revision_before,
            ?current_revision,
            input_changed,
            found = chosen.is_some(),
            "polymorph machine selection rejected: recipe no longer matches"
        );
        return false;
    }
    let Some(selected) = chosen else {
        return false;
    };
    crafter.set_selected_recipe(Some(selected.clone()));
    // The same menu is still locked; expose the chosen genuine recipe through
    // its non-interactive preview without dispensing or removing any items.
    menu.refresh_selected_preview(Some(&selected));
    true
}

unsafe extern "C" fn remainder(stack: Str, out: Sink) -> bool {
    let Some(saved) = read_stack(stack)
        .and_then(|stack| stack.crafting_remainder())
        .and_then(|stack| stack.to_nbt())
    else {
        return false;
    };
    let data = Nbt::from(saved).write();
    // SAFETY: the remainder's bytes live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&data)) };
    true
}

pub static CRAFTING: abi::CraftingV1 = abi::CraftingV1 {
    size: size_of::<abi::CraftingV1>(),
    match_recipe,
    remainder,
    enumerate_recipes,
    current_choices,
    select_recipe,
    enable_result_cycle,
    current_machine_choices,
    select_machine_recipe,
};

unsafe extern "C" fn enable_result_cycle() -> bool {
    pumpkin_inventory::crafting::crafting_screen_handler::enable_recipe_result_cycling();
    true
}
