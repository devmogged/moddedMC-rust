//! World-state decisions from Create 6.0.10 OpenEndedPipe.
//! Applying blocks, scheduling fluid ticks and playing collision effects is a host operation.

/// A loaded block's fluid/replaceability information, as read by the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidCell<'a> {
    pub loaded: bool,
    pub replaceable: bool,
    pub has_waterlogged_property: bool,
    /// None means empty; otherwise this is the still-fluid registry id.
    pub fluid: Option<&'a str>,
    pub source: bool,
}

/// Fluid capabilities that OpenEndedPipe.provideFluidToSpace checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FluidKind<'a> {
    pub id: &'a str,
    pub amount: u32,
    pub flowing: bool,
    pub has_block_state: bool,
    pub water_tag: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillDecision {
    Reject,
    /// FluidReactions.handlePipeSpillCollision runs even for a simulated fill.
    ReactWithExisting,
    /// The effect handler can consume a fluid that has no block form.
    EffectOnly,
    /// Accepted by the handler but no block should be placed.
    AcceptWithoutPlacing,
    /// The water source evaporates and the world plays the extinguish sound.
    Evaporate,
    Waterlog,
    PlaceSource,
}

/// OpenEndedPipe.provideFluidToSpace. Java's in-world mutation is deliberately
/// separate so a SIMULATE result never writes the world through this function.
pub fn plan_fill(
    cell: FluidCell<'_>,
    fluid: FluidKind<'_>,
    placement_enabled: bool,
    ultra_warm: bool,
    simulate: bool,
) -> FillDecision {
    if !cell.loaded
        || (!cell.has_waterlogged_property && !cell.replaceable)
        || fluid.amount == 0
        || fluid.id.is_empty()
        || !fluid.flowing
    {
        return FillDecision::Reject;
    }
    if !fluid.has_block_state {
        return FillDecision::EffectOnly;
    }
    if let Some(existing) = cell.fluid
        && existing != fluid.id
    {
        return FillDecision::ReactWithExisting;
    }
    if cell.source || (cell.has_waterlogged_property && fluid.id != "minecraft:water") {
        return FillDecision::Reject;
    }
    if simulate || !placement_enabled {
        return FillDecision::AcceptWithoutPlacing;
    }
    if ultra_warm && fluid.water_tag {
        return FillDecision::Evaporate;
    }
    if cell.has_waterlogged_property {
        FillDecision::Waterlog
    } else {
        FillDecision::PlaceSource
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrainDecision {
    Reject,
    /// Emptying this block must also schedule the WATER fluid tick after one tick.
    RemoveWaterlogging,
    /// Source stays present because vanilla would regenerate it immediately.
    InfiniteSource,
    /// Replace the source with its flowing-level-14 state.
    ReplaceWithFlowing,
}

/// OpenEndedPipe.removeFluidFromSpace, after VanillaFluidTargets.drainBlock
/// checks cauldrons/honey. The caller must perform that special-target check first.
pub fn plan_source_drain(cell: FluidCell<'_>, would_regenerate: bool) -> DrainDecision {
    if !cell.loaded || (!cell.has_waterlogged_property && !cell.replaceable) {
        return DrainDecision::Reject;
    }
    if cell.fluid.is_none() || !cell.source {
        return DrainDecision::Reject;
    }
    if cell.has_waterlogged_property {
        return DrainDecision::RemoveWaterlogging;
    }
    if would_regenerate {
        DrainDecision::InfiniteSource
    } else {
        DrainDecision::ReplaceWithFlowing
    }
}

#[cfg(test)]
mod tests {
    use super::{DrainDecision, FillDecision, FluidCell, FluidKind, plan_fill, plan_source_drain};

    fn air() -> FluidCell<'static> {
        FluidCell {
            loaded: true,
            replaceable: true,
            has_waterlogged_property: false,
            fluid: None,
            source: false,
        }
    }

    fn water() -> FluidKind<'static> {
        FluidKind {
            id: "minecraft:water",
            amount: 1000,
            flowing: true,
            has_block_state: true,
            water_tag: true,
        }
    }

    #[test]
    fn filling_refuses_unloaded_and_unreplaceable_blocks() {
        assert_eq!(
            plan_fill(
                FluidCell {
                    loaded: false,
                    ..air()
                },
                water(),
                true,
                false,
                false
            ),
            FillDecision::Reject
        );
        assert_eq!(
            plan_fill(
                FluidCell {
                    replaceable: false,
                    ..air()
                },
                water(),
                true,
                false,
                false
            ),
            FillDecision::Reject
        );
        assert_eq!(
            plan_fill(air(), water(), true, false, false),
            FillDecision::PlaceSource
        );
    }

    #[test]
    fn existing_source_rejects_same_fluid_and_different_fluid_reacts() {
        let source = FluidCell {
            fluid: Some("minecraft:water"),
            source: true,
            ..air()
        };
        assert_eq!(
            plan_fill(source, water(), true, false, false),
            FillDecision::Reject
        );
        let lava = FluidKind {
            id: "minecraft:lava",
            water_tag: false,
            ..water()
        };
        assert_eq!(
            plan_fill(source, lava, true, false, true),
            FillDecision::ReactWithExisting
        );
    }

    #[test]
    fn source_placement_setting_simulation_and_nether_water_have_distinct_outcomes() {
        assert_eq!(
            plan_fill(air(), water(), false, false, false),
            FillDecision::AcceptWithoutPlacing
        );
        assert_eq!(
            plan_fill(air(), water(), true, true, true),
            FillDecision::AcceptWithoutPlacing
        );
        assert_eq!(
            plan_fill(air(), water(), true, true, false),
            FillDecision::Evaporate
        );
        assert_eq!(
            plan_fill(air(), water(), true, false, false),
            FillDecision::PlaceSource
        );
    }

    #[test]
    fn waterlogging_accepts_water_and_refuses_lava() {
        let slab = FluidCell {
            replaceable: false,
            has_waterlogged_property: true,
            ..air()
        };
        assert_eq!(
            plan_fill(slab, water(), true, false, false),
            FillDecision::Waterlog
        );
        let lava = FluidKind {
            id: "minecraft:lava",
            water_tag: false,
            ..water()
        };
        assert_eq!(
            plan_fill(slab, lava, true, false, false),
            FillDecision::Reject
        );
        let tagged_water = FluidKind {
            id: "example:waterlike",
            water_tag: true,
            ..water()
        };
        assert_eq!(
            plan_fill(slab, tagged_water, true, false, false),
            FillDecision::Reject
        );
    }

    #[test]
    fn fluids_without_placed_block_states_take_the_effect_path() {
        let effect = FluidKind {
            id: "create:potion",
            has_block_state: false,
            water_tag: false,
            ..water()
        };
        assert_eq!(
            plan_fill(air(), effect, true, false, false),
            FillDecision::EffectOnly
        );
        assert_eq!(
            plan_fill(
                air(),
                FluidKind {
                    flowing: false,
                    ..water()
                },
                true,
                false,
                false
            ),
            FillDecision::Reject
        );
    }

    #[test]
    fn draining_a_source_matches_waterlogging_and_infinite_source_rules() {
        let source = FluidCell {
            fluid: Some("minecraft:water"),
            source: true,
            ..air()
        };
        assert_eq!(
            plan_source_drain(source, false),
            DrainDecision::ReplaceWithFlowing
        );
        assert_eq!(
            plan_source_drain(source, true),
            DrainDecision::InfiniteSource
        );
        assert_eq!(
            plan_source_drain(
                FluidCell {
                    has_waterlogged_property: true,
                    ..source
                },
                true
            ),
            DrainDecision::RemoveWaterlogging
        );
        assert_eq!(plan_source_drain(air(), false), DrainDecision::Reject);
        assert_eq!(
            plan_source_drain(
                FluidCell {
                    source: false,
                    ..source
                },
                false
            ),
            DrainDecision::Reject
        );
    }
}
