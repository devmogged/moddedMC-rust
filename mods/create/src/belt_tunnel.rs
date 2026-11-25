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
const DOWN: u8 = 0;
const UP: u8 = 1;
// Iterate.horizontalDirections follows Direction.from2DDataValue.
const HORIZONTAL: [u8; 4] = [3, 4, 2, 5];

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
    axis: u8,
    shape: Shape,
}

impl Tunnel {
    fn all() -> impl Iterator<Item = Self> {
        [0, 2]
            .into_iter()
            .flat_map(|axis| Shape::ALL.map(|shape| Self { axis, shape }))
    }

    fn properties(self) -> String {
        format!(
            "axis={},shape={}",
            AXES[usize::from(self.axis)],
            self.shape.name()
        )
    }

    fn state(self, states: &StateIds) -> Option<u32> {
        states.lookup(&format!("{ID}[{}]", self.properties()))
    }

    const fn left(self) -> u8 {
        // Clockwise from the positive direction of the belt axis.
        if self.axis == 0 { 3 } else { 4 }
    }

    fn open(self, side: u8) -> bool {
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
    let rows: Vec<_> = Tunnel::all()
        .map(|tunnel| {
            let model = if tunnel.shape == Shape::Closed {
                Shape::Straight
            } else {
                tunnel.shape
            };
            (
                tunnel,
                tunnel.properties(),
                format!("create:block/tunnel/andesite_tunnel/{}", model.name()),
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
            id: ID,
            display_name: "Andesite Tunnel",
            entity: "",
            item_model: "create:item/andesite_tunnel",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for tunnel in Tunnel::all() {
        states.add(content, ID, &tunnel.properties(), None);
        if let Some(id) = tunnel.state(states) {
            states.tunnels.insert(id, tunnel);
        }
    }
}

fn shift(pos: Pos, side: u8) -> Pos {
    offset(pos, FACES[usize::from(side)])
}

fn read(world: World<'_>, pos: Pos) -> u32 {
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

fn valid_output(world: World<'_>, states: &StateIds, pos: Pos, state: u32, side: u8) -> bool {
    if let Some(Kind::Belt(belt)) = states.kind(state) {
        return face_axis(belt.facing) == face_axis(side);
    }
    let block = states.by_id.get(&state).map(|state| state.block);
    matches!(
        block,
        Some(
            crate::depot::ID
                | crate::fluids::item_drain::ID
                | crate::millstone::ID
                | crate::basin::ID
        )
    ) || crate::chute::supports_direct_input(world, states, pos, state)
        || states
            .funnels
            .get(&state)
            .is_some_and(|funnel| funnel.supports_direct_input())
        || states
            .crusher_controllers
            .get(&state)
            .is_some_and(|&(facing, _)| facing == DOWN || facing == side)
}

fn desired(world: World<'_>, states: &StateIds, pos: Pos, belt: Belt) -> Tunnel {
    let below = shift(pos, DOWN);
    let mut tunnel = Tunnel {
        axis: face_axis(belt.facing),
        shape: Shape::Straight,
    };
    let left = tunnel.left();
    let right = left ^ 1;
    let outputs = [left, right].map(|side| {
        let out = shift(below, side);
        valid_output(world, states, out, read(world, out), side)
    });
    tunnel.shape = match outputs {
        [true, true] => Shape::Cross,
        [true, false] => Shape::Left,
        [false, true] => Shape::Right,
        [false, false] => {
            let forward = if tunnel.axis == 0 { 5 } else { 3 };
            let [front, back] = [forward, forward ^ 1].map(|side| {
                let state = read(world, shift(pos, side));
                (
                    states.tunnels.contains_key(&state),
                    extended_funnel(states, state, side),
                )
            });
            if (front.0 && (back.0 || back.1)) || (front.1 && back.0) {
                Shape::Window
            } else {
                Shape::Straight
            }
        }
    };
    tunnel
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
    desired(args.world, states, args.position, belt).state(states)
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
        if states.tunnels.contains_key(&event.state) {
            kinetics.tunnels.insert(event.position);
        } else if states.tunnels.contains_key(&event.previous) {
            kinetics.tunnels.remove(&event.position);
        }
    }
    for pos in std::mem::take(&mut kinetics.pending_tunnel_casings) {
        let below = shift(pos, DOWN);
        let state = read(world, below);
        if kinetics.tunnels.contains(&pos)
            && let Some(belt) = belt_below(states, state)
            && let Some(be) = kinetics.get_mut(below)
            && be.belt_casing == Casing::None
        {
            belt_casing::set_casing(world, below, state, states, belt, be, Casing::Andesite);
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
            if kinetics.tunnels.contains(&candidate) {
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
        let next = desired(world, states, pos, belt);
        if next != previous
            && !(next.axis == previous.axis
                && next.shape == Shape::Window
                && previous.shape == Shape::Closed)
        {
            write(world, states, pos, next);
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

pub(crate) fn use_block(context: &UseBlock<'_>, states: &StateIds) -> Option<BlockUseResult> {
    let mut tunnel = *states.tunnels.get(&context.state)?;
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
    if kinetics.tunnels.contains(&pos) {
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
        u32::try_from(belt_inventory::insert_into_segment(
            kinetics,
            shift(offer.position, DOWN),
            &item,
            offer.simulate,
        ))
        .unwrap_or_default()
    })
}

/// Returns whether a full outlet held the stack and whether a transfer changed its count.
pub(crate) fn split(
    world: World<'_>,
    kinetics: &mut Kinetics,
    chain: Chain,
    item: &mut TransportedItemStack,
    next: f32,
) -> (bool, bool) {
    if kinetics.tunnels.is_empty() || count(&item.item) <= 1 {
        return (false, false);
    }
    let segment = if next == 0.0 && chain.direction_aware_speed() < 0.0 {
        -1
    } else {
        next as i32
    };
    if item.pos as i32 == segment || !(0..chain.length).contains(&segment) {
        return (false, false);
    }
    let belt = chain.position_for_offset(segment);
    let pos = shift(belt, UP);
    if !kinetics.tunnels.contains(&pos) {
        return (false, false);
    }
    let Some(states) = crate::states() else {
        return (false, false);
    };
    let Some(&tunnel) = states.tunnels.get(&read(world, pos)) else {
        return (false, false);
    };
    if tunnel.axis != face_axis(chain.movement_facing())
        || !matches!(tunnel.shape, Shape::Left | Shape::Right | Shape::Cross)
    {
        return (false, false);
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
            return (true, changed);
        }
        if !valid_output(world, states, out, state, side) {
            continue;
        }
        let stack = TransportedItemStack::new(with_count(&item.item, 1));
        let accepted = if let Some(Kind::Belt(belt)) = states.kind(state) {
            if !kinetics
                .get(out)
                .is_some_and(|be| belt_inventory::can_insert_from(be, belt, side))
            {
                continue;
            }
            belt_inventory::try_inserting_from_side(
                kinetics,
                Some(chain.pos),
                out,
                &stack,
                side,
                false,
            )
        } else if let Some(depot) = kinetics.depots.get_mut(&out) {
            crate::depot::try_inserting_from_side(world, out, depot, &stack, side) == 1
        } else if let Some(drain) = kinetics.item_drains.get_mut(&out) {
            drain.insert(&stack, side) == 1
        } else if states.chutes.contains_key(&state) {
            crate::chute::direct_insert(world, states, &mut kinetics.chutes, out, &stack) == 1
        } else if states.funnels.contains_key(&state) {
            crate::funnel::insert_from_belt_end(world, kinetics, out, &stack) == 1
        } else {
            crate::funnel::insert(world, kinetics, out, side ^ 1, &stack, false) == 1
        };
        if !accepted {
            return (true, changed);
        }
        item.resize(world, count(&item.item) - 1);
        changed = true;
        if count(&item.item) <= 1 {
            break;
        }
    }
    (false, changed)
}
