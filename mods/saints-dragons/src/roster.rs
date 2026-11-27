//! The other 0.9.88 dragon entities and their baseline movement.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    commands::{Argument, Node},
    content::World,
    entities::{Entity, EntityView},
    interaction::{UseOnBlock, UseResult},
    items::Item,
    mob_ai::{Goal, goal_control, goal_phase},
    mobs::Mob,
};

use crate::{
    COMMANDS, ENTITIES, INTERACTION, ITEMS, MOB_AI, MOBS, ai, flight, swarm, taming, visuals,
};

pub const AMBIENT: u64 = 9;
const SPAWN_TAG: u64 = 100;

struct Species {
    name: &'static str,
    display: &'static str,
    carrier: &'static str,
    health: f64,
    speed: f64,
    walk: &'static str,
    flight: Option<(&'static str, &'static str)>,
    swim: Option<&'static str>,
    aquatic: bool,
}

// Adult health and ground movement speed come from the release's stat profiles.
const SPECIES: [Species; 13] = [
    Species {
        name: "raevyx",
        display: "Raevyx",
        carrier: "ravager",
        health: 180.0,
        speed: 0.30,
        walk: "walk",
        flight: Some(("flap", "fly_glide")),
        swim: None,
        aquatic: false,
    },
    Species {
        name: "stegonaut",
        display: "Stegonaut",
        carrier: "ravager",
        health: 100.0,
        speed: 0.28,
        walk: "walk",
        flight: None,
        swim: None,
        aquatic: false,
    },
    Species {
        name: "varasuchus",
        display: "Varasuchus",
        carrier: "ravager",
        health: 200.0,
        speed: 0.33,
        walk: "walk",
        flight: None,
        swim: Some("swim_move"),
        aquatic: false,
    },
    Species {
        name: "ignivorus",
        display: "Ignivorus",
        carrier: "ravager",
        health: 450.0,
        speed: 0.30,
        walk: "walk",
        flight: Some(("flap", "glide")),
        swim: None,
        aquatic: false,
    },
    Species {
        name: "volitans",
        display: "Volitans",
        carrier: "ravager",
        health: 160.0,
        speed: 0.30,
        walk: "walk",
        flight: Some(("flap", "fly_glide")),
        swim: Some("swim"),
        aquatic: false,
    },
    Species {
        name: "nulljaw",
        display: "Nulljaw",
        carrier: "spider",
        health: 70.0,
        speed: 0.20,
        walk: "hover",
        flight: Some(("hover", "hover")),
        swim: None,
        aquatic: false,
    },
    Species {
        name: "atroxiia",
        display: "Atroxiia",
        carrier: "ravager",
        health: 200.0,
        speed: 0.33,
        walk: "walk",
        flight: None,
        swim: Some("swim"),
        aquatic: false,
    },
    Species {
        name: "moop",
        display: "Moop",
        carrier: "cod",
        health: 3.0,
        speed: 0.8,
        walk: "on_land",
        flight: None,
        swim: Some("swim"),
        aquatic: true,
    },
    Species {
        name: "mossback",
        display: "Mossback",
        carrier: "wolf",
        health: 10.0,
        speed: 0.15,
        walk: "walk",
        flight: None,
        swim: None,
        aquatic: false,
    },
    Species {
        name: "latcher",
        display: "Latcher",
        carrier: "spider",
        health: 12.0,
        speed: 0.25,
        walk: "move",
        flight: None,
        swim: None,
        aquatic: false,
    },
    Species {
        name: "winged",
        display: "Winged",
        carrier: "ravager",
        health: 6.0,
        speed: 0.50,
        walk: "move",
        flight: None,
        swim: None,
        aquatic: false,
    },
    Species {
        name: "whettled",
        display: "Whettled",
        carrier: "ravager",
        health: 16.0,
        speed: 0.30,
        walk: "idleandslowmove",
        flight: None,
        swim: None,
        aquatic: false,
    },
    Species {
        name: crate::ivy::SPECIES,
        display: crate::ivy::NAME,
        carrier: "villager",
        health: 20.0,
        speed: 0.35,
        walk: "walk",
        flight: None,
        swim: None,
        aquatic: false,
    },
];

