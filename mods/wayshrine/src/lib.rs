//! `wayshrine:shrine`: a Waystones-style hand port. Using a shrine discovers it
//! for that player and opens a menu of every other shrine they have discovered;
//! picking one teleports them there. `/wayshrine` gives one.
//!
//! Shrines and discoveries live in `shrines.txt` in the mod's data folder.
//! Block state stays empty: the shrine list is world-wide, not per block.

mod content {
    wit_bindgen::generate!({
        path: "../../wit/mods",
        world: "content-mod",
    });
}

mod shrines;

use std::cell::RefCell;

use content::{
    exports::moddedmc::mods::content::Guest,
    moddedmc::mods::{
        registry,
        types::{
            BlockDef, BlockEntityDef, BlockPos, BreakInput, PlaceInput, RegistrationError,
            TickInput, TickUpdate, UseInput, UseResult,
        },
    },
};
use pumpkin_plugin_api::{
    Context, EventHandler, Plugin, PluginMetadata, Server,
    command::{Command, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    common::{Hand, NbtTag, NbtTree},
    events::{DialogClickActionEvent, EventPriority},
    events_wit::DialogClickActionEventData,
    item_stack::ItemStack,
    java_dialog::{
        Action, ActionButton, AfterAction, CustomClickAction, Dialog, DialogBody, DialogType,
    },
    permissions::{FS_READ_DATA, FS_WRITE_DATA},
    register_plugin,
    text::TextComponent,
};
use shrines::{Shrine, Shrines, Site};

const BLOCK_ID: &str = "wayshrine:shrine";
const STAND_IN: &str = "minecraft:barrel";
const STATE_VERSION: u32 = 1;
const GO_PREFIX: &str = "wayshrine:go/";

thread_local! {
    static SHRINES: RefCell<Shrines> = RefCell::new(Shrines::default());
    static FILE: RefCell<Option<String>> = const { RefCell::new(None) };
    static SERVER: RefCell<Option<Server>> = const { RefCell::new(None) };
}

fn site(dimension: String, pos: &BlockPos) -> Site {
    Site { dimension, x: pos.x, y: pos.y, z: pos.z }
}

/// Applies `change` to the shrine list and saves it if it returns true.
fn update(change: impl FnOnce(&mut Shrines) -> bool) {
    SHRINES.with_borrow_mut(|shrines| {
        if !change(shrines) {
            return;
        }
        if let Some(path) = FILE.with_borrow(Clone::clone)
            && let Err(error) = std::fs::write(&path, shrines.save())
        {
            tracing::error!("wayshrine: could not save {path}: {error}");
        }
    });
}

fn menu(here: &Site, known: &[Shrine]) -> Dialog {
    let buttons = known
        .iter()
        .map(|s| ActionButton {
            text: TextComponent::text(&s.name),
            tooltip: Some(TextComponent::text(&format!(
                "{} {} {} in {}",
                s.site.x, s.site.y, s.site.z, s.site.dimension
            ))),
            width: None,
            action: Action::CustomClick(CustomClickAction {
                // A resource location allows one ':', so the dimension's own ':' becomes '/'.
                id: format!("{GO_PREFIX}{}", s.site.key().replacen(':', "/", 1)),
                payload: None,
            }),
        })
        .collect();
    Dialog {
        title: TextComponent::text("Wayshrines"),
        type_: DialogType::MultiAction,
        body: vec![DialogBody::PlainMessage(TextComponent::text(&format!(
            "You are at {}. Choose a destination.",
            SHRINES.with_borrow(|s| s.get(here).map(|s| s.name.clone()).unwrap_or_default())
        )))],
        inputs: vec![],
        buttons,
        links: vec![],
        after_action: Some(AfterAction::Pop),
        can_close_with_escape: true,
        external_title: None,
    }
}

struct Wayshrine;

impl Guest for Wayshrine {
    fn register() -> Result<(), RegistrationError> {
        registry::add_block_entity(&BlockEntityDef {
            id: BLOCK_ID.into(),
            state_version: STATE_VERSION,
            initial_state: vec![],
            tick_interval: None,
        })?;
        registry::add_block(&BlockDef {
            id: BLOCK_ID.into(),
            display_name: "Wayshrine".into(),
            stand_in: STAND_IN.into(),
            block_entity: Some(BLOCK_ID.into()),
        })
    }

    fn tick_block_entities(_: u64, _: String, _: Vec<TickInput>) -> Vec<TickUpdate> {
        vec![]
    }

    fn place_block(input: PlaceInput) -> Option<Vec<u8>> {
        let here = site(input.dimension, &input.pos);
        update(|s| {
            let name = s.add(here.clone(), input.player_name.unwrap_or_default());
            tracing::info!("wayshrine: placed {name} at {}", here.key());
            true
        });
        None
    }

    fn break_block(input: BreakInput) {
        let here = site(input.dimension, &input.pos);
        update(|s| s.remove(&here));
    }

    fn use_block(input: UseInput) -> UseResult {
        let here = site(input.dimension, &input.pos);
        let player_name = input.player_name;
        let mut newly = false;
        update(|s| {
            newly = s.discover(&player_name, &here);
            newly
        });
        let known = SHRINES.with_borrow(|s| s.known_by(&player_name, &here));
        let name = SHRINES.with_borrow(|s| s.get(&here).map(|s| s.name.clone()));
        let Some(name) = name else {
            // Placed before the mod saw it (e.g. data file lost): nothing to offer.
            return message("This wayshrine is not attuned.");
        };
        if known.is_empty() {
            return message(&if newly {
                format!("Discovered {name}. Discover another shrine to travel.")
            } else {
                format!("{name}: you know no other shrines.")
            });
        }
        let shown = SERVER.with_borrow(|server| {
            let player = server.as_ref()?.get_player_by_name(&player_name)?;
            player.as_java()?.show_dialog(menu(&here, &known));
            Some(())
        });
        if shown.is_none() {
            return message("Could not open the wayshrine menu.");
        }
        if newly { message(&format!("Discovered {name}.")) } else { message(&name) }
    }
}

fn message(text: &str) -> UseResult {
    UseResult { state: None, message: Some(text.into()) }
}

content::export!(Wayshrine with_types_in content);

/// Teleports a player who picked a destination, if they really know that shrine.
struct TravelHandler;

impl EventHandler<DialogClickActionEvent> for TravelHandler {
    fn handle(&self, server: Server, event: DialogClickActionEventData) -> DialogClickActionEventData {
        let Some(path) = event.id.strip_prefix(GO_PREFIX) else {
            return event;
        };
        let key = path.replacen('/', ":", 1);
        let key = key.as_str();
        let player_name = event.player.get_name();
        // The click comes from the client: only shrines this player has discovered are valid.
        let Some(target) = SHRINES.with_borrow(|s| s.travel_target(&player_name, key)) else {
            tracing::warn!("wayshrine: {player_name} asked for unknown destination {key}");
            return event;
        };
        let Some(world) = server.get_world_by_name(&target.site.dimension) else {
            return event;
        };
        let pos = (
            f64::from(target.site.x) + 0.5,
            f64::from(target.site.y) + 1.0,
            f64::from(target.site.z) + 0.5,
        );
        event.player.teleport(pos, None, None, world);
        tracing::info!("wayshrine: {player_name} travelled to {}", target.name);
        event
    }
}

struct GiveShrine;

impl CommandHandler for GiveShrine {
    fn handle(&self, sender: CommandSender, _: Server, _: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can receive a wayshrine."));
            return Ok(0);
        };
        let stack = ItemStack::new(STAND_IN, 1);
        // The host recognises placed items by this tag (moddedmc-host ITEM_BLOCK_KEY).
        stack.set_custom_data(
            "moddedmc",
            "block",
            &NbtTree { root: 0, tags: vec![NbtTag::StringTag(BLOCK_ID.into())] },
        );
        stack.set_custom_name(Some(TextComponent::text("Wayshrine")));
        player.set_item_in_hand(Hand::Right, Some(stack));
        Ok(1)
    }
}

struct WayshrinePlugin;

impl Plugin for WayshrinePlugin {
    fn new() -> Self {
        WayshrinePlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "wayshrine".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["moddedMC-rust".into()],
            description: "Discoverable fast-travel shrines".into(),
            dependencies: vec![],
            permissions: vec![FS_READ_DATA.into(), FS_WRITE_DATA.into()],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let path = format!("{}/shrines.txt", context.get_data_folder());
        let saved = std::fs::read_to_string(&path).unwrap_or_default();
        SHRINES.set(Shrines::load(&saved));
        FILE.set(Some(path));
        SERVER.set(Some(context.get_server()));
        context.register_event_handler::<DialogClickActionEvent, _>(
            TravelHandler,
            EventPriority::Normal,
            false,
        )?;
        let cmd = Command::new(&["wayshrine".into()], "Give yourself a wayshrine").execute(GiveShrine);
        context.register_command(cmd, "minecraft:command.help");
        Ok(())
    }
}

register_plugin!(WayshrinePlugin);
