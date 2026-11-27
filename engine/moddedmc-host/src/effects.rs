//! `moddedmc:effects` v1: sounds, particles and mob effect admission.

use std::{ptr, sync::Arc};

use moddedmc_api::abi::{self, Str, WorldRef};
use pumpkin_core::{
    entity::EntityBase,
    plugin::{
        BoxFuture, Cancellable, EventHandler,
        api::events::entity::entity_potion_effect::EntityPotionEffectEvent,
    },
    server::Server,
};
use pumpkin_data::{
    BlockStateId,
    data_component_impl::{IdOr, SoundEvent},
    effect::StatusEffect,
    item::Item,
    item_stack::ItemStack,
    particle::Particle,
    sound::{Sound, SoundCategory},
};
use pumpkin_protocol::{
    VarInt,
    codec::{
        data_component::data_to_proto_sound, item_stack_seralizer::ItemStackTemplateSerializer,
    },
    java::client::play::{CEntitySoundEffect, CParticle},
};
use pumpkin_util::{math::vector3::Vector3, random::get_seed};

use crate::content::world;

pub struct EffectVetoHandler;

impl EventHandler<EntityPotionEffectEvent> for EffectVetoHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityPotionEffectEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() {
                return;
            }
            let Some(effect) = event
                .effect_name
                .strip_prefix("effect.minecraft.")
                .and_then(StatusEffect::from_name)
            else {
                return;
            };
            let Some((world, entity)) = server.worlds.load().iter().find_map(|world| {
                world
                    .get_entity_by_id(event.entity_id)
                    .map(|entity| (world.clone(), entity))
            }) else {
                return;
            };
            let base: &dyn EntityBase = entity.as_ref();
            let Some(callback) = crate::mobs::effect_veto(base) else {
                return;
            };
            // SAFETY: the world, mob and effect id remain live for this synchronous callback.
            let allowed = unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    ptr::from_ref(&base).cast(),
                    Str::new(effect.minecraft_name.as_bytes()),
                    event.duration,
                    event.amplifier,
                )
            };
            if !allowed {
                event.set_cancelled(true);
            }
        })
    }
}

/// Vanilla `SoundSource` in declaration order, which the ABI ids follow.
const SOURCES: [SoundCategory; 11] = [
    SoundCategory::Master,
    SoundCategory::Music,
    SoundCategory::Records,
    SoundCategory::Weather,
    SoundCategory::Blocks,
    SoundCategory::Hostile,
    SoundCategory::Neutral,
    SoundCategory::Players,
    SoundCategory::Ambient,
    SoundCategory::Voice,
    SoundCategory::Ui,
];

fn finite(abi::Vec3([x, y, z]): abi::Vec3) -> Option<Vector3<f64>> {
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vector3::new(x, y, z))
}

fn sound_event(name: &str) -> IdOr<SoundEvent> {
    let vanilla = name
        .strip_prefix("minecraft:")
        .or_else(|| (!name.contains(':')).then_some(name))
        .and_then(Sound::from_name);
    vanilla.map_or_else(
        || {
            IdOr::Value(SoundEvent {
                sound_name: name.to_owned(),
                range: None,
            })
        },
        IdOr::Id,
    )
}

unsafe extern "C" fn play_sound(
    raw: WorldRef,
    position: abi::Vec3,
    sound: Str,
    source: u8,
    volume: f32,
    pitch: f32,
) -> bool {
    // SAFETY: the world and id are borrowed for this call.
    let (Some(world), name) = (unsafe { world(raw) }, unsafe { sound.str() }) else {
        return false;
    };
    let (Some(&category), Some(position), true, false) = (
        SOURCES.get(usize::from(source)),
        finite(position),
        volume.is_finite() && pitch.is_finite(),
        name.is_empty(),
    ) else {
        return false;
    };
    world.play_sound_event_fine(&sound_event(name), category, &position, volume, pitch);
    true
}

