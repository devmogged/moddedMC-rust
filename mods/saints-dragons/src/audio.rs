use moddedmc_api::{
    content::World,
    effects::{ParticleOption, SOUND_NEUTRAL},
    entities::{Entity, EntityView},
};

use crate::{CINDERVANE, EFFECTS, MOBS};
use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

fn event(stock: &'static str, original: &'static str) -> &'static str {
    if cfg!(feature = "original-assets") {
        original
    } else {
        stock
    }
}

fn vocal(kind: &str, death: bool) -> Option<(&'static str, f32, f32)> {
    let (hurt, die, hurt_volume, death_volume, death_pitch) = match kind {
        "saintsdragons:raevyx" => (
            "saintsdragons:raevyx_hurt",
            "saintsdragons:raevyx_die",
            1.2,
            1.5,
            0.95,
        ),
        "saintsdragons:stegonaut" => (
            "saintsdragons:stegonaut_hurt",
            "saintsdragons:stegonaut_die",
            1.2,
            1.2,
            1.0,
        ),
        "saintsdragons:varasuchus" => (
            "saintsdragons:varasuchus_hurt",
            "saintsdragons:varasuchus_die",
            1.2,
            1.35,
            0.9,
        ),
        "saintsdragons:ignivorus" => (
            "saintsdragons:ignivorus_hurt",
            "saintsdragons:ignivorus_die",
            1.2,
            1.8,
            0.9,
        ),
        "saintsdragons:volitans" => (
            "saintsdragons:volitans_hurt",
            "saintsdragons:volitans_die",
            1.3,
            1.6,
            1.0,
        ),
        "saintsdragons:nulljaw" => (
            "saintsdragons:nulljaw_hurt",
            "saintsdragons:nulljaw_die",
            1.2,
            1.2,
            1.0,
        ),
        "saintsdragons:atroxiia" => (
            "saintsdragons:atroxiia_hurt",
            "saintsdragons:atroxiia_die",
            1.2,
            1.5,
            1.0,
        ),
        "saintsdragons:latcher" | "saintsdragons:winged" | "saintsdragons:whettled" => (
            "saintsdragons:draconian_swarm_hurt",
            "saintsdragons:draconian_swarm_death",
            1.0,
            1.0,
            1.0,
        ),
        "saintsdragons:mossback" => (
            "saintsdragons:mossback_hurt",
            "saintsdragons:mossback_die",
            1.0,
            1.0,
            1.0,
        ),
        // Moop extends AbstractFish without overriding LivingEntity's hurt/death sounds.
        "saintsdragons:moop" => (
            "minecraft:entity.generic.hurt",
            "minecraft:entity.generic.death",
            1.0,
            1.0,
            1.0,
        ),
        _ => return None,
    };
    Some(if death {
        (die, death_volume, death_pitch)
    } else {
        (hurt, hurt_volume, 1.0)
    })
}

pub fn hurt(world: World<'_>, dragon: Entity<'_>, kind: &str) {
    let sound = if kind == CINDERVANE {
        Some((
            event("entity.ender_dragon.hurt", "saintsdragons:cindervane_hurt"),
            if cfg!(feature = "original-assets") {
                1.2
            } else {
                0.65
            },
            if cfg!(feature = "original-assets") {
                0.95 + random_float() * 0.1
            } else {
                1.2
            },
        ))
    } else if cfg!(feature = "original-assets") {
        vocal(kind, false).map(|(id, volume, pitch)| {
            (
                id,
                volume,
                if kind
                    .strip_prefix("saintsdragons:")
                    .and_then(ambient_profile)
                    .is_some()
                {
                    0.95 + random_float() * 0.1
                } else {
                    pitch
                },
            )
        })
    } else {
        None
    };
    if let Some((sound, volume, pitch)) = sound {
        if cfg!(feature = "original-assets")
            && kind
                .strip_prefix("saintsdragons:")
                .and_then(ambient_profile)
                .is_some()
        {
            moving(world, dragon, sound, volume, pitch);
        } else {
            play(world, dragon, sound, volume, pitch);
        }
    }
}

