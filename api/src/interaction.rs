//! Using a mod's items on blocks: the use context, the held stack and item cooldowns.

use std::marker::PhantomData;
use std::sync::OnceLock;

use crate::{
    InterfacePrefix, InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World,
    entities::Entity, guard,
};

pub use abi::MAX_STACK_DATA;

#[derive(Clone, Copy)]
pub struct PlayerStackData(&'static abi::PlayerStackDataV1);

pub fn require_player_stack_data() -> Result<PlayerStackData, InterfaceUnavailable> {
    // SAFETY: this is the published table for player_stack_data v1.
    unsafe { crate::require_interface(abi::PLAYER_STACK_DATA, 1) }.map(PlayerStackData)
}

impl PlayerStackData {
    /// Update a persistent compass target alongside its state, or clear the target with None.
    pub fn replace_lodestone(
        self,
        player: &crate::Player<'_>,
        item: &str,
        expected: &[u8],
        new: &[u8],
        target: Option<(&str, Pos)>,
    ) -> bool {
        if expected.is_empty() || expected.len() > MAX_STACK_DATA || new.len() > MAX_STACK_DATA {
            return false;
        }
        let (dimension, pos) = target.unwrap_or(("", Pos::default()));
        // SAFETY: player and slices are borrowed during the call.
        unsafe {
            (self.0.compare_lodestone)(
                player.raw,
                crate::s(item),
                abi::Str::new(expected),
                abi::Str::new(new),
                crate::s(dimension),
                pos,
            )
        }
    }

    pub fn contains(self, player: &crate::Player<'_>, item: &str, expected: &[u8]) -> bool {
        self.compare(player, item, expected, None)
    }

    pub fn replace(
        self,
        player: &crate::Player<'_>,
        item: &str,
        expected: &[u8],
        new: &[u8],
    ) -> bool {
        self.compare(player, item, expected, Some(new))
    }

    fn compare(
        self,
        player: &crate::Player<'_>,
        item: &str,
        expected: &[u8],
        new: Option<&[u8]>,
    ) -> bool {
        if expected.is_empty()
            || expected.len() > MAX_STACK_DATA
            || new.is_some_and(|v| v.len() > MAX_STACK_DATA)
        {
            return false;
        }
        // SAFETY: player and slices are borrowed during the call.
        unsafe {
            (self.0.compare)(
                player.raw,
                crate::s(item),
                abi::Str::new(expected),
                abi::Str::new(new.unwrap_or_default()),
                new.is_some(),
            )
        }
    }
}

#[derive(Clone, Copy)]
pub struct Interaction(InterfacePrefix<abi::InteractionV1>);

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
    api: InterfacePrefix<abi::InteractionV1>,
}

/// The player's held stack, valid only during the callback.
pub struct Stack<'a> {
    raw: abi::StackRef,
    api: InterfacePrefix<abi::InteractionV1>,
    scope: PhantomData<&'a mut ()>,
}

pub type Use = fn(&mut UseOnBlock<'_>) -> UseResult;

static TABLE: OnceLock<InterfacePrefix<abi::InteractionV1>> = OnceLock::new();
static USE: OnceLock<Use> = OnceLock::new();
static VANILLA_USE: OnceLock<Use> = OnceLock::new();

pub fn require() -> Result<Interaction, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:interaction v1; appended fields are
    // read only after require_extension checks the size.
    let table =
        unsafe { InterfacePrefix::require(abi::INTERACTION, 1, abi::INTERACTION_V1_BASE_SIZE) }?;
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
    /// `Interaction::on_vanilla_use_on_block`.
    VanillaItemUse,
    /// `UseBlock::place_held_block`.
    PlaceHeldBlock,
    /// Normal item use, including completion and release for timed actions.
    ItemUse,
    /// `Interaction::on_use_mob`.
    MobUse,
    /// `Interaction::on_melee_damage`.
    MeleeDamage,
    ImmediateItemUse,
    PlayerPlaced,
    Exhaustion,
    HeldTracker,
    BrewedPotion,
}

