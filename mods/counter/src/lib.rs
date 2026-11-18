//! `demo:counter`: the M1 content fixture. A barrel stand-in whose block entity
//! counts up once a second; right-click shows the count. `/counter` gives one.

use std::sync::Arc;

use moddedmc_api::{
    BlockDef, BlockEntityDef, ContentHost, ContentMod, RegistrationError, Registry, SERVICE,
    TickEntry, UseInput, UseResult,
    pumpkin_core::{
        command::{
            argument_builder::{ArgumentBuilder, command},
            context::CommandContext,
            node::{CommandExecutor, CommandExecutorResult},
        },
        plugin::{Context, Plugin, PluginFuture},
        world::World,
    },
};

const BLOCK_ID: &str = "demo:counter";
const TICK_INTERVAL: u32 = 20;
const STATE_VERSION: u32 = 1;

fn count(state: &[u8]) -> u64 {
    state.try_into().map_or(0, u64::from_le_bytes)
}

struct Counter;

impl ContentMod for Counter {
    fn register(&self, registry: &mut dyn Registry) -> Result<(), RegistrationError> {
        registry.add_block_entity(BlockEntityDef {
            id: BLOCK_ID.into(),
            state_version: STATE_VERSION,
            initial_state: 0u64.to_le_bytes().to_vec(),
            tick_interval: Some(TICK_INTERVAL),
        })?;
        registry.add_block(BlockDef {
            id: BLOCK_ID.into(),
            display_name: "Counter".into(),
            stand_in: "minecraft:barrel".into(),
            block_entity: Some(BLOCK_ID.into()),
        })
    }

    fn tick(&self, _tick: u64, _world: &Arc<World>, entries: &mut [TickEntry]) {
        for entry in entries {
            let next = count(&entry.state) + 1;
            entry.state = next.to_le_bytes().to_vec();
            entry.changed = true;
        }
    }

    fn use_block(&self, input: UseInput<'_>) -> UseResult {
        UseResult {
            state: None,
            message: Some(format!("Counter: {}", count(input.state))),
        }
    }
}

struct GiveCounter(Arc<ContentHost>);

impl CommandExecutor for GiveCounter {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let Some(player) = context.source.as_player() else {
            return Ok(0);
        };
        Ok(i32::from((self.0.give_block)(&player, BLOCK_ID)))
    }
}

struct CounterPlugin;

impl Plugin for CounterPlugin {
    fn on_load(&self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let host = context
                .get_service::<ContentHost>(SERVICE)
                .await
                .ok_or("moddedmc content host is not running")?;
            (host.add_mod)(Arc::new(Counter));
            context.register_command(
                command("counter", "Give yourself a demo:counter").executes(GiveCounter(host)),
                "minecraft:command.help",
            );
            Ok(())
        })
    }
}

moddedmc_api::export_plugin!(CounterPlugin, "counter", "demo:counter content fixture");

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn count_roundtrip() {
        assert_eq!(count(&41u64.to_le_bytes()), 41);
        assert_eq!(count(&[]), 0);
    }
}
