//! Sounds, particles and mob effect admission.

use std::{mem::offset_of, sync::OnceLock};

use crate::{
    InterfacePrefix, InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World,
    entities::Entity, guard, s,
};

/// Return false to refuse a pending effect on a mod mob.
pub type EffectHandler = fn(World<'_>, Entity<'_>, &str, i32, u8) -> bool;
static EFFECT_HANDLER: OnceLock<EffectHandler> = OnceLock::new();

unsafe extern "C" fn effect_callback(
    world: abi::WorldRef,
    mob: abi::EntityRef,
    effect: abi::Str,
    duration: i32,
    amplifier: u8,
) -> bool {
    guard("mob effect", true, || {
        let (Some(handler), Some(world), Some(mob)) = (
            EFFECT_HANDLER.get(),
            World::borrowed(world),
            Entity::borrowed(mob),
        ) else {
            return true;
        };
        // SAFETY: the effect registry id is borrowed for this callback.
        handler(world, mob, unsafe { effect.str() }, duration, amplifier)
    })
}

pub use abi::{
    SOUND_AMBIENT, SOUND_BLOCKS, SOUND_MASTER, SOUND_NEUTRAL, SOUND_PLAYERS, SOUND_RECORDS,
};

#[derive(Clone, Copy)]
pub struct Effects(InterfacePrefix<abi::EffectsV1>);

