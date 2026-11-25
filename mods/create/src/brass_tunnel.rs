//! Brass Tunnel buffering and connected output distribution.

use fastnbt::Value;
use moddedmc_api::{
    Pos,
    content::{self, ContentEvent, World},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    STACKS, StateIds, WRENCH,
    belt_inventory::{self, Chain, TransportedItemStack, count, with_count},
    belt_tunnel::{Crossing, DirectInput, HORIZONTAL, Tunnel, read, shift},
    filtering::{self, Filter},
    kinetics::{FACES, Kinetics, face_axis},
    processing_recipes::item_id,
};

const UP: u8 = 1;
const DOWN: u8 = 0;
const ARRIVAL_DELAY: u8 = 2;
const BRASS_TUNNEL_TIMER: u8 = 10;

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Mode {
    #[default]
    Split,
    ForcedSplit,
    RoundRobin,
    ForcedRoundRobin,
    PreferNearest,
    Randomize,
    Synchronize,
}

impl Mode {
    fn next(self, backwards: bool) -> Self {
        const MODES: [Mode; 7] = [
            Mode::Split,
            Mode::ForcedSplit,
            Mode::RoundRobin,
            Mode::ForcedRoundRobin,
            Mode::PreferNearest,
            Mode::Randomize,
            Mode::Synchronize,
        ];
        let increment = if backwards { MODES.len() - 1 } else { 1 };
        MODES[(self as usize + increment) % MODES.len()]
    }

