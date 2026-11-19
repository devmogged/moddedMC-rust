use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    commands::{self, Argument, Commands, Node},
    content::{
        self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World,
        WorldExtension,
    },
    entities::{
        self, ENTITY_LIVING, ENTITY_PLAYER, Entities, EntitiesExtension, Entity, EntityChange,
    },
    interaction::{self, Interaction},
    items::{self, Item, Items},
    stacks::{self, Displays, ItemStacks},
};
use serde::{Deserialize, Serialize};

mod belt;
mod belt_inventory;
mod belt_movement;
mod kinetics;
use kinetics::{Be, FACES, Kind, Kinetics, Level, face_axis, offset};

const AXES: [&str; 3] = ["x", "y", "z"];
const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const SHAFT: &str = "create:shaft";
const SMALL: &str = "create:cogwheel";
const LARGE: &str = "create:large_cogwheel";
const MOTOR: &str = "create:creative_motor";
const GEARBOX: &str = "create:gearbox";
const CLUTCH: &str = "create:clutch";
const GEARSHIFT: &str = "create:gearshift";
const ENCASED_SHAFT: &str = "create:andesite_encased_shaft";
const CASING: &str = "create:andesite_casing";
const TURNTABLE: &str = "create:turntable";
const ANDESITE_ALLOY: &str = "create:andesite_alloy";
const BRACKETED: &str = "create:simple_kinetic";
/// `CreativeMotorBlockEntity.MAX_SPEED`.
const MAX_SPEED: i32 = 256;
/// `SharedProperties.stone()`.
const STONE: &str = "minecraft:andesite";

static CONTENT: OnceLock<Content> = OnceLock::new();
static COMMANDS: OnceLock<Commands> = OnceLock::new();
static ITEMS: OnceLock<Items> = OnceLock::new();
static ENTITIES: OnceLock<Entities> = OnceLock::new();
static INTERACTION: OnceLock<Interaction> = OnceLock::new();
static STACKS: OnceLock<ItemStacks> = OnceLock::new();
static DISPLAYS: OnceLock<Displays> = OnceLock::new();
static STATES: OnceLock<StateIds> = OnceLock::new();
/// Each loaded world's kinetic block entities and networks, by `World::id`.
static WORLDS: Mutex<Option<HashMap<usize, Kinetics>>> = Mutex::new(None);

type Relay = fn(u8) -> Kind;
const RELAYS: [(&str, &str, Relay); 3] = [
    (SHAFT, "Shaft", Kind::Shaft),
    (SMALL, "Cogwheel", Kind::Small),
    (LARGE, "Large Cogwheel", Kind::Large),
];

/// `axis=` blockstate rotations Create's `axisBlock` generator writes: x, y, z.
const AXIS_ROTATIONS: [(i16, i16); 3] = [(90, 90), (0, 0), (90, 180)];

struct Create;

impl Mod for Create {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_extension(WorldExtension::NeighborSignal)?;
        content.require_extension(WorldExtension::Destroy)?;
        content.require_extension(WorldExtension::FaceSturdy)?;
        let _ = CONTENT.set(content);
        let _ = COMMANDS.set(commands::require()?);
        let _ = ITEMS.set(items::require()?);
        let entities = entities::require()?;
        entities.require_extension(EntitiesExtension::Movement)?;
        let _ = ENTITIES.set(entities);
        let _ = INTERACTION.set(interaction::require()?);
        let _ = STACKS.set(stacks::require()?);
        let _ = DISPLAYS.set(stacks::require_displays()?);
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
        register_relays(content, registry)?;
        register_motor(content, registry)?;
        register_casings(content, registry)?;
        belt::register(content, registry)?;
        ITEMS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add_item(
                registry,
                &Item {
                    id: belt::BELT_CONNECTOR,
                    display_name: "Mechanical Belt",
                    carrier: "minecraft:leather",
                    model: "create:item/belt_connector",
                },
            )?;
        INTERACTION
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .on_use_on_block(registry, belt::use_on_block)?;
        content.on_tick(registry, tick)?;
        ENTITIES
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .on_entity_inside(registry, entity_inside)?;

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
        let (Some(world), Some(states)) = (content.command_world(&command), states()) else {
            return 0;
        };
        let pos = Pos { x, y, z };
        let mut state = [0];
        if !world.read(&[pos], &mut state) {
            return 0;
        }
        let Some(known) = states.by_id.get(&state[0]).filter(|s| s.kind.is_some()) else {
            command.reply("There is no loaded kinetic block at that position");
            return 0;
        };
        with_world(world, |kinetics| {
            if tag == 1 {
                let Some(rpm) = commands.integer(&command, "rpm") else {
                    return 0;
                };
                if !matches!(known.kind, Some(Kind::Motor(_))) {
                    command.reply("Only creative motors generate rotation");
                    return 0;
                }
                kinetics.set_generated(&mut WorldLevel { world, states }, pos, rpm);
                flush(world, kinetics);
            }
            let Some(be) = kinetics.get(pos) else {
                command.reply("Kinetic block entity is unavailable");
                return 0;
            };
            let overstressed = if be.overstressed { " Overstressed" } else { "" };
            command.reply(&format!(
                "{} at {x} {y} {z} Speed={} Stress={} Capacity={}{overstressed}",
                known.selector, be.speed, be.stress, be.capacity
            ));
            1
        })
    }
}

