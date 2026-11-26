//! Crafting with the world's vanilla and datapack recipes.

use crate::{InterfaceUnavailable, Player, abi, content::World, require_interface};

pub use abi::{MAX_CRAFTING_CHOICES, MAX_CRAFTING_SIDE};

#[derive(Clone, Copy)]
pub struct Crafting(&'static abi::CraftingV1);

#[derive(Default)]
pub struct RecipeMatch {
    /// Product as saved stack NBT.
    pub result: Vec<u8>,
    /// Absent when the engine has no id for the built-in recipe.
    pub id: Option<String>,
    pub serializer: String,
}

/// Matches in the same stable engine order as normal crafting, without
/// deduplicating conflicting recipes that happen to share an output.
pub struct RecipeChoices {
    pub matches: Vec<RecipeMatch>,
    /// Additional matching definitions exist beyond the requested bound.
    pub truncated: bool,
}

pub fn require() -> Result<Crafting, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:crafting v1.
    unsafe { require_interface(abi::CRAFTING, 1) }.map(Crafting)
}

unsafe extern "C" fn matched(ctx: *mut std::ffi::c_void, key: u64, value: abi::Str) {
    // SAFETY: ctx is the caller's local RecipeMatch and value is live until this sink returns.
    let (recipe, value) = unsafe { (&mut *ctx.cast::<RecipeMatch>(), value.bytes()) };
    match key {
        0 => recipe.result.extend_from_slice(value),
        1 => recipe.id = std::str::from_utf8(value).ok().map(str::to_owned),
        2 => {
            if let Ok(serializer) = std::str::from_utf8(value) {
                recipe.serializer.push_str(serializer);
            }
        }
        _ => {}
    }
}

unsafe extern "C" fn matched_choice(ctx: *mut std::ffi::c_void, key: u64, value: abi::Str) {
    let Ok(index) = usize::try_from(key / 3) else {
        return;
    };
    if index >= usize::from(MAX_CRAFTING_CHOICES) {
        return;
    }
    // SAFETY: the caller lends its vector for the entire synchronous callback.
    let matches = unsafe { &mut *ctx.cast::<Vec<RecipeMatch>>() };
    if index > matches.len() {
        // A skipped match index is malformed. Do not reserve arbitrary storage.
        return;
    }
    if index == matches.len() {
        matches.push(RecipeMatch::default());
    }
    // SAFETY: the host owns this buffer only until the callback returns.
    let value = unsafe { value.bytes() };
    let choice = &mut matches[index];
    match key % 3 {
        0 => choice.result.extend_from_slice(value),
        1 => choice.id = std::str::from_utf8(value).ok().map(str::to_owned),
        2 => {
            if let Ok(serializer) = std::str::from_utf8(value) {
                choice.serializer.push_str(serializer);
            }
        }
        _ => unreachable!(),
    }
}

fn collect_choices(
    limit: u16,
    needs_menu: bool,
    call: impl FnOnce(abi::Sink) -> u16,
) -> Option<RecipeChoices> {
    let mut matches = Vec::<RecipeMatch>::new();
    let sink = abi::Sink {
        ctx: (&raw mut matches).cast(),
        put: matched_choice,
    };
    let raw = call(sink);
    if needs_menu && raw & 0x4000 == 0 {
        return None;
    }
    let count = usize::from(raw & 0x3fff);
    if count != matches.len()
        || count > usize::from(limit)
        || matches
            .iter()
            .any(|recipe| recipe.result.is_empty() || recipe.serializer.is_empty())
    {
        return None;
    }
    Some(RecipeChoices {
        matches,
        truncated: raw & 0x8000 != 0,
    })
}

impl Crafting {
    /// Enable right-clicking the visible result to rotate among genuinely
    /// matching recipes without closing the stock client's 2x2/3x3 menu.
    /// This is a server-side opt-in; normal left/shift clicks still craft.
    pub fn enable_result_cycle(self) -> bool {
        let required_size = std::mem::offset_of!(abi::CraftingV1, enable_result_cycle)
            + std::mem::size_of::<usize>();
        self.0.size >= required_size && unsafe { (self.0.enable_result_cycle)() }
    }

    /// A host can advertise the original matcher but not yet implement
    /// menu-local choice transactions. Mods needing real Polymorph selection
    /// should fail closed at startup when this appended table tail is absent.
    #[must_use]
    pub fn supports_player_selection(self) -> bool {
        self.0.size >= std::mem::offset_of!(abi::CraftingV1, select_recipe)
            + std::mem::size_of::<usize>()
    }

