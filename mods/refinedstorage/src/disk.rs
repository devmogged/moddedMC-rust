//! Persistent Refined Storage 2.0.9 item-disk contents. The source variants
//! use 1024, 4096, 16384 and 65536 item units. Each drive has eight slots.
//! Every inventory mutation is a CAS of one complete drive block entity.

use crate::stack::Stack;
use fastnbt::Value;
use moddedmc_api::{Pos, content::World};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const KEY: &str = "refinedstorage_disks";
const STORAGE_BLOCK_KEY: &str = "refinedstorage_storage_block";
pub const DRIVE_SLOTS: usize = 8;
const MAX_TYPES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    OneK,
    FourK,
    SixteenK,
    SixtyFourK,
}

impl Tier {
    pub fn from_item(id: &str) -> Option<Self> {
        match id {
            "refinedstorage:1k_storage_disk" => Some(Self::OneK),
            "refinedstorage:4k_storage_disk" => Some(Self::FourK),
            "refinedstorage:16k_storage_disk" => Some(Self::SixteenK),
            "refinedstorage:64k_storage_disk" => Some(Self::SixtyFourK),
            _ => None,
        }
    }
    pub fn item(self) -> &'static str {
        match self {
            Self::OneK => "refinedstorage:1k_storage_disk",
            Self::FourK => "refinedstorage:4k_storage_disk",
            Self::SixteenK => "refinedstorage:16k_storage_disk",
            Self::SixtyFourK => "refinedstorage:64k_storage_disk",
        }
    }
    pub fn from_part(id: &str) -> Option<Self> {
        match id {
            "refinedstorage:1k_storage_part" => Some(Self::OneK),
            "refinedstorage:4k_storage_part" => Some(Self::FourK),
            "refinedstorage:16k_storage_part" => Some(Self::SixteenK),
            "refinedstorage:64k_storage_part" => Some(Self::SixtyFourK),
            _ => None,
        }
    }
    pub fn from_block(id: &str) -> Option<Self> {
        match id {
            "refinedstorage:1k_storage_block" => Some(Self::OneK),
            "refinedstorage:4k_storage_block" => Some(Self::FourK),
            "refinedstorage:16k_storage_block" => Some(Self::SixteenK),
            "refinedstorage:64k_storage_block" => Some(Self::SixtyFourK),
            _ => None,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::OneK => "1k",
            Self::FourK => "4k",
            Self::SixteenK => "16k",
            Self::SixtyFourK => "64k",
        }
    }
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "1k" => Some(Self::OneK),
            "4k" => Some(Self::FourK),
            "16k" => Some(Self::SixteenK),
            "64k" => Some(Self::SixtyFourK),
            _ => None,
        }
    }
    pub fn capacity(self) -> u64 {
        match self {
            Self::OneK => 1024,
            Self::FourK => 4096,
            Self::SixteenK => 16384,
            Self::SixtyFourK => 65536,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Record {
    pub sample: Stack,
    pub count: u64,
}

#[derive(Clone, Debug)]
pub struct Disk {
    pub tier: Tier,
    pub items: Vec<Record>,
}

impl Disk {
    pub(crate) fn empty(tier: Tier) -> Self {
        Self {
            tier,
            items: Vec::new(),
        }
    }
    pub fn used(&self) -> u64 {
        self.items
            .iter()
            .fold(0u64, |acc, entry| acc.saturating_add(entry.count))
    }
    pub fn remaining(&self) -> u64 {
        self.tier.capacity().saturating_sub(self.used())
    }
    pub(crate) fn validate(&self) -> bool {
        self.items.len() <= MAX_TYPES
            && self.used() <= self.tier.capacity()
            && self.items.iter().all(|entry| {
                entry.count > 0
                    && entry.sample.count == 1
                    && Tier::from_item(&entry.sample.id).is_none()
            } /* storage disks cannot nest */)
    }
    pub(crate) fn insert(&mut self, item: &Stack, amount: u32) -> u32 {
        if Tier::from_item(&item.id).is_some() {
            return 0;
        }
        let accepted = amount.min(self.remaining().min(u64::from(u32::MAX)) as u32);
        if accepted == 0 {
            return 0;
        }
        if let Some(record) = self.items.iter_mut().find(|r| r.sample.same_item(item)) {
            record.count += u64::from(accepted);
        } else {
            if self.items.len() >= MAX_TYPES {
                return 0;
            }
            let Some(raw) = item.with_count(1) else {
                return 0;
            };
            let Some(sample) = Stack::parse(&raw) else {
                return 0;
            };
            self.items.push(Record {
                sample,
                count: u64::from(accepted),
            });
        }
        accepted
    }
    pub(crate) fn withdraw(&mut self, item: &Stack, amount: u32) -> u32 {
        let Some(idx) = self.items.iter().position(|r| r.sample.same_item(item)) else {
            return 0;
        };
        let taken = u64::from(amount).min(self.items[idx].count);
        self.items[idx].count -= taken;
        if self.items[idx].count == 0 {
            self.items.remove(idx);
        }
        taken as u32
    }
    fn encode(&self) -> Option<Compound> {
        self.validate().then_some(())?;
        let items = self
            .items
            .iter()
            .map(|record| {
                let mut template = record.sample.key.clone();
                template.insert("count".into(), Value::Int(1));
                Value::Compound(Compound::from([
                    ("item".into(), Value::Compound(template)),
                    (
                        "amount".into(),
                        Value::Long(i64::try_from(record.count).unwrap_or(0)),
                    ),
                ]))
            })
            .collect();
        Some(Compound::from([
            ("tier".into(), Value::String(self.tier.name().into())),
            ("items".into(), Value::List(items)),
        ]))
    }
    fn decode(fields: &Compound) -> Option<Self> {
        let Value::String(tier) = fields.get("tier")? else {
            return None;
        };
        let tier = Tier::from_name(tier)?;
        let Value::List(entries) = fields.get("items")? else {
            return None;
        };
        if entries.len() > MAX_TYPES {
            return None;
        }
        let mut items = Vec::<Record>::with_capacity(entries.len());
        for entry in entries {
            let Value::Compound(fields) = entry else {
                return None;
            };
            let (Some(Value::Compound(template)), Some(Value::Long(count))) =
                (fields.get("item"), fields.get("amount"))
            else {
                return None;
            };
            let sample = Stack::parse(&fastnbt::to_bytes(template).ok()?)?;
            if *count <= 0 || items.iter().any(|r| r.sample.same_item(&sample)) {
                return None;
            }
            items.push(Record {
                sample,
                count: u64::try_from(*count).ok()?,
            });
        }
        let disk = Self { tier, items };
        disk.validate().then_some(disk)
    }
    pub(crate) fn craft(
        &mut self,
        ingredients: &[Option<Stack>],
        result: &Stack,
        remainders: &[Stack],
    ) -> bool {
        let before = self.clone();
        for input in ingredients.iter().flatten() {
            if self.withdraw(input, 1) != 1 {
                *self = before;
                return false;
            }
        }
        for extra in remainders.iter().chain(std::iter::once(result)) {
            if self.insert(extra, extra.count) != extra.count {
                *self = before;
                return false;
            }
        }
        true
    }
}

#[derive(Clone, Debug)]
pub struct Drive {
    pub disks: Vec<Option<Disk>>,
}
impl Default for Drive {
    fn default() -> Self {
        Self {
            disks: vec![None; DRIVE_SLOTS],
        }
    }
}

impl Drive {
    pub fn capacity(&self) -> u64 {
        self.disks
            .iter()
            .filter_map(Option::as_ref)
            .map(|d| d.tier.capacity())
            .sum()
    }
    pub fn used(&self) -> u64 {
        self.disks
            .iter()
            .filter_map(Option::as_ref)
            .map(Disk::used)
            .sum()
    }
    pub fn has_disk(&self) -> bool {
        self.disks.iter().any(Option::is_some)
    }
    pub fn install(&mut self, disk: Disk) -> bool {
        if !disk.validate() {
            return false;
        }
        let Some(slot) = self.disks.iter_mut().find(|entry| entry.is_none()) else {
            return false;
        };
        *slot = Some(disk);
        true
    }
    pub fn eject(&mut self, slot: usize) -> Option<Disk> {
        self.disks.get_mut(slot)?.take()
    }
    /// Source's storage_disk_upgrade recipe replaces the capacity tier while
    /// keeping existing contents. Expose that upgrade via a held storage part
    /// until the host adds a custom recipe serializer.
    pub fn upgrade_first(&mut self, next: Tier) -> bool {
        let Some(disk) = self
            .disks
            .iter_mut()
            .flatten()
            .find(|disk| disk.tier.capacity() < next.capacity())
        else {
            return false;
        };
        disk.tier = next;
        true
    }
    pub fn insert(&mut self, item: &Stack, requested: u32) -> u32 {
        let mut moved = 0;
        for disk in self.disks.iter_mut().flatten() {
            moved += disk.insert(item, requested - moved);
            if moved == requested {
                break;
            }
        }
        moved
    }
    pub fn withdraw(&mut self, item: &Stack, requested: u32) -> u32 {
        let mut moved = 0;
        for disk in self.disks.iter_mut().flatten() {
            moved += disk.withdraw(item, requested - moved);
            if moved == requested {
                break;
            }
        }
        moved
    }
    pub fn count(&self, item: &Stack) -> u64 {
        self.disks
            .iter()
            .flatten()
            .flat_map(|disk| disk.items.iter())
            .filter(|entry| entry.sample.same_item(item))
            .map(|entry| entry.count)
            .sum()
    }
    pub fn records(&self) -> impl Iterator<Item = &Record> {
        self.disks
            .iter()
            .flatten()
            .flat_map(|disk| disk.items.iter())
    }
    /// Transactional crafting within this *single drive block entity*. Every
    /// ingredient and its real recipe remainder must fit this CAS snapshot.
    pub fn craft(
        &mut self,
        ingredients: &[Option<Stack>],
        result: &Stack,
        remainders: &[Stack],
    ) -> bool {
        let previous = self.clone();
        for input in ingredients.iter().flatten() {
            if self.withdraw(input, 1) != 1 {
                *self = previous;
                return false;
            }
        }
        for remainder in remainders.iter().chain(std::iter::once(result)) {
            if self.insert(remainder, remainder.count) != remainder.count {
                *self = previous;
                return false;
            }
        }
        true
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.disks.len() != DRIVE_SLOTS {
            return None;
        }
        let entries = self
            .disks
            .iter()
            .map(|slot| match slot {
                Some(disk) => disk.encode().map(Value::Compound),
                None => Some(Value::Compound(Compound::new())),
            })
            .collect::<Option<Vec<_>>>()?;
        root.insert(
            KEY.into(),
            Value::Compound(Compound::from([
                ("v".into(), Value::Int(1)),
                ("disks".into(), Value::List(entries)),
            ])),
        );
        Some(())
    }
    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(stored) = root.get(KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(stored) = stored else {
            return None;
        };
        let (Some(Value::Int(1)), Some(Value::List(disks))) =
            (stored.get("v"), stored.get("disks"))
        else {
            return None;
        };
        if disks.len() != DRIVE_SLOTS {
            return None;
        }
        let mut slots = Vec::with_capacity(DRIVE_SLOTS);
        for item in disks {
            let Value::Compound(raw) = item else {
                return None;
            };
            slots.push(if raw.is_empty() {
                None
            } else {
                Some(Disk::decode(raw)?)
            });
        }
        Some(Self { disks: slots })
    }
}

/// Loaded named-root block data, preserving all foreign fields on each CAS.
pub fn load(world: World<'_>, pos: Pos) -> Option<Drive> {
    let snapshot = world.entity_snapshot(pos)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Drive::decode(&root)
}
pub fn update<T>(
    world: World<'_>,
    pos: Pos,
    apply: impl FnOnce(&mut Drive) -> Option<T>,
) -> Option<T> {
    let snapshot = world.entity_snapshot(pos)?;
    let mut root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    let mut drive = Drive::decode(&root)?;
    let output = apply(&mut drive)?;
    drive.encode(&mut root)?;
    let encoded = fastnbt::to_bytes(&root).ok()?;
    world
        .compare_entity(pos, &snapshot, &encoded)
        .then_some(output)
}

/// A disk item's private NBT carries its contents when moved between drives.
pub fn load_cartridge(item: &Stack) -> Option<Disk> {
    let tier = Tier::from_item(&item.id)?;
    if item.count != 1 {
        return None;
    }
    let Some(component) = item.key.get("components") else {
        return Some(Disk::empty(tier));
    };
    let Value::Compound(parts) = component else {
        return None;
    };
    let Some(custom) = parts.get("minecraft:custom_data") else {
        return Some(Disk::empty(tier));
    };
    let Value::Compound(data) = custom else {
        return None;
    };
    let Some(saved) = data.get("refinedstorage_disk") else {
        return Some(Disk::empty(tier));
    };
    let Value::Compound(raw) = saved else {
        return None;
    };
    let disk = Disk::decode(raw)?;
    (disk.tier == tier).then_some(disk)
}
pub fn cartridge_item(disk: &Disk) -> Option<Vec<u8>> {
    let mut data = Compound::from([
        ("id".into(), Value::String(disk.tier.item().into())),
        ("count".into(), Value::Int(1)),
    ]);
    data.insert(
        "components".into(),
        Value::Compound(Compound::from([(
            "minecraft:custom_data".into(),
            Value::Compound(Compound::from([(
                "refinedstorage_disk".into(),
                Value::Compound(disk.encode()?),
            )])),
        )])),
    );
    fastnbt::to_bytes(&data).ok()
}

/// A Storage Block has one permanently mounted capacity tier. Source RS 2
/// stores its cells through an external storage repository keyed by UUID;
/// until the host exposes that repository, persist one bounded cell inside
/// the block entity's own CAS NBT.
pub fn load_block(world: World<'_>, site: Pos, tier: Tier) -> Option<Disk> {
    let snapshot = world.entity_snapshot(site)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    decode_block(&root, tier)
}

fn decode_block(root: &Compound, tier: Tier) -> Option<Disk> {
    let Some(record) = root.get(STORAGE_BLOCK_KEY) else {
        return Some(Disk::empty(tier));
    };
    let Value::Compound(raw) = record else {
        return None;
    };
    let disk = Disk::decode(raw)?;
    (disk.tier == tier).then_some(disk)
}

pub fn update_block<T>(
    world: World<'_>,
    site: Pos,
    tier: Tier,
    apply: impl FnOnce(&mut Disk) -> Option<T>,
) -> Option<T> {
    let snapshot = world.entity_snapshot(site)?;
    let mut root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    let mut disk = decode_block(&root, tier)?;
    let result = apply(&mut disk)?;
    root.insert(STORAGE_BLOCK_KEY.into(), Value::Compound(disk.encode()?));
    let encoded = fastnbt::to_bytes(&root).ok()?;
    world
        .compare_entity(site, &snapshot, &encoded)
        .then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str, count: i32, damage: i32) -> Stack {
        Stack::parse(
            &fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(count)),
                (
                    "components".into(),
                    Value::Compound(Compound::from([(
                        "minecraft:damage".into(),
                        Value::Int(damage),
                    )])),
                ),
            ]))
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn official_disk_capacities_and_8_slots_bound_storage() {
        assert_eq!(Tier::OneK.capacity(), 1024);
        assert_eq!(Tier::FourK.capacity(), 4096);
        assert_eq!(Tier::SixteenK.capacity(), 16384);
        assert_eq!(Tier::SixtyFourK.capacity(), 65536);
        let mut drive = Drive::default();
        assert_eq!(drive.capacity(), 0);
        for _ in 0..DRIVE_SLOTS {
            assert!(drive.install(Disk::empty(Tier::OneK)));
        }
        assert!(!drive.install(Disk::empty(Tier::OneK)));
        assert_eq!(drive.capacity(), 8192);
    }
    #[test]
    fn durable_components_capacity_and_cartridge_move() {
        let sword = item("minecraft:diamond_sword", 1, 8);
        let other = item("minecraft:diamond_sword", 1, 9);
        let mut drive = Drive::default();
        assert!(drive.install(Disk::empty(Tier::OneK)));
        assert_eq!(drive.insert(&sword, 100), 100);
        assert_eq!(drive.insert(&other, 50), 50);
        assert_eq!(drive.count(&sword), 100);
        assert_eq!(drive.count(&other), 50);
        let mut root = Compound::from([("foreign".into(), Value::Int(123))]);
        drive.encode(&mut root).unwrap();
        let persisted: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        assert_eq!(persisted.get("foreign"), Some(&Value::Int(123)));
        let mut restored = Drive::decode(&persisted).unwrap();
        assert_eq!(restored.withdraw(&sword, 20), 20);
        assert_eq!(restored.count(&sword), 80);
        let cartridge = restored.eject(0).unwrap();
        let encoded = cartridge_item(&cartridge).unwrap();
        let opened = load_cartridge(&Stack::parse(&encoded).unwrap()).unwrap();
        assert_eq!(opened.items.len(), 2);
        assert_eq!(opened.used(), 130);
    }
    #[test]
    fn crafting_consumes_real_ingredients_and_rolls_back_if_output_full() {
        let diamond = item("minecraft:diamond", 1, 0);
        let gold = item("minecraft:gold_ingot", 1, 0);
        let result = item("minecraft:diamond_block", 1, 0);
        let mut drive = Drive::default();
        assert!(drive.install(Disk::empty(Tier::OneK)));
        assert_eq!(drive.insert(&diamond, 9), 9);
        assert_eq!(drive.insert(&gold, 1), 1);
        assert!(drive.craft(&vec![Some(diamond.clone()); 9], &result, &[]));
        assert_eq!(drive.count(&diamond), 0);
        assert_eq!(drive.count(&result), 1);
        assert_eq!(drive.count(&gold), 1);
        assert!(!drive.craft(&vec![Some(diamond.clone()); 9], &result, &[]));
        assert_eq!(drive.count(&result), 1);
        assert_eq!(drive.insert(&gold, 1022), 1022);
        assert_eq!(drive.insert(&gold, 1), 0);
        assert!(!drive.craft(&[Some(gold.clone())], &result, &[diamond.clone()]));
        assert_eq!(drive.count(&gold), 1023);
    }
    #[test]
    fn corrupt_disk_capacity_and_malformed_item_fail_closed() {
        let mut drive = Drive::default();
        assert!(drive.install(Disk::empty(Tier::OneK)));
        let stone = item("minecraft:stone", 1, 0);
        assert_eq!(drive.insert(&stone, 1024), 1024);
        assert_eq!(drive.insert(&stone, 1), 0);
        let mut nbt = Compound::new();
        drive.encode(&mut nbt).unwrap();
        let Value::Compound(stored) = nbt.get_mut(KEY).unwrap() else {
            unreachable!()
        };
        let Value::List(entries) = stored.get_mut("disks").unwrap() else {
            unreachable!()
        };
        let Value::Compound(disk) = &mut entries[0] else {
            unreachable!()
        };
        let Value::List(items) = disk.get_mut("items").unwrap() else {
            unreachable!()
        };
        let Value::Compound(record) = &mut items[0] else {
            unreachable!()
        };
        record.insert("amount".into(), Value::Long(1025));
        assert!(Drive::decode(&nbt).is_none());
    }

    #[test]
    fn disk_upgrade_keeps_contents_and_refuses_downgrade() {
        let diamond = item("minecraft:diamond", 1, 0);
        let mut drive = Drive::default();
        assert!(drive.install(Disk::empty(Tier::OneK)));
        assert_eq!(drive.insert(&diamond, 1019), 1019);
        assert!(drive.upgrade_first(Tier::FourK));
        assert_eq!(drive.capacity(), 4096);
        assert_eq!(drive.count(&diamond), 1019);
        assert!(!drive.upgrade_first(Tier::OneK));
        assert!(drive.upgrade_first(Tier::SixtyFourK));
        assert_eq!(drive.capacity(), 65536);
        let mut nbt = Compound::new();
        drive.encode(&mut nbt).unwrap();
        let restored = Drive::decode(&nbt).unwrap();
        assert_eq!(restored.count(&diamond), 1019);
        assert_eq!(restored.capacity(), 65536);
    }
    #[test]
    fn storage_block_rejects_foreign_capacity_or_tampered_records() {
        let gold = item("minecraft:gold_ingot", 1, 0);
        let tier = Tier::FourK;
        let mut cell = decode_block(&Compound::new(), tier).unwrap();
        assert_eq!(cell.tier, tier);
        assert_eq!(cell.insert(&gold, 4096), 4096);
        assert_eq!(cell.insert(&gold, 1), 0);
        let root = Compound::from([
            ("foreign".into(), Value::Byte(7)),
            (
                STORAGE_BLOCK_KEY.into(),
                Value::Compound(cell.encode().unwrap()),
            ),
        ]);
        let restored = decode_block(&root, tier).unwrap();
        assert_eq!(restored.used(), 4096);
        assert!(decode_block(&root, Tier::OneK).is_none());
        assert_eq!(root.get("foreign"), Some(&Value::Byte(7)));
        assert!(restored.clone().craft(
            &[Some(gold.clone())],
            &item("minecraft:diamond", 1, 0),
            &[],
        ));
    }
}
