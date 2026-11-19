//! Create 6.0.10's `BeltMovementHandler`: belts carry the entities standing on them.

use moddedmc_api::{
    Pos,
    content::{self, World},
    entities::{
        self, ENTITY_ALIVE, ENTITY_FLYING, ENTITY_HANGING, ENTITY_ITEM, ENTITY_LIVING,
        ENTITY_PLAYER, ENTITY_PUSH_IGNORED, ENTITY_SNEAKING, Entities, Entity, EntityChange,
    },
};

use crate::{
    ENTITIES, StateIds, belt_inventory,
    kinetics::{Belt, Kind, Kinetics, Part, Passenger, Slope, face_axis, positive},
};

/// `BeltBlockEntity.getBeltMovementSpeed` divides the speed by this.
pub const SPEED_DIVISOR: f32 = 480.0;
/// The top of a flat belt in `transportEntity`.
const TOP: f32 = 13.0 / 16.0;
/// `transportEntity` gives a moving up entity at least this much downward push first.
const MIN_VELOCITY: f32 = 0.13;

/// `BeltBlock.canTransportObjects`.
pub const fn can_transport_objects(belt: Belt) -> bool {
    !matches!(belt.slope, Slope::Vertical | Slope::Sideways)
}

/// `Entity.blockPosition`.
fn block_position([x, y, z]: [f64; 3]) -> Pos {
    Pos {
        x: x.floor() as i32,
        y: y.floor() as i32,
        z: z.floor() as i32,
    }
}

fn is_belt(kinetics: &Kinetics, pos: Pos) -> bool {
    matches!(kinetics.kind(pos), Some(Kind::Belt(_)))
}

/// `BeltBlock.entityInside`. Cardboard boots and diving boots don't exist, so sneaking players
/// always step off.
pub fn entity_inside(
    kinetics: &mut Kinetics,
    pos: Pos,
    state: u32,
    belt: Belt,
    entity: Entity<'_>,
) {
    if !can_transport_objects(belt) {
        return;
    }
    let Some(view) = entity.view() else { return };
    if view.kind & ENTITY_PLAYER != 0 && entity.flags() & (ENTITY_SNEAKING | ENTITY_FLYING) != 0 {
        return;
    }
    // BeltHelper.getSegmentBE.
    let Some(segment) = kinetics.get(pos) else {
        return;
    };
    if view.kind & ENTITY_ITEM != 0 {
        belt_inventory::entity_inside(kinetics, pos, entity, view.motion[1]);
        return;
    }
    // getControllerBE.
    let Some(controller) = segment
        .controller
        .filter(|&controller| is_belt(kinetics, controller))
    else {
        return;
    };
    let Some(passengers) = kinetics
        .get_mut(controller)
        .and_then(|be| be.passengers.as_mut())
    else {
        return;
    };
    let id = entity.id();
    if let Some(info) = passengers.get_mut(&id) {
        if info.ticks_since_last_collision != 0 || pos == block_position(view.position) {
            *info = Passenger::new(pos, state);
        }
    } else {
        passengers.insert(id, Passenger::new(pos, state));
        entity.change(&EntityChange {
            mask: entities::SET_ON_GROUND,
            on_ground: true,
            ..EntityChange::default()
        });
    }
}

/// `BeltMovementHandler.canBeTransported`.
fn can_be_transported(entity: Entity<'_>) -> bool {
    let flags = entity.flags();
    if flags & ENTITY_ALIVE == 0 {
        return false;
    }
    let player = entity
        .view()
        .is_some_and(|view| view.kind & ENTITY_PLAYER != 0);
    !(player && flags & ENTITY_SNEAKING != 0)
}

/// The passenger loop at the end of `BeltBlockEntity.tick`, for each controller in ticker
/// order. It runs after the kinetic ticks rather than inside each one: moving entities doesn't
/// touch the networks.
pub fn tick(world: World<'_>, kinetics: &mut Kinetics, states: &StateIds) {
    let Some(&entities) = ENTITIES.get() else {
        return;
    };
    players_on_belts(world, kinetics, states, entities);
    for controller in kinetics.belt_controllers() {
        let Some(be) = kinetics.get_mut(controller) else {
            continue;
        };
        let (Kind::Belt(belt), speed) = (be.kind, be.get_speed()) else {
            continue;
        };
        if speed == 0.0 {
            continue;
        }
        let mut passengers = be.passengers.take().unwrap_or_default();
        let left_after = if belt.slope == Slope::Horizontal {
            1
        } else {
            3
        };
        let belts: &Kinetics = kinetics;
        passengers.retain(|&id, info| {
            let mut keep = false;
            entities.with_entity(world, id, |entity| {
                if !can_be_transported(entity) || info.ticks_since_last_collision > left_after {
                    return;
                }
                keep = true;
                info.ticks_since_last_collision += 1;
                transport_entity(
                    world,
                    belts,
                    states,
                    entities,
                    (speed, belt.slope),
                    entity,
                    info,
                );
            });
            keep
        });
        if let Some(be) = kinetics.get_mut(controller) {
            be.passengers = Some(passengers);
        }
    }
}

