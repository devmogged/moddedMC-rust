//! `SimpleNetworkEnergyStorage` from Create: New Age 1.2.0.
//! The original is a 64-bit snapshot participant. Host transaction and energy
//! capabilities will wrap this core after WS-4-01/WS-0 publishes services.

/// New Age's network energy storage, distinct from Crafts & Additions'
/// integer NeoForge `InternalEnergyStorage`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimpleNetworkEnergyStorage {
    pub capacity: i64,
    pub stored: i64,
    pub supports_insertion: bool,
    pub supports_extraction: bool,
    pub max_insert: i64,
    pub max_extract: i64,
}

/// The original transaction creates a snapshot before each external
/// modification and can restore it if the enclosing transaction aborts.
/// Lifecycle/rollback callbacks are owned by the host, not this struct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot(pub i64);

impl SimpleNetworkEnergyStorage {
    /// Constructor throws `IllegalArgumentException` for negative capacity.
    pub fn new(capacity: i64) -> Option<Self> {
        (capacity >= 0).then_some(Self {
            capacity,
            stored: 0,
            supports_insertion: true,
            supports_extraction: true,
            max_insert: i64::MAX,
            max_extract: i64::MAX,
        })
    }

    pub fn snapshot(self) -> Snapshot {
        Snapshot(self.stored)
    }

    pub fn restore(&mut self, snapshot: Snapshot) {
        self.stored = snapshot.0;
    }

    pub fn set_capacity(&mut self, capacity: i64) {
        self.capacity = capacity;
        // Exact Java setter: capacity changes trim the stored amount.
        self.stored = self.stored.min(capacity);
    }

    pub fn set_stored_energy(&mut self, amount: i64) {
        self.stored = amount.max(0).min(self.capacity);
    }

    pub fn set_supports_insertion(&mut self, supported: bool) {
        self.supports_insertion = supported;
    }

    pub fn set_supports_extraction(&mut self, supported: bool) {
        self.supports_extraction = supported;
    }

    pub fn set_max_insert(&mut self, limit: i64) {
        self.max_insert = limit;
    }

    pub fn set_max_extract(&mut self, limit: i64) {
        self.max_extract = limit;
    }

    /// External insert: apply supportsInsertion, per-operation limit,
    /// available capacity and allow the host to restore a snapshot on abort.
    pub fn insert(&mut self, amount: i64) -> i64 {
        if !self.supports_insertion {
            return 0;
        }
        let inserted = amount.min(self.max_insert).min(self.capacity - self.stored);
        self.stored = self.stored.wrapping_add(inserted);
        inserted
    }

    /// External extraction with supportsExtraction and maxExtract cap.
    pub fn extract(&mut self, amount: i64) -> i64 {
        if !self.supports_extraction {
            return 0;
        }
        let extracted = amount.min(self.max_extract).min(self.stored);
        self.stored = self.stored.wrapping_sub(extracted);
        extracted
    }

    /// Internal generator transfer bypasses supportsInsertion and maxInsert.
    /// `simulated` must not modify the stored energy.
    pub fn internal_insert(&mut self, amount: i64, simulated: bool) -> i64 {
        let inserted = amount.min(self.capacity - self.stored);
        if !simulated {
            self.stored = self.stored.wrapping_add(inserted);
        }
        inserted
    }

    /// Internal consumer transfer bypasses supportsExtraction and maxExtract.
    pub fn internal_extract(&mut self, amount: i64, simulated: bool) -> i64 {
        let extracted = amount.min(self.stored);
        if !simulated {
            self.stored = self.stored.wrapping_sub(extracted);
        }
        extracted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_rejects_negative_capacity_and_setter_clamps_stored() {
        assert!(SimpleNetworkEnergyStorage::new(-1).is_none());
        let mut storage = SimpleNetworkEnergyStorage::new(100).expect("capacity");
        storage.set_stored_energy(200);
        assert_eq!(storage.stored, 100);
        storage.set_capacity(40);
        assert_eq!(storage.stored, 40);
        storage.set_stored_energy(-1);
        assert_eq!(storage.stored, 0);
    }

    #[test]
    fn external_energy_transfer_honors_direction_and_per_call_limits() {
        let mut storage = SimpleNetworkEnergyStorage::new(100).expect("capacity");
        storage.set_max_insert(30);
        storage.set_max_extract(20);
        assert_eq!(storage.insert(100), 30);
        assert_eq!(storage.insert(100), 30);
        assert_eq!(storage.insert(100), 30);
        assert_eq!(storage.insert(100), 10);
        assert_eq!(storage.extract(80), 20);
        assert_eq!(storage.stored, 80);
        storage.set_supports_insertion(false);
        assert_eq!(storage.insert(1), 0);
        storage.set_supports_extraction(false);
        assert_eq!(storage.extract(1), 0);
        assert_eq!(storage.stored, 80);
    }

    #[test]
    fn internal_transfer_bypasses_external_limits_and_simulates() {
        let mut storage = SimpleNetworkEnergyStorage::new(100).expect("capacity");
        storage.set_supports_insertion(false);
        storage.set_supports_extraction(false);
        storage.set_max_insert(0);
        storage.set_max_extract(0);
        assert_eq!(storage.internal_insert(80, true), 80);
        assert_eq!(storage.stored, 0);
        assert_eq!(storage.internal_insert(80, false), 80);
        assert_eq!(storage.internal_insert(80, false), 20);
        assert_eq!(storage.internal_extract(90, true), 90);
        assert_eq!(storage.stored, 100);
        assert_eq!(storage.internal_extract(90, false), 90);
        assert_eq!(storage.stored, 10);
    }

    #[test]
    fn snapshot_restore_rolls_back_energy_to_transaction_start() {
        let mut storage = SimpleNetworkEnergyStorage::new(100).expect("capacity");
        let beginning = storage.snapshot();
        assert_eq!(storage.insert(50), 50);
        assert_eq!(storage.extract(20), 20);
        assert_eq!(storage.stored, 30);
        storage.restore(beginning);
        assert_eq!(storage.stored, 0);
    }

    #[test]
    fn network_conductivity_and_receiver_capacity_conserve_energy() {
        use crate::electrical_network::ElectricalNetwork;
        let mut network = ElectricalNetwork::new(24);
        network.add_connection(1, 2, 40);
        network.tick();
        let mut receiver = SimpleNetworkEnergyStorage::new(100).expect("capacity");
        receiver.set_max_insert(30);
        // Sender may transfer only the amount accepted by the target's
        // external storage. That acceptance is simulated before decrementing
        // edge conductivity, preserving the original transaction's intent.
        let max_accept =
            receiver.capacity.min(receiver.stored + receiver.max_insert) - receiver.stored;
        let route = network.insert(1, 2, 60, max_accept);
        assert_eq!(route, 30);
        assert_eq!(receiver.insert(route), 30);
        assert_eq!(receiver.stored, 30);
        assert_eq!(network.insert(1, 2, 60, 30), 10);
        assert_eq!(receiver.insert(10), 10);
        assert_eq!(receiver.stored, 40);
        assert_eq!(network.insert(1, 2, 10, 10), 0);
    }
}
