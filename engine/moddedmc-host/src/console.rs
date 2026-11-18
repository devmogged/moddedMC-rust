//! Console-only test and benchmark commands, handled before Pumpkin's dispatcher:
//!   moddedmc place <block-id> <count> <x> <y> <z>   grid of `count` blocks in the overworld
//!   moddedmc dump <block-id> [limit]                log live blocks and their state bytes
//! Work is queued and done at the start of the next tick.

use std::{fmt::Write as _, sync::Arc};

use pumpkin_core::{
    block::entities::create_block_entity_for_state,
    plugin::{BoxFuture, Cancellable, EventHandler, server::server_command::ServerCommandEvent},
    server::Server,
    world::World,
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use pumpkin_world::world::BlockFlags;

use crate::{
    HOST, NAMESPACE, index_insert, lock, notify_placed, read, read_block_id, read_state,
    write_identity,
};

pub enum Request {
    Place {
        block: String,
        count: u32,
        origin: BlockPos,
    },
    Dump {
        block: String,
        limit: usize,
    },
}

pub struct ConsoleHandler;

impl EventHandler<ServerCommandEvent> for ConsoleHandler {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut ServerCommandEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let line = event.command.trim().trim_start_matches('/').to_owned();
            let Some(args) = line.strip_prefix("moddedmc ") else {
                return;
            };
            event.set_cancelled(true);
            if let Some(request) = parse(args) {
                lock(&HOST.console).push(request);
            } else {
                tracing::warn!(
                    "usage: moddedmc place <block-id> <count> <x> <y> <z> | moddedmc dump <block-id> [limit]"
                );
            }
        })
    }
}

fn parse(args: &str) -> Option<Request> {
    let parts: Vec<&str> = args.split_whitespace().collect();
    match parts.as_slice() {
        ["place", block, count, x, y, z] => Some(Request::Place {
            block: (*block).to_owned(),
            count: count.parse().ok()?,
            origin: BlockPos(Vector3::new(
                x.parse().ok()?,
                y.parse().ok()?,
                z.parse().ok()?,
            )),
        }),
        ["dump", block] => Some(Request::Dump {
            block: (*block).to_owned(),
            limit: 10,
        }),
        ["dump", block, limit] => Some(Request::Dump {
            block: (*block).to_owned(),
            limit: limit.parse().ok()?,
        }),
        _ => None,
    }
}

pub async fn run_queued(server: &Arc<Server>) {
    let requests = std::mem::take(&mut *lock(&HOST.console));
    if requests.is_empty() {
        return;
    }
    let Some(world) = server
        .worlds
        .load()
        .iter()
        .find(|w| w.dimension.minecraft_name == "minecraft:overworld")
        .cloned()
    else {
        return;
    };
    for request in requests {
        match request {
            Request::Place {
                block,
                count,
                origin,
            } => place(&world, &block, count, origin).await,
            Request::Dump { block, limit } => dump(&world, &block, limit),
        }
    }
}

/// Places `count` blocks in a square grid on the plane at `origin.y`, one block apart.
async fn place(world: &Arc<World>, id: &str, count: u32, origin: BlockPos) {
    let block_idx = read(&HOST.registry).block(id);
    let Some(block_idx) = block_idx else {
        tracing::warn!("moddedmc place: unknown block {id}");
        return;
    };
    let side = (f64::from(count).sqrt().ceil() as i32).max(1);
    let placed = place_grid(world, block_idx, count, origin, side);
    for &pos in &placed {
        notify_placed(world, pos, block_idx, None).await;
    }
    tracing::info!(
        "moddedmc place: placed {}/{count} {id} from {origin:?} ({side} per row)",
        placed.len()
    );
}

fn place_grid(
    world: &Arc<World>,
    block_idx: usize,
    count: u32,
    origin: BlockPos,
    side: i32,
) -> Vec<BlockPos> {
    let registry = read(&HOST.registry);
    let block = &registry.blocks[block_idx];
    let entity = &registry.entity_types[block.entity_type];
    let state = block.stand_in.default_state.id;
    let mut placed = Vec::new();
    for i in 0..count.cast_signed() {
        let pos = BlockPos(Vector3::new(
            origin.0.x + i % side,
            origin.0.y,
            origin.0.z + i / side,
        ));
        world.set_block_state(&pos, state, BlockFlags::NOTIFY_ALL);
        if world.get_block_entity(&pos).is_none()
            && let Some(block_entity) = create_block_entity_for_state(state, pos)
        {
            world.add_block_entity(block_entity);
        }
        if world.get_block_entity(&pos).is_none() {
            continue; // chunk not loaded
        }
        write_identity(world, &pos, block, entity);
        index_insert(world, pos, block.entity_type, crate::phase_count(entity));
        placed.push(pos);
    }
    placed
}

fn dump(world: &Arc<World>, id: &str, limit: usize) {
    let mut found: Vec<(BlockPos, Vec<u8>)> = world
        .custom_block_entity_data
        .iter()
        .filter(|e| e.value().get_compound(NAMESPACE).is_some())
        .map(|e| *e.key())
        .filter(|pos| read_block_id(world, pos).as_deref() == Some(id))
        .filter(|pos| {
            world
                .block_entities
                .get(&pos.chunk_position())
                .is_some_and(|c| c.contains_key(pos))
        })
        .filter_map(|pos| Some((pos, read_state(world, &pos)?)))
        .collect();
    found.sort_by_key(|(pos, _)| (pos.0.x, pos.0.y, pos.0.z));
    tracing::info!("moddedmc dump: {} live {id}", found.len());
    for (pos, state) in found.iter().take(limit) {
        let hex = state.iter().fold(String::new(), |mut hex, b| {
            let _ = write!(hex, "{b:02x}");
            hex
        });
        tracing::info!(
            "moddedmc dump: {id} at {} {} {} state={hex}",
            pos.0.x,
            pos.0.y,
            pos.0.z
        );
    }
}