pub const SWARM_EGG: &str = "draconian_swarm";

/// The current carrier melee bases (ravager 12, spider 2), with released
/// attributes for Varasuchus and Ignivorus's age-dependent growth profile.
pub fn melee_base(name: &str, juvenile: bool) -> Option<f64> {
    match name {
        "ignivorus" => Some(if juvenile { 0.0 } else { 50.0 }),
        "varasuchus" => Some(10.0),
        "raevyx" | "volitans" => Some(12.0),
        "stegonaut" => Some(2.0),
        "atroxiia" => Some(10.0),
        "nulljaw" => Some(2.0),
        _ => None,
    }
}

impl Species {
    /// Release createAttributes and dragon_attribute config defaults. A carrier can have
    /// attributes the Java creature lacks; explicitly neutralize those instead of inheriting
    /// the ravager/spider's combat statistics.
    fn attributes(&self) -> Vec<(&'static str, f64)> {
        let (armor, range, knockback, attack, flying) = match self.name {
            "raevyx" => (8.0, 64.0, 1.0, 0.0, Some(0.5)),
            "stegonaut" => (15.0, 32.0, 1.0, 2.0, None),
            "varasuchus" => (8.0, 32.0, 1.0, 10.0, None),
            "ignivorus" => (4.0, 128.0, 2.0, 50.0, Some(0.35)),
            "volitans" => (6.0, 64.0, 0.0, 0.0, Some(0.38)),
            "nulljaw" => (4.0, 48.0, 0.0, 0.0, Some(0.42)),
            "atroxiia" => (10.0, 32.0, 1.0, 10.0, None),
            "latcher" | "winged" | "whettled" => (
                0.0,
                64.0,
                0.0,
                swarm::damage(self.name).unwrap_or(0.0),
                None,
            ),
            crate::ivy::SPECIES => (0.0, 32.0, 0.0, 4.0, None),
            _ => (0.0, 16.0, 0.0, 0.0, None),
        };
        let mut attributes = vec![
            ("minecraft:max_health", self.health),
            ("minecraft:movement_speed", self.speed),
            ("minecraft:armor", armor),
            ("minecraft:follow_range", range),
            ("minecraft:knockback_resistance", knockback),
            ("minecraft:attack_damage", attack),
        ];
        if let Some(speed) = flying {
            attributes.push(("minecraft:flying_speed", speed));
        }
        attributes
    }

