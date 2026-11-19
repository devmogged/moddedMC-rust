//! `moddedmc:effects` v1: sounds and particles.

use moddedmc_api::abi::{self, Str, WorldRef};
use pumpkin_data::{
    data_component_impl::{IdOr, SoundEvent},
    particle::Particle,
    sound::{Sound, SoundCategory},
};
use pumpkin_util::math::vector3::Vector3;

use crate::content::world;

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
    let vanilla = name
        .strip_prefix("minecraft:")
        .or_else(|| (!name.contains(':')).then_some(name));
    let event = vanilla.and_then(Sound::from_name).map_or_else(
        || {
            IdOr::Value(SoundEvent {
                sound_name: name.to_owned(),
                range: None,
            })
        },
        IdOr::Id,
    );
    world.play_sound_event_fine(&event, category, &position, volume, pitch);
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

pub static EFFECTS: abi::EffectsV1 = abi::EffectsV1 {
    size: size_of::<abi::EffectsV1>(),
    play_sound,
    particles,
};