/// Vanilla Item.use, with the held stack and the look reported by this action.
pub struct UseItem<'a> {
    pub world: World<'a>,
    pub player: Option<Entity<'a>>,
    pub stack: Stack<'a>,
    pub item: &'a str,
    pub yaw: f32,
    pub pitch: f32,
}

impl UseItem<'_> {
    /// Read the player's sneaking state during this item callback.
    pub fn sneaking(&self) -> bool {
        let Some(player) = self.player else {
            return false;
        };
        if self.stack.api.size
            < std::mem::offset_of!(abi::InteractionV1, player_sneaking) + size_of::<usize>()
        {
            return false;
        }
        // SAFETY: the checked callback borrows the player for this call.
        unsafe { ((&raw const (*self.stack.api.ptr).player_sneaking).read())(player.raw()) }
    }

    /// Start the cooldown for the held item.
    pub fn cooldown(&self, ticks: u32) {
        if let Some(player) = self.player {
            // SAFETY: both the player and item identifier are live for the call.
            unsafe {
                ((&raw const (*self.stack.api.ptr).cooldown).read())(
                    player.raw(),
                    crate::s(self.item),
                    ticks,
                );
            }
        }
    }
}

/// Called after a player successfully places a block owned by this mod.
pub type PlayerPlaced = fn(World<'_>, Entity<'_>, Pos, u32);
static PLAYER_PLACED: OnceLock<PlayerPlaced> = OnceLock::new();

/// Observe a player taking a brewing stand bottle, with its item id and saved stack NBT.
pub type BrewedPotion = fn(World<'_>, Entity<'_>, &str, &[u8]);
static BREWED_POTION: OnceLock<BrewedPotion> = OnceLock::new();

unsafe extern "C" fn brewed_potion(
    world: abi::WorldRef,
    player: abi::EntityRef,
    item: abi::Str,
    stack: abi::Str,
) {
    guard("brewed potion", (), || {
        if let (Some(callback), Some(world), Some(player)) = (
            BREWED_POTION.get(),
            World::borrowed(world),
            Entity::borrowed(player),
        ) {
            // SAFETY: both slices are borrowed from the host for this callback.
            callback(world, player, unsafe { item.str() }, unsafe {
                stack.bytes()
            });
        }
    });
}

unsafe extern "C" fn player_placed(
    world: abi::WorldRef,
    player: abi::EntityRef,
    position: Pos,
    state: u32,
) {
    guard("player placed block", (), || {
        if let (Some(callback), Some(world), Some(player)) = (
            PLAYER_PLACED.get(),
            World::borrowed(world),
            Entity::borrowed(player),
        ) {
            callback(world, player, position, state);
        }
    });
}

pub type ItemUseCallback = fn(&mut UseItem<'_>) -> UseResult;
static ITEM_USE: OnceLock<ItemUseCallback> = OnceLock::new();

unsafe extern "C" fn use_item(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    context: *const abi::UseItem,
) -> i32 {
    guard("item use", abi::USE_PASS, || {
        // SAFETY: the host borrows the context, player, world and stack for the call.
        let (Some(&api), Some(callback), Some(world), Some(context)) = (
            TABLE.get(),
            ITEM_USE.get(),
            World::borrowed(world),
            unsafe { context.as_ref() },
        ) else {
            return abi::USE_PASS;
        };
        let mut use_item = UseItem {
            world,
            player: Entity::borrowed(player),
            stack: Stack {
                raw: stack,
                api,
                scope: PhantomData,
            },
            // SAFETY: the item ID belongs to the borrowed context.
            item: unsafe { context.item.str() },
            yaw: context.yaw,
            pitch: context.pitch,
        };
        match callback(&mut use_item) {
            UseResult::Pass => abi::USE_PASS,
            UseResult::Success => abi::USE_SUCCESS,
            UseResult::Consume => abi::USE_CONSUME,
            UseResult::Fail => abi::USE_FAIL,
        }
    })
}

/// A player right-clicking a mod mob with one hand, valid only during the callback.
pub struct UseMob<'a> {
    pub world: World<'a>,
    pub player: Option<Entity<'a>>,
    /// The stack in the hand used, possibly empty.
    pub stack: Stack<'a>,
    pub mob: Option<Entity<'a>>,
    pub sneaking: bool,
    pub creative: bool,
    pub main_hand: bool,
    raw_player: abi::EntityRef,
    api: InterfacePrefix<abi::InteractionV1>,
}