    fn clip(&self, view: EntityView, wet: bool) -> &'static str {
        match self.name {
            "latcher" | "winged" => {
                return if flight::length_squared(view.motion) > 0.003 {
                    self.walk
                } else {
                    "idle"
                };
            }
            "whettled" => return self.walk,
            _ => {}
        }
        if self.aquatic {
            if !wet {
                return self.walk;
            }
            let speed_squared = view.motion[0].powi(2) + view.motion[2].powi(2);
            return if speed_squared > 0.01 {
                "swim_fast"
            } else if speed_squared > 0.0004 {
                "swim"
            } else {
                "idle"
            };
        }
        if wet && let Some(swim) = self.swim {
            return swim;
        }
        let moving = view.motion[0].hypot(view.motion[2]) > 0.035;
        if !view.on_ground
            && let Some((flap, cruise)) = self.flight
        {
            return if view.motion[1] > 0.1 { flap } else { cruise };
        }
        if moving { self.walk } else { "idle" }
    }
}

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let missing = RegistrationError::InvalidDefinition;
    let (mobs, items, ai, commands) = (
        MOBS.get().ok_or(missing)?,
        ITEMS.get().ok_or(missing)?,
        MOB_AI.get().ok_or(missing)?,
        COMMANDS.get().ok_or(missing)?,
    );
    for species in &SPECIES {
        let kind = format!("saintsdragons:{}", species.name);
        let swarm_damage = swarm::damage(species.name);
        let mut attributes = species.attributes();
        if species.name == crate::ivy::SPECIES {
            attributes.push(("minecraft:armor_toughness", 0.0));
        }
        if species.name == "mossback" {
            attributes.push(("minecraft:armor", 0.0));
            items.add_item(
                registry,
                &Item {
                    id: "saintsdragons:baby_mossback_display",
                    display_name: "Mossback",
                    carrier: "minecraft:paper",
                    model: "minecraft:item/pufferfish",
                },
            )?;
        }
        mobs.add_mob(
            registry,
            &Mob {
                id: &kind,
                carrier: &format!("minecraft:{}", species.carrier),
                attributes: &attributes,
            },
        )?;
        mobs.on_death(registry, &kind, taming::death)?;
        if species.name == crate::ivy::SPECIES {
            mobs.on_merchant_close(registry, &kind, crate::ivy::merchant_close)?;
        }
        let stun = crate::stun::uses_stun(species.name);
        if swarm_damage.is_some()
            || stun
            || species.name == "stegonaut"
            || species.name == "varasuchus"
            || species.name == crate::ivy::SPECIES
            || species.name == "mossback"
            || cfg!(feature = "original-assets")
        {
            ai.on_hurt(registry, &kind, crate::combat::hurt)?;
        }
        if matches!(species.name, crate::ivy::SPECIES | "varasuchus") {
            ai.on_hurt_final(registry, &kind, crate::combat::hurt_final)?;
        }
        if stun {
            ai.add_goal(
                registry,
                &Goal {
                    kind: &kind,
                    key: crate::stun::GOAL,
                    priority: 0,
                    controls: goal_control::MOVE | goal_control::LOOK | goal_control::JUMP,
                },
            )?;
        }
        if species.name == "atroxiia" {
            crate::combat_atroxiia::register(registry)?;
        }
        if swarm_damage.is_none() && !matches!(species.name, "moop" | "mossback") {
            INTERACTION
                .get()
                .ok_or(missing)?
                .on_use_mob(registry, &kind, taming::interact)?;
            for (key, priority, controls) in [
                (
                    taming::RIDE_OR_SIT,
                    0,
                    goal_control::MOVE | goal_control::LOOK | goal_control::JUMP,
                ),
                (taming::FOLLOW, 2, goal_control::MOVE),
            ]
            .into_iter()
            .chain(crate::breed::breeds(species.name).then_some((
                crate::breed::GOAL,
                1,
                goal_control::MOVE | goal_control::LOOK,
            )))
            .chain(crate::hunt::supported(species.name).then_some((
                crate::hunt::GOAL,
                1,
                goal_control::MOVE | goal_control::LOOK,
            )))
            .chain((species.name == "varasuchus").then_some((
                crate::wild::GOAL,
                0,
                goal_control::MOVE | goal_control::LOOK | goal_control::JUMP,
            ))) {
                ai.add_goal(
                    registry,
                    &Goal {
                        kind: &kind,
                        key,
                        priority,
                        controls,
                    },
                )?;
            }
        }
        items.add_item(
            registry,
            &Item {
                id: &format!("{kind}_display"),
                display_name: species.display,
                carrier: "minecraft:paper",
                model: if species.aquatic {
                    "minecraft:item/cod"
                } else if species.name == crate::ivy::SPECIES {
                    "minecraft:item/villager_spawn_egg"
                } else {
                    "minecraft:item/dragon_head"
                },
            },
        )?;
        if species.name == "moop" {
            crate::moop::register(registry)?;
        } else if !species.aquatic {
            ai.replace_goals(registry, &kind)?;
            if species.name == "mossback" {
                INTERACTION
                    .get()
                    .ok_or(missing)?
                    .on_use_mob(registry, &kind, taming::interact)?;
                for (key, priority, controls) in [
                    (crate::mossback::FLOAT, 0, goal_control::JUMP),
                    (crate::mossback::PANIC, 1, goal_control::MOVE),
                    (
                        crate::mossback::BREED,
                        2,
                        goal_control::MOVE | goal_control::LOOK,
                    ),
                    (crate::mossback::WANDER, 4, goal_control::MOVE),
                    (ai::IDLE_LOOK, 5, goal_control::LOOK),
                ] {
                    ai.add_goal(
                        registry,
                        &Goal {
                            kind: &kind,
                            key,
                            priority,
                            controls,
                        },
                    )?;
                }
            } else {
                if swarm_damage.is_some() {
                    for (key, priority, controls) in [
                        (swarm::TARGETING, 1, goal_control::TARGET),
                        (swarm::COMBAT, 2, goal_control::MOVE | goal_control::LOOK),
                    ] {
                        ai.add_goal(
                            registry,
                            &Goal {
                                kind: &kind,
                                key,
                                priority,
                                controls,
                            },
                        )?;
                    }
                }
                for (key, priority, controls) in [
                    (ai::IDLE_LOOK, 3, goal_control::LOOK),
                    (
                        if swarm_damage.is_some() {
                            swarm::WANDER
                        } else {
                            ai::GROUND_WANDER
                        },
                        if swarm_damage.is_some() { 7 } else { 4 },
                        goal_control::MOVE,
                    ),
                ] {
                    ai.add_goal(
                        registry,
                        &Goal {
                            kind: &kind,
                            key,
                            priority,
                            controls,
                        },
                    )?;
                }
            }
        }
        if cfg!(feature = "original-assets") && matches!(species.name, "ignivorus" | "varasuchus") {
            ai.add_goal(
                registry,
                &Goal {
                    kind: &kind,
                    key: crate::roost::GOAL,
                    priority: 1,
                    controls: goal_control::MOVE | goal_control::LOOK,
                },
            )?;
        }
        if species.name == "ignivorus" {
            for (key, priority, controls) in [
                (crate::combat_ignivorus::TARGETING, 0, goal_control::TARGET),
                (
                    crate::combat_ignivorus::COMBAT,
                    1,
                    goal_control::MOVE | goal_control::LOOK,
                ),
            ] {
                ai.add_goal(
                    registry,
                    &Goal {
                        kind: &kind,
                        key,
                        priority,
                        controls,
                    },
                )?;
            }
        }
        if species.name == "stegonaut" {
            crate::combat_stegonaut::register(registry)?;
        }
        if species.flight.is_some() {
            ai.add_goal(
                registry,
                &Goal {
                    kind: &kind,
                    key: flight::GOAL,
                    priority: 2,
                    controls: goal_control::MOVE | goal_control::LOOK,
                },
            )?;
        }
        if species.name == "volitans" {
            for (key, controls) in [
                (crate::combat_volitans::TARGET, goal_control::TARGET),
                (
                    crate::combat_volitans::COMBAT,
                    goal_control::MOVE | goal_control::LOOK,
                ),
            ] {
                ai.add_goal(
                    registry,
                    &Goal {
                        kind: &kind,
                        key,
                        priority: 1,
                        controls,
                    },
                )?;
            }
        }
        if species.name == "nulljaw" {
            for (key, controls) in [
                (crate::combat_nulljaw::TARGETING, 0),
                (
                    crate::combat_nulljaw::COMBAT,
                    goal_control::MOVE | goal_control::LOOK,
                ),
            ] {
                ai.add_goal(
                    registry,
                    &Goal {
                        kind: &kind,
                        key,
                        priority: 1,
                        controls,
                    },
                )?;
            }
        }
        if species.name == "varasuchus" {
            for (key, priority, controls) in [
                (crate::combat_varasuchus::TARGET, 0, 0),
                (
                    crate::combat_varasuchus::COMBAT,
                    1,
                    goal_control::MOVE | goal_control::LOOK,
                ),
            ] {
                ai.add_goal(
                    registry,
                    &Goal {
                        kind: &kind,
                        key,
                        priority,
                        controls,
                    },
                )?;
            }
        }
        if species.name == "raevyx" {
            for (key, priority, controls) in [
                (crate::combat_raevyx::TARGETING, 0, 0),
                (
                    crate::combat_raevyx::COMBAT,
                    1,
                    goal_control::MOVE | goal_control::LOOK,
                ),
            ] {
                ai.add_goal(
                    registry,
                    &Goal {
                        kind: &kind,
                        key,
                        priority,
                        controls,
                    },
                )?;
            }
        }
        if species.name == crate::ivy::SPECIES {
            ai.add_goal(
                registry,
                &Goal {
                    kind: &kind,
                    key: crate::ivy::COMBAT,
                    priority: 1,
                    controls: goal_control::MOVE | goal_control::LOOK,
                },
            )?;
        }
        ai.add_goal(
            registry,
            &Goal {
                kind: &kind,
                key: AMBIENT,
                priority: 0,
                controls: 0,
            },
        )?;
    }
    INTERACTION
        .get()
        .ok_or(missing)?
        .on_use_on_block(registry, use_egg)?;
    let mut nodes = vec![
        crate::literal("spawn"),
        Node {
            tag: Some(crate::catalog::OPEN),
            ..crate::literal("items")
        },
    ];
    for (index, (name, _)) in names().enumerate() {
        let base = nodes.len() as u32;
        nodes.push(Node {
            parent: Some(0),
            name,
            argument: Argument::Literal,
            tag: None,
        });
        nodes.extend([
            crate::number(Some(base), "x", None),
            crate::number(Some(base + 1), "y", None),
            crate::number(Some(base + 2), "z", Some(SPAWN_TAG + index as u64)),
        ]);
    }
    commands.add(
        registry,
        "saintsdragons",
        "Browse Saint's Dragons items or spawn creatures",
        2,
        &nodes,
    )
}

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    std::iter::once(("cindervane", "Cindervane")).chain(
        SPECIES
            .iter()
            .map(|species| (species.name, species.display)),
    )
}

