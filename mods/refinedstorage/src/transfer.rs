//! External Storage inventories are physical IItemHandlers. Importers and
//! exporters transfer between them using the host's exact-full-source-NBT
//! slot transaction. Virtual disks never participate in these host transfers.

use crate::{
    network::{self, Network},
    stack::Stack,
};
use fastnbt::Value;
use moddedmc_api::{
    Pos,
    containers::{Containers, Transfers},
    content::World,
};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const POLICY: &str = "refinedstorage_port";
const EXTERNAL_POLICY: &str = "refinedstorage_external";

#[derive(Clone, Debug)]
pub struct External {
    pub priority: i32,
    pub allowlist: bool,
    pub filters: Vec<Stack>,
    pub can_insert: bool,
    pub can_extract: bool,
    pub needs_signal: bool,
}
impl Default for External {
    fn default() -> Self {
        Self {
            priority: 0,
            allowlist: false,
            filters: Vec::new(),
            can_insert: true,
            can_extract: true,
            needs_signal: false,
        }
    }
}
impl External {
    pub fn allows(&self, item: &Stack) -> bool {
        if self.filters.is_empty() {
            return !self.allowlist;
        }
        let found = self.filters.iter().any(|pattern| pattern.same_item(item));
        if self.allowlist { found } else { !found }
    }
    pub fn add(&mut self, item: &Stack) -> bool {
        if self.filters.len() >= 9 || self.filters.iter().any(|pattern| pattern.same_item(item)) {
            return false;
        }
        let Some(bytes) = item.with_count(1) else {
            return false;
        };
        let Some(pattern) = Stack::parse(&bytes) else {
            return false;
        };
        self.filters.push(pattern);
        true
    }
    pub fn enabled(&self, signal: bool) -> bool {
        !self.needs_signal || signal
    }
    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(raw) = root.get(EXTERNAL_POLICY) else {
            return Some(Self::default());
        };
        let Value::Compound(raw) = raw else {
            return None;
        };
        let (
            Some(Value::Int(priority)),
            Some(Value::Byte(allow)),
            Some(Value::Byte(insert)),
            Some(Value::Byte(extract)),
            Some(Value::Byte(signal)),
            Some(Value::List(patterns)),
        ) = (
            raw.get("priority"),
            raw.get("allow"),
            raw.get("insert"),
            raw.get("extract"),
            raw.get("signal"),
            raw.get("filters"),
        )
        else {
            return None;
        };
        if !(-1000..=1000).contains(priority) || patterns.len() > 9 {
            return None;
        }
        let mut filters = Vec::with_capacity(patterns.len());
        for pattern in patterns {
            let Value::Compound(fields) = pattern else {
                return None;
            };
            let stack = Stack::parse(&fastnbt::to_bytes(fields).ok()?)?;
            if stack.count != 1
                || filters
                    .iter()
                    .any(|previous: &Stack| previous.same_item(&stack))
            {
                return None;
            }
            filters.push(stack);
        }
        Some(Self {
            priority: *priority,
            allowlist: *allow != 0,
            filters,
            can_insert: *insert != 0,
            can_extract: *extract != 0,
            needs_signal: *signal != 0,
        })
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if !(-1000..=1000).contains(&self.priority) || self.filters.len() > 9 {
            return None;
        }
        let mut filters = Vec::with_capacity(self.filters.len());
        for item in &self.filters {
            let mut data = item.key.clone();
            data.insert("count".into(), Value::Int(1));
            filters.push(Value::Compound(data));
        }
        root.insert(
            EXTERNAL_POLICY.into(),
            Value::Compound(Compound::from([
                ("priority".into(), Value::Int(self.priority)),
                ("allow".into(), Value::Byte(i8::from(self.allowlist))),
                ("insert".into(), Value::Byte(i8::from(self.can_insert))),
                ("extract".into(), Value::Byte(i8::from(self.can_extract))),
                ("signal".into(), Value::Byte(i8::from(self.needs_signal))),
                ("filters".into(), Value::List(filters)),
            ])),
        );
        Some(())
    }
}
pub fn load_external(world: World<'_>, site: Pos) -> Option<External> {
    let snapshot = world.entity_snapshot(site)?;
    let fields: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    External::decode(&fields)
}
pub fn update_external(
    world: World<'_>,
    site: Pos,
    mutate: impl FnOnce(&mut External) -> bool,
) -> bool {
    let Some(snapshot) = world.entity_snapshot(site) else {
        return false;
    };
    let Ok(mut fields) = fastnbt::from_bytes::<Compound>(&snapshot.nbt) else {
        return false;
    };
    let Some(mut policy) = External::decode(&fields) else {
        return false;
    };
    if !mutate(&mut policy) || policy.encode(&mut fields).is_none() {
        return false;
    }
    let Ok(bytes) = fastnbt::to_bytes(&fields) else {
        return false;
    };
    world.compare_entity(site, &snapshot, &bytes)
}