fn with_world<T>(world: World<'_>, f: impl FnOnce(&mut Kinetics) -> T) -> T {
    let mut worlds = WORLDS.lock().unwrap_or_else(PoisonError::into_inner);
    f(worlds
        .get_or_insert_with(HashMap::new)
        .entry(world.id())
        .or_default())
}

fn register_relays(
    content: &Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
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
            .map(|(_, _, kind)| relay_shape(*kind))
            .collect();
        let states: Vec<_> = selectors
            .iter()
            .zip(&shapes)
            .map(|((properties, physical, kind), shape)| State {
                properties,
                physical,
                model: "",
                rotating_model: &rotating_model,
                x: 0,
                y: 0,
                orientation: relay_orientation(kind.axis()),
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
    Ok(())
}

fn register_motor(content: &Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
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
    )
}

/// Properties, model, x and y rotation, and the axis the rotating part turns about.
type StateRow = (String, String, i16, i16, u8);

/// The gearbox, the split shafts, the encased shaft, the casing and the turntable.
fn register_casings(
    content: &Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let axis_states = |model: &str| -> Vec<StateRow> {
        [1u8, 0, 2]
            .into_iter()
            .map(|axis| {
                let (x, y) = AXIS_ROTATIONS[usize::from(axis)];
                (
                    format!("axis={}", AXES[usize::from(axis)]),
                    model.to_owned(),
                    x,
                    y,
                    axis,
                )
            })
            .collect()
    };
    let add = |registry: &mut Registry<'_>,
               id: &str,
               name: &str,
               entity: &str,
               rotating: &str,
               states: &[StateRow],
               shape: &[ShapeBox],
               physical: &str|
     -> Result<(), RegistrationError> {
        let states: Vec<_> = states
            .iter()
            .map(|(properties, model, x, y, axis)| State {
                properties,
                physical,
                model,
                rotating_model: rotating,
                x: *x,
                y: *y,
                orientation: relay_orientation(*axis),
                shape,
            })
            .collect();
        let item_model = format!("create:item/{}", &id["create:".len()..]);
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity,
                item_model: &item_model,
                states: &states,
            },
            Some(placed_state),
        )
    };
    add(
        registry,
        GEARBOX,
        "Gearbox",
        "create:gearbox",
        "",
        &axis_states("create:block/gearbox/block"),
        &[],
        STONE,
    )?;
    for (id, name, entity, folder) in [
        (CLUTCH, "Clutch", "create:clutch", "clutch"),
        (GEARSHIFT, "Gearshift", "create:gearshift", "gearshift"),
    ] {
        let states: Vec<_> = [1u8, 0, 2]
            .into_iter()
            .flat_map(|axis| {
                let (x, y) = AXIS_ROTATIONS[usize::from(axis)];
                [false, true].map(|powered| {
                    let suffix = if powered { "_powered" } else { "" };
                    (
                        format!("axis={},powered={powered}", AXES[usize::from(axis)]),
                        format!("create:block/{folder}/block{suffix}"),
                        x,
                        y,
                        axis,
                    )
                })
            })
            .collect();
        add(registry, id, name, entity, "", &states, &[], STONE)?;
    }
    add(
        registry,
        ENCASED_SHAFT,
        "Andesite Encased Shaft",
        "create:encased_shaft",
        "create:block/shaft",
        &axis_states("create:block/encased_shaft/block_andesite"),
        &[],
        STONE,
    )?;
    let single = |model: &str| vec![(String::new(), model.to_owned(), 0, 0, 1)];
    add(
        registry,
        CASING,
        "Andesite Casing",
        "",
        "",
        &single("create:block/andesite_casing"),
        &[],
        STONE,
    )?;
    // AllShapes.TURNTABLE_SHAPE.
    add(
        registry,
        TURNTABLE,
        "Turntable",
        "create:turntable",
        "create:block/turntable",
        &single(""),
        &[
            ShapeBox {
                min: [1, 4, 1],
                max: [15, 8, 15],
            },
            ShapeBox {
                min: [5, 0, 5],
                max: [11, 4, 11],
            },
        ],
        "minecraft:stripped_spruce_wood[axis=y]",
    )
}