pub fn display(species: &str) -> &'static str {
    names()
        .find(|(name, _)| *name == species)
        .map_or("Dragon", |(_, display)| display)
}

pub fn egg_names() -> impl Iterator<Item = (&'static str, &'static str)> {
    names()
        .map(|(species, label)| {
            (
                if species == crate::ivy::SPECIES {
                    crate::ivy::EGG
                } else {
                    species
                },
                label,
            )
        })
        .chain([(SWARM_EGG, "Draconian Swarm")])
}

pub fn spawn_kind(tag: u64) -> Option<String> {
    let index = usize::try_from(tag.checked_sub(SPAWN_TAG)?).ok()?;
    if index == 0 {
        return Some(crate::CINDERVANE.to_owned());
    }
    Some(format!("saintsdragons:{}", SPECIES.get(index - 1)?.name))
}

pub fn spawn(world: World<'_>, kind: &str, at: [f64; 3]) -> Option<i32> {
    let mobs = MOBS.get()?;
    let id = mobs.spawn(world, kind, at)?;
    if let Some(entities) = ENTITIES.get() {
        entities.with_entity(world, id, |mob| {
            mobs.set_state(mob, b"wild");
            if kind
                .strip_prefix("saintsdragons:")
                .and_then(swarm::damage)
                .is_some()
            {
                mobs.set_no_gravity(mob, true);
            }
        });
    }
    Some(id)
}

