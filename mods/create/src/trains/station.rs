//! Station placement follows TrackTargetingBlockItem and TrackTargetingBehaviour.
use super::{
    bogey::{AssemblyTrack, Placement as BogeyPlacement},
    railway, runtime, signal,
    targeting::{
        Selection, TargetUse, coords, directed_edge, place_block, placement, pos, sound, state_at,
        use_target,
    },
    track::TrackShape,
};
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, ShapeBox, State, World},
    interaction::{BlockUseResult, UseBlock, UseOnBlock, UseResult},
    saved_data::{Key, SavedData},
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub(crate) const ID: &str = "create:track_station";
const SHAPE: &[ShapeBox] = &[
    ShapeBox {
        min: [0, 0, 0],
        max: [16, 2, 16],
    },
    ShapeBox {
        min: [1, 0, 1],
        max: [15, 13, 15],
    },
];
static SAVE: OnceLock<(SavedData, Key)> = OnceLock::new();
static STATE_IDS: OnceLock<[Option<u32>; 4]> = OnceLock::new();
const PROPERTIES: [&str; 4] = [
    "assembling=false,waterlogged=false",
    "assembling=false,waterlogged=true",
    "assembling=true,waterlogged=false",
    "assembling=true,waterlogged=true",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Station {
    dimension: String,
    position: [i32; 3],
    target: [i32; 3],
    front: bool,
    name: String,
}

#[derive(Default, Deserialize, Serialize)]
struct Stations {
    #[serde(rename = "Stations")]
    stations: Vec<Station>,
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
    saved: SavedData,
) -> Result<(), RegistrationError> {
    let key = saved.register(registry, "create_stations")?;
    SAVE.set((saved, key))
        .map_err(|_| RegistrationError::DuplicateId)?;
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Train Station",
            item_model: "create:block/track_station/item",
            entity: ID,
            states: &PROPERTIES.map(|properties| State {
                properties,
                physical: if properties.ends_with("true") {
                    "minecraft:stone_slab[type=bottom,waterlogged=true]"
                } else {
                    "minecraft:stone_slab[type=bottom,waterlogged=false]"
                },
                model: "create:block/track_station/block",
                rotating_model: if properties.starts_with("assembling=true") {
                    "create:block/track_station/flag_assemble"
                } else {
                    "create:block/track_station/flag_off"
                },
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: SHAPE,
            }),
        },
        None,
    )
}

fn read(world: World<'_>) -> Option<Stations> {
    let &(save, key) = SAVE.get()?;
    match save.read(&world, key) {
        Some(bytes) => fastnbt::from_bytes(&bytes).ok(),
        None => Some(Stations::default()),
    }
}

fn write(world: World<'_>, stations: &Stations) -> bool {
    let (Some(&(save, key)), Ok(bytes)) = (SAVE.get(), fastnbt::to_bytes(stations)) else {
        return false;
    };
    save.write(&world, key, &bytes)
}

fn station_states(content: Content) -> &'static [Option<u32>; 4] {
    STATE_IDS
        .get_or_init(|| PROPERTIES.map(|properties| content.state(&format!("{ID}[{properties}]"))))
}

fn is_station(content: Content, state: u32) -> bool {
    station_states(content).contains(&Some(state))
}

fn occupied(stations: &Stations, selection: &Selection) -> bool {
    // Straight track centers are separated by at least one block.
    stations.stations.iter().any(|station| {
        station.dimension == selection.dimension && station.target == selection.target
    })
}

pub(crate) fn use_on_block(context: &mut UseOnBlock<'_>, content: Content) -> UseResult {
    match use_target(context, content, |world, selection| {
        !claimed(world, selection) && !signal::claimed(world, selection)
    }) {
        TargetUse::Handled(result) => result,
        TargetUse::Place(selection, clicked) => {
            let world = context.world;
            crate::with_world(world, |_| place(context, content, selection, clicked))
        }
    }
}

fn place(
    context: &mut UseOnBlock<'_>,
    content: Content,
    selection: Selection,
    clicked: u32,
) -> UseResult {
    let world = context.world;
    let Some(mut stations) = read(world) else {
        return UseResult::Fail;
    };
    let Some(placed) = placement(context, content, &selection, clicked) else {
        return UseResult::Fail;
    };
    if occupied(&stations, &selection) || signal::claimed(world, &selection) {
        return UseResult::Fail;
    }
    let position = placed.0;
    let target = coords(position);
    let wet = world.fluid_id(position).as_deref() == Some("minecraft:water");
    let Some(state) = station_states(content)[usize::from(wet)] else {
        return UseResult::Fail;
    };

    let station = Station {
        dimension: selection.dimension.clone(),
        position: target,
        target: selection.target,
        front: selection.front,
        name: "Track Station".to_owned(),
    };
    stations.stations.push(station);
    place_block(context, &selection, placed, state, || {
        write(world, &stations)
    })
}

pub(super) fn claimed(world: World<'_>, selection: &Selection) -> bool {
    read(world).is_none_or(|stations| occupied(&stations, selection))
}

