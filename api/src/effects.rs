//! Sounds and particles.

use crate::{InterfaceUnavailable, abi, content::World, require_interface};

pub use abi::{SOUND_AMBIENT, SOUND_BLOCKS, SOUND_MASTER, SOUND_NEUTRAL, SOUND_PLAYERS};

#[derive(Clone, Copy)]
pub struct Effects(&'static abi::EffectsV1);

pub fn require() -> Result<Effects, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:effects v1.
    unsafe { require_interface(abi::EFFECTS, 1) }.map(Effects)
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
            (self.0.play_sound)(
                world.raw(),
                abi::Vec3(position),
                abi::Str::new(sound.as_bytes()),
                source,
                volume,
                pitch,
            )
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
            (self.0.particles)(
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
