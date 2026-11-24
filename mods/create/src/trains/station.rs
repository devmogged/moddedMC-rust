//! Station placement follows TrackTargetingBlockItem and TrackTargetingBehaviour.
use super::{graph::TrackEdge, item, math::Vec3, railway, runtime, save::RailwayData};
use fastnbt::IntArray;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, ShapeBox, State, World},
    effects::SOUND_BLOCKS,
    interaction::{UseOnBlock, UseResult},
    saved_data::{Key, SavedData},
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

pub(crate) const ID: &str = "create:track_station";
const RANGE: i64 = 16;
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
struct Selection {
    dimension: String,
    target: [i32; 3],
    front: bool,
}

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

#[derive(Deserialize, Serialize)]
struct StationNbt {
    #[serde(rename = "TargetTrack")]
    target: IntArray,
    #[serde(rename = "TargetDirection")]
    front: bool,
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
                rotating_model: "",
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

fn pos([x, y, z]: [i32; 3]) -> Pos {
    Pos { x, y, z }
}

fn coords(pos: Pos) -> [i32; 3] {
    [pos.x, pos.y, pos.z]
}

fn state_at(world: World<'_>, pos: Pos) -> Option<u32> {
    let mut state = [content::UNLOADED_STATE];
    (world.read(&[pos], &mut state) && state[0] != content::UNLOADED_STATE).then_some(state[0])
}

fn station_states(content: Content) -> &'static [Option<u32>; 4] {
    STATE_IDS
        .get_or_init(|| PROPERTIES.map(|properties| content.state(&format!("{ID}[{properties}]"))))
}

fn is_station(content: Content, state: u32) -> bool {
    station_states(content).contains(&Some(state))
}

fn directed_edge<'a>(
    data: &'a RailwayData,
    world: World<'_>,
    content: Content,
    target: &Selection,
) -> Option<(&'a TrackEdge, f64)> {
    let position = pos(target.target);
    let (shape, _) = runtime::track_state(content, state_at(world, position)?)?;
    let [axis] = shape.axes() else { return None };
    let center = Vec3 {
        x: f64::from(position.x) + 0.5,
        y: f64::from(position.y) + if axis.y == 0.0 { 0.0 } else { 0.5 },
        z: f64::from(position.z) + 0.5,
    };
    let wanted = axis
        .normalized()
        .scale(if target.front { -1.0 } else { 1.0 });

    data.graphs
        .iter()
        .flat_map(|saved| saved.graph.connections.values())
        .flat_map(|edges| edges.values())
        .find_map(|edge| {
            if edge.turn.is_some()
                || edge.from.dimension != target.dimension
                || edge.to.dimension != target.dimension
            {
                return None;
            }
            let from = edge.from.world();
            let direction = edge.to.world().minus(from).normalized();
            if direction.dot(wanted) < 1.0 - 1.0 / 4096.0 {
                return None;
            }
            let offset = center.minus(from);
            let distance = offset.dot(direction);
            let error = offset.minus(direction.scale(distance));
            ((0.0..edge.length()).contains(&distance) && error.dot(error) < 1.0 / 4096.0)
                .then_some((edge, distance))
        })
}

fn occupied(stations: &Stations, selection: &Selection) -> bool {
    // Straight track centers are separated by at least one block.
    stations.stations.iter().any(|station| {
        station.dimension == selection.dimension && station.target == selection.target
    })
}