fn use_egg(context: &mut UseOnBlock<'_>) -> UseResult {
    if let Some(result) = crate::nucleus::use_on_block(context) {
        return result;
    }
    if let Some(result) = crate::binders::use_on_block(context) {
        return result;
    }
    let Some(kind) = context.item.strip_suffix("_spawn_egg") else {
        return UseResult::Pass;
    };
    let Some(name) = kind.strip_prefix("saintsdragons:") else {
        return UseResult::Pass;
    };
    if !egg_names().any(|(known, _)| name == known) || context.stack.count() == 0 {
        return UseResult::Pass;
    }
    let Some(&[dx, dy, dz]) = [
        [0, -1, 0],
        [0, 1, 0],
        [0, 0, -1],
        [0, 0, 1],
        [-1, 0, 0],
        [1, 0, 0],
    ]
    .get(usize::from(context.face)) else {
        return UseResult::Fail;
    };
    let world = context.world;
    let clicked = context.position;
    let adjacent = Pos {
        x: clicked.x + dx,
        y: clicked.y + dy,
        z: clicked.z + dz,
    };
    let empty = |pos| {
        world
            .collision_shape(pos)
            .is_some_and(|shape| shape.is_empty())
    };
    let pos = if empty(clicked) {
        clicked
    } else if empty(adjacent) {
        adjacent
    } else {
        return UseResult::Fail;
    };
    let at = [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    ];
    let spawned = if name == SWARM_EGG {
        // The group egg attempts all three members even if one spawn fails.
        let mut spawned = false;
        for ((member, _), offset) in swarm::MEMBERS.into_iter().zip([-2.0, 0.0, 2.0]) {
            let position = [at[0] + offset, at[1], at[2]];
            spawned |= spawn(world, &format!("saintsdragons:{member}"), position).is_some();
        }
        spawned
    } else {
        let kind = if name == crate::ivy::EGG {
            crate::ivy::KIND
        } else {
            kind
        };
        let Some(id) = spawn(world, kind, at) else {
            return UseResult::Fail;
        };
        if let Some(entities) = ENTITIES.get() {
            let in_end = world.dimension_id() == Some("minecraft:the_end");
            entities.with_entity(world, id, |mob| {
                crate::appearance::spawned_from_egg(mob, name, in_end);
            });
        }
        true
    };
    if !spawned {
        return UseResult::Fail;
    }
    if !context.creative {
        context.stack.shrink(1);
    }
    UseResult::Success
}

