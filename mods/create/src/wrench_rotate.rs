//! Create's `IWrenchable.onWrenched`: a Wrench use rotates the clicked block around the clicked
//! face's axis.
//!
//! Blocks with their own Wrench handling elsewhere in the mod (chutes, funnels, tunnels, tanks,
//! observers, switches, crafters, chain drives, ladders, whistles, links) never reach this path or
//! are left out of `rule`, as are blocks whose override needs machinery the port lacks yet.

use moddedmc_api::{
    content::{BlockUpdate, Content, UPDATE_ALL, UPDATE_FROM_NEIGHBOR_SHAPES},
    effects::SOUND_BLOCKS,
    interaction::{UseOnBlock, UseResult},
};

use crate::{
    CONTENT, EFFECTS, StateIds, belt_tunnel::read, directed_detector::clockwise,
    kinetics::face_axis,
};

const DIRECTIONS: [&str; 6] = ["down", "up", "north", "south", "west", "east"];
const AXES: [&str; 3] = ["x", "y", "z"];
const Y: u8 = 1;

/// The `onWrenched` override a block uses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rule {
    /// `IWrenchable.getRotatedBlockState`.
    Default,
    /// EncasedFanBlock: its own block update instead of `updateFromNeighbourShapes`.
    Fan,
    /// WrenchableDirectionalBlock.
    Directional,
    /// PumpBlock flips its facing.
    Opposite,
    /// SawBlock also flips the blade when it turns between floor and ceiling.
    Saw,
    /// MechanicalPistonBlock only turns while retracted.
    Piston,
    /// BearingBlock and ClockworkBearingBlock disassemble after turning.
    Bearing,
    /// BeltBlock removes a pulley's shaft.
    Belt,
    /// DeployerBlock switches mode from its front half, otherwise turns.
    Deployer,
    Fail,
    Pass,
}

/// Blocks that use `IWrenchable`'s default rotation unchanged. Shafts and cogwheels only add
/// bracket removal, which the port has no brackets for yet.
const DEFAULT: [&str; 73] = [
    "create:andesite_encased_shaft",
    "create:andesite_scaffolding",
    "create:andesite_table_cloth",
    "create:blaze_burner",
    "create:brass_encased_shaft",
    "create:brass_scaffolding",
    "create:brass_table_cloth",
    "create:chain_conveyor",
    "create:clipboard",
    "create:clutch",
    "create:cogwheel",
    "create:contraption_controls",
    "create:controls",
    "create:copper_backtank",
    "create:copper_scaffolding",
    "create:copper_table_cloth",
    "create:copper_valve_handle",
    "create:creative_motor",
    "create:crushing_wheel",
    "create:cuckoo_clock",
    "create:depot",
    "create:display_board",
    "create:elevator_pulley",
    "create:factory_gauge",
    "create:fluid_valve",
    "create:flywheel",
    "create:framed_glass_trapdoor",
    "create:gantry_carriage",
    "create:gearbox",
    "create:gearshift",
    "create:hand_crank",
    "create:hose_pulley",
    "create:item_drain",
    "create:item_hatch",
    "create:large_cogwheel",
    "create:linear_chassis",
    "create:lit_blaze_burner",
    "create:mechanical_arm",
    "create:mechanical_drill",
    "create:mechanical_mixer",
    "create:mechanical_press",
    "create:millstone",
    "create:mysterious_cuckoo_clock",
    "create:netherite_backtank",
    "create:nixie_tube",
    "create:package_frogport",
    "create:placard",
    "create:powered_latch",
    "create:powered_shaft",
    "create:powered_toggle_latch",
    "create:pulse_extender",
    "create:pulse_repeater",
    "create:pulse_timer",
    "create:radial_chassis",
    "create:redstone_requester",
    "create:rope_pulley",
    "create:rotation_speed_controller",
    "create:secondary_linear_chassis",
    "create:shaft",
    "create:smart_chute",
    "create:smart_fluid_pipe",
    "create:speedometer",
    "create:spout",
    "create:steam_engine",
    "create:stock_link",
    "create:stock_ticker",
    "create:stressometer",
    "create:track_observer",
    "create:track_station",
    "create:train_trapdoor",
    "create:turntable",
    "create:water_wheel",
    "create:weighted_ejector",
];