/// Vanilla `Mob.mobInteract`; anything but `Pass` ends the click there.
pub type UseMobCallback = fn(&mut UseMob<'_>) -> UseResult;
static USE_MOB: OnceLock<UseMobCallback> = OnceLock::new();

unsafe extern "C" fn use_mob(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    mob: abi::EntityRef,
    context: *const abi::UseMob,
) -> i32 {
    guard("use_mob", abi::USE_PASS, || {
        // SAFETY: the host passes a context that is live for this call.
        let (Some(&api), Some(callback), Some(world), Some(context)) =
            (TABLE.get(), USE_MOB.get(), World::borrowed(world), unsafe {
                context.as_ref()
            })
        else {
            return abi::USE_PASS;
        };
        let mut use_mob = UseMob {
            world,
            player: Entity::borrowed(player),
            stack: Stack {
                raw: stack,
                api,
                scope: PhantomData,
            },
            mob: Entity::borrowed(mob),
            sneaking: context.sneaking,
            creative: context.creative,
            main_hand: context.main_hand,
            raw_player: player,
            api,
        };
        match callback(&mut use_mob) {
            UseResult::Pass => abi::USE_PASS,
            UseResult::Success => abi::USE_SUCCESS,
            UseResult::Consume => abi::USE_CONSUME,
            UseResult::Fail => abi::USE_FAIL,
        }
    })
}

impl UseMob<'_> {
    /// Add a cooldown to the held item, using its registered ID.
    pub fn cooldown(&self, item: &str, ticks: u32) {
        // SAFETY: the player is live and the ID is borrowed for this call.
        unsafe {
            ((&raw const (*self.api.ptr).cooldown).read())(self.raw_player, crate::s(item), ticks)
        }
    }

    /// Vanilla `Inventory.placeItemBackInInventory` with a saved stack; what doesn't fit drops.
    pub fn give(&self, stack: &[u8]) -> bool {
        // SAFETY: the player is live for this callback and the bytes are borrowed for the call.
        unsafe { ((&raw const (*self.api.ptr).give).read())(self.raw_player, abi::Str::new(stack)) }
    }
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
    api: InterfacePrefix<abi::InteractionV1>,
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
    /// Add food exhaustion to the player who clicked the block.
    pub fn add_exhaustion(&self, amount: f32) {
        if self.api.size
            >= std::mem::offset_of!(abi::InteractionV1, add_exhaustion) + size_of::<usize>()
        {
            // SAFETY: the function is present and the player is borrowed for the call.
            unsafe {
                ((&raw const (*self.api.ptr).add_exhaustion).read())(self.raw_player, amount)
            };
        }
    }

    /// Show a status message to this player. Older hosts omit the message.
    pub fn action_bar(&self, text: &str) {
        action_bar(self.api, self.raw_player, text);
    }

    /// Vanilla `Inventory.placeItemBackInInventory` with a saved stack; what doesn't fit drops.
    pub fn give(&self, stack: &[u8]) -> bool {
        // SAFETY: the player is live for this callback and the bytes are borrowed for the call.
        unsafe { ((&raw const (*self.api.ptr).give).read())(self.raw_player, abi::Str::new(stack)) }
    }

    /// Place the held block at the center of a clicked face. The caller spends the item on success.
    pub fn place_held_block(&self, clicked: Pos, face: u8) -> bool {
        // SAFETY: the size check covers the appended function and the player is borrowed.
        self.api.size
            >= std::mem::offset_of!(abi::InteractionV1, place_held_block) + size_of::<usize>()
            && unsafe {
                ((&raw const (*self.api.ptr).place_held_block).read())(
                    self.raw_player,
                    clicked,
                    face,
                )
            }
    }
}

