//! Create 6.0.10 tank storage, capacity and controller NBT.

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

    /// Store the Create controller tank shape and single-fluid inventory as NBT.
    /// Boiler and positional state are owned by the enclosing block entity.
    pub fn write_controller_nbt(&self) -> std::collections::HashMap<String, fastnbt::Value> {
        use fastnbt::Value;
        use std::collections::HashMap;
        HashMap::from([
            ("Window".into(), Value::Byte(i8::from(self.window))),
            ("Size".into(), Value::Int(i32::from(self.width))),
            ("Height".into(), Value::Int(i32::from(self.height))),
            ("TankContent".into(), self.content_nbt()),
        ])
    }

    pub(super) fn content_nbt(&self) -> fastnbt::Value {
        use fastnbt::Value;
        use std::collections::HashMap;
        let mut content = HashMap::new();
        if let Some(fluid) = &self.fluid {
            let mut saved = HashMap::from([
                ("id".into(), Value::String(fluid.id.clone())),
                ("amount".into(), Value::Int(fluid.amount as i32)),
            ]);
            if let Some(components) = &fluid.components {
                saved.insert("components".into(), components.clone());
            }
            content.insert("Fluid".into(), Value::Compound(saved));
        }
        Value::Compound(content)
    }

    /// Reject malformed controller shapes or fluid payloads transactionally.
    pub fn read_controller_nbt(
        &mut self,
        data: &std::collections::HashMap<String, fastnbt::Value>,
    ) -> Result<(), TankShapeError> {
        use fastnbt::Value;
        let width = match data.get("Size") {
            Some(Value::Int(value)) => {
                u8::try_from(*value).map_err(|_| TankShapeError::InvalidDimensions)?
            }
            _ => return Err(TankShapeError::InvalidDimensions),
        };
        let height = match data.get("Height") {
            Some(Value::Int(value)) => {
                u16::try_from(*value).map_err(|_| TankShapeError::InvalidDimensions)?
            }
            _ => return Err(TankShapeError::InvalidDimensions),
        };
        let capacity = capacity_for(width, height, self.max_height, self.millibuckets_per_block)?;
        let content = data
            .get("TankContent")
            .ok_or(TankShapeError::InvalidDimensions)?;
        let fluid = Self::fluid_from_content(content, capacity)?;
        self.width = width;
        self.height = height;
        self.window = matches!(data.get("Window"), Some(Value::Byte(1)));
        self.fluid = fluid;
        Ok(())
    }

    pub(super) fn read_content_nbt(
        &mut self,
        content: &fastnbt::Value,
    ) -> Result<(), TankShapeError> {
        self.fluid = Self::fluid_from_content(content, self.capacity())?;
        Ok(())
    }

    fn fluid_from_content(
        content: &fastnbt::Value,
        capacity: u32,
    ) -> Result<Option<FluidStack>, TankShapeError> {
        use fastnbt::Value;
        let Value::Compound(content) = content else {
            return Err(TankShapeError::InvalidDimensions);
        };
        match content.get("Fluid") {
            None => Ok(None),
            Some(Value::Compound(tag)) => {
                let id = match tag.get("id") {
                    Some(Value::String(id)) if !id.is_empty() => id.clone(),
                    _ => return Err(TankShapeError::InvalidDimensions),
                };
                let amount = match tag.get("amount") {
                    Some(Value::Int(amount)) if *amount > 0 => *amount as u32,
                    _ => return Err(TankShapeError::InvalidDimensions),
                };
                let stack = FluidStack {
                    id,
                    amount: amount.min(capacity),
                    components: tag.get("components").cloned(),
                };
                if stack.is_empty() {
                    return Err(TankShapeError::InvalidDimensions);
                }
                Ok(Some(stack))
            }
            _ => Err(TankShapeError::InvalidDimensions),
        }
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
    fn java_controller_nbt_shape_and_components_roundtrip() {
        let mut tank = TankInventory::new(8_000, 8).unwrap();
        tank.resize(3, 4).unwrap();
        tank.set_window(false);
        let potion = FluidStack {
            id: "create:potion".into(),
            amount: 1_234,
            components: Some(Value::Compound(HashMap::from([(
                "potion".into(),
                Value::String("minecraft:healing".into()),
            )]))),
        };
        assert_eq!(tank.fill(&potion, false), 1_234);
        let saved = tank.write_controller_nbt();
        assert_eq!(saved.get("Size"), Some(&Value::Int(3)));
        assert_eq!(saved.get("Height"), Some(&Value::Int(4)));
        assert_eq!(saved.get("Window"), Some(&Value::Byte(0)));
        let mut loaded = TankInventory::new(8_000, 8).unwrap();
        loaded.read_controller_nbt(&saved).unwrap();
        assert_eq!(loaded.width(), 3);
        assert_eq!(loaded.height(), 4);
        assert!(!loaded.window());
        assert_eq!(loaded.fluid(), Some(&potion));
        assert_eq!(loaded.write_controller_nbt(), saved);
    }

    #[test]
    fn malformed_java_tank_nbt_rejected_without_partial_state_changes() {
        let mut tank = TankInventory::new(1_000, 4).unwrap();
        tank.fill(&water(500), false);
        let original = tank.write_controller_nbt();

        let mut invalid = original.clone();
        invalid.insert("Size".into(), Value::Int(100));
        assert_eq!(
            tank.read_controller_nbt(&invalid),
            Err(TankShapeError::InvalidDimensions)
        );
        assert_eq!(tank.write_controller_nbt(), original);

        let mut invalid = original.clone();
        invalid.insert("TankContent".into(), Value::String("not a compound".into()));
        assert_eq!(
            tank.read_controller_nbt(&invalid),
            Err(TankShapeError::InvalidDimensions)
        );
        assert_eq!(tank.write_controller_nbt(), original);

        let mut invalid = original.clone();
        let content = HashMap::from([(
            "Fluid".into(),
            Value::Compound(HashMap::from([
                ("id".into(), Value::String("minecraft:water".into())),
                ("amount".into(), Value::Int(-1)),
            ])),
        )]);
        invalid.insert("TankContent".into(), Value::Compound(content));
        assert_eq!(
            tank.read_controller_nbt(&invalid),
            Err(TankShapeError::InvalidDimensions)
        );
        assert_eq!(tank.write_controller_nbt(), original);
    }

    #[test]
    fn loading_oversized_inventory_caps_to_controller_capacity() {
        let mut tank = TankInventory::new(1_000, 4).unwrap();
        let mut large = TankInventory::new(10_000, 4).unwrap();
        large.fill(&water(8_000), false);
        tank.read_controller_nbt(&large.write_controller_nbt())
            .unwrap();
        assert_eq!(tank.capacity(), 1_000);
        assert_eq!(tank.amount(), 1_000);
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

#[cfg(test)]
mod fluid_parity_regressions {
    use super::*;
    use fastnbt::Value;
    use std::collections::HashMap;

    fn water(components: Option<Value>) -> FluidStack {
        FluidStack {
            id: "minecraft:water".into(),
            amount: 1000,
            components,
        }
    }

    #[test]
    fn empty_patch_fills_and_drains_same_tank() {
        let mut tank = TankInventory::new(8000, 4).unwrap();
        let absent = water(None);
        let explicit = water(Some(Value::Compound(HashMap::new())));
        assert_eq!(tank.fill(&absent, false), 1000);
        assert_eq!(tank.fill(&explicit, false), 1000);
        assert_eq!(tank.drain(&explicit, false).unwrap().amount, 1000);
        assert_eq!(tank.amount(), 1000);
    }

    #[test]
    fn minecraft_empty_cannot_fill_tank() {
        let mut tank = TankInventory::new(8000, 4).unwrap();
        let empty = FluidStack {
            id: "minecraft:empty".into(),
            amount: 1000,
            components: None,
        };
        assert_eq!(tank.fill(&empty, false), 0);
        assert!(tank.fluid().is_none());
    }
}