// Create's AllShapes.SIX_VOXEL_POLE, SMALL_GEAR and LARGE_GEAR.
fn relay_shape(kind: Kind) -> Vec<ShapeBox> {
    let mut boxes = vec![ShapeBox {
        min: [5, 0, 5],
        max: [11, 16, 11],
    }];
    match kind {
        Kind::Small(_) => boxes.push(ShapeBox {
            min: [2, 6, 2],
            max: [14, 10, 14],
        }),
        Kind::Large(_) => boxes.push(ShapeBox {
            min: [0, 6, 0],
            max: [16, 10, 16],
        }),
        _ => {}
    }
    for bounds in &mut boxes {
        bounds.min.swap(1, usize::from(kind.axis()));
        bounds.max.swap(1, usize::from(kind.axis()));
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
    block: &'static str,
    /// `None` for the casing, which has no kinetic behaviour.
    kind: Option<Kind>,
}

#[derive(Default)]
struct StateIds {
    by_id: HashMap<u32, KnownState>,
    by_selector: HashMap<String, u32>,
    /// Vanilla blocks a belt may run through (`canBeReplaced`).
    replaceable: HashSet<u32>,
    /// Vanilla blocks with `getDestroySpeed == -1`.
    unbreakable: HashSet<u32>,
    /// States holding water: water itself and waterlogged content states.
    water: HashSet<u32>,
    /// Vanilla states the belt code writes, by selector.
    vanilla: HashMap<String, u32>,
}

impl StateIds {
    fn add(&mut self, content: Content, block: &'static str, properties: &str, kind: Option<Kind>) {
        let selector = if properties.is_empty() {
            block.to_owned()
        } else {
            format!("{block}[{properties}]")
        };
        if let Some(id) = content.state(&selector) {
            self.by_selector.insert(selector.clone(), id);
            self.by_id.insert(
                id,
                KnownState {
                    selector,
                    block,
                    kind,
                },
            );
        }
    }

    fn kind(&self, state: u32) -> Option<Kind> {
        self.by_id.get(&state)?.kind
    }

    /// A content or recorded vanilla state by exact selector.
    fn lookup(&self, selector: &str) -> Option<u32> {
        self.by_selector
            .get(selector)
            .or_else(|| self.vanilla.get(selector))
            .copied()
    }

    /// `ProperWaterloggedBlock.withWater`: whether this state holds a water source.
    fn wet(&self, state: u32) -> bool {
        self.water.contains(&state)
    }

    /// The state for a clutch or gearshift `POWERED` toggle.
    fn state_of(&self, kind: Kind) -> Option<u32> {
        let (block, axis, powered) = match kind {
            Kind::Clutch(axis, powered) => (CLUTCH, axis, powered),
            Kind::Gearshift(axis, powered) => (GEARSHIFT, axis, powered),
            _ => return None,
        };
        self.by_selector
            .get(&format!(
                "{block}[axis={},powered={powered}]",
                AXES[usize::from(axis)]
            ))
            .copied()
    }
}

fn states() -> Option<&'static StateIds> {
    let content = *CONTENT.get()?;
    Some(STATES.get_or_init(|| {
        let mut ids = StateIds::default();
        for (block, _, relay) in RELAYS {
            for (index, axis) in AXES.into_iter().enumerate() {
                for wet in [false, true] {
                    ids.add(
                        content,
                        block,
                        &format!("axis={axis},waterlogged={wet}"),
                        Some(relay(index as u8)),
                    );
                }
            }
        }
        for (face, name) in DIRECTIONS.into_iter().enumerate() {
            ids.add(
                content,
                MOTOR,
                &format!("facing={name}"),
                Some(Kind::Motor(face as u8)),
            );
        }
        for (index, axis) in AXES.into_iter().enumerate() {
            let axis_index = index as u8;
            ids.add(
                content,
                GEARBOX,
                &format!("axis={axis}"),
                Some(Kind::Gearbox(axis_index)),
            );
            ids.add(
                content,
                ENCASED_SHAFT,
                &format!("axis={axis}"),
                Some(Kind::EncasedShaft(axis_index)),
            );
            for powered in [false, true] {
                let properties = format!("axis={axis},powered={powered}");
                ids.add(
                    content,
                    CLUTCH,
                    &properties,
                    Some(Kind::Clutch(axis_index, powered)),
                );
                ids.add(
                    content,
                    GEARSHIFT,
                    &properties,
                    Some(Kind::Gearshift(axis_index, powered)),
                );
            }
        }
        ids.add(content, TURNTABLE, "", Some(Kind::Turntable));
        ids.add(content, CASING, "", None);
        belt::add_states(&mut ids, content);
        for name in [
            "air",
            "cave_air",
            "void_air",
            "water",
            "short_grass",
            "fern",
            "dead_bush",
        ] {
            if let Some(id) = content.state(&format!("minecraft:{name}")) {
                ids.replaceable.insert(id);
                ids.vanilla.insert(format!("minecraft:{name}"), id);
            }
        }
        for name in [
            "bedrock",
            "barrier",
            "end_portal_frame",
            "reinforced_deepslate",
        ] {
            if let Some(id) = content.state(&format!("minecraft:{name}")) {
                ids.unbreakable.insert(id);
            }
        }
        ids.water = ids
            .by_id
            .iter()
            .filter(|(_, known)| known.selector.contains("waterlogged=true"))
            .map(|(id, _)| *id)
            .chain(content.state("minecraft:water"))
            .collect();
        ids
    }))
}

