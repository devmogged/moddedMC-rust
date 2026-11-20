//! Create 6.0.10 FluidTankBlockEntity's single-fluid inventory and multiblock
//! capacity rules. This is the state model; world block registration, controller
//! discovery, persistence and sided capability exposure await the host interfaces.
//! TODO(abi): #42 - register a sided tank capability with stable handler identity.

use super::{network::FluidHandler, pipe_connection::FluidStack};

/// Create's FluidTankBlockEntity.MAX_SIZE (horizontal dimensions).
pub const MAX_WIDTH: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TankShapeError {
    InvalidDimensions,
    CapacityOverflow,
}

/// The controller holds the fluid for all width * width * height blocks.
/// Capacity per block and maximum height are Create server configuration values,
/// so neither is hard-coded to an arbitrary pack default.
#[derive(Clone, Debug)]
pub struct TankInventory {
    width: u8,
    height: u16,
    max_height: u16,
    millibuckets_per_block: u32,
    fluid: Option<FluidStack>,
    window: bool,
}

fn capacity_for(
    width: u8,
    height: u16,
    max_height: u16,
    millibuckets_per_block: u32,
) -> Result<u32, TankShapeError> {
    if width == 0
        || width > MAX_WIDTH
        || height == 0
        || height > max_height
        || millibuckets_per_block == 0
    {
        return Err(TankShapeError::InvalidDimensions);
    }

    u32::from(width)
        .checked_mul(u32::from(width))
        .and_then(|area| area.checked_mul(u32::from(height)))
        .and_then(|blocks| blocks.checked_mul(millibuckets_per_block))
        .filter(|capacity| *capacity <= i32::MAX as u32)
        .ok_or(TankShapeError::CapacityOverflow)
}

impl TankInventory {
    /// A newly placed tank is a 1x1x1 controller, with windows enabled.
    pub fn new(millibuckets_per_block: u32, max_height: u16) -> Result<Self, TankShapeError> {
        capacity_for(1, 1, max_height, millibuckets_per_block)?;
        Ok(Self {
            width: 1,
            height: 1,
            max_height,
            millibuckets_per_block,
            fluid: None,
            window: true,
        })
    }

    pub fn width(&self) -> u8 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn window(&self) -> bool {
        self.window
    }

    pub fn set_window(&mut self, enabled: bool) {
        self.window = enabled;
    }

    pub fn capacity(&self) -> u32 {
        // Validated by new() and resize().
        u32::from(self.width)
            * u32::from(self.width)
            * u32::from(self.height)
            * self.millibuckets_per_block
    }

    pub fn amount(&self) -> u32 {
        self.fluid.as_ref().map_or(0, |fluid| fluid.amount)
    }

    pub fn fluid(&self) -> Option<&FluidStack> {
        self.fluid.as_ref()
    }

    pub fn fill_ratio(&self) -> f32 {
        self.amount() as f32 / self.capacity() as f32
    }

    /// Mirrors applyFluidTankSize: when the controller shrinks, excess fluid
    /// is drained. Return that discarded quantity explicitly to account for
    /// it in split/merge logic; never silently truncate on resize.
    pub fn resize(&mut self, width: u8, height: u16) -> Result<u32, TankShapeError> {
        let capacity = capacity_for(width, height, self.max_height, self.millibuckets_per_block)?;
        let overflow = self.amount().saturating_sub(capacity);
        self.width = width;
        self.height = height;
        if let Some(stored) = &mut self.fluid {
            stored.amount -= overflow;
            if stored.amount == 0 {
                self.fluid = None;
            }
        }
        Ok(overflow)
    }
}

impl FluidHandler for TankInventory {
    fn handler_identity(&self) -> u64 {
        // Only used for the in-process model. Sided host adapters must report
        // their common underlying controller's identity, not wrapper addresses.
        self as *const Self as usize as u64
    }

    fn tank_count(&self) -> usize {
        1
    }

    fn fluid_in_tank(&self, tank: usize) -> Option<FluidStack> {
        (tank == 0).then(|| {
            self.fluid.clone().unwrap_or_else(|| FluidStack {
                id: String::new(),
                amount: 0,
                components: None,
            })
        })
    }

    fn fill(&mut self, offered: &FluidStack, simulate: bool) -> u32 {
        if offered.is_empty()
            || self
                .fluid
                .as_ref()
                .is_some_and(|existing| !existing.same_fluid_same_components(offered))
        {
            return 0;
        }
        let accepted = offered
            .amount
            .min(self.capacity().saturating_sub(self.amount()));
        if !simulate && accepted != 0 {
            if let Some(stored) = &mut self.fluid {
                stored.amount += accepted;
            } else {
                let mut stored = offered.clone();
                stored.amount = accepted;
                self.fluid = Some(stored);
            }
        }
        accepted
    }

