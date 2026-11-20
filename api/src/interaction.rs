//! Using a mod's items on blocks: the use context, the held stack and item cooldowns.

use std::marker::PhantomData;
use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World, entities::Entity,
    guard, require_interface_prefix,
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
    // SAFETY: this is the published table type for moddedmc:interaction v1; appended fields are
    // read only after require_extension checks the size.
    let table = unsafe {
        require_interface_prefix::<abi::InteractionV1>(
            abi::INTERACTION,
            1,
            abi::INTERACTION_V1_BASE_SIZE,
        )
    }?;
    let _ = TABLE.set(table);
    Ok(Interaction(table))
}

/// Interaction functions added after the first ABI 1 release. Require the ones a mod uses in
/// `Mod::start`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InteractionExtension {
    /// Using content blocks, reading and writing the held stack, and giving players items.
    BlockUse,
    /// `Interaction::on_player_will_destroy`.
    PlayerWillDestroy,
    /// `Interaction::on_redstone`.
    Redstone,
}

/// Vanilla `getSignal` (or `getDirectSignal` when `direct`) for one of the mod's blocks.
pub struct RedstoneQuery<'a> {
    pub world: World<'a>,
    pub position: Pos,
    pub state: u32,
    /// Vanilla's direction argument, in order down, up, north, south, west, east.
    pub direction: u8,
    pub direct: bool,
}

pub type RedstoneSignalCallback = fn(&RedstoneQuery<'_>) -> u8;
/// `isSignalSource() && canConnectRedstone(direction)` for a state of the mod's blocks.
pub type RedstoneConnectsCallback = fn(state: u32, direction: u8) -> bool;
static REDSTONE: OnceLock<(RedstoneSignalCallback, RedstoneConnectsCallback)> = OnceLock::new();

unsafe extern "C" fn redstone_signal(
    world: abi::WorldRef,
    position: Pos,
    state: u32,
    direction: u8,
    direct: bool,
) -> u8 {
    guard("redstone signal", 0, || {
        let (Some((signal, _)), Some(world)) = (REDSTONE.get(), World::borrowed(world)) else {
            return 0;
        };
        signal(&RedstoneQuery {
            world,
            position,
            state,
            direction,
            direct,
        })
        .min(15)
    })
}

unsafe extern "C" fn redstone_connects(state: u32, direction: u8) -> bool {
    guard("redstone connects", false, || {
        REDSTONE
            .get()
            .is_some_and(|(_, connects)| connects(state, direction))
    })
}

/// The result of using a content block. `TryEmptyHand` from `useItemOn` runs `useWithoutItem`
/// next for the main hand, as vanilla `TRY_WITH_EMPTY_HAND` does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlockUseResult {
    Pass,
    Success,
    Consume,
    Fail,
    TryEmptyHand,
}

/// Vanilla `useItemOn` (`with_item`) or `useWithoutItem` on one of the mod's blocks, valid only
/// during the callback.
pub struct UseBlock<'a> {
    pub world: World<'a>,
    pub player: Option<Entity<'a>>,
    /// The held stack for `useItemOn`, an empty one for `useWithoutItem`.
    pub stack: Stack<'a>,
    pub position: Pos,
    pub state: u32,
    /// Vanilla directions in order down, up, north, south, west, east.
    pub face: u8,
    pub hit: [f32; 3],
    pub sneaking: bool,
    pub creative: bool,
    pub main_hand: bool,
    pub with_item: bool,
    raw_player: abi::EntityRef,
    api: &'static abi::InteractionV1,
}

pub type UseBlockCallback = fn(&mut UseBlock<'_>) -> BlockUseResult;

/// A player about to break one of the mod's blocks, valid only during the callback.
pub struct PlayerWillDestroy<'a> {
    pub world: World<'a>,
    pub player: Option<Entity<'a>>,
    pub position: Pos,
    pub state: u32,
    pub creative: bool,
}

