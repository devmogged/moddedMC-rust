//! Pending player death drops. A mod may take the whole batch after storing it.
use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, Player, RegistrationError, Registry, abi,
    content::{self, World},
    guard, require_interface,
    saved_data::Key,
};

#[derive(Clone, Copy)]
pub struct PlayerDeaths(&'static abi::PlayerDeathV1);

pub struct Death<'a> {
    pub world: World<'a>,
    pub player: Player<'a>,
    pub player_uuid: [u8; 16],
    pub position: [f64; 3],
    drops: &'a [abi::PlayerDeathStack],
    context: &'a abi::PlayerDeathContext,
}

pub struct Drop<'a> {
    /// Hotbar 0–8, storage 9–35, feet 36, legs 37, chest 38, head 39, offhand 40.
    pub slot: u32,
    /// Vanilla saved item stack, in named-root Java NBT.
    pub stack: &'a [u8],
}

impl<'a> Death<'a> {
    pub fn drops(&self) -> impl ExactSizeIterator<Item = Drop<'a>> {
        self.drops.iter().map(|drop| Drop {
            slot: drop.slot,
            // SAFETY: the ABI borrows the complete batch and every stack for this callback.
            stack: unsafe { drop.stack.bytes() },
        })
    }

    /// Commit the mod's record under a registered saved-data key and claim all the drops.
    /// False leaves the batch available to a later claim or the normal drop path.
    pub fn claim(&mut self, key: Key, record: &[u8]) -> bool {
        // SAFETY: the transaction and bytes are borrowed for this synchronous call.
        unsafe { (self.context.claim)(self.context.transaction, key.0, abi::Str::new(record)) }
    }
}

/// Inspect or claim the current death batch with `Death::claim`.
pub type Claim = fn(Death<'_>);

static CLAIM: OnceLock<Claim> = OnceLock::new();

pub fn require() -> Result<PlayerDeaths, InterfaceUnavailable> {
    content::require()?;
    // SAFETY: PlayerDeathV1 is the published type for the named interface.
    unsafe { require_interface(abi::PLAYER_DEATH, 1) }.map(PlayerDeaths)
}

impl PlayerDeaths {
    pub fn on_drops(
        self,
        registry: &mut Registry<'_>,
        claim: Claim,
    ) -> Result<(), RegistrationError> {
        CLAIM
            .set(claim)
            .map_err(|_| RegistrationError::DuplicateId)?;
        // SAFETY: the registration handle is live and the callback stays loaded.
        RegistrationError::from_code(unsafe { (self.0.on_drops)(registry.reg, dispatch) })
    }
}

unsafe extern "C" fn dispatch(context: *const abi::PlayerDeathContext) {
    guard("player death drops", (), || {
        // SAFETY: the context is borrowed from the host for this callback.
        let (Some(context), Some(claim)) = (unsafe { context.as_ref() }, CLAIM.get()) else {
            return;
        };
        let (Some(world), Some(player)) =
            (World::borrowed(context.world), Player::new(context.player))
        else {
            return;
        };
        if context.count == 0 || context.drops.is_null() {
            return;
        }
        // SAFETY: the host provides count live entries for the callback duration.
        let drops = unsafe { std::slice::from_raw_parts(context.drops, context.count) };
        claim(Death {
            world,
            player,
            player_uuid: context.player_uuid,
            position: context.position,
            drops,
            context,
        });
    })
}