#[derive(Clone, Debug)]
pub struct Port {
    pub allowlist: bool,
    pub filters: Vec<Stack>,
    pub needs_signal: bool,
    pub rate: u32,
    pub upgrades: Vec<Upgrade>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upgrade {
    Speed,
    Stack,
}

impl Upgrade {
    pub fn from_item(id: &str) -> Option<Self> {
        match id {
            "refinedstorage:speed_upgrade" => Some(Self::Speed),
            "refinedstorage:stack_upgrade" => Some(Self::Stack),
            _ => None,
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Self::Speed => "refinedstorage:speed_upgrade",
            Self::Stack => "refinedstorage:stack_upgrade",
        }
    }
}
impl Port {
    pub fn defaults(exporter: bool) -> Self {
        Self {
            allowlist: exporter,
            filters: Vec::new(),
            needs_signal: false,
            rate: 4,
            upgrades: Vec::new(),
        }
    }
    pub fn install(&mut self, upgrade: Upgrade) -> bool {
        if self.upgrades.len() >= 4
            || (upgrade == Upgrade::Stack && self.upgrades.contains(&upgrade))
        {
            return false;
        }
        self.upgrades.push(upgrade);
        self.rate = if self.upgrades.contains(&Upgrade::Stack) {
            64
        } else {
            4u32.saturating_mul(
                1 + self
                    .upgrades
                    .iter()
                    .filter(|&&u| u == Upgrade::Speed)
                    .count() as u32,
            )
            .min(64)
        };
        true
    }
    pub fn allows(&self, item: &Stack) -> bool {
        let found = self.filters.iter().any(|pattern| pattern.same_item(item));
        if self.filters.is_empty() {
            return !self.allowlist;
        }
        if self.allowlist { found } else { !found }
    }
    pub fn add(&mut self, item: &Stack) -> bool {
        if self.filters.len() >= 9 || self.filters.iter().any(|entry| entry.same_item(item)) {
            return false;
        }
        let Some(raw) = item.with_count(1) else {
            return false;
        };
        let Some(item) = Stack::parse(&raw) else {
            return false;
        };
        self.filters.push(item);
        true
    }
    pub fn enabled(&self, signal: bool) -> bool {
        !self.needs_signal || signal
    }
    pub fn decode(root: &Compound, exporter: bool) -> Option<Self> {
        let Some(saved) = root.get(POLICY) else {
            return Some(Self::defaults(exporter));
        };
        let Value::Compound(saved) = saved else {
            return None;
        };
        let (
            Some(Value::Byte(allow)),
            Some(Value::Byte(signal)),
            Some(Value::Int(rate)),
            Some(Value::List(items)),
        ) = (
            saved.get("allow"),
            saved.get("signal"),
            saved.get("rate"),
            saved.get("filters"),
        )
        else {
            return None;
        };
        let rate = u32::try_from(*rate).ok()?;
        if !(1..=64).contains(&rate) || items.len() > 9 {
            return None;
        }
        let mut upgrades = Vec::new();
        if let Some(upgrade_list) = saved.get("upgrades") {
            let Value::List(entries) = upgrade_list else {
                return None;
            };
            if entries.len() > 4 {
                return None;
            }
            for entry in entries {
                let Value::String(id) = entry else {
                    return None;
                };
                let upgrade = Upgrade::from_item(id)?;
                if upgrade == Upgrade::Stack && upgrades.contains(&upgrade) {
                    return None;
                }
                upgrades.push(upgrade);
            }
        }
        // The transfer rate must be fully explained by consumed upgrades.
        // A malformed saved rate cannot silently grant free throughput.
        let expected_rate = if upgrades.contains(&Upgrade::Stack) {
            64
        } else {
            4 * (1 + upgrades
                .iter()
                .filter(|&&upgrade| upgrade == Upgrade::Speed)
                .count() as u32)
        };
        if rate != expected_rate {
            return None;
        }
        let mut filters = Vec::with_capacity(items.len());
        for entry in items {
            let Value::Compound(raw) = entry else {
                return None;
            };
            let item = Stack::parse(&fastnbt::to_bytes(raw).ok()?)?;
            if item.count != 1 || filters.iter().any(|prior: &Stack| prior.same_item(&item)) {
                return None;
            }
            filters.push(item);
        }
        Some(Self {
            allowlist: *allow != 0,
            filters,
            needs_signal: *signal != 0,
            rate,
            upgrades,
        })
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.filters.len() > 9 || !(1..=64).contains(&self.rate) {
            return None;
        }
        let filters = self
            .filters
            .iter()
            .map(|item| {
                let mut fields = item.key.clone();
                fields.insert("count".into(), Value::Int(1));
                Value::Compound(fields)
            })
            .collect();
        root.insert(
            POLICY.into(),
            Value::Compound(Compound::from([
                ("allow".into(), Value::Byte(i8::from(self.allowlist))),
                ("signal".into(), Value::Byte(i8::from(self.needs_signal))),
                ("rate".into(), Value::Int(i32::try_from(self.rate).ok()?)),
                (
                    "upgrades".into(),
                    Value::List(
                        self.upgrades
                            .iter()
                            .map(|upgrade| Value::String(upgrade.id().into()))
                            .collect(),
                    ),
                ),
                ("filters".into(), Value::List(filters)),
            ])),
        );
        Some(())
    }
}

pub fn load(world: World<'_>, pos: Pos, exporter: bool) -> Option<Port> {
    let snapshot = world.entity_snapshot(pos)?;
    let nbt: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Port::decode(&nbt, exporter)
}
pub fn update(
    world: World<'_>,
    pos: Pos,
    exporter: bool,
    apply: impl FnOnce(&mut Port) -> bool,
) -> bool {
    let Some(snapshot) = world.entity_snapshot(pos) else {
        return false;
    };
    let Ok(mut nbt) = fastnbt::from_bytes::<Compound>(&snapshot.nbt) else {
        return false;
    };
    let Some(mut port) = Port::decode(&nbt, exporter) else {
        return false;
    };
    if !apply(&mut port) || port.encode(&mut nbt).is_none() {
        return false;
    }
    let Ok(encoded) = fastnbt::to_bytes(&nbt) else {
        return false;
    };
    world.compare_entity(pos, &snapshot, &encoded)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub pos: Pos,
    pub face: u8,
}
pub fn endpoint(site: Pos, direction: u8) -> Option<Endpoint> {
    Some(Endpoint {
        pos: network::neighbor(site, direction)?,
        face: direction ^ 1,
    })
}

pub trait Slots {
    fn size(&mut self, endpoint: Endpoint) -> Option<u32>;
    fn stack(&mut self, endpoint: Endpoint, slot: u32) -> Option<Vec<u8>>;
    fn transfer(
        &mut self,
        source: Endpoint,
        slot: u32,
        expected: &[u8],
        target: Endpoint,
        target_slot: u32,
        amount: u32,
    ) -> u32;
}
pub struct Live<'a> {
    pub containers: Containers,
    pub transactions: Transfers,
    pub world: World<'a>,
}
impl Slots for Live<'_> {
    fn size(&mut self, endpoint: Endpoint) -> Option<u32> {
        self.containers
            .at(self.world, endpoint.pos, endpoint.face)?
            .slots()
    }
    fn stack(&mut self, endpoint: Endpoint, slot: u32) -> Option<Vec<u8>> {
        self.containers
            .at(self.world, endpoint.pos, endpoint.face)?
            .stack(slot)
    }
    fn transfer(
        &mut self,
        source: Endpoint,
        slot: u32,
        expected: &[u8],
        target: Endpoint,
        target_slot: u32,
        amount: u32,
    ) -> u32 {
        self.transactions.transfer(
            self.world,
            (source.pos, source.face, slot),
            expected,
            (target.pos, target.face, target_slot),
            amount,
        )
    }
}

