//! The engine crafting matcher exposed to mods.

use std::sync::RwLock;

use moddedmc_api::abi::{self, Sink, Str, WorldRef};
use pumpkin_data::{
    item_stack::ItemStack,
    recipes::{CraftingRecipeTypes, SpecialCraftingRecipe},
};
use pumpkin_inventory::crafting::{
    crafting_inventory::CraftingInventory, crafting_screen_handler::match_crafting_recipe_with,
    recipe_provider::GenericRecipe,
};
use pumpkin_nbt::Nbt;
use pumpkin_protocol::codec::recipe::OwnedCraftingRecipe;

use crate::{content::world, stacks::read_stack};

fn recipe_definition(recipe: GenericRecipe<'_>) -> (Option<&str>, &'static str) {
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
            return (recipe_id.as_deref(), "minecraft:crafting_shaped");
        }
        GenericRecipe::Dynamic(OwnedCraftingRecipe::Shapeless { recipe_id, .. }) => {
            return (recipe_id.as_deref(), "minecraft:crafting_shapeless");
        }
    };
    (None, serializer)
}

unsafe extern "C" fn match_recipe(
    raw: WorldRef,
    width: u8,
    height: u8,
    grid: *const Str,
    out: Sink,
) -> bool {
    if grid.is_null()
        || !(1..=abi::MAX_CRAFTING_SIDE).contains(&width)
        || !(1..=abi::MAX_CRAFTING_SIDE).contains(&height)
    {
        return false;
    }
    // SAFETY: the caller borrows a world and width * height initialized slices for this call.
    let (world, grid) = unsafe {
        (
            world(raw),
            std::slice::from_raw_parts(grid, usize::from(width) * usize::from(height)),
        )
    };
    let Some(server) = world.and_then(|world| world.server.upgrade()) else {
        return false;
    };
    let items = grid
        .iter()
        .map(|&stack| {
            if stack.len == 0 {
                Some(ItemStack::EMPTY.clone())
            } else {
                read_stack(stack)
            }
        })
        .collect::<Option<Vec<_>>>();
    let Some(items) = items else {
        return false;
    };
    let input = CraftingInventory {
        width,
        height,
        items: RwLock::new(items),
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
            if let Some(id) = id {
                emit(1, id.as_bytes());
            }
            emit(2, serializer.as_bytes());
            true
        },
    )
    .unwrap_or(false)
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
};