pub type PlayerWillDestroyCallback = fn(&PlayerWillDestroy<'_>);
static PLAYER_WILL_DESTROY: OnceLock<PlayerWillDestroyCallback> = OnceLock::new();

unsafe extern "C" fn player_will_destroy(
    world: abi::WorldRef,
    player: abi::EntityRef,
    position: Pos,
    state: u32,
    creative: bool,
) {
    guard("player_will_destroy", (), || {
        let (Some(callback), Some(world)) = (PLAYER_WILL_DESTROY.get(), World::borrowed(world))
        else {
            return;
        };
        callback(&PlayerWillDestroy {
            world,
            player: Entity::borrowed(player),
            position,
            state,
            creative,
        });
    });
}
static USE_BLOCK: OnceLock<UseBlockCallback> = OnceLock::new();

unsafe extern "C" fn use_block(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    context: *const abi::UseBlock,
) -> i32 {
    guard("use_block", abi::USE_PASS, || {
        // SAFETY: the host passes a context that is live for this call.
        let (Some(&api), Some(callback), Some(world), Some(context)) = (
            TABLE.get(),
            USE_BLOCK.get(),
            World::borrowed(world),
            unsafe { context.as_ref() },
        ) else {
            return abi::USE_PASS;
        };
        let mut use_block = UseBlock {
            world,
            player: Entity::borrowed(player),
            stack: Stack {
                raw: stack,
                api,
                scope: PhantomData,
            },
            position: context.position,
            state: context.state,
            face: context.face,
            hit: context.hit,
            sneaking: context.sneaking,
            creative: context.creative,
            main_hand: context.main_hand,
            with_item: context.with_item,
            raw_player: player,
            api,
        };
        match callback(&mut use_block) {
            BlockUseResult::Pass => abi::USE_PASS,
            BlockUseResult::Success => abi::USE_SUCCESS,
            BlockUseResult::Consume => abi::USE_CONSUME,
            BlockUseResult::Fail => abi::USE_FAIL,
            BlockUseResult::TryEmptyHand => abi::USE_TRY_EMPTY_HAND,
        }
    })
}

impl UseBlock<'_> {
    /// Vanilla `Inventory.placeItemBackInInventory` with a saved stack; what doesn't fit drops.
    pub fn give(&self, stack: &[u8]) -> bool {
        // SAFETY: the player is live for this callback and the bytes are borrowed for the call.
        unsafe { (self.api.give)(self.raw_player, abi::Str::new(stack)) }
    }
}

impl Interaction {
    pub fn require_extension(
        self,
        extension: InteractionExtension,
    ) -> Result<(), InterfaceUnavailable> {
        let (end, name) = match extension {
            InteractionExtension::BlockUse => (
                std::mem::offset_of!(abi::InteractionV1, give),
                "moddedmc:interaction.block_use",
            ),
            InteractionExtension::PlayerWillDestroy => (
                std::mem::offset_of!(abi::InteractionV1, on_player_will_destroy),
                "moddedmc:interaction.player_will_destroy",
            ),
            InteractionExtension::Redstone => (
                std::mem::offset_of!(abi::InteractionV1, on_redstone),
                "moddedmc:interaction.redstone",
            ),
        };
        if self.0.size >= end + size_of::<usize>() {
            Ok(())
        } else {
            Err(InterfaceUnavailable { name, version: 1 })
        }
    }

    /// Vanilla `useItemOn` and `useWithoutItem` for every content block this mod registers.
    /// Needs `InteractionExtension::BlockUse`.
    pub fn on_use_block(
        self,
        reg: &mut Registry<'_>,
        callback: UseBlockCallback,
    ) -> Result<(), RegistrationError> {
        if USE_BLOCK.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampoline static.
        RegistrationError::from_code(unsafe { (self.0.on_use_block)(reg.reg, use_block) })
    }

    /// Vanilla `Block.playerWillDestroy` for every content block this mod registers: after the
    /// break is accepted, before drops and removal. Needs
    /// `InteractionExtension::PlayerWillDestroy`.
    pub fn on_player_will_destroy(
        self,
        reg: &mut Registry<'_>,
        callback: PlayerWillDestroyCallback,
    ) -> Result<(), RegistrationError> {
        if PLAYER_WILL_DESTROY.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampoline static.
        RegistrationError::from_code(unsafe {
            (self.0.on_player_will_destroy)(reg.reg, player_will_destroy)
        })
    }

    /// Redstone output for every content block this mod registers: `connects` says on which
    /// sides a state is a signal source, `signal` gives its strength there. The host already
    /// notifies neighbours when a block write changes a state. Needs
    /// `InteractionExtension::Redstone`.
    pub fn on_redstone(
        self,
        reg: &mut Registry<'_>,
        signal: RedstoneSignalCallback,
        connects: RedstoneConnectsCallback,
    ) -> Result<(), RegistrationError> {
        if REDSTONE.set((signal, connects)).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampolines static.
        RegistrationError::from_code(unsafe {
            (self.0.on_redstone)(reg.reg, redstone_signal, redstone_connects)
        })
    }

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

    /// The stack as saved NBT, `None` when empty. Needs `InteractionExtension::BlockUse`.
    pub fn read(&self) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the stack is live and the sink collects into the local Vec synchronously.
        unsafe { (self.api.stack_read)(self.raw, sink) }.then_some(data)
    }

    /// Replace the stack with saved NBT; empty clears it. Needs `InteractionExtension::BlockUse`.
    pub fn write(&mut self, stack: &[u8]) -> bool {
        // SAFETY: the stack is live and the bytes are borrowed for the call.
        unsafe { (self.api.stack_write)(self.raw, abi::Str::new(stack)) }
    }
}
