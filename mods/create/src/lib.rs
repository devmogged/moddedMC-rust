use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    abi::MAX_WORLD_BATCH,
    commands::{self, Argument, Commands, Node},
    content::{self, Block, Content, ContentEvent, Placement, ShapeBox, State, World},
    items::{self, Item, Items},
};
use serde::{Deserialize, Serialize};

mod rotation;
use rotation::{FACES, OFFSETS, Piece, Rotation, face_axis, offset, ratio};

const AXES: [&str; 3] = ["x", "y", "z"];
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const SHAFT: &str = "create:shaft";
const SMALL: &str = "create:cogwheel";
const LARGE: &str = "create:large_cogwheel";
const MOTOR: &str = "create:creative_motor";
const ANDESITE_ALLOY: &str = "create:andesite_alloy";
const BRACKETED: &str = "create:simple_kinetic";
const DEFAULT_SPEED: i32 = 16;
const MAX_SPEED: i32 = 256;

static CONTENT: OnceLock<Content> = OnceLock::new();
static COMMANDS: OnceLock<Commands> = OnceLock::new();
static ITEMS: OnceLock<Items> = OnceLock::new();
static NETWORK_UPDATE: Mutex<()> = Mutex::new(());
static STATES: OnceLock<StateIds> = OnceLock::new();

type Relay = fn(u8) -> Piece;
const RELAYS: [(&str, &str, Relay); 3] = [
    (SHAFT, "Shaft", Piece::Shaft),
    (SMALL, "Cogwheel", Piece::Small),
    (LARGE, "Large Cogwheel", Piece::Large),
];

struct Create;

impl Mod for Create {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        let _ = COMMANDS.set(commands::require()?);
        let _ = ITEMS.set(items::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let content = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        ITEMS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add_item(
                registry,
                &Item {
                    id: ANDESITE_ALLOY,
                    display_name: "Andesite Alloy",
                    carrier: "minecraft:iron_ingot",
                    model: "create:item/andesite_alloy",
                },
            )?;
        for (id, name, relay) in RELAYS {
            let path = &id["create:".len()..];
            let item_model = format!("create:item/{path}");
            let rotating_model = format!("create:block/{path}");
            let selectors: Vec<_> = [1, 0, 2]
                .into_iter()
                .flat_map(|axis| {
                    let name = AXES[axis];
                    [false, true].map(move |wet| {
                        (
                            format!("axis={name},waterlogged={wet}"),
                            format!("minecraft:iron_chain[axis={name},waterlogged={wet}]"),
                            relay(axis as u8),
                        )
                    })
                })
                .collect();
            let shapes: Vec<_> = selectors
                .iter()
                .map(|(_, _, piece)| relay_shape(*piece))
                .collect();
            let states: Vec<_> = selectors
                .iter()
                .zip(&shapes)
                .map(|((properties, physical, piece), shape)| State {
                    properties,
                    physical,
                    model: "",
                    rotating_model: &rotating_model,
                    x: 0,
                    y: 0,
                    orientation: relay_orientation(piece.axis()),
                    shape,
                })
                .collect();
            content.add_block(
                registry,
                &Block {
                    id,
                    display_name: name,
                    entity: BRACKETED,
                    item_model: &item_model,
                    states: &states,
                },
                Some(placed_state),
            )?;
        }

        let model = "create:block/creative_motor/block";
        let vertical = "create:block/creative_motor/block_vertical";
        let motor_faces = [2, 3, 4, 5, 1, 0];
        let motor_shapes = motor_faces.map(|face| [motor_shape(face)]);
        let motor_states = [
            ("facing=north", model, 0, 180),
            ("facing=south", model, 0, 0),
            ("facing=west", model, 0, 90),
            ("facing=east", model, 0, 270),
            ("facing=up", vertical, 0, 0),
            ("facing=down", vertical, 180, 0),
        ];
        let motor_states: Vec<_> = motor_states
            .into_iter()
            .zip(motor_faces)
            .zip(&motor_shapes)
            .map(|(((properties, model, x, y), face), shape)| State {
                properties,
                physical: "minecraft:stone",
                model,
                rotating_model: "create:block/shaft_half",
                x,
                y,
                orientation: motor_orientation(face),
                shape,
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id: MOTOR,
                display_name: "Creative Motor",
                entity: "create:motor",
                item_model: "create:item/creative_motor",
                states: &motor_states,
            },
            Some(placed_state),
        )?;
        content.on_tick(registry, tick)?;

        COMMANDS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add(
                registry,
                "kinetics",
                "Read a kinetic block or change a creative motor's RPM",
                2,
                &[
                    number(None, "x", -30_000_000, 30_000_000, None),
                    number(Some(0), "y", -2_048, 2_047, None),
                    number(Some(1), "z", -30_000_000, 30_000_000, Some(0)),
                    number(Some(2), "rpm", -MAX_SPEED, MAX_SPEED, Some(1)),
                ],
            )
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let Some((content, commands)) = CONTENT.get().zip(COMMANDS.get()) else {
            return 0;
        };
        let (Some(x), Some(y), Some(z)) = (
            commands.integer(&command, "x"),
            commands.integer(&command, "y"),
            commands.integer(&command, "z"),
        ) else {
            return 0;
        };
        let Some(world) = content.command_world(&command) else {
            return 0;
        };
        let pos = Pos { x, y, z };
        let mut state = [0];
        if !world.read(&[pos], &mut state) {
            return 0;
        }
        let Some(block) = states().and_then(|states| states.by_id.get(&state[0])) else {
            command.reply("There is no loaded kinetic block at that position");
            return 0;
        };
        if tag == 1 {
            let Some(rpm) = commands.integer(&command, "rpm") else {
                return 0;
            };
            if !matches!(block.piece, Piece::Motor(..)) {
                command.reply("Only creative motors generate rotation");
                return 0;
            }
            update_network(world, [pos], Some((pos, rpm)));
        }
        let Some(data) = world
            .entity(pos)
            .and_then(|bytes| fastnbt::from_bytes::<KineticData>(&bytes).ok())
        else {
            command.reply("Kinetic block entity is unavailable");
            return 0;
        };
        command.reply(&format!(
            "{} at {x} {y} {z} Speed={}",
            block.selector, data.speed
        ));
        1
    }
}

