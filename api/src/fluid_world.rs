//! Read fluid state and schedule fluid ticks in loaded vanilla chunks.

use crate::{InterfaceUnavailable, Pos, abi, content::World, require_interface};

#[derive(Clone, Copy)]
pub struct FluidWorld {
    base: &'static abi::FluidWorldV1,
    tick_control: Option<&'static abi::FluidWorldV1TickControl>,
}

#[derive(Clone, Copy)]
pub struct FluidFlow(&'static abi::FluidWorldV1Flow);

pub fn require_flow() -> Result<FluidFlow, InterfaceUnavailable> {
    // SAFETY: this is the appended flow table type for moddedmc:fluid_world v1.
    unsafe { require_interface::<abi::FluidWorldV1Flow>(abi::FLUID_WORLD, 1) }.map(FluidFlow)
}

impl FluidFlow {
    /// The loaded fluid cell's flow vector, or zero for a dry cell.
    pub fn velocity(self, world: World<'_>, pos: Pos) -> Option<[f64; 3]> {
        let mut out = abi::Vec3([0.0; 3]);
        // SAFETY: the world and output are borrowed for the call.
        unsafe { (self.0.velocity)(world.raw(), pos, &raw mut out) }.then_some(out.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FluidCell {
    pub state: u32,
    /// The still-fluid resource id. Empty cells have no fluid.
    pub fluid: Option<String>,
    pub source: bool,
    pub replaceable: bool,
    pub has_waterlogged_property: bool,
    pub waterlogged: bool,
    pub liquid_block: bool,
    pub blocks_motion: bool,
}

pub fn require() -> Result<FluidWorld, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:fluid_world v1.
    unsafe { require_interface(abi::FLUID_WORLD, 1) }.map(|base| FluidWorld {
        base,
        tick_control: None,
    })
}

/// Require fluid tick cancellation and block writes with `UPDATE_KNOWN_SHAPE`.
pub fn require_tick_control() -> Result<FluidWorld, InterfaceUnavailable> {
    // SAFETY: this is the extended table type for moddedmc:fluid_world v1.
    unsafe { require_interface::<abi::FluidWorldV1TickControl>(abi::FLUID_WORLD, 1) }.map(|table| {
        FluidWorld {
            base: &table.base,
            tick_control: Some(table),
        }
    })
}

impl FluidWorld {
    /// Distinguish a loaded dry cell from an unavailable position.
    pub fn inspect(self, world: World<'_>, pos: Pos) -> Option<FluidCell> {
        let mut fluid = Vec::new();
        let mut cell = abi::FluidCell::default();
        let sink = abi::Sink {
            ctx: (&raw mut fluid).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the world, out value and sink are borrowed for this call.
        if !unsafe { (self.base.inspect)(world.raw(), pos, &raw mut cell, sink) } {
            return None;
        }
        Some(FluidCell {
            state: cell.state,
            fluid: if fluid.is_empty() {
                None
            } else {
                Some(String::from_utf8(fluid).ok()?)
            },
            source: cell.flags & abi::FLUID_SOURCE != 0,
            replaceable: cell.flags & abi::FLUID_REPLACEABLE != 0,
            has_waterlogged_property: cell.flags & abi::FLUID_HAS_WATERLOGGED != 0,
            waterlogged: cell.flags & abi::FLUID_WATERLOGGED != 0,
            liquid_block: cell.flags & abi::FLUID_LIQUID_BLOCK != 0,
            blocks_motion: cell.flags & abi::FLUID_BLOCKS_MOTION != 0,
        })
    }

    pub fn schedule_tick(self, world: World<'_>, pos: Pos, fluid: &str, delay: u32) -> bool {
        // SAFETY: the world and registry name are borrowed for this call.
        unsafe { (self.base.schedule_tick)(world.raw(), pos, abi::Str::from(fluid), delay) }
    }

    pub fn schedule_default_tick(self, world: World<'_>, pos: Pos, fluid: &str) -> bool {
        self.schedule_tick(world, pos, fluid, abi::DEFAULT_FLUID_TICK_DELAY)
    }

    /// Predict source recreation for a liquid block replaced by its flowing state.
    pub fn would_regenerate_source(self, world: World<'_>, pos: Pos, fluid: &str) -> Option<bool> {
        // SAFETY: the world and registry name are borrowed for this call.
        match unsafe {
            (self.base.would_regenerate_source)(world.raw(), pos, abi::Str::from(fluid))
        } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    pub fn water_evaporates(self, world: World<'_>, pos: Pos) -> bool {
        // SAFETY: the world is borrowed for this call.
        unsafe { (self.base.water_evaporates)(world.raw(), pos) }
    }

    pub fn clear_ticks(self, world: World<'_>, pos: Pos) -> bool {
        self.tick_control.is_some_and(|table| {
            // SAFETY: the world is borrowed for the call.
            unsafe { (table.clear_ticks)(world.raw(), pos) }
        })
    }
}