fn move_to(
    io: &mut impl Slots,
    source: Endpoint,
    source_slot: u32,
    target: Endpoint,
    original: &Stack,
    amount: u32,
) -> u32 {
    if amount == 0 || source.pos == target.pos {
        return 0;
    }
    let Some(slots) = io.size(target) else {
        return 0;
    };
    if slots > 256 {
        return 0;
    }
    let mut moved = 0;
    for target_slot in 0..slots {
        let left = amount - moved;
        if left == 0 {
            break;
        }
        let Some(full_nbt) = io.stack(source, source_slot) else {
            break;
        };
        let Some(now) = Stack::parse(&full_nbt) else {
            break;
        };
        if !now.same_item(original) || now.count > original.count.saturating_sub(moved) {
            break;
        }
        let count = io.transfer(
            source,
            source_slot,
            &full_nbt,
            target,
            target_slot,
            left.min(now.count),
        );
        if count > left.min(now.count) {
            break;
        }
        moved += count;
    }
    moved
}

/// One bounded import/export operation into another network-connected external
/// inventory. The caller chooses the physical source and ordered destinations.
pub fn route(io: &mut impl Slots, source: Endpoint, destinations: &[Endpoint], port: &Port) -> u32 {
    let unrestricted: Vec<_> = destinations
        .iter()
        .copied()
        .map(|endpoint| (endpoint, External::default()))
        .collect();
    route_configured(io, source, &unrestricted, port, None)
}