unsafe extern "C" fn play_entity_sound(
    raw: WorldRef,
    entity: i32,
    sound: Str,
    source: u8,
    volume: f32,
    pitch: f32,
) -> bool {
    // SAFETY: the world and ID are borrowed for this call.
    let (Some(world), name) = (unsafe { world(raw) }, unsafe { sound.str() }) else {
        return false;
    };
    let (Some(&category), Some(entity)) = (
        SOURCES.get(usize::from(source)),
        world.get_entity_by_id(entity),
    ) else {
        return false;
    };
    if name.is_empty() || !volume.is_finite() || !pitch.is_finite() {
        return false;
    }
    let source = entity.get_entity();
    if entity.get_player().is_none() && !world.is_tracked_by_any_player(source) {
        return false;
    }
    let packet = CEntitySoundEffect::new(
        data_to_proto_sound(&sound_event(name)),
        category,
        VarInt(source.entity_id),
        volume,
        pitch,
        get_seed() as i64,
    );
    world.send_to_tracking_players_and_self(source, &packet);
    true
}

unsafe extern "C" fn particles(
    raw: WorldRef,
    particle: Str,
    position: abi::Vec3,
    spread: abi::Vec3,
    speed: f32,
    count: u32,
) -> bool {
    // SAFETY: the world and id are borrowed for this call.
    let (Some(world), name) = (unsafe { world(raw) }, unsafe { particle.str() }) else {
        return false;
    };
    let (Some(particle), Some(position), Some(spread), true) = (
        Particle::from_name(name.strip_prefix("minecraft:").unwrap_or(name)),
        finite(position),
        finite(spread),
        speed.is_finite(),
    ) else {
        return false;
    };
    let spread = Vector3::new(spread.x as f32, spread.y as f32, spread.z as f32);
    world.spawn_particle(
        position,
        spread,
        speed,
        i32::try_from(count).unwrap_or(i32::MAX),
        particle,
    );
    true
}

fn particle_payload(particle: Particle, options: abi::ParticleOptions) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    match (particle, options.kind) {
        (Particle::Item, abi::PARTICLE_ITEM) => {
            // SAFETY: the caller keeps the item id borrowed for this call.
            let item = Item::from_registry_key(unsafe { options.item.str() })?;
            if item.id == Item::AIR.id {
                return None;
            }
            // 26.3 ItemParticleOption uses ItemStackTemplate.STREAM_CODEC.
            ItemStackTemplateSerializer::from(ItemStack::new(1, item))
                .write(&mut data)
                .ok()?;
        }
        (Particle::Block | Particle::FallingDust, abi::PARTICLE_BLOCK) => {
            let state = BlockStateId::from_runtime_id(u16::try_from(options.block_state).ok()?)?;
            VarInt(i32::from(state.vanilla_view().as_u16()))
                .encode(&mut data)
                .ok()?;
        }
        (Particle::Dust, abi::PARTICLE_DUST) => {
            if options.rgb > 0x00ff_ffff || !options.scale.is_finite() {
                return None;
            }
            data.extend_from_slice(&options.rgb.to_be_bytes());
            data.extend_from_slice(&options.scale.clamp(0.01, 4.0).to_be_bytes());
        }
        // 26.3 ParticleTypes registers these as option-bearing particle types.
        // Never send an empty payload for them.
        (_, abi::PARTICLE_SIMPLE)
            if !matches!(
                particle,
                Particle::Block
                    | Particle::BlockMarker
                    | Particle::Geyser
                    | Particle::GeyserBase
                    | Particle::GeyserPoof
                    | Particle::GeyserPlume
                    | Particle::DragonBreath
                    | Particle::Dust
                    | Particle::DustColorTransition
                    | Particle::Effect
                    | Particle::EntityEffect
                    | Particle::FallingDust
                    | Particle::TintedLeaves
                    | Particle::SculkCharge
                    | Particle::Flash
                    | Particle::InstantEffect
                    | Particle::Item
                    | Particle::Vibration
                    | Particle::Trail
                    | Particle::Shriek
                    | Particle::DustPillar
                    | Particle::BlockCrumble
            ) => {}
        _ => return None,
    }
    Some(data)
}