/// `BeltBlock.updateEntityAfterFallOn` for players. Create's client runs it when its player
/// lands on a belt. A stock client stands on the carrier block's full cube instead and the
/// server never moves players, so look for players resting on each moving chain.
fn players_on_belts(
    world: World<'_>,
    kinetics: &mut Kinetics,
    states: &StateIds,
    entities: Entities,
) {
    let mut found = Vec::new();
    for controller in kinetics.belt_controllers() {
        if kinetics
            .get(controller)
            .is_none_or(|be| be.get_speed() == 0.0)
        {
            continue;
        }
        let chain = kinetics.belt_chain(controller);
        let (Some(first), Some(last)) = (chain.first(), chain.last()) else {
            continue;
        };
        let min = [
            first.x.min(last.x),
            first.y.min(last.y),
            first.z.min(last.z),
        ];
        let max = [
            first.x.max(last.x),
            first.y.max(last.y),
            first.z.max(last.z),
        ];
        found.extend(entities.entities_in(
            world,
            min.map(f64::from),
            [max[0] + 1, max[1] + 2, max[2] + 1].map(f64::from),
            // No entity has this id, so nothing is left out.
            i32::MIN,
        ));
    }
    found.sort_unstable();
    found.dedup();
    for id in found {
        entities.with_entity(world, id, |entity| {
            let Some(view) = entity.view() else { return };
            if view.kind & ENTITY_PLAYER == 0 || !view.on_ground {
                return;
            }
            let standing = block_position(view.position);
            let below = Pos {
                y: standing.y - 1,
                ..standing
            };
            let mut state = [content::UNLOADED_STATE; 2];
            if !world.read(&[standing, below], &mut state) {
                return;
            }
            for (pos, state) in [(standing, state[0]), (below, state[1])] {
                if let Some(Kind::Belt(belt)) = states.kind(state) {
                    entity_inside(kinetics, pos, state, belt, entity);
                    return;
                }
            }
        });
    }
}

