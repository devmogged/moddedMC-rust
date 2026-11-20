//! TrackShape and TrackBlock placement/overlay rules from Create 6.0.10.

use super::graph::Vec3;

const NORTH: Vec3 = vec3(0.0, 0.0, -1.0);
const SOUTH: Vec3 = vec3(0.0, 0.0, 1.0);
const EAST: Vec3 = vec3(1.0, 0.0, 0.0);
const WEST: Vec3 = vec3(-1.0, 0.0, 0.0);

const fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 { x, y, z }
}

// Enum order mirrors TrackShape.java; the static table avoids per-tick heap allocations.
static TRACK_AXES: [&[Vec3]; 19] = [
    &[vec3(0.0, 0.0, 0.0)],
    &[SOUTH],
    &[EAST],
    &[vec3(1.0, 0.0, 1.0)],
    &[vec3(-1.0, 0.0, 1.0)],
    &[vec3(0.0, 1.0, -1.0)],
    &[vec3(0.0, 1.0, 1.0)],
    &[vec3(1.0, 1.0, 0.0)],
    &[vec3(-1.0, 1.0, 0.0)],
    &[NORTH],
    &[SOUTH],
    &[EAST],
    &[WEST],
    &[SOUTH, EAST],
    &[vec3(1.0, 0.0, 1.0), vec3(-1.0, 0.0, 1.0)],
    &[EAST, vec3(1.0, 0.0, 1.0)],
    &[SOUTH, vec3(1.0, 0.0, 1.0)],
    &[EAST, vec3(-1.0, 0.0, 1.0)],
    &[SOUTH, vec3(-1.0, 0.0, 1.0)],
];

/// The enum mirrors TrackShape.java's serialized blockstate values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(usize)]
pub(super) enum TrackShape {
    None,
    ZOrtho,
    XOrtho,
    PositiveDiagonal,
    NegativeDiagonal,
    AscendingNorth,
    AscendingSouth,
    AscendingEast,
    AscendingWest,
    TeleportNorth,
    TeleportSouth,
    TeleportEast,
    TeleportWest,
    CrossOrtho,
    CrossDiagonal,
    CrossPositiveX,
    CrossPositiveZ,
    CrossNegativeX,
    CrossNegativeZ,
}

impl TrackShape {
    pub fn serialized(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ZOrtho => "zo",
            Self::XOrtho => "xo",
            Self::PositiveDiagonal => "pd",
            Self::NegativeDiagonal => "nd",
            Self::AscendingNorth => "an",
            Self::AscendingSouth => "as",
            Self::AscendingEast => "ae",
            Self::AscendingWest => "aw",
            Self::TeleportNorth => "tn",
            Self::TeleportSouth => "ts",
            Self::TeleportEast => "te",
            Self::TeleportWest => "tw",
            Self::CrossOrtho => "cr_o",
            Self::CrossDiagonal => "cr_d",
            Self::CrossPositiveX => "cr_pdx",
            Self::CrossPositiveZ => "cr_pdz",
            Self::CrossNegativeX => "cr_ndx",
            Self::CrossNegativeZ => "cr_ndz",
        }
    }

    pub fn axes(self) -> &'static [Vec3] {
        TRACK_AXES[self as usize]
    }

    pub fn is_junction(self) -> bool {
        self.axes().len() > 1
    }

    pub fn is_portal(self) -> bool {
        matches!(
            self,
            Self::TeleportNorth | Self::TeleportSouth | Self::TeleportEast | Self::TeleportWest
        )
    }

    pub fn normal(self) -> Vec3 {
        match self {
            Self::AscendingNorth => vec3(0.0, 1.0, 1.0).normalized(),
            Self::AscendingSouth => vec3(0.0, 1.0, -1.0).normalized(),
            Self::AscendingEast => vec3(-1.0, 1.0, 0.0).normalized(),
            Self::AscendingWest => vec3(1.0, 1.0, 0.0).normalized(),
            _ => vec3(0.0, 1.0, 0.0),
        }
    }

    /// Create TrackShape.zMirror table, including cross-shaped overlays.
    pub fn mirror_z(self) -> Self {
        use TrackShape as T;
        match self {
            T::PositiveDiagonal => T::NegativeDiagonal,
            T::NegativeDiagonal => T::PositiveDiagonal,
            T::AscendingNorth => T::AscendingSouth,
            T::AscendingSouth => T::AscendingNorth,
            T::CrossPositiveX => T::CrossNegativeX,
            T::CrossNegativeX => T::CrossPositiveX,
            T::CrossPositiveZ => T::CrossNegativeZ,
            T::CrossNegativeZ => T::CrossPositiveZ,
            other => other,
        }
    }

    /// Create TrackShape.xMirror table.
    pub fn mirror_x(self) -> Self {
        use TrackShape as T;
        match self {
            T::PositiveDiagonal => T::NegativeDiagonal,
            T::NegativeDiagonal => T::PositiveDiagonal,
            T::AscendingEast => T::AscendingWest,
            T::AscendingWest => T::AscendingEast,
            T::CrossPositiveX => T::CrossNegativeX,
            T::CrossNegativeX => T::CrossPositiveX,
            T::CrossPositiveZ => T::CrossNegativeZ,
            T::CrossNegativeZ => T::CrossPositiveZ,
            other => other,
        }
    }

    /// One 90-degree clockwise rotation, using Create TrackShape.clockwise.
    pub fn rotate_clockwise(self) -> Self {
        use TrackShape as T;
        match self {
            T::PositiveDiagonal => T::NegativeDiagonal,
            T::NegativeDiagonal => T::PositiveDiagonal,
            T::XOrtho => T::ZOrtho,
            T::ZOrtho => T::XOrtho,
            T::AscendingEast => T::AscendingSouth,
            T::AscendingSouth => T::AscendingWest,
            T::AscendingWest => T::AscendingNorth,
            T::AscendingNorth => T::AscendingEast,
            T::CrossPositiveX => T::CrossNegativeZ,
            T::CrossNegativeX => T::CrossPositiveZ,
            T::CrossPositiveZ => T::CrossNegativeX,
            T::CrossNegativeZ => T::CrossPositiveX,
            other => other,
        }
    }
}