pub(crate) fn use_on_block(context: &mut UseOnBlock<'_>, content: Content) -> UseResult {
    let (Some(player), Some(clicked)) = (context.player, state_at(context.world, context.position))
    else {
        return UseResult::Fail;
    };
    let Some(dimension) = context.world.dimension_id() else {
        return UseResult::Fail;
    };

    let saved = context
        .stack
        .data()
        .and_then(|bytes| fastnbt::from_bytes::<Selection>(&bytes).ok());
    if context.sneaking && saved.is_some() {
        return if context.stack.set_data(&[]) {
            sound(
                context.world,
                context.position,
                "minecraft:block.lever.click",
                0.5,
            );
            UseResult::Success
        } else {
            UseResult::Fail
        };
    }
    if let Some((shape, _)) = runtime::track_state(content, clicked) {
        let (Some(view), [axis]) = (player.view(), shape.axes()) else {
            return UseResult::Fail;
        };
        let (sin, cos) = view.yaw.to_radians().sin_cos();
        let selected = Selection {
            dimension: dimension.to_owned(),
            target: coords(context.position),
            front: (-f64::from(sin) * axis.x + f64::from(cos) * axis.z) < 0.0,
        };
        let (Ok(graphs), Some(stations)) = (railway::read(&context.world), read(context.world))
        else {
            return UseResult::Fail;
        };
        let Ok(bytes) = fastnbt::to_bytes(&selected) else {
            return UseResult::Fail;
        };
        return if !occupied(&stations, &selected)
            && directed_edge(&graphs, context.world, content, &selected).is_some()
            && context.stack.set_data(&bytes)
        {
            sound(
                context.world,
                context.position,
                "minecraft:block.lever.click",
                1.0,
            );
            UseResult::Success
        } else {
            UseResult::Fail
        };
    }

    let Some(selection) = saved.filter(|saved| saved.dimension == dimension) else {
        return UseResult::Fail;
    };
    let world = context.world;
    crate::with_world(world, |_| place(context, content, selection, clicked))
}

fn place(
    context: &mut UseOnBlock<'_>,
    content: Content,
    selection: Selection,
    clicked: u32,
) -> UseResult {
    let (Some(states), Some(mut stations)) = (crate::states(), read(context.world)) else {
        return UseResult::Fail;
    };
    let world = context.world;
    let position = if states.replaceable.contains(&clicked) {
        context.position
    } else {
        let Some(pos) = item::relative(context.position, context.face) else {
            return UseResult::Fail;
        };
        pos
    };
    let Some(previous) =
        state_at(world, position).filter(|state| states.replaceable.contains(state))
    else {
        return UseResult::Fail;
    };
    let target = coords(position);
    let offsets =
        std::array::from_fn::<_, 3, _>(|i| i64::from(selection.target[i]) - i64::from(target[i]));
    if offsets.iter().any(|&offset| offset.abs() >= RANGE)
        || offsets.iter().map(|offset| offset * offset).sum::<i64>() >= RANGE * RANGE
        || occupied(&stations, &selection)
        || !railway::read(&world)
            .is_ok_and(|data| directed_edge(&data, world, content, &selection).is_some())
    {
        return UseResult::Fail;
    }

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
    let Ok(nbt) = fastnbt::to_bytes(&StationNbt {
        target: IntArray::new(offsets.map(|offset| offset as i32).to_vec()),
        front: station.front,
    }) else {
        return UseResult::Fail;
    };
    let Ok(selected) = fastnbt::to_bytes(&selection) else {
        return UseResult::Fail;
    };
    if !context.stack.set_data(&[]) {
        return UseResult::Fail;
    }

    stations.stations.push(station);
    let set_state = |state| {
        world.write(&[BlockUpdate {
            position,
            state,
            flags: content::UPDATE_ALL,
        }]) == 1
    };
    if !set_state(state) || !world.set_entity(position, &nbt) || !write(world, &stations) {
        set_state(previous);
        context.stack.set_data(&selected);
        return UseResult::Fail;
    }
    if !context.creative {
        context.stack.shrink(1);
    }
    sound(world, position, "minecraft:block.stone.place", 0.8);
    UseResult::Success
}

fn sound(world: World<'_>, position: Pos, sound: &str, pitch: f32) {
    if let Some(effects) = crate::EFFECTS.get() {
        effects.play_sound(
            world,
            coords(position).map(|axis| f64::from(axis) + 0.5),
            sound,
            SOUND_BLOCKS,
            0.75,
            pitch,
        );
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
    let station = read(world)?
        .stations
        .into_iter()
        .find(|station| station.dimension == dimension && station.position == coords(position))?;
    let [x, y, z] = station.target;
    let selection = Selection {
        dimension: dimension.to_owned(),
        target: station.target,
        front: station.front,
    };
    let graph = railway::read(&world).ok()?;
    let status = if let Some((edge, distance)) = directed_edge(&graph, world, content, &selection) {
        format!("Connected at {distance:.2}/{:.2}", edge.length())
    } else {
        "Disconnected".to_owned()
    };
    Some(format!("{} targets {x} {y} {z} {status}", station.name))
}
