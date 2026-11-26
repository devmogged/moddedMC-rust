//! Persistent, selectable nonliving entities on the stock interaction carrier.

use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, Player, RegistrationError, Registry, abi,
    content::{self, World},
    entities::{self, EntitiesExtension, Entity},
    guard, require_interface,
};

#[derive(Clone, Copy)]
pub struct Actors(&'static abi::NativeEntitiesV1);

#[derive(Clone, Copy)]
pub struct ActorKind(u64);

pub struct Spawn<'a> {
    pub uuid: [u8; 16],
    pub position: [f64; 3],
    pub width: f32,
    pub height: f32,
    pub physics: bool,
    pub data: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loaded,
    Tick,
    Used,
    Unloaded,
    Removed,
}

#[derive(Clone, Copy)]
pub struct Actor<'a> {
    pub entity: Entity<'a>,
    pub uuid: [u8; 16],
    raw: abi::EntityRef,
    table: &'static abi::NativeEntitiesV1,
}

impl Actor<'_> {
    pub fn data(self) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: content::bytes,
        };
        // SAFETY: the actor and sink are borrowed for the call.
        unsafe { (self.table.data)(self.raw, sink) }.then_some(data)
    }

    pub fn set_data(self, data: &[u8]) -> bool {
        // SAFETY: the host copies these bytes before returning.
        unsafe { (self.table.set_data)(self.raw, abi::Str::new(data)) }
    }
}

pub struct Event<'a> {
    pub actor: Actor<'a>,
    pub world: World<'a>,
    pub player: Option<Player<'a>>,
    pub phase: Phase,
}

pub type OnEvent = fn(Event<'_>) -> bool;
static CALLBACK: OnceLock<OnEvent> = OnceLock::new();
static TABLE: OnceLock<&'static abi::NativeEntitiesV1> = OnceLock::new();

pub fn require() -> Result<Actors, InterfaceUnavailable> {
    content::require()?;
    let entities = entities::require()?;
    entities.require_extension(EntitiesExtension::Carriers)?;
    // SAFETY: this is the registered, versioned native entity table.
    let table = unsafe { require_interface(abi::NATIVE_ENTITIES, 1) }?;
    let _ = TABLE.set(table);
    Ok(Actors(table))
}

impl Actors {
    pub fn register(
        self,
        registry: &mut Registry<'_>,
        callback: OnEvent,
    ) -> Result<ActorKind, RegistrationError> {
        CALLBACK
            .set(callback)
            .map_err(|_| RegistrationError::DuplicateId)?;
        let mut kind = 0;
        // SAFETY: the registry and output live through the call.
        RegistrationError::from_code(unsafe {
            (self.0.register)(registry.reg, dispatch, &raw mut kind)
        })?;
        Ok(ActorKind(kind))
    }

    /// Spawn in a loaded chunk. Use a stable, nonzero UUID for a persistent actor.
    pub fn spawn(self, world: &World<'_>, kind: ActorKind, settings: &Spawn<'_>) -> bool {
        let spawn = abi::NativeEntitySpawn {
            world: world.raw(),
            kind: kind.0,
            uuid: settings.uuid,
            position: abi::Vec3(settings.position),
            width: settings.width,
            height: settings.height,
            physics: settings.physics,
            data: abi::Str::new(settings.data),
        };
        // SAFETY: the world, settings and data are borrowed for the call.
        unsafe { (self.0.spawn)(&raw const spawn) }
    }
}

unsafe extern "C" fn dispatch(context: *const abi::NativeEntityContext) -> bool {
    guard("native entity event", false, || {
        // SAFETY: the context and its handles are live for this callback.
        let (Some(context), Some(callback), Some(&table)) =
            (unsafe { context.as_ref() }, CALLBACK.get(), TABLE.get())
        else {
            return false;
        };
        let (Some(world), Some(entity)) = (
            World::borrowed(context.world),
            Entity::borrowed(context.entity),
        ) else {
            return false;
        };
        let phase = match context.phase {
            abi::native_entity_phase::LOADED => Phase::Loaded,
            abi::native_entity_phase::TICK => Phase::Tick,
            abi::native_entity_phase::USED => Phase::Used,
            abi::native_entity_phase::UNLOADED => Phase::Unloaded,
            abi::native_entity_phase::REMOVED => Phase::Removed,
            _ => return false,
        };
        callback(Event {
            actor: Actor {
                entity,
                uuid: context.uuid,
                raw: context.entity,
                table,
            },
            world,
            player: Player::new(context.player),
            phase,
        })
    })
}