pub fn eat(world: World<'_>, dragon: Entity<'_>, food: &str) {
    // DragonHuntAndEatBehaviour.spawnEatingParticles: use the actual food's texture.
    if let (Some(effects), Some(view), Some(body)) = (EFFECTS.get(), dragon.view(), dragon.body()) {
        let [x, _, z] = view.position;
        let [min_x, min_y, min_z] = body.min;
        let [max_x, max_y, max_z] = body.max;
        let height = max_y - min_y;
        let horizontal = ((max_x - min_x).min(max_z - min_z) * 0.35).max(0.15);
        let vertical = (height * 0.25).max(0.15);
        effects.particles_with_options(
            world,
            "minecraft:item",
            [x, min_y + height * 0.65, z],
            [horizontal, vertical, horizontal],
            0.08,
            12,
            ParticleOption::Item(food),
            None,
        );
    }
    let name = MOBS
        .get()
        .and_then(|mobs| mobs.kind(dragon))
        .and_then(|kind| kind.strip_prefix("saintsdragons:"));
    if cfg!(feature = "original-assets")
        && let Some(name) = name
        && ambient_profile(name).is_some()
    {
        let pitch = if !matches!(name, "atroxiia" | "nulljaw") && crate::eggs::is_juvenile(dragon) {
            1.6
        } else {
            1.0
        };
        moving(
            world,
            dragon,
            &format!("saintsdragons:{name}_eat"),
            1.0,
            pitch,
        );
    } else {
        play(world, dragon, "entity.generic.eat", 0.9, 0.8);
    }
}

pub fn death(world: World<'_>, dragon: Entity<'_>) {
    forget(dragon.id());
    let Some(kind) = MOBS.get().and_then(|mobs| mobs.kind(dragon)) else {
        return;
    };
    let sound = if kind == CINDERVANE {
        Some((
            event("entity.ender_dragon.death", "saintsdragons:cindervane_die"),
            if cfg!(feature = "original-assets") {
                1.5
            } else {
                0.65
            },
            if cfg!(feature = "original-assets") {
                1.0
            } else {
                1.2
            },
        ))
    } else if cfg!(feature = "original-assets") {
        vocal(kind, true)
    } else {
        None
    };
    if let Some((sound, volume, pitch)) = sound {
        play(world, dragon, sound, volume, pitch);
    }
}

pub fn volley(world: World<'_>, dragon: Entity<'_>) {
    if !cfg!(feature = "original-assets") {
        play(world, dragon, "entity.ender_dragon.shoot", 0.9, 1.0);
    }
}

pub fn attack(world: World<'_>, dragon: Entity<'_>, attack: crate::abilities::Attack) {
    use crate::abilities::Attack;
    if cfg!(feature = "original-assets") {
        // Each released ability plays one cue when its attack animation starts.
        let (sound, volume, pitch) = match attack {
            Attack::Bite => ("saintsdragons:cindervane_bite", 1.0, 0.95),
            Attack::DoubleBite => ("saintsdragons:cindervane_double_bite", 1.2, 1.0),
            Attack::SlashGrab => ("saintsdragons:cindervane_slash", 1.0, 1.0),
            Attack::MagmaVolley => ("saintsdragons:cindervane_magma_volley", 2.0, 1.0),
        };
        moving(world, dragon, sound, volume, pitch);
    } else if attack != Attack::MagmaVolley {
        play(world, dragon, "entity.ender_dragon.growl", 0.8, 1.15);
    }
}

fn play(world: World<'_>, dragon: Entity<'_>, id: &str, volume: f32, pitch: f32) {
    if let (Some(effects), Some(view)) = (EFFECTS.get(), dragon.view()) {
        effects.play_sound(world, view.position, id, SOUND_NEUTRAL, volume, pitch);
    }
}

