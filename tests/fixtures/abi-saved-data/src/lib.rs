//! Deterministic test mod for save-global bytes across restarts and distinct saves.
use std::sync::{OnceLock, atomic::{AtomicBool, Ordering}};
use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Content, World},
    saved_data::{self, Key, SavedData},
};

struct Fixture;
static SAVE: OnceLock<SavedData> = OnceLock::new();
static CONTENT: OnceLock<Content> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();
static PENDING: AtomicBool = AtomicBool::new(false);
const PAYLOAD: &[u8] = &[0, 1, 128, 255, 42, 0, 12];

fn tick(world: World<'_>, _time: u64, _events: &[content::ContentEvent]) {
    if !PENDING.swap(false, Ordering::SeqCst) {
        return;
    }
    let (Some(&api), Some(&key)) = (SAVE.get(), KEY.get()) else { return };
    match api.read(&world, key) {
        None => {
            if api.write(&world, key, PAYLOAD) {
                moddedmc_api::info("abi_saved_data wrote binary payload");
            } else {
                moddedmc_api::error("abi_saved_data write failed");
            }
        }
        Some(bytes) if bytes == PAYLOAD => {
            moddedmc_api::info("abi_saved_data persisted binary payload");
        }
        Some(_) => {
            moddedmc_api::error("abi_saved_data corrupt binary payload");
        }
    }
}

impl Mod for Fixture {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = SAVE.set(saved_data::require()?);
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let save = *SAVE.get().expect("saved_data required in start");
        let _ = KEY.set(save.register(registry, "global_data")?);
        assert!(save.register(registry, "global_data").is_err());
        assert!(save.register(registry, "../invalid").is_err());
        CONTENT.get().expect("content required in start").on_tick(registry, tick)?;
        registry.add_command("abi_data", "Check save-global mod data", 0)
    }

    fn command(&self, _tag: u64, _command: Command<'_>) -> i32 {
        PENDING.store(true, Ordering::SeqCst);
        1
    }
}

moddedmc_api::export_mod!(Fixture, id = "abi_saved_data");
