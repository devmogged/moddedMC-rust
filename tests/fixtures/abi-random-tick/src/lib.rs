//! Observes delivery directly so idempotent crop writes cannot hide stale callbacks.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, AtomicU32, Ordering},
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    abi::UPDATE_ALL,
    content::{self, Block, BlockUpdate, Content, State, World, WorldExtension},
};

const BLOCK: &str = "abi_random_tick:sampler";
static CONTENT: OnceLock<Content> = OnceLock::new();
static CALLS: AtomicU32 = AtomicU32::new(0);
static LOADED_LIGHT: AtomicBool = AtomicBool::new(false);
static UNLOADED_LIGHT: AtomicBool = AtomicBool::new(false);

fn tick(world: World<'_>, pos: Pos, _: u32) {
    CALLS.fetch_add(1, Ordering::Relaxed);
    LOADED_LIGHT.store(world.raw_brightness(pos, 0).is_some(), Ordering::Relaxed);
    let remote = Pos {
        x: pos.x + 50_000,
        ..pos
    };
    UNLOADED_LIGHT.store(world.raw_brightness(remote, 0).is_some(), Ordering::Relaxed);

    let content = CONTENT.get().expect("content required in start");
    let air = content.state("minecraft:air").expect("vanilla air");
    world.write(&[BlockUpdate {
        position: pos,
        state: air,
        flags: UPDATE_ALL,
    }]);
}

struct Fixture;

impl Mod for Fixture {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_random_tick()?;
        content.require_extension(WorldExtension::RawBrightness)?;
        let _ = CONTENT.set(content);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let content = *CONTENT.get().expect("content required in start");
        content.add_block(
            registry,
            &Block {
                id: BLOCK,
                display_name: "Random Tick Sampler",
                entity: "",
                item_model: "minecraft:item/stone",
                states: &[State {
                    properties: "",
                    physical: "minecraft:stone",
                    model: "minecraft:block/stone",
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )?;
        content.on_random_tick(registry, &[BLOCK], tick)?;
        registry.add_command("abi_random_ticks", "Inspect random tick callbacks", 0)
    }

    fn command(&self, _tag: u64, _command: Command<'_>) -> i32 {
        moddedmc_api::info(&format!(
            "abi_random_tick calls={} loaded_light={} unloaded_light={}",
            CALLS.load(Ordering::Relaxed),
            LOADED_LIGHT.load(Ordering::Relaxed),
            UNLOADED_LIGHT.load(Ordering::Relaxed),
        ));
        1
    }
}

moddedmc_api::export_mod!(Fixture, id = "abi_random_tick");