/// WrenchableDirectionalBlock subclasses without an `onWrenched` override.
const DIRECTIONAL: [&str; 15] = [
    "create:copycat_bars",
    "create:creative_crate",
    "create:desk_bell",
    "create:display_link",
    "create:mechanical_piston_head",
    "create:metal_bracket",
    "create:packager",
    "create:piston_extension_pole",
    "create:portable_fluid_interface",
    "create:portable_storage_interface",
    "create:repackager",
    "create:sail_frame",
    "create:sticker",
    "create:white_sail",
    "create:wooden_bracket",
];

/// Dyed families registered in a `DyeColor` loop, with their rule.
const DYED: [(&str, Rule); 5] = [
    ("_valve_handle", Rule::Default),
    ("_sail", Rule::Directional),
    ("_postbox", Rule::Default),
    ("_table_cloth", Rule::Default),
    ("_nixie_tube", Rule::Default),
];

fn rule(block: &str) -> Option<Rule> {
    let rule = match block {
        "create:encased_fan" => Rule::Fan,
        "create:mechanical_pump" => Rule::Opposite,
        "create:mechanical_saw" => Rule::Saw,
        "create:mechanical_piston" | "create:sticky_mechanical_piston" => Rule::Piston,
        "create:mechanical_bearing" | "create:windmill_bearing" | "create:clockwork_bearing" => {
            Rule::Bearing
        }
        "create:belt" => Rule::Belt,
        "create:deployer" => Rule::Deployer,
        "create:andesite_casing"
        | "create:brass_casing"
        | "create:copper_casing"
        | "create:shadow_steel_casing"
        | "create:refined_radiance_casing"
        | "create:railway_casing"
        | "create:mechanical_harvester"
        | "create:mechanical_plough"
        | "create:mechanical_roller"
        | "create:nozzle" => Rule::Fail,
        "create:large_water_wheel" | "create:water_wheel_structure" | "create:metal_girder" => {
            Rule::Pass
        }
        _ if DEFAULT.contains(&block) => Rule::Default,
        _ if DIRECTIONAL.contains(&block) => Rule::Directional,
        _ => {
            let name = block.strip_prefix("create:")?;
            return DYED.iter().find_map(|&(suffix, rule)| {
                name.strip_suffix(suffix)
                    .filter(|dye| crate::wrenchable::DYES.contains(dye))
                    .map(|_| rule)
            });
        }
    };
    Some(rule)
}

/// A described block state with editable properties.
struct Described {
    block: String,
    properties: Vec<(String, String)>,
}

impl Described {
    fn parse(description: &str) -> Option<Self> {
        let (block, properties) = match description.split_once('[') {
            Some((block, rest)) => (block, rest.strip_suffix(']')?),
            None => (description, ""),
        };
        let properties = properties
            .split(',')
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.split_once('=')
                    .map(|(key, value)| (key.to_owned(), value.to_owned()))
            })
            .collect::<Option<_>>()?;
        Some(Self {
            block: block.to_owned(),
            properties,
        })
    }

    fn get(&self, key: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }

    fn set(&mut self, key: &str, value: &str) {
        if let Some((_, slot)) = self.properties.iter_mut().find(|(name, _)| name == key) {
            value.clone_into(slot);
        }
    }

    fn cycle_bool(&mut self, key: &str) {
        let next = if self.get(key) == Some("true") {
            "false"
        } else {
            "true"
        };
        self.set(key, next);
    }

    fn direction(&self, key: &str) -> Option<u8> {
        let value = self.get(key)?;
        DIRECTIONS
            .iter()
            .position(|&name| name == value)
            .map(|index| index as u8)
    }

    fn set_direction(&mut self, key: &str, direction: u8) {
        self.set(key, DIRECTIONS[usize::from(direction)]);
    }

    fn axis(&self, key: &str) -> Option<u8> {
        let value = self.get(key)?;
        AXES.iter()
            .position(|&name| name == value)
            .map(|index| index as u8)
    }

    fn selector(&self) -> String {
        if self.properties.is_empty() {
            return self.block.clone();
        }
        let properties = self
            .properties
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>()
            .join(",");
        format!("{}[{properties}]", self.block)
    }

    fn state(&self, content: Content) -> Option<u32> {
        content.state(&self.selector())
    }

    /// Whether `key` also accepts `value`, telling `BlockStateProperties.AXIS` and `FACING` apart
    /// from their horizontal counterparts, which share the property name.
    fn accepts(&self, content: Content, key: &str, value: &str) -> bool {
        let mut probe = Self {
            block: self.block.clone(),
            properties: self.properties.clone(),
        };
        probe.set(key, value);
        probe.state(content).is_some()
    }
}

