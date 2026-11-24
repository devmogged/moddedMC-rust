use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    block_displays::{self, BlockDisplays},
    commands::{self, Argument, Commands, Node},
    containers::{self, Containers},
    content::{
        self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World,
        WorldExtension,
    },
    effects::{self, Effects},
    entities::{
        self, ENTITY_LIVING, ENTITY_PLAYER, Entities, EntitiesExtension, Entity, EntityChange,
    },
    interaction::{
        self, BlockUseResult, Interaction, InteractionExtension, UseBlock, UseOnBlock, UseResult,
    },
    items::{self, Item, Items},
    resources::{self, Resources},
    stacks::{self, Displays, ItemStacks},
    tags::{self, Tags},
};
use serde::{Deserialize, Serialize};

mod basin;
pub mod basin_inventory;
mod bearing;
mod belt;
mod belt_inventory;
mod belt_movement;
mod blaze_burner;
mod chassis;
mod chute;
mod collider;
mod contraption;
mod crusher;
mod depot;
mod drill;
mod elevator;
mod fan;
mod funnel;
// WS-2 Create fluids
pub mod fluids;
mod gantry;
mod glue;
mod harvester;
mod kinetics;
mod manual_application;
mod millstone;
mod mixer;
mod piston;
mod press;
mod processing_recipes;
mod pulley;
mod seat;
mod sticker;
mod trains;
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
const COPPER_CASING: &str = "create:copper_casing";
const TURNTABLE: &str = "create:turntable";
const ANDESITE_ALLOY: &str = "create:andesite_alloy";
const WRENCH: &str = "create:wrench";
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
static BLOCK_DISPLAYS: OnceLock<BlockDisplays> = OnceLock::new();
static EFFECTS: OnceLock<Effects> = OnceLock::new();
static CONTAINERS: OnceLock<Containers> = OnceLock::new();
static RESOURCES: OnceLock<Resources> = OnceLock::new();
static TAGS: OnceLock<Tags> = OnceLock::new();
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
        content.require_replaced()?;
        fluids::tank_connectivity::require()?;
        fluids::open_end_runtime::require()?;
        content.require_extension(WorldExtension::NeighborSignal)?;
        content.require_extension(WorldExtension::Destroy)?;
        content.require_extension(WorldExtension::FaceSturdy)?;
        content.require_extension(WorldExtension::CollisionShape)?;
        content.require_extension(WorldExtension::DescribeState)?;
        content.require_extension(WorldExtension::FluidId)?;
        content.require_extension(WorldExtension::BlockEntities)?;
        content.require_extension(WorldExtension::Survival)?;
        content.require_extension(WorldExtension::BlockDrops)?;
        let _ = CONTENT.set(content);
        // WS-3: save-global Create railway data through the generic ABI.
        trains::require_saved()?;
        let _ = COMMANDS.set(commands::require()?);
        let _ = ITEMS.set(items::require()?);
        let entities = entities::require()?;
        entities.require_extension(EntitiesExtension::Movement)?;
        entities.require_extension(EntitiesExtension::Data)?;
        entities.require_extension(EntitiesExtension::MayBuild)?;
        entities.require_extension(EntitiesExtension::Carriers)?;
        let _ = ENTITIES.set(entities);
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        interaction.require_extension(InteractionExtension::VanillaItemUse)?;
        interaction.require_extension(InteractionExtension::PlaceHeldBlock)?;
        interaction.require_extension(InteractionExtension::Redstone)?;
        interaction.require_extension(InteractionExtension::PlayerWillDestroy)?;
        let _ = INTERACTION.set(interaction);
        let _ = EFFECTS.set(effects::require()?);
        let item_stacks = stacks::require()?;
        item_stacks.require_extension(stacks::StacksExtension::Component)?;
        item_stacks.require_extension(stacks::StacksExtension::Fuel)?;
        let _ = STACKS.set(item_stacks);
        let _ = DISPLAYS.set(stacks::require_displays()?);
        let _ = BLOCK_DISPLAYS.set(block_displays::require()?);
        let _ = CONTAINERS.set(containers::require()?);
        let _ = RESOURCES.set(resources::require()?);
        let _ = TAGS.set(tags::require()?);
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
        // WS-3: official Create 6.0.10 track blockstates and placement.
        trains::register(content, registry)?;
        trains::register_saved(registry)?;
        belt::register(content, registry)?;
        depot::register(*content, registry)?;
        chute::register(*content, registry)?;
        funnel::register(*content, registry)?;
        press::register(*content, registry)?;
        basin::register(*content, registry)?;
        mixer::register(*content, registry)?;
        blaze_burner::register(*content, registry)?;
        // WS-2 Create fluids
        fluids::blocks::register(*content, registry)?;
        fluids::tank_runtime::register(*content, registry)?;
        fluids::tank_connectivity::register(registry)?;
        content.on_replaced(registry, content_replaced)?;
        fluids::item_drain::register(*content, registry)?;
        fluids::spout::register(*content, registry)?;
        fluids::valve::register(*content, registry)?;
        fluids::hose_pulley::register(*content, registry)?;
        fan::register(*content, registry)?;
        bearing::register(*content, registry)?;
        piston::register(*content, registry)?;
        gantry::register(*content, registry)?;
        pulley::register(*content, registry)?;
        elevator::register(*content, registry)?;
        chassis::register(*content, registry)?;
        sticker::register(*content, registry)?;
        glue::register(registry)?;
        seat::register(*content, registry)?;
        drill::register(*content, registry)?;
        harvester::register(*content, registry)?;
        millstone::register(*content, registry)?;
        crusher::register(*content, registry)?;
        ITEMS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add_item(
                registry,
                &Item {
                    id: WRENCH,
                    display_name: "Wrench",
                    carrier: "minecraft:stick",
                    model: "create:item/wrench",
                },
            )?;
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
        let interaction = INTERACTION
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?;
        interaction.on_use_on_block(registry, use_on_block)?;
        interaction.on_vanilla_use_on_block(registry, manual_application::use_on_block)?;
        interaction.on_use_block(registry, use_block)?;
        interaction.on_redstone(registry, elevator::signal, elevator::connects)?;
        interaction.on_player_will_destroy(registry, piston::player_will_destroy)?;
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
            )?;
        // WS-2: read Create's exact pipe state for behavioral scenarios.
        // WS-3: read-only track state inspection for placement/restart scenarios.
        COMMANDS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add(
                registry,
                "fluidpipe",
                "Inspect the registered state of a Create fluid pipe or pump",
                2,
                &[
                    number(None, "x", -30_000_000, 30_000_000, None),
                    number(Some(0), "y", -2_048, 2_047, None),
                    number(Some(1), "z", -30_000_000, 30_000_000, Some(200)),
                ],
            )?;
        COMMANDS
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add(
                registry,
                "track",
                "Inspect a loaded Create railway track blockstate",
                2,
                &[
                    number(None, "x", -30_000_000, 30_000_000, None),
                    number(Some(0), "y", -2_048, 2_047, None),
                    number(Some(1), "z", -30_000_000, 30_000_000, Some(2)),
                ],
            )?;
        // WS-3: read the saved railway root, or rebuild at a known-dimension
        // admin command position while world callback dimension is unavailable.
        for (name, help, tag) in [
            ("railway", "Inspect stored Create railway networks", 3),
            (
                "railwaybuild",
                "Build and save a Create railway at a rail position",
                4,
            ),
        ] {
            COMMANDS
                .get()
                .ok_or(RegistrationError::InvalidDefinition)?
                .add(
                    registry,
                    name,
                    help,
                    2,
                    &[
                        number(None, "x", -30_000_000, 30_000_000, None),
                        number(Some(0), "y", -2_048, 2_047, None),
                        number(Some(1), "z", -30_000_000, 30_000_000, Some(tag)),
                    ],
                )?;
        }
        Ok(())
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
        if tag == 200 {
            match fluids::runtime::inspect(world, pos) {
                Some(selector) => command.reply(&format!(
                    "{selector} at {x} {y} {z}{}",
                    fluids::runtime::pressure_text(world, pos)
                )),
                None => command.reply("There is no loaded Create fluid block at that position"),
            }
            return 1;
        }
        if tag == 3 || tag == 4 {
            let result = if tag == 3 {
                trains::summary(&world)
            } else {
                trains::rebuild_at(&world, &command, *content, pos)
            };
            match result {
                Ok(message) => {
                    command.reply(&message);
                    return 1;
                }
                Err(message) => {
                    command.reply(&message);
                    return 0;
                }
            }
        }
        let mut state = [0];
        if !world.read(&[pos], &mut state) {
            return 0;
        }
        // WS-3: exact internal Create track selector, without vanilla command parsing.
        if tag == 2 {
            if let Some(selector) = trains::describe_state(*content, state[0]) {
                command.reply(&format!("{selector} at {x} {y} {z}"));
                return 1;
            }
            command.reply("There is no loaded Create track at that position");
            return 0;
        }
        let Some(states) = states() else {
            return 0;
        };
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
    add(
        registry,
        COPPER_CASING,
        "Copper Casing",
        "",
        "",
        &single("create:block/copper_casing"),
        &[],
        "minecraft:copper_block",
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
    contraption_blocks: contraption::BlockMovementChecks,
    chutes: HashMap<u32, chute::ChuteState>,
    funnels: HashMap<u32, funnel::FunnelState>,
    contacts: HashMap<u32, elevator::Contact>,
    crusher_controllers: HashMap<u32, (u8, bool)>,
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
            for wet in [false, true] {
                ids.add(
                    content,
                    "create:mechanical_pump",
                    &format!("facing={name},waterlogged={wet}"),
                    Some(Kind::Pump(face as u8)),
                );
            }
        }
        for valve in fluids::valve::variants() {
            ids.add(
                content,
                fluids::valve::ID,
                &valve.properties(),
                Some(Kind::Valve(valve.shaft_axis())),
            );
        }
        for facing in fluids::hose_pulley::FACINGS {
            ids.add(
                content,
                fluids::hose_pulley::ID,
                &format!("facing={}", DIRECTIONS[usize::from(facing)]),
                Some(Kind::HosePulley(facing)),
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
        for properties in ["waterlogged=false", "waterlogged=true"] {
            ids.add(content, depot::ID, properties, None);
        }
        belt::add_states(&mut ids, content);
        chute::add_states(content, &mut ids);
        funnel::add_states(content, &mut ids);
        press::add_states(content, &mut ids);
        basin::add_states(content, &mut ids);
        ids.add(content, mixer::ID, "", Some(Kind::Mixer));
        fan::add_states(content, &mut ids);
        bearing::add_states(content, &mut ids);
        seat::add_states(content, &mut ids);
        drill::add_states(content, &mut ids);
        harvester::add_states(content, &mut ids);
        piston::add_states(content, &mut ids);
        gantry::add_states(content, &mut ids);
        pulley::add_states(content, &mut ids);
        elevator::add_states(content, &mut ids);
        chassis::add_states(content, &mut ids);
        sticker::add_states(content, &mut ids);
        ids.contraption_blocks.register_states(content);
        millstone::add_states(content, &mut ids);
        crusher::add_states(content, &mut ids);
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
    if let Some(state) = blaze_burner::placed(&args) {
        return Some(state);
    }
    // WS-2 fluid blocks choose connected faces, axis, and waterlogged state.
    if let Some(state) = fluids::runtime::placed_state(&args) {
        return Some(state);
    }
    // WS-3: train tracks use their own orientation and support checks.
    if let Some(track) = CONTENT
        .get()
        .and_then(|content| trains::placed_state(&args, *content))
    {
        return Some(track);
    }
    let states = states()?;
    if states.chutes.contains_key(&args.state) {
        return chute::placed_state(args, states);
    }
    if let Some(&default) = states.funnels.get(&args.state) {
        return funnel::placed_state(args, default, states);
    }
    let placed = states.by_id.get(&args.state)?;
    if seat::is_seat(placed.block) {
        return seat::placed_state(args, placed.block);
    }
    if placed.block == harvester::ID {
        return harvester::placed_state(&args, nearest_looking_direction(args.yaw, 0.0));
    }
    if placed.block == press::ID {
        return press::placed_state(args, states);
    }
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
        Some(Kind::HosePulley(_)) => {
            return fluids::hose_pulley::placed_state(&args, &ids, states);
        }
        None if placed.block == sticker::ID => {
            return sticker::placed_state(
                args.world,
                args.position,
                if args.sneak { look } else { look ^ 1 },
            );
        }
        None if chassis::is_chassis(placed.block) => {
            return chassis::placed_state(
                args.world,
                placed.block,
                args.position,
                args.face,
                face_axis(look),
                args.sneak,
            );
        }
        None if placed.block == elevator::CONTACT => {
            return elevator::placed_contact(&args, look, states);
        }
        Some(Kind::Elevator(_)) => return elevator::placed_pulley(args.yaw),
        Some(Kind::Pulley(_)) => {
            let horizontal = connected
                .filter(|&direction| face_axis(direction) != 1)
                .map(face_axis);
            let preferred = horizontal
                .clone()
                .next()
                .filter(|first| horizontal.clone().all(|axis| *first == axis));
            let look_axis = face_axis(nearest_looking_direction(args.yaw, 0.0));
            return pulley::placed_state(preferred.unwrap_or(look_axis ^ 2));
        }
        Some(Kind::GantryShaft(_)) => return gantry::place_shaft(args, states, &ids, look),
        Some(Kind::GantryCarriage(_)) => {
            return gantry::place_carriage(args.face ^ 1, states.kind(ids[usize::from(args.face)]));
        }
        Some(Kind::Piston(block)) => {
            let facing = if args.sneak { look } else { look ^ 1 };
            return piston::placed_state(args, facing, connected, block.sticky);
        }
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
        Some(Kind::Fan(_)) => {
            let toward_support = args.face;
            let face = if states
                .chutes
                .contains_key(&ids[usize::from(toward_support)])
            {
                toward_support
            } else if states
                .chutes
                .contains_key(&ids[usize::from(toward_support ^ 1)])
            {
                toward_support ^ 1
            } else {
                let direction = connected
                    .clone()
                    .next_back()
                    .filter(|_| preferred.is_some())
                    .unwrap_or(look);
                direction ^ u8::from(!args.sneak)
            };
            format!("{}[facing={}]", fan::ID, DIRECTIONS[usize::from(face)])
        }
        Some(Kind::Bearing(_) | Kind::Windmill(_) | Kind::Drill(_) | Kind::Clockwork(_)) => {
            let face = if args.sneak {
                look
            } else {
                connected
                    .clone()
                    .next_back()
                    .filter(|_| preferred.is_some())
                    .unwrap_or(look)
                    ^ 1
            };
            let direction = DIRECTIONS[usize::from(face)];
            if placed.block == drill::ID {
                format!("{}[facing={direction},waterlogged={wet}]", placed.block)
            } else {
                format!("{}[facing={direction}]", placed.block)
            }
        }
        None if placed.block == piston::POLE => return piston::pole_placement(args, wet),
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
        Some(Kind::Wheel(_)) => {
            for face in 0..6u8 {
                let block = ids[usize::from(face)];
                if let Some(Kind::Wheel(other_axis)) = states.kind(block) {
                    if other_axis != pillar_axis || face_axis(face) != pillar_axis {
                        return None;
                    }
                } else if face_axis(face) != pillar_axis
                    && states.crusher_controllers.contains_key(&block)
                {
                    return None;
                }
            }
            format!("{}[axis={}]", crusher::ID, AXES[usize::from(pillar_axis)])
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
        None if placed.block == depot::ID => {
            format!("{}[waterlogged={}]", depot::ID, states.wet(ids[6]))
        }
        Some(Kind::Turntable | Kind::Belt(_) | Kind::Press(_) | Kind::Pump(_) | Kind::Valve(_))
        | Some(Kind::Millstone | Kind::Mixer)
        | None => {
            return Some(args.state);
        }
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
    pointer: Option<fluids::valve::Pointer>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    press: Option<press::PressingData>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    mixer: Option<mixer::MixerData>,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    millstone: Option<millstone::MillstoneData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bearing: Option<bearing::BearingData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    piston: Option<piston::PistonData>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    gantries: Vec<gantry::GantryContraption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pulley: Option<pulley::PulleyData>,
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
    be.valve = data.pointer;
    if let Some(press) = &mut be.press {
        press.data = data.press.unwrap_or_default();
    }
    if kind == Kind::Mixer {
        be.mixer = data.mixer.map(mixer::Mixer::load).map(Box::new);
    }
    if kind == Kind::Millstone {
        be.millstone = data.millstone.map(millstone::Millstone::load).map(Box::new);
    }
    if kind.is_bearing()
        && let Some(data) = data.bearing
    {
        let mut bearing = bearing::MechanicalBearing::default();
        bearing.data = data;
        if matches!(kind, Kind::Windmill(_)) {
            be.generated = bearing.get_generated_speed();
        }
        be.bearing = Some(Box::new(bearing));
    }
    if matches!(kind, Kind::Piston(_))
        && let Some(data) = data.piston
    {
        let mut piston = piston::MechanicalPiston::default();
        piston.data = data;
        be.piston = Some(Box::new(piston));
    }
    if matches!(kind, Kind::GantryShaft(_)) {
        be.gantries = data.gantries;
    }
    if matches!(kind, Kind::Pulley(_) | Kind::Elevator(_))
        && let Some(data) = data.pulley
    {
        let mut pulley = pulley::RopePulley::default();
        pulley.data = data;
        be.pulley = Some(Box::new(pulley));
    }
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
        pointer: be.valve,
        press: be.press.as_ref().map(|press| press.data),
        mixer: be.mixer.as_ref().map(|mixer| mixer.data.clone()),
        millstone: be.millstone.as_ref().map(|millstone| millstone.save()),
        bearing: be.bearing.as_ref().map(|bearing| bearing.data.clone()),
        piston: be.piston.as_ref().map(|piston| piston.data.clone()),
        gantries: be.gantries.clone(),
        pulley: be.pulley.as_ref().map(|pulley| pulley.data.clone()),
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
        inventory: (be.controller == Some(pos)).then(|| {
            be.inventory
                .as_ref()
                .map_or_else(Default::default, belt_inventory::BeltInventory::captured)
        }),
        ..KineticData::default()
    }
}

fn restore_moved_belt(world: World<'_>, pos: Pos, saved: &[u8]) -> bool {
    let Ok(saved) = fastnbt::from_bytes::<KineticData>(saved) else {
        return false;
    };
    let data = KineticData {
        is_controller: saved.is_controller,
        inventory: saved.inventory,
        ..KineticData::default()
    };
    fastnbt::to_bytes(&data).is_ok_and(|bytes| world.set_entity(pos, &bytes))
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

fn tick(world: World<'_>, time: u64, events: &[ContentEvent]) {
    let Some(states) = states() else { return };
    with_world(world, |kinetics| {
        let mut level = WorldLevel { world, states };
        let mut gantry_changed = false;
        for event in events {
            // WS-2: recalculate fluid pipe faces when adjacent blocks change.
            fluids::runtime::event(world, event);
            fluids::item_drain::event(world, &mut kinetics.item_drains, event);
            fluids::hose_pulley::event(world, &mut kinetics.hose_pulleys, event);
            blaze_burner::event(world, &mut kinetics.blaze_burners, event);
            fluids::spout::event(world, &mut kinetics.spouts, event);
            depot::event(world, &mut kinetics.depots, states, event);
            chute::event(world, &mut kinetics.chutes, states, event);
            funnel::event(world, &mut kinetics.funnels, states, event);
            crusher::event(world, states, kinetics, event);
            pulley::event(world, states, event);
            elevator::event(world, states, event);
            chassis::event(world, states, event);
            sticker::event(world, states, event);
            seat::event(world, states, event);
            if states
                .by_id
                .get(&event.state)
                .is_some_and(|entry| entry.block == harvester::ID)
            {
                harvester::event(world, event);
            }
            let pos = event.position;
            let (before, after) = (states.kind(event.previous), states.kind(event.state));
            if before == Some(Kind::Mixer)
                && after != before
                && let Some(mixer) = kinetics.get_mut(pos).and_then(|be| be.mixer.as_mut())
            {
                mixer.clear(world);
            }
            gantry_changed |= matches!(before, Some(Kind::GantryShaft(_)))
                || matches!(after, Some(Kind::GantryShaft(_)));
            match event.reason {
                content::LOADED => {
                    if let Some(kind) = after {
                        kinetics.insert(pos, load(world, pos, kind));
                    }
                }
                content::UNLOADED => {
                    if let Some(be) = kinetics.get_mut(pos) {
                        if let Some(bearing) = &mut be.bearing {
                            bearing.clear_displays(world);
                        }
                        if let Some(piston) = &mut be.piston {
                            piston.displays.clear(world);
                        }
                        for gantry in &mut be.gantries {
                            gantry.clear_displays(world);
                        }
                        if let Some(pulley) = &mut be.pulley {
                            pulley.clear_displays(world);
                        }
                    }
                    kinetics.unload(pos);
                }
                content::NEIGHBOR_CHANGED => {
                    if let Some(be) = kinetics.get_mut(pos)
                        && matches!(be.kind, Kind::Fan(_))
                    {
                        be.fan = None;
                    }
                    kinetics.neighbor_changed(&mut level, pos);
                }
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
                            if event.reason == content::MOVED_STATE_CHANGED {
                                kinetics.remove_moved(&mut level, pos);
                            } else {
                                kinetics.remove(&mut level, pos);
                            }
                        }
                        if let Some(kind) = after {
                            let mut be = Be::new(kind);
                            if matches!(kind, Kind::Belt(belt) if belt.part == kinetics::Part::Start)
                            {
                                be.inventory = load(world, pos, kind).inventory;
                            }
                            kinetics.insert(pos, be);
                        }
                    }
                },
            }
        }
        if gantry_changed {
            gantry::refresh_lines(world, &mut level, kinetics);
        }
        kinetics.tick(&mut level);
        kinetics.fluid_tanks.tick(world, time, events);
        // WS-2: distribute pump RPM as directional pipe pressures.
        let valve_changed = fluids::runtime::tick_valves(world, kinetics);
        fluids::hose_pulley::tick(world, kinetics);
        fluids::runtime::pressure_tick(world, kinetics, events, valve_changed);
        fluids::spout::tick(world, &mut kinetics.spouts);
        funnel::update_states(world, states, kinetics);
        press::tick(world, kinetics);
        blaze_burner::tick(world, &mut kinetics.blaze_burners);
        mixer::tick(world, time, states, kinetics);
        bearing::tick(world, &states.contraption_blocks, kinetics, &mut level);
        piston::tick(world, &states.contraption_blocks, kinetics);
        gantry::tick(world, &states.contraption_blocks, kinetics);
        pulley::tick(world, states, kinetics);
        for (pos, millstone) in kinetics.dropped_millstones.drain(..) {
            millstone.drop_contents(world, pos);
        }
        millstone::tick(world, kinetics);
        belt_movement::tick(world, kinetics, states);
        belt_inventory::tick(world, kinetics);
        fluids::item_drain::tick(world, kinetics);
        chute::tick(world, states, kinetics);
        funnel::tick(world, kinetics);
        funnel::pull_from_depots(world, kinetics);
        depot::tick(world, kinetics);
        fan::tick(world, kinetics);
        crusher::tick(world, states, kinetics);
        flush(world, kinetics);
    });
}

