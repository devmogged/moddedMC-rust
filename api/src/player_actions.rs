//! Server-defined, rebindable client key actions.
use std::sync::OnceLock;

use crate::{
    InterfacePrefix, InterfaceUnavailable, RegistrationError, Registry, abi,
    content::{self, World},
    entities::{self, Entity},
    guard,
};

pub use abi::{ACTION_HELD, ACTION_PRESSED, ACTION_RELEASED};
pub const MOUNTED_ATTACK: &str = "moddedmc:mounted_attack";
pub const MOUNTED_USE: &str = "moddedmc:mounted_use";
pub const MOUNTED_INVENTORY: &str = "moddedmc:mounted_inventory";
pub const DOUBLE_TAP_FORWARD: &str = "moddedmc:double_tap/forward";
pub const DOUBLE_TAP_BACK: &str = "moddedmc:double_tap/back";
pub const DOUBLE_TAP_LEFT: &str = "moddedmc:double_tap/left";
pub const DOUBLE_TAP_RIGHT: &str = "moddedmc:double_tap/right";
pub type Action = fn(World<'_>, Entity<'_>, &str, u8);
static CALLBACK: OnceLock<Action> = OnceLock::new();

#[derive(Clone, Copy)]
pub struct PlayerActions(InterfacePrefix<abi::PlayerActionsV1>);

pub fn require() -> Result<PlayerActions, InterfaceUnavailable> {
    content::require()?;
    entities::require()?;
    // SAFETY: this is the published table layout.
    unsafe {
        InterfacePrefix::require(
            abi::PLAYER_ACTIONS,
            1,
            std::mem::offset_of!(abi::PlayerActionsV1, subscribe),
        )
    }
    .map(PlayerActions)
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
            ((&raw const (*self.0.ptr).add).read())(
                reg.reg,
                id.into(),
                translation.into(),
                key.into(),
                phases,
                dispatch,
            )
        })
    }

    pub fn require_mounted(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(std::mem::offset_of!(abi::PlayerActionsV1, subscribe))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:player_actions.subscribe",
                version: 1,
            })
    }

    pub fn subscribe(
        self,
        reg: &mut Registry<'_>,
        id: &str,
        phases: u8,
        callback: Action,
    ) -> Result<(), RegistrationError> {
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::PlayerActionsV1, subscribe))
        else {
            return Err(RegistrationError::InvalidDefinition);
        };
        if !std::ptr::fn_addr_eq(*CALLBACK.get_or_init(|| callback), callback) {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: the checked extension includes subscribe, and id is borrowed for this call.
        RegistrationError::from_code(unsafe {
            ((&raw const (*api).subscribe).read())(reg.reg, id.into(), phases, dispatch)
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