/// `VoxelShaper.axisAsFace(axis).getClockWise(around).getAxis()`.
fn rotate_axis(axis: u8, around: u8) -> u8 {
    const POSITIVE_FACE: [u8; 3] = [5, 1, 3];
    face_axis(clockwise(POSITIVE_FACE[usize::from(axis)], around))
}

/// `IWrenchable.getRotatedBlockState`.
fn default_rotation(content: Content, state: &mut Described, face: u8) {
    let around = face_axis(face);
    let axis = state.axis("axis");
    let full_axis = axis.is_some() && state.accepts(content, "axis", "y");
    let facing = state.direction("facing");
    let full_facing = facing.is_some() && state.accepts(content, "facing", "up");
    if around == Y {
        if let Some(axis) = axis
            && !full_axis
        {
            state.set("axis", AXES[usize::from(rotate_axis(axis, around))]);
            return;
        }
        if let Some(facing) = facing
            && !full_facing
        {
            state.set_direction("facing", clockwise(facing, around));
            return;
        }
    }
    if let Some(axis) = axis
        && full_axis
    {
        state.set("axis", AXES[usize::from(rotate_axis(axis, around))]);
        return;
    }
    let Some(mut facing) = facing.filter(|_| full_facing) else {
        return;
    };
    let along_first = state.get("axis_along_first").is_some();
    if face_axis(facing) == around {
        if along_first {
            state.cycle_bool("axis_along_first");
        }
        return;
    }
    loop {
        facing = clockwise(facing, around);
        if around == Y && along_first {
            state.cycle_bool("axis_along_first");
        }
        if face_axis(facing) != around {
            break;
        }
    }
    state.set_direction("facing", facing);
}

/// WrenchableDirectionalBlock.getRotatedBlockState.
fn directional_rotation(state: &mut Described, face: u8) {
    let around = face_axis(face);
    if let Some(facing) = state.direction("facing")
        && face_axis(facing) != around
    {
        state.set_direction("facing", clockwise(facing, around));
    }
}