/// TrackBlock.getStateForPlacement, after the world-facing block placement
/// has provided upper-face support at the negative/positive axis neighbors.
pub(super) fn select_placement_shape(
    player_look: Vec3,
    player_yaw: f32,
    elevated_negative: bool,
    elevated_positive: bool,
) -> TrackShape {
    let mut horizontal = vec3(player_look.x, 0.0, player_look.z);
    if horizontal.dot(horizontal).sqrt() < 1.0e-5 {
        let yaw = f64::from(player_yaw).to_radians();
        horizontal = vec3(-yaw.sin(), 0.0, yaw.cos());
    }
    horizontal = horizontal.normalized();
    let mut best = TrackShape::ZOrtho;
    let mut best_distance = f64::from(f32::MAX);

    // Mirrors the order of TrackShape.values(). Ties replace the prior shape.
    for shape in [
        TrackShape::None,
        TrackShape::ZOrtho,
        TrackShape::XOrtho,
        TrackShape::PositiveDiagonal,
        TrackShape::NegativeDiagonal,
        TrackShape::AscendingNorth,
        TrackShape::AscendingSouth,
        TrackShape::AscendingEast,
        TrackShape::AscendingWest,
    ] {
        let axis = shape.axes()[0];
        let distance = axis.minus(horizontal).dot(axis.minus(horizontal)).min(
            axis.normalized()
                .scale(-1.0)
                .minus(horizontal)
                .dot(axis.normalized().scale(-1.0).minus(horizontal)),
        );
        if distance > best_distance {
            continue;
        }
        best_distance = distance;
        best = shape;
    }

    if (best.axes()[0].dot(best.axes()[0]) - 1.0).abs() < 1.0e-9 {
        if elevated_negative {
            best = match best {
                TrackShape::XOrtho => TrackShape::AscendingWest,
                TrackShape::ZOrtho => TrackShape::AscendingNorth,
                _ => best,
            };
        }
        if elevated_positive {
            best = match best {
                TrackShape::XOrtho => TrackShape::AscendingEast,
                TrackShape::ZOrtho => TrackShape::AscendingSouth,
                _ => best,
            };
        }
    }
    best
}

