use pumpkin_util::math::{
    boundingbox::BoundingBox,
    vector3::{Axis, Vector3},
};

use crate::{entities::Entities, terrain::World};

const STEP_HEIGHT: f32 = 0.6;
const SHAPE_EPSILON: f64 = 1.0e-7;

pub fn back_off_from_edge(
    world: &World,
    entities: &Entities,
    bounds: BoundingBox,
    mut delta: Vector3<f64>,
    grounded: bool,
    fall_distance: f64,
) -> Vector3<f64> {
    const STEP: f64 = 0.05;
    let can_fall = |x, z, depth: f64| {
        let min =
            bounds.min + Vector3::new(x + SHAPE_EPSILON, -depth - SHAPE_EPSILON, z + SHAPE_EPSILON);
        let max = bounds.max + Vector3::new(x - SHAPE_EPSILON, 0.0, z - SHAPE_EPSILON);
        let area = BoundingBox::new(min, Vector3::new(max.x, bounds.min.y, max.z));
        world.shapes(area).is_empty() && entities.solid_shapes(area).is_empty()
    };
    let height = f64::from(STEP_HEIGHT);
    if delta.y > 0.0
        || !(grounded || fall_distance < height && !can_fall(0.0, 0.0, height - fall_distance))
    {
        return delta;
    }
    let step_towards_zero = |v: f64| {
        if v.abs() <= STEP {
            0.0
        } else {
            v - v.signum() * STEP
        }
    };
    while delta.x != 0.0 && can_fall(delta.x, 0.0, height) {
        delta.x = step_towards_zero(delta.x);
    }
    while delta.z != 0.0 && can_fall(0.0, delta.z, height) {
        delta.z = step_towards_zero(delta.z);
    }
    while delta.x != 0.0 && delta.z != 0.0 && can_fall(delta.x, delta.z, height) {
        delta.x = step_towards_zero(delta.x);
        delta.z = step_towards_zero(delta.z);
    }
    delta
}

pub fn collide(
    world: &World,
    entities: &Entities,
    bounds: BoundingBox,
    delta: Vector3<f64>,
    grounded: bool,
) -> Vector3<f64> {
    if delta == Vector3::default() {
        return delta;
    }
    let mut area = bounds.stretch(delta);
    area.max.y += f64::from(STEP_HEIGHT);
    let colliders = entities.solid_shapes(area.expand_all(SHAPE_EPSILON));
    let mut shapes = world.shapes(bounds.stretch(delta));
    shapes.extend_from_slice(&colliders);
    let clipped = clip(bounds, delta, &shapes);
    let landed = delta.y < 0.0 && clipped.y != delta.y;
    if !(landed || grounded) || (clipped.x == delta.x && clipped.z == delta.z) {
        return clipped;
    }

    let floor = if landed { clipped.y } else { 0.0 };
    let base = bounds.shift(Vector3::new(0.0, floor, 0.0));
    let mut scan = base.expand_towards(delta.x, f64::from(STEP_HEIGHT), delta.z);
    if !landed {
        scan.min.y -= f64::from(1.0e-5f32);
    }
    let mut shapes = world.shapes(scan);
    shapes.extend(colliders);
    let mut heights = shapes
        .iter()
        .flat_map(|shape| [shape.min.y, shape.max.y])
        .map(|y| (y - base.min.y) as f32)
        .filter(|&height| (0.0..=STEP_HEIGHT).contains(&height) && height != clipped.y as f32)
        .collect::<Vec<_>>();
    heights.sort_unstable_by(f32::total_cmp);
    heights.dedup();
    for height in heights {
        let mut step = clip(
            base,
            Vector3::new(delta.x, f64::from(height), delta.z),
            &shapes,
        );
        if step.horizontal_length_squared() > clipped.horizontal_length_squared() {
            step.y += floor;
            return step;
        }
    }
    clipped
}

fn clip(mut bounds: BoundingBox, delta: Vector3<f64>, shapes: &[BoundingBox]) -> Vector3<f64> {
    let axes = if delta.x.abs() < delta.z.abs() {
        [Axis::Y, Axis::Z, Axis::X]
    } else {
        Axis::all()
    };
    let mut result = Vector3::default();
    for axis in axes {
        let mut distance = delta.get_axis(axis);
        for shape in shapes {
            if distance.abs() < SHAPE_EPSILON {
                distance = 0.0;
                break;
            }
            if !Axis::excluding(axis).into_iter().all(|side| {
                shape.max.get_axis(side) > bounds.min.get_axis(side) + SHAPE_EPSILON
                    && shape.min.get_axis(side) < bounds.max.get_axis(side) - SHAPE_EPSILON
            }) {
                continue;
            }
            if distance > 0.0 {
                let gap = shape.min.get_axis(axis) - bounds.max.get_axis(axis);
                if gap >= -SHAPE_EPSILON {
                    distance = distance.min(gap);
                }
            } else {
                let gap = shape.max.get_axis(axis) - bounds.min.get_axis(axis);
                if gap <= SHAPE_EPSILON {
                    distance = distance.max(gap);
                }
            }
        }
        result.set_axis(axis, distance);
        bounds
            .min
            .set_axis(axis, bounds.min.get_axis(axis) + distance);
        bounds
            .max
            .set_axis(axis, bounds.max.get_axis(axis) + distance);
    }
    result
}