pub fn tick(world: World<'_>, id: i32, view: EntityView, now: u64) {
    if cfg!(feature = "original-assets") {
        return;
    }
    let phase = now.wrapping_add(id as u64);
    if !phase.is_multiple_of(20) {
        return;
    }
    let (sound, volume, pitch) = if !view.on_ground && view.motion[1] > 0.08 {
        (
            event("entity.ender_dragon.flap", "saintsdragons:cindervane_flap"),
            0.55,
            1.3,
        )
    } else if phase.is_multiple_of(200) {
        let grumble = match phase / 200 % 3 {
            0 => "saintsdragons:cindervane_grumble1",
            1 => "saintsdragons:cindervane_grumble2",
            _ => "saintsdragons:cindervane_grumble3",
        };
        (event("entity.ender_dragon.ambient", grumble), 0.45, 1.2)
    } else {
        return;
    };
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(world, view.position, sound, SOUND_NEUTRAL, volume, pitch);
    }
}
fn random_float() -> f32 {
    crate::ai::random(1 << 24) as f32 / (1 << 24) as f32
}

/// DragonSoundSpec.moving: the stock client tracks the emitter for the recording.
fn moving(world: World<'_>, mob: Entity<'_>, sound: &str, volume: f32, pitch: f32) {
    if let Some(effects) = EFFECTS.get()
        && !effects.play_entity_sound(world, mob.id(), sound, SOUND_NEUTRAL, volume, pitch)
    {
        play(world, mob, sound, volume, pitch);
    }
}

#[derive(Clone, Copy)]
struct Ambient {
    min: u64,
    max: u64,
    seed: bool,
    weights: [f32; 2],
    voices: [(f32, f32, f32); 3],
}
fn ambient_profile(name: &str) -> Option<Ambient> {
    let (min, max, seed, weights, voices) = match name {
        "cindervane" => (
            180,
            420,
            true,
            [0.45, 0.8],
            [(1.1, 0.98, 0.06), (1.2, 0.96, 0.08), (1.0, 1.0, 0.05)],
        ),
        "raevyx" => (200, 600, true, [0.4, 0.7], [(0.8, 0.95, 0.1); 3]),
        "stegonaut" => (200, 600, true, [0.4, 0.7], [(0.6, 1.1, 0.2); 3]),
        "varasuchus" => (200, 600, true, [0.4, 0.7], [(0.8, 0.95, 0.1); 3]),
        "ignivorus" => (
            180,
            520,
            false,
            [0.34, 0.67],
            [(1.1, 0.95, 0.08), (1.15, 1.0, 0.08), (1.2, 0.9, 0.08)],
        ),
        "volitans" => (
            220,
            420,
            false,
            [0.34, 0.67],
            [(1.0, 0.98, 0.08), (1.0, 0.98, 0.08), (1.0, 1.0, 0.06)],
        ),
        "nulljaw" => (220, 420, true, [0.34, 0.67], [(1.0, 1.0, 0.08); 3]),
        "atroxiia" => (
            200,
            600,
            true,
            [1.0 / 3.0, 2.0 / 3.0],
            [(1.0, 0.95, 0.1); 3],
        ),
        _ => return None,
    };
    Some(Ambient {
        min,
        max,
        seed,
        weights,
        voices,
    })
}
impl Ambient {
    fn delay(self, first: bool) -> u64 {
        self.min + crate::ai::random(self.max - self.min + 1)
            - if first && self.seed {
                crate::ai::random(80)
            } else {
                0
            }
    }
    fn voice(self, roll: f32) -> usize {
        if roll < self.weights[0] {
            0
        } else if roll < self.weights[1] {
            1
        } else {
            2
        }
    }
}

