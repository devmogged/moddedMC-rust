//! `moddedmc:effects` v1: sounds, particles and mob effect admission.

use std::{
    collections::HashMap,
    ptr,
    sync::{Arc, LazyLock, Mutex, Weak},
};

use moddedmc_api::abi::{self, Str, WorldRef};
use pumpkin_core::{
    entity::{EntityBase, player::Player},
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
    java::client::play::{
        BosseventAction, CBossEvent, CCustomPayload, CEntitySoundEffect, CParticle,
    },
};
use pumpkin_util::{math::vector3::Vector3, random::get_seed, text::TextComponent};
use uuid::Uuid;

use crate::content::world;

struct Lease {
    player: Weak<Player>,
    world: Uuid,
    until: u64,
}

impl Lease {
    fn new(player: &Arc<Player>, ttl: u32) -> Self {
        Self {
            player: Arc::downgrade(player),
            world: player.world().uuid,
            until: now().saturating_add(u64::from(ttl)),
        }
    }

    fn matches(&self, player: &Arc<Player>) -> bool {
        self.world == player.world().uuid && self.player.ptr_eq(&Arc::downgrade(player))
    }

    fn expired(&self, player: &Player, time: u64) -> bool {
        time >= self.until || player.world().uuid != self.world
    }
}

struct Bar {
    lease: Lease,
    uuid: Uuid,
    title: TextComponent,
    progress: f32,
    color: u8,
    style: u8,
}

type Key = (i32, String);
static BARS: LazyLock<Mutex<HashMap<Key, Bar>>> = LazyLock::new(Mutex::default);
static MUSIC: LazyLock<Mutex<HashMap<Key, Lease>>> = LazyLock::new(Mutex::default);