/// Each block's `getStateForPlacement`.
fn placed_state(args: Placement<'_>) -> Option<u32> {
    if usize::from(args.face) >= FACES.len() {
        return None;
    }
    let states = states()?;
    let placed = states.by_id.get(&args.state)?;
    let mut positions = [args.position; 7];
    for (pos, step) in positions.iter_mut().zip(FACES) {
        *pos = offset(args.position, step);
    }
    let mut ids = [0; 7];
    if !args.world.read(&positions, &mut ids) {
        return None;
    }
    // RotatedPillarKineticBlock.getPreferredAxis
    let connected = (0..6u8).filter(|&face| {
        states
            .kind(ids[usize::from(face)])
            .is_some_and(|neighbor| neighbor.has_shaft_towards(face ^ 1))
    });
    let preferred = connected
        .clone()
        .next_back()
        .filter(|w| {
            connected
                .clone()
                .all(|face| face_axis(*w) == face_axis(face))
        })
        .map(face_axis);
    let look = nearest_looking_direction(args.yaw, args.pitch);
    let clicked_axis = face_axis(args.face);
    let wet = CONTENT.get()?.state("minecraft:water") == Some(ids[6]);
    // RotatedPillarKineticBlock.getStateForPlacement
    let pillar_axis = match (preferred, args.sneak) {
        (Some(axis), false) => axis,
        (Some(_), true) => clicked_axis,
        (None, _) => face_axis(look),
    };
    let selector = match placed.kind {
        Some(Kind::Motor(_)) => {
            let face = if args.sneak {
                look
            } else {
                connected
                    .clone()
                    .next_back()
                    .filter(|_| preferred.is_some())
                    .unwrap_or(look ^ 1)
            };
            format!("{MOTOR}[facing={}]", DIRECTIONS[usize::from(face)])
        }
        Some(Kind::Shaft(_)) => format!(
            "{SHAFT}[axis={},waterlogged={wet}]",
            AXES[usize::from(pillar_axis)]
        ),
        Some(Kind::Small(_) | Kind::Large(_)) => {
            // CogWheelBlock.getAxisForPlacement
            let against = states.kind(ids[usize::from(args.face ^ 1)]);
            let axis = if args.sneak {
                clicked_axis
            } else if let Some(Kind::Small(axis)) = against {
                axis
            } else {
                preferred.unwrap_or(clicked_axis)
            };
            format!(
                "{}[axis={},waterlogged={wet}]",
                placed.block,
                AXES[usize::from(axis)]
            )
        }
        Some(Kind::Gearbox(_)) => format!("{GEARBOX}[axis=y]"),
        // AbstractEncasedShaftBlock.getStateForPlacement
        Some(Kind::EncasedShaft(_)) => format!(
            "{ENCASED_SHAFT}[axis={}]",
            AXES[usize::from(encased_axis(preferred, pillar_axis, look, args.sneak))]
        ),
        Some(Kind::Clutch(..) | Kind::Gearshift(..)) => format!(
            "{}[axis={},powered={}]",
            placed.block,
            AXES[usize::from(encased_axis(preferred, pillar_axis, look, args.sneak))],
            args.world.has_neighbor_signal(args.position)
        ),
        Some(Kind::Turntable | Kind::Belt(_)) | None => return Some(args.state),
    };
    states.by_selector.get(&selector).copied()
}

