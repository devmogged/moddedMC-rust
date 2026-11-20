//! Saint's Dragons, ported from Saint's Dragons 0.9.88 (commit 13aeb2eb2c). See docs/P2-DRAGONS.md.
//!
//! P2a: Cindervane is a registered mob carried by a vanilla mob, with its own saved state and
//! ground AI (`ai`).

mod ai;

use std::sync::OnceLock;

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    commands::{self, Argument, Commands, Node},
    content::{self, Content},
    entities::{self, Entities, EntitiesExtension},
    mob_ai::{self, Goal, MobAi, goal_control},
    mobs::{self, Mob, Mobs},
};

const CINDERVANE: &str = "saintsdragons:cindervane";
/// Stock clients see this until the model is drawn with displays (P2-DRAGONS.md).
const CINDERVANE_CARRIER: &str = "minecraft:ravager";
/// `CindervaneStatProfile` and its config defaults for an adult.
const CINDERVANE_ATTRIBUTES: &[(&str, f64)] = &[
    ("minecraft:max_health", 80.0),
    ("minecraft:armor", 4.0),
    ("minecraft:flying_speed", 0.60),
    ("minecraft:movement_speed", 0.30),
    ("minecraft:follow_range", 64.0),
];

const SPAWN: u64 = 1;
const FIND: u64 = 2;
/// How far `/cindervane find` looks from its position, in blocks.
const FIND_RADIUS: f64 = 16.0;

static CONTENT: OnceLock<Content> = OnceLock::new();
static COMMANDS: OnceLock<Commands> = OnceLock::new();
static ENTITIES: OnceLock<Entities> = OnceLock::new();
static MOBS: OnceLock<Mobs> = OnceLock::new();
static MOB_AI: OnceLock<MobAi> = OnceLock::new();

struct SaintsDragons;

impl Mod for SaintsDragons {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        let _ = COMMANDS.set(commands::require()?);
        let entities = entities::require()?;
        entities.require_extension(EntitiesExtension::Movement)?;
        let _ = ENTITIES.set(entities);
        let _ = MOBS.set(mobs::require()?);
        let _ = MOB_AI.set(mob_ai::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let missing = RegistrationError::InvalidDefinition;
        MOBS.get().ok_or(missing)?.add_mob(
            registry,
            &Mob {
                id: CINDERVANE,
                carrier: CINDERVANE_CARRIER,
                attributes: CINDERVANE_ATTRIBUTES,
            },
        )?;
        let ai = MOB_AI.get().ok_or(missing)?;
        ai.on_goal(ai::HANDLER)?;
        ai.replace_goals(registry, CINDERVANE)?;
        for (key, priority, controls) in [
            (ai::IDLE_LOOK, 1, goal_control::LOOK),
            (ai::GROUND_WANDER, 2, goal_control::MOVE),
        ] {
            ai.add_goal(
                registry,
                &Goal {
                    kind: CINDERVANE,
                    key,
                    priority,
                    controls,
                },
            )?;
        }
        let at = |parent, tag| {
            [
                number(Some(parent), "x", None),
                number(Some(parent + 1), "y", None),
                number(Some(parent + 2), "z", Some(tag)),
            ]
        };
        let mut nodes = vec![literal("spawn")];
        nodes.extend(at(0, SPAWN));
        nodes.push(literal("find"));
        nodes.extend(at(4, FIND));
        COMMANDS.get().ok_or(missing)?.add(
            registry,
            "cindervane",
            "Spawn or find Cindervanes",
            2,
            &nodes,
        )
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(content), Some(commands), Some(entities), Some(mobs)) =
            (CONTENT.get(), COMMANDS.get(), ENTITIES.get(), MOBS.get())
        else {
            return 0;
        };
        let (Some(x), Some(y), Some(z), Some(world)) = (
            commands.integer(&command, "x"),
            commands.integer(&command, "y"),
            commands.integer(&command, "z"),
            content.command_world(&command),
        ) else {
            return 0;
        };
        let at = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
        match tag {
            SPAWN => {
                let Some(id) = mobs.spawn(world, CINDERVANE, at) else {
                    command.reply("cindervane: could not spawn");
                    return 0;
                };
                let tagged = entities
                    .with_entity(world, id, |mob| mobs.set_state(mob, b"wild"))
                    .unwrap_or(false);
                command.reply(&format!("cindervane: spawned {id}, state set {tagged}"));
                1
            }
            FIND => {
                let min = at.map(|c| c - FIND_RADIUS);
                let max = at.map(|c| c + FIND_RADIUS);
                let found: Vec<String> = entities
                    .entities_in(world, min, max, -1)
                    .into_iter()
                    .filter_map(|id| {
                        entities.with_entity(world, id, |mob| {
                            let kind = mobs.kind(mob)?;
                            let state = mobs.state(mob)?;
                            Some(format!(
                                "{kind} {} state={} ai={}",
                                mob.type_id(),
                                String::from_utf8_lossy(&state),
                                if ai::seen(id) { "on" } else { "off" }
                            ))
                        })?
                    })
                    .collect();
                command.reply(&format!(
                    "cindervane: found {}: {}",
                    found.len(),
                    found.join(", ")
                ));
                i32::try_from(found.len()).unwrap_or(i32::MAX)
            }
            _ => 0,
        }
    }
}

const fn literal(name: &str) -> Node<'_> {
    Node {
        parent: None,
        name,
        argument: Argument::Literal,
        tag: None,
    }
}

const fn number(parent: Option<u32>, name: &str, tag: Option<u64>) -> Node<'_> {
    Node {
        parent,
        name,
        argument: Argument::Integer {
            min: -30_000_000,
            max: 30_000_000,
        },
        tag,
    }
}

moddedmc_api::export_mod!(SaintsDragons);
