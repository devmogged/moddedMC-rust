use std::{
    collections::HashMap,
    ptr,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use moddedmc_api::abi::{self, Reg, Str, code, native_entity_phase as phase};
use pumpkin_core::{
    entity::{
        Entity, EntityBase, RemovalReason,
        interaction::{InteractionBehavior, InteractionEntity},
        player::Player,
    },
    world::World,
};
use pumpkin_data::entity::EntityType;
use pumpkin_nbt::tag::NbtTag;
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use uuid::Uuid;

use crate::{content, lock, native};

const DATA_NAMESPACE: &str = "moddedmc";
const OWNER_KEY: &str = "native_actor";
const PHYSICS_KEY: &str = "native_actor_physics";
const DATA_KEY: &str = "native_actor_data";

#[derive(Clone, Copy)]
struct Kind {
    id: u64,
    owner: &'static str,
    callback: abi::NativeEntityCallback,
}

type KindMap = HashMap<&'static str, Kind>;
static NEXT_KIND: AtomicU64 = AtomicU64::new(1);
static KINDS: LazyLock<Mutex<KindMap>> = LazyLock::new(Mutex::default);

pub static NATIVE_ENTITIES: abi::NativeEntitiesV1 = abi::NativeEntitiesV1 {
    size: size_of::<abi::NativeEntitiesV1>(),
    register: stage,
    spawn,
    data,
    set_data,
};

unsafe extern "C" fn stage(reg: Reg, callback: abi::NativeEntityCallback, out: *mut u64) -> i32 {
    if out.is_null() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: the registration argument is borrowed by the active register callback.
    let staging = unsafe { native::staging(reg) };
    if staging.native_actor.is_some() {
        return code::DUPLICATE_ID;
    }
    let id = NEXT_KIND.fetch_add(1, Ordering::Relaxed);
    staging.native_actor = Some((id, callback));
    // SAFETY: the registration borrows the output for this call.
    unsafe { out.write(id) };
    code::OK
}

pub fn register(owner: &'static str, binding: Option<(u64, abi::NativeEntityCallback)>) {
    if let Some((id, callback)) = binding {
        lock(&KINDS).insert(
            owner,
            Kind {
                id,
                owner,
                callback,
            },
        );
    }
}

pub fn attach(entity: &Arc<dyn EntityBase>) {
    if entity.get_entity().entity_type.id != EntityType::INTERACTION.id {
        return;
    }
    let Some(carrier) = entity.cast_any().downcast_ref::<InteractionEntity>() else {
        return;
    };
    let Some(NbtTag::String(owner)) = carrier.entity.get_custom_data(DATA_NAMESPACE, OWNER_KEY)
    else {
        return;
    };
    let Some(kind) = lock(&KINDS).get(owner.as_ref()).copied() else {
        return;
    };
    let physics = matches!(
        carrier.entity.get_custom_data(DATA_NAMESPACE, PHYSICS_KEY),
        Some(NbtTag::Byte(1))
    );
    carrier.entity.no_physics.store(!physics, Ordering::Relaxed);
    carrier.attach(Arc::new(kind));
}

impl Kind {
    fn send(
        self,
        world: &World,
        carrier: &InteractionEntity,
        player: Option<&Player>,
        phase: u8,
    ) -> bool {
        let base: &dyn EntityBase = carrier;
        let context = abi::NativeEntityContext {
            world: ptr::from_ref(world).cast(),
            entity: ptr::from_ref(&base).cast(),
            player: abi::PlayerRef(player.map_or(ptr::null(), |p| ptr::from_ref(p).cast())),
            uuid: *carrier.entity.entity_uuid.as_bytes(),
            phase,
        };
        // SAFETY: the entity, world and optional player are borrowed for this synchronous event.
        unsafe { (self.callback)(&raw const context) }
    }
}

impl InteractionBehavior for Kind {
    fn loaded(&self, carrier: &InteractionEntity) {
        self.send(&carrier.entity.world.load(), carrier, None, phase::LOADED);
    }

    fn tick(&self, carrier: &InteractionEntity) {
        if !carrier.entity.is_removed() {
            self.send(&carrier.entity.world.load(), carrier, None, phase::TICK);
        }
    }

    fn interact(&self, carrier: &InteractionEntity, player: &Player) -> bool {
        !carrier.entity.is_removed()
            && self.send(
                &carrier.entity.world.load(),
                carrier,
                Some(player),
                phase::USED,
            )
    }

    fn removed(&self, carrier: &InteractionEntity, reason: RemovalReason) {
        let phase = if reason.should_destroy() {
            phase::REMOVED
        } else {
            phase::UNLOADED
        };
        self.send(&carrier.entity.world.load(), carrier, None, phase);
    }
}

unsafe extern "C" fn spawn(settings: *const abi::NativeEntitySpawn) -> bool {
    // SAFETY: the caller borrows its spawn settings for the call.
    let Some(settings) = (unsafe { settings.as_ref() }) else {
        return false;
    };
    let id = Uuid::from_bytes(settings.uuid);
    let [x, y, z] = settings.position.0;
    let position = Vector3::new(x, y, z);
    if id.is_nil()
        || ![x, y, z].iter().all(|number| number.is_finite())
        || !(0.001..=16.0).contains(&settings.width)
        || !(0.001..=16.0).contains(&settings.height)
        || settings.data.len > abi::MAX_ENTITY_DATA_BYTES
    {
        return false;
    }
    // SAFETY: the world handle is borrowed during the spawn call.
    let Some(world) = (unsafe { content::world(settings.world) }).and_then(content::shared) else {
        return false;
    };
    let Some(kind) = lock(&KINDS)
        .values()
        .find(|kind| kind.id == settings.kind)
        .copied()
    else {
        return false;
    };
    if !world
        .level
        .is_chunk_loaded(&BlockPos::floored_v(position).chunk_position())
    {
        return false;
    }
    let carrier = InteractionEntity::new(Entity::from_uuid(
        id,
        world.clone(),
        position,
        &EntityType::INTERACTION,
    ));
    carrier.set_width(settings.width);
    carrier.set_height(settings.height);
    carrier
        .entity
        .set_custom_data(DATA_NAMESPACE, OWNER_KEY, NbtTag::from(kind.owner));
    carrier.entity.set_custom_data(
        DATA_NAMESPACE,
        PHYSICS_KEY,
        NbtTag::Byte(i8::from(settings.physics)),
    );
    // SAFETY: the actor payload is borrowed during the spawn call.
    store_data(&carrier.entity, unsafe { settings.data.bytes() });
    world.spawn_entity(carrier)
}

fn store_data(carrier: &Entity, data: &[u8]) {
    carrier.set_custom_data(
        DATA_NAMESPACE,
        DATA_KEY,
        NbtTag::ByteArray(data.iter().map(|&byte| byte.cast_signed()).collect()),
    );
}

unsafe extern "C" fn data(raw: abi::EntityRef, out: abi::Sink) -> bool {
    // SAFETY: the entity is borrowed by the current callback.
    let Some(carrier) = (unsafe { content::entity(raw) }).map(EntityBase::get_entity) else {
        return false;
    };
    let Some(NbtTag::ByteArray(data)) = carrier.get_custom_data(DATA_NAMESPACE, DATA_KEY) else {
        return false;
    };
    let bytes: Vec<u8> = data.iter().map(|&byte| byte.cast_unsigned()).collect();
    // SAFETY: the caller borrows the sink and the bytes are available until put returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn set_data(raw: abi::EntityRef, data: Str) -> bool {
    if data.len > abi::MAX_ENTITY_DATA_BYTES {
        return false;
    }
    // SAFETY: the entity is borrowed by the current callback.
    let Some(carrier) = (unsafe { content::entity(raw) })
        .map(EntityBase::get_entity)
        .filter(|entity| !entity.is_removed())
    else {
        return false;
    };
    // SAFETY: the data bytes are borrowed through this call and are copied by store_data.
    store_data(carrier, unsafe { data.bytes() });
    true
}
