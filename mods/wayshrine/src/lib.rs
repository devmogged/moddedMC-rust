//! `wayshrine:shrine`: a Waystones-style hand port. Using a shrine discovers it
//! for that player and opens a menu of every other shrine they have discovered;
//! picking one teleports them there. `/wayshrine` gives one.
//!
//! Shrines and discoveries live in `shrines.txt` in the mod's data folder.
//! Block state stays empty: the shrine list is world-wide, not per block.

mod shrines;

use std::{
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex, OnceLock, PoisonError},
};

use moddedmc_api::{
    BlockDef, BlockEntityDef, BlockPos, BreakInput, ContentHost, ContentMod, PlaceInput,
    RegistrationError, Registry, SERVICE, TextComponent, UseInput, UseResult, Vector3,
    dialog::{ActionButton, Dialog, DialogAction, DialogBody},
    pumpkin_core::{
        command::{
            argument_builder::{ArgumentBuilder, command},
            context::CommandContext,
            node::{CommandExecutor, CommandExecutorResult},
        },
        entity::player::Player,
        plugin::{
            BoxFuture, Context, EventHandler, EventPriority, Plugin, PluginFuture,
            dialog::dialog_click_action::DialogClickActionEvent,
        },
        server::Server,
        world::World,
    },
};
use shrines::{Shrine, Shrines, Site};

const BLOCK_ID: &str = "wayshrine:shrine";
const STATE_VERSION: u32 = 1;
const GO_PREFIX: &str = "wayshrine:go/";

static SHRINES: LazyLock<Mutex<Shrines>> = LazyLock::new(Mutex::default);
static FILE: OnceLock<PathBuf> = OnceLock::new();
static HOST: OnceLock<Arc<ContentHost>> = OnceLock::new();

fn site(world: &World, pos: &BlockPos) -> Site {
    Site {
        dimension: world.dimension.minecraft_name.to_owned(),
        x: pos.0.x,
        y: pos.0.y,
        z: pos.0.z,
    }
}

/// Runs `f` on the shrine list and saves it if `f` returns true.
fn update(f: impl FnOnce(&mut Shrines) -> bool) {
    let mut shrines = SHRINES.lock().unwrap_or_else(PoisonError::into_inner);
    if !f(&mut shrines) {
        return;
    }
    if let Some(path) = FILE.get()
        && let Err(error) = std::fs::write(path, shrines.save())
    {
        tracing::error!("wayshrine: could not save {}: {error}", path.display());
    }
}

fn with_shrines<R>(f: impl FnOnce(&Shrines) -> R) -> R {
    f(&SHRINES.lock().unwrap_or_else(PoisonError::into_inner))
}

fn menu(here_name: &str, known: &[Shrine]) -> Dialog {
    let buttons = known
        .iter()
        .map(|s| ActionButton {
            text: TextComponent::text(s.name.clone()),
            tooltip: Some(TextComponent::text(format!(
                "{} {} {} in {}",
                s.site.x, s.site.y, s.site.z, s.site.dimension
            ))),
            width: None,
            action: DialogAction::Custom {
                // A resource location allows one ':', so the dimension's own ':' becomes '/'.
                id: format!("{GO_PREFIX}{}", s.site.key().replacen(':', "/", 1)),
                payload: None,
            },
        })
        .collect();
    Dialog {
        r#type: "minecraft:multi_action".into(),
        title: TextComponent::text("Wayshrines"),
        body: vec![DialogBody::PlainMessage {
            contents: TextComponent::text(format!("You are at {here_name}. Choose a destination.")),
        }],
        inputs: vec![],
        buttons,
        links: vec![],
        exit_action: None,
        after_action: Some("pop".into()),
        can_close_with_escape: true,
        external_title: None,
    }
}

const fn message(text: String) -> UseResult {
    UseResult {
        state: None,
        message: Some(text),
    }
}

struct Wayshrine;

