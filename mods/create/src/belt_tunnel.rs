//! Andesite Tunnel block states and BeltTunnelInteractionHandler's side transfers.

use std::collections::HashSet;

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, World},
    entities::Entity,
    interaction::{BlockUseResult, UseBlock},
    item_input::Offer,
};

use crate::{
    AXES, STONE, StateIds, WRENCH,
    belt_casing::{self, Casing},
    belt_inventory::{self, Chain, TransportedItemStack, count, with_count},
    kinetics::{Belt, FACES, Kind, Kinetics, Slope, face_axis, offset},
    processing_recipes::item_id,
};

pub(crate) const ID: &str = "create:andesite_tunnel";
pub(crate) const BRASS: &str = "create:brass_tunnel";
const DOWN: u8 = 0;
const UP: u8 = 1;
// Iterate.horizontalDirections follows Direction.from2DDataValue.
pub(crate) const HORIZONTAL: [u8; 4] = [3, 4, 2, 5];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Straight,
    Window,
    Closed,
    Left,
    Right,
    Cross,
}

impl Shape {
    const ALL: [Self; 6] = [
        Self::Straight,
        Self::Window,
        Self::Closed,
        Self::Left,
        Self::Right,
        Self::Cross,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Straight => "straight",
            Self::Window => "window",
            Self::Closed => "closed",
            Self::Left => "t_left",
            Self::Right => "t_right",
            Self::Cross => "cross",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tunnel {
    pub(crate) axis: u8,
    shape: Shape,
    pub(crate) brass: bool,
}

impl Tunnel {
    fn all(brass: bool) -> impl Iterator<Item = Self> {
        [0, 2]
            .into_iter()
            .flat_map(move |axis| Shape::ALL.map(|shape| Self { axis, shape, brass }))
    }

    fn id(self) -> &'static str {
        if self.brass { BRASS } else { ID }
    }

    fn properties(self) -> String {
        format!(
            "axis={},shape={}",
            AXES[usize::from(self.axis)],
            self.shape.name()
        )
    }

    fn state(self, states: &StateIds) -> Option<u32> {
        states.lookup(&format!("{}[{}]", self.id(), self.properties()))
    }

    pub(crate) const fn left(self) -> u8 {
        // Clockwise from the positive direction of the belt axis.
        if self.axis == 0 { 3 } else { 4 }
    }

    pub(crate) fn open(self, side: u8) -> bool {
        face_axis(side) == self.axis
            || match self.shape {
                Shape::Cross => true,
                Shape::Left => side == self.left(),
                Shape::Right => side == self.left() ^ 1,
                _ => false,
            }
    }

    fn bounds(self) -> Vec<ShapeBox> {
        // BeltTunnelShapes: remove a 12x15 opening from the outer two voxels of each open side.
        let mut boxes = vec![
            ShapeBox {
                min: [0, 10, 0],
                max: [16, 16, 16],
            },
            ShapeBox {
                min: [2, -5, 2],
                max: [14, 10, 14],
            },
        ];
        for (side, z) in [(2, 0), (3, 14)] {
            if self.open(side) {
                for x in [0, 14] {
                    boxes.push(ShapeBox {
                        min: [x, -5, z],
                        max: [x + 2, 10, z + 2],
                    });
                }
            } else {
                boxes.push(ShapeBox {
                    min: [0, -5, z],
                    max: [16, 10, z + 2],
                });
            }
        }
        for (side, x) in [(4, 0), (5, 14)] {
            if !self.open(side) {
                boxes.push(ShapeBox {
                    min: [x, -5, 2],
                    max: [x + 2, 10, 14],
                });
            }
        }
        boxes
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    register_material(content, registry, false)?;
    register_material(content, registry, true)
}

fn register_material(
    content: Content,
    registry: &mut Registry<'_>,
    brass: bool,
) -> Result<(), RegistrationError> {
    let (id, material, name) = if brass {
        (BRASS, "brass", "Brass Tunnel")
    } else {
        (ID, "andesite", "Andesite Tunnel")
    };
    let rows: Vec<_> = Tunnel::all(brass)
        .map(|tunnel| {
            let model = if tunnel.shape == Shape::Closed {
                Shape::Straight
            } else {
                tunnel.shape
            };
            (
                tunnel,
                tunnel.properties(),
                format!("create:block/tunnel/{material}_tunnel/{}", model.name()),
                tunnel.bounds(),
            )
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(tunnel, properties, model, shape)| State {
            properties,
            physical: STONE,
            model,
            rotating_model: "",
            x: 0,
            y: if tunnel.axis == 0 { 0 } else { 90 },
            orientation: [0., 0., 0., 1.],
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id,
            display_name: name,
            entity: "",
            item_model: &format!("create:item/{material}_tunnel"),
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for tunnel in Tunnel::all(false).chain(Tunnel::all(true)) {
        states.add(content, tunnel.id(), &tunnel.properties(), None);
        if let Some(id) = tunnel.state(states) {
            states.tunnels.insert(id, tunnel);
        }
    }
}

pub(crate) fn shift(pos: Pos, side: u8) -> Pos {
    offset(pos, FACES[usize::from(side)])
}

pub(crate) fn read(world: World<'_>, pos: Pos) -> u32 {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[pos], &mut state);
    state[0]
}

fn belt_below(states: &StateIds, state: u32) -> Option<Belt> {
    match states.kind(state) {
        Some(Kind::Belt(belt)) if belt.slope == Slope::Horizontal => Some(belt),
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum DirectInput {
    Belt(Belt),
    Depot,
    Drain,
    Chute,
    Funnel,
    Millstone,
    Basin,
    Crusher,
}

impl DirectInput {
    pub(crate) fn at(states: &StateIds, state: u32) -> Option<Self> {
        if let Some(Kind::Belt(belt)) = states.kind(state) {
            return Some(Self::Belt(belt));
        }
        if states.chutes.contains_key(&state) {
            return Some(Self::Chute);
        }
        if states.funnels.contains_key(&state) {
            return Some(Self::Funnel);
        }
        if states.crusher_controllers.contains_key(&state) {
            return Some(Self::Crusher);
        }
        match states.by_id.get(&state)?.block {
            crate::depot::ID => Some(Self::Depot),
            crate::fluids::item_drain::ID => Some(Self::Drain),
            crate::millstone::ID => Some(Self::Millstone),
            crate::basin::ID => Some(Self::Basin),
            _ => None,
        }
    }

    fn supports(self, world: World<'_>, states: &StateIds, pos: Pos, state: u32, side: u8) -> bool {
        match self {
            Self::Chute => crate::chute::supports_direct_input(world, states, pos, state),
            Self::Funnel => states.funnels[&state].supports_direct_input(),
            Self::Crusher => {
                let (facing, _) = states.crusher_controllers[&state];
                facing == DOWN || facing == side
            }
            _ => true,
        }
    }

    pub(crate) fn accepts(
        self,
        world: World<'_>,
        states: &StateIds,
        kinetics: &Kinetics,
        pos: Pos,
        state: u32,
        side: u8,
    ) -> bool {
        if let Self::Belt(belt) = self {
            return kinetics
                .get(pos)
                .is_some_and(|be| belt_inventory::can_insert_from(be, belt, side));
        }
        self.supports(world, states, pos, state, side)
    }

    pub(crate) fn offer(
        self,
        world: World<'_>,
        kinetics: &mut Kinetics,
        (pos, side): (Pos, u8),
        stack: &TransportedItemStack,
        simulate: bool,
        ticking: Option<Pos>,
    ) -> i32 {
        match self {
            Self::Belt(_) => {
                if belt_inventory::try_inserting_from_side(
                    world, kinetics, ticking, pos, stack, side, simulate,
                ) {
                    count(&stack.item)
                } else {
                    0
                }
            }
            Self::Depot => kinetics.depots.get_mut(&pos).map_or(0, |depot| {
                crate::depot::offer_from_side(world, pos, depot, stack, side, simulate)
            }),
            Self::Drain => kinetics
                .item_drains
                .get_mut(&pos)
                .map_or(0, |drain| drain.offer(stack, side, simulate)),
            Self::Chute => crate::states().map_or(0, |states| {
                crate::chute::direct_insert(
                    world,
                    states,
                    &mut kinetics.chutes,
                    pos,
                    stack,
                    simulate,
                )
            }),
            Self::Funnel => {
                crate::funnel::insert_from_belt_end(world, kinetics, pos, stack, simulate)
            }
            Self::Millstone => crate::millstone::insert(kinetics, pos, &stack.item, simulate),
            Self::Basin => crate::basin::insert(world, pos, &stack.item, simulate).unwrap_or(0),
            Self::Crusher => crate::crusher::insert(kinetics, pos, &stack.item, simulate),
        }
    }
}

fn valid_output(world: World<'_>, states: &StateIds, pos: Pos, state: u32, side: u8) -> bool {
    match DirectInput::at(states, state) {
        Some(DirectInput::Belt(belt)) => face_axis(belt.facing) == face_axis(side),
        Some(input) => input.supports(world, states, pos, state, side),
        None => false,
    }
}

fn desired(
    world: World<'_>,
    states: &StateIds,
    pos: Pos,
    belt: Belt,
    brass: bool,
) -> Option<Tunnel> {
    let below = shift(pos, DOWN);
    let mut tunnel = Tunnel {
        axis: face_axis(belt.facing),
        shape: Shape::Straight,
        brass,
    };
    let left = tunnel.left();
    let right = left ^ 1;
    let outputs = [left, right].map(|side| {
        let out = shift(below, side);
        let state = read(world, out);
        (state != content::UNLOADED_STATE).then(|| valid_output(world, states, out, state, side))
    });
    let [Some(left), Some(right)] = outputs else {
        return None;
    };
    tunnel.shape = match [left, right] {
        [true, true] => Shape::Cross,
        [true, false] => Shape::Left,
        [false, true] => Shape::Right,
        [false, false] => {
            let forward = if tunnel.axis == 0 { 5 } else { 3 };
            let [front, back] = [forward, forward ^ 1].map(|side| read(world, shift(pos, side)));
            if [front, back].contains(&content::UNLOADED_STATE) {
                return None;
            }
            let front_tunnel = states.tunnels.contains_key(&front);
            let back_tunnel = states.tunnels.contains_key(&back);
            if (front_tunnel && (back_tunnel || extended_funnel(states, back, forward ^ 1)))
                || (back_tunnel && extended_funnel(states, front, forward))
            {
                Shape::Window
            } else {
                Shape::Straight
            }
        }
    };
    Some(tunnel)
}

fn extended_funnel(states: &StateIds, state: u32, side: u8) -> bool {
    states
        .funnels
        .get(&state)
        .is_some_and(|funnel| funnel.extends_towards(side ^ 1))
}

pub(crate) fn placed_state(args: Placement<'_>) -> Option<u32> {
    let states = crate::states()?;
    let belt = belt_below(states, read(args.world, shift(args.position, DOWN)))?;
    let brass = states.tunnels.get(&args.state)?.brass;
    desired(args.world, states, args.position, belt, brass)?.state(states)
}

pub(crate) fn player_placed(world: World<'_>, _: Entity<'_>, pos: Pos, state: u32) {
    if crate::states().is_some_and(|states| states.tunnels.contains_key(&state)) {
        crate::with_world(world, |kinetics| kinetics.pending_tunnel_casings.push(pos));
    }
}

pub(crate) fn update(
    world: World<'_>,
    states: &StateIds,
    kinetics: &mut Kinetics,
    events: &[ContentEvent],
) {
    for event in events {
        let before = states.tunnels.get(&event.previous);
        let after = states.tunnels.get(&event.state);
        if let Some(&state) = after {
            kinetics.tunnels.insert(event.position, state);
        } else if before.is_some() {
            kinetics.tunnels.remove(&event.position);
        }
        if before.is_some_and(|state| state.brass) || after.is_some_and(|state| state.brass) {
            crate::brass_tunnel::event(
                world,
                kinetics,
                event,
                after.is_some_and(|state| state.brass),
            );
        }
    }
    for pos in std::mem::take(&mut kinetics.pending_tunnel_casings) {
        let below = shift(pos, DOWN);
        let state = read(world, below);
        if let Some(&tunnel) = kinetics.tunnels.get(&pos)
            && let Some(belt) = belt_below(states, state)
            && let Some(be) = kinetics.get_mut(below)
            && be.belt_casing == Casing::None
        {
            let casing = if tunnel.brass {
                Casing::Brass
            } else {
                Casing::Andesite
            };
            belt_casing::set_casing(world, below, state, states, belt, be, casing);
        }
    }
    if kinetics.tunnels.is_empty() {
        return;
    }
    let mut affected = HashSet::new();
    for event in events {
        let pos = event.position;
        let above = shift(pos, UP);
        for candidate in [pos, above].into_iter().chain(
            HORIZONTAL
                .into_iter()
                .flat_map(|side| [shift(pos, side), shift(above, side)]),
        ) {
            if kinetics.tunnels.contains_key(&candidate) {
                affected.insert(candidate);
            }
        }
    }
    for pos in affected {
        let state = read(world, pos);
        let Some(&previous) = states.tunnels.get(&state) else {
            continue;
        };
        let support = read(world, shift(pos, DOWN));
        if support == content::UNLOADED_STATE {
            continue;
        }
        let Some(belt) = belt_below(states, support).filter(|_| {
            states
                .by_id
                .get(&support)
                .is_some_and(|entry| entry.selector.contains("casing=true"))
        }) else {
            world.destroy(pos, true);
            continue;
        };
        let Some(next) = desired(world, states, pos, belt, previous.brass) else {
            continue;
        };
        if next != previous
            && !(next.axis == previous.axis
                && next.shape == Shape::Window
                && previous.shape == Shape::Closed)
            && write(world, states, pos, next)
        {
            kinetics.tunnels.insert(pos, next);
        }
    }
}

fn write(world: World<'_>, states: &StateIds, pos: Pos, tunnel: Tunnel) -> bool {
    tunnel.state(states).is_some_and(|state| {
        world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: content::UPDATE_CLIENTS,
        }]) == 1
    })
}

pub(crate) fn use_block(context: &mut UseBlock<'_>, states: &StateIds) -> Option<BlockUseResult> {
    let mut tunnel = *states.tunnels.get(&context.state)?;
    if tunnel.brass
        && let Some(result) = crate::brass_tunnel::use_block(context, tunnel)
    {
        return Some(result);
    }
    if !context.with_item
        || context.sneaking
        || !context.player.is_some_and(|player| player.may_build())
    {
        return None;
    }
    let item = fastnbt::from_bytes(&context.stack.read()?).ok()?;
    if item_id(&item) != Some(WRENCH) {
        return None;
    }
    tunnel.shape = match tunnel.shape {
        Shape::Window => Shape::Closed,
        Shape::Closed => Shape::Window,
        _ => return None,
    };
    Some(if write(context.world, states, context.position, tunnel) {
        BlockUseResult::Success
    } else {
        BlockUseResult::Fail
    })
}

pub(crate) fn item_target(kinetics: &Kinetics, pos: Pos) -> Pos {
    if kinetics.tunnels.contains_key(&pos) {
        shift(pos, DOWN)
    } else {
        pos
    }
}

pub(crate) fn receive_item(offer: Offer<'_>) -> u32 {
    let Ok(item) = fastnbt::from_bytes(offer.stack) else {
        return 0;
    };
    let item = TransportedItemStack::new(item);
    crate::with_world(offer.world, |kinetics| {
        u32::try_from(insert(
            offer.world,
            kinetics,
            offer.position,
            &item,
            offer.simulate,
        ))
        .unwrap_or_default()
    })
}

pub(crate) fn insert(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    item: &TransportedItemStack,
    simulate: bool,
) -> i32 {
    if let Some(accepted) = crate::brass_tunnel::offer(world, kinetics, pos, item, simulate) {
        return accepted;
    }
    belt_inventory::insert_into_segment(kinetics, shift(pos, DOWN), item, simulate)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Crossing {
    Pass,
    Held,
    Buffered,
}

/// Returns the belt action and whether an Andesite side transfer changed the remaining count.
pub(crate) fn handle_crossing(
    world: World<'_>,
    kinetics: &mut Kinetics,
    chain: Chain,
    item: &mut TransportedItemStack,
    next: f32,
) -> (Crossing, bool) {
    if kinetics.tunnels.is_empty() {
        return (Crossing::Pass, false);
    }
    let segment = if next == 0.0 && chain.direction_aware_speed() < 0.0 {
        -1
    } else {
        next as i32
    };
    if item.pos as i32 == segment || !(0..chain.length).contains(&segment) {
        return (Crossing::Pass, false);
    }
    let belt = chain.position_for_offset(segment);
    let pos = shift(belt, UP);
    let Some(&tunnel) = kinetics.tunnels.get(&pos) else {
        return (Crossing::Pass, false);
    };
    if tunnel.brass {
        return (
            crate::brass_tunnel::crossing(world, kinetics, pos, chain, item),
            false,
        );
    }
    let Some(states) = crate::states() else {
        return (Crossing::Pass, false);
    };
    if count(&item.item) <= 1
        || tunnel.axis != face_axis(chain.movement_facing())
        || !matches!(tunnel.shape, Shape::Left | Shape::Right | Shape::Cross)
    {
        return (Crossing::Pass, false);
    }
    let mut changed = false;
    for side in HORIZONTAL {
        if face_axis(side) == tunnel.axis || !tunnel.open(side) {
            continue;
        }
        let adjacent = read(world, shift(pos, side));
        if states.tunnels.contains_key(&adjacent) || extended_funnel(states, adjacent, side) {
            continue;
        }
        let out = shift(belt, side);
        let state = read(world, out);
        if state == content::UNLOADED_STATE {
            return (Crossing::Held, changed);
        }
        let Some(input) = DirectInput::at(states, state)
            .filter(|input| input.accepts(world, states, kinetics, out, state, side))
        else {
            continue;
        };
        let stack = TransportedItemStack::new(with_count(&item.item, 1));
        if input.offer(world, kinetics, (out, side), &stack, false, Some(chain.pos)) != 1 {
            return (Crossing::Held, changed);
        }
        item.resize(world, count(&item.item) - 1);
        changed = true;
        if count(&item.item) <= 1 {
            break;
        }
    }
    (Crossing::Pass, changed)
}