pub(crate) fn use_station(context: &UseBlock<'_>, content: Content) -> Option<BlockUseResult> {
    let row = station_states(content)
        .iter()
        .position(|&state| state == Some(context.state))?;
    if context.sneaking {
        return Some(BlockUseResult::Pass);
    }
    if context.with_item {
        return Some(if context.stack.read().is_none() {
            BlockUseResult::TryEmptyHand
        } else {
            BlockUseResult::Pass
        });
    }
    let world = context.world;
    let position = context.position;
    let can_edit = context.creative || context.player.is_some_and(|player| player.may_build());
    if !can_edit {
        return Some(BlockUseResult::Fail);
    }
    Some(crate::with_world(world, |_| {
        if row < 2 {
            let Some(dimension) = world.dimension_id() else {
                return BlockUseResult::Fail;
            };
            let Some(station) = station_at(world, dimension, position) else {
                return BlockUseResult::Fail;
            };
            let selection = station.selection();
            if assembly_track(world, content, &station).is_none()
                || !railway::read(&world)
                    .is_ok_and(|data| directed_edge(&data, world, content, &selection).is_some())
            {
                return BlockUseResult::Fail;
            }
        }
        let Some(state) = station_states(content)[row ^ 2] else {
            return BlockUseResult::Fail;
        };
        if world.write(&[BlockUpdate {
            position,
            state,
            flags: content::UPDATE_ALL,
        }]) != 1
        {
            return BlockUseResult::Fail;
        }
        sound(world, position, "minecraft:block.lever.click", 1.0);
        BlockUseResult::Success
    }))
}

impl Station {
    fn selection(&self) -> Selection {
        Selection {
            dimension: self.dimension.clone(),
            target: self.target,
            front: self.front,
        }
    }
}

fn station_at(world: World<'_>, dimension: &str, position: Pos) -> Option<Station> {
    read(world)?
        .stations
        .into_iter()
        .find(|station| station.dimension == dimension && station.position == coords(position))
}

fn assembly_track(world: World<'_>, content: Content, station: &Station) -> Option<AssemblyTrack> {
    let anchor = pos(station.target);
    let state = state_at(world, anchor)?;
    let (shape, _) = runtime::track_state(content, state)?;
    let sign = if station.front { 1 } else { -1 };
    let step = match shape {
        TrackShape::XOrtho => [sign, 0, 0],
        TrackShape::ZOrtho => [0, 0, sign],
        _ => return None,
    };
    Some(AssemblyTrack {
        anchor,
        step,
        shape,
        waterlogged: runtime::track_waterlogged(content, state),
    })
}

fn bogey(world: World<'_>, content: Content, position: Pos, has_casing: bool) -> BogeyPlacement {
    let (Some(dimension), Some(data)) = (world.dimension_id(), read(world)) else {
        return BogeyPlacement::None;
    };
    for station in data
        .stations
        .iter()
        .filter(|station| station.dimension == dimension)
    {
        let Some(station_state) = state_at(world, pos(station.position)) else {
            continue;
        };
        if !station_states(content)[2..].contains(&Some(station_state)) {
            continue;
        }
        let Some(track) = assembly_track(world, content, station) else {
            continue;
        };
        let placement = track.place(world, content, position, has_casing);
        if !matches!(placement, BogeyPlacement::None) {
            return placement;
        }
    }
    BogeyPlacement::None
}

pub(crate) fn use_casing(context: &mut UseOnBlock<'_>, content: Content) -> UseResult {
    let world = context.world;
    if !state_at(world, context.position)
        .is_some_and(|state| runtime::track_state(content, state).is_some())
    {
        return UseResult::Pass;
    }
    crate::with_world(world, |_| {
        match bogey(world, content, context.position, true) {
            BogeyPlacement::None => UseResult::Pass,
            BogeyPlacement::Rejected => UseResult::Fail,
            BogeyPlacement::Resized => UseResult::Success,
            BogeyPlacement::Place => {
                if !context.creative {
                    context.stack.shrink(1);
                }
                UseResult::Success
            }
        }
    })
}

pub(crate) fn use_assembly_track(
    context: &UseBlock<'_>,
    content: Content,
) -> Option<BlockUseResult> {
    runtime::track_state(content, context.state)?;
    if context.with_item {
        return (context.stack.read().is_none()).then_some(BlockUseResult::TryEmptyHand);
    }
    let world = context.world;
    let result = crate::with_world(world, |_| {
        bogey(world, content, context.position, context.creative)
    });
    match result {
        BogeyPlacement::Place | BogeyPlacement::Resized => Some(BlockUseResult::Success),
        BogeyPlacement::None | BogeyPlacement::Rejected => None,
    }
}

pub(crate) fn tick(world: World<'_>, content: Content, events: &[ContentEvent]) {
    let removed: Vec<_> = events
        .iter()
        .filter_map(|event| {
            (matches!(
                event.reason,
                content::STATE_CHANGED | content::MOVED_STATE_CHANGED
            ) && is_station(content, event.previous))
            .then_some(event.position)
        })
        .filter(|&pos| state_at(world, pos).is_some_and(|state| !is_station(content, state)))
        .collect();
    if removed.is_empty() {
        return;
    }
    let (Some(dimension), Some(mut stations)) = (world.dimension_id(), read(world)) else {
        return;
    };
    let count = stations.stations.len();
    stations.stations.retain(|station| {
        station.dimension != dimension || !removed.contains(&pos(station.position))
    });
    if stations.stations.len() != count && !write(world, &stations) {
        moddedmc_api::warn("Could not save the updated train stations");
    }
}

pub(crate) fn describe(world: World<'_>, content: Content, position: Pos) -> Option<String> {
    let dimension = world.dimension_id()?;
    if !is_station(content, state_at(world, position)?) {
        return None;
    }
    let station = station_at(world, dimension, position)?;
    let [x, y, z] = station.target;
    let selection = station.selection();
    let graph = railway::read(&world).ok()?;
    let status = if let Some((edge, distance)) = directed_edge(&graph, world, content, &selection) {
        format!("Connected at {distance:.2}/{:.2}", edge.length())
    } else {
        "Disconnected".to_owned()
    };
    let assembling = state_at(world, position)
        .is_some_and(|state| station_states(content)[2..].contains(&Some(state)));
    let mode = if assembling { " Assembling" } else { "" };
    Some(format!(
        "{} targets {x} {y} {z} {status}{mode}",
        station.name
    ))
}
