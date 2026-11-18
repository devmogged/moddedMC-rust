//! `demo:counter`: the M1 content fixture. A barrel stand-in whose block entity
//! counts up once a second; right-click shows the count. `/counter` gives one.

mod content {
    wit_bindgen::generate!({
        path: "../../wit/mods",
        world: "content-mod",
    });
}

use content::{
    exports::moddedmc::mods::content::Guest,
    moddedmc::mods::{
        registry,
        types::{
            BlockDef, BlockEntityDef, BreakInput, PlaceInput, RegistrationError, TickInput,
            TickUpdate, UseInput, UseResult,
        },
    },
};
use pumpkin_plugin_api::{
    Context, Plugin, PluginMetadata, Server,
    command::{Command, CommandError, CommandSender, ConsumedArgs},
    commands::CommandHandler,
    common::{Hand, NbtTag, NbtTree},
    item_stack::ItemStack,
    register_plugin,
    text::TextComponent,
};

const BLOCK_ID: &str = "demo:counter";
const STAND_IN: &str = "minecraft:barrel";
const TICK_INTERVAL: u32 = 20;
const STATE_VERSION: u32 = 1;

fn count(state: &[u8]) -> u64 {
    state.try_into().map_or(0, u64::from_le_bytes)
}

struct Counter;

impl Guest for Counter {
    fn register() -> Result<(), RegistrationError> {
        registry::add_block_entity(&BlockEntityDef {
            id: BLOCK_ID.into(),
            state_version: STATE_VERSION,
            initial_state: 0u64.to_le_bytes().to_vec(),
            tick_interval: Some(TICK_INTERVAL),
        })?;
        registry::add_block(&BlockDef {
            id: BLOCK_ID.into(),
            display_name: "Counter".into(),
            stand_in: STAND_IN.into(),
            block_entity: Some(BLOCK_ID.into()),
        })
    }

    fn tick_block_entities(_tick: u64, _dimension: String, entries: Vec<TickInput>) -> Vec<TickUpdate> {
        entries
            .into_iter()
            .map(|entry| TickUpdate {
                token: entry.token,
                state: (count(&entry.state) + 1).to_le_bytes().to_vec(),
            })
            .collect()
    }

    fn use_block(input: UseInput) -> UseResult {
        UseResult {
            state: None,
            message: Some(format!("Counter: {}", count(&input.state))),
        }
    }

    fn place_block(_input: PlaceInput) -> Option<Vec<u8>> {
        None
    }

    fn break_block(_input: BreakInput) {}
}

content::export!(Counter with_types_in content);

struct GiveCounter;

impl CommandHandler for GiveCounter {
    fn handle(&self, sender: CommandSender, _: Server, _: ConsumedArgs) -> Result<i32, CommandError> {
        let Some(player) = sender.as_player() else {
            sender.send_message(TextComponent::text("Only players can receive a counter."));
            return Ok(0);
        };
        let stack = ItemStack::new(STAND_IN, 1);
        // The host recognises placed items by this tag (moddedmc-host ITEM_BLOCK_KEY).
        stack.set_custom_data(
            "moddedmc",
            "block",
            &NbtTree { root: 0, tags: vec![NbtTag::StringTag(BLOCK_ID.into())] },
        );
        stack.set_custom_name(Some(TextComponent::text("Counter")));
        player.set_item_in_hand(Hand::Right, Some(stack));
        Ok(1)
    }
}

struct CounterPlugin;

impl Plugin for CounterPlugin {
    fn new() -> Self {
        CounterPlugin
    }

    fn metadata(&self) -> PluginMetadata {
        PluginMetadata {
            name: "counter".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            authors: vec!["moddedMC-rust".into()],
            description: "demo:counter content fixture".into(),
            dependencies: vec![],
            permissions: vec![],
        }
    }

    fn on_load(&self, context: Context) -> Result<(), String> {
        let cmd = Command::new(&["counter".into()], "Give yourself a demo:counter").execute(GiveCounter);
        context.register_command(cmd, "minecraft:command.help");
        Ok(())
    }
}

register_plugin!(CounterPlugin);

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn count_roundtrip() {
        assert_eq!(count(&41u64.to_le_bytes()), 41);
        assert_eq!(count(&[]), 0);
    }
}
