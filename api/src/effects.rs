//! Sounds and particles.

use std::mem::offset_of;

use crate::{InterfacePrefix, InterfaceUnavailable, abi, content::World};

pub use abi::{
    SOUND_AMBIENT, SOUND_BLOCKS, SOUND_MASTER, SOUND_NEUTRAL, SOUND_PLAYERS, SOUND_RECORDS,
};

#[derive(Clone, Copy)]
pub struct Effects(InterfacePrefix<abi::EffectsV1>);

pub fn require() -> Result<Effects, InterfaceUnavailable> {
    // SAFETY: the size check covers the published v1 prefix.
    unsafe {
        InterfacePrefix::require(
            abi::EFFECTS,
            1,
            offset_of!(abi::EffectsV1, play_entity_sound),
        )
    }
    .map(Effects)
}

impl Effects {
    /// Vanilla `Level.playSound(null, ...)`. `source` is a `SOUND_*` id.
    pub fn play_sound(
        self,
        world: World<'_>,
        position: [f64; 3],
        sound: &str,
        source: u8,
        volume: f32,
        pitch: f32,
    ) -> bool {
        // SAFETY: the world and id are borrowed for the call.
        unsafe {
            ((&raw const (*self.0.ptr).play_sound).read())(
                world.raw(),
                abi::Vec3(position),
                abi::Str::new(sound.as_bytes()),
                source,
                volume,
                pitch,
            )
        }
    }

    /// Require the appended entity-sound callback.
    pub fn require_entity_sound(self) -> Result<(), InterfaceUnavailable> {
        self.0.full().map(|_| ()).ok_or(InterfaceUnavailable {
            name: "moddedmc:effects.play_entity_sound",
            version: 1,
        })
    }

    /// Follow an entity's position. Stock Java clients stop the sound when it is removed.
    pub fn play_entity_sound(
        self,
        world: World<'_>,
        entity: i32,
        sound: &str,
        source: u8,
        volume: f32,
        pitch: f32,
    ) -> bool {
        let Some(api) = self.0.full() else {
            return false;
        };
        // SAFETY: the full table includes this callback and the world and ID are borrowed.
        unsafe {
            (api.play_entity_sound)(world.raw(), entity, crate::s(sound), source, volume, pitch)
        }
    }

    /// Vanilla `ServerLevel.sendParticles` for a particle without options.
    pub fn particles(
        self,
        world: World<'_>,
        particle: &str,
        position: [f64; 3],
        spread: [f64; 3],
        speed: f32,
        count: u32,
    ) -> bool {
        // SAFETY: the world and id are borrowed for the call.
        unsafe {
            ((&raw const (*self.0.ptr).particles).read())(
                world.raw(),
                abi::Str::new(particle.as_bytes()),
                abi::Vec3(position),
                abi::Vec3(spread),
                speed,
                count,
            )
        }
    }
}