/// `AbstractEncasedShaftBlock.getStateForPlacement`: sneaking falls back to the pillar rule.
fn encased_axis(preferred: Option<u8>, pillar_axis: u8, look: u8, sneak: bool) -> u8 {
    if sneak {
        pillar_axis
    } else {
        preferred.unwrap_or_else(|| face_axis(look))
    }
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

/// `KineticBlockEntity.write` and `read`, with Create's field names.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct KineticData {
    #[serde(default)]
    speed: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<fastnbt::IntArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network: Option<NetworkData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scroll_value: Option<i32>,
    // BeltBlockEntity.write and read.
    #[serde(skip_serializing_if = "Option::is_none")]
    controller: Option<fastnbt::IntArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    is_controller: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    length: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    casing: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    covered: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    inventory: Option<belt_inventory::BeltInventory>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct NetworkData {
    id: i64,
    #[serde(default)]
    stress: f32,
    #[serde(default)]
    capacity: f32,
    #[serde(default)]
    size: i32,
    #[serde(default, skip_serializing_if = "is_zero")]
    added_stress: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    added_capacity: f32,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes a reference"
)]
fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

fn load(world: World<'_>, pos: Pos, kind: Kind) -> Be {
    let mut be = Be::new(kind);
    let Some(data) = world
        .entity(pos)
        .and_then(|bytes| fastnbt::from_bytes::<KineticData>(&bytes).ok())
    else {
        return be;
    };
    be.speed = data.speed;
    be.source = data.source.and_then(|pos| match *pos {
        [x, y, z] => Some(Pos { x, y, z }),
        _ => None,
    });
    if let Some(network) = data.network {
        be.network = Some(network.id);
        be.stress = network.stress;
        be.capacity = network.capacity;
        be.network_size = network.size;
        be.last_stress_applied = network.added_stress;
        be.last_capacity_provided = network.added_capacity;
        be.overstressed = be.capacity < be.stress;
    }
    if let Some(value) = data.scroll_value {
        be.generated = value.clamp(-MAX_SPEED, MAX_SPEED);
    }
    if matches!(kind, Kind::Belt(_)) {
        be.controller = if data.is_controller == Some(true) {
            Some(pos)
        } else {
            data.controller.and_then(|pos| match *pos {
                [x, y, z] => Some(Pos { x, y, z }),
                _ => None,
            })
        };
        be.belt_length = data.length.unwrap_or(0);
        be.belt_index = data.index.unwrap_or(0);
        if be.controller == Some(pos) {
            be.inventory = data.inventory;
        }
    }
    be
}

fn save(pos: Pos, be: &Be) -> KineticData {
    KineticData {
        speed: be.speed,
        source: be
            .source
            .map(|pos| fastnbt::IntArray::new(vec![pos.x, pos.y, pos.z])),
        network: be.network.map(|id| NetworkData {
            id,
            stress: be.stress,
            capacity: be.capacity,
            size: be.network_size,
            added_stress: be.last_stress_applied,
            added_capacity: be.last_capacity_provided,
        }),
        scroll_value: matches!(be.kind, Kind::Motor(_)).then_some(be.generated),
        ..belt_data(pos, be)
    }
}

fn belt_data(pos: Pos, be: &Be) -> KineticData {
    if !matches!(be.kind, Kind::Belt(_)) {
        return KineticData::default();
    }
    KineticData {
        controller: be
            .controller
            .map(|pos| fastnbt::IntArray::new(vec![pos.x, pos.y, pos.z])),
        is_controller: Some(be.controller == Some(pos)),
        length: Some(be.belt_length),
        index: Some(be.belt_index),
        casing: Some("NONE".to_owned()),
        covered: Some(false),
        inventory: (be.controller == Some(pos)).then(|| be.inventory.clone().unwrap_or_default()),
        ..KineticData::default()
    }
}

struct WorldLevel<'a> {
    world: World<'a>,
    states: &'static StateIds,
}