fn scale([x, y, z]: [f64; 3], factor: f64) -> [f64; 3] {
    [x * factor, y * factor, z * factor]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `Vec3.normalize`.
pub fn normalize(v: [f64; 3]) -> [f64; 3] {
    let length = v.iter().map(|c| c * c).sum::<f64>().sqrt();
    if length < f64::from(1.0e-5_f32) {
        [0.0; 3]
    } else {
        scale(v, 1.0 / length)
    }
}

fn belt_block_at(world: World<'_>, states: &StateIds, pos: Pos) -> bool {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[pos], &mut state) && matches!(states.kind(state[0]), Some(Kind::Belt(_)))
}

/// Not `shouldIgnoreBlocking`: a hanging entity, one pistons ignore, or one riding `me`.
fn blocks(me: Entity<'_>, other: Entity<'_>) -> bool {
    other.flags() & (ENTITY_HANGING | ENTITY_PUSH_IGNORED) == 0
        && !me.has_indirect_passenger(other.id())
}

/// `BeltMovementHandler.transportEntity`. `controller` is the belt entity's speed and slope.
#[expect(
    clippy::too_many_lines,
    reason = "one Create method, kept whole for review against it"
)]
fn transport_entity(
    world: World<'_>,
    kinetics: &Kinetics,
    states: &StateIds,
    entities: Entities,
    (speed, controller_slope): (f32, Slope),
    entity: Entity<'_>,
    info: &mut Passenger,
) {
    let pos = info.last_pos;
    let Some(view) = entity.view() else { return };
    let below_passenger = block_position(view.position);
    // Don't fight other belts.
    if !is_belt(kinetics, pos) || (below_passenger != pos && is_belt(kinetics, below_passenger)) {
        return;
    }
    let Some(Kind::Belt(belt)) = states.kind(info.last_state) else {
        return;
    };
    let not_horizontal = controller_slope != Slope::Horizontal;
    if speed.abs() < 1.0 {
        return;
    }
    let [x, y, z] = view.position;
    if y - f64::from(0.25_f32) < f64::from(pos.y) {
        return;
    }
    // Lock entities in place.
    let is_player = view.kind & ENTITY_PLAYER != 0;
    let living = view.kind & ENTITY_LIVING != 0;
    if living && !is_player {
        entity.add_effect("minecraft:slowness", 10, 1, false, false);
    }

    let axis = face_axis(belt.facing);
    let movement_speed = speed / SPEED_DIVISOR;
    // Direction.get(axis == X ? NEGATIVE : POSITIVE, axis), and the clockwise axis's positive.
    let (movement_direction, centering_direction) = if axis == 0 {
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0])
    } else {
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
    };
    let mut movement = scale(movement_direction, f64::from(movement_speed));
    let diff_center = if axis == 2 {
        f64::from(pos.x as f32 + 0.5) - x
    } else {
        f64::from(pos.z as f32 + 0.5) - z
    };
    if diff_center.abs() > f64::from(48.0_f32 / 64.0) {
        return;
    }

    let height = y - f64::from(pos.y);
    let top = f64::from(TOP);
    let upward = belt.slope == Slope::Upward;
    let on_slope = not_horizontal
        && (matches!(belt.part, Part::Middle | Part::Pulley)
            || belt.part == (if upward { Part::End } else { Part::Start }) && height < top
            || belt.part == (if upward { Part::Start } else { Part::End }) && height > top);
    // movementFacing is the belt axis's positive end when the speed is negative.
    let with_facing = (speed < 0.0) == positive(belt.facing);
    let (down, up) = if with_facing {
        (Slope::Downward, Slope::Upward)
    } else {
        (Slope::Upward, Slope::Downward)
    };
    let mut moving_down = on_slope && belt.slope == down;
    let mut moving_up = on_slope && belt.slope == up;
    if axis == 2 {
        std::mem::swap(&mut moving_down, &mut moving_up);
    }
    let along = movement[usize::from(axis)].abs();
    if moving_up {
        movement[1] += along;
    }
    if moving_down {
        movement[1] -= along;
    }

    let centering = scale(
        centering_direction,
        diff_center * f64::from(movement_speed.abs().min(0.1)) * 4.0,
    );
    let body = entity.body();
    // Non-living entities report no input.
    if body.is_none_or(|body| body.input[0] == 0.0 && body.input[2] == 0.0) {
        movement = add(movement, centering);
    }

    let step = if is_player {
        None
    } else {
        entity.step_height().map(|step| step as f32)
    };
    if step.is_some() {
        entity.set_step_height(1.0);
    }

    // Entity collisions.
    if movement_speed.abs() < 0.5
        && let Some(body) = body
    {
        let check = scale(normalize(movement), 0.5);
        // bb.move(check).inflate(-|check|), which AABB's constructor re-sorts per axis.
        let corners = [0, 1, 2].map(|i| {
            let (a, b) = (
                body.min[i] + check[i] + check[i].abs(),
                body.max[i] + check[i] - check[i].abs(),
            );
            (a.min(b), a.max(b))
        });
        let blocked = entities
            .entities_in(
                world,
                corners.map(|c| c.0),
                corners.map(|c| c.1),
                entity.id(),
            )
            .into_iter()
            .any(|other| {
                entities
                    .with_entity(world, other, |other| blocks(entity, other))
                    .unwrap_or(false)
            });
        if blocked {
            // Create leaves the step height at 1 on this path.
            entity.change(&EntityChange {
                mask: entities::SET_MOTION,
                ..EntityChange::default()
            });
            info.ticks_since_last_collision -= 1;
            return;
        }
    }

    entity.reset_fall_distance();
    if is_player {
        // Create moves the player on its own client. A stock client moves itself and Pumpkin's
        // move skips players, so the push goes to the client as motion, which its next move
        // applies.
        entity.change(&EntityChange {
            mask: entities::SET_MOTION | entities::SET_ON_GROUND,
            motion: movement,
            on_ground: true,
            ..EntityChange::default()
        });
        return;
    }
    if moving_up {
        let y_movement = -(movement[1].abs().max(f64::from(MIN_VELOCITY))) as f32;
        entity.move_by([0.0, f64::from(y_movement), 0.0]);
        entity.move_by([movement[0], 0.0, movement[2]]);
    } else if moving_down {
        entity.move_by([movement[0], 0.0, movement[2]]);
        entity.move_by([0.0, movement[1], 0.0]);
    } else {
        entity.move_by(movement);
    }
    entity.change(&EntityChange {
        mask: entities::SET_ON_GROUND,
        on_ground: true,
        ..EntityChange::default()
    });
    if let Some(step) = step {
        entity.set_step_height(f64::from(step));
    }

    let Some(moved) = entity.view() else { return };
    let here = block_position(moved.position);
    let moved_past_ending_slope = on_slope
        && (belt_block_at(world, states, here)
            || belt_block_at(
                world,
                states,
                Pos {
                    y: here.y - 1,
                    ..here
                },
            ));
    if moved_past_ending_slope && !moving_down && movement_speed.abs() > 0.0 {
        let [x, y, z] = moved.position;
        entity.set_position([x, y + movement[1], z]);
    }
    if moved_past_ending_slope {
        entity.change(&EntityChange {
            mask: entities::SET_MOTION,
            motion: movement,
            ..EntityChange::default()
        });
    }
}
