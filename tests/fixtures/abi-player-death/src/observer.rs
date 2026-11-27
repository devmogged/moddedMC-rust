use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    player_death::{self, Death, PlayerDeaths},
};

static DEATHS: OnceLock<PlayerDeaths> = OnceLock::new();

fn observe(_death: Death<'_>) {
    moddedmc_api::info("abi_player_death_observer received unclaimed drops");
}

struct Observer;

impl Mod for Observer {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = DEATHS.set(player_death::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        DEATHS
            .get()
            .expect("player death required in start")
            .on_drops(registry, observe)?;
        moddedmc_api::info("abi_player_death_observer ready");
        Ok(())
    }
}

moddedmc_api::export_mod!(Observer, id = "abi_player_death_observer");
