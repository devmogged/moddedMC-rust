use moddedmc_api::{
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{Entity, EntityView},
};

use crate::{CINDERVANE, EFFECTS, MOBS};

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
            0.65,
            1.2,
        ))
    } else if cfg!(feature = "original-assets") {
        vocal(kind, false)
    } else {
        None
    };
    if let Some((sound, volume, pitch)) = sound {
        play(world, dragon, sound, volume, pitch);
    }
}

pub fn eat(world: World<'_>, dragon: Entity<'_>) {
    if MOBS.get().and_then(|mobs| mobs.kind(dragon)) == Some(crate::wild::VARASUCHUS) {
        // `VarasuchusInteractionHandler.playEatSound`.
        play(
            world,
            dragon,
            event("entity.generic.eat", "saintsdragons:varasuchus_eat"),
            1.0,
            if crate::eggs::is_juvenile(dragon) {
                1.6
            } else {
                1.0
            },
        );
        return;
    }
    play(
        world,
        dragon,
        event("entity.generic.eat", "saintsdragons:cindervane_eat"),
        0.9,
        0.8,
    );
}

pub fn death(world: World<'_>, dragon: Entity<'_>) {
    let Some(kind) = MOBS.get().and_then(|mobs| mobs.kind(dragon)) else {
        return;
    };
    let sound = if kind == CINDERVANE {
        Some((
            event("entity.ender_dragon.death", "saintsdragons:cindervane_die"),
            0.65,
            1.2,
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
        play(world, dragon, sound, volume, pitch);
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