impl Level for WorldLevel<'_> {
    fn destroy(&mut self, pos: Pos) {
        self.world.destroy(pos, true);
    }

    fn has_neighbor_signal(&self, pos: Pos) -> bool {
        self.world.has_neighbor_signal(pos)
    }

    fn is_loaded(&self, pos: Pos) -> bool {
        let mut state = [content::UNLOADED_STATE];
        self.world.read(&[pos], &mut state) && state[0] != content::UNLOADED_STATE
    }

    fn replace(&mut self, pos: Pos, kind: Option<Kind>) {
        let mut current = [content::UNLOADED_STATE];
        if !self.world.read(&[pos], &mut current) {
            return;
        }
        if let Some(state) = belt::leftover(self.states, kind, self.states.wet(current[0])) {
            self.world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: content::UPDATE_ALL,
            }]);
        }
    }

    fn set_kind(&mut self, pos: Pos, kind: Kind) {
        if let Some(state) = self.states.state_of(kind) {
            self.world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: content::UPDATE_CLIENTS,
            }]);
        }
    }
}

fn tick(world: World<'_>, _time: u64, events: &[ContentEvent]) {
    let Some(states) = states() else { return };
    with_world(world, |kinetics| {
        let mut level = WorldLevel { world, states };
        for event in events {
            let pos = event.position;
            let (before, after) = (states.kind(event.previous), states.kind(event.state));
            match event.reason {
                content::LOADED => {
                    if let Some(kind) = after {
                        kinetics.insert(pos, load(world, pos, kind));
                    }
                }
                content::UNLOADED => kinetics.unload(pos),
                content::NEIGHBOR_CHANGED => kinetics.neighbor_changed(&mut level, pos),
                _ => match (before, after) {
                    (Some(before), Some(after)) if before.equivalent(after) => {
                        kinetics.set_kind(pos, after);
                    }
                    (Some(before), Some(after))
                        if std::mem::discriminant(&before) == std::mem::discriminant(&after) =>
                    {
                        kinetics.reset(pos, after);
                    }
                    _ => {
                        if before.is_some() {
                            kinetics.remove(&mut level, pos);
                        }
                        if let Some(kind) = after {
                            kinetics.insert(pos, Be::new(kind));
                        }
                    }
                },
            }
        }
        kinetics.tick(&mut level);
        belt_movement::tick(world, kinetics, states);
        belt_inventory::tick(world, kinetics);
        flush(world, kinetics);
    });
}

fn entity_inside(world: World<'_>, pos: Pos, state: u32, entity: Entity<'_>) {
    match states().and_then(|states| states.kind(state)) {
        Some(Kind::Turntable) => turntable_entity_inside(world, pos, entity),
        Some(Kind::Belt(belt)) => with_world(world, |kinetics| {
            belt_movement::entity_inside(kinetics, pos, state, belt, entity);
        }),
        _ => {}
    }
}

/// `TurntableBlock.entityInside`. Players turn on their own client, so the server leaves them alone.
fn turntable_entity_inside(world: World<'_>, pos: Pos, entity: Entity<'_>) {
    let Some(view) = entity.view() else { return };
    if !view.on_ground || view.motion[1] > 0.0 || view.position[1] < f64::from(pos.y as f32 + 0.5) {
        return;
    }
    let speed = with_world(world, |kinetics| kinetics.get(pos).map(Be::get_speed)).unwrap_or(0.0)
        * 3.0
        / 10.0;
    if speed == 0.0 || view.kind & ENTITY_PLAYER != 0 {
        return;
    }
    let mut change = EntityChange {
        mask: entities::SET_YAW,
        yaw: view.yaw - speed,
        ..EntityChange::default()
    };
    if view.kind & ENTITY_LIVING != 0 {
        // setNoActionTime(20) has no Pumpkin counterpart yet; hurtMarked resends the motion.
        let diff = view.head_yaw - speed;
        change.mask |= entities::SET_BODY_YAW
            | entities::SET_HEAD_YAW
            | entities::SET_ON_GROUND
            | entities::SET_MOTION;
        change.body_yaw = diff;
        change.head_yaw = diff;
        change.on_ground = false;
        change.motion = view.motion;
    }
    entity.change(&change);
}

/// Save changed block entities and show their speed (`getSpeed`, zero when overstressed).
fn flush(world: World<'_>, kinetics: &mut Kinetics) {
    for (pos, be) in kinetics.drain_dirty() {
        if let Ok(bytes) = fastnbt::to_bytes(&save(pos, &be)) {
            let _ = world.set_entity(pos, &bytes);
        }
        let _ = world.rotate(pos, be.kind.axis(), be.get_speed());
    }
}

moddedmc_api::export_mod!(Create);
