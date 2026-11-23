//! Create 6.0.10 VanillaFluidTargets.drainBlock, including cauldrons and honey.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Other,
    EmptyCauldron,
    LavaCauldron,
    WaterCauldron {
        level: u8,
    },
    /// A block with Create's LEVEL_HONEY property.
    Honey {
        level: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fluid {
    Water,
    Lava,
    Honey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Drain {
    pub fluid: Fluid,
    pub amount: u32,
    pub resulting_block: Target,
}

/// VanillaFluidTargets.canProvideFluidWithoutCapability includes containers
/// which are empty or not full: the later drain check decides their output.
pub fn can_provide_without_capability(target: Target) -> bool {
    matches!(
        target,
        Target::EmptyCauldron
            | Target::LavaCauldron
            | Target::WaterCauldron { .. }
            | Target::Honey { .. }
    )
}

/// A draining plan, preserving SIMULATE semantics. The caller resolves the
/// resulting block selector and performs its UPDATE_ALL write when executing.
pub fn drain_block(target: Target, simulate: bool) -> Option<Drain> {
    let (fluid, amount, emptied) = match target {
        Target::Honey { level } if level >= 5 => (Fluid::Honey, 250, Target::Honey { level: 0 }),
        Target::LavaCauldron => (Fluid::Lava, 1000, Target::EmptyCauldron),
        Target::WaterCauldron { level: 3 } => (Fluid::Water, 1000, Target::EmptyCauldron),
        _ => return None,
    };
    Some(Drain {
        fluid,
        amount,
        resulting_block: if simulate { target } else { emptied },
    })
}

#[cfg(test)]
mod tests {
    use super::{Fluid, Target, can_provide_without_capability, drain_block};

    #[test]
    fn water_cauldron_drains_only_at_full_level_and_simulation_keeps_state() {
        for level in [1, 2] {
            assert!(drain_block(Target::WaterCauldron { level }, false).is_none());
        }
        let full = Target::WaterCauldron { level: 3 };
        let preview = drain_block(full, true).unwrap();
        assert_eq!((preview.fluid, preview.amount), (Fluid::Water, 1000));
        assert_eq!(preview.resulting_block, full);
        assert_eq!(
            drain_block(full, false).unwrap().resulting_block,
            Target::EmptyCauldron
        );
    }

    #[test]
    fn honey_level_five_drains_one_quarter_bucket_and_resets_level() {
        assert!(drain_block(Target::Honey { level: 4 }, false).is_none());
        let full = Target::Honey { level: 5 };
        let simulated = drain_block(full, true).unwrap();
        assert_eq!((simulated.fluid, simulated.amount), (Fluid::Honey, 250));
        assert_eq!(simulated.resulting_block, full);
        assert_eq!(
            drain_block(full, false).unwrap().resulting_block,
            Target::Honey { level: 0 }
        );
    }

    #[test]
    fn lava_cauldron_drains_whole_bucket_and_empties_on_execution() {
        let full = Target::LavaCauldron;
        assert_eq!(drain_block(full, true).unwrap().resulting_block, full);
        let result = drain_block(full, false).unwrap();
        assert_eq!((result.fluid, result.amount), (Fluid::Lava, 1000));
        assert_eq!(result.resulting_block, Target::EmptyCauldron);
    }

    #[test]
    fn empty_and_partial_containers_are_connectable_but_not_drainable() {
        for target in [
            Target::EmptyCauldron,
            Target::WaterCauldron { level: 2 },
            Target::Honey { level: 0 },
        ] {
            assert!(can_provide_without_capability(target));
            assert!(drain_block(target, false).is_none());
        }
        assert!(!can_provide_without_capability(Target::Other));
        assert!(drain_block(Target::Other, true).is_none());
    }
}
