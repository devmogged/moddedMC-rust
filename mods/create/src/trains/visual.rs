//! Draw saved Bezier curves on stock clients using the existing track carrier.
use super::{curve::CurveRecord, math::Vec3, runtime, track::TrackShape};
use moddedmc_api::{
    Pos,
    block_displays::BlockDisplay,
    content::{Content, World},
};

// The straight track model spans -13.95..29.95 along its long axis.
const MODEL_LENGTH: f64 = 43.9 / 16.0;

pub(super) fn show(
    world: World<'_>,
    content: Content,
    anchor: Pos,
    curves: &[CurveRecord],
    ids: &mut Vec<i32>,
) {
    let Some(&displays) = crate::BLOCK_DISPLAYS.get() else {
        return;
    };
    let Some(track) = runtime::rail_state(content, TrackShape::ZOrtho, false) else {
        return;
    };
    let origin = Vec3 {
        x: f64::from(anchor.x) + 0.5,
        y: f64::from(anchor.y),
        z: f64::from(anchor.z) + 0.5,
    };

    for curve in curves.iter().filter(|curve| curve.primary) {
        let Some(geometry) = curve.geometry() else {
            continue;
        };
        let segments = geometry.segment_count();
        for i in (0..segments).step_by(2) {
            let (Some(a), Some(b)) = (
                geometry.segment_t(i),
                geometry.segment_t((i + 2).min(segments)),
            ) else {
                continue;
            };
            let (a, b) = (geometry.position(a), geometry.position(b));
            let delta = b.minus(a);
            let length = delta.dot(delta).sqrt();
            if length < 1.0e-6 {
                continue;
            }
            let forward = delta.scale(length.recip());
            let yaw = forward.x.atan2(forward.z);
            let pitch = -forward.y.asin();
            let (sy, cy) = (yaw * 0.5).sin_cos();
            let (sp, cp) = (pitch * 0.5).sin_cos();
            let right = Vec3 {
                x: yaw.cos(),
                y: 0.0,
                z: -yaw.sin(),
            };
            let scale = length / MODEL_LENGTH;
            let translation = a
                .plus(b)
                .scale(0.5)
                .minus(origin)
                .minus(right.scale(0.5))
                .minus(forward.scale(scale * 0.5));
            let display = BlockDisplay {
                // All segments share the owning chunk and unload with the primary endpoint.
                position: [origin.x, origin.y, origin.z],
                translation: [
                    translation.x as f32,
                    translation.y as f32,
                    translation.z as f32,
                ],
                rotation: [
                    (cy * sp) as f32,
                    (sy * cp) as f32,
                    (-sy * sp) as f32,
                    (cy * cp) as f32,
                ],
                scale: [1.0, 1.0, scale as f32],
                transform_interpolation: 0,
                position_interpolation: 0,
                brightness: -1,
            };
            if let Some(id) = displays.spawn(world, track, &display) {
                ids.push(id);
            }
        }
    }
}

pub(super) fn clear(world: World<'_>, ids: Vec<i32>) {
    if let Some(&displays) = crate::BLOCK_DISPLAYS.get() {
        for id in ids {
            displays.remove(world, id);
        }
    }
}
