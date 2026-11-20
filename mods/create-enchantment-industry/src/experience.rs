//! Exact 2.5.4 Fabric ExperienceHelper / CEIFluidUnits arithmetic.
//! The released 1.20.1 bytecode uses wrapping i32 for XP level totals,
//! unlike 2.5.4b NeoForge's newer saturating long implementation.

const FABRIC_UNITS_PER_MILLIBUCKET: i64 = 81;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExperienceError {
    Overflow,
    DivisionByZero,
}

/// `ExperienceHelper.getExperienceForNextLevel`: 2L+7, 5L-38 or 9L-158.
/// The published Java method uses signed 32-bit integer arithmetic.
pub fn get_experience_for_next_level(level: i32) -> i32 {
    if level >= 30 {
        level.wrapping_mul(9).wrapping_sub(158)
    } else if level >= 15 {
        level.wrapping_mul(5).wrapping_sub(38)
    } else {
        level.wrapping_mul(2).wrapping_add(7)
    }
}

/// `ExperienceHelper.getExperienceForTotalLevel` in the released Fabric
/// **2.5.4** binary. Java integer overflows before division by two.
/// Do not substitute the later NeoForge long-math clamp.
pub fn get_experience_for_total_level(level: i32) -> i32 {
    if level == 0 {
        0
    } else if level >= 31 {
        level
            .wrapping_mul(9)
            .wrapping_mul(level)
            .wrapping_sub(level.wrapping_mul(325))
            / 2
            + 2220
    } else if level >= 16 {
        level
            .wrapping_mul(5)
            .wrapping_mul(level)
            .wrapping_sub(level.wrapping_mul(81))
            / 2
            + 360
    } else {
        level
            .wrapping_mul(level)
            .wrapping_add(level.wrapping_mul(6))
    }
}

/// `getExperienceForPlayer` adds Java `Math.round(float)` progress XP.
/// Java breaks half-way ties toward +infinity, unlike Rust `f32::round`.
pub fn get_experience_for_player(level: i32, progress: f32) -> i32 {
    let partial = progress * get_experience_for_next_level(level) as f32;
    let rounded = (f64::from(partial) + 0.5).floor() as i32;
    get_experience_for_total_level(level).wrapping_add(rounded)
}

/// Fabric CEIFluidUnits.toMillibuckets divides by 81 raw fluid units.
pub fn to_millibuckets(raw_units: i64) -> i64 {
    raw_units / FABRIC_UNITS_PER_MILLIBUCKET
}

/// `ExperienceHelper.getExperienceFromFluid`, after the source's empty
/// stack check and 81-unit conversion. Standard XP fluid is one XP/mB;
/// a data-map-provided other fluid uses the given mB per XP.
pub fn get_experience_from_fluid(
    raw_units: i64,
    is_experience_fluid: bool,
    other_fluid_unit: Option<i32>,
) -> Result<i32, ExperienceError> {
    if raw_units <= 0 {
        return Ok(0);
    }
    let mb = to_millibuckets(raw_units);
    if is_experience_fluid {
        return i32::try_from(mb).map_err(|_| ExperienceError::Overflow);
    }
    let Some(unit) = other_fluid_unit else {
        return Ok(0);
    };
    if unit == 0 {
        return Err(ExperienceError::DivisionByZero);
    }
    i32::try_from(mb / i64::from(unit)).map_err(|_| ExperienceError::Overflow)
}

/// `getFluidFromExperience` and `CEIFluidUnits.millibuckets`: first
/// Math.multiplyExact(unit * XP), then Math.multiplyExact(result * 81).
/// An unknown fluid has a zero unit and returns a zero-sized stack.
pub fn get_fluid_from_experience(
    experience: i32,
    is_experience_fluid: bool,
    other_fluid_unit: Option<i32>,
) -> Result<i64, ExperienceError> {
    let unit = if is_experience_fluid {
        1_i64
    } else {
        i64::from(other_fluid_unit.unwrap_or_default())
    };
    unit.checked_mul(i64::from(experience))
        .and_then(|millibuckets| millibuckets.checked_mul(FABRIC_UNITS_PER_MILLIBUCKET))
        .ok_or(ExperienceError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_level_curve_changes_at_exact_fabric_boundaries() {
        assert_eq!(get_experience_for_next_level(0), 7);
        assert_eq!(get_experience_for_next_level(14), 35);
        assert_eq!(get_experience_for_next_level(15), 37);
        assert_eq!(get_experience_for_next_level(29), 107);
        assert_eq!(get_experience_for_next_level(30), 112);
        assert_eq!(get_experience_for_next_level(31), 121);
    }

    #[test]
    fn total_xp_level_boundaries_match_published_fabric_bytecode() {
        assert_eq!(get_experience_for_total_level(0), 0);
        assert_eq!(get_experience_for_total_level(1), 7);
        assert_eq!(get_experience_for_total_level(14), 280);
        assert_eq!(get_experience_for_total_level(15), 315);
        assert_eq!(get_experience_for_total_level(16), 352);
        assert_eq!(get_experience_for_total_level(29), 1288);
        assert_eq!(get_experience_for_total_level(30), 1395);
        assert_eq!(get_experience_for_total_level(31), 1507);
        assert_eq!(get_experience_for_total_level(32), 1628);
    }

    #[test]
    fn released_fabric_overflows_i32_instead_of_clamping_high_levels() {
        assert_eq!(get_experience_for_total_level(5_000), 111_689_720);
        assert_eq!(get_experience_for_total_level(50_000), 504_458_980);
        assert_eq!(get_experience_for_total_level(100_000), -113_404_388);
    }

    #[test]
    fn player_progress_rounds_half_up_as_java_not_away_from_zero() {
        assert_eq!(get_experience_for_player(15, 0.5), 334);
        assert_eq!(get_experience_for_player(0, 0.5), 4);
        assert_eq!(get_experience_for_player(1, 0.5), 12);
        assert_eq!(get_experience_for_player(0, -0.5 / 7.0), 0);
        assert_eq!(get_experience_for_player(0, f32::NAN), 0);
    }

    #[test]
    fn fabric_fluid_uses_eighty_one_raw_units_per_millibucket() {
        assert_eq!(to_millibuckets(80), 0);
        assert_eq!(to_millibuckets(81), 1);
        assert_eq!(get_experience_from_fluid(81, true, None), Ok(1));
        assert_eq!(get_experience_from_fluid(81 * 25, false, Some(2)), Ok(12));
        assert_eq!(get_experience_from_fluid(81 * 25, false, None), Ok(0));
        assert_eq!(get_experience_from_fluid(0, true, None), Ok(0));
        assert_eq!(
            get_experience_from_fluid(81, false, Some(0)),
            Err(ExperienceError::DivisionByZero)
        );
    }

    #[test]
    fn experience_to_fluid_preserves_exact_multiplication_and_overflow() {
        assert_eq!(get_fluid_from_experience(10, true, None), Ok(810));
        assert_eq!(get_fluid_from_experience(10, false, Some(2)), Ok(1620));
        assert_eq!(get_fluid_from_experience(10, false, None), Ok(0));
        assert_eq!(
            get_fluid_from_experience(i32::MAX, false, Some(i32::MAX)),
            Err(ExperienceError::Overflow)
        );
        assert_eq!(
            get_experience_from_fluid(i64::from(i32::MAX) * 81, true, None),
            Ok(i32::MAX)
        );
        assert_eq!(
            get_experience_from_fluid((i64::from(i32::MAX) + 1) * 81, true, None),
            Err(ExperienceError::Overflow)
        );
    }
}
