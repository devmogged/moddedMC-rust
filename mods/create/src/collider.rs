//! Create 6.0.10's ContraptionCollider and ContinuousOBBCollider.

use moddedmc_api::{
    Pos,
    abi::{ENTITY_NO_PHYSICS, ENTITY_PASSENGER},
    content::{CollisionBox, World},
    entities::{
        ENTITY_ALIVE, ENTITY_HANGING, ENTITY_ITEM, ENTITY_LIVING, ENTITY_PLAYER,
        ENTITY_PUSH_IGNORED, EntityChange, SET_MOTION, SET_ON_GROUND,
    },
};
use serde::{Deserialize, Serialize};

use crate::{ENTITIES, contraption::Blocks, seat};

const ENTITY_BOUNDS_EPSILON: f64 = 1.0e-7;
const SEPARATION_EPSILON: f64 = 1.0e-4;
const HORIZONTAL_EPSILON: f64 = 1.0 / 128.0;
const UNIT_AXES: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter().zip(right).map(|(a, b)| a * b).sum()
}

#[derive(Clone, Copy, Debug)]
struct Rotation {
    axis: usize,
    sin: f64,
    cos: f64,
}

impl Rotation {
    const IDENTITY: Self = Self {
        axis: 1,
        sin: 0.0,
        cos: 1.0,
    };

    fn new(axis: u8, degrees: f32) -> Self {
        let (sin, cos) = f64::from(degrees).to_radians().sin_cos();
        Self {
            axis: usize::from(axis),
            sin,
            cos,
        }
    }

    fn inverse(self) -> Self {
        Self {
            sin: -self.sin,
            ..self
        }
    }

    fn transform(self, mut position: [f64; 3]) -> [f64; 3] {
        let u = (self.axis + 1) % 3;
        let v = (self.axis + 2) % 3;
        let (first, second) = (position[u], position[v]);
        position[u] = first * self.cos - second * self.sin;
        position[v] = first * self.sin + second * self.cos;
        position
    }

    fn project_extents(self, extents: [f64; 3]) -> [f64; 3] {
        std::array::from_fn(|i| {
            dot(
                self.inverse().transform(UNIT_AXES[i]).map(f64::abs),
                extents,
            )
        })
    }