/// TrackBlock.overlay combines independently oriented rails at a junction.
pub(super) fn overlay(existing: TrackShape, placed: TrackShape) -> TrackShape {
    use TrackShape as T;
    match (existing, placed) {
        (T::XOrtho, T::ZOrtho) | (T::ZOrtho, T::XOrtho) => T::CrossOrtho,
        (T::PositiveDiagonal, T::NegativeDiagonal) | (T::NegativeDiagonal, T::PositiveDiagonal) => {
            T::CrossDiagonal
        }
        (T::XOrtho, T::PositiveDiagonal) | (T::PositiveDiagonal, T::XOrtho) => T::CrossPositiveX,
        (T::ZOrtho, T::PositiveDiagonal) | (T::PositiveDiagonal, T::ZOrtho) => T::CrossPositiveZ,
        (T::XOrtho, T::NegativeDiagonal) | (T::NegativeDiagonal, T::XOrtho) => T::CrossNegativeX,
        (T::ZOrtho, T::NegativeDiagonal) | (T::NegativeDiagonal, T::ZOrtho) => T::CrossNegativeZ,
        _ => existing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_names_and_axes_match_create_blockstates() {
        let all = [
            (TrackShape::None, "none"),
            (TrackShape::ZOrtho, "zo"),
            (TrackShape::XOrtho, "xo"),
            (TrackShape::PositiveDiagonal, "pd"),
            (TrackShape::NegativeDiagonal, "nd"),
            (TrackShape::AscendingNorth, "an"),
            (TrackShape::AscendingSouth, "as"),
            (TrackShape::AscendingEast, "ae"),
            (TrackShape::AscendingWest, "aw"),
            (TrackShape::TeleportNorth, "tn"),
            (TrackShape::TeleportSouth, "ts"),
            (TrackShape::TeleportEast, "te"),
            (TrackShape::TeleportWest, "tw"),
            (TrackShape::CrossOrtho, "cr_o"),
            (TrackShape::CrossDiagonal, "cr_d"),
            (TrackShape::CrossPositiveX, "cr_pdx"),
            (TrackShape::CrossPositiveZ, "cr_pdz"),
            (TrackShape::CrossNegativeX, "cr_ndx"),
            (TrackShape::CrossNegativeZ, "cr_ndz"),
        ];
        assert_eq!(all.len(), 19);
        for (shape, serialized) in all {
            assert_eq!(shape.serialized(), serialized);
            assert!(!shape.axes().is_empty());
        }
        assert_eq!(TrackShape::PositiveDiagonal.axes(), &[vec3(1.0, 0.0, 1.0)]);
        assert_eq!(TrackShape::NegativeDiagonal.axes(), &[vec3(-1.0, 0.0, 1.0)]);
        assert!(TrackShape::CrossDiagonal.is_junction());
        assert!(TrackShape::TeleportWest.is_portal());
        assert!(!TrackShape::AscendingWest.is_portal());
        assert!(!TrackShape::PositiveDiagonal.is_junction());
    }

    #[test]
    fn horizontal_look_selects_straight_and_diagonal_rails() {
        assert_eq!(
            select_placement_shape(EAST, 0.0, false, false),
            TrackShape::XOrtho
        );
        assert_eq!(
            select_placement_shape(WEST, 0.0, false, false),
            TrackShape::XOrtho
        );
        assert_eq!(
            select_placement_shape(NORTH, 0.0, false, false),
            TrackShape::ZOrtho
        );
        assert_eq!(
            select_placement_shape(vec3(1.0, 0.0, 1.0), 0.0, false, false),
            TrackShape::PositiveDiagonal
        );
        assert_eq!(
            select_placement_shape(vec3(-1.0, 0.0, 1.0), 0.0, false, false),
            TrackShape::NegativeDiagonal
        );
    }

    #[test]
    fn vertical_look_uses_yaw_and_neighbor_support_creates_slope() {
        assert_eq!(
            select_placement_shape(vec3(0.0, -1.0, 0.0), 0.0, false, false),
            TrackShape::ZOrtho
        );
        assert_eq!(
            select_placement_shape(vec3(0.0, -1.0, 0.0), 90.0, false, false),
            TrackShape::XOrtho
        );
        assert_eq!(
            select_placement_shape(EAST, 0.0, true, false),
            TrackShape::AscendingWest
        );
        assert_eq!(
            select_placement_shape(EAST, 0.0, false, true),
            TrackShape::AscendingEast
        );
        assert_eq!(
            select_placement_shape(SOUTH, 0.0, true, false),
            TrackShape::AscendingNorth
        );
        assert_eq!(
            select_placement_shape(SOUTH, 0.0, false, true),
            TrackShape::AscendingSouth
        );
        // Iterate.trueAndFalse examines the negative end first.
        assert_eq!(
            select_placement_shape(EAST, 0.0, true, true),
            TrackShape::AscendingWest
        );
    }

    #[test]
    fn overlay_pairs_match_the_six_java_junction_rules_both_ways() {
        let cases = [
            (
                TrackShape::XOrtho,
                TrackShape::ZOrtho,
                TrackShape::CrossOrtho,
            ),
            (
                TrackShape::PositiveDiagonal,
                TrackShape::NegativeDiagonal,
                TrackShape::CrossDiagonal,
            ),
            (
                TrackShape::XOrtho,
                TrackShape::PositiveDiagonal,
                TrackShape::CrossPositiveX,
            ),
            (
                TrackShape::ZOrtho,
                TrackShape::PositiveDiagonal,
                TrackShape::CrossPositiveZ,
            ),
            (
                TrackShape::XOrtho,
                TrackShape::NegativeDiagonal,
                TrackShape::CrossNegativeX,
            ),
            (
                TrackShape::ZOrtho,
                TrackShape::NegativeDiagonal,
                TrackShape::CrossNegativeZ,
            ),
        ];
        for (a, b, merged) in cases {
            assert_eq!(overlay(a, b), merged);
            assert_eq!(overlay(b, a), merged);
        }
        assert_eq!(
            overlay(TrackShape::XOrtho, TrackShape::AscendingSouth),
            TrackShape::XOrtho
        );
        assert_eq!(
            overlay(TrackShape::CrossOrtho, TrackShape::XOrtho),
            TrackShape::CrossOrtho
        );
    }

    #[test]
    fn ascending_shapes_have_sloped_up_normals() {
        let north = TrackShape::AscendingNorth.normal();
        assert_eq!(north.x, 0.0);
        assert!((north.y - std::f64::consts::FRAC_1_SQRT_2).abs() < 1.0e-12);
        assert_eq!(north.z, north.y);
        assert_eq!(TrackShape::ZOrtho.normal(), vec3(0.0, 1.0, 0.0));
        // TrackShape.java assigns the slope's *surface* normal, not its axis.
        for shape in [
            TrackShape::AscendingNorth,
            TrackShape::AscendingSouth,
            TrackShape::AscendingEast,
            TrackShape::AscendingWest,
        ] {
            assert!(shape.axes()[0].dot(shape.normal()).abs() < 1.0e-12);
            assert!((shape.normal().dot(shape.normal()) - 1.0).abs() < 1.0e-12);
        }
    }

    #[test]
    fn track_shape_rotation_and_mirrors_follow_java_tables() {
        use TrackShape as T;
        for shape in [
            T::None,
            T::ZOrtho,
            T::XOrtho,
            T::PositiveDiagonal,
            T::NegativeDiagonal,
            T::AscendingNorth,
            T::AscendingSouth,
            T::AscendingEast,
            T::AscendingWest,
            T::TeleportNorth,
            T::TeleportSouth,
            T::TeleportEast,
            T::TeleportWest,
            T::CrossOrtho,
            T::CrossDiagonal,
            T::CrossPositiveX,
            T::CrossPositiveZ,
            T::CrossNegativeX,
            T::CrossNegativeZ,
        ] {
            assert_eq!(shape.mirror_x().mirror_x(), shape);
            assert_eq!(shape.mirror_z().mirror_z(), shape);
            assert_eq!(
                shape
                    .rotate_clockwise()
                    .rotate_clockwise()
                    .rotate_clockwise()
                    .rotate_clockwise(),
                shape
            );
        }
        assert_eq!(T::AscendingNorth.mirror_z(), T::AscendingSouth);
        assert_eq!(T::AscendingWest.mirror_x(), T::AscendingEast);
        assert_eq!(T::PositiveDiagonal.mirror_x(), T::NegativeDiagonal);
        assert_eq!(T::PositiveDiagonal.mirror_z(), T::NegativeDiagonal);
        assert_eq!(T::AscendingEast.rotate_clockwise(), T::AscendingSouth);
        assert_eq!(T::AscendingSouth.rotate_clockwise(), T::AscendingWest);
        assert_eq!(T::CrossPositiveX.rotate_clockwise(), T::CrossNegativeZ);
        assert_eq!(T::CrossNegativeZ.rotate_clockwise(), T::CrossPositiveX);
        assert_eq!(T::CrossNegativeX.rotate_clockwise(), T::CrossPositiveZ);
        assert_eq!(T::CrossPositiveZ.rotate_clockwise(), T::CrossNegativeX);
    }
}
