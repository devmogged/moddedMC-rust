//! The translating case of Create 6.0.10's ContraptionCollider and ContinuousOBBCollider.

use moddedmc_api::{
    Pos,
    content::{CollisionBox, World},
    entities::{
        ENTITY_ALIVE, ENTITY_HANGING, ENTITY_ITEM, ENTITY_LIVING, ENTITY_PLAYER,
        ENTITY_PUSH_IGNORED, EntityChange, SET_MOTION, SET_ON_GROUND,
    },
};
use serde::{Deserialize, Serialize};

use crate::{ENTITIES, contraption::Blocks};

const ENTITY_BOUNDS_EPSILON: f64 = 1.0e-7;
const SEPARATION_EPSILON: f64 = 1.0e-4;
const HORIZONTAL_EPSILON: f64 = 1.0 / 128.0;

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
    axis: usize,
    step: f64,
}

impl Default for ContinuousSeparationManifold {
    fn default() -> Self {
        Self {
            entry: -1.0,
            exit: f64::MAX,
            discrete: true,
            separation: f64::MAX,
            axis: 1,
            step: f64::MAX,
        }
    }
}

impl ContinuousSeparationManifold {
    fn separate(&mut self, axis: usize, relative_center: f64, extents: f64, motion: f64) -> bool {
        let diff = relative_center.abs() - extents;
        if diff > 0.0 {
            if motion.signum() == relative_center.signum() || diff > motion.abs() {
                return true;
            }
            self.discrete = false;
            self.entry = self.entry.max(diff / motion.abs());
            self.exit = self.exit.min((diff + extents) / motion.abs());
        } else if axis == 1 {
            self.step = (extents - relative_center).min(self.step);
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
    colliders: &[Collider],
    motion: [f64; 3],
    max_step: f64,
) -> CollisionResponse {
    let center = entity.center();
    let extent = entity.extents();
    let mut response = CollisionResponse {
        displacement: [0.0; 3],
        time: 1.0,
        surface: false,
    };
    for vertical in [false, true] {
        for collider in colliders {
            let block_center = collider.center();
            let block_extent = collider.extents();
            if (0..3).any(|i| {
                (center[i] + motion[i] - block_center[i]).abs() > block_extent[i] + extent[i] + 0.5
            }) {
                continue;
            }
            let mut manifold = ContinuousSeparationManifold::default();
            if (0..3).any(|i| {
                manifold.separate(
                    i,
                    center[i] + response.displacement[i] - block_center[i],
                    extent[i] + block_extent[i],
                    motion[i],
                )
            }) {
                continue;
            }
            response.surface |= vertical;
            let time = manifold.time_of_impact();
            if time > 0.0 && time < 1.0 {
                response.time = response.time.min(time);
            } else if manifold.discrete {
                if manifold.step <= max_step {
                    response.displacement[1] += with_signed_epsilon(manifold.step);
                } else if manifold.separation < f64::MAX {
                    response.displacement[manifold.axis] +=
                        with_signed_epsilon(manifold.separation);
                }
            }
        }
        if response.displacement[1] == 0.0 && response.time == 1.0 {
            break;
        }
        if !vertical {
            response.displacement[0] *= 1.0 + HORIZONTAL_EPSILON;
            response.displacement[2] *= 1.0 + HORIZONTAL_EPSILON;
        }
    }
    response
}

pub fn collide_entities(
    world: World<'_>,
    blocks: &Blocks,
    anchor: Pos,
    before: [f64; 3],
    after: [f64; 3],
) {
    let Some(entities) = ENTITIES.get() else {
        return;
    };
    let origin = [anchor.x, anchor.y, anchor.z].map(f64::from);
    let translation = std::array::from_fn(|i| origin[i] + after[i]);
    let movement = std::array::from_fn::<_, 3, _>(|i| after[i] - before[i]);
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
    let mut query = bounds.translate(translation).inflate(2.0);
    query.max[1] += 32.0;
    let ids = entities.entities_in(world, query.min, query.max, 0);
    for id in ids {
        entities.with_entity(world, id, |entity| {
            let flags = entity.flags();
            if flags & (ENTITY_ALIVE | ENTITY_HANGING | ENTITY_PUSH_IGNORED) != ENTITY_ALIVE {
                return;
            }
            let (Some(view), Some(body)) = (entity.view(), entity.body()) else {
                return;
            };
            // shortcut: living entities and loose items until generic push reactions and riders are exposed.
            if view.kind & ENTITY_PLAYER != 0 || view.kind & (ENTITY_ITEM | ENTITY_LIVING) == 0 {
                return;
            }
            let body = Collider {
                min: body.min,
                max: body.max,
            }
            .translate(translation.map(|n| -n))
            .inflate(ENTITY_BOUNDS_EPSILON);
            let relative_motion = std::array::from_fn(|i| view.motion[i] - movement[i]);
            let response = collide_many(
                body,
                &colliders,
                relative_motion,
                entity.step_height().unwrap_or(0.0),
            );
            let mut motion = view.motion;
            let mut change = EntityChange {
                mask: SET_MOTION,
                ..EntityChange::default()
            };
            if response.time != 1.0 {
                motion[1] = relative_motion[1] * response.time + movement[1];
            }
            if response.displacement != [0.0; 3] {
                for i in 0..3 {
                    let threshold = if i == 1 { 0.0 } else { HORIZONTAL_EPSILON };
                    if response.displacement[i].abs() > threshold
                        && motion[i] * response.displacement[i] < 0.0
                    {
                        motion[i] = if i == 1 { movement[i] } else { 0.0 };
                    }
                }
                entity.move_by(response.displacement);
            }
            if response.surface {
                entity.reset_fall_distance();
                change.mask |= SET_ON_GROUND;
                change.on_ground = true;
                if view.kind & ENTITY_ITEM != 0 {
                    motion[0] *= 0.5;
                    motion[2] *= 0.5;
                }
                entity.move_by([movement[0], 0.0, movement[2]]);
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
        let result = collide_many(entity, &[Collider::BLOCK], [0.0, -1.0, 0.0], 0.0);
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
        let result = collide_many(entity, &[Collider::BLOCK], [0.0; 3], 0.6);
        assert!((result.displacement[0] - 0.1001).abs() < 1.0e-12);
        assert_eq!(result.displacement[1], 0.0);
        assert!(!result.surface);
    }
}