    fn vertical(self) -> bool {
        self.axis != 1 && (self.sin != 0.0 || self.cos != 1.0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    origin: [f64; 3],
    rotation: Rotation,
}

impl Frame {
    fn new(anchor: Pos, offset: [f64; 3], rotation: Rotation) -> Self {
        let origin = [anchor.x, anchor.y, anchor.z];
        Self {
            origin: std::array::from_fn(|i| f64::from(origin[i]) + offset[i]),
            rotation,
        }
    }

    pub fn translated(anchor: Pos, offset: [f64; 3]) -> Self {
        Self::new(anchor, offset, Rotation::IDENTITY)
    }

    pub fn rotating(anchor: Pos, axis: u8, angle: f32) -> Self {
        Self::new(anchor, [0.0; 3], Rotation::new(axis, angle))
    }

    pub fn to_local(self, point: [f64; 3]) -> [f64; 3] {
        let from_pivot = std::array::from_fn(|i| point[i] - self.origin[i] - 0.5);
        self.rotation
            .inverse()
            .transform(from_pivot)
            .map(|n| n + 0.5)
    }

    pub fn to_world(self, point: [f64; 3]) -> [f64; 3] {
        let from_pivot = self.rotation.transform(point.map(|n| n - 0.5));
        std::array::from_fn(|i| from_pivot[i] + self.origin[i] + 0.5)
    }

    fn bounds(self, collider: Collider) -> Collider {
        Collider::from_center(
            self.to_world(collider.center()),
            self.rotation.project_extents(collider.extents()),
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct Collider {
    min: [f64; 3],
    max: [f64; 3],
}

impl Collider {
    pub const BLOCK: Self = Self {
        min: [0.0; 3],
        max: [1.0; 3],
    };

    pub const fn from_shape(shape: CollisionBox) -> Self {
        Self {
            min: shape.min,
            max: shape.max,
        }
    }

    fn from_center(center: [f64; 3], extent: [f64; 3]) -> Self {
        Self {
            min: std::array::from_fn(|i| center[i] - extent[i]),
            max: std::array::from_fn(|i| center[i] + extent[i]),
        }
    }

    pub fn translate(self, delta: [f64; 3]) -> Self {
        Self {
            min: std::array::from_fn(|i| self.min[i] + delta[i]),
            max: std::array::from_fn(|i| self.max[i] + delta[i]),
        }
    }

    fn center(self) -> [f64; 3] {
        std::array::from_fn(|i| (self.min[i] + self.max[i]) * 0.5)
    }

    fn extents(self) -> [f64; 3] {
        std::array::from_fn(|i| (self.max[i] - self.min[i]) * 0.5)
    }

    fn inflate(self, by: f64) -> Self {
        Self {
            min: self.min.map(|n| n - by),
            max: self.max.map(|n| n + by),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ContinuousSeparationManifold {
    entry: f64,
    exit: f64,
    discrete: bool,
    separation: f64,
    axis: [f64; 3],
    step_axis: [f64; 3],
    step: f64,
}

impl ContinuousSeparationManifold {
    fn new(step_axis: [f64; 3]) -> Self {
        Self {
            entry: -1.0,
            exit: f64::MAX,
            discrete: true,
            separation: f64::MAX,
            axis: step_axis,
            step_axis,
            step: f64::MAX,
        }
    }
    fn separate(
        &mut self,
        axis: [f64; 3],
        relative_center: f64,
        extents: f64,
        motion: f64,
    ) -> bool {
        let diff = relative_center.abs() - extents;
        if diff > 0.0 {
            if motion.signum() == relative_center.signum() || diff > motion.abs() {
                return true;
            }
            self.discrete = false;
            self.entry = self.entry.max(diff / motion.abs());
            self.exit = self.exit.min((diff + extents) / motion.abs());
        } else {
            let alignment = dot(self.step_axis, axis);
            if alignment != 0.0 {
                let step = (extents - relative_center * alignment.signum()) / alignment.abs();
                if step != 0.0 || alignment.abs() == 1.0 {
                    self.step = step.min(self.step);
                }
            }
        }
        if relative_center != 0.0 && -diff <= self.separation.abs() {
            self.axis = axis;
            self.separation = diff.abs().copysign(relative_center);
        }
        false
    }

    fn time_of_impact(self) -> f64 {
        if self.entry <= self.exit {
            self.entry
        } else {
            -1.0
        }
    }
}

fn with_signed_epsilon(separation: f64) -> f64 {
    if separation == 0.0 {
        0.0
    } else {
        separation + SEPARATION_EPSILON.copysign(separation)
    }
}

#[derive(Clone, Copy, Debug)]
struct CollisionResponse {
    displacement: [f64; 3],
    time: f64,
    surface: bool,
}

fn collide_many(
    entity: Collider,
    rotation: Rotation,
    colliders: &[Collider],
    motion: [f64; 3],
    max_step: f64,
) -> CollisionResponse {
    let center = entity.center();
    let extent = entity.extents();
    let entity_axes = UNIT_AXES.map(|axis| rotation.transform(axis));
    let entity_extent_in_local = rotation.project_extents(extent);
    let local_query_extent: [f64; 3] = std::array::from_fn(|i| {
        extent[i]
            * rotation
                .inverse()
                .transform(UNIT_AXES[i])
                .map(f64::abs)
                .iter()
                .sum::<f64>()
            + 0.5
    });
    let entity_motion = rotation.inverse().transform(motion);
    let mut response = CollisionResponse {
        displacement: [0.0; 3],
        time: 1.0,
        surface: false,
    };
    for vertical in [false, true] {
        if !vertical && rotation.vertical() {
            continue;
        }
        for collider in colliders {
            let block_center = collider.center();
            let block_extent = collider.extents();
            if (0..3).any(|i| {
                (center[i] + motion[i] - block_center[i]).abs()
                    > block_extent[i] + local_query_extent[i]
            }) {
                continue;
            }
            let mut manifold = ContinuousSeparationManifold::new(entity_axes[1]);
            let relative_center =
                std::array::from_fn(|i| center[i] + response.displacement[i] - block_center[i]);
            if (0..3).any(|i| {
                manifold.separate(
                    UNIT_AXES[i],
                    relative_center[i],
                    entity_extent_in_local[i] + block_extent[i],
                    motion[i],
                )
            }) {
                continue;
            }
            let relative_entity = rotation.inverse().transform(relative_center);
            let block_extent_in_entity = rotation.inverse().project_extents(block_extent);
            if (0..3).any(|i| {
                manifold.separate(
                    entity_axes[i],
                    relative_entity[i],
                    extent[i] + block_extent_in_entity[i],
                    entity_motion[i],
                )
            }) {
                continue;
            }
            response.surface |= vertical;
            let time = manifold.time_of_impact();
            if time > 0.0 && time < 1.0 {
                response.time = response.time.min(time);
            } else if manifold.discrete {
                let (axis, separation) = if manifold.step <= max_step {
                    (manifold.step_axis, manifold.step)
                } else {
                    (manifold.axis, manifold.separation)
                };
                if separation < f64::MAX {
                    for (displacement, axis) in response.displacement.iter_mut().zip(axis) {
                        *displacement += axis * with_signed_epsilon(separation);
                    }
                }
            }
        }
        if vertical || (response.displacement[1] == 0.0 && response.time == 1.0) {
            break;
        }
        response.displacement[0] *= 1.0 + HORIZONTAL_EPSILON;
        response.displacement[2] *= 1.0 + HORIZONTAL_EPSILON;
    }
    response
}

fn supported_in_frame(body: Collider, frame: Frame, colliders: &[Collider]) -> bool {
    let [x, y, z] = frame.to_local(body.min);
    let [end_x, _, end_z] = frame.to_local(body.max);
    colliders.iter().any(|shape| {
        let gap = y - shape.max[1];
        (0.0..=SEPARATION_EPSILON + 2.0 * ENTITY_BOUNDS_EPSILON).contains(&gap)
            && x < shape.max[0]
            && end_x > shape.min[0]
            && z < shape.max[2]
            && end_z > shape.min[2]
    })
}

pub fn collide_entities(
    world: World<'_>,
    blocks: &mut Blocks,
    anchor: Pos,
    before: [f64; 3],
    after: [f64; 3],
) {
    collide(
        world,
        blocks,
        Frame::translated(anchor, before),
        Frame::translated(anchor, after),
    );
}

pub fn collide_rotating(
    world: World<'_>,
    blocks: &mut Blocks,
    anchor: Pos,
    axis: u8,
    before: f32,
    after: f32,
) {
    collide(
        world,
        blocks,
        Frame::rotating(anchor, axis, before),
        Frame::rotating(anchor, axis, after),
    );
}

fn collide(world: World<'_>, blocks: &mut Blocks, before: Frame, after: Frame) {
    blocks.harvest(world, before, after);
    for local in blocks.seats() {
        let point = seat::position(local);
        seat::move_carrier(world, before.to_world(point), after.to_world(point));
    }
    let Some(entities) = ENTITIES.get() else {
        return;
    };
    let movement = std::array::from_fn::<_, 3, _>(|i| after.origin[i] - before.origin[i]);
    let rotation = after.rotation.inverse();
    let colliders: Vec<_> = blocks.colliders().collect();
    if colliders.is_empty() {
        return;
    }
    let mut bounds = Collider {
        min: [f64::MAX; 3],
        max: [f64::MIN; 3],
    };
    for collider in &colliders {
        for i in 0..3 {
            bounds.min[i] = bounds.min[i].min(collider.min[i]);
            bounds.max[i] = bounds.max[i].max(collider.max[i]);
        }
    }
    let mut query = after.bounds(bounds).inflate(2.0);
    query.max[1] += 32.0;
    let ids = entities.entities_matching(
        world,
        query.min,
        query.max,
        -1,
        ENTITY_HANGING | ENTITY_NO_PHYSICS | ENTITY_PUSH_IGNORED | ENTITY_PASSENGER,
    );
    for id in ids {
        entities.with_entity(world, id, |entity| {
            if entity.flags() & ENTITY_ALIVE == 0 {
                return;
            }
            let (Some(view), Some(body)) = (entity.view(), entity.body()) else {
                return;
            };
            if view.kind & ENTITY_PLAYER != 0 {
                return;
            }
            if view.kind & (ENTITY_ITEM | ENTITY_LIVING) == 0 {
                let kind = entity.type_id();
                // shortcut: keep the non-living filter explicit until the host reports all push reactions.
                if !(kind.ends_with("minecart")
                    || kind.ends_with("_boat")
                    || kind.ends_with("_raft"))
                {
                    return;
                }
            }
            let body = Collider {
                min: body.min,
                max: body.max,
            };
            let oriented_body =
                Collider::from_center(after.to_local(body.center()), body.extents())
                    .inflate(ENTITY_BOUNDS_EPSILON);
            let relative_motion =
                rotation.transform(std::array::from_fn(|i| view.motion[i] - movement[i]));
            let mut response = collide_many(
                oriented_body,
                rotation,
                &colliders,
                relative_motion,
                entity.step_height().unwrap_or(0.0),
            );
            if !response.surface
                && movement != [0.0; 3]
                && movement[1] == 0.0
                && view.motion[1] <= 0.0
                && before.rotation.sin == 0.0
                && before.rotation.cos == 1.0
            {
                // The separation slop leaves a resting body just above the previous top face.
                response.surface = supported_in_frame(body, before, &colliders);
            }
            let mut motion = view.motion;
            let mut change = EntityChange {
                mask: SET_MOTION,
                ..EntityChange::default()
            };
            if response.time != 1.0 {
                let motion_response = after
                    .rotation
                    .transform(relative_motion.map(|n| n * response.time));
                motion[1] = motion_response[1] + movement[1];
            }
            let displacement = after.rotation.transform(response.displacement);
            if displacement != [0.0; 3] {
                for i in 0..3 {
                    let threshold = if i == 1 { 0.0 } else { HORIZONTAL_EPSILON };
                    if displacement[i].abs() > threshold && motion[i] * displacement[i] < 0.0 {
                        motion[i] = if i == 1 { movement[i] } else { 0.0 };
                    }
                }
                entity.move_by(displacement);
            }
            if response.surface {
                entity.reset_fall_distance();
                change.mask |= SET_ON_GROUND;
                change.on_ground = true;
                if view.kind & ENTITY_ITEM != 0 {
                    motion[0] *= 0.5;
                    motion[2] *= 0.5;
                }
                let current = entity.view().map_or(view.position, |view| view.position);
                let contact = after.to_world(before.to_local(current));
                entity.move_by([contact[0] - current[0], 0.0, contact[2] - current[2]]);
            }
            if response.surface || response.displacement != [0.0; 3] || response.time != 1.0 {
                change.motion = motion;
                entity.change(&change);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falling_entity_hits_platform_halfway_through_its_motion() {
        let entity = Collider {
            min: [0.25, 1.5, 0.25],
            max: [0.75, 2.5, 0.75],
        };
        let result = collide_many(
            entity,
            Rotation::IDENTITY,
            &[Collider::BLOCK],
            [0.0, -1.0, 0.0],
            0.0,
        );
        assert_eq!(result.time, 0.5);
        assert_eq!(result.displacement, [0.0; 3]);
        assert!(result.surface);
    }

    #[test]
    fn side_overlap_resolves_horizontally_when_the_step_is_too_high() {
        let entity = Collider {
            min: [0.9, 0.2, 0.2],
            max: [1.4, 0.8, 0.8],
        };
        let result = collide_many(
            entity,
            Rotation::IDENTITY,
            &[Collider::BLOCK],
            [0.0; 3],
            0.6,
        );
        assert!((result.displacement[0] - 0.1001).abs() < 1.0e-12);
        assert_eq!(result.displacement[1], 0.0);
        assert!(!result.surface);
    }
}