/// The normal item-use lifecycle. Completion and release refer to the original hand.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemUsePhase {
    Start,
    Finish,
    Release,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemUseResult {
    Pass,
    Handled,
    Start(u16),
}

pub struct ItemUse<'a> {
    pub world: World<'a>,
    pub player: Option<Entity<'a>>,
    pub stack: Stack<'a>,
    pub other_hand: Stack<'a>,
    pub item: &'a str,
    pub hit: [f64; 3],
    pub creative: bool,
    pub main_hand: bool,
    pub phase: ItemUsePhase,
    raw_player: abi::EntityRef,
    api: InterfacePrefix<abi::InteractionV1>,
}

impl ItemUse<'_> {
    /// Show a status message to this player. Older hosts omit the message.
    pub fn action_bar(&self, text: &str) {
        action_bar(self.api, self.raw_player, text);
    }

    pub fn give(&self, stack: &[u8]) -> bool {
        // SAFETY: requiring ItemUse covers the earlier give field. The player and bytes are live.
        unsafe { ((&raw const (*self.api.ptr).give).read())(self.raw_player, abi::Str::new(stack)) }
    }
}

fn action_bar(api: InterfacePrefix<abi::InteractionV1>, player: abi::EntityRef, text: &str) {
    if api.size >= std::mem::offset_of!(abi::InteractionV1, action_bar) + size_of::<usize>() {
        // SAFETY: the function is present and both arguments are borrowed for this call.
        unsafe { ((&raw const (*api.ptr).action_bar).read())(player, abi::Str::from(text)) };
    }
}

