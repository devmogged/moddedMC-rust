use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use moddedmc_api::{
    BlockDef, BlockEntityDef, Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    entities::{self, EntitiesExtension},
    native_entity::{self, Actor, ActorKind, Actors, Event, Model, Phase, SleepingVisual, Spawn},
};
use serde::Serialize;

static ACTORS: OnceLock<Actors> = OnceLock::new();
static KIND: OnceLock<ActorKind> = OnceLock::new();
static TICK_REPORTED: AtomicBool = AtomicBool::new(false);
const TOKEN: &str = "abi_native_entity:token";

#[derive(Serialize)]
struct SavedStack {
    id: &'static str,
    count: i32,
}

fn show(actor: Actor<'_>, data: &[u8], model: Model) -> bool {
    let helmet = fastnbt::to_bytes(&SavedStack {
        id: "minecraft:iron_helmet",
        count: 1,
    })
    .expect("encode the fixture equipment");
    let mut equipment = [None; 6];
    equipment[5] = Some(helmet.as_slice());
    actor.show_sleeping(&SleepingVisual {
        model,
        yaw: 0.0,
        offset: [f64::from(data[1]) * 2.0, 0.0, 0.0],
        profile: Some(&data[2..]),
        equipment,
    })
}

fn lifecycle(event: Event<'_>) -> bool {
    let Some(mut data) = event.actor.data() else {
        return false;
    };
    let [count, _, ..] = data.as_slice() else {
        return false;
    };
    let count = *count;
    match event.phase {
        Phase::Loaded => {
            moddedmc_api::info(&format!("abi_native_entity loaded {count}"));
            let model = if count < 2 {
                Model::Player
            } else {
                Model::Skeleton
            };
            return show(event.actor, &data, model);
        }
        Phase::Unloaded => moddedmc_api::info(&format!("abi_native_entity unloaded {count}")),
        Phase::Removed => moddedmc_api::info(&format!("abi_native_entity removed {count}")),
        Phase::Used => {
            let Some(player) = event
                .player
                .as_ref()
                .filter(|player| player.name() == "tester")
            else {
                return false;
            };
            let count = count.saturating_add(1);
            data[0] = count;
            if !event.actor.set_data(&data) {
                return false;
            }
            moddedmc_api::info(&format!("abi_native_entity used {count}"));
            if count == 1 {
                return player.give_block(TOKEN);
            }
            if count == 2 {
                return show(event.actor, &data, Model::Skeleton);
            }
            if count == 3 {
                let removed = event.actor.entity.discard();
                if removed {
                    moddedmc_api::info("abi_native_entity discard returned");
                }
                return removed;
            }
        }
        Phase::Tick => {
            if !TICK_REPORTED.swap(true, Ordering::Relaxed) {
                moddedmc_api::info("abi_native_entity tick");
            }
        }
    }
    true
}

struct Fixture;

impl Mod for Fixture {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        entities::require()?.require_extension(EntitiesExtension::Player)?;
        let actors = native_entity::require()?;
        actors.require_visual()?;
        let _ = ACTORS.set(actors);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let actors = ACTORS.get().expect("actors required in start");
        let _ = KIND.set(actors.register(registry, lifecycle)?);
        registry.add_block_entity(BlockEntityDef {
            id: TOKEN.into(),
            state_version: 1,
            initial_state: Vec::new(),
            tick_interval: None,
        })?;
        registry.add_block(BlockDef {
            id: TOKEN.into(),
            display_name: "Actor Test Token".into(),
            stand_in: "minecraft:barrel".into(),
            block_entity: TOKEN.into(),
        })?;
        registry.add_command("abi_actor_spawn", "Spawn a saved test actor", 0)?;
        registry.add_command(
            "abi_actor_boundary",
            "Spawn a visual across a chunk edge",
            1,
        )?;
        moddedmc_api::info("abi_native_entity ready");
        Ok(())
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let (Some(player), Some(&actors), Some(&kind)) = (command.player, ACTORS.get(), KIND.get())
        else {
            return 0;
        };
        let Some(profile) = actors.player_profile(&player) else {
            return 0;
        };
        let edge = tag == 1;
        let mut initial = vec![0, u8::from(edge)];
        initial.extend(profile);
        let spec = Spawn {
            uuid: if edge {
                *b"abcdfixture-0002"
            } else {
                *b"abcdfixture-0001"
            },
            position: [if edge { 1039.5 } else { 1025.5 }, 300.0, 1025.5],
            width: 1.4,
            height: 0.45,
            physics: false,
            data: &initial,
        };
        let spawned = player.with_entity_context(|world, _| actors.spawn(&world, kind, &spec));
        if spawned == Some(false) {
            moddedmc_api::info("abi_native_entity spawn rejected");
        }
        i32::from(spawned == Some(true))
    }
}

moddedmc_api::export_mod!(Fixture, id = "abi_native_entity");
