//! Using a mod's items on blocks: the use context, the held stack and item cooldowns.

use std::marker::PhantomData;
use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World, entities::Entity,
    guard, require_interface,
};

pub use abi::MAX_STACK_DATA;

#[derive(Clone, Copy)]
pub struct Interaction(&'static abi::InteractionV1);

/// Vanilla `InteractionResult` for a use on a block.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UseResult {
    /// Not handled: the server places the item's block, if it has one.
    Pass,
    Success,
    Consume,
    Fail,
}

/// Vanilla `UseOnContext`, valid only during the callback.
pub struct UseOnBlock<'a> {
    pub world: World<'a>,
    /// The player, when the mod also required `moddedmc:entities`.
    pub player: Option<Entity<'a>>,
    pub stack: Stack<'a>,
    pub item: &'a str,
    pub position: Pos,
    /// Vanilla directions in order down, up, north, south, west, east.
    pub face: u8,
    pub hit: [f32; 3],
    pub sneaking: bool,
    pub creative: bool,
    raw_player: abi::EntityRef,
    api: &'static abi::InteractionV1,
}

/// The player's held stack, valid only during the callback.
pub struct Stack<'a> {
    raw: abi::StackRef,
    api: &'static abi::InteractionV1,
    scope: PhantomData<&'a mut ()>,
}

pub type Use = fn(&mut UseOnBlock<'_>) -> UseResult;

static TABLE: OnceLock<&'static abi::InteractionV1> = OnceLock::new();
static USE: OnceLock<Use> = OnceLock::new();

pub fn require() -> Result<Interaction, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:interaction v1.
    let table = unsafe { require_interface::<abi::InteractionV1>(abi::INTERACTION, 1) }?;
    let _ = TABLE.set(table);
    Ok(Interaction(table))
}

impl Interaction {
    /// Vanilla `Item.useOn` for every content item this mod registers.
    pub fn on_use_on_block(
        self,
        reg: &mut Registry<'_>,
        callback: Use,
    ) -> Result<(), RegistrationError> {
        if USE.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: registration is live and the trampoline is a static function.
        RegistrationError::from_code(unsafe { (self.0.on_use_on_block)(reg.reg, use_on_block) })
    }
}

unsafe extern "C" fn use_on_block(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    context: *const abi::UseOnBlock,
) -> i32 {
    guard("use_on_block", abi::USE_PASS, || {
        // SAFETY: the host passes a context that is live for this call.
        let (Some(&api), Some(callback), Some(world), Some(context)) =
            (TABLE.get(), USE.get(), World::borrowed(world), unsafe {
                context.as_ref()
            })
        else {
            return abi::USE_PASS;
        };
        let mut use_on = UseOnBlock {
            world,
            player: Entity::borrowed(player),
            stack: Stack {
                raw: stack,
                api,
                scope: PhantomData,
            },
            // SAFETY: the host's item id is live for this call.
            item: unsafe { context.item.str() },
            position: context.position,
            face: context.face,
            hit: context.hit,
            sneaking: context.sneaking,
            creative: context.creative,
            raw_player: player,
            api,
        };
        match callback(&mut use_on) {
            UseResult::Pass => abi::USE_PASS,
            UseResult::Success => abi::USE_SUCCESS,
            UseResult::Consume => abi::USE_CONSUME,
            UseResult::Fail => abi::USE_FAIL,
        }
    })
}

impl UseOnBlock<'_> {
    /// Vanilla `ItemCooldowns.addCooldown` for the held item.
    pub fn cooldown(&self, ticks: u32) {
        // SAFETY: the player is live for this callback and the id is borrowed for the call.
        unsafe { (self.api.cooldown)(self.raw_player, abi::Str::new(self.item.as_bytes()), ticks) }
    }
}

impl Stack<'_> {
    pub fn count(&self) -> u32 {
        // SAFETY: the stack is live for this callback.
        unsafe { (self.api.stack_count)(self.raw) }
    }

    pub fn shrink(&mut self, count: u32) {
        // SAFETY: the stack is live for this callback.
        unsafe { (self.api.stack_shrink)(self.raw, count) }
    }

    /// The mod data stored on this stack, if any.
    pub fn data(&self) -> Option<Vec<u8>> {
        let mut out = vec![0; MAX_STACK_DATA];
        // SAFETY: the stack is live and `out` has the capacity passed.
        let len = unsafe { (self.api.stack_data)(self.raw, out.as_mut_ptr(), out.len()) };
        (len != usize::MAX && len <= out.len()).then(|| {
            out.truncate(len);
            out
        })
    }

    /// Replace the stack's mod data; empty removes it.
    pub fn set_data(&mut self, data: &[u8]) -> bool {
        // SAFETY: the stack is live and the slice is borrowed for the call.
        unsafe { (self.api.set_stack_data)(self.raw, data.as_ptr(), data.len()) }
    }
}
