//! Pure landing selection for a future permission-checked compass teleport.
//! The host must provide one coherent, generated column snapshot and recheck
//! destination safety and authorization at the actual teleport transaction.

use moddedmc_api::Pos;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TeleportDenied {
    Disabled,
    NoPermission,
    StaleCompass,
    NoSavedTarget,
    WrongDimension,
    UnsafeLanding,
}

/// Mod-side checks required immediately before a future host-controlled
/// teleport. The saved result supplies X/Z and dimension, never client text.
/// `stack_matches` is the host's exact-stack/nonce check, not an item-ID test.
/// Both permission and generated-column safety must be rechecked by the host
/// when it moves the player, since either may change after a menu was shown.
#[derive(Clone, Copy, Debug)]
pub struct TeleportCheck<'a> {
    pub enabled: bool,
    pub permitted: bool,
    pub stack_matches: bool,
    pub current_dimension: &'a str,
    pub saved_target: Option<(&'a str, Pos)>,
    pub landing_y: Option<i32>,
}

pub fn authorize(check: TeleportCheck<'_>) -> Result<Pos, TeleportDenied> {
    if !check.enabled {
        return Err(TeleportDenied::Disabled);
    }
    if !check.permitted {
        return Err(TeleportDenied::NoPermission);
    }
    if !check.stack_matches {
        return Err(TeleportDenied::StaleCompass);
    }
    let (target_dimension, target) = check.saved_target.ok_or(TeleportDenied::NoSavedTarget)?;
    if target_dimension != check.current_dimension {
        return Err(TeleportDenied::WrongDimension);
    }
    let y = check.landing_y.ok_or(TeleportDenied::UnsafeLanding)?;
    Ok(Pos {
        x: target.x,
        y,
        z: target.z,
    })
}

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

    #[test]
    fn permission_and_exact_stack_gate_even_a_previously_displayed_teleport() {
        let target = Pos {
            x: -287,
            y: 71,
            z: 6,
        };
        let mut column = [Cell::Blocked; 16];
        column[7] = Cell::Support;
        column[8] = Cell::Free;
        column[9] = Cell::Free;
        let mut check = TeleportCheck {
            enabled: true,
            permitted: true,
            stack_matches: true,
            current_dimension: "minecraft:overworld",
            saved_target: Some(("minecraft:overworld", target)),
            landing_y: find_landing(55, 63, &column),
        };
        assert_eq!(
            authorize(check),
            Ok(Pos {
                x: -287,
                y: 63,
                z: 6
            })
        );
        // Even a formerly authorized action must be denied if the privilege
        // is revoked before the click, regardless of creative game mode.
        check.permitted = false;
        assert_eq!(authorize(check), Err(TeleportDenied::NoPermission));
        check.permitted = true;
        check.stack_matches = false;
        assert_eq!(authorize(check), Err(TeleportDenied::StaleCompass));
        check.stack_matches = true;
        check.current_dimension = "minecraft:the_nether";
        assert_eq!(authorize(check), Err(TeleportDenied::WrongDimension));
        check.current_dimension = "minecraft:overworld";
        check.saved_target = None;
        assert_eq!(authorize(check), Err(TeleportDenied::NoSavedTarget));
        check.saved_target = Some(("minecraft:overworld", target));
        // A fresh generated-column classification may have invalidated the
        // previous landing. Do not use its old Y as a fallback.
        column[9] = Cell::Blocked;
        check.landing_y = find_landing(55, 63, &column);
        assert_eq!(authorize(check), Err(TeleportDenied::UnsafeLanding));
        check.enabled = false;
        assert_eq!(authorize(check), Err(TeleportDenied::Disabled));
    }
}
