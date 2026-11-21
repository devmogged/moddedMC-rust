//! Create 6.0.10's SeatBlock and SeatEntity.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    abi::{CARRIER_SEAT, ENTITY_PASSENGER},
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    entities::{ENTITY_LIVING, ENTITY_PLAYER, Entity},
    interaction::{BlockUseResult, UseBlock},
};

use crate::{CONTENT, ENTITIES, StateIds, TAGS};

const KEY: &str = "create:seat";
const COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];
const IDS: [&str; 16] = [
    "create:white_seat",
    "create:orange_seat",
    "create:magenta_seat",
    "create:light_blue_seat",
    "create:yellow_seat",
    "create:lime_seat",
    "create:pink_seat",
    "create:gray_seat",
    "create:light_gray_seat",
    "create:cyan_seat",
    "create:purple_seat",
    "create:blue_seat",
    "create:brown_seat",
    "create:green_seat",
    "create:red_seat",
    "create:black_seat",
];
const RIDING_HEIGHT: f64 = 1.0 / 16.0;
const TAG: &[u8] = &[10, 0, 0, 0];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for (index, color) in COLORS.into_iter().enumerate() {
        let name = color
            .split('_')
            .map(|part| {
                let mut text = part.to_owned();
                text[..1].make_ascii_uppercase();
                text
            })
            .collect::<Vec<_>>()
            .join(" ");
        let physical = format!("minecraft:{color}_wool");
        let model = format!("create:block/{color}_seat");
        let item_model = format!("create:item/{color}_seat");
        let states = ["waterlogged=false", "waterlogged=true"].map(|properties| State {
            properties,
            physical: &physical,
            model: &model,
            rotating_model: "",
            x: 0,
            y: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            // The host has one shape, used here for the default non-player context.
            shape: &[ShapeBox {
                min: [0, 0, 0],
                max: [16, 6, 16],
            }],
        });
        content.add_block(
            registry,
            &Block {
                id: IDS[index],
                display_name: &format!("{name} Seat"),
                entity: "",
                item_model: &item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub fn placed_state(args: Placement<'_>, id: &str) -> Option<u32> {
    let wet = args.world.fluid_id(args.position)?.as_str() == "minecraft:water";
    CONTENT.get()?.state(&format!("{id}[waterlogged={wet}]"))
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for id in IDS {
        for properties in ["waterlogged=false", "waterlogged=true"] {
            ids.add(content, id, properties, None);
        }
    }
}

pub fn is_seat(id: &str) -> bool {
    IDS.contains(&id)
}

pub fn position(pos: Pos) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + RIDING_HEIGHT,
        f64::from(pos.z) + 0.5,
    ]
}

pub fn carrier_at(world: World<'_>, position: [f64; 3], range: f64) -> Option<i32> {
    let entities = ENTITIES.get()?;
    entities
        .entities_matching(
            world,
            position.map(|n| n - range),
            position.map(|n| n + range),
            -1,
            ENTITY_PASSENGER,
        )
        .into_iter()
        .find(|&id| {
            entities.with_entity(world, id, |entity| {
                entity.type_id() == "item_display"
                    && entity.data(KEY).is_some()
                    && entity.view().is_some_and(|view| {
                        (0..3).all(|i| (view.position[i] - position[i]).abs() <= range)
                    })
            }) == Some(true)
        })
}

pub fn move_carrier(world: World<'_>, before: [f64; 3], after: [f64; 3]) {
    if before != after
        && let Some(id) = carrier_at(world, before, 0.1)
        && let Some(entities) = ENTITIES.get()
    {
        entities.with_entity(world, id, |entity| entity.set_position(after));
    }
}

fn passenger(world: World<'_>, position: [f64; 3], carrier: i32) -> Option<(i32, bool)> {
    let entities = ENTITIES.get()?;
    entities
        .entities_matching(
            world,
            position.map(|n| n - 2.0),
            position.map(|n| n + 2.0),
            carrier,
            0,
        )
        .into_iter()
        .find_map(|id| {
            entities
                .with_entity(world, id, |rider| {
                    (rider.vehicle() == Some(carrier)).then(|| {
                        (
                            id,
                            rider
                                .view()
                                .is_some_and(|view| view.kind & ENTITY_PLAYER != 0),
                        )
                    })
                })
                .flatten()
        })
}

fn sit_down(world: World<'_>, position: [f64; 3], rider: Entity<'_>) -> Option<i32> {
    let entities = ENTITIES.get()?;
    let carrier = entities.spawn_carrier(world, CARRIER_SEAT, position, KEY, TAG)?;
    if rider.start_riding(carrier, true) {
        Some(carrier)
    } else {
        entities.with_entity(world, carrier, |seat| seat.discard());
        None
    }
}

#[derive(serde::Deserialize)]
struct ItemId {
    id: String,
}

pub fn use_item_on(context: &UseBlock<'_>, selector: &str) -> BlockUseResult {
    let Some(player) = context.player else {
        return BlockUseResult::Pass;
    };
    if context.sneaking {
        return BlockUseResult::TryEmptyHand;
    }
    if let Some(item) = context
        .stack
        .read()
        .as_deref()
        .and_then(|data| fastnbt::from_bytes::<ItemId>(data).ok())
        && let Some(color) = item
            .id
            .strip_prefix("minecraft:")
            .and_then(|id| id.strip_suffix("_dye"))
        && let Some(index) = COLORS.iter().position(|&name| name == color)
        && let Some((block, props)) = selector.split_once('[')
        && IDS[index] != block
        && let Some(state) = CONTENT
            .get()
            .and_then(|content| content.state(&format!("{}[{props}", IDS[index])))
    {
        return if context.world.write(&[BlockUpdate {
            position: context.position,
            state,
            flags: content::UPDATE_ALL,
        }]) == 1
        {
            BlockUseResult::Success
        } else {
            BlockUseResult::Fail
        };
    }
    let world = context.world;
    let pos = position(context.position);
    if let Some(carrier) = carrier_at(world, pos, 0.1) {
        if let Some((rider, is_player)) = passenger(world, pos, carrier) {
            if is_player {
                return BlockUseResult::TryEmptyHand;
            }
            if let Some(entities) = ENTITIES.get() {
                entities.with_entity(world, rider, |rider| rider.stop_riding());
            }
        }
        return if player.start_riding(carrier, false) {
            BlockUseResult::Success
        } else {
            BlockUseResult::Fail
        };
    }
    if sit_down(world, pos, player).is_some() {
        BlockUseResult::Success
    } else {
        BlockUseResult::Fail
    }
}

pub fn on_landed(world: World<'_>, pos: Pos, entity: Entity<'_>) {
    let kind = entity.type_id();
    if entity.vehicle().is_some()
        || kind == "shulker"
        || !entity.view().is_some_and(|view| {
            view.kind & (ENTITY_PLAYER | ENTITY_LIVING) == ENTITY_LIVING
                && view.motion[1] < 0.0
                && view.position[1].floor() as i32 == pos.y
        })
    {
        return;
    }
    if TAGS.get().is_some_and(|tags| {
        tags.contains(
            "entity_type",
            "create:ignore_seat",
            &format!("minecraft:{kind}"),
        )
    }) {
        return;
    }
    let position = position(pos);
    if let Some(carrier) = carrier_at(world, position, 0.1) {
        if passenger(world, position, carrier).is_none() {
            entity.start_riding(carrier, false);
        }
    } else {
        sit_down(world, position, entity);
    }
}

pub fn event(world: World<'_>, ids: &StateIds, event: &ContentEvent) {
    if event.reason != content::STATE_CHANGED {
        return;
    }
    let is_seat_state = |state| {
        ids.by_id
            .get(&state)
            .is_some_and(|entry| is_seat(entry.block))
    };
    if is_seat_state(event.previous)
        && !is_seat_state(event.state)
        && let Some(carrier) = carrier_at(world, position(event.position), 0.1)
        && let Some(entities) = ENTITIES.get()
    {
        entities.with_entity(world, carrier, |seat| seat.discard());
    }
}