/// DragonEntity.tickAmbientVocalSounds resets its timer even when the selector
/// or DragonSoundHandler suppresses the vocal. The individual species decide
/// whether to pause the timer before reaching that method.
fn ambient_gate(name: &str, juvenile: bool, sitting: bool, targeting: bool) -> (bool, bool) {
    let pause = match name {
        "cindervane" | "atroxiia" => juvenile || targeting,
        "ignivorus" | "volitans" => juvenile || targeting || sitting,
        "varasuchus" => juvenile || sitting,
        _ => false,
    };
    let mute = juvenile
        || (sitting && name != "nulljaw")
        || (targeting && matches!(name, "stegonaut" | "raevyx" | "varasuchus"));
    (pause, mute)
}

fn ambient_pitch(name: &str, base: f32, variance: f32, roll: f32) -> f32 {
    // NulljawSoundProfile alone centers its pitch variance around zero.
    base + if name == "nulljaw" { roll - 0.5 } else { roll } * variance
}

#[derive(Default)]
struct AudioState {
    remaining: u64,
    clip: &'static str,
    clip_tick: u64,
    last: u64,
    on_ground: Option<bool>,
    swarm_ambient: i32,
}
static AUDIO: Mutex<Option<HashMap<i32, AudioState>>> = Mutex::new(None);
fn forget(id: i32) {
    if let Some(all) = AUDIO
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.remove(&id);
    }
}
pub fn sweep(now: u64) {
    if let Some(all) = AUDIO
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
    {
        all.retain(|_, state| now.saturating_sub(state.last) < 200);
    }
}

/// Released adult movement animation sound_effects. Seconds converted to nearest server tick.
/// shortcut: the stock client has no animation callback or sub-tick playback; use the selected server clip.
fn movement_cues(name: &str, clip: &str) -> Option<(u64, &'static [u64])> {
    Some(match (name, clip) {
        ("cindervane", "run") => (24, &[0, 13]),
        ("cindervane", "flap") => (60, &[18]),
        ("cindervane", "fly_idle") => (120, &[0, 56]),
        ("cindervane", "sprint_flap") => (180, &[12, 46, 83, 118, 153]),
        ("raevyx", "walk") => (33, &[1, 18]),
        ("raevyx", "run") => (25, &[1, 13]),
        ("raevyx", "flap") => (144, &[14, 50, 87, 123]),
        ("raevyx", "fly_idle") => (144, &[14, 51, 88, 124]),
        ("raevyx", "sprint_flap") => (144, &[4, 27, 48, 69, 89, 110, 129]),
        ("ignivorus", "walk") => (50, &[0, 18, 28, 43]),
        ("ignivorus", "run") => (25, &[0, 13]),
        ("ignivorus", "flap") => (240, &[15, 65, 113, 160, 208]),
        ("ignivorus", "fly_idle") => (42, &[7]),
        ("ignivorus", "sprint_flap") => (192, &[12, 37, 61, 85, 109, 133, 156, 180]),
        ("ignivorus", "phase2_walk") => (42, &[1, 22]),
        ("ignivorus", "phase2_run") => (33, &[1, 18]),
        ("ignivorus", "bulldozing") => (27, &[0, 13]),
        ("volitans", "walk") => (33, &[0, 17]),
        ("volitans", "run") => (25, &[0, 4]),
        ("volitans", "flap") => (194, &[0, 29, 55, 83, 112, 141, 168]),
        ("volitans", "fly_idle") => (180, &[2, 27, 52, 77, 102, 126, 154]),
        ("volitans", "sprint_flap") => (96, &[0, 25, 49, 72]),
        ("stegonaut", "walk") => (33, &[0, 17]),
        ("stegonaut", "run") => (20, &[0, 12]),
        ("varasuchus", "walk") => (50, &[0, 13, 25, 38]),
        ("varasuchus", "run") => (33, &[0, 16]),
        ("varasuchus", "run2") => (27, &[0, 13]),
        ("atroxiia", "walk") => (50, &[0, 23]),
        ("atroxiia", "run") => (25, &[0, 13]),
        ("mossback", "walk") => (15, &[0, 7]),
        _ => return None,
    })
}

