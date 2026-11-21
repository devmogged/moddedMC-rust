//! Simple Storage Network 1.13.2 base cable and its adjacent cable states.

use std::{collections::HashSet, sync::OnceLock};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    commands::{self, Argument, Commands, Node},
    content::{self, Block, BlockUpdate, Content, ContentEvent, ShapeBox, State, World},
};

const ID: &str = "storagenetwork:kabel";
const DIRECTIONS: [(&str, [i32; 3]); 6] = [
    ("down", [0, -1, 0]),
    ("up", [0, 1, 0]),
    ("north", [0, 0, -1]),
    ("south", [0, 0, 1]),
    ("west", [-1, 0, 0]),
    ("east", [1, 0, 0]),
];
const STATES: usize = 1 << DIRECTIONS.len();

static COMMANDS: OnceLock<Commands> = OnceLock::new();
static CONTENT: OnceLock<Content> = OnceLock::new();
static STATE_IDS: OnceLock<[u32; STATES]> = OnceLock::new();

struct SimpleStorageNetwork;

impl Mod for SimpleStorageNetwork {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = COMMANDS.set(commands::require()?);
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let content = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        let variants: Vec<_> = (0..STATES)
            .map(|mask| {
                let model = if mask == 0 {
                    "storagenetwork:block/kabel".to_owned()
                } else {
                    format!("storagenetwork:block/kabel_{mask}")
                };
                (properties(mask), model, shape(mask))
            })
            .collect();
        let states: Vec<_> = variants
            .iter()
            .map(|(props, model, boxes)| State {
                properties: props,
                physical: "minecraft:stone",
                model,
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: boxes,
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id: ID,
                display_name: "Network Cable",
                entity: "",
                item_model: "storagenetwork:item/kabel",
                states: &states,
            },
            None,
        )?;
        content.on_tick(registry, tick)?;
        let commands = COMMANDS.get().ok_or(RegistrationError::InvalidDefinition)?;
        commands.add(
            registry,
            "storage_network_cable",
            "Inspect the cable at a position",
            2,
            &[
                Node {
                    parent: None,
                    name: "x",
                    argument: Argument::Integer {
                        min: -30_000_000,
                        max: 30_000_000,
                    },
                    tag: None,
                },
                Node {
                    parent: Some(0),
                    name: "y",
                    argument: Argument::Integer {
                        min: -2048,
                        max: 2048,
                    },
                    tag: None,
                },
                Node {
                    parent: Some(1),
                    name: "z",
                    argument: Argument::Integer {
                        min: -30_000_000,
                        max: 30_000_000,
                    },
                    tag: Some(0),
                },
            ],
        )
    }

    fn command(&self, _: u64, command: Command<'_>) -> i32 {
        let Some(commands) = COMMANDS.get() else {
            return 0;
        };
        let (Some(x), Some(y), Some(z)) = (
            commands.integer(&command, "x"),
            commands.integer(&command, "y"),
            commands.integer(&command, "z"),
        ) else {
            return 0;
        };
        let position = Pos { x, y, z };
        let (Some(content), Some(ids)) = (CONTENT.get(), STATE_IDS.get()) else {
            return 0;
        };
        let mut state = [content::UNLOADED_STATE];
        let mask = content.command_world(&command).and_then(|world| {
            world
                .read(&[position], &mut state)
                .then(|| ids.iter().position(|id| *id == state[0]))
                .flatten()
        });
        match mask {
            Some(mask) => command.reply(&format!(
                "Network Cable at {},{},{}: {}",
                position.x,
                position.y,
                position.z,
                properties(mask)
            )),
            None => command.reply("No loaded Network Cable at that position"),
        }
        i32::from(mask.is_some())
    }
}

fn properties(mask: usize) -> String {
    DIRECTIONS
        .iter()
        .enumerate()
        .map(|(bit, (face, _))| {
            let connection = if mask & (1 << bit) == 0 {
                "none"
            } else {
                "cable"
            };
            format!("{face}={connection}")
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn shape(mask: usize) -> Vec<ShapeBox> {
    let mut boxes = vec![ShapeBox {
        min: [6; 3],
        max: [10; 3],
    }];
    for (bit, (_, offset)) in DIRECTIONS.iter().enumerate() {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let mut arm = ShapeBox {
            min: [6; 3],
            max: [10; 3],
        };
        for (axis, direction) in offset.iter().enumerate() {
            if *direction < 0 {
                arm.min[axis] = 0;
                arm.max[axis] = 6;
            } else if *direction > 0 {
                arm.min[axis] = 10;
                arm.max[axis] = 16;
            }
        }
        boxes.push(arm);
    }
    boxes
}

fn neighbors(pos: Pos) -> [Pos; DIRECTIONS.len()] {
    DIRECTIONS.map(|(_, [dx, dy, dz])| Pos {
        x: pos.x + dx,
        y: pos.y + dy,
        z: pos.z + dz,
    })
}

fn tick(world: World<'_>, _: u64, events: &[ContentEvent]) {
    if events.is_empty() {
        return;
    }
    let Some(content) = CONTENT.get() else { return };
    let ids = STATE_IDS.get_or_init(|| {
        std::array::from_fn(|mask| {
            content
                .state(&format!("{ID}[{}]", properties(mask)))
                .unwrap_or(content::UNLOADED_STATE)
        })
    });
    if ids.contains(&content::UNLOADED_STATE) {
        return;
    }
    let mut changed = HashSet::new();
    for event in events {
        let was_cable = ids.contains(&event.previous);
        let is_cable = ids.contains(&event.state);
        if !was_cable && !is_cable {
            continue;
        }
        changed.insert(event.position);
        if was_cable != is_cable || matches!(event.reason, content::LOADED | content::UNLOADED) {
            changed.extend(neighbors(event.position));
        }
    }
    let mut updates = Vec::new();
    for position in changed {
        let mut current = [content::UNLOADED_STATE];
        if !world.read(&[position], &mut current) || !ids.contains(&current[0]) {
            continue;
        }
        let mut adjacent = [content::UNLOADED_STATE; DIRECTIONS.len()];
        if !world.read(&neighbors(position), &mut adjacent) {
            continue;
        }
        let mask = adjacent.iter().enumerate().fold(0, |mask, (bit, state)| {
            mask | (usize::from(ids.contains(state)) << bit)
        });
        if ids[mask] != current[0] {
            updates.push(BlockUpdate {
                position,
                state: ids[mask],
                flags: content::UPDATE_CLIENTS,
            });
        }
    }
    world.write(&updates);
}

moddedmc_api::export_mod!(SimpleStorageNetwork);