    /// Match a rectangular grid in row order. Empty byte slices represent empty slots.
    pub fn match_recipe(
        self,
        world: World<'_>,
        width: u8,
        height: u8,
        grid: &[Vec<u8>],
    ) -> Option<RecipeMatch> {
        if !(1..=MAX_CRAFTING_SIDE).contains(&width)
            || !(1..=MAX_CRAFTING_SIDE).contains(&height)
            || grid.len() != usize::from(width) * usize::from(height)
        {
            return None;
        }
        let grid: Vec<_> = grid.iter().map(|stack| abi::Str::new(stack)).collect();
        let mut recipe = RecipeMatch {
            result: Vec::new(),
            id: None,
            serializer: String::new(),
        };
        let sink = abi::Sink {
            ctx: (&raw mut recipe).cast(),
            put: matched,
        };
        // SAFETY: the world and complete grid are borrowed and the sink is synchronous.
        let found =
            unsafe { (self.0.match_recipe)(world.raw(), width, height, grid.as_ptr(), sink) };
        (found && !recipe.result.is_empty() && !recipe.serializer.is_empty()).then_some(recipe)
    }

    /// Enumerate real built-in and datapack recipe conflicts without changing
    /// the recipe selected by an ordinary crafting menu. Consumers must still
    /// use a selected-recipe transaction to make output and ingredient removal
    /// agree; merely displaying a choice does not authorize crafting it.
    /// Returns `None` if the host predates this appended capability.
    pub fn enumerate_recipes(
        self,
        world: World<'_>,
        width: u8,
        height: u8,
        grid: &[Vec<u8>],
        limit: u16,
    ) -> Option<RecipeChoices> {
        if !(1..=MAX_CRAFTING_SIDE).contains(&width)
            || !(1..=MAX_CRAFTING_SIDE).contains(&height)
            || grid.len() != usize::from(width) * usize::from(height)
            || !(1..=MAX_CRAFTING_CHOICES).contains(&limit)
        {
            return None;
        }
        let required_size = std::mem::offset_of!(abi::CraftingV1, enumerate_recipes)
            + std::mem::size_of::<usize>();
        if self.0.size < required_size {
            return None;
        }
        let grid: Vec<_> = grid.iter().map(|stack| abi::Str::new(stack)).collect();
        collect_choices(limit, false, |sink| {
            // SAFETY: the table's size was checked and all borrowed buffers
            // live until the host returns from this synchronous enumeration.
            unsafe {
                (self.0.enumerate_recipes)(world.raw(), width, height, grid.as_ptr(), limit, sink)
            }
        })
    }

    /// Enumerate the recipes currently matching the player's open crafting
    /// table or 2x2 inventory grid, without replacing or closing the screen.
    /// Returns `None` for an older host or a different active menu.
    pub fn current_choices(self, player: Player<'_>, limit: u16) -> Option<RecipeChoices> {
        if !(1..=MAX_CRAFTING_CHOICES).contains(&limit)
            || self.0.size < std::mem::offset_of!(abi::CraftingV1, current_choices)
                + std::mem::size_of::<usize>()
        {
            return None;
        }
        collect_choices(limit, true, |sink| {
            // SAFETY: player and sink are valid during the command callback.
            unsafe { (self.0.current_choices)(player.raw, limit, sink) }
        })
    }

    /// Pin a recipe index to the exact current crafting grid. A later
    /// ingredient change invalidates the choice. Only this player's result
    /// slot is updated; the engine's normal take/quick-move path consumes the
    /// matching ingredients and remainders.
    pub fn select_recipe(self, player: Player<'_>, index: u16) -> bool {
        if index >= MAX_CRAFTING_CHOICES
            || self.0.size < std::mem::offset_of!(abi::CraftingV1, select_recipe)
                + std::mem::size_of::<usize>()
        {
            return false;
        }
        // SAFETY: player is borrowed for the current callback, and the
        // implementation owns the current crafting-screen lock.
        unsafe { (self.0.select_recipe)(player.raw, index) }
    }

    /// The stack returned to the grid when this ingredient is consumed, as saved NBT.
    pub fn remainder(self, stack: &[u8]) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the stack is borrowed and the sink fills the local Vec synchronously.
        unsafe { (self.0.remainder)(abi::Str::new(stack), sink) }.then_some(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_decoder_preserves_order_and_recipe_identity() {
        let mut matches = Vec::<RecipeMatch>::new();
        let sink = abi::Sink {
            ctx: (&raw mut matches).cast(),
            put: matched_choice,
        };
        // SAFETY: the sink borrows a live vector and these static strings.
        unsafe {
            (sink.put)(sink.ctx, 0, abi::Str::from("one"));
            (sink.put)(sink.ctx, 2, abi::Str::from("minecraft:crafting_shaped"));
            (sink.put)(sink.ctx, 3, abi::Str::from("two"));
            (sink.put)(sink.ctx, 4, abi::Str::from("test:other"));
            (sink.put)(sink.ctx, 5, abi::Str::from("minecraft:crafting_shapeless"));
        }
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].result, b"one");
        assert_eq!(matches[0].id, None);
        assert_eq!(matches[1].result, b"two");
        assert_eq!(matches[1].id.as_deref(), Some("test:other"));
        assert_eq!(matches[1].serializer, "minecraft:crafting_shapeless");
    }
}
