//! Native content API for moddedMC-rust mods.
//!
//! A mod is a Rust `cdylib` loaded by Pumpkin's native plugin loader. It must be built in the
//! server's workspace with the server's compiler, because types cross the boundary as plain
//! Rust values. In `on_load` it fetches the [`ContentHost`] service and hands over its
//! [`ContentMod`].
//!
//! A `.dll` carries its own copy of every library's globals (Tokio's runtime handle, the
//! tracing dispatcher). Pure reads of server objects are safe from mod code; anything that
//! may spawn tasks or touch the runtime goes through the host's function pointers below,
//! which run server code.

use std::{any::Any, sync::Arc};

pub use pumpkin_core;
use pumpkin_core::{entity::player::Player, plugin::Payload, world::World};
pub use pumpkin_protocol::java::client::dialog;
pub use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    text::TextComponent,
};

/// Service name the host registers [`ContentHost`] under.
pub const SERVICE: &str = "moddedmc:content-host";

/// A block entity type. Its state is opaque bytes owned by the mod; the host persists them
/// with the block and hands them back on every call.
pub struct BlockEntityDef {
    /// Namespaced id, e.g. `demo:counter`.
    pub id: String,
    /// Bumped by the mod when its state layout changes; saved next to the state.
    pub state_version: u32,
    pub initial_state: Vec<u8>,
    /// `Some(n)`: loaded instances tick every `n` ticks (n > 0). `None`: event-driven only.
    pub tick_interval: Option<u32>,
}

pub struct BlockDef {
    /// Namespaced id, e.g. `demo:counter`.
    pub id: String,
    pub display_name: String,
    /// Vanilla block the client sees: `minecraft:barrel`, `minecraft:dropper` or `minecraft:dispenser`.
    pub stand_in: String,
    /// Id of a block entity type registered by this mod.
    pub block_entity: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationError {
    InvalidDefinition,
    DuplicateId,
    UnknownEntityType,
    UnsupportedStandIn,
}

/// Open only during [`ContentMod::register`].
pub trait Registry {
    /// Returns the entity type index used in later calls.
    fn add_block_entity(&mut self, def: BlockEntityDef) -> Result<u32, RegistrationError>;
    fn add_block(&mut self, def: BlockDef) -> Result<(), RegistrationError>;
}

/// One due block entity. Change `state` in place and set `changed` to have it saved.
pub struct TickEntry {
    pub entity_type: u32,
    pub pos: BlockPos,
    pub state: Vec<u8>,
    pub changed: bool,
}

/// A player right-clicked one of this mod's blocks.
pub struct UseInput<'a> {
    pub entity_type: u32,
    pub pos: BlockPos,
    pub world: &'a Arc<World>,
    pub state: &'a [u8],
    pub player: &'a Arc<Player>,
}

#[derive(Default)]
pub struct UseResult {
    /// New state, or `None` to keep it.
    pub state: Option<Vec<u8>>,
    /// Shown to the player in the action bar.
    pub message: Option<String>,
}

/// One of this mod's blocks was placed (by the console when `player` is `None`).
pub struct PlaceInput<'a> {
    pub entity_type: u32,
    pub pos: BlockPos,
    pub world: &'a Arc<World>,
    /// The entity type's initial state.
    pub state: &'a [u8],
    pub player: Option<&'a Arc<Player>>,
}

/// One of this mod's blocks is about to be removed. Its state is final.
pub struct BreakInput<'a> {
    pub entity_type: u32,
    pub pos: BlockPos,
    pub world: &'a Arc<World>,
    pub state: &'a [u8],
    pub player: Option<&'a Arc<Player>>,
}

/// Implemented by a mod. A panic in any call is caught and logged by the host.
pub trait ContentMod: Send + Sync {
    /// Called once, on the first tick after the mod is added.
    fn register(&self, registry: &mut dyn Registry) -> Result<(), RegistrationError>;

    /// Due block entities of one world.
    fn tick(&self, _tick: u64, _world: &Arc<World>, _entries: &mut [TickEntry]) {}

    fn use_block(&self, _input: UseInput<'_>) -> UseResult {
        UseResult::default()
    }

    /// Return a replacement for the initial state, or `None`.
    fn place_block(&self, _input: PlaceInput<'_>) -> Option<Vec<u8>> {
        None
    }

    fn break_block(&self, _input: BreakInput<'_>) {}
}

/// The host, as seen by mods. Every function runs server code.
pub struct ContentHost {
    pub add_mod: fn(Arc<dyn ContentMod>),
    /// Puts the item that places `block_id` into the player's main hand. False if unknown.
    pub give_block: fn(&Arc<Player>, &str) -> bool,
    /// Moves the player, across dimensions if needed.
    pub teleport: fn(&Arc<Player>, Arc<World>, Vector3<f64>),
    pub show_dialog: fn(&Arc<Player>, &dialog::Dialog),
}

impl Payload for ContentHost {
    fn get_name_static() -> &'static str {
        SERVICE
    }
    fn get_name(&self) -> &'static str {
        SERVICE
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Exports the three symbols Pumpkin's native loader reads.
///
/// ```ignore
/// moddedmc_api::export_plugin!(MyPlugin, "my-mod", "What it does");
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin:expr, $name:expr, $description:expr) => {
        #[unsafe(no_mangle)]
        pub static PUMPKIN_API_VERSION: u32 = $crate::pumpkin_core::plugin::PLUGIN_API_VERSION;

        #[unsafe(no_mangle)]
        pub static METADATA: ::std::sync::LazyLock<$crate::pumpkin_core::plugin::PluginMetadata> =
            ::std::sync::LazyLock::new(|| $crate::pumpkin_core::plugin::PluginMetadata {
                name: $name.into(),
                version: env!("CARGO_PKG_VERSION").into(),
                authors: vec!["moddedMC-rust".into()],
                description: $description.into(),
                dependencies: Vec::new(),
                permissions: Vec::new(),
            });

        #[unsafe(no_mangle)]
        pub fn plugin() -> Box<dyn $crate::pumpkin_core::plugin::Plugin> {
            Box::new($plugin)
        }
    };
}
