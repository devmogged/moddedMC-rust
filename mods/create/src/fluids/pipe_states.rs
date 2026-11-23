//! Connection-state logic from Create 6.0.10 FluidPipeBlock.updateBlockState
//! and FluidPropagator.getStraightPipeAxis.

/// Vanilla direction order: down, up, north, south, west, east.
pub type OpenFaces = [bool; 6];

/// Recompute the directional properties of a regular fluid pipe.
/// The caller obtains the neighboring connectivity from the loaded world.
/// A bracketed pipe never changes its connections automatically.
pub fn update_block_state(
    previous: OpenFaces,
    preferred_direction: u8,
    ignored_direction: Option<u8>,
    can_connect: OpenFaces,
    has_bracket: bool,
) -> OpenFaces {
    if has_bracket {
        return previous;
    }

    let previous_count = previous.iter().filter(|&&open| open).count();
    let mut updated = previous;
    for face in 0..6u8 {
        if ignored_direction != Some(face) {
            updated[usize::from(face)] = can_connect[usize::from(face)];
        }
    }

    let mut connected_direction = None;
    for face in 0..6u8 {
        if !updated[usize::from(face)] {
            continue;
        }
        if connected_direction.is_some() {
            return updated;
        }
        connected_direction = Some(face);
    }

    if let Some(only_face) = connected_direction {
        updated[usize::from(only_face ^ 1)] = true;
        return updated;
    }

    if previous_count == 2 {
        return previous;
    }

    if preferred_direction < 6 {
        updated[usize::from(preferred_direction)] = true;
        updated[usize::from(preferred_direction ^ 1)] = true;
    }
    updated
}

/// FluidPropagator.getStraightPipeAxis: only *exactly* two open
/// opposite faces form a straight pipe. Indices are x=0, y=1, z=2.
pub fn straight_pipe_axis(open: OpenFaces) -> Option<u8> {
    if open.iter().filter(|&&v| v).count() != 2 {
        return None;
    }
    for (axis, face) in [(0u8, 4usize), (1, 0), (2, 2)] {
        if open[face] && open[face ^ 1] {
            return Some(axis);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{straight_pipe_axis, update_block_state};

    const NONE: [bool; 6] = [false; 6];

    #[test]
    fn one_connected_neighbor_opens_the_opposite_end() {
        let mut neighbors = NONE;
        neighbors[5] = true;
        assert_eq!(
            update_block_state(NONE, 2, None, neighbors, false),
            [false, false, false, false, true, true]
        );
    }

    #[test]
    fn two_or_more_connections_keep_their_exact_shape() {
        let mut neighbors = NONE;
        neighbors[1] = true;
        neighbors[5] = true;
        assert_eq!(
            update_block_state(NONE, 2, None, neighbors, false),
            neighbors
        );

        neighbors[2] = true;
        assert_eq!(
            update_block_state(NONE, 2, None, neighbors, false),
            neighbors
        );
    }

    #[test]
    fn isolated_pipe_keeps_an_existing_pair_but_uses_preferred_when_new() {
        let prior = [false, false, false, false, true, true];
        assert_eq!(update_block_state(prior, 2, None, NONE, false), prior);
        assert_eq!(
            update_block_state(NONE, 2, None, NONE, false),
            [false, false, true, true, false, false]
        );
    }

    #[test]
    fn update_shape_ignores_the_neighbor_that_triggered_it() {
        let mut prior = NONE;
        prior[4] = true;
        let mut neighbors = NONE;
        neighbors[2] = true;
        assert_eq!(
            update_block_state(prior, 5, Some(4), neighbors, false),
            [false, false, true, false, true, false]
        );
    }

    #[test]
    fn bracketed_pipe_does_not_auto_connect() {
        let mut prior = NONE;
        prior[0] = true;
        let mut neighbors = NONE;
        neighbors[1] = true;
        assert_eq!(update_block_state(prior, 2, None, neighbors, true), prior);
    }

    #[test]
    fn straight_axis_rejects_corners_junctions_and_single_sides() {
        assert_eq!(
            straight_pipe_axis([false, false, false, false, true, true]),
            Some(0)
        );
        assert_eq!(
            straight_pipe_axis([true, true, false, false, false, false]),
            Some(1)
        );
        assert_eq!(
            straight_pipe_axis([false, false, true, true, false, false]),
            Some(2)
        );
        assert_eq!(
            straight_pipe_axis([false, true, true, false, false, false]),
            None
        );
        assert_eq!(
            straight_pipe_axis([false, false, true, false, false, false]),
            None
        );
        assert_eq!(straight_pipe_axis(NONE), None);
    }
}