    fn name(self) -> &'static str {
        match self {
            Self::Split => "Split",
            Self::ForcedSplit => "Forced Split",
            Self::RoundRobin => "Round Robin",
            Self::ForcedRoundRobin => "Forced Round Robin",
            Self::PreferNearest => "Prefer Nearest",
            Self::Randomize => "Randomize",
            Self::Synchronize => "Synchronize",
        }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Saved {
    mode: Mode,
    filters: [Filter; 4],
    #[serde(skip_serializing_if = "Option::is_none")]
    stack_to_distribute: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stack_entered_from: Option<u8>,
    distribution_progress: u8,
    previous_index: i32,
    synced_output: bool,
}

#[derive(Default)]
pub(crate) struct TunnelData {
    saved: Saved,
    displays: [Option<TransportedItemStack>; 4],
    dirty: bool,
}

fn unload(world: World<'_>, pos: Pos, data: TunnelData, drop_contents: bool) {
    for display in data.displays.into_iter().flatten() {
        belt_inventory::remove_display(world, &display);
    }
    if drop_contents {
        for filter in data.saved.filters {
            filter.drop_item(world, pos);
        }
        if let (Some(stack), Some(stacks)) = (data.saved.stack_to_distribute, STACKS.get())
            && let Ok(bytes) = fastnbt::to_bytes(&stack)
        {
            stacks.spawn(world, center(pos), [0.; 3], &bytes);
        }
    }
}

pub(crate) fn event(world: World<'_>, kinetics: &mut Kinetics, event: &ContentEvent, after: bool) {
    let pos = event.position;
    match event.reason {
        content::NEIGHBOR_CHANGED => {}
        _ if after => {
            if kinetics.brass_tunnels.contains_key(&pos) {
                return;
            }
            let saved = if event.reason == content::LOADED
                || event.reason == content::MOVED_STATE_CHANGED
            {
                world
                    .entity(pos)
                    .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
                    .unwrap_or_default()
            } else {
                let adjacent = kinetics.tunnels.get(&pos).and_then(|&tunnel| {
                    [tunnel.left() ^ 1, tunnel.left()]
                        .into_iter()
                        .filter_map(|side| neighbor(kinetics, pos, tunnel, side))
                        .find_map(|pos| kinetics.brass_tunnels.get(&pos))
                });
                Saved {
                    mode: adjacent.map_or(Mode::Split, |data| data.saved.mode),
                    ..Saved::default()
                }
            };
            kinetics.brass_tunnels.insert(
                pos,
                TunnelData {
                    saved,
                    dirty: event.reason == content::STATE_CHANGED,
                    ..TunnelData::default()
                },
            );
        }
        _ => {
            if let Some(data) = kinetics.brass_tunnels.remove(&pos) {
                unload(world, pos, data, event.reason == content::STATE_CHANGED);
            }
        }
    }
}

fn neighbor(kinetics: &Kinetics, pos: Pos, tunnel: Tunnel, side: u8) -> Option<Pos> {
    let next = shift(pos, side);
    kinetics
        .tunnels
        .get(&next)
        .is_some_and(|other| other.brass && other.axis == tunnel.axis)
        .then_some(next)
}

fn group(world: World<'_>, kinetics: &Kinetics, pos: Pos, tunnel: Tunnel) -> Vec<Pos> {
    let mut group = vec![pos];
    let Some(states) = crate::states() else {
        return group;
    };
    if !has_any_flap(world, states, pos, tunnel) {
        return group;
    }
    for side in [tunnel.left() ^ 1, tunnel.left()] {
        let mut current = pos;
        while let Some(adjacent) = neighbor(kinetics, current, tunnel, side) {
            if !has_any_flap(world, states, adjacent, kinetics.tunnels[&adjacent]) {
                break;
            }
            group.push(adjacent);
            current = adjacent;
        }
    }
    group
}

fn has_any_flap(world: World<'_>, states: &StateIds, pos: Pos, tunnel: Tunnel) -> bool {
    HORIZONTAL
        .into_iter()
        .any(|side| tunnel.open(side) && has_flap(world, states, pos, side))
}

fn has_flap(world: World<'_>, states: &StateIds, pos: Pos, side: u8) -> bool {
    let adjacent = read(world, shift(pos, side));
    adjacent != content::UNLOADED_STATE
        && !states.tunnels.contains_key(&adjacent)
        && !states
            .funnels
            .get(&adjacent)
            .is_some_and(|funnel| funnel.extends_towards(side ^ 1))
}

fn distributes(world: World<'_>, kinetics: &Kinetics, pos: Pos, tunnel: Tunnel) -> bool {
    let Some(states) = crate::states() else {
        return false;
    };
    let side_flap = HORIZONTAL.into_iter().any(|side| {
        face_axis(side) != tunnel.axis && tunnel.open(side) && has_flap(world, states, pos, side)
    });
    side_flap
        || (has_any_flap(world, states, pos, tunnel)
            && [tunnel.left(), tunnel.left() ^ 1]
                .into_iter()
                .filter_map(|side| neighbor(kinetics, pos, tunnel, side))
                .any(|other| has_any_flap(world, states, other, kinetics.tunnels[&other])))
}

fn buffer(data: &mut TunnelData, value: &Value, from: Option<u8>, simulate: bool) -> bool {
    if data.saved.stack_to_distribute.is_some() || data.saved.synced_output {
        return false;
    }
    if !simulate {
        data.saved.stack_to_distribute = Some(value.clone());
        data.saved.stack_entered_from = from;
        data.saved.distribution_progress = ARRIVAL_DELAY;
        data.dirty = true;
    }
    true
}

pub(crate) fn offer(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
    simulate: bool,
) -> Option<i32> {
    offer_from(world, kinetics, pos, item, None, simulate)
}

pub(crate) fn offer_from(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
    from: Option<u8>,
    simulate: bool,
) -> Option<i32> {
    let &tunnel = kinetics.tunnels.get(&pos)?;
    if !tunnel.brass || !distributes(world, kinetics, pos, tunnel) {
        return None;
    }
    let data = kinetics.brass_tunnels.get_mut(&pos)?;
    let amount = count(&item.item);
    if amount <= 0 || from.is_some_and(|side| !matches_filter(&data.saved, side, &item.item)) {
        return Some(0);
    }
    Some(if buffer(data, &item.item, from, simulate) {
        amount
    } else {
        0
    })
}

pub(crate) fn crossing(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    chain: Chain,
    item: &mut TransportedItemStack,
) -> Crossing {
    let side = chain.movement_facing() ^ 1;
    if kinetics.brass_tunnels.get(&pos).is_some_and(|data| {
        !matches_filter(&data.saved, side, &item.item)
            || (data.saved.stack_to_distribute.is_some()
                && kinetics
                    .tunnels
                    .get(&pos)
                    .is_some_and(|&tunnel| distributes(world, kinetics, pos, tunnel)))
    }) {
        let boundary = if chain.direction_aware_speed() > 0.0 {
            0.99
        } else {
            0.01
        };
        item.pos = item.pos.trunc() + boundary;
        return Crossing::Held;
    }
    match offer_from(world, kinetics, pos, item, Some(side), false) {
        Some(0) => Crossing::Held,
        Some(_) => Crossing::Buffered,
        None => Crossing::Pass,
    }
}

fn filter_index(side: u8) -> Option<usize> {
    (2..=5).contains(&side).then(|| usize::from(side - 2))
}

fn matches_filter(saved: &Saved, side: u8, item: &Value) -> bool {
    filter_index(side).is_none_or(|idx| saved.filters[idx].matches(item))
}

pub(crate) fn use_block(context: &mut UseBlock<'_>, tunnel: Tunnel) -> Option<BlockUseResult> {
    if !context.main_hand || !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let item = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok());
    let pos = context.position;
    let world = context.world;
    crate::with_world(world, |kinetics| {
        if context.face == UP
            && item.as_ref().and_then(item_id) == Some(WRENCH)
            && distributes(world, kinetics, pos, tunnel)
        {
            let mode = kinetics
                .brass_tunnels
                .get(&pos)?
                .saved
                .mode
                .next(context.sneaking);
            for member in group(world, kinetics, pos, tunnel) {
                if let Some(data) = kinetics.brass_tunnels.get_mut(&member) {
                    data.saved.mode = mode;
                    data.saved.synced_output = false;
                    data.dirty = true;
                }
            }
            context.action_bar(mode.name());
            return Some(BlockUseResult::Success);
        }
        let side = context.face;
        let index = filter_index(side)?;
        if !tunnel.open(side)
            || !(0.60..=1.0).contains(&context.hit[1])
            || (!context.with_item && !context.sneaking)
        {
            return None;
        }
        let column = filtering::slot_column(side, context.hit)?;
        let data = kinetics.brass_tunnels.get_mut(&pos)?;
        if !data.saved.filters[index].configure(context, item.as_ref(), column, false) {
            return None;
        }
        data.dirty = true;
        Some(BlockUseResult::Success)
    })
}