fn now() -> u64 {
    crate::native::SERVER.get().map_or(0, |s| {
        u64::try_from(s.tick_count.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(0)
    })
}

fn remove_bar(player: &Player, uuid: &Uuid) {
    player.try_send_client_packet(&CBossEvent::new(uuid, BosseventAction::Remove));
}

fn stop_track(player: &Player, key: &str) {
    player.stop_sound(Some(key.to_owned()), Some(SoundCategory::Records));
}

/// Leases also expire when the source block unloads or the player changes dimension.
pub fn tick_display(time: u64) {
    crate::lock(&BARS).retain(|_, bar| {
        let Some(player) = bar.lease.player.upgrade() else {
            return false;
        };
        if bar.lease.expired(&player, time) {
            remove_bar(&player, &bar.uuid);
            return false;
        }
        true
    });
    crate::lock(&MUSIC).retain(|(_, key), lease| {
        let Some(player) = lease.player.upgrade() else {
            return false;
        };
        if lease.expired(&player, time) {
            stop_track(&player, key);
            return false;
        }
        true
    });
}

fn valid_key(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id.split_once(':').is_some_and(|(ns, path)| {
            !ns.is_empty()
                && !path.is_empty()
                && ns
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
        })
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn boss_bar(
    raw: WorldRef,
    player: i32,
    id: Str,
    title: Str,
    progress: f32,
    color: u8,
    style: u8,
    ttl: u32,
) -> bool {
    // SAFETY: all ABI arguments are borrowed for this synchronous call.
    let (Some(world), id, title) = (unsafe { world(raw) }, unsafe { id.str() }, unsafe {
        title.str()
    }) else {
        return false;
    };
    if !valid_key(id)
        || title.len() > 8192
        || !progress.is_finite()
        || color > 6
        || style > 4
        || ttl == 0
    {
        return false;
    }
    let (Some(player_ref), Ok(title)) = (
        world.get_player_by_id(player),
        serde_json::from_str::<TextComponent>(title),
    ) else {
        return false;
    };
    let progress = progress.clamp(0.0, 1.0);
    let mut bars = crate::lock(&BARS);
    let key = (player, id.to_owned());
    if let Some(bar) = bars.get_mut(&key) {
        if bar.lease.matches(&player_ref) {
            bar.lease = Lease::new(&player_ref, ttl);
            if bar.title != title {
                player_ref.try_send_client_packet(&CBossEvent::new(
                    &bar.uuid,
                    BosseventAction::UpdateTile(title.clone()),
                ));
                bar.title = title;
            }
            if bar.progress != progress {
                player_ref.try_send_client_packet(&CBossEvent::new(
                    &bar.uuid,
                    BosseventAction::UpdateHealth(progress),
                ));
                bar.progress = progress;
            }
            if bar.color != color || bar.style != style {
                player_ref.try_send_client_packet(&CBossEvent::new(
                    &bar.uuid,
                    BosseventAction::UpdateStyle {
                        color: VarInt(i32::from(color)),
                        dividers: VarInt(i32::from(style)),
                    },
                ));
                bar.color = color;
                bar.style = style;
            }
            return true;
        }
        if let Some(old) = bar.lease.player.upgrade() {
            remove_bar(&old, &bar.uuid);
        }
    }
    let uuid = Uuid::new_v4();
    player_ref.try_send_client_packet(&CBossEvent::new(
        &uuid,
        BosseventAction::Add {
            title: title.clone(),
            health: progress,
            color: VarInt(i32::from(color)),
            division: VarInt(i32::from(style)),
            flags: 0,
        },
    ));
    bars.insert(
        key,
        Bar {
            lease: Lease::new(&player_ref, ttl),
            uuid,
            title,
            progress,
            color,
            style,
        },
    );
    true
}

unsafe extern "C" fn remove_boss_bar(raw: WorldRef, player: i32, id: Str) -> bool {
    // SAFETY: the world and id are borrowed for this call.
    let (Some(world), id) = (unsafe { world(raw) }, unsafe { id.str() }) else {
        return false;
    };
    let Some(player_ref) = world.get_player_by_id(player) else {
        return false;
    };
    let mut bars = crate::lock(&BARS);
    let key = (player, id.to_owned());
    if bars
        .get(&key)
        .is_some_and(|bar| bar.lease.matches(&player_ref))
        && let Some(bar) = bars.remove(&key)
    {
        remove_bar(&player_ref, &bar.uuid);
    }
    true
}

#[allow(clippy::too_many_arguments)]
unsafe extern "C" fn loop_music(
    raw: WorldRef,
    player: i32,
    key: Str,
    volume: f32,
    fade_in: f32,
    fade_out: f32,
    ttl: u32,
) -> bool {
    // SAFETY: the world and key are borrowed for this call.
    let (Some(world), key) = (unsafe { world(raw) }, unsafe { key.str() }) else {
        return false;
    };
    if !valid_key(key)
        || !(0.0..=1.0).contains(&volume)
        || !(0.0..=1.0).contains(&fade_in)
        || !(0.0..=1.0).contains(&fade_out)
        || ttl == 0
    {
        return false;
    }
    let Some(player_ref) = world.get_player_by_id(player) else {
        return false;
    };
    let mut music = crate::lock(&MUSIC);
    let map_key = (player, key.to_owned());
    let refresh = music
        .get(&map_key)
        .is_some_and(|lease| lease.matches(&player_ref));
    if !refresh && let Some(old) = music.get(&map_key).and_then(|lease| lease.player.upgrade()) {
        stop_track(&old, key);
    }
    // Companion supplies the loop, relative position and gain ramp that vanilla SOUND
    // cannot express. The following vanilla SOUND is the actual start trigger.
    let Ok(metadata) = serde_json::to_vec(&serde_json::json!({
        "key": key, "volume": volume, "fadeIn": fade_in, "fadeOut": fade_out, "ttl": ttl,
    })) else {
        return false;
    };
    player_ref.try_send_client_packet(&CCustomPayload::new("moddedmc:loop_music", &metadata));
    if !refresh {
        player_ref.play_custom_sound(
            key,
            SoundCategory::Records,
            &player_ref.position(),
            volume,
            1.0,
        );
    }
    let mut lease = Lease::new(&player_ref, ttl);
    if refresh && let Some(previous) = music.get(&map_key) {
        lease.until = lease.until.max(previous.until);
    }
    music.insert(map_key, lease);
    true
}

unsafe extern "C" fn stop_music(raw: WorldRef, player: i32, key: Str) -> bool {
    // SAFETY: the world and key are borrowed for this call.
    let (Some(world), key) = (unsafe { world(raw) }, unsafe { key.str() }) else {
        return false;
    };
    let Some(player_ref) = world.get_player_by_id(player) else {
        return false;
    };
    let mut music = crate::lock(&MUSIC);
    let map_key = (player, key.to_owned());
    if music
        .get(&map_key)
        .is_some_and(|lease| lease.matches(&player_ref))
    {
        music.remove(&map_key);
        stop_track(&player_ref, key);
    }
    true
}

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
    boss_bar,
    remove_boss_bar,
    loop_music,
    stop_music,
    block_animation: crate::content::block_animation,
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