/// A configured External Storage interface can refuse input or extraction,
/// filter exact item components and set destination priority. Each actual
/// source/destination change still goes exclusively through host transfers.
pub fn route_configured(
    io: &mut impl Slots,
    source: Endpoint,
    destinations: &[(Endpoint, External)],
    port: &Port,
    source_policy: Option<&External>,
) -> u32 {
    let Some(source_slots) = io.size(source) else {
        return 0;
    };
    if source_slots > 256 {
        return 0;
    }
    for slot in 0..source_slots {
        let Some(raw) = io.stack(source, slot) else {
            continue;
        };
        let Some(item) = Stack::parse(&raw) else {
            continue;
        };
        if !port.allows(&item)
            || source_policy.is_some_and(|source| !source.can_extract || !source.allows(&item))
        {
            continue;
        }
        let mut moved = 0u32;
        for (target, policy) in destinations {
            if moved >= port.rate {
                break;
            }
            if !policy.can_insert || !policy.allows(&item) {
                continue;
            }
            moved += move_to(
                io,
                source,
                slot,
                *target,
                &item,
                port.rate.min(item.count) - moved,
            );
        }
        if moved > 0 {
            return moved;
        }
    }
    0
}

pub fn storage_interfaces(net: &Network) -> Vec<Endpoint> {
    let mut seen = std::collections::HashSet::new();
    net.external
        .iter()
        .filter_map(|&(site, face)| endpoint(site, face))
        .filter(|e| seen.insert(e.pos))
        .collect()
}

pub fn external_connections(world: World<'_>, net: &Network) -> Vec<(Endpoint, External)> {
    let connected: Vec<_> = net
        .external
        .iter()
        .filter_map(|&(site, face)| {
            let policy = load_external(world, site)?;
            if !policy.enabled(world.has_neighbor_signal(site)) {
                return None;
            }
            Some((endpoint(site, face)?, policy, site))
        })
        .collect();
    prioritize_external(connected)
}