    fn drain(&mut self, request: &FluidStack, simulate: bool) -> Option<FluidStack> {
        let stored = self.fluid.as_ref()?;
        if request.is_empty() || !stored.same_fluid_same_components(request) {
            return None;
        }
        let count = stored.amount.min(request.amount);
        if count == 0 {
            return None;
        }
        let mut extracted = stored.clone();
        extracted.amount = count;
        if !simulate {
            let remaining = stored.amount - count;
            if remaining == 0 {
                self.fluid = None;
            } else if let Some(stored) = &mut self.fluid {
                stored.amount = remaining;
            }
        }
        Some(extracted)
    }

    fn drain_any(&mut self, amount: u32, simulate: bool) -> Option<FluidStack> {
        let mut requested = self.fluid.as_ref()?.clone();
        requested.amount = amount;
        self.drain(&requested, simulate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fluids::network::transfer_tick;
    use fastnbt::Value;
    use std::collections::HashMap;

    fn water(amount: u32) -> FluidStack {
        FluidStack {
            id: "minecraft:water".into(),
            amount,
            components: None,
        }
    }

    #[test]
    fn twelve_block_tank_has_twelve_times_configured_capacity() {
        let mut tank = TankInventory::new(8_000, 8).unwrap();
        assert_eq!(tank.capacity(), 8_000);
        assert!(tank.window());
        assert_eq!(tank.resize(2, 3), Ok(0));
        assert_eq!(tank.capacity(), 96_000);
        assert_eq!(tank.fill(&water(80_000), false), 80_000);
        assert_eq!(tank.fill(&water(30_000), true), 16_000);
        assert_eq!(tank.amount(), 80_000);
        assert_eq!(tank.fill_ratio(), 80_000f32 / 96_000f32);
        assert_eq!(tank.fill(&water(30_000), false), 16_000);
        assert_eq!(tank.amount(), 96_000);
        assert_eq!(tank.drain_any(5_000, false), Some(water(5_000)));
        assert_eq!(tank.amount(), 91_000);
    }

    #[test]
    fn shrinking_tank_reports_exact_overflow_and_rejects_invalid_shapes() {
        let mut tank = TankInventory::new(1_000, 4).unwrap();
        tank.resize(3, 4).unwrap();
        assert_eq!(tank.fill(&water(35_000), false), 35_000);
        assert_eq!(tank.resize(0, 2), Err(TankShapeError::InvalidDimensions));
        assert_eq!(tank.resize(4, 2), Err(TankShapeError::InvalidDimensions));
        assert_eq!(tank.resize(2, 5), Err(TankShapeError::InvalidDimensions));
        assert_eq!(tank.amount(), 35_000);
        assert_eq!(tank.capacity(), 36_000);
        assert_eq!(tank.resize(2, 3), Ok(23_000));
        assert_eq!(tank.amount(), 12_000);
        assert_eq!(tank.capacity(), 12_000);
    }

    #[test]
    fn fluid_identity_includes_components_and_simulation_is_read_only() {
        let mut tank = TankInventory::new(8_000, 4).unwrap();
        let mut potion = water(2_000);
        potion.components = Some(Value::Compound(HashMap::from([(
            "minecraft:potion_contents".into(),
            Value::String("minecraft:healing".into()),
        )])));
        assert_eq!(tank.fill(&potion, true), 2_000);
        assert_eq!(tank.amount(), 0);
        assert_eq!(tank.fill(&potion, false), 2_000);
        assert_eq!(tank.fill(&water(1_000), false), 0);
        assert_eq!(tank.drain(&water(1_000), false), None);
        assert_eq!(
            tank.drain_any(500, true).map(|fluid| fluid.amount),
            Some(500)
        );
        assert_eq!(tank.amount(), 2_000);
        assert_eq!(tank.drain(&potion, false), Some(potion));
        assert!(tank.fluid().is_none());
        assert_eq!(tank.fill(&water(1_000), false), 1_000);
    }

    #[test]
    fn full_tank_source_and_partial_target_conserve_fluid() {
        let mut source = TankInventory::new(8_000, 4).unwrap();
        let mut target = TankInventory::new(8_000, 4).unwrap();
        source.fill(&water(8_000), false);
        target.fill(&water(7_000), false);
        let moved = transfer_tick(&mut source, &mut [&mut target], &water(8_000), 4_000.0);
        assert_eq!(moved, 1_000);
        assert_eq!(source.amount(), 7_000);
        assert_eq!(target.amount(), 8_000);
    }

    #[test]
    fn invalid_capacity_never_overflows_or_changes_geometry() {
        assert!(matches!(
            TankInventory::new(u32::MAX, 4),
            Err(TankShapeError::CapacityOverflow)
        ));
        let mut tank = TankInventory::new(i32::MAX as u32 / 2, 8).unwrap();
        assert_eq!(tank.resize(3, 8), Err(TankShapeError::CapacityOverflow));
        assert_eq!((tank.width(), tank.height()), (1, 1));
    }
}
