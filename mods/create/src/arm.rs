//! Mechanical Arm placement and item transfer from Create 6.0.10.

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, Placement, ShapeBox, State, World},
    entities::Entity,
    interaction::{BlockUseResult, Stack, UseBlock, UseOnBlock, UseResult},
};
use serde::{Deserialize, Serialize};

use crate::{
    STACKS, StateIds, WRENCH,
    belt_inventory::{self, TransportedItemStack, count, with_count},
    belt_movement::can_transport_objects,
    filtering::{self, Extraction},
    kinetics::{FACES, Kind, Kinetics, offset},
    processing_recipes::item_id,
};

pub(crate) const ID: &str = "create:mechanical_arm";
const RANGE: i32 = 5;
const UP: u8 = 1;
const LAZY_TICKS: u64 = 10;
const SETTINGS: &str = "CreateArm";

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let shape = [
        ShapeBox {
            min: [2, 0, 2],
            max: [14, 10, 14],
        },
        ShapeBox {
            min: [3, 0, 3],
            max: [13, 14, 13],
        },
        ShapeBox {
            min: [0, 0, 0],
            max: [16, 6, 16],
        },
    ];
    let ceiling_shape = shape.map(|bounds| ShapeBox {
        min: [bounds.min[0], 16 - bounds.max[1], bounds.min[2]],
        max: [bounds.max[0], 16 - bounds.min[1], bounds.max[2]],
    });
    let states = [false, true].map(|ceiling| State {
        properties: properties(ceiling),
        physical: "minecraft:copper_block",
        model: "create:block/mechanical_arm/block",
        rotating_model: "create:block/mechanical_arm/cog",
        x: if ceiling { 180 } else { 0 },
        y: 0,
        orientation: if ceiling {
            [1., 0., 0., 0.]
        } else {
            [0., 0., 0., 1.]
        },
        shape: if ceiling { &ceiling_shape } else { &shape },
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Mechanical Arm",
            entity: ID,
            item_model: "create:item/mechanical_arm",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

const fn properties(ceiling: bool) -> &'static str {
    if ceiling {
        "ceiling=true"
    } else {
        "ceiling=false"
    }
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for ceiling in [false, true] {
        states.add(content, ID, properties(ceiling), Some(Kind::Arm(ceiling)));
    }
}

pub(crate) fn placed(args: Placement<'_>, states: &StateIds) -> Option<u32> {
    states.lookup(&format!("{ID}[{}]", properties(args.face == UP)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PointType {
    Basin,
    Belt,
    BlazeBurner,
    Chute,
    Crafter,
    CrushingWheels,
    Deployer,
    Depot,
    Funnel,
    Millstone,
}

impl PointType {
    fn at(world: World<'_>, states: &StateIds, pos: Pos) -> Option<Self> {
        let state = crate::belt_tunnel::read(world, pos);
        if crate::blaze_burner::is_burner(state) {
            return Some(Self::BlazeBurner);
        }
        if let Some(&funnel) = states.funnels.get(&state) {
            return funnel.is_arm_point().then_some(Self::Funnel);
        }
        if states.chutes.contains_key(&state) {
            return Some(Self::Chute);
        }
        if states.crusher_controllers.contains_key(&state) {
            return Some(Self::CrushingWheels);
        }
        match states.kind(state) {
            Some(Kind::Belt(belt))
                if can_transport_objects(belt)
                    && !states
                        .tunnels
                        .contains_key(&crate::belt_tunnel::read(world, offset(pos, [0, 1, 0]))) =>
            {
                Some(Self::Belt)
            }
            Some(Kind::Crafter(_)) => Some(Self::Crafter),
            Some(Kind::Deployer { .. }) => Some(Self::Deployer),
            Some(Kind::Millstone) => Some(Self::Millstone),
            _ => match states.by_id.get(&state)?.block {
                crate::basin::ID => Some(Self::Basin),
                crate::depot::ID => Some(Self::Depot),
                _ => None,
            },
        }
    }

    fn can_take(self) -> bool {
        !matches!(
            self,
            Self::Funnel | Self::Crafter | Self::CrushingWheels | Self::BlazeBurner
        )
    }

    fn insert(
        self,
        world: World<'_>,
        kinetics: &mut Kinetics,
        pos: Pos,
        item: &Value,
        simulate: bool,
    ) -> (i32, Option<Value>) {
        if self == Self::BlazeBurner {
            return crate::blaze_burner::insert_from_arm(
                world,
                &mut kinetics.blaze_burners,
                pos,
                item,
                simulate,
            )
            .map_or((0, None), |replacement| (1, replacement));
        }
        let transport = TransportedItemStack::new(item.clone());
        let accepted = if self == Self::Funnel {
            crate::funnel::insert_from_arm(world, kinetics, pos, &transport, simulate)
        } else {
            crate::funnel::insert(world, kinetics, pos, UP, &transport, simulate)
        };
        (accepted, None)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Selection {
    pub(crate) pos: [i32; 3],
    pub(crate) kind: PointType,
    take: bool,
}

impl Selection {
    fn target(self, anchor: Pos) -> Option<Pos> {
        let [x, y, z] = self.pos;
        (i64::from(x)
            .pow(2)
            .saturating_add(i64::from(y).pow(2))
            .saturating_add(i64::from(z).pow(2))
            < i64::from(RANGE * RANGE))
        .then(|| offset(anchor, self.pos))
    }

    fn position(self) -> Pos {
        let [x, y, z] = self.pos;
        Pos { x, y, z }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Setup {
    dimension: String,
    points: Vec<Selection>,
}

const MAX_POINTS: usize = (RANGE * 2 + 1).pow(3) as usize;

fn load_setup(item: &Value, dimension: &str) -> Setup {
    filtering::settings(item, SETTINGS)
        .and_then(|fields| fastnbt::from_value::<Setup>(&Value::Compound(fields.clone())).ok())
        .filter(|setup| setup.dimension == dimension && setup.points.len() <= MAX_POINTS)
        .unwrap_or_else(|| Setup {
            dimension: dimension.to_owned(),
            points: Vec::new(),
        })
}

fn edit_point(
    mut setup: Setup,
    mut item: Value,
    held: &mut Stack<'_>,
    pos: Pos,
    kind: PointType,
    clear: bool,
) -> Option<&'static str> {
    let pos = [pos.x, pos.y, pos.z];
    let entry = setup.points.iter().position(|point| point.pos == pos);
    let status = if let Some(index) = entry.filter(|_| clear) {
        setup.points.remove(index);
        "Point removed"
    } else if let Some(index) = entry {
        let point = &mut setup.points[index];
        point.kind = kind;
        point.take = kind.can_take() && !point.take;
        if point.take {
            "Take items here"
        } else {
            "Deposit items here"
        }
    } else if setup.points.len() < MAX_POINTS {
        // The source creates a deposit point and immediately cycles it on the first click.
        setup.points.push(Selection {
            pos,
            kind,
            take: kind.can_take(),
        });
        if kind.can_take() {
            "Take items here"
        } else {
            "Deposit items here"
        }
    } else {
        return None;
    };
    let Value::Compound(data) = fastnbt::to_value(&setup).ok()? else {
        return None;
    };
    *filtering::settings_mut(&mut item, SETTINGS)? = data;
    held.write(&fastnbt::to_bytes(&item).ok()?)
        .then_some(status)
}

pub(crate) fn select_custom(
    context: &mut UseBlock<'_>,
    states: &StateIds,
) -> Option<BlockUseResult> {
    if !context.with_item || !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let kind = PointType::at(context.world, states, context.position)?;
    let item = fastnbt::from_bytes::<Value>(&context.stack.read()?).ok()?;
    if item_id(&item) != Some(ID) {
        return None;
    }
    let setup = load_setup(&item, context.world.dimension_id()?);
    if let Some(status) = edit_point(
        setup,
        item,
        &mut context.stack,
        context.position,
        kind,
        context.sneaking,
    ) {
        context.action_bar(status);
        Some(BlockUseResult::Success)
    } else {
        Some(BlockUseResult::Fail)
    }
}

pub(crate) fn select(context: &mut UseOnBlock<'_>, states: &StateIds) -> UseResult {
    let Some(player) = context.player.filter(|player| player.may_build()) else {
        return UseResult::Fail;
    };
    let Some(bytes) = context.stack.read() else {
        return UseResult::Fail;
    };
    let Ok(item) = fastnbt::from_bytes::<Value>(&bytes) else {
        return UseResult::Fail;
    };
    let Some(dimension) = context.world.dimension_id() else {
        return UseResult::Fail;
    };
    let setup = load_setup(&item, dimension);
    let pos = context.position;
    if let Some(kind) = PointType::at(context.world, states, pos) {
        let Some(status) = edit_point(setup, item, &mut context.stack, pos, kind, context.sneaking)
        else {
            return UseResult::Fail;
        };
        context.action_bar(status);
        return UseResult::Success;
    }

    let Some(step) = FACES.get(usize::from(context.face)) else {
        return UseResult::Fail;
    };
    let placed = if states
        .replaceable
        .contains(&crate::belt_tunnel::read(context.world, pos))
    {
        pos
    } else {
        offset(pos, *step)
    };
    let points: Vec<_> = setup
        .points
        .into_iter()
        .filter_map(|point| {
            let target = point.position();
            let moved = Selection {
                pos: [
                    target.x.checked_sub(placed.x)?,
                    target.y.checked_sub(placed.y)?,
                    target.z.checked_sub(placed.z)?,
                ],
                ..point
            };
            (moved.target(placed).is_some()
                && PointType::at(context.world, states, target) == Some(point.kind))
            .then_some(moved)
        })
        .collect();
    crate::with_world(context.world, |kinetics| {
        kinetics
            .pending_arm_points
            .insert((player.id(), placed), points);
    });
    UseResult::Pass
}

pub(crate) fn player_placed(world: World<'_>, player: Entity<'_>, pos: Pos) {
    crate::with_world(world, |kinetics| {
        if let Some(points) = kinetics.pending_arm_points.remove(&(player.id(), pos)) {
            kinetics.placed_arm_points.push((pos, points));
        }
    });
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Phase {
    #[default]
    SearchInputs,
    MoveToInput,
    SearchOutputs,
    MoveToOutput,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    #[default]
    RoundRobin,
    ForcedRoundRobin,
    PreferFirst,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub(crate) struct Arm {
    pub(crate) interaction_points: Vec<Selection>,
    phase: Phase,
    mode: Mode,
    pub(crate) movement_progress: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) held_item: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) target_point_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_input: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_output: Option<usize>,
}

impl Default for Arm {
    fn default() -> Self {
        Self {
            interaction_points: Vec::new(),
            phase: Phase::SearchInputs,
            mode: Mode::RoundRobin,
            movement_progress: 1.0,
            held_item: None,
            target_point_index: None,
            last_input: None,
            last_output: None,
        }
    }
}

impl Arm {
    pub(crate) fn take_held(self) -> Option<Value> {
        self.held_item
    }

    fn phase(&mut self, phase: Phase, target: Option<usize>) {
        self.phase = phase;
        self.target_point_index = target;
        self.movement_progress = 0.0;
    }

    fn valid_target(&self, setup: &Context<'_>, index: usize) -> Option<(Selection, Pos)> {
        let point = *self.interaction_points.get(index)?;
        let target = point.target(setup.pos)?;
        (PointType::at(setup.world, setup.states, target) == Some(point.kind))
            .then_some((point, target))
    }

    fn amount_fit(&self, setup: &Context<'_>, kinetics: &mut Kinetics, value: &Value) -> i32 {
        let amount = count(value);
        let mut remaining = amount;
        for (i, _) in self.interaction_points.iter().enumerate() {
            if let Some((point, target)) = self.valid_target(setup, i)
                && !point.take
            {
                remaining -= point
                    .kind
                    .insert(
                        setup.world,
                        kinetics,
                        target,
                        &with_count(value, remaining),
                        true,
                    )
                    .0;
                if remaining <= 0 {
                    break;
                }
            }
        }
        amount - remaining.max(0)
    }

    fn choose(
        &self,
        setup: &Context<'_>,
        take: bool,
        last: &mut Option<usize>,
        mut fits: impl FnMut(Selection, Pos) -> bool,
    ) -> Option<usize> {
        let start = if self.mode == Mode::PreferFirst {
            0
        } else {
            last.map_or(0, |i| i.saturating_add(1))
        };
        let candidates = self
            .interaction_points
            .iter()
            .enumerate()
            .skip(start)
            .filter(|(_, point)| point.take == take);
        let limit = if self.mode == Mode::ForcedRoundRobin {
            1
        } else {
            self.interaction_points.len()
        };
        for (idx, _) in candidates.take(limit) {
            if let Some((point, target)) = self.valid_target(setup, idx)
                && fits(point, target)
            {
                *last = self.interaction_points[idx + 1..]
                    .iter()
                    .any(|point| point.take == take)
                    .then_some(idx);
                return Some(idx);
            }
        }
        if self.mode != Mode::ForcedRoundRobin {
            *last = None;
        }
        None
    }

    fn step(
        &mut self,
        setup: &Context<'_>,
        kinetics: &mut Kinetics,
        speed: f32,
        time: u64,
    ) -> bool {
        let previous_progress = self.movement_progress;
        self.movement_progress =
            (self.movement_progress + speed.abs().min(256.0) / 1024.0).clamp(0.0, 1.0);
        let changed = previous_progress != self.movement_progress;
        if self.movement_progress < 1.0 {
            if self.phase == Phase::MoveToInput
                && let Some((point, target)) = self
                    .target_point_index
                    .and_then(|i| self.valid_target(setup, i))
                && point.kind == PointType::Belt
            {
                belt_inventory::lock_segment(kinetics, target);
            }
            if self.phase != Phase::SearchOutputs
                || self.movement_progress < 0.5
                || !time.is_multiple_of(LAZY_TICKS)
            {
                return changed;
            }
        }
        match self.phase {
            Phase::SearchInputs => {
                if setup.world.has_neighbor_signal(setup.pos) {
                    return changed;
                }
                let mut last = self.last_input;
                let next = self.choose(setup, true, &mut last, |_, target| {
                    extract(setup.world, kinetics, target, 64, true)
                        .is_some_and(|item| self.amount_fit(setup, kinetics, &item) > 0)
                });
                self.last_input = last;
                if next.is_some() {
                    self.phase(Phase::MoveToInput, next);
                    return true;
                }
            }
            Phase::MoveToInput => {
                if let Some((_, target)) = self
                    .target_point_index
                    .and_then(|i| self.valid_target(setup, i))
                    && let Some(sample) = extract(setup.world, kinetics, target, 64, true)
                {
                    let amount = self.amount_fit(setup, kinetics, &sample);
                    if amount > 0 {
                        self.held_item = extract(setup.world, kinetics, target, amount, false);
                    }
                }
                self.phase(
                    if self.held_item.is_some() {
                        Phase::SearchOutputs
                    } else {
                        Phase::SearchInputs
                    },
                    None,
                );
                return true;
            }
            Phase::SearchOutputs => {
                if let Some(item) = &self.held_item {
                    let mut last = self.last_output;
                    let next = self.choose(setup, false, &mut last, |point, target| {
                        point
                            .kind
                            .insert(setup.world, kinetics, target, item, true)
                            .0
                            > 0
                    });
                    self.last_output = last;
                    if next.is_some() {
                        self.phase(Phase::MoveToOutput, next);
                        return true;
                    }
                }
            }
            Phase::MoveToOutput => {
                if let Some(item) = self.held_item.take() {
                    let (accepted, replacement) = self
                        .target_point_index
                        .and_then(|i| self.valid_target(setup, i))
                        .map_or((0, None), |(point, target)| {
                            point
                                .kind
                                .insert(setup.world, kinetics, target, &item, false)
                        });
                    let remaining = count(&item) - accepted;
                    self.held_item = replacement
                        .or_else(|| (remaining > 0).then(|| with_count(&item, remaining)));
                }
                self.phase(
                    if self.held_item.is_some() {
                        Phase::SearchOutputs
                    } else {
                        Phase::SearchInputs
                    },
                    None,
                );
                return true;
            }
        }
        changed
    }
}

struct Context<'a> {
    world: World<'a>,
    pos: Pos,
    states: &'a StateIds,
}

fn extract(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    amount: i32,
    simulate: bool,
) -> Option<Value> {
    crate::funnel::extract_at(
        world,
        kinetics,
        pos,
        UP,
        Extraction::up_to(amount),
        simulate,
    )
}

pub(crate) fn use_block(context: &UseBlock<'_>, kinetics: &mut Kinetics) -> BlockUseResult {
    let Some(be) = kinetics.get_mut(context.position) else {
        return BlockUseResult::TryEmptyHand;
    };
    let Some(arm) = be.arm.as_mut() else {
        return BlockUseResult::TryEmptyHand;
    };
    let item = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok());
    if item.as_ref().and_then(item_id) == Some(WRENCH)
        && context.player.is_some_and(|player| player.may_build())
    {
        arm.mode = match arm.mode {
            Mode::RoundRobin => Mode::ForcedRoundRobin,
            Mode::ForcedRoundRobin => Mode::PreferFirst,
            Mode::PreferFirst => Mode::RoundRobin,
        };
        context.action_bar(match arm.mode {
            Mode::RoundRobin => "Round Robin",
            Mode::ForcedRoundRobin => "Forced Round Robin",
            Mode::PreferFirst => "Prefer First",
        });
    } else if let Some(item) = &arm.held_item {
        if !fastnbt::to_bytes(item)
            .ok()
            .is_some_and(|bytes| context.give(&bytes))
        {
            return BlockUseResult::Fail;
        }
        arm.held_item = None;
        arm.phase(Phase::SearchInputs, None);
    } else {
        return BlockUseResult::TryEmptyHand;
    }
    be.dirty = true;
    BlockUseResult::Success
}

pub(crate) fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics, time: u64) {
    kinetics.pending_arm_points.clear();
    for (pos, points) in std::mem::take(&mut kinetics.placed_arm_points) {
        if let Some(be) = kinetics.get_mut(pos)
            && let Some(arm) = be.arm.as_mut()
        {
            arm.interaction_points = points;
            be.dirty = true;
        }
    }
    for pos in kinetics.positions(|kind| matches!(kind, Kind::Arm(_))) {
        let Some((mut arm, speed, Kind::Arm(ceiling))) = kinetics
            .get_mut(pos)
            .and_then(|be| Some((be.arm.take()?, be.get_speed(), be.kind)))
        else {
            continue;
        };
        let changed = arm.step(&Context { world, pos, states }, kinetics, speed, time);
        kinetics
            .arm_displays
            .entry(pos)
            .or_default()
            .render(world, pos, ceiling, &arm);
        if let Some(be) = kinetics.get_mut(pos) {
            be.arm = Some(arm);
            be.dirty |= changed;
        }
    }
    for (pos, item) in kinetics.dropped_arms.drain(..) {
        if let Some(stacks) = STACKS.get()
            && let Ok(bytes) = fastnbt::to_bytes(&item)
        {
            let center = [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5);
            stacks.spawn(world, center, [0.0; 3], &bytes);
        }
    }
}
