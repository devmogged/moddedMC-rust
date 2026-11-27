//! Server-defined, rebindable client key actions.
use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi,
    content::{self, World},
    entities::{self, Entity},
    guard, require_interface,
};

pub use abi::{ACTION_HELD, ACTION_PRESSED, ACTION_RELEASED};
pub type Action = fn(World<'_>, Entity<'_>, &str, u8);
static CALLBACK: OnceLock<Action> = OnceLock::new();

#[derive(Clone, Copy)]
pub struct PlayerActions(&'static abi::PlayerActionsV1);

pub fn require() -> Result<PlayerActions, InterfaceUnavailable> {
    content::require()?;
    entities::require()?;
    // SAFETY: this is the published table layout.
    unsafe { require_interface(abi::PLAYER_ACTIONS, 1) }.map(PlayerActions)
}

impl PlayerActions {
    pub fn add(
        self,
        reg: &mut Registry<'_>,
        id: &str,
        translation: &str,
        key: &str,
        phases: u8,
        callback: Action,
    ) -> Result<(), RegistrationError> {
        let installed = CALLBACK.get_or_init(|| callback);
        if !std::ptr::fn_addr_eq(*installed, callback) {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: all strings and registration are borrowed for this call.
        RegistrationError::from_code(unsafe {
            (self.0.add)(
                reg.reg,
                id.into(),
                translation.into(),
                key.into(),
                phases,
                dispatch,
            )
        })
    }
}

unsafe extern "C" fn dispatch(
    world: abi::WorldRef,
    player: abi::EntityRef,
    id: abi::Str,
    phase: u8,
) {
    guard("player action", (), || {
        if let (Some(world), Some(player), Some(callback)) = (
            World::borrowed(world),
            Entity::borrowed(player),
            CALLBACK.get(),
        ) {
            // SAFETY: the host borrows the registered id for the callback.
            callback(world, player, unsafe { id.str() }, phase);
        }
    });
}