impl ContentMod for Wayshrine {
    fn register(&self, registry: &mut dyn Registry) -> Result<(), RegistrationError> {
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
            block_entity: Some(BLOCK_ID.into()),
        })
    }

    fn place_block(&self, input: PlaceInput<'_>) -> Option<Vec<u8>> {
        let here = site(input.world, &input.pos);
        let owner = input
            .player
            .map_or_else(String::new, |p| p.gameprofile.name.clone());
        update(|s| {
            let name = s.add(here.clone(), owner);
            tracing::info!("wayshrine: placed {name} at {}", here.key());
            true
        });
        None
    }

    fn break_block(&self, input: BreakInput<'_>) {
        let here = site(input.world, &input.pos);
        update(|s| s.remove(&here));
    }

    fn use_block(&self, input: UseInput<'_>) -> UseResult {
        let here = site(input.world, &input.pos);
        let player_name = &input.player.gameprofile.name;
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
        let Some(host) = HOST.get() else {
            return message("Could not open the wayshrine menu.".into());
        };
        (host.show_dialog)(input.player, &menu(&name, &known));
        message(if newly {
            format!("Discovered {name}.")
        } else {
            name
        })
    }
}

/// Teleports a player who picked a destination, if they really know that shrine.
struct TravelHandler;

impl EventHandler<DialogClickActionEvent> for TravelHandler {
    fn handle<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a DialogClickActionEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move { travel(server, &event.player, &event.id) })
    }
}

fn travel(server: &Arc<Server>, player: &Arc<Player>, id: &str) {
    let Some(path) = id.strip_prefix(GO_PREFIX) else {
        return;
    };
    let key = path.replacen('/', ":", 1);
    let player_name = &player.gameprofile.name;
    // The click comes from the client: only shrines this player has discovered are valid.
    let Some(target) = with_shrines(|s| s.travel_target(player_name, &key)) else {
        tracing::warn!("wayshrine: {player_name} asked for unknown destination {key}");
        return;
    };
    let worlds = server.worlds.load();
    let Some(world) = worlds
        .iter()
        .find(|w| w.dimension.minecraft_name == target.site.dimension)
    else {
        return;
    };
    let Some(host) = HOST.get() else { return };
    let pos = Vector3::new(
        f64::from(target.site.x) + 0.5,
        f64::from(target.site.y) + 1.0,
        f64::from(target.site.z) + 0.5,
    );
    (host.teleport)(player, world.clone(), pos);
    tracing::info!("wayshrine: {player_name} travelled to {}", target.name);
}

struct GiveShrine;

impl CommandExecutor for GiveShrine {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let (Some(player), Some(host)) = (context.source.as_player(), HOST.get()) else {
            return Ok(0);
        };
        Ok(i32::from((host.give_block)(&player, BLOCK_ID)))
    }
}

struct WayshrinePlugin;

impl Plugin for WayshrinePlugin {
    fn on_load(&self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            // This library has its own copy of tracing; route it to the server log.
            context.init_log();
            let host = context
                .get_service::<ContentHost>(SERVICE)
                .await
                .ok_or("moddedmc content host is not running")?;
            let folder = context.get_data_folder();
            std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            let path = folder.join("shrines.txt");
            let saved = std::fs::read_to_string(&path).unwrap_or_default();
            *SHRINES.lock().unwrap_or_else(PoisonError::into_inner) = Shrines::load(&saved);
            let _ = FILE.set(path);
            let _ = HOST.set(host.clone());
            (host.add_mod)(Arc::new(Wayshrine));
            context.register_event::<DialogClickActionEvent, _>(
                Arc::new(TravelHandler),
                EventPriority::Normal,
                false,
            );
            context.register_command(
                command("wayshrine", "Give yourself a wayshrine").executes(GiveShrine),
                "minecraft:command.help",
            );
            Ok(())
        })
    }
}

moddedmc_api::export_plugin!(
    WayshrinePlugin,
    "wayshrine",
    "Discoverable fast-travel shrines"
);
