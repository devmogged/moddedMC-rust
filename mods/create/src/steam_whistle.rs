//! Create 6.0.10 Steam Whistle placement, pitch extensions and redstone.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    effects::{SOUND_BLOCKS, SOUND_RECORDS},
    interaction::{BlockUseResult, UseBlock},
    stacks::ItemDisplay,
};

use crate::{
    CONTENT, DIRECTIONS, EFFECTS, StateIds, WRENCH,
    belt_tunnel::read,
    display_model::ModelDisplay,
    fluids::{
        boiler::{Attachments, Controllers},
        runtime::is_tank,
        tank_runtime,
    },
    kinetics::{FACES, Kinetics, offset},
    nearest_looking_direction,
    processing_recipes::item_id,
    steam_engine::spin,
};

pub(crate) const ID: &str = "create:steam_whistle";
const EXTENSION: &str = "create:steam_whistle_extension";
const MAX_EXTENSIONS: i32 = 6;
const OCTAVE: f32 = 12.0;
const SIZES: [&str; 3] = ["small", "medium", "large"];
const FACES_HORIZONTAL: [u8; 4] = [2, 3, 4, 5];
const ROTATIONS: [i16; 4] = [180, 0, 90, 270];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WhistleBlock {
    facing: u8,
    wall: bool,
    powered: bool,
    size: u8,
}