#[derive(Clone, Copy)]
enum Output {
    Input(DirectInput),
    Eject { chain: Chain, segment: i32 },
}

#[derive(Clone, Copy)]
struct Target {
    pos: Pos,
    side: u8,
    output: Output,
}

impl Target {
    fn insert(
        self,
        world: World<'_>,
        kinetics: &mut Kinetics,
        item: &Value,
        amount: i32,
        simulate: bool,
    ) -> i32 {
        if amount <= 0 {
            return 0;
        }
        let mut stack = TransportedItemStack::new(with_count(item, amount));
        match self.output {
            Output::Input(input) => input.offer(
                world,
                kinetics,
                (self.pos, self.side),
                &stack,
                simulate,
                None,
            ),
            Output::Eject { chain, segment } => {
                stack.pos = (segment + i32::from(chain.direction_aware_speed() > 0.0)) as f32;
                if simulate || belt_inventory::eject(world, chain, &stack) {
                    amount
                } else {
                    0
                }
            }
        }
    }
}

fn chain_below(kinetics: &Kinetics, pos: Pos) -> Option<(Chain, i32)> {
    let belt = kinetics.get(shift(pos, DOWN))?;
    let chain = Chain::of(kinetics, belt.controller?)?;
    Some((chain, belt.belt_index))
}