fn content_replaced(world: World<'_>, event: &ContentEvent) {
    fluids::tank_connectivity::replaced(world, event);
    basin::replaced(world, event);
}

fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    if context.item == blaze_burner::EMPTY {
        return blaze_burner::capture(context);
    }
    if context.item == ANDESITE_ALLOY {
        return manual_application::use_on_block(context);
    }
    if context.item == glue::ID {
        return glue::use_on_block(context);
    }
    // WS-3: TrackBlockItem's item-on-rail selection and straight connections.
    if context.item == "create:track" {
        return trains::use_on_block(context);
    }
    if context.item == chute::ID {
        chute::use_on_block(context)
    } else {
        belt::use_on_block(context)
    }
}

/// `useItemOn` for the mod's blocks.
fn use_block(context: &mut UseBlock<'_>) -> BlockUseResult {
    if !context.with_item {
        return BlockUseResult::Pass;
    }
    if let Some(result) = fluids::runtime::use_item_on(context) {
        return result;
    }
    if blaze_burner::is_burner(context.state) {
        return with_world(context.world, |kinetics| {
            blaze_burner::use_item(&mut kinetics.blaze_burners, context)
                .unwrap_or(BlockUseResult::TryEmptyHand)
        });
    }
    let Some(states) = states() else {
        return BlockUseResult::Pass;
    };
    if let Some(entry) = states
        .by_id
        .get(&context.state)
        .filter(|entry| seat::is_seat(entry.block))
    {
        return seat::use_item_on(context, &entry.selector);
    }
    if states
        .by_id
        .get(&context.state)
        .is_some_and(|state| chassis::is_chassis(state.block))
    {
        return chassis::use_item_on(context);
    }
    if states.chutes.contains_key(&context.state) {
        return with_world(context.world, |kinetics| {
            chute::use_item_on(context.world, &mut kinetics.chutes, context)
        });
    }
    if states.funnels.contains_key(&context.state) {
        return with_world(context.world, |kinetics| {
            funnel::use_item_on(context.world, states, kinetics, context)
        });
    }
    if depot::is_depot(states, context.state) {
        return with_world(context.world, |kinetics| {
            depot::use_item_on(&mut kinetics.depots, context)
        });
    }
    if basin::is_basin(states, context.state) {
        return basin::use_item_on(context);
    }
    match states.kind(context.state) {
        Some(Kind::Belt(_)) => with_world(context.world, |kinetics| {
            belt_inventory::use_item_on(kinetics, context)
        }),
        Some(Kind::Millstone) => with_world(context.world, |kinetics| {
            millstone::use_item_on(kinetics, context)
        }),
        Some(Kind::Bearing(_) | Kind::Windmill(_) | Kind::Clockwork(_)) => {
            with_world(context.world, |kinetics| {
                bearing::use_item_on(kinetics, &states.contraption_blocks, context)
            })
        }
        Some(Kind::Piston(block)) => with_world(context.world, |kinetics| {
            piston::use_item_on(kinetics, block, context)
        }),
        Some(Kind::GantryCarriage(_)) => with_world(context.world, |kinetics| {
            gantry::use_item_on(kinetics, context)
        }),
        Some(Kind::Pulley(_) | Kind::Elevator(_)) => with_world(context.world, |kinetics| {
            pulley::use_item_on(kinetics, context)
        }),
        _ => BlockUseResult::TryEmptyHand,
    }
}

