//! Crafting with the world's vanilla and datapack recipes.

use crate::{InterfaceUnavailable, abi, content::World, require_interface};

pub use abi::MAX_CRAFTING_SIDE;

#[derive(Clone, Copy)]
pub struct Crafting(&'static abi::CraftingV1);

pub struct RecipeMatch {
    /// Product as saved stack NBT.
    pub result: Vec<u8>,
    /// Absent when the engine has no id for the built-in recipe.
    pub id: Option<String>,
    pub serializer: String,
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

impl Crafting {
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