type TimedItemUseCallback = fn(&mut ItemUse<'_>) -> ItemUseResult;
static TIMED_ITEM_USE: OnceLock<TimedItemUseCallback> = OnceLock::new();

unsafe extern "C" fn item_use(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    other_hand: abi::StackRef,
    context: *const abi::ItemUseContext,
) -> i32 {
    guard("item use", 0, || {
        // SAFETY: the host borrows the context for this callback.
        let (Some(&api), Some(callback), Some(world), Some(context)) = (
            TABLE.get(),
            TIMED_ITEM_USE.get(),
            World::borrowed(world),
            unsafe { context.as_ref() },
        ) else {
            return 0;
        };
        let phase = match context.phase {
            abi::ITEM_USE_START => ItemUsePhase::Start,
            abi::ITEM_USE_FINISH => ItemUsePhase::Finish,
            abi::ITEM_USE_RELEASE => ItemUsePhase::Release,
            _ => return 0,
        };
        let borrow = |raw| Stack {
            raw,
            api,
            scope: PhantomData,
        };
        let result = callback(&mut ItemUse {
            world,
            player: Entity::borrowed(player),
            stack: borrow(stack),
            other_hand: borrow(other_hand),
            // SAFETY: the item name is borrowed for this callback.
            item: unsafe { context.item.str() },
            hit: context.hit,
            creative: context.creative,
            main_hand: context.main_hand,
            phase,
            raw_player: player,
            api,
        });
        match result {
            ItemUseResult::Pass => 0,
            ItemUseResult::Handled => -1,
            ItemUseResult::Start(ticks) => i32::from(ticks),
        }
    })
}

/// Item ID, registered mod mob ID (empty for vanilla), and damage before defenses.
pub type MeleeDamage = fn(&str, &str, f32) -> f32;
static MELEE_DAMAGE: OnceLock<MeleeDamage> = OnceLock::new();

unsafe extern "C" fn melee_damage(item: abi::Str, target_kind: abi::Str, damage: f32) -> f32 {
    guard("melee damage", damage, || {
        MELEE_DAMAGE.get().map_or(damage, |callback| {
            // SAFETY: both strings are borrowed for this callback.
            unsafe { callback(item.str(), target_kind.str(), damage) }
        })
    })
}

impl Interaction {
    /// Modify direct player melee damage from this mod's items, before target defenses.
    /// Needs `InteractionExtension::MeleeDamage`.
    pub fn on_melee_damage(
        self,
        reg: &mut Registry<'_>,
        callback: MeleeDamage,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::MeleeDamage)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        if MELEE_DAMAGE.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampoline static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_melee_damage).read())(reg.reg, melee_damage)
        })
    }

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
            InteractionExtension::VanillaItemUse => (
                std::mem::offset_of!(abi::InteractionV1, on_vanilla_use_on_block),
                "moddedmc:interaction.vanilla_item_use",
            ),
            InteractionExtension::PlaceHeldBlock => (
                std::mem::offset_of!(abi::InteractionV1, place_held_block),
                "moddedmc:interaction.place_held_block",
            ),
            InteractionExtension::ItemUse => (
                std::mem::offset_of!(abi::InteractionV1, item_use_animation),
                "moddedmc:interaction.item_use",
            ),
            InteractionExtension::MeleeDamage => (
                std::mem::offset_of!(abi::InteractionV1, on_melee_damage),
                "moddedmc:interaction.melee_damage",
            ),
            InteractionExtension::PlayerPlaced => (
                std::mem::offset_of!(abi::InteractionV1, on_player_placed),
                "moddedmc:interaction.player_placed",
            ),
            InteractionExtension::Exhaustion => (
                std::mem::offset_of!(abi::InteractionV1, add_exhaustion),
                "moddedmc:interaction.add_exhaustion",
            ),
            InteractionExtension::HeldTracker => (
                std::mem::offset_of!(abi::InteractionV1, stack_set_lodestone),
                "moddedmc:interaction.stack_set_lodestone",
            ),
            InteractionExtension::BrewedPotion => (
                std::mem::offset_of!(abi::InteractionV1, on_brewed_potion),
                "moddedmc:interaction.brewed_potion",
            ),
            InteractionExtension::ImmediateItemUse => (
                std::mem::offset_of!(abi::InteractionV1, on_use_item),
                "moddedmc:interaction.use_item",
            ),
            InteractionExtension::MobUse => (
                std::mem::offset_of!(abi::InteractionV1, on_use_mob),
                "moddedmc:interaction.mob_use",
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
        self.require_extension(InteractionExtension::BlockUse)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        if USE_BLOCK.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampoline static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_use_block).read())(reg.reg, use_block)
        })
    }

    /// Vanilla `Block.playerWillDestroy` for every content block this mod registers: after the
    /// break is accepted, before drops and removal. Needs
    /// `InteractionExtension::PlayerWillDestroy`.
    pub fn on_player_will_destroy(
        self,
        reg: &mut Registry<'_>,
        callback: PlayerWillDestroyCallback,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::PlayerWillDestroy)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        if PLAYER_WILL_DESTROY.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampoline static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_player_will_destroy).read())(reg.reg, player_will_destroy)
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
        self.require_extension(InteractionExtension::Redstone)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        if REDSTONE.set((signal, connects)).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required; registration is live and the trampolines static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_redstone).read())(
                reg.reg,
                redstone_signal,
                redstone_connects,
            )
        })
    }

    /// Right-clicks on mobs of the mod mob type `kind`. A mod has one callback: the first one
    /// passed serves every kind. Needs `InteractionExtension::MobUse`.
    pub fn on_use_mob(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
        callback: UseMobCallback,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::MobUse)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        let _ = USE_MOB.set(callback);
        // SAFETY: the extension was required; registration is live and the trampoline static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_use_mob).read())(reg.reg, crate::s(kind), use_mob)
        })
    }

    /// Handle normal use of this mod's items.
    pub fn on_item_use(
        self,
        reg: &mut Registry<'_>,
        callback: TimedItemUseCallback,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::ItemUse)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        TIMED_ITEM_USE
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the table size was checked and the registrar is live.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_item_use).read())(reg.reg, item_use)
        })
    }

    /// Present a timed hand-use animation to the stock client for a registered item.
    /// The use callback still controls the server's item and the timer.
    pub fn item_use_animation(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        ticks: u16,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::ItemUse)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        // SAFETY: the table size was checked and the registrar and name are live.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).item_use_animation).read())(reg.reg, crate::s(item), ticks)
        })
    }

    /// Vanilla `Item.useOn` for every content item this mod registers.
    /// Observe successful player placement of this mod's registered blocks.
    pub fn on_player_placed(
        self,
        reg: &mut Registry<'_>,
        callback: PlayerPlaced,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::PlayerPlaced)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        let _ = PLAYER_PLACED.set(callback);
        // SAFETY: the table size was checked and the trampoline is static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_player_placed).read())(reg.reg, player_placed)
        })
    }

    /// Register one observer for player takes from brewing stand bottle slots.
    pub fn on_brewed_potion(
        self,
        reg: &mut Registry<'_>,
        callback: BrewedPotion,
    ) -> Result<(), RegistrationError> {
        let table = self
            .0
            .upto(std::mem::offset_of!(abi::InteractionV1, on_brewed_potion))
            .ok_or(RegistrationError::InvalidDefinition)?;
        BREWED_POTION
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the table size was checked and the registration and trampoline are live.
        RegistrationError::from_code(unsafe {
            ((&raw const (*table).on_brewed_potion).read())(reg.reg, brewed_potion)
        })
    }

    pub fn on_use_item(
        self,
        reg: &mut Registry<'_>,
        callback: ItemUseCallback,
    ) -> Result<(), RegistrationError> {
        self.require_extension(InteractionExtension::ImmediateItemUse)
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        ITEM_USE
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the checked field, live registration and static trampoline are valid.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_use_item).read())(reg.reg, use_item)
        })
    }

    pub fn on_use_on_block(
        self,
        reg: &mut Registry<'_>,
        callback: Use,
    ) -> Result<(), RegistrationError> {
        if USE.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: registration is live and the trampoline is a static function.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_use_on_block).read())(reg.reg, use_on_block)
        })
    }

    /// Intercept vanilla `Item.useOn`. Needs `InteractionExtension::VanillaItemUse`.
    pub fn on_vanilla_use_on_block(
        self,
        reg: &mut Registry<'_>,
        callback: Use,
    ) -> Result<(), RegistrationError> {
        if VANILLA_USE.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: the extension was required, registration is live and the trampoline is static.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_vanilla_use_on_block).read())(
                reg.reg,
                vanilla_use_on_block,
            )
        })
    }
}