fn targets(
    world: World<'_>,
    states: &StateIds,
    kinetics: &Kinetics,
    members: &[Pos],
    item: &Value,
    came_from: Option<u8>,
) -> [Vec<Target>; 2] {
    let mut targets = [Vec::new(), Vec::new()];
    for (index, &pos) in members.iter().enumerate() {
        let (Some((chain, segment)), Some(tunnel), Some(data)) = (
            chain_below(kinetics, pos),
            kinetics.tunnels.get(&pos),
            kinetics.brass_tunnels.get(&pos),
        ) else {
            continue;
        };
        let movement = chain.movement_facing();
        for perpendicular in [true, false] {
            if perpendicular && index != 0 {
                continue;
            }
            for side in HORIZONTAL {
                if (face_axis(side) != face_axis(movement)) != perpendicular
                    || !tunnel.open(side)
                    || side == movement ^ 1
                    || (side == movement && chain.speed == 0.0)
                    || (index == 0 && came_from == Some(side))
                    || !matches_filter(&data.saved, side, item)
                {
                    continue;
                }
                let out = shift(shift(pos, DOWN), side);
                let state = read(world, out);
                if state == content::UNLOADED_STATE
                    || states
                        .funnels
                        .get(&read(world, shift(out, UP)))
                        .is_some_and(|funnel| funnel.pulls_towards(side))
                {
                    continue;
                }
                let output = match DirectInput::at(states, state) {
                    Some(input) if input.accepts(world, states, kinetics, out, state, side) => {
                        Output::Input(input)
                    }
                    None if side == movement && !world.face_sturdy(out, side ^ 1) => {
                        Output::Eject { chain, segment }
                    }
                    _ => continue,
                };
                let filtered =
                    filter_index(side).is_some_and(|idx| data.saved.filters[idx].stack.is_some());
                targets[usize::from(!filtered)].push(Target {
                    pos: out,
                    side,
                    output,
                });
            }
        }
    }
    targets
}

/// Simulate and distribute one priority tier before trying unfiltered mouths.
fn distribute<const SPLIT: bool>(
    saved: &mut Saved,
    out: &[Target],
    mut offer: impl FnMut(Target, i32, bool) -> i32,
    amount: i32,
) -> i32 {
    if out.is_empty() || amount <= 0 {
        return amount;
    }
    let start = match saved.mode {
        Mode::Randomize => belt_inventory::next_int(out.len() as u32) as usize,
        Mode::PreferNearest | Mode::Synchronize => 0,
        _ => saved.previous_index.max(0) as usize % out.len(),
    };
    let ordered: Vec<_> = out[start..].iter().chain(&out[..start]).copied().collect();
    let forced = matches!(saved.mode, Mode::ForcedSplit | Mode::ForcedRoundRobin);
    let robin = matches!(saved.mode, Mode::RoundRobin | Mode::ForcedRoundRobin);
    let mut accepted = 0;
    if SPLIT {
        let mut ready = Vec::with_capacity(ordered.len());
        for target in ordered {
            let capacity = offer(target, amount, true);
            if capacity > 0 {
                ready.push((target, capacity));
            } else if forced {
                return amount;
            }
        }
        let mut left = amount;
        let mut planned = vec![0; ready.len()];
        for _ in 0..2 {
            let available = planned
                .iter()
                .zip(&ready)
                .filter(|(planned, (_, capacity))| **planned < *capacity)
                .count() as i32;
            if available == 0 || left == 0 {
                break;
            }
            let (share, remainder) = (left / available, left % available);
            let mut eligible = 0;
            for (slot, &(_, capacity)) in planned.iter_mut().zip(&ready) {
                let unused = capacity - *slot;
                if unused <= 0 {
                    continue;
                }
                let sent = unused.min(share + i32::from(eligible < remainder));
                *slot += sent;
                left -= sent;
                eligible += 1;
            }
        }
        for ((target, _), send) in ready.into_iter().zip(planned) {
            accepted += offer(target, send, false);
        }
    } else {
        for target in ordered {
            if forced && offer(target, amount, true) <= 0 {
                return amount;
            }
            accepted = offer(target, amount, false);
            if accepted > 0 || robin {
                break;
            }
        }
    }
    saved.previous_index = ((start + 1) % out.len()) as i32;
    amount - accepted
}