fn prioritize_external(mut connected: Vec<(Endpoint, External, Pos)>) -> Vec<(Endpoint, External)> {
    connected.sort_by(|left, right| {
        right
            .1
            .priority
            .cmp(&left.1.priority)
            .then_with(|| (left.2.x, left.2.y, left.2.z).cmp(&(right.2.x, right.2.y, right.2.z)))
    });
    let mut seen = std::collections::HashSet::new();
    connected
        .into_iter()
        .filter_map(|(target, policy, _)| seen.insert(target.pos).then_some((target, policy)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Fake {
        bins: HashMap<Pos, Vec<Option<Vec<u8>>>>,
        tamper: bool,
    }
    impl Slots for Fake {
        fn size(&mut self, e: Endpoint) -> Option<u32> {
            Some(self.bins.get(&e.pos)?.len() as u32)
        }
        fn stack(&mut self, e: Endpoint, slot: u32) -> Option<Vec<u8>> {
            self.bins.get(&e.pos)?.get(slot as usize)?.clone()
        }
        fn transfer(
            &mut self,
            source: Endpoint,
            slot: u32,
            expected: &[u8],
            target: Endpoint,
            target_slot: u32,
            amount: u32,
        ) -> u32 {
            if self.tamper {
                self.tamper = false;
                self.bins.get_mut(&source.pos).unwrap()[slot as usize] =
                    Some(item("minecraft:gold_ingot", 9));
            }
            let Some(before) = self.stack(source, slot) else {
                return 0;
            };
            if before != expected {
                return 0;
            }
            let Some(stack) = Stack::parse(&before) else {
                return 0;
            };
            let Some(dest) = self
                .bins
                .get(&target.pos)
                .and_then(|b| b.get(target_slot as usize))
            else {
                return 0;
            };
            let available = match dest {
                Some(existing) => {
                    let Some(existing) = Stack::parse(existing) else {
                        return 0;
                    };
                    if !existing.same_item(&stack) {
                        return 0;
                    }
                    64u32.saturating_sub(existing.count)
                }
                None => 64,
            };
            let moved = amount.min(stack.count).min(available);
            if moved == 0 {
                return 0;
            }
            self.bins.get_mut(&source.pos).unwrap()[slot as usize] = (stack.count > moved)
                .then(|| stack.with_count(stack.count - moved))
                .flatten();
            let bin = self.bins.get_mut(&target.pos).unwrap();
            let previous = bin[target_slot as usize]
                .as_ref()
                .and_then(|b| Stack::parse(b))
                .map_or(0, |s| s.count);
            bin[target_slot as usize] = stack.with_count(previous + moved);
            moved
        }
    }
    fn p(x: i32) -> Pos {
        Pos { x, y: 80, z: 0 }
    }
    fn item(id: &str, count: i32) -> Vec<u8> {
        fastnbt::to_bytes(&Compound::from([
            ("id".into(), Value::String(id.into())),
            ("count".into(), Value::Int(count)),
        ]))
        .unwrap()
    }
    #[test]
    fn atomic_move_across_full_and_partial_slots_conserves_every_item() {
        let mut f = Fake::default();
        f.bins
            .insert(p(1), vec![Some(item("minecraft:iron_ingot", 10))]);
        f.bins.insert(
            p(2),
            vec![
                Some(item("minecraft:iron_ingot", 64)),
                Some(item("minecraft:iron_ingot", 63)),
                None,
            ],
        );
        let source = Endpoint { pos: p(1), face: 5 };
        let target = Endpoint { pos: p(2), face: 4 };
        assert_eq!(route(&mut f, source, &[target], &Port::defaults(false)), 4);
        assert_eq!(
            Stack::parse(f.bins[&p(1)][0].as_deref().unwrap())
                .unwrap()
                .count,
            6
        );
        assert_eq!(
            Stack::parse(f.bins[&p(2)][1].as_deref().unwrap())
                .unwrap()
                .count,
            64
        );
        assert_eq!(
            Stack::parse(f.bins[&p(2)][2].as_deref().unwrap())
                .unwrap()
                .count,
            3
        );
    }
    #[test]
    fn changed_full_source_preimage_fails_without_touching_destination() {
        let mut f = Fake {
            tamper: true,
            ..Default::default()
        };
        f.bins
            .insert(p(1), vec![Some(item("minecraft:iron_ingot", 10))]);
        f.bins.insert(p(2), vec![None]);
        let source = Endpoint { pos: p(1), face: 5 };
        let target = Endpoint { pos: p(2), face: 4 };
        assert_eq!(route(&mut f, source, &[target], &Port::defaults(false)), 0);
        assert!(f.bins[&p(2)][0].is_none());
        assert_eq!(
            Stack::parse(f.bins[&p(1)][0].as_deref().unwrap())
                .unwrap()
                .id,
            "minecraft:gold_ingot"
        );
    }
    #[test]
    fn port_filter_requires_configuration_for_export() {
        let diamond = Stack::parse(&item("minecraft:diamond", 1)).unwrap();
        let iron = Stack::parse(&item("minecraft:iron_ingot", 1)).unwrap();
        let mut export = Port::defaults(true);
        assert!(!export.allows(&diamond));
        assert!(export.add(&diamond));
        assert!(export.allows(&diamond));
        assert!(!export.allows(&iron));
        let mut nbt = Compound::new();
        export.encode(&mut nbt).unwrap();
        let restored = Port::decode(&nbt, true).unwrap();
        assert_eq!(restored.filters.len(), 1);
        assert!(restored.allows(&diamond));
    }
    #[test]
    fn upgrades_are_real_paid_items_with_persisted_rate() {
        let mut port = Port::defaults(false);
        assert!(port.install(Upgrade::Speed));
        assert_eq!(port.rate, 8);
        assert!(port.install(Upgrade::Speed));
        assert_eq!(port.rate, 12);
        assert!(port.install(Upgrade::Stack));
        assert_eq!(port.rate, 64);
        assert!(!port.install(Upgrade::Stack));
        let mut root = Compound::new();
        port.encode(&mut root).unwrap();
        let restored = Port::decode(&root, false).unwrap();
        assert_eq!(restored.rate, 64);
        assert_eq!(
            restored.upgrades,
            vec![Upgrade::Speed, Upgrade::Speed, Upgrade::Stack]
        );
        if let Some(Value::Compound(saved)) = root.get_mut(POLICY) {
            saved.insert("rate".into(), Value::Int(4));
        }
        assert!(Port::decode(&root, false).is_none());
    }

    #[test]
    fn external_policy_saved_access_filters_and_bounds_fail_closed() {
        let diamond = Stack::parse(&item("minecraft:diamond", 1)).unwrap();
        let iron = Stack::parse(&item("minecraft:iron_ingot", 1)).unwrap();
        let mut policy = External::default();
        assert!(policy.can_insert && policy.can_extract);
        assert!(policy.allows(&diamond));
        policy.allowlist = true;
        assert!(!policy.allows(&diamond));
        policy.allowlist = false;
        assert!(policy.allows(&diamond));
        let mut empty_allowlist = External::default();
        empty_allowlist.allowlist = true;
        let mut empty_nbt = Compound::new();
        empty_allowlist.encode(&mut empty_nbt).unwrap();
        let restored_empty_allowlist = External::decode(&empty_nbt).unwrap();
        assert!(!restored_empty_allowlist.allows(&diamond));
        assert!(policy.add(&diamond));
        policy.allowlist = true;
        policy.priority = -17;
        policy.can_extract = false;
        policy.needs_signal = true;
        assert!(policy.allows(&diamond));
        assert!(!policy.allows(&iron));
        assert!(!policy.enabled(false));
        assert!(policy.enabled(true));
        let mut nbt = Compound::from([("foreign".into(), Value::Int(37))]);
        policy.encode(&mut nbt).unwrap();
        let roundtrip: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&nbt).unwrap()).unwrap();
        let restored = External::decode(&roundtrip).unwrap();
        assert_eq!(restored.priority, -17);
        assert!(!restored.can_extract);
        assert!(restored.can_insert);
        assert!(restored.needs_signal);
        assert_eq!(restored.filters.len(), 1);
        assert_eq!(roundtrip.get("foreign"), Some(&Value::Int(37)));

        let mut malformed = roundtrip.clone();
        if let Some(Value::Compound(saved)) = malformed.get_mut(EXTERNAL_POLICY) {
            saved.insert("priority".into(), Value::Int(1001));
        }
        assert!(External::decode(&malformed).is_none());
        if let Some(Value::Compound(saved)) = malformed.get_mut(EXTERNAL_POLICY) {
            saved.insert("priority".into(), Value::Int(0));
            saved.insert("filters".into(), Value::String("wrong type".into()));
        }
        assert!(External::decode(&malformed).is_none());
    }

    #[test]
    fn external_insert_and_extract_permissions_enforced_before_host_transfer() {
        let source = Endpoint { pos: p(1), face: 5 };
        let sink = Endpoint { pos: p(2), face: 4 };
        let mut storage = Fake::default();
        storage
            .bins
            .insert(p(1), vec![Some(item("minecraft:diamond", 8))]);
        storage.bins.insert(p(2), vec![None]);
        let importer = Port::defaults(false);
        let mut source_policy = External::default();
        source_policy.can_extract = false;
        assert_eq!(
            route_configured(
                &mut storage,
                source,
                &[(sink, External::default())],
                &importer,
                Some(&source_policy),
            ),
            0
        );
        assert!(storage.bins[&p(2)][0].is_none());
        source_policy.can_extract = true;
        let mut sink_policy = External::default();
        sink_policy.can_insert = false;
        assert_eq!(
            route_configured(
                &mut storage,
                source,
                &[(sink, sink_policy.clone())],
                &importer,
                Some(&source_policy),
            ),
            0
        );
        sink_policy.can_insert = true;
        sink_policy.allowlist = true;
        assert_eq!(
            route_configured(
                &mut storage,
                source,
                &[(sink, sink_policy.clone())],
                &importer,
                Some(&source_policy),
            ),
            0
        );
        assert!(sink_policy.add(&Stack::parse(&item("minecraft:diamond", 1)).unwrap()));
        sink_policy.allowlist = false;
        assert_eq!(
            route_configured(
                &mut storage,
                source,
                &[(sink, sink_policy.clone())],
                &importer,
                Some(&source_policy),
            ),
            0
        );
        sink_policy.allowlist = true;
        assert_eq!(
            route_configured(
                &mut storage,
                source,
                &[(sink, sink_policy)],
                &importer,
                Some(&source_policy),
            ),
            4
        );
        assert_eq!(
            Stack::parse(storage.bins[&p(1)][0].as_deref().unwrap())
                .unwrap()
                .count,
            4,
        );
        assert_eq!(
            Stack::parse(storage.bins[&p(2)][0].as_deref().unwrap())
                .unwrap()
                .count,
            4,
        );
    }

    #[test]
    fn priority_order_and_shared_inventory_dedup_stable_across_discovery_orders() {
        let a = Endpoint { pos: p(1), face: 4 };
        let duplicate = Endpoint { pos: p(1), face: 5 };
        let b = Endpoint { pos: p(2), face: 4 };
        let c = Endpoint { pos: p(3), face: 4 };
        let mut high = External::default();
        high.priority = 10;
        let mut low = External::default();
        low.priority = -2;
        let mut tie = External::default();
        tie.priority = 10;
        let originals = vec![
            (b, low, p(9)),
            (c, tie, p(6)),
            (a, high.clone(), p(5)),
            (duplicate, high, p(4)),
        ];
        let output = prioritize_external(originals.clone());
        let reverse = prioritize_external(originals.into_iter().rev().collect());
        let expected = vec![a.pos, c.pos, b.pos];
        assert_eq!(
            output.iter().map(|entry| entry.0.pos).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            reverse.iter().map(|entry| entry.0.pos).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(output[0].0.face, duplicate.face);
        assert_eq!(output[0].1.priority, 10);
        assert_eq!(output[2].1.priority, -2);
    }
}
