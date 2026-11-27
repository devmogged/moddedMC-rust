//! Exercises whole-batch acceptance, decline and durable readback.
use std::sync::{
    OnceLock,
    atomic::{AtomicBool, Ordering},
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, ContentEvent, World},
    player_death::{self, Death, PlayerDeaths},
    saved_data::{self, Key, SavedData},
};
use serde::Deserialize;

static DEATHS: OnceLock<PlayerDeaths> = OnceLock::new();
static SAVED: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();
static VIEW: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
struct Stack {
    id: String,
    count: i32,
}

fn claim(mut death: Death<'_>) {
    let Some(&key) = KEY.get() else {
        return;
    };
    let Some(stacks): Option<Vec<_>> = death
        .drops()
        .map(|drop| {
            fastnbt::from_bytes::<Stack>(drop.stack)
                .ok()
                .map(|stack| (drop.slot, stack))
        })
        .collect()
    else {
        return;
    };
    if stacks
        .iter()
        .any(|(_, stack)| stack.id == "minecraft:copper_ingot")
    {
        let oversized = vec![0; moddedmc_api::abi::MAX_SAVED_DATA_BYTES + 1];
        let committed = death.claim(key, &oversized);
        moddedmc_api::info(&format!(
            "abi_player_death failed write returned {committed}"
        ));
        return;
    }
    if !stacks
        .iter()
        .any(|(_, stack)| stack.id == "minecraft:diamond")
        || stacks
            .iter()
            .any(|(_, stack)| stack.id == "minecraft:iron_sword")
    {
        moddedmc_api::info("abi_player_death declined");
        return;
    }

    let summary = stacks
        .iter()
        .map(|(slot, stack)| format!("{slot}/{}:{}", stack.id, stack.count))
        .collect::<Vec<_>>()
        .join(" ");
    if !death.claim(key, summary.as_bytes()) {
        return;
    }
    moddedmc_api::info(&format!("abi_player_death accepted {summary}"));
    let duplicate = death.claim(key, b"duplicate");
    moddedmc_api::info(&format!(
        "abi_player_death repeated claim returned {duplicate}"
    ));
}

fn tick(world: World<'_>, _time: u64, _events: &[ContentEvent]) {
    if !VIEW.swap(false, Ordering::Relaxed) {
        return;
    }
    let (Some(&save), Some(&key)) = (SAVED.get(), KEY.get()) else {
        return;
    };
    if let Some(record) = save.read(&world, key) {
        moddedmc_api::info(&format!(
            "abi_player_death persisted {}",
            String::from_utf8_lossy(&record)
        ));
    }
}

struct Fixture;

impl Mod for Fixture {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = DEATHS.set(player_death::require()?);
        let save = saved_data::require()?;
        let _ = SAVED.set(save);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let save = SAVED.get().expect("saved data required in start");
        let _ = KEY.set(save.register(registry, "death")?);
        DEATHS
            .get()
            .expect("player death required in start")
            .on_drops(registry, claim)?;
        content::require()
            .map_err(|_| RegistrationError::InvalidDefinition)?
            .on_tick(registry, tick)?;
        registry.add_command("abi_death_view", "Read the saved death fixture", 0)?;
        moddedmc_api::info("abi_player_death ready");
        Ok(())
    }

    fn command(&self, _tag: u64, _command: Command<'_>) -> i32 {
        VIEW.store(true, Ordering::Relaxed);
        1
    }
}

moddedmc_api::export_mod!(Fixture, id = "abi_player_death");