// Create's AllShapes.SIX_VOXEL_POLE, SMALL_GEAR and LARGE_GEAR.
fn relay_shape(piece: Piece) -> Vec<ShapeBox> {
    let mut boxes = vec![ShapeBox {
        min: [5, 0, 5],
        max: [11, 16, 11],
    }];
    match piece {
        Piece::Small(_) => boxes.push(ShapeBox {
            min: [2, 6, 2],
            max: [14, 10, 14],
        }),
        Piece::Large(_) => boxes.push(ShapeBox {
            min: [0, 6, 0],
            max: [16, 10, 16],
        }),
        _ => {}
    }
    for bounds in &mut boxes {
        bounds.min.swap(1, usize::from(piece.axis()));
        bounds.max.swap(1, usize::from(piece.axis()));
    }
    boxes
}

fn motor_shape(face: u8) -> ShapeBox {
    let mut bounds = ShapeBox {
        min: [3, 3, 3],
        max: [13, 13, 13],
    };
    let axis = usize::from(face_axis(face));
    (bounds.min[axis], bounds.max[axis]) = if face.is_multiple_of(2) {
        (2, 16)
    } else {
        (0, 14)
    };
    bounds
}

const HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

const fn relay_orientation(axis: u8) -> [f32; 4] {
    match axis {
        0 => [0.0, 0.0, -HALF, HALF],
        2 => [HALF, 0.0, 0.0, HALF],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

const fn motor_orientation(face: u8) -> [f32; 4] {
    match face {
        0 => [HALF, 0.0, 0.0, HALF],
        1 => [-HALF, 0.0, 0.0, HALF],
        2 => [0.0, 1.0, 0.0, 0.0],
        4 => [0.0, -HALF, 0.0, HALF],
        5 => [0.0, HALF, 0.0, HALF],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

const fn number(parent: Option<u32>, name: &str, min: i32, max: i32, tag: Option<u64>) -> Node<'_> {
    Node {
        parent,
        name,
        argument: Argument::Integer { min, max },
        tag,
    }
}

struct KnownState {
    selector: String,
    piece: Piece,
}

#[derive(Default)]
struct StateIds {
    by_id: HashMap<u32, KnownState>,
    by_selector: HashMap<String, u32>,
}

impl StateIds {
    fn add(&mut self, content: Content, selector: String, piece: Piece) {
        if let Some(id) = content.state(&selector) {
            self.by_selector.insert(selector.clone(), id);
            self.by_id.insert(id, KnownState { selector, piece });
        }
    }
}

fn states() -> Option<&'static StateIds> {
    let content = *CONTENT.get()?;
    Some(STATES.get_or_init(|| {
        let mut ids = StateIds::default();
        for (name, _, relay) in RELAYS {
            for (index, axis) in AXES.into_iter().enumerate() {
                for wet in [false, true] {
                    ids.add(
                        content,
                        format!("{name}[axis={axis},waterlogged={wet}]"),
                        relay(index as u8),
                    );
                }
            }
        }
        for (face, name) in DIRECTIONS.into_iter().enumerate() {
            ids.add(
                content,
                format!("{MOTOR}[facing={name}]"),
                Piece::Motor(face as u8, DEFAULT_SPEED),
            );
        }
        ids
    }))
}

fn placed_state(args: Placement<'_>) -> Option<u32> {
    if usize::from(args.face) >= FACES.len() {
        return None;
    }
    let states = states()?;
    let piece = states.by_id.get(&args.state)?.piece;
    let mut positions = [args.position; 7];
    for (pos, step) in positions.iter_mut().zip(FACES) {
        *pos = offset(args.position, step);
    }
    let mut ids = [0; 7];
    if !args.world.read(&positions, &mut ids) {
        return None;
    }
    let connected = FACES.iter().enumerate().filter_map(|(face, _)| {
        let neighbor = states.by_id.get(&ids[face])?.piece;
        neighbor.shaft_towards(face as u8 ^ 1).then_some(face as u8)
    });
    let preferred = connected.clone().next_back().filter(|w| {
        connected
            .clone()
            .all(|face| face_axis(*w) == face_axis(face))
    });
    let look = nearest_looking_direction(args.yaw, args.pitch);
    let face_axis = face_axis(args.face);
    let wet = CONTENT.get()?.state("minecraft:water") == Some(ids[6]);
    let selector = match piece {
        Piece::Motor(..) => {
            let face = if args.sneak {
                look
            } else {
                preferred.unwrap_or(look ^ 1)
            };
            format!("{MOTOR}[facing={}]", DIRECTIONS[usize::from(face)])
        }
        Piece::Shaft(_) => {
            let axis = match (preferred, args.sneak) {
                (Some(_), true) => face_axis,
                (Some(face), false) => rotation::face_axis(face),
                (None, _) => rotation::face_axis(look),
            };
            format!(
                "{SHAFT}[axis={},waterlogged={wet}]",
                AXES[usize::from(axis)]
            )
        }
        Piece::Small(_) | Piece::Large(_) => {
            let against = states.by_id.get(&ids[usize::from(args.face ^ 1)]);
            let axis = if args.sneak {
                face_axis
            } else if let Some(KnownState {
                piece: Piece::Small(axis),
                ..
            }) = against
            {
                *axis
            } else {
                preferred.map(rotation::face_axis).unwrap_or(face_axis)
            };
            let id = if matches!(piece, Piece::Small(_)) {
                SMALL
            } else {
                LARGE
            };
            format!("{id}[axis={},waterlogged={wet}]", AXES[usize::from(axis)])
        }
    };
    states.by_selector.get(&selector).copied()
}

fn nearest_looking_direction(yaw: f32, pitch: f32) -> u8 {
    let (sin_yaw, cos_yaw) = yaw.to_radians().sin_cos();
    let (sin_pitch, cos_pitch) = pitch.to_radians().sin_cos();
    let x = -sin_yaw * cos_pitch;
    let y = -sin_pitch;
    let z = cos_yaw * cos_pitch;
    // Direction.orderedByNearest chooses X and Z before Y at ties.
    if x.abs() >= y.abs() && x.abs() >= z.abs() {
        if x > 0.0 { 5 } else { 4 }
    } else if z.abs() >= y.abs() {
        if z > 0.0 { 3 } else { 2 }
    } else if y > 0.0 {
        1
    } else {
        0
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct KineticData {
    #[serde(default)]
    speed: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<fastnbt::IntArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<KineticNetwork>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scroll_value: Option<i32>,
}

#[derive(Serialize, Deserialize)]
struct KineticNetwork {
    #[serde(rename = "Id")]
    id: i64,
}

const fn packed(pos: Pos) -> i64 {
    ((pos.x as i64 & 0x3ff_ffff) << 38)
        | ((pos.z as i64 & 0x3ff_ffff) << 12)
        | (pos.y as i64 & 0xfff)
}

fn piece_at(world: World<'_>, pos: Pos, piece: Piece, setting: Option<(Pos, i32)>) -> Piece {
    let Piece::Motor(face, _) = piece else {
        return piece;
    };
    let rpm = setting
        .filter(|(change, _)| *change == pos)
        .map(|(_, rpm)| rpm)
        .or_else(|| {
            let bytes = world.entity(pos)?;
            fastnbt::from_bytes::<KineticData>(&bytes)
                .ok()?
                .scroll_value
        })
        .unwrap_or(DEFAULT_SPEED)
        .clamp(-MAX_SPEED, MAX_SPEED);
    Piece::Motor(face, rpm)
}

fn tick(world: World<'_>, _time: u64, events: &[ContentEvent]) {
    let mut touched = events
        .iter()
        .filter(|event| event.reason != content::NEIGHBOR_CHANGED)
        .map(|event| event.position)
        .peekable();
    if touched.peek().is_some() {
        update_network(world, touched, None);
    }
}

fn discover(
    world: World<'_>,
    seeds: impl IntoIterator<Item = Pos>,
    state_ids: &StateIds,
) -> HashMap<Pos, Piece> {
    let roots: HashSet<_> = seeds.into_iter().collect();
    let mut frontier: HashSet<_> = roots
        .iter()
        .copied()
        .flat_map(|pos| {
            OFFSETS
                .into_iter()
                .map(move |step| offset(pos, step))
                .chain([pos])
        })
        .collect();
    let mut cache = HashMap::new();
    let mut nodes = HashMap::new();
    let mut state = vec![0; MAX_WORLD_BATCH];
    let mut first = true;
    while !frontier.is_empty() {
        let positions: Vec<_> = frontier.drain().collect();
        for part in positions.chunks(MAX_WORLD_BATCH) {
            if world.read(part, &mut state[..part.len()]) {
                cache.extend(
                    part.iter().copied().zip(
                        state
                            .iter()
                            .map(|id| state_ids.by_id.get(id).map(|block| block.piece)),
                    ),
                );
            }
        }
        let mut candidates = Vec::new();
        if first {
            candidates.extend(
                positions
                    .into_iter()
                    .filter_map(|pos| Some((pos, (*cache.get(&pos)?)?))),
            );
            first = false;
        } else {
            for pos in positions {
                let Some(Some(piece)) = cache.get(&pos).copied() else {
                    continue;
                };
                if OFFSETS.into_iter().any(|step| {
                    nodes
                        .get(&offset(pos, step))
                        .is_some_and(|other| ratio(piece, *other, step).is_some())
                }) {
                    candidates.push((pos, piece));
                }
            }
        }

        while let Some((pos, piece)) = candidates.pop() {
            if nodes.insert(pos, piece).is_some() {
                continue;
            }
            for step in OFFSETS {
                let next = offset(pos, step);
                match cache.get(&next) {
                    Some(Some(other))
                        if !nodes.contains_key(&next) && ratio(piece, *other, step).is_some() =>
                    {
                        candidates.push((next, *other));
                    }
                    None => {
                        frontier.insert(next);
                    }
                    _ => {}
                }
            }
        }
    }
    nodes
}

fn update_network(
    world: World<'_>,
    seeds: impl IntoIterator<Item = Pos>,
    setting: Option<(Pos, i32)>,
) {
    let _update = NETWORK_UPDATE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(states) = states() else { return };
    let mut nodes = discover(world, seeds, states);
    for (&pos, piece) in &mut nodes {
        *piece = piece_at(world, pos, *piece, setting);
    }
    for (
        pos,
        Rotation {
            speed,
            source,
            network,
        },
    ) in rotation::solve(&nodes)
    {
        let piece = nodes[&pos];
        let data = KineticData {
            speed,
            source: source.map(|pos| fastnbt::IntArray::new(vec![pos.x, pos.y, pos.z])),
            network: network.map(|pos| KineticNetwork { id: packed(pos) }),
            scroll_value: if let Piece::Motor(_, rpm) = piece {
                Some(rpm)
            } else {
                None
            },
        };
        if let Ok(bytes) = fastnbt::to_bytes(&data) {
            let _ = world.set_entity(pos, &bytes);
        }
        let _ = world.rotate(pos, piece.axis(), speed);
    }
}

moddedmc_api::export_mod!(Create);