impl WhistleBlock {
    fn variants() -> impl Iterator<Item = Self> {
        FACES_HORIZONTAL.into_iter().flat_map(|facing| {
            [false, true].into_iter().flat_map(move |wall| {
                [false, true].into_iter().flat_map(move |powered| {
                    [1, 0, 2].map(move |size| Self {
                        facing,
                        wall,
                        powered,
                        size,
                    })
                })
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "facing={},powered={},size={},wall={}",
            DIRECTIONS[usize::from(self.facing)],
            self.powered,
            SIZES[usize::from(self.size)],
            self.wall
        )
    }

    fn model(self) -> String {
        let attachment = if self.wall { "wall" } else { "floor" };
        let suffix = if self.powered { "_powered" } else { "" };
        format!(
            "create:block/steam_whistle/block_{}_{attachment}{suffix}",
            SIZES[usize::from(self.size)]
        )
    }

    pub(crate) fn tank(self, pos: Pos) -> Pos {
        let toward_tank = if self.wall { self.facing } else { 0 };
        offset(pos, FACES[usize::from(toward_tank)])
    }

    fn shape(self) -> [ShapeBox; 3] {
        let inset = 4 - i16::from(self.size);
        let mut base = [
            ShapeBox {
                min: [1, 0, 1],
                max: [15, 3, 15],
            },
            ShapeBox {
                min: [5, 0, 5],
                max: [11, 8, 11],
            },
        ];
        if self.wall {
            let turns = [2, 0, 1, 3][usize::from(self.facing - 2)];
            for bounds in &mut base {
                let [x1, y1, z1] = bounds.min;
                let [x2, y2, z2] = bounds.max;
                *bounds = ShapeBox {
                    min: [x1, z1, 16 - y2],
                    max: [x2, z2, 16 - y1],
                };
                for _ in 0..turns {
                    let [x1, y1, z1] = bounds.min;
                    let [x2, y2, z2] = bounds.max;
                    *bounds = ShapeBox {
                        min: [16 - z2, y1, x1],
                        max: [16 - z1, y2, x2],
                    };
                }
            }
        }
        [
            base[0],
            base[1],
            ShapeBox {
                min: [inset, 3, inset],
                max: [16 - inset, 16, 16 - inset],
            },
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExtenderShape {
    Single,
    Double,
    Connected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Extension {
    shape: ExtenderShape,
    size: u8,
}

impl Extension {
    fn variants() -> impl Iterator<Item = Self> {
        [1, 0, 2].into_iter().flat_map(|size| {
            [
                ExtenderShape::Single,
                ExtenderShape::Double,
                ExtenderShape::Connected,
            ]
            .map(move |shape| Self { size, shape })
        })
    }

    fn properties(self) -> String {
        let shape = match self.shape {
            ExtenderShape::Single => "single",
            ExtenderShape::Double => "double",
            ExtenderShape::Connected => "double_connected",
        };
        format!("shape={shape},size={}", SIZES[usize::from(self.size)])
    }

    fn model(self) -> String {
        let shape = if self.shape == ExtenderShape::Single {
            "single"
        } else {
            "double"
        };
        format!(
            "create:block/steam_whistle/extension/{}_{shape}",
            SIZES[usize::from(self.size)]
        )
    }

    fn bounds(self) -> [ShapeBox; 1] {
        let inset = 4 - i16::from(self.size);
        let height = match self.shape {
            ExtenderShape::Single => 10,
            ExtenderShape::Double => 18,
            ExtenderShape::Connected => 16,
        };
        [ShapeBox {
            min: [inset, 0, inset],
            max: [16 - inset, height, 16 - inset],
        }]
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows: Vec<_> = WhistleBlock::variants()
        .map(|block| {
            (
                block.properties(),
                block.model(),
                block.shape(),
                ROTATIONS[usize::from(block.facing - 2)],
            )
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(properties, model, shape, y)| State {
            properties,
            physical: "minecraft:copper_block",
            // Share the empty carrier across whistle bodies and draw each release model.
            model: "",
            rotating_model: model,
            x: 0,
            y: 0,
            orientation: spin(1, -f32::from(*y).to_radians()),
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Steam Whistle",
            entity: ID,
            item_model: "create:item/steam_whistle",
            states: &states,
        },
        Some(crate::placed_state),
    )?;

    let rows: Vec<_> = Extension::variants()
        .map(|block| (block.properties(), block.model(), block.bounds()))
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(properties, model, shape)| State {
            properties,
            physical: "minecraft:copper_block",
            model,
            rotating_model: "",
            x: 0,
            y: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: EXTENSION,
            display_name: "Steam Whistle Extension",
            entity: "",
            item_model: "",
            states: &states,
        },
        None,
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for block in WhistleBlock::variants() {
        let properties = block.properties();
        states.add(content, ID, &properties, None);
        if let Some(state) = states.lookup(&format!("{ID}[{properties}]")) {
            states.steam_whistles.insert(state, block);
        }
    }
    for block in Extension::variants() {
        let properties = block.properties();
        states.add(content, EXTENSION, &properties, None);
        if let Some(state) = states.lookup(&format!("{EXTENSION}[{properties}]")) {
            states.whistle_extensions.insert(state, block);
        }
    }
}

pub(crate) fn place(args: Placement<'_>, states: &StateIds) -> Option<u32> {
    let wall = args.face >= 2;
    let block = WhistleBlock {
        facing: if wall {
            // Pumpkin supplies the face pointing toward the attached block.
            args.face
        } else {
            nearest_looking_direction(args.yaw, 0.0)
        },
        wall,
        powered: args.world.has_neighbor_signal(args.position),
        size: 1,
    };
    is_tank(read(args.world, block.tank(args.position)))
        .then(|| states.lookup(&format!("{ID}[{}]", block.properties())))?
}

fn update(world: World<'_>, pos: Pos, id: &str, properties: &str, flags: u8) -> bool {
    let Some(state) = CONTENT
        .get()
        .and_then(|content| content.state(&format!("{id}[{properties}]")))
    else {
        return false;
    };
    world.write(&[BlockUpdate {
        position: pos,
        state,
        flags,
    }]) == 1
}

fn root(world: World<'_>, states: &StateIds, mut pos: Pos) -> Option<(Pos, WhistleBlock)> {
    loop {
        let state = read(world, pos);
        if let Some(&block) = states.steam_whistles.get(&state) {
            return Some((pos, block));
        }
        if !states.whistle_extensions.contains_key(&state) {
            return None;
        }
        pos = offset(pos, [0, -1, 0]);
    }
}

fn grow(world: World<'_>, states: &StateIds, pos: Pos, size: u8) {
    for height in 1..=MAX_EXTENSIONS {
        let at = offset(pos, [0, height, 0]);
        let state = read(world, at);
        let (shape, pitch) = match states.whistle_extensions.get(&state) {
            Some(Extension {
                shape: ExtenderShape::Single,
                ..
            }) => (ExtenderShape::Double, height * 2),
            Some(_) => continue,
            None if states.replaceable.contains(&state) => (ExtenderShape::Single, height * 2 - 1),
            None => return,
        };
        if update(
            world,
            at,
            EXTENSION,
            &Extension { shape, size }.properties(),
            content::UPDATE_ALL,
        ) && let Some(effects) = EFFECTS.get()
        {
            let position = [at.x, at.y, at.z].map(|axis| f64::from(axis) + 0.5);
            let frequency = 2.0f32.powf(-(pitch as f32) / OCTAVE);
            effects.play_sound(
                world,
                position,
                "minecraft:block.note_block.xylophone",
                SOUND_BLOCKS,
                0.25,
                frequency,
            );
            effects.play_sound(
                world,
                position,
                "minecraft:block.copper.hit",
                SOUND_BLOCKS,
                1.0,
                frequency,
            );
        }
        return;
    }
}

pub(crate) fn use_block(context: &mut UseBlock<'_>, states: &StateIds) -> Option<BlockUseResult> {
    if !states.steam_whistles.contains_key(&context.state)
        && !states.whistle_extensions.contains_key(&context.state)
    {
        return None;
    }
    let (pos, mut block) = root(context.world, states, context.position)?;
    let item = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok());
    let held = item.as_ref().and_then(item_id);
    match held {
        Some(ID) => {
            grow(context.world, states, pos, block.size);
            Some(BlockUseResult::Success)
        }
        Some(WRENCH)
            if context.player.is_some_and(|player| player.may_build()) && !context.sneaking =>
        {
            block.size = (block.size + 1) % SIZES.len() as u8;
            let changed = update(
                context.world,
                pos,
                ID,
                &block.properties(),
                content::UPDATE_ALL,
            );
            Some(if changed {
                BlockUseResult::Success
            } else {
                BlockUseResult::Consume
            })
        }
        Some(WRENCH)
            if context.player.is_some_and(|player| player.may_build()) && context.sneaking =>
        {
            let clicked = context.position;
            if let Some(extension) = states.whistle_extensions.get(&context.state)
                && extension.shape != ExtenderShape::Single
                && context.hit[1] >= 0.5
            {
                let extension = Extension {
                    shape: ExtenderShape::Single,
                    ..*extension
                };
                update(
                    context.world,
                    clicked,
                    EXTENSION,
                    &extension.properties(),
                    content::UPDATE_ALL,
                );
            } else {
                context.world.destroy(clicked, !context.creative);
            }
            Some(BlockUseResult::Success)
        }
        _ => Some(BlockUseResult::TryEmptyHand),
    }
}

fn update_extension(world: World<'_>, states: &StateIds, pos: Pos, mut block: Extension) {
    let below = read(world, offset(pos, [0, -1, 0]));
    if below == content::UNLOADED_STATE {
        return;
    }
    let size = if let Some(root) = states.steam_whistles.get(&below) {
        root.size
    } else if let Some(previous) = states
        .whistle_extensions
        .get(&below)
        .filter(|previous| previous.shape != ExtenderShape::Single)
    {
        previous.size
    } else {
        world.destroy(pos, false);
        return;
    };
    let above = read(world, offset(pos, [0, 1, 0]));
    let shape = if states.whistle_extensions.contains_key(&above) {
        ExtenderShape::Connected
    } else if block.shape == ExtenderShape::Connected {
        ExtenderShape::Double
    } else {
        block.shape
    };
    if block.shape != shape || block.size != size {
        block.shape = shape;
        block.size = size;
        update(
            world,
            pos,
            EXTENSION,
            &block.properties(),
            content::UPDATE_ALL,
        );
    }
}

pub(crate) fn event(
    world: World<'_>,
    states: &StateIds,
    actors: &mut std::collections::HashMap<Pos, Whistle>,
    event: &ContentEvent,
) {
    let pos = event.position;
    if event.reason == content::UNLOADED {
        if let Some(mut whistle) = actors.remove(&pos) {
            whistle.clear(world);
        }
        return;
    }
    if let Some(&block) = states.whistle_extensions.get(&read(world, pos)) {
        update_extension(world, states, pos, block);
    }
    if let Some(&block) = states.steam_whistles.get(&read(world, pos)) {
        actors.entry(pos).or_default();
        let support = read(world, block.tank(pos));
        if support != content::UNLOADED_STATE && !is_tank(support) {
            world.destroy(pos, true);
        }
    } else if let Some(mut whistle) = actors.remove(&pos) {
        whistle.clear(world);
    }
}

#[derive(Default)]
pub(crate) struct Whistle {
    pitch: Option<i32>,
    mouth: ModelDisplay,
    rim: ModelDisplay,
    last_pose: Option<(u8, u8, i32)>,
    animation: f32,
    last_sound: Option<(u8, i32)>,
    next_sound: u64,
}

pub(crate) fn collect(
    world: World<'_>,
    states: &StateIds,
    kinetics: &Kinetics,
    controllers: &mut Controllers,
) -> Vec<(Pos, WhistleBlock, Option<Pos>)> {
    kinetics
        .steam_whistles
        .keys()
        .filter_map(|&pos| {
            let &block = states.steam_whistles.get(&read(world, pos))?;
            let owner = tank_runtime::canonical(world, block.tank(pos))
                .filter(|(owner, _)| !kinetics.fluid_tanks.pending(*owner))
                .map(|(owner, tank)| {
                    controllers
                        .entry(owner)
                        .or_insert((tank, Attachments::default()))
                        .1
                        .whistles
                        .push(pos);
                    owner
                });
            Some((pos, block, owner))
        })
        .collect()
}

pub(crate) fn tick(
    world: World<'_>,
    states: &StateIds,
    kinetics: &mut Kinetics,
    whistles: Vec<(Pos, WhistleBlock, Option<Pos>)>,
    time: u64,
) {
    for (pos, mut block, owner) in whistles {
        let powered = world.has_neighbor_signal(pos);
        if block.powered != powered {
            block.powered = powered;
            update(world, pos, ID, &block.properties(), content::UPDATE_CLIENTS);
        }
        let Some(whistle) = kinetics.steam_whistles.get_mut(&pos) else {
            continue;
        };
        let pitch = pitch(world, states, pos);
        if whistle.pitch != Some(pitch) {
            let nbt = fastnbt::nbt!({ "Pitch": pitch });
            if fastnbt::to_bytes(&nbt).is_ok_and(|bytes| world.set_entity(pos, &bytes)) {
                whistle.pitch = Some(pitch);
            }
        }
        let sounding = powered && owner.is_some_and(|owner| kinetics.boilers.heated(owner));
        whistle.tick_display(world, pos, block, pitch, time, sounding);
    }
}

fn pitch(world: World<'_>, states: &StateIds, pos: Pos) -> i32 {
    let mut pitch = 0;
    for height in 1..=13 {
        let state = read(world, offset(pos, [0, height, 0]));
        let Some(extender) = states.whistle_extensions.get(&state) else {
            break;
        };
        if extender.shape == ExtenderShape::Single {
            pitch += 1;
            break;
        }
        pitch += 2;
    }
    pitch
}

const WHISTLE_SOUNDS: [&str; 3] = [
    "create:whistle_high",
    "create:whistle",
    "create:whistle_low",
];
const SAMPLE_DURATION_TICKS: f32 = 60.0;
const MOUTHS: [&str; 3] = [
    "create:moddedmc/parts/steam_whistle/small_mouth",
    "create:moddedmc/parts/steam_whistle/medium_mouth",
    "create:moddedmc/parts/steam_whistle/large_mouth",
];
const RIMS: [&str; 3] = [
    "create:moddedmc/parts/steam_whistle/small_top_rim",
    "create:moddedmc/parts/steam_whistle/medium_top_rim",
    "create:moddedmc/parts/steam_whistle/large_top_rim",
];

impl Whistle {
    fn clear(&mut self, world: World<'_>) {
        self.mouth.clear(world);
        self.rim.clear(world);
    }

    fn tick_display(
        &mut self,
        world: World<'_>,
        pos: Pos,
        block: WhistleBlock,
        pitch: i32,
        time: u64,
        sounding: bool,
    ) {
        let sound = sounding.then_some((block.size, pitch));
        let new_sound = self.last_sound != sound;
        if new_sound {
            // The client stops the old entity-bound sound as soon as its emitter is removed.
            self.mouth.clear(world);
            self.last_pose = None;
            self.last_sound = sound;
            self.next_sound = time;
        }
        let old_mouth = self.mouth.entity_id();
        self.display(world, pos, block, pitch, time, sounding);
        if self.mouth.entity_id() != old_mouth {
            self.next_sound = time;
        }

        let (Some((size, pitch)), Some(effects)) = (sound, EFFECTS.get()) else {
            return;
        };
        let frequency = 2.0f32.powf(-(pitch as f32) / OCTAVE);
        if new_sound {
            effects.play_sound(
                world,
                [pos.x, pos.y, pos.z].map(|axis| f64::from(axis) + 0.5),
                "create:chiff",
                SOUND_BLOCKS,
                0.175,
                if size == 0 {
                    frequency + 0.75
                } else {
                    frequency
                },
            );
        }
        if time >= self.next_sound
            && self.mouth.entity_id().is_some_and(|entity| {
                effects.play_entity_sound(
                    world,
                    entity,
                    WHISTLE_SOUNDS[usize::from(size)],
                    SOUND_RECORDS,
                    1.0,
                    frequency,
                )
            })
        {
            self.next_sound = time + (SAMPLE_DURATION_TICKS / frequency).round() as u64;
        }
    }

    fn display(
        &mut self,
        world: World<'_>,
        pos: Pos,
        block: WhistleBlock,
        pitch: i32,
        time: u64,
        sounding: bool,
    ) {
        let previous = self.animation;
        self.animation = if sounding {
            previous + (1.0 - previous) * 0.5
        } else {
            (previous - 0.4).max(0.0)
        };
        let size = usize::from(block.size);
        let pose = (block.facing, block.size, pitch);
        if !sounding
            && previous == self.animation
            && self.last_pose == Some(pose)
            && !time.is_multiple_of(20)
        {
            return;
        }
        let origin = [pos.x, pos.y, pos.z].map(|axis| f64::from(axis) + 0.5);
        let wave = if sounding && self.animation > 0.5 {
            let phase = (time % 8) as f32 * std::f32::consts::TAU / 8.0;
            (phase * f32::from(4 - block.size)).sin() / 16.0
        } else {
            0.0
        };
        let degrees = [180.0f32, 0.0, -90.0, 90.0][usize::from(block.facing - 2)];
        self.mouth.show(
            world,
            MOUTHS[size],
            &ItemDisplay {
                position: [
                    origin[0],
                    origin[1] + f64::from(self.animation - wave) / 4.0,
                    origin[2],
                ],
                rotation: spin(1, degrees.to_radians()),
                scale: 1.0,
                context: 0,
                interpolation: 1,
            },
        );
        if self.last_pose != Some(pose) || time.is_multiple_of(20) {
            if pitch > 0 && pitch % 2 == 0 {
                self.rim.show(
                    world,
                    RIMS[size],
                    &ItemDisplay {
                        position: [origin[0], origin[1] + f64::from(pitch / 2), origin[2]],
                        rotation: [0.0, 0.0, 0.0, 1.0],
                        scale: 1.0,
                        context: 0,
                        interpolation: 0,
                    },
                );
            } else {
                self.rim.clear(world);
            }
        }
        self.last_pose = Some(pose);
    }
}