/// Data carried by a particle. Block states are runtime ids from the content interface.
#[derive(Clone, Copy, Debug)]
pub enum ParticleOption<'a> {
    None,
    Item(&'a str),
    Block(u32),
    Dust { rgb: [u8; 3], scale: f32 },
}

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
    pub fn require_encounter_display(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::EffectsV1, stop_music))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:effects.stop_music",
                version: 1,
            })
    }

    /// Colors and styles are vanilla boss-event ordinals.
    #[allow(clippy::too_many_arguments)]
    pub fn boss_bar(
        self,
        world: World<'_>,
        player: i32,
        id: &str,
        title: &str,
        progress: f32,
        color: u8,
        style: u8,
        ttl: u32,
    ) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, boss_bar)) else {
            return false;
        };
        // SAFETY: table prefix checked; all data is borrowed for this call.
        unsafe {
            ((&raw const (*api).boss_bar).read())(
                world.raw(),
                player,
                s(id),
                s(title),
                progress,
                color,
                style,
                ttl,
            )
        }
    }

    pub fn remove_boss_bar(self, world: World<'_>, player: i32, id: &str) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, remove_boss_bar)) else {
            return false;
        };
        // SAFETY: table prefix checked; all data is borrowed for this call.
        unsafe { ((&raw const (*api).remove_boss_bar).read())(world.raw(), player, s(id)) }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn loop_music(
        self,
        world: World<'_>,
        player: i32,
        key: &str,
        volume: f32,
        fade_in: f32,
        fade_out: f32,
        ttl: u32,
    ) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, loop_music)) else {
            return false;
        };
        // SAFETY: table prefix checked; all data is borrowed for this call.
        unsafe {
            ((&raw const (*api).loop_music).read())(
                world.raw(),
                player,
                s(key),
                volume,
                fade_in,
                fade_out,
                ttl,
            )
        }
    }

    pub fn stop_music(self, world: World<'_>, player: i32, key: &str) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, stop_music)) else {
            return false;
        };
        // SAFETY: table prefix checked; all data is borrowed for this call.
        unsafe { ((&raw const (*api).stop_music).read())(world.raw(), player, s(key)) }
    }

    pub fn require_block_animation(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::EffectsV1, block_animation))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:effects.block_animation",
                version: 1,
            })
    }

    /// Select a clip from the block overlay's companion rig descriptor.
    pub fn block_animation(self, world: World<'_>, pos: Pos, clip: &str) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, block_animation)) else {
            return false;
        };
        // SAFETY: the table prefix was checked and the clip is borrowed for this call.
        unsafe { ((&raw const (*api).block_animation).read())(world.raw(), pos, s(clip)) }
    }

    /// Require timed entity audio and carrier sound control.
    pub fn require_entity_audio(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::EffectsV1, set_entity_silent))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:effects.set_entity_silent",
                version: 1,
            })
    }

    /// Start a sound following the entity, stopping after duration ticks or on removal.
    #[allow(clippy::too_many_arguments)]
    pub fn start_entity_audio(
        self,
        world: World<'_>,
        entity: i32,
        key: &str,
        source: u8,
        volume: f32,
        pitch: f32,
        duration: u32,
        looping: bool,
    ) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, start_entity_audio)) else {
            return false;
        };
        // SAFETY: the table prefix was checked and the key is borrowed for this call.
        unsafe {
            ((&raw const (*api).start_entity_audio).read())(
                world.raw(),
                entity,
                s(key),
                source,
                volume,
                pitch,
                duration,
                looping,
            )
        }
    }

    pub fn stop_entity_audio(self, world: World<'_>, entity: i32, key: &str) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, stop_entity_audio)) else {
            return false;
        };
        // SAFETY: the table prefix was checked and the key is borrowed for this call.
        unsafe { ((&raw const (*api).stop_entity_audio).read())(world.raw(), entity, s(key)) }
    }

    pub fn set_entity_silent(self, world: World<'_>, entity: i32, silent: bool) -> bool {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, set_entity_silent)) else {
            return false;
        };
        // SAFETY: the table prefix was checked.
        unsafe { ((&raw const (*api).set_entity_silent).read())(world.raw(), entity, silent) }
    }

    pub fn require_effect_flags(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::EffectsV1, has_effect))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:effects.has_effect",
                version: 1,
            })
    }

    /// Apply a vanilla effect with separate particle and icon visibility.
    #[allow(clippy::too_many_arguments)]
    pub fn add_effect_with_flags(
        self,
        entity: Entity<'_>,
        effect: &str,
        duration: i32,
        amplifier: u8,
        ambient: bool,
        show_particles: bool,
        show_icon: bool,
    ) -> bool {
        let Some(api) = self
            .0
            .upto(offset_of!(abi::EffectsV1, add_effect_with_flags))
        else {
            return false;
        };
        // SAFETY: prefix checked and both handles are borrowed for this call.
        unsafe {
            ((&raw const (*api).add_effect_with_flags).read())(
                entity.raw(),
                s(effect),
                duration,
                amplifier,
                ambient,
                show_particles,
                show_icon,
            )
        }
    }

    pub fn has_effect(self, entity: Entity<'_>, effect: &str) -> Option<bool> {
        let api = self.0.upto(offset_of!(abi::EffectsV1, has_effect))?;
        // SAFETY: prefix checked and both handles are borrowed for this call.
        Some(unsafe { ((&raw const (*api).has_effect).read())(entity.raw(), s(effect)) })
    }

    /// Require the appended mob effect admission callback.
    pub fn require_effect_veto(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::EffectsV1, on_effect))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:effects.on_effect",
                version: 1,
            })
    }

    /// Register the mod's shared effect handler for a mob kind, after registering the mob.
    pub fn on_effect(
        self,
        reg: &mut Registry<'_>,
        kind: &str,
        handler: EffectHandler,
    ) -> Result<(), RegistrationError> {
        let Some(api) = self.0.upto(offset_of!(abi::EffectsV1, on_effect)) else {
            return Err(RegistrationError::InvalidDefinition);
        };
        let _ = EFFECT_HANDLER.set(handler);
        // SAFETY: registration is live and the table size covers the appended function.
        RegistrationError::from_code(unsafe {
            ((&raw const (*api).on_effect).read())(reg.reg, s(kind), effect_callback)
        })
    }

    /// Require the appended parameterized-particle callback.
    pub fn require_particle_options(self) -> Result<(), InterfaceUnavailable> {
        self.0
            .upto(offset_of!(abi::EffectsV1, particles_with_options))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
                name: "moddedmc:effects.particles_with_options",
                version: 1,
            })
    }

    /// Send a simple, item, block/falling_dust, or dust particle. Viewer None broadcasts.
    /// False on older hosts or for invalid options or viewer.
    #[allow(clippy::too_many_arguments)]
    pub fn particles_with_options(
        self,
        world: World<'_>,
        particle: &str,
        position: [f64; 3],
        spread: [f64; 3],
        speed: f32,
        count: u32,
        option: ParticleOption<'_>,
        viewer: Option<i32>,
    ) -> bool {
        let Some(api) = self
            .0
            .upto(offset_of!(abi::EffectsV1, particles_with_options))
        else {
            return false;
        };
        if viewer.is_some_and(|id| id < 0) {
            return false;
        }
        let mut options = abi::ParticleOptions {
            item: abi::Str::EMPTY,
            kind: abi::PARTICLE_SIMPLE,
            block_state: 0,
            rgb: 0,
            scale: 0.0,
        };
        match option {
            ParticleOption::None => {}
            ParticleOption::Item(id) => {
                options.kind = abi::PARTICLE_ITEM;
                options.item = crate::s(id);
            }
            ParticleOption::Block(state) => {
                options.kind = abi::PARTICLE_BLOCK;
                options.block_state = state;
            }
            ParticleOption::Dust {
                rgb: [r, g, b],
                scale,
            } => {
                options.kind = abi::PARTICLE_DUST;
                options.rgb = u32::from_be_bytes([0, r, g, b]);
                options.scale = scale;
            }
        }
        // SAFETY: the checked table includes the callback; all references are borrowed for it.
        unsafe {
            ((&raw const (*api).particles_with_options).read())(
                world.raw(),
                crate::s(particle),
                abi::Vec3(position),
                abi::Vec3(spread),
                speed,
                count,
                options,
                viewer.unwrap_or(-1),
            )
        }
    }

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
        self.0
            .upto(std::mem::offset_of!(abi::EffectsV1, play_entity_sound))
            .map(|_| ())
            .ok_or(InterfaceUnavailable {
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
        let Some(api) = self
            .0
            .upto(std::mem::offset_of!(abi::EffectsV1, play_entity_sound))
        else {
            return false;
        };
        // SAFETY: the table reaches this callback and the world and ID are borrowed.
        unsafe {
            ((&raw const (*api).play_entity_sound).read())(
                world.raw(),
                entity,
                crate::s(sound),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_effects_prefix_has_no_particle_options() {
        let effects = Effects(InterfacePrefix {
            ptr: std::ptr::null(),
            size: offset_of!(abi::EffectsV1, particles_with_options),
        });
        assert!(effects.require_entity_sound().is_ok());
        assert!(effects.require_particle_options().is_err());
    }

    #[test]
    fn particle_options_prefix_has_no_effect_veto() {
        let effects = Effects(InterfacePrefix {
            ptr: std::ptr::null(),
            size: offset_of!(abi::EffectsV1, on_effect),
        });
        assert!(effects.require_particle_options().is_ok());
        assert!(effects.require_effect_veto().is_err());
    }

    #[test]
    fn effect_veto_prefix_has_no_encounter_display() {
        let effects = Effects(InterfacePrefix {
            ptr: std::ptr::null(),
            size: offset_of!(abi::EffectsV1, boss_bar),
        });
        assert!(effects.require_effect_veto().is_ok());
        assert!(effects.require_encounter_display().is_err());
    }

    #[test]
    fn block_animation_prefix_has_no_entity_audio() {
        let effects = Effects(InterfacePrefix {
            ptr: std::ptr::null(),
            size: offset_of!(abi::EffectsV1, start_entity_audio),
        });
        assert!(effects.require_block_animation().is_ok());
        assert!(effects.require_entity_audio().is_err());
    }

    #[test]
    fn entity_audio_prefix_has_no_effect_flags() {
        let effects = Effects(InterfacePrefix {
            ptr: std::ptr::null(),
            size: offset_of!(abi::EffectsV1, add_effect_with_flags),
        });
        assert!(effects.require_entity_audio().is_ok());
        assert!(effects.require_effect_flags().is_err());
    }
}
