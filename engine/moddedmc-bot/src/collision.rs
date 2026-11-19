use pumpkin_util::math::{
    boundingbox::BoundingBox,
    vector3::{Axis, Vector3},
};

use crate::terrain::World;

const STEP_HEIGHT: f32 = 0.6;
const SHAPE_EPSILON: f64 = 1.0e-7;

pub fn collide(
    world: &World,
    bounds: BoundingBox,
    delta: Vector3<f64>,
    grounded: bool,
) -> Vector3<f64> {
    if delta == Vector3::default() {
        return delta;
    }
    let shapes = world.shapes(bounds.stretch(delta));
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
    let shapes = world.shapes(scan);
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
