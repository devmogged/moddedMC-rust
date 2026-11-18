//! `demo:counter`: the M1 content fixture. A barrel stand-in whose block entity
//! counts up once a second; right-click shows the count. `/counter` gives one.

use moddedmc_api::{
    Block, BlockDef, BlockEntityDef, Command, Mod, RegistrationError, Registry, Tick, UseResult,
};

const BLOCK_ID: &str = "demo:counter";
const TICK_INTERVAL: u32 = 20;
const STATE_VERSION: u32 = 1;

fn count(state: &[u8]) -> u64 {
    state.try_into().map_or(0, u64::from_le_bytes)
}

struct Counter;

impl Mod for Counter {
    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
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
            block_entity: BLOCK_ID.into(),
        })?;
        registry.add_command("counter", "Give yourself a demo:counter", 0)
    }

    fn tick(&self, tick: &Tick<'_>) {
        for entry in tick.entries() {
            tick.set_state(entry.index, &(count(entry.state) + 1).to_le_bytes());
        }
    }

    fn use_block(&self, block: Block<'_>) -> UseResult {
        UseResult {
            state: None,
            message: Some(format!("Counter: {}", count(block.state))),
        }
    }

    fn command(&self, _tag: u64, command: Command<'_>) -> i32 {
        command
            .player
            .map_or(0, |p| i32::from(p.give_block(BLOCK_ID)))
    }
}

moddedmc_api::export_mod!(Counter);

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn count_roundtrip() {
        assert_eq!(count(&41u64.to_le_bytes()), 41);
        assert_eq!(count(&[]), 0);
    }
}