unsafe extern "C" fn particles_with_options(
    raw: WorldRef,
    particle: Str,
    position: abi::Vec3,
    spread: abi::Vec3,
    speed: f32,
    count: u32,
    options: abi::ParticleOptions,
    viewer: i32,
) -> bool {
    // SAFETY: the world and particle id are borrowed for this call.
    let (Some(world), name) = (unsafe { world(raw) }, unsafe { particle.str() }) else {
        return false;
    };
    let (Some(particle), Some(position), Some(spread), true) = (
        Particle::from_name(name.strip_prefix("minecraft:").unwrap_or(name)),
        finite(position),
        finite(spread),
        speed.is_finite(),
    ) else {
        return false;
    };
    let spread = Vector3::new(spread.x as f32, spread.y as f32, spread.z as f32);
    if !spread.x.is_finite() || !spread.y.is_finite() || !spread.z.is_finite() {
        return false;
    }
    let Some(data) = particle_payload(particle, options) else {
        return false;
    };
    let packet = CParticle::new(
        false,
        false,
        position,
        spread,
        speed,
        i32::try_from(count).unwrap_or(i32::MAX),
        VarInt(i32::from(particle.to_id())),
        &data,
    );
    if viewer == -1 {
        world.broadcast_packet_all(&packet);
    } else {
        let Some(entity) = world.get_entity_by_id(viewer) else {
            return false;
        };
        let Some(player) = entity.get_player() else {
            return false;
        };
        if !std::ptr::eq(world, player.world().as_ref()) {
            return false;
        }
        player.try_send_client_packet(&packet);
    }
    true
}

pub static EFFECTS: abi::EffectsV1 = abi::EffectsV1 {
    size: size_of::<abi::EffectsV1>(),
    play_sound,
    particles,
    play_entity_sound,
    particles_with_options,
    on_effect: crate::mobs::on_effect,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn options(kind: u32) -> abi::ParticleOptions {
        abi::ParticleOptions {
            item: Str::new(b"minecraft:salmon"),
            kind,
            block_state: 1,
            rgb: 0x0012_3456,
            scale: 0.5,
        }
    }

    #[test]
    fn dust_payload_is_packed_rgb_then_scale() {
        assert_eq!(
            particle_payload(Particle::Dust, options(abi::PARTICLE_DUST)),
            Some(vec![0, 0x12, 0x34, 0x56, 0x3f, 0, 0, 0])
        );
        let mut invalid = options(abi::PARTICLE_DUST);
        invalid.scale = f32::NAN;
        assert!(particle_payload(Particle::Dust, invalid).is_none());
    }

    #[test]
    fn particle_options_must_match_the_particle_type() {
        assert!(particle_payload(Particle::Dust, options(abi::PARTICLE_ITEM)).is_none());
        assert!(particle_payload(Particle::Item, options(abi::PARTICLE_SIMPLE)).is_none());
        assert_eq!(
            particle_payload(Particle::Heart, options(abi::PARTICLE_SIMPLE)),
            Some(vec![])
        );
        assert_eq!(
            particle_payload(Particle::FallingDust, options(abi::PARTICLE_BLOCK)),
            Some(vec![1])
        );
    }

    #[test]
    fn item_payload_is_a_26_3_stack_template() {
        let mut expected = Vec::new();
        assert!(
            VarInt(i32::from(Item::SALMON.id))
                .encode(&mut expected)
                .is_ok()
        );
        expected.extend_from_slice(&[1, 0, 0]);
        assert_eq!(
            particle_payload(Particle::Item, options(abi::PARTICLE_ITEM)),
            Some(expected)
        );
    }
}
