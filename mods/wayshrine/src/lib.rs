//! `wayshrine:shrine`: a Waystones-style hand port. Using a shrine discovers it
//! for that player and opens a menu of every other shrine they have discovered;
//! picking one teleports them there. `/wayshrine` gives one.
//!
//! Shrines and discoveries live in `shrines.txt` in the mod's data folder.
//! Block state stays empty: the shrine list is world-wide, not per block.

mod shrines;

use std::sync::{LazyLock, Mutex, PoisonError};

use moddedmc_api::{
    Block, BlockDef, BlockEntityDef, Command, Menu, MenuButton, Mod, Player, Pos,
    RegistrationError, Registry, UseResult,
};
use shrines::{Shrine, Shrines, Site};

const BLOCK_ID: &str = "wayshrine:shrine";
const STATE_VERSION: u32 = 1;
const GO_PREFIX: &str = "wayshrine:go/";

static SHRINES: LazyLock<Mutex<Shrines>> = LazyLock::new(Mutex::default);

fn site(dimension: &str, pos: Pos) -> Site {
    Site {
        dimension: dimension.to_owned(),
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}

/// Runs `f` on the shrine list and saves it if `f` returns true.
fn update(f: impl FnOnce(&mut Shrines) -> bool) {
    let mut shrines = SHRINES.lock().unwrap_or_else(PoisonError::into_inner);
    if !f(&mut shrines) {
        return;
    }
    let path = moddedmc_api::data_dir().join("shrines.txt");
    if let Err(error) = std::fs::write(&path, shrines.save()) {
        moddedmc_api::error(&format!(
            "wayshrine: could not save {}: {error}",
            path.display()
        ));
    }
}

fn with_shrines<R>(f: impl FnOnce(&Shrines) -> R) -> R {
    f(&SHRINES.lock().unwrap_or_else(PoisonError::into_inner))
}

fn menu(here_name: &str, known: &[Shrine]) -> Menu {
    Menu {
        title: "Wayshrines".into(),
        body: format!("You are at {here_name}. Choose a destination."),
        buttons: known
            .iter()
            .map(|s| MenuButton {
                label: s.name.clone(),
                tooltip: format!(
                    "{} {} {} in {}",
                    s.site.x, s.site.y, s.site.z, s.site.dimension
                ),
                // A resource location allows one ':', so the dimension's own ':' becomes '/'.
                action: format!("{GO_PREFIX}{}", s.site.key().replacen(':', "/", 1)),
            })
            .collect(),
    }
}

const fn message(text: String) -> UseResult {
    UseResult {
        state: None,
        message: Some(text),
    }
}

struct Wayshrine;

impl Mod for Wayshrine {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let saved = std::fs::read_to_string(moddedmc_api::data_dir().join("shrines.txt"))
            .unwrap_or_default();
        *SHRINES.lock().unwrap_or_else(PoisonError::into_inner) = Shrines::load(&saved);
        registry.add_block_entity(BlockEntityDef {
            id: BLOCK_ID.into(),
            state_version: STATE_VERSION,
            initial_state: vec![],
            tick_interval: None,
        })?;
        registry.add_block(BlockDef {
            id: BLOCK_ID.into(),
            display_name: "Wayshrine".into(),
            stand_in: "minecraft:barrel".into(),
            block_entity: BLOCK_ID.into(),
        })?;
        registry.add_command("wayshrine", "Give yourself a wayshrine", 0)
    }

    fn place_block(&self, block: Block<'_>) -> Option<Vec<u8>> {
        let here = site(block.dimension, block.pos);
        let owner = block
            .player
            .map_or_else(String::new, |p| p.name().to_owned());
        update(|s| {
            let name = s.add(here.clone(), owner);
            moddedmc_api::info(&format!("wayshrine: placed {name} at {}", here.key()));
            true
        });
        None
    }

    fn break_block(&self, block: Block<'_>) {
        let here = site(block.dimension, block.pos);
        update(|s| s.remove(&here));
    }

    fn use_block(&self, block: Block<'_>) -> UseResult {
        let here = site(block.dimension, block.pos);
        let Some(player) = block.player else {
            return UseResult::default();
        };
        let player_name = player.name();
        let mut newly = false;
        update(|s| {
            newly = s.discover(player_name, &here);
            newly
        });
        let (known, name) = with_shrines(|s| {
            (
                s.known_by(player_name, &here),
                s.get(&here).map(|s| s.name.clone()),
            )
        });
        let Some(name) = name else {
            // Placed before the mod saw it (e.g. data file lost): nothing to offer.
            return message("This wayshrine is not attuned.".into());
        };
        if known.is_empty() {
            return message(if newly {
                format!("Discovered {name}. Discover another shrine to travel.")
            } else {
                format!("{name}: you know no other shrines.")
            });
        }
        player.show_menu(&menu(&name, &known));
        message(if newly {
            format!("Discovered {name}.")
        } else {
            name
        })
    }

    /// Teleports a player who picked a destination, if they really know that shrine.
    fn menu_click(&self, player: Player<'_>, action: &str) {
        let Some(path) = action.strip_prefix(GO_PREFIX) else {
            return;
        };
        let key = path.replacen('/', ":", 1);
        let player_name = player.name();
        // The click comes from the client: only shrines this player has discovered are valid.
        let Some(target) = with_shrines(|s| s.travel_target(player_name, &key)) else {
            moddedmc_api::warn(&format!(
                "wayshrine: {player_name} asked for unknown destination {key}"
            ));
            return;
        };
        let site = &target.site;
        if player.teleport(
            &site.dimension,
            f64::from(site.x) + 0.5,
            f64::from(site.y) + 1.0,
            f64::from(site.z) + 0.5,
            None,
        ) {
            moddedmc_api::info(&format!(
                "wayshrine: {player_name} travelled to {}",
                target.name
            ));
        }
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        command
            .player
            .map_or(0, |p| i32::from(p.give_block(BLOCK_ID)))
    }
}

moddedmc_api::export_mod!(Wayshrine);
