//! `InternalEnergyStorage` from Create Crafts & Additions 1.7.2.
//! Its NBT field is exactly `energy`; NeoForge transfer limits still apply.
//! No server energy capability is registered until the host service exists.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnergyStore {
    pub capacity: i32,
    pub max_receive: i32,
    pub max_extract: i32,
    pub energy: i32,
}

#[derive(Serialize, Deserialize)]
struct SavedEnergy {
    energy: i32,
}

impl EnergyStore {
    pub fn new(capacity: i32, max_receive: i32, max_extract: i32) -> Self {
        Self {
            capacity,
            max_receive,
            max_extract,
            energy: 0,
        }
    }

    pub fn can_receive(self) -> bool {
        self.max_receive > 0
    }

    pub fn can_extract(self) -> bool {
        self.max_extract > 0
    }

    /// NeoForge EnergyStorage.receiveEnergy: respect the handler's I/O cap
    /// and remaining space; simulation does not alter the store.
    pub fn receive(&mut self, max_requested: i32, simulate: bool) -> i32 {
        if !self.can_receive() || max_requested <= 0 {
            return 0;
        }
        let inserted = (self.capacity - self.energy)
            .min(self.max_receive)
            .min(max_requested)
            .max(0);
        if !simulate {
            self.energy += inserted;
        }
        inserted
    }

    /// NeoForge EnergyStorage.extractEnergy: respect side's handler cap.
    pub fn extract(&mut self, max_requested: i32, simulate: bool) -> i32 {
        if !self.can_extract() || max_requested <= 0 {
            return 0;
        }
        let extracted = self.energy.min(self.max_extract).min(max_requested).max(0);
        if !simulate {
            self.energy -= extracted;
        }
        extracted
    }

    /// `InternalEnergyStorage.internalConsumeEnergy` ignores transfer caps
    /// and returns the FE actually consumed. Used by electric motors.
    pub fn consume_internal(&mut self, amount: i32) -> i32 {
        let before = self.energy;
        self.energy = self.energy.wrapping_sub(amount).max(0);
        before.wrapping_sub(self.energy)
    }

    /// `InternalEnergyStorage.internalProduceEnergy` ignores transfer caps,
    /// bounded by internal capacity. Used by the alternator.
    pub fn produce_internal(&mut self, amount: i32) -> i32 {
        let before = self.energy;
        self.energy = self.energy.wrapping_add(amount).min(self.capacity);
        self.energy.wrapping_sub(before)
    }

    /// `InternalEnergyStorage.write` adds `energy` to the block's NBT.
    /// Caller merges this field with the other Java block-entity tags.
    pub fn energy_nbt(self) -> Vec<u8> {
        fastnbt::to_bytes(&SavedEnergy {
            energy: self.energy,
        })
        .unwrap_or_default()
    }

    /// Read `energy` from the Java named-root compound.
    pub fn read_energy_nbt(&mut self, bytes: &[u8]) -> Result<(), fastnbt::error::Error> {
        let saved: SavedEnergy = fastnbt::from_bytes(bytes)?;
        // Java setEnergy does not clamp when restoring NBT.
        self.energy = saved.energy;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;

    #[test]
    fn electric_motor_buffer_accepts_input_but_no_public_extract() {
        // Original ElectricMotorBlockEntity constructor:
        // capacity 5000; maxReceive 5000; maxExtract 0.
        let mut energy = EnergyStore::new(5000, 5000, 0);
        assert!(energy.can_receive());
        assert!(!energy.can_extract());
        assert_eq!(energy.receive(5500, true), 5000);
        assert_eq!(energy.energy, 0);
        assert_eq!(energy.receive(1000, false), 1000);
        assert_eq!(energy.extract(100, false), 0);
        assert_eq!(energy.consume_internal(480), 480);
        assert_eq!(energy.energy, 520);
        assert_eq!(energy.consume_internal(800), 520);
        assert_eq!(energy.energy, 0);
    }

    #[test]
    fn alternator_generates_then_exposes_capped_transfer() {
        // Original AlternatorBlockEntity constructor:
        // capacity 5000; maxReceive 0; maxExtract 5000.
        let mut energy = EnergyStore::new(5000, 0, 5000);
        assert!(!energy.can_receive());
        assert!(energy.can_extract());
        assert_eq!(energy.produce_internal(4000), 4000);
        assert_eq!(energy.produce_internal(360), 360);
        assert_eq!(energy.produce_internal(1000), 640);
        assert_eq!(energy.energy, 5000);
        assert_eq!(energy.extract(800, true), 800);
        assert_eq!(energy.energy, 5000);
        assert_eq!(energy.extract(800, false), 800);
        assert_eq!(energy.energy, 4200);
    }

    #[test]
    fn saved_energy_uses_exact_java_key_and_preserves_nonclamped_value() {
        let mut motor = EnergyStore::new(5000, 5000, 0);
        motor.receive(1200, false);
        let bytes = motor.energy_nbt();
        let Value::Compound(nbt) = fastnbt::from_bytes::<Value>(&bytes).expect("valid NBT") else {
            panic!("expected compound");
        };
        assert_eq!(nbt.get("energy"), Some(&Value::Int(1200)));
        let mut restored = EnergyStore::new(5000, 5000, 0);
        restored.read_energy_nbt(&bytes).expect("read energy");
        assert_eq!(restored.energy, 1200);
        let raw = fastnbt::to_bytes(&SavedEnergy { energy: 7000 }).expect("NBT");
        restored.read_energy_nbt(&raw).expect("read energy");
        assert_eq!(restored.energy, 7000);
    }
}