/// Runs when a dragon's AI starts. `DragonEntity.finalizeSpawn` for an adult that joined without
/// an appearance.
pub fn finalize_spawn(world: World<'_>, mob: Entity<'_>) {
    let kind = MOBS.get().and_then(|mobs| mobs.kind(mob));
    // Dragons are `TamableAnimal`s, which never despawn far from players; their ravager and
    // spider carriers would.
    if let (Some(mobs), Some(kind)) = (MOBS.get(), kind)
        && crate::spawns::DRAGONS.contains(&kind)
    {
        mobs.make_persistent(mob);
    }
    // `Volitans.canBreatheUnderwater`: its ravager carrier would drown.
    if matches!(
        kind,
        Some("saintsdragons:volitans" | "saintsdragons:atroxiia")
    ) {
        mob.add_effect("minecraft:water_breathing", -1, 0, true, false);
    }
    if let Some(name) = kind.and_then(|kind| kind.strip_prefix("saintsdragons:"))
        && !crate::eggs::is_juvenile(mob)
    {
        let in_end = world.dimension_id() == Some("minecraft:the_end");
        crate::appearance::ensure(mob, name, in_end);
    }
}

pub fn ambient(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(kind), Some(view)) = (MOBS.get().and_then(|mobs| mobs.kind(mob)), mob.view()) else {
        return false;
    };
    let Some(name) = kind.strip_prefix("saintsdragons:") else {
        return false;
    };
    if name == "nulljaw" {
        crate::combat_nulljaw::rider_tick(world, mob);
    }
    let Some(species) = SPECIES.iter().find(|species| species.name == name) else {
        return false;
    };
    if name == "mossback" {
        crate::mossback::ambient(world, mob, view);
        return false;
    }
    if name == crate::ivy::SPECIES {
        crate::ivy::downed_tick(world, mob);
        crate::ivy::target_tick(world, mob);
    }
    let volitans_clip = (name == "volitans")
        .then(|| crate::combat_volitans::ambient(world, mob))
        .flatten();
    if crate::hunger::DRAGONS.contains(&name) {
        crate::hunger::tick(world, mob, species.speed, None, name);
    }
    if crate::hunger::DRAGONS.contains(&name) {
        taming::regrow(mob);
    }
    if crate::eggs::grow(mob, name) {
        let moving = view.motion[0].hypot(view.motion[2]) > 0.035;
        visuals::tick_species(
            world,
            mob,
            &format!("baby_{name}"),
            view,
            if moving && name == "nulljaw" {
                "hover"
            } else if name == "varasuchus" {
                crate::baby::eating_clip(mob.id()).unwrap_or(if moving { "walk" } else { "idle" })
            } else if moving {
                "walk"
            } else {
                "idle"
            },
        );
        return false;
    }
    crate::hunt::tick(world, mob, name);
    crate::roost::owner_tick(world, mob, name);
    let wet = species.swim.is_some()
        && matches!(
            world.fluid_id(flight::cell(view.position)).as_deref(),
            Some("minecraft:water" | "minecraft:flowing_water")
        );
    if swarm::damage(name).is_some() {
        if crate::nucleus::clean_up(world, mob) {
            swarm::forget(mob.id());
            return false;
        }
        swarm::hover(mob, view.motion);
    }
    let clip = if cfg!(feature = "original-assets") {
        crate::roost::sleep_clip(mob.id())
    } else {
        None
    };
    let clip = clip.or_else(|| {
        if name == "nulljaw" {
            return crate::combat_nulljaw::clip(mob);
        }
        if name == "stegonaut" {
            return crate::combat_stegonaut::clip(mob);
        }
        if name == "varasuchus" {
            crate::combat_varasuchus::clip(mob).or_else(|| crate::baby::eating_clip(mob.id()))
        } else if name == crate::ivy::SPECIES {
            crate::ivy::downed_clip(mob).or_else(|| {
                crate::ivy::combat_clip(mob.id(), view.motion[0].hypot(view.motion[2]) > 0.035)
            })
        } else {
            None
        }
    });
    let clip = if name == "raevyx" {
        crate::combat_raevyx::ambient(world, mob, view).or(clip)
    } else {
        clip
    };
    let clip = clip.or_else(|| {
        (name == "atroxiia")
            .then(|| crate::combat_atroxiia::clip(mob))
            .flatten()
    });
    let clip = if name == "ignivorus" {
        crate::combat_ignivorus::ambient(world, mob);
        crate::combat_ignivorus::clip(mob).or(clip)
    } else {
        clip
    };
    let clip = clip
        .or(volitans_clip)
        .or_else(|| swarm::clip(mob.id(), moddedmc_api::tick_count()))
        .unwrap_or_else(|| species.clip(view, wet));
    visuals::tick_species(world, mob, name, view, clip);
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn melee_bases_preserve_carriers_and_ignivorus_growth() {
        assert_eq!(melee_base("raevyx", false), Some(12.0));
        assert_eq!(melee_base("varasuchus", false), Some(10.0));
        assert_eq!(melee_base("varasuchus", true), Some(10.0));
        assert_eq!(melee_base("nulljaw", false), Some(2.0));
        assert_eq!(melee_base("ignivorus", false), Some(50.0));
        assert_eq!(melee_base("ignivorus", true), Some(0.0));
        assert_eq!(melee_base("cindervane", false), None);
        assert_eq!(melee_base("mossback", false), None);
    }

    #[test]
    fn released_adult_attributes_replace_carrier_defaults() {
        for (name, armor, range, knockback, attack) in [
            ("raevyx", 8.0, 64.0, 1.0, 0.0),
            ("ignivorus", 4.0, 128.0, 2.0, 50.0),
            ("stegonaut", 15.0, 32.0, 1.0, 2.0),
            ("varasuchus", 8.0, 32.0, 1.0, 10.0),
            ("volitans", 6.0, 64.0, 0.0, 0.0),
            ("atroxiia", 10.0, 32.0, 1.0, 10.0),
            ("nulljaw", 4.0, 48.0, 0.0, 0.0),
            ("moop", 0.0, 16.0, 0.0, 0.0),
            ("mossback", 0.0, 16.0, 0.0, 0.0),
            ("latcher", 0.0, 64.0, 0.0, 4.0),
            ("winged", 0.0, 64.0, 0.0, 1.5),
            ("whettled", 0.0, 64.0, 0.0, 4.0),
            (crate::ivy::SPECIES, 0.0, 32.0, 0.0, 4.0),
        ] {
            let attributes = SPECIES
                .iter()
                .find(|s| s.name == name)
                .unwrap()
                .attributes();
            for (key, value) in [
                ("minecraft:armor", armor),
                ("minecraft:follow_range", range),
                ("minecraft:knockback_resistance", knockback),
                ("minecraft:attack_damage", attack),
            ] {
                assert!(attributes.contains(&(key, value)), "{name}: {key}");
            }
        }
    }

    #[test]
    fn moop_uses_the_released_swim_speeds_and_land_clip() {
        let moop = SPECIES
            .iter()
            .find(|species| species.name == "moop")
            .unwrap();
        for (motion, wet, clip) in [
            ([0.2, 0.0, 0.0], false, "on_land"),
            ([0.0, 0.2, 0.0], true, "idle"),
            ([0.03, 0.0, 0.0], true, "swim"),
            ([0.08, 0.0, 0.08], true, "swim_fast"),
        ] {
            assert_eq!(
                moop.clip(
                    EntityView {
                        motion,
                        ..EntityView::default()
                    },
                    wet
                ),
                clip
            );
        }
    }
}