/// Server-side timing for DragonEntity.tickAmbientVocalSounds and movement clips.
pub fn tick_clip(
    world: World<'_>,
    mob: Entity<'_>,
    name: &str,
    view: EntityView,
    clip: &'static str,
    now: u64,
) {
    if !cfg!(feature = "original-assets") {
        return;
    }
    let juvenile = name.starts_with("baby_");
    let name = name.strip_prefix("baby_").unwrap_or(name);
    let profile = ambient_profile(name);
    let swarm = matches!(name, "latcher" | "winged" | "whettled");
    if profile.is_none() && !swarm && name != "mossback" {
        return;
    }
    let sleeping =
        matches!(name, "ignivorus" | "varasuchus") && crate::roost::sleep_clip(mob.id()).is_some();
    let riding = matches!(name, "cindervane" | "volitans" | "nulljaw")
        && MOBS.get().is_some_and(|m| !m.passengers(mob).is_empty());
    let targeting =
        profile.is_some() && crate::MOB_AI.get().and_then(|ai| ai.target(mob)).is_some();
    let sitting = profile.is_some() && crate::taming::order_name(mob) == "stay";
    let (pause_vocal, mute_vocal) = ambient_gate(name, juvenile, sitting, targeting);
    let cindervane_attack = name == "cindervane"
        && (crate::combat::active_attack(mob).is_some() || crate::combat::fire_active(mob));
    let tick_vocal = !pause_vocal
        && !sleeping
        && !riding
        && !cindervane_attack
        && clip != "die"
        && !(matches!(name, "cindervane" | "volitans")
            && matches!(clip, "takeoff" | "landing" | "landed"))
        && !(name == "ignivorus" && matches!(clip, "bulldozing" | "leap"));
    let (voice, cue, transition, swarm_voice) = {
        let mut all = AUDIO.lock().unwrap_or_else(PoisonError::into_inner);
        let state = all.get_or_insert_default().entry(mob.id()).or_default();
        if state.on_ground.is_some() && state.last == now {
            return;
        }
        let voice = profile.and_then(|p| {
            if state.remaining == 0 {
                state.remaining = p.delay(true);
            }
            if !tick_vocal {
                return None;
            }
            state.remaining -= 1;
            if state.remaining != 0 {
                return None;
            }
            state.remaining = p.delay(false);
            if mute_vocal || clip == "hurt" {
                return None;
            }
            let i = p.voice(random_float());
            Some((i, p.voices[i]))
        });
        if state.clip != clip || now.saturating_sub(state.last) > 1 {
            state.clip = clip;
            state.clip_tick = 0;
        } else if state.last != now {
            state.clip_tick += 1;
        }
        let cue = !juvenile
            && movement_cues(name, clip)
                .is_some_and(|(period, ticks)| ticks.contains(&(state.clip_tick % period)));
        let transition = state
            .on_ground
            .filter(|old| *old != view.on_ground)
            .map(|_| view.on_ground);
        // AbstractDraconianSwarmEntity.getAmbientSound uses the vanilla Mob timer.
        let swarm_voice = if swarm {
            let due = (crate::ai::random(1000) as i32) < state.swarm_ambient;
            state.swarm_ambient = if due { -80 } else { state.swarm_ambient + 1 };
            due
        } else {
            false
        };
        state.on_ground = Some(view.on_ground);
        state.last = now;
        (voice, cue, transition, swarm_voice)
    };
    if swarm_voice && let Some(effects) = EFFECTS.get() {
        // SoundSource.HOSTILE is ordinal 5.
        effects.play_sound(
            world,
            view.position,
            &format!("saintsdragons:draconian_swarm_idle{}", crate::ai::random(3)),
            5,
            1.0,
            1.0,
        );
    }
    if let Some((i, (volume, pitch, variance))) = voice {
        let separator = if name == "raevyx" { "_" } else { "" };
        moving(
            world,
            mob,
            &format!("saintsdragons:{name}_grumble{separator}{}", i + 1),
            volume,
            ambient_pitch(name, pitch, variance, random_float()),
        );
    }
    if cue {
        let flap = matches!(clip, "flap" | "fly_idle" | "sprint_flap");
        if flap {
            let (volume, pitch) = match name {
                "cindervane" => (1.1, 0.98 + (random_float() - 0.5) * 0.1),
                "ignivorus" => (1.2, 0.95 + (random_float() - 0.5) * 0.1),
                "volitans" => (1.0, 0.96 + (random_float() - 0.5) * 0.1),
                "raevyx" => {
                    let speed = view.motion[0].hypot(view.motion[2]) as f32;
                    ((0.9 + speed * 0.2).max(0.6), 1.0 + speed * 0.3)
                }
                _ => return,
            };
            play(
                world,
                mob,
                &format!("saintsdragons:{name}_flap"),
                volume,
                pitch,
            );
        } else {
            // DragonEntity.playAnimationKeyframeSound plays steps at volume and pitch 1.
            play(world, mob, &format!("saintsdragons:{name}_step"), 1.0, 1.0);
        }
    }
    if let Some(landed) = transition {
        // shortcut: ground contact is the available flight transition signal for roster carriers.
        let params = match (name, landed) {
            ("cindervane" | "raevyx", true) => Some((1.0, 1.0)),
            ("cindervane", false) if clip == "takeoff" => Some((1.2, 1.0)),
            ("raevyx", false) if view.motion[1] > 0.08 => Some((1.2, 0.94 + random_float() * 0.12)),
            ("ignivorus", true) => Some((1.0, 1.0)),
            ("ignivorus", false) if view.motion[1] > 0.08 => Some((1.0, 1.0)),
            ("volitans", _) => Some((2.0, 1.0)),
            _ => None,
        };
        if let Some((volume, pitch)) = params
            && !juvenile
        {
            let suffix = if landed { "landed" } else { "takeoff" };
            moving(
                world,
                mob,
                &format!("saintsdragons:{name}_{suffix}"),
                volume,
                pitch,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ambient_delays_and_weight_boundaries() {
        let p = ambient_profile("cindervane").unwrap();
        for _ in 0..100 {
            assert!((101..=420).contains(&p.delay(true)));
            assert!((180..=420).contains(&p.delay(false)));
        }
        assert_eq!([p.voice(0.0), p.voice(0.45), p.voice(0.8)], [0, 1, 2]);
        assert!(ambient_profile("moop").is_none());
    }
    #[test]
    fn ambient_gates_distinguish_paused_timers_from_muted_vocals() {
        assert_eq!(ambient_gate("stegonaut", false, true, false), (false, true));
        assert_eq!(ambient_gate("stegonaut", false, false, true), (false, true));
        assert_eq!(ambient_gate("atroxiia", false, false, true), (true, false));
        assert_eq!(ambient_gate("nulljaw", false, true, false), (false, false));
        assert_eq!(ambient_gate("nulljaw", true, false, false), (false, true));
        assert_eq!(
            ambient_gate("varasuchus", false, false, true),
            (false, true)
        );
        assert!((ambient_pitch("nulljaw", 1.0, 0.08, 0.0) - 0.96).abs() < 0.0001);
        assert!((ambient_pitch("raevyx", 1.0, 0.08, 0.0) - 1.0).abs() < 0.0001);
    }
    #[test]
    fn released_keyframes_use_clip_time() {
        assert_eq!(
            movement_cues("ignivorus", "walk"),
            Some((50, &[0, 18, 28, 43][..]))
        );
        assert_eq!(
            movement_cues("raevyx", "flap"),
            Some((144, &[14, 50, 87, 123][..]))
        );
        assert_eq!(movement_cues("raevyx", "idle"), None);
        assert_eq!(
            movement_cues("volitans", "flap"),
            Some((194, &[0, 29, 55, 83, 112, 141, 168][..]))
        );
        assert_eq!(
            vocal("saintsdragons:moop", false).unwrap().0,
            "minecraft:entity.generic.hurt"
        );
    }
}
