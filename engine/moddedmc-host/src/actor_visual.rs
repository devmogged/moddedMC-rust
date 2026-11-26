use std::{
    any::Any,
    collections::HashMap,
    io::Cursor,
    sync::{Arc, LazyLock, Mutex, Weak, atomic::Ordering},
};

use moddedmc_api::abi::{self, Str};
use pumpkin_core::{
    entity::{
        Entity, EntityBase, decoration::mannequin::MannequinEntity, living::LivingEntity,
        r#type::from_type,
    },
    plugin::{
        BoxFuture, Cancellable, EventHandler,
        api::events::player::player_interact_entity_event::PlayerInteractEntityEvent,
    },
    server::Server,
};
use pumpkin_data::{
    damage::DamageType,
    data_component_impl::{DataComponentImpl, EquipmentSlot, ProfileImpl, ProfileProperty},
    entity::{EntityPose, EntityType},
    item_stack::ItemStack,
};
use pumpkin_nbt::{Nbt, compound::NbtCompound, deserializer::NbtReadHelperJava, tag::NbtTag};
use pumpkin_protocol::java::server::play::ActionType;
use pumpkin_util::{
    math::{vector2::Vector2, vector3::Vector3},
    text::TextComponent,
};
use uuid::Uuid;

use crate::{content, lock, native, stacks};

struct Proxy {
    inner: Arc<dyn EntityBase>,
    owner: Weak<dyn EntityBase>,
    offset: Vector3<f64>,
}

static VISUALS: LazyLock<Mutex<HashMap<i32, Arc<Proxy>>>> = LazyLock::new(Mutex::default);

impl EntityBase for Proxy {
    fn get_entity(&self) -> &Entity {
        self.inner.get_entity()
    }

    fn get_living_entity(&self) -> Option<&LivingEntity> {
        self.inner.get_living_entity()
    }

    fn cast_any(&self) -> &dyn Any {
        self
    }

    fn init_data_tracker(&self) {
        self.inner.init_data_tracker();
    }

    fn is_pushable(&self) -> bool {
        false
    }

    fn can_hit(&self) -> bool {
        false
    }

    fn tick(&self, _: &dyn EntityBase, _: &Server) {}

    fn damage_with_context(
        &self,
        _: &dyn EntityBase,
        _: f32,
        _: DamageType,
        _: Option<Vector3<f64>>,
        _: Option<&dyn EntityBase>,
        _: Option<&dyn EntityBase>,
    ) -> bool {
        false
    }

    fn kill(&self, _: &dyn EntityBase) {
        if let Some(owner) = self.owner.upgrade() {
            owner
                .get_entity()
                .world
                .load()
                .remove_entity(owner.as_ref());
        }
    }

    fn should_drop_loot(&self) -> bool {
        false
    }

    fn counts_toward_mob_cap(&self) -> bool {
        false
    }

    fn chunk_lifecycle_anchor(&self) -> Option<Vector2<i32>> {
        self.owner
            .upgrade()
            .map(|owner| owner.get_entity().chunk_pos.load())
    }

    fn write_custom_nbt(&self, nbt: &mut NbtCompound) {
        self.inner.write_custom_nbt(nbt);
    }

    fn saves_with_chunk(&self) -> bool {
        false
    }
}

pub struct UseVisual;

impl EventHandler<PlayerInteractEntityEvent> for UseVisual {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerInteractEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() {
                return;
            }
            let Some(visual) = event.target.cast_any().downcast_ref::<Proxy>() else {
                return;
            };
            if event.action == ActionType::Attack {
                event.set_cancelled(true);
                return;
            }
            let parent = visual.owner.upgrade();
            if let Some(parent) = parent.filter(|parent| {
                !parent.get_entity().is_removed()
                    && Arc::ptr_eq(&parent.get_entity().world.load(), &event.player.world())
                    && event.player.can_interact_with_entity(parent.as_ref(), 1.0)
            }) {
                event.target = parent;
            } else {
                event.set_cancelled(true);
            }
        })
    }
}

pub fn follow(owner: &Entity) {
    let Some(proxy) = lock(&VISUALS).get(&owner.entity_id).cloned() else {
        return;
    };
    let next = owner.pos.load() + proxy.offset;
    if proxy.inner.get_entity().pos.load() != next {
        proxy.inner.get_entity().set_pos(next);
    }
}

pub fn remove(owner: &Entity) {
    let Some(proxy) = lock(&VISUALS).remove(&owner.entity_id) else {
        return;
    };
    let world = owner.world.load();
    if world
        .get_entity_by_id(proxy.get_entity().entity_id)
        .is_some()
    {
        world.remove_entity(proxy.as_ref());
    }
}