fn tick_distribution(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics, pos: Pos) {
    let Some((chain, _)) = chain_below(kinetics, pos) else {
        return;
    };
    if chain.speed == 0.0 {
        return;
    }
    let Some(&tunnel) = kinetics.tunnels.get(&pos) else {
        return;
    };
    let members = group(world, kinetics, pos, tunnel);
    let Some(data) = kinetics.brass_tunnels.get_mut(&pos) else {
        return;
    };
    let mode = data.saved.mode;
    if data.saved.distribution_progress > 0 {
        data.saved.distribution_progress -= 1;
        if data.saved.distribution_progress != 0 {
            return;
        }
    }
    let synced = data.saved.synced_output;
    let mut saved = data.saved.clone();
    if mode == Mode::Synchronize {
        let ready = members
            .iter()
            .filter(|p| {
                kinetics
                    .brass_tunnels
                    .get(p)
                    .is_some_and(|data| data.saved.stack_to_distribute.is_some())
            })
            .count();
        if (ready == members.len() && !synced) || (ready == 0 && synced) {
            let active = ready != 0;
            for member in &members {
                if let Some(data) = kinetics.brass_tunnels.get_mut(member) {
                    data.saved.synced_output = active;
                    data.dirty = true;
                }
            }
            saved.synced_output = active;
        }
        if !saved.synced_output {
            return;
        }
    }
    let Some(item) = saved.stack_to_distribute.take() else {
        return;
    };
    let candidates = targets(
        world,
        states,
        kinetics,
        &members,
        &item,
        saved.stack_entered_from,
    );
    let mut amount = count(&item);
    for tier in candidates {
        let simulate_or_insert =
            |target: Target, n, simulate| target.insert(world, kinetics, &item, n, simulate);
        amount = if matches!(mode, Mode::Split | Mode::ForcedSplit) {
            distribute::<true>(&mut saved, &tier, simulate_or_insert, amount)
        } else {
            distribute::<false>(&mut saved, &tier, simulate_or_insert, amount)
        };
    }
    if amount > 0 {
        saved.stack_to_distribute = Some(with_count(&item, amount));
        saved.distribution_progress = BRASS_TUNNEL_TIMER;
    } else {
        saved.stack_entered_from = None;
    }
    if let Some(data) = kinetics.brass_tunnels.get_mut(&pos) {
        data.saved = saved;
        data.dirty = true;
    }
}

fn center(pos: Pos) -> [f64; 3] {
    [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5)
}

pub(crate) fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    let mut active: Vec<_> = kinetics
        .brass_tunnels
        .iter()
        .filter_map(|(&pos, data)| {
            (data.saved.stack_to_distribute.is_some() || data.saved.synced_output).then_some(pos)
        })
        .collect();
    active.sort_unstable_by_key(|pos| (pos.x, pos.y, pos.z));
    for pos in active {
        tick_distribution(world, states, kinetics, pos);
    }
    for (&pos, data) in &mut kinetics.brass_tunnels {
        if data.dirty
            && let Ok(bytes) = fastnbt::to_bytes(&data.saved)
            && world.set_entity(pos, &bytes)
        {
            data.dirty = false;
        }
        let Some(tunnel) = kinetics.tunnels.get(&pos) else {
            continue;
        };
        for side in HORIZONTAL {
            let Some(index) = filter_index(side) else {
                continue;
            };
            let sample = data.saved.filters[index]
                .stack
                .as_ref()
                .filter(|_| tunnel.open(side));
            if sample.is_none() && data.displays[index].is_none() {
                continue;
            }
            let [x, y, z] = center(pos);
            let [dx, _, dz] = FACES[usize::from(side)].map(f64::from);
            let mut shown = belt_inventory::item_display(
                [x + dx * 0.49, y + 5.0 / 16.0, z + dz * 0.49],
                filtering::preview_angle(side),
                true,
            );
            shown.scale = 0.24;
            filtering::render(world, sample, &mut data.displays[index], shown);
        }
    }
}