fn entity_inside(world: World<'_>, pos: Pos, state: u32, entity: Entity<'_>) {
    if fluids::runtime::is_item_drain(state) {
        with_world(world, |kinetics| {
            if let Some(drain) = kinetics.item_drains.get_mut(&pos) {
                fluids::item_drain::on_landed(pos, drain, entity);
            }
        });
        return;
    }
    let Some(states) = states() else { return };
    if basin::is_basin(states, state) {
        basin::on_landed(world, pos, entity);
        return;
    }
    if states
        .by_id
        .get(&state)
        .is_some_and(|entry| seat::is_seat(entry.block))
    {
        seat::on_landed(world, pos, entity);
        return;
    }
    if states
        .crusher_controllers
        .get(&state)
        .is_some_and(|(_, valid)| *valid)
    {
        with_world(world, |kinetics| {
            crusher::entity_inside(kinetics, pos, entity)
        });
        return;
    }
    if depot::is_depot(states, state) {
        with_world(world, |kinetics| {
            if let Some(depot) = kinetics.depots.get_mut(&pos) {
                depot::on_landed(world, pos, depot, entity);
            }
        });
        return;
    }
    if let Some(&state) = states.funnels.get(&state) {
        with_world(world, |kinetics| {
            funnel::entity_inside(world, kinetics, pos, state, entity)
        });
        return;
    }
    match states.kind(state) {
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
        let data = save(pos, &be);
        if matches!(be.kind, Kind::HosePulley(_)) {
            fluids::hose_pulley::save_kinetics(world, pos, &data);
        } else if let Ok(bytes) = fastnbt::to_bytes(&data) {
            let _ = world.set_entity(pos, &bytes);
        }
        let _ = world.rotate(pos, be.kind.axis(), be.get_speed());
    }
}

moddedmc_api::export_mod!(Create);