unsafe extern "C" fn use_on_block(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    context: *const abi::UseOnBlock,
) -> i32 {
    // SAFETY: this trampoline forwards unmodified values from the host.
    unsafe { dispatch_use(USE.get(), world, player, stack, context) }
}

unsafe extern "C" fn vanilla_use_on_block(
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    context: *const abi::UseOnBlock,
) -> i32 {
    // SAFETY: this trampoline forwards unmodified values from the host.
    unsafe { dispatch_use(VANILLA_USE.get(), world, player, stack, context) }
}

unsafe fn dispatch_use(
    callback: Option<&Use>,
    world: abi::WorldRef,
    player: abi::EntityRef,
    stack: abi::StackRef,
    context: *const abi::UseOnBlock,
) -> i32 {
    guard("use_on_block", abi::USE_PASS, || {
        // SAFETY: the host passes a context that is live for this call.
        let (Some(&api), Some(callback), Some(world), Some(context)) =
            (TABLE.get(), callback, World::borrowed(world), unsafe {
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
    /// Show a status message to this player. Older hosts omit the message.
    pub fn action_bar(&self, text: &str) {
        action_bar(self.api, self.raw_player, text);
    }

    /// Add a saved stack to the player's inventory, dropping any remainder.
    pub fn give(&self, stack: &[u8]) -> bool {
        if self.api.size < std::mem::offset_of!(abi::InteractionV1, give) + size_of::<usize>() {
            return false;
        }
        // SAFETY: the player and saved stack are borrowed for this synchronous call.
        unsafe { ((&raw const (*self.api.ptr).give).read())(self.raw_player, abi::Str::new(stack)) }
    }

    /// Vanilla `ItemCooldowns.addCooldown` for the held item.
    pub fn cooldown(&self, ticks: u32) {
        // SAFETY: the player is live for this callback and the id is borrowed for the call.
        unsafe {
            ((&raw const (*self.api.ptr).cooldown).read())(
                self.raw_player,
                abi::Str::new(self.item.as_bytes()),
                ticks,
            )
        }
    }
}

impl Stack<'_> {
    /// Replace the callback stack's mod data and lodestone target in one operation.
    pub fn set_lodestone(&mut self, data: &[u8], tracker: Option<(&str, Pos)>) -> bool {
        if self.api.size
            < std::mem::offset_of!(abi::InteractionV1, stack_set_lodestone) + size_of::<usize>()
        {
            return false;
        }
        let (dimension, pos) = tracker.unwrap_or_default();
        // SAFETY: the checked callback borrows the stack, data and dimension for this call.
        unsafe {
            ((&raw const (*self.api.ptr).stack_set_lodestone).read())(
                self.raw,
                abi::Str::new(data),
                crate::s(dimension),
                pos,
            )
        }
    }

    pub fn count(&self) -> u32 {
        // SAFETY: the stack is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).stack_count).read())(self.raw) }
    }

    pub fn shrink(&mut self, count: u32) {
        // SAFETY: the stack is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).stack_shrink).read())(self.raw, count) }
    }

    /// The mod data stored on this stack, if any.
    pub fn data(&self) -> Option<Vec<u8>> {
        let mut out = vec![0; MAX_STACK_DATA];
        // SAFETY: the stack is live and `out` has the capacity passed.
        let len = unsafe {
            ((&raw const (*self.api.ptr).stack_data).read())(self.raw, out.as_mut_ptr(), out.len())
        };
        (len != usize::MAX && len <= out.len()).then(|| {
            out.truncate(len);
            out
        })
    }

    /// Replace the stack's mod data; empty removes it.
    pub fn set_data(&mut self, data: &[u8]) -> bool {
        // SAFETY: the stack is live and the slice is borrowed for the call.
        unsafe {
            ((&raw const (*self.api.ptr).set_stack_data).read())(
                self.raw,
                data.as_ptr(),
                data.len(),
            )
        }
    }

    /// The stack as saved NBT, `None` when empty. Needs `InteractionExtension::BlockUse`.
    pub fn read(&self) -> Option<Vec<u8>> {
        Interaction(self.api)
            .require_extension(InteractionExtension::BlockUse)
            .ok()?;
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the stack is live and the sink collects into the local Vec synchronously.
        unsafe { ((&raw const (*self.api.ptr).stack_read).read())(self.raw, sink) }.then_some(data)
    }

    /// Replace the stack with saved NBT; empty clears it. Needs `InteractionExtension::BlockUse`.
    pub fn write(&mut self, stack: &[u8]) -> bool {
        if Interaction(self.api)
            .require_extension(InteractionExtension::BlockUse)
            .is_err()
        {
            return false;
        }
        // SAFETY: the stack is live and the bytes are borrowed for the call.
        unsafe { ((&raw const (*self.api.ptr).stack_write).read())(self.raw, abi::Str::new(stack)) }
    }
}
