//! Create 6.0.10's BezierConnection.Runtime geometry. The input axes point
//! inward from each endpoint (the final curve tangent is opposite the second).

use super::math::Vec3;
use std::f64::consts::PI;

// Pending: M3 train navigation will consume the cached Bezier fields
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Debug)]
pub(super) struct BezierGeometry {
    starts: [Vec3; 2],
    axes: [Vec3; 2],
    pub primary: bool,
    pub girder: bool,
    controls: [Vec3; 2],
    radius: f64,
    handle_length: f64,
    length: f64,
    step_lut: Vec<f32>,
}

// The carriage runtime will use the cached handles and traversal rate.
#[cfg_attr(not(test), allow(dead_code))]
impl BezierGeometry {
    pub fn new(starts: [Vec3; 2], axes: [Vec3; 2], primary: bool, girder: bool) -> Option<Self> {
        let [start, end] = starts;
        let [axis1, axis2] = axes.map(Vec3::normalized);
        if axis1.dot(axis1) == 0.0 || axis2.dot(axis2) == 0.0 {
            return None;
        }

        let [radius, handle_length] = determine_handles(start, end, axis1, axis2);
        let controls = [
            start.plus(axis1.scale(handle_length)),
            end.plus(axis2.scale(handle_length)),
        ];
        let mut length = 0.0;
        let mut previous = start;
        for i in 0..=16 {
            let t = f64::from(i as f32 / 16.0);
            let current = point(starts, controls, t);
            length += current.distance(previous);
            previous = current;
        }

        // Create caps endpoint separation at 128 blocks even with its placement config raised.
        let segments = (length * 2.0) as usize;
        if !length.is_finite() || segments == 0 || segments > 512 {
            return None;
        }

        let mut step_lut = vec![0.0; segments + 1];
        step_lut[0] = 1.0;
        let mut accumulated = 0.0f32;
        previous = start;
        for (i, slot) in step_lut.iter_mut().enumerate() {
            let t = i as f32 / segments as f32;
            let current = point(starts, controls, f64::from(t));
            if i != 0 {
                accumulated += (current.distance(previous) / length) as f32;
                *slot = t / accumulated;
            }
            previous = current;
        }

        Some(Self {
            starts,
            axes: [axis1, axis2],
            primary,
            girder,
            controls,
            radius,
            handle_length,
            length,
            step_lut,
        })
    }

    pub fn secondary(&self) -> Self {
        Self::new(
            [self.starts[1], self.starts[0]],
            [self.axes[1], self.axes[0]],
            !self.primary,
            self.girder,
        )
        .expect("reversing valid Bezier geometry remains valid")
    }

    pub fn position(&self, t: f64) -> Vec3 {
        point(self.starts, self.controls, t.clamp(0.0, 1.0))
    }

    pub fn length(&self) -> f64 {
        self.length
    }

    pub fn radius(&self) -> f64 {
        self.radius
    }

    pub fn handle_length(&self) -> f64 {
        self.handle_length
    }

    pub fn segment_count(&self) -> usize {
        self.step_lut.len() - 1
    }

    /// This approximates an equal-distance segment using Create's lookup table.
    pub fn segment_t(&self, index: usize) -> Option<f64> {
        let segment_count = self.segment_count();
        if index > segment_count {
            return None;
        }
        if index == segment_count {
            return Some(1.0);
        }
        Some(f64::from(
            index as f32 * self.step_lut[index] / segment_count as f32,
        ))
    }

    /// Create's TrackEdge divides distance by edge length before calling this.
    pub fn increment_t(&self, current_t: f64, normalized_distance: f64) -> f64 {
        let derivative = derivative(self.starts, self.controls, current_t);
        let rate = derivative.dot(derivative).sqrt() / self.length;
        current_t + normalized_distance / rate
    }

    pub fn track_item_cost(&self) -> usize {
        self.segment_count().div_ceil(2)
    }

    pub fn girder_item_cost(&self) -> usize {
        if self.girder {
            self.track_item_cost() * 2
        } else {
            0
        }
    }
}

/// Catnip's VecHelper.bezier takes both endpoints, then both control points.
fn point([a, b]: [Vec3; 2], [c, d]: [Vec3; 2], t: f64) -> Vec3 {
    let s = 1.0 - t;
    a.scale(s * s * s)
        .plus(c.scale(3.0 * s * s * t))
        .plus(d.scale(3.0 * s * t * t))
        .plus(b.scale(t * t * t))
}

// Pending: M3 carriage traversal integrates Bezier derivatives
#[cfg_attr(not(test), allow(dead_code))]
fn derivative([a, b]: [Vec3; 2], [c, d]: [Vec3; 2], t: f64) -> Vec3 {
    let s = 1.0 - t;
    c.minus(a)
        .scale(3.0 * s * s)
        .plus(d.minus(c).scale(6.0 * s * t))
        .plus(b.minus(d).scale(3.0 * t * t))
}

/// Solve start + tangent*t = end + other*u in the X/Z plane, as VecHelper.intersect(..., Axis.Y).
fn intersect(start: Vec3, end: Vec3, tangent: Vec3, other: Vec3) -> Option<[f64; 2]> {
    let delta = end.minus(start);
    let det = other.x * tangent.z - tangent.x * other.z;
    if det.abs() < 1.0e-8 {
        return None;
    }
    Some([
        (delta.z * other.x - delta.x * other.z) / det,
        (tangent.x * delta.z - tangent.z * delta.x) / det,
    ])
}

