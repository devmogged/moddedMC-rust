//! Geometry for the turn branches of TrackPlacement.tryConnect.
use super::{curve::CurveRecord, item::ConnectingFrom, math::Vec3};

pub(super) struct CurvePlacement {
    pub curve: CurveRecord,
    pub extents: [i32; 2],
    pub axes: [Vec3; 2],
}

fn intersect(a: Vec3, b: Vec3, u: Vec3, v: Vec3, plane: usize) -> Option<[f64; 2]> {
    let project = |p: Vec3| match plane {
        0 => [p.z, p.y],
        1 => [p.x, p.z],
        _ => [p.x, p.y],
    };
    let cross = |[x, y]: [f64; 2], [a, b]: [f64; 2]| x * b - y * a;
    let (delta, u, v) = (project(b.minus(a)), project(u), project(v));
    let divisor = cross(u, v);
    (divisor.abs() >= 1.0e-7).then(|| [cross(delta, v) / divisor, cross(delta, u) / divisor])
}

pub(super) fn curve_placement(
    first: ConnectingFrom,
    second: ConnectingFrom,
) -> Option<CurvePlacement> {
    let positions = [first.pos, second.pos];
    let vec = |[x, y, z]: [f64; 3]| Vec3 { x, y, z };
    let delta = vec(std::array::from_fn(|i| {
        f64::from(second.pos[i]) - f64::from(first.pos[i])
    }));
    let distance = delta.dot(delta).sqrt();
    if distance == 0.0 || distance > 32.0 {
        return None;
    }

    let [mut a, mut b] = [vec(first.axis), vec(second.axis)];
    let [mut start, mut end] = [vec(first.end), vec(second.end)];
    let normals = [vec(first.normal), vec(second.normal)];
    if a.dot(end.minus(start)) < 0.0 {
        start = start.minus(a);
        a = a.scale(-1.0);
    }
    let u = a.normalized();
    let mut v = b.normalized();
    let horizontal = intersect(start, end, u, v, 1);
    if horizontal.map_or_else(|| u.dot(v) > 0.0, |[t, s]| t < 0.0 || s < 0.0) {
        end = end.minus(b);
        b = b.scale(-1.0);
        v = v.scale(-1.0);
    }

    let ascent = end.y - start.y;
    let rise = ascent.abs();
    let lengths = [a.dot(a).sqrt(), b.dot(b).sqrt()];
    let mut extents;
    let slope = normals[0] != normals[1];
    let equal = |a: f64, b: f64| (a - b).abs() < 1.0e-5;
    let centered = |difference: f64| {
        let n = difference as i32;
        [n / 2 + n % 2, n / 2]
    };

    if horizontal.is_none() {
        let perpendicular = v.cross(Vec3 {
            x: 0.0,
            y: 1.0,
            z: 0.0,
        });
        let [along, across] = intersect(start, end, u, perpendicular, 1)?;
        if !equal(across, 0.0) {
            if along < 0.0 || slope || !equal(ascent, 0.0) || u.y != 0.0 {
                return None;
            }
            let width = across.abs();
            let target = if width <= 1.0 { 3.0 } else { width * 2.0 };
            if along < target {
                return None;
            }
            extents = centered((along - target) / lengths[0]);
        } else if slope {
            if equal(normals[0].dot(normals[1]), 0.0)
                || ((a.y < 0.0 || b.y > 0.0) && ascent > 0.0)
                || ((a.y > 0.0 || b.y < 0.0) && ascent < 0.0)
            {
                return None;
            }
            let plane = if equal(a.x, 0.0) { 0 } else { 2 };
            let [t, s] = intersect(start, end, u, v, plane)?;
            let [t, s] = [t / lengths[0], s / lengths[1]];
            let turn = t.min(s);
            if turn < 2.0 {
                return None;
            }
            extents = [
                (t - s).max(0.0).round() as i32,
                (s - t).max(0.0).round() as i32,
            ];
            extents = extents.map(|extent| extent + (turn - 2.0) as i32);
        } else {
            // Collinear, level tracks and continuous ascending tracks use the straight path.
            if equal(ascent, 0.0)
                || (a.y != 0.0 && (equal(rise + 1.0, distance / lengths[0]) || a.y == -b.y))
            {
                return None;
            }
            let extent = ((distance + 1.0) / lengths[0]).round();
            let minimum = (rise * if rise < 4.0 { 4.0 } else { 3.0 }).max(6.0) / lengths[0];
            if extent < minimum {
                return None;
            }
            extents = centered(extent - minimum);
        }
    } else {
        if slope {
            return None;
        }
        let angle = (v.z.atan2(v.x) - u.z.atan2(u.x)).to_degrees().abs() as f32;
        if !(60.0..=300.0).contains(&angle) {
            return None;
        }
        let [t, s] = intersect(start, end, u, v, 1)?;
        if t < 0.0 || s < 0.0 {
            return None;
        }
        let ninety = (angle + 0.25) % 90.0 < 1.0;
        let minimum = if ninety {
            7.0 + (rise - 3.0).max(0.0) * 2.0
        } else {
            3.25 + (rise - 1.5).max(0.0) * 1.5
        };
        let extra = t.min(s) - 0.1 - minimum;
        if extra < 0.0 {
            return None;
        }
        extents = [
            (((t - s).max(0.0) / lengths[0]) as f32 + (extra / lengths[0]) as f32).floor() as i32,
            (((s - t).max(0.0) / lengths[1]) as f32 + (extra / lengths[1]) as f32).floor() as i32,
        ];
    }

    let axes = [a, b];
    let offsets = [
        a.scale(f64::from(extents[0])),
        b.scale(f64::from(extents[1])),
    ];
    let curve = CurveRecord {
        block_positions: [
            offset_position(positions[0], offsets[0])?,
            offset_position(positions[1], offsets[1])?,
        ],
        starts: [start.plus(offsets[0]), end.plus(offsets[1])],
        axes: [u, v],
        normals,
        material: "create:andesite".to_owned(),
        primary: true,
        girder: false,
        smoothing: None,
    };
    curve.geometry()?;
    Some(CurvePlacement {
        curve,
        extents,
        axes,
    })
}

pub(super) fn offset_position(pos: [i32; 3], offset: Vec3) -> Option<[i32; 3]> {
    Some([
        pos[0].checked_add(offset.x.floor() as i32)?,
        pos[1].checked_add(offset.y.floor() as i32)?,
        pos[2].checked_add(offset.z.floor() as i32)?,
    ])
}