pub unsafe extern "C" fn player_profile(raw: abi::PlayerRef, out: abi::Sink) -> bool {
    // SAFETY: the player is borrowed by the caller until the sink returns.
    let Some(player) = (unsafe { native::player(raw) }) else {
        return false;
    };
    let source = &player.gameprofile;
    let (hi, lo) = source.id.as_u64_pair();
    let profile = ProfileImpl {
        id: Some([(hi >> 32) as i32, hi as i32, (lo >> 32) as i32, lo as i32]),
        name: Some(source.name.clone()),
        properties: source
            .properties
            .load()
            .iter()
            .map(|property| ProfileProperty {
                name: property.name.to_string(),
                value: property.value.to_string(),
                signature: property.signature.as_ref().map(ToString::to_string),
            })
            .collect(),
        ..ProfileImpl::default()
    };
    let NbtTag::Compound(mut profile) = profile.write_data() else {
        return false;
    };
    let settings = player.config.load();
    profile.put_int("skin_layers", i32::from(settings.skin_parts));
    profile.put_int("main_arm", i32::from(settings.main_hand as u8));
    profile.put_int("hotbar_slot", i32::from(player.inventory().get_selected_slot()));
    let bytes = Nbt::from(profile).write();
    // SAFETY: the output callback and bytes are valid through the call.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

fn apply_profile(model: &MannequinEntity, profile: Str) -> bool {
    if profile.len == 0 {
        return true;
    }
    // SAFETY: the profile is borrowed through the surrounding call to show_sleeping.
    let bytes = unsafe { profile.bytes() };
    let mut reader = Cursor::new(bytes);
    let Ok(nbt) = Nbt::read(&mut NbtReadHelperJava::new(&mut reader)) else {
        return false;
    };
    if reader.position() != bytes.len() as u64 {
        return false;
    }
    let skin_layers = nbt
        .root_tag
        .get_int("skin_layers")
        .and_then(|value| u8::try_from(value).ok());
    let main_arm = nbt
        .root_tag
        .get_int("main_arm")
        .and_then(|value| u8::try_from(value).ok());
    let Some(profile) = ProfileImpl::read_data(&NbtTag::Compound(nbt.root_tag)) else {
        return false;
    };
    model.set_profile(profile);
    if let Some(skin_layers) = skin_layers {
        model.set_layers_mask(skin_layers);
    }
    if let Some(main_arm) = main_arm {
        model.set_main_hand_id(main_arm);
    }
    true
}

pub unsafe extern "C" fn show_sleeping(
    raw: abi::EntityRef,
    visual: *const abi::NativeEntityVisual,
) -> bool {
    // SAFETY: the actor and visual settings are live in the current mod callback.
    let (Some(owner), Some(visual)) = (unsafe { content::entity(raw) }, unsafe { visual.as_ref() })
    else {
        return false;
    };
    let owner = owner.get_entity();
    let [x, y, z] = visual.offset.0;
    if owner.entity_type.id != EntityType::INTERACTION.id
        || owner.is_removed()
        || ![x, y, z].iter().all(|d| d.is_finite() && d.abs() <= 4.0)
        || !visual.yaw.is_finite()
        || visual.profile.len > abi::MAX_ENTITY_DATA_BYTES
    {
        return false;
    }
    let offset = Vector3::new(x, y, z);
    let Some(equipment): Option<Vec<_>> = visual
        .equipment
        .iter()
        .map(|&stack| {
            if stack.len == 0 {
                Some(ItemStack::EMPTY.clone())
            } else {
                stacks::read_stack(stack)
            }
        })
        .collect()
    else {
        return false;
    };
    let world = owner.world.load_full();
    let Some(owner_ref) = world.get_entity_by_id(owner.entity_id) else {
        return false;
    };
    let position = owner.pos.load() + offset;
    let model: Arc<dyn EntityBase> = match visual.model {
        abi::ACTOR_VISUAL_PLAYER => {
            let mannequin =
                MannequinEntity::new(Entity::new(world.clone(), position, &EntityType::MANNEQUIN));
            if !apply_profile(&mannequin, visual.profile) {
                return false;
            }
            mannequin.set_description_state(TextComponent::empty(), true);
            mannequin.set_immovable(true);
            mannequin
        }
        abi::ACTOR_VISUAL_SKELETON => {
            from_type(&EntityType::SKELETON, position, &world, Uuid::new_v4())
        }
        _ => return false,
    };
    let entity = model.get_entity();
    entity.set_rotation(visual.yaw, 0.0);
    entity.body_yaw.store(visual.yaw);
    entity.set_pose(EntityPose::Sleeping);
    entity.no_physics.store(true, Ordering::Relaxed);
    entity.set_has_no_gravity(true);
    entity.set_silent(true);
    entity.set_invulnerable(true);
    if let Some(living) = model.get_living_entity() {
        let mut target = lock(&living.entity_equipment);
        for (slot, stack) in [
            EquipmentSlot::MAIN_HAND,
            EquipmentSlot::OFF_HAND,
            EquipmentSlot::FEET,
            EquipmentSlot::LEGS,
            EquipmentSlot::CHEST,
            EquipmentSlot::HEAD,
        ]
        .iter()
        .zip(equipment)
        {
            target.put(slot, stack);
        }
    }
    remove(owner);
    let proxy = Arc::new(Proxy {
        inner: model,
        owner: Arc::downgrade(&owner_ref),
        offset,
    });
    proxy.init_data_tracker();
    world.spawn_entity_non_save(proxy.clone());
    lock(&VISUALS).insert(owner.entity_id, proxy);
    true
}