pub(crate) fn use_on_block(context: &UseOnBlock<'_>, states: &StateIds) -> Option<UseResult> {
    if context.sneaking || !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let content = *CONTENT.get()?;
    let world = context.world;
    let pos = context.position;
    let original = read(world, pos);
    let mut state = Described::parse(&content.describe_state(original)?)?;
    let face = context.face;
    let rule = rule(&state.block)?;
    match rule {
        Rule::Fail => return Some(UseResult::Fail),
        Rule::Pass => return Some(UseResult::Pass),
        Rule::Piston if state.get("state") != Some("retracted") => return Some(UseResult::Pass),
        Rule::Belt => return Some(wrench_belt(context, content, &mut state)),
        Rule::Deployer if deployer_front(&state, context.hit) => {
            // Punch mode is not ported yet, so the mode switch does nothing.
            return Some(UseResult::Success);
        }
        _ => {}
    }
    let flips_blade = rule == Rule::Saw && state.get("axis_along_first") == Some("false");
    match rule {
        Rule::Directional => directional_rotation(&mut state, face),
        Rule::Opposite => {
            if let Some(facing) = state.direction("facing") {
                state.set_direction("facing", facing ^ 1);
            }
        }
        _ => default_rotation(content, &mut state, face),
    }
    // SawBlock.getRotatedBlockState flips the blade between floor and ceiling.
    if flips_blade && face < 2 && state.direction("facing").is_some_and(|facing| facing < 2) {
        state.cycle_bool("flipped");
    }
    let rotated = state.state(content)?;
    if world.can_survive(pos, rotated) != Some(true) {
        return Some(UseResult::Pass);
    }
    if rotated != original {
        write(context, rotated, rule != Rule::Fan);
    }
    if rule == Rule::Bearing {
        let axis = state.direction("facing").map_or(Y, face_axis);
        crate::with_world(world, |kinetics| {
            if let Some(be) = kinetics.get_mut(pos)
                && let Some(bearing) = &mut be.bearing
            {
                be.dirty |= bearing.disassemble(world, &states.contraption_blocks, axis);
            }
        });
    }
    if read(world, pos) != original {
        play_rotate_sound(context);
    }
    Some(UseResult::Success)
}

/// `KineticBlockEntity.switchToBlockState` after `updateAfterWrenched`.
fn write(context: &UseOnBlock<'_>, state: u32, neighbor_shapes: bool) -> bool {
    let update = BlockUpdate {
        position: context.position,
        state,
        flags: if neighbor_shapes {
            UPDATE_ALL | UPDATE_FROM_NEIGHBOR_SHAPES
        } else {
            UPDATE_ALL
        },
    };
    if neighbor_shapes {
        context.world.write_with_neighbor_shapes(&[update]) == 1
    } else {
        context.world.write(&[update]) == 1
    }
}

/// BeltBlock.onWrenched: a pulley gives its shaft back. Encased belts lose their casing in
/// `belt_casing` before the item use runs.
fn wrench_belt(context: &UseOnBlock<'_>, content: Content, state: &mut Described) -> UseResult {
    if state.get("part") != Some("pulley") {
        return UseResult::Pass;
    }
    state.set("part", "middle");
    let Some(next) = state.state(content) else {
        return UseResult::Fail;
    };
    if !write(context, next, false) {
        return UseResult::Fail;
    }
    if !context.creative
        && let Ok(bytes) = fastnbt::to_bytes(&fastnbt::nbt!({"id": "create:shaft", "count": 1i32}))
    {
        context.give(&bytes);
    }
    UseResult::Success
}

/// DeployerBlock.onWrenched's test for a click on the front half.
fn deployer_front(state: &Described, hit: [f32; 3]) -> bool {
    let Some(facing) = state.direction("facing") else {
        return false;
    };
    let axis = usize::from(face_axis(facing));
    let positive = facing % 2 == 1;
    let normal = if positive { 1.0 } else { -1.0 };
    // (click - (center - normal / 2)) * normal along the facing axis.
    let depth = (hit[axis] - (0.5 - normal * 0.5)) * normal;
    depth > 0.75
}

/// `IWrenchable.playRotateSound`: WRENCH_ROTATE plays the wooden trapdoor close at a quarter
/// volume and 1.25 times a random pitch from 0.5 to 1.5.
fn play_rotate_sound(context: &UseOnBlock<'_>) {
    let Some(effects) = EFFECTS.get() else {
        return;
    };
    let pos = context.position;
    let coords = [pos.x, pos.y, pos.z];
    effects.play_sound(
        context.world,
        coords.map(|coord| f64::from(coord) + 0.5),
        "minecraft:block.wooden_trapdoor.close",
        SOUND_BLOCKS,
        0.25,
        1.25 * (0.5 + crate::decor::wrench_pitch(coords)),
    );
}