fn determine_handles(start: Vec3, end: Vec3, axis1: Vec3, axis2: Vec3) -> [f64; 2] {
    let up = Vec3 {
        x: 0.0,
        y: 1.0,
        z: 0.0,
    };
    let cross1 = axis1.cross(up);
    let cross2 = axis2.cross(up);
    let circle = 2.0 * PI;
    let a1 = (-axis2.z).atan2(-axis2.x);
    let a2 = axis1.z.atan2(axis1.x);
    let mut angle = (a1 - a2 + circle) % circle;
    if (circle - angle).abs() < angle.abs() {
        angle = circle - angle;
    }

    if angle.abs() < 1.0e-5 {
        if let Some([t, u]) = intersect(start, end, axis1, cross2) {
            let min = t.abs().min(u.abs());
            let max = t.abs().max(u.abs());
            if min > 1.2 && max / min > 1.0 && max / min < 3.0 {
                return [0.0, max - min];
            }
        }
        return [0.0, start.distance(end) / 3.0];
    }

    let n = circle / angle;
    let factor = 4.0 / 3.0 * (PI / (2.0 * n)).tan();
    let Some([_, u]) = intersect(start, end, cross1, cross2) else {
        return [0.0, start.distance(end) / 3.0];
    };
    let radius = u.abs();
    let mut handle_length = radius * factor;
    if handle_length.abs() < 1.0e-5 {
        handle_length = 1.0;
    }
    [radius, handle_length]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xyz(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3 { x, y, z }
    }

    fn quarter(girder: bool) -> BezierGeometry {
        BezierGeometry::new(
            [xyz(0.0, 64.0, 0.0), xyz(8.0, 64.0, 8.0)],
            [xyz(1.0, 0.0, 0.0), xyz(0.0, 0.0, -1.0)],
            true,
            girder,
        )
        .expect("a valid quarter curve")
    }

    #[test]
    fn curve_handle_matches_java_quarter_turn_formula() {
        let curve = quarter(false);
        assert!((curve.radius() - 8.0).abs() < 1.0e-9);
        let expected = 8.0 * 4.0 / 3.0 * (PI / 8.0).tan();
        assert!((curve.handle_length() - expected).abs() < 1.0e-9);
        assert!(curve.primary);
        assert!(!curve.girder);
        assert!((curve.length() - PI * 4.0).abs() < 0.03);
        assert_eq!(curve.position(0.0), xyz(0.0, 64.0, 0.0));
        assert_eq!(curve.position(1.0), xyz(8.0, 64.0, 8.0));
        let middle = curve.position(0.5);
        assert!((middle.x - 5.656854249).abs() < 0.05);
        assert!((middle.z - 2.343145751).abs() < 0.05);
    }

    #[test]
    fn reverse_has_same_positions_and_opposite_primary() {
        let original = quarter(true);
        let reverse = original.secondary();
        assert!(!reverse.primary);
        assert!(reverse.girder);
        assert_eq!(reverse.position(0.0), original.position(1.0));
        assert_eq!(reverse.position(1.0), original.position(0.0));
        assert!((reverse.length() - original.length()).abs() < 1.0e-7);
        for i in 0..=16 {
            let t = i as f64 / 16.0;
            let left = original.position(t);
            let right = reverse.position(1.0 - t);
            assert!(left.distance(right) < 1.0e-7);
        }
    }

    #[test]
    fn segment_count_t_and_item_cost_follow_create_runtime() {
        let curve = quarter(true);
        assert_eq!(curve.segment_count(), (curve.length() * 2.0) as usize);
        assert_eq!(curve.segment_t(0), Some(0.0));
        assert_eq!(curve.segment_t(curve.segment_count()), Some(1.0));
        assert_eq!(curve.segment_t(curve.segment_count() + 1), None);
        for i in 1..=curve.segment_count() {
            assert!(curve.segment_t(i).unwrap() > curve.segment_t(i - 1).unwrap());
        }
        let expected_tracks = curve.segment_count().div_ceil(2);
        assert_eq!(curve.track_item_cost(), expected_tracks);
        assert_eq!(curve.girder_item_cost(), expected_tracks * 2);
        assert_eq!(quarter(false).girder_item_cost(), 0);
    }

    #[test]
    fn straight_control_handles_are_one_third_of_span() {
        let straight = BezierGeometry::new(
            [xyz(0.0, 70.0, 0.0), xyz(8.0, 70.0, 0.0)],
            [xyz(1.0, 0.0, 0.0), xyz(-1.0, 0.0, 0.0)],
            true,
            false,
        )
        .unwrap();
        assert_eq!(straight.radius(), 0.0);
        assert!((straight.handle_length() - 8.0 / 3.0).abs() < 1.0e-9);
        assert!((straight.length() - 8.0).abs() < 1.0e-7);
        assert_eq!(straight.segment_count(), 16);
        assert!((straight.position(0.25).x - 2.0).abs() < 1.0e-7);
        assert!((straight.increment_t(0.25, 0.1) - 0.35).abs() < 1.0e-7);
    }

    #[test]
    fn invalid_curves_fail_closed() {
        assert!(
            BezierGeometry::new(
                [xyz(0.0, 0.0, 0.0), xyz(0.0, 0.0, 0.0)],
                [xyz(1.0, 0.0, 0.0), xyz(-1.0, 0.0, 0.0)],
                true,
                false,
            )
            .is_none()
        );
        assert!(
            BezierGeometry::new(
                [xyz(0.0, 0.0, 0.0), xyz(8.0, 0.0, 0.0)],
                [xyz(0.0, 0.0, 0.0), xyz(-1.0, 0.0, 0.0)],
                true,
                false,
            )
            .is_none()
        );
    }
}
