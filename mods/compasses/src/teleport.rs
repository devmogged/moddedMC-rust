//! Pure landing selection for a future permission-checked compass teleport.
//! The host must provide one coherent, generated column snapshot and recheck
//! destination safety and authorization at the actual teleport transaction.

/// A cell is classified by the host, not by a guessed block identifier.
/// Free must be collision-free, dry and nonhazardous for the player's body.
/// Support must be a safe, full-width upper surface. Blocked is neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cell {
    Free,
    Support,
    Blocked,
}

/// Sea-level-outwards scan, preferring higher Y on equal distance. Both
/// original compass packet handlers use that order. Unlike their unsafe
/// fallback to Y=256, absence is explicit. Feet, head and floor must all
/// lie inside the supplied dimension's actual build-height interval.
pub fn find_landing(min_y: i32, sea_level: i32, column: &[Cell]) -> Option<i32> {
    if column.len() < 3 || column.len() > 4096 {
        return None;
    }
    let max_y = min_y.checked_add(i32::try_from(column.len()).ok()?.checked_sub(1)?)?;
    let lower = min_y.checked_add(1)?;
    let upper = max_y.checked_sub(1)?;
    let start = sea_level.clamp(lower, upper);
    let distance = i64::from(start - lower).max(i64::from(upper - start));
    let is_valid = |y: i64| {
        if y < i64::from(lower) || y > i64::from(upper) {
            return false;
        }
        let i = (y - i64::from(min_y)) as usize;
        column[i - 1] == Cell::Support && column[i] == Cell::Free && column[i + 1] == Cell::Free
    };
    for offset in 0..=distance {
        let up = i64::from(start) + offset;
        if is_valid(up) {
            return i32::try_from(up).ok();
        }
        let down = i64::from(start) - offset;
        if offset != 0 && is_valid(down) {
            return i32::try_from(down).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_sea_level_and_upward_tie_match_source_scan() {
        let mut col = [Cell::Blocked; 32];
        for feet in [8, 16] {
            col[feet - 1] = Cell::Support;
            col[feet] = Cell::Free;
            col[feet + 1] = Cell::Free;
        }
        assert_eq!(find_landing(0, 12, &col), Some(16));
        assert_eq!(find_landing(0, 11, &col), Some(8));
        assert_eq!(find_landing(-64, -52, &col), Some(-48));
    }

    #[test]
    fn no_landing_in_air_fluid_fire_or_without_headroom() {
        assert_eq!(find_landing(-64, 63, &[Cell::Free; 384]), None);
        let mut col = [Cell::Blocked; 384];
        col[127] = Cell::Free;
        col[126] = Cell::Support;
        // A non-free head cell makes the would-be landing invalid.
        assert_eq!(find_landing(-64, 63, &col), None);
        col[128] = Cell::Free;
        assert_eq!(find_landing(-64, 63, &col), Some(63));
        // A hazardous or partial support block is classified Blocked.
        col[126] = Cell::Blocked;
        assert_eq!(find_landing(-64, 63, &col), None);
    }

    #[test]
    fn bounds_exclude_top_bottom_and_extreme_sea_levels() {
        let col = [
            Cell::Support,
            Cell::Free,
            Cell::Free,
            Cell::Support,
            Cell::Free,
            Cell::Free,
        ];
        assert_eq!(find_landing(-100, i32::MIN, &col), Some(-99));
        assert_eq!(find_landing(-100, i32::MAX, &col), Some(-96));
        assert_eq!(find_landing(i32::MAX, 0, &col), None);
        assert_eq!(find_landing(0, 0, &[Cell::Free; 2]), None);
        assert_eq!(find_landing(0, 0, &[Cell::Free; 4097]), None);
    }
}
