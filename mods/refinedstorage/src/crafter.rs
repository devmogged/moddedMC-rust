//! Refined Storage 1.12.4 Crafter pattern container state.
//!
//! The source has nine single-pattern slots, four speed upgrades, facing-based
//! crafter chaining, and four redstone modes. This module owns only the block
//! entity's mod-local state; physical machine insertion and network crafting
//! remain disabled until the host can atomically debit digital storage and
//! deliver all inputs to a machine.

use crate::{
    autocrafting::RecipePattern,
    processing::{CraftingPatternItem, ProcessingPattern},
    stack::Stack,
};
use fastnbt::Value;
use moddedmc_api::{Pos, content::World};
use std::collections::{HashMap, HashSet};

type Compound = HashMap<String, Value>;
const KEY: &str = "refinedstorage_crafter";
const PATTERN_SLOTS: usize = 9;
const MAX_SPEED_UPGRADES: u8 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    #[default]
    Ignore = 0,
    SignalUnlocks = 1,
    SignalLocks = 2,
    PulseInsertsNextSet = 3,
}

impl Mode {
    fn from_id(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Ignore),
            1 => Some(Self::SignalUnlocks),
            2 => Some(Self::SignalLocks),
            3 => Some(Self::PulseInsertsNextSet),
            _ => None,
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Ignore => Self::SignalUnlocks,
            Self::SignalUnlocks => Self::SignalLocks,
            Self::SignalLocks => Self::PulseInsertsNextSet,
            Self::PulseInsertsNextSet => Self::Ignore,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Crafter {
    pub patterns: Vec<Option<InstalledPattern>>,
    pub speed_upgrades: u8,
    pub mode: Mode,
    pub locked: bool,
    pub was_powered: bool,
}

#[derive(Clone, Debug)]
pub enum InstalledPattern {
    Processing(ProcessingPattern),
    V1124Crafting(CraftingPatternItem),
    Crafting(RecipePattern),
}

impl InstalledPattern {
    pub fn from_stack(stack: &Stack) -> Option<Self> {
        if stack.id != "refinedstorage:pattern" || stack.count != 1 {
            return None;
        }
        let Value::Compound(components) = stack.key.get("components")? else {
            return None;
        };
        let Value::Compound(custom) = components.get("minecraft:custom_data")? else {
            return None;
        };
        let Value::ByteArray(bytes) = custom.get("moddedmc:data")? else {
            return None;
        };
        let data = bytes.iter().map(|value| *value as u8).collect::<Vec<_>>();
        if let Some(pattern) = ProcessingPattern::from_item_data(&data) {
            Some(Self::Processing(pattern))
        } else if let Some(pattern) = CraftingPatternItem::from_item_data(&data) {
            Some(Self::V1124Crafting(pattern))
        } else {
            RecipePattern::from_item_data(&data).map(Self::Crafting)
        }
    }
    pub fn item_stack(&self) -> Option<Vec<u8>> {
        match self {
            Self::Processing(pattern) => pattern.item_stack(),
            Self::V1124Crafting(pattern) => pattern.item_stack(),
            Self::Crafting(pattern) => pattern.item_stack(),
        }
    }
    pub fn output(&self) -> Option<&Stack> {
        match self {
            Self::Processing(pattern) => pattern.outputs.iter().flatten().next(),
            Self::V1124Crafting(pattern) => pattern.0.outputs.iter().flatten().next(),
            Self::Crafting(pattern) => Some(&pattern.result),
        }
    }
    pub fn same_pattern(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Processing(a), Self::Processing(b)) => a.same_pattern(b),
            (Self::V1124Crafting(a), Self::V1124Crafting(b)) => a.same_pattern(b),
            (Self::Crafting(a), Self::Crafting(b)) => a.same_recipe(b),
            _ => false,
        }
    }
}

impl Default for Crafter {
    fn default() -> Self {
        Self {
            patterns: vec![None; PATTERN_SLOTS],
            speed_upgrades: 0,
            mode: Mode::Ignore,
            locked: false,
            was_powered: false,
        }
    }
}

impl Crafter {
    pub fn install_pattern(&mut self, pattern: InstalledPattern) -> bool {
        if matches!(&pattern, InstalledPattern::Processing(processing) if !processing.valid())
            || matches!(&pattern, InstalledPattern::V1124Crafting(crafting) if !crafting.valid())
            || self
                .patterns
                .iter()
                .flatten()
                .any(|saved| saved.same_pattern(&pattern))
        {
            return false;
        }
        let Some(slot) = self.patterns.iter_mut().find(|entry| entry.is_none()) else {
            return false;
        };
        *slot = Some(pattern);
        true
    }

    pub fn eject_pattern(&mut self, index: usize) -> Option<InstalledPattern> {
        self.patterns.get_mut(index)?.take()
    }

    pub fn restore_pattern(&mut self, index: usize, pattern: InstalledPattern) -> bool {
        let Some(slot) = self.patterns.get_mut(index) else {
            return false;
        };
        if slot.is_some() {
            return false;
        }
        *slot = Some(pattern);
        true
    }

    pub fn install_speed_upgrade(&mut self) -> bool {
        if self.speed_upgrades >= MAX_SPEED_UPGRADES {
            return false;
        }
        self.speed_upgrades += 1;
        true
    }

    pub fn remove_speed_upgrade(&mut self) -> bool {
        if self.speed_upgrades == 0 {
            return false;
        }
        self.speed_upgrades -= 1;
        true
    }

    /// Source CrafterNetworkNode.getUpdateInterval().
    pub fn update_interval(&self) -> u64 {
        match self.speed_upgrades {
            0 => 10,
            1 => 8,
            2 => 6,
            3 => 4,
            4 => 2,
            _ => 10,
        }
    }

    /// Source CrafterNetworkNode.getMaximumSuccessfulCraftingUpdates().
    pub fn max_successful_updates(&self) -> u8 {
        self.speed_upgrades.saturating_add(1).min(5)
    }

    pub fn is_locked(&self, powered: bool) -> bool {
        match self.mode {
            Mode::Ignore => false,
            Mode::SignalUnlocks => !powered,
            Mode::SignalLocks => powered,
            Mode::PulseInsertsNextSet => self.locked,
        }
    }

    pub fn on_used_for_processing(&mut self) {
        if self.mode == Mode::PulseInsertsNextSet {
            self.locked = true;
        }
    }

    /// Source pulse mode remembers a rising powered interval and unlocks only
    /// when that signal subsequently falls.
    pub fn update_power(&mut self, powered: bool) {
        if self.mode != Mode::PulseInsertsNextSet {
            return;
        }
        if powered {
            self.was_powered = true;
        } else if self.was_powered {
            self.was_powered = false;
            self.locked = false;
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.was_powered = false;
        self.locked = false;
    }

    fn encode_pattern(pattern: &InstalledPattern) -> Option<Value> {
        let bytes = pattern.item_stack()?;
        let stack = Stack::parse(&bytes)?;
        let mut fields = stack.key;
        fields.insert("count".into(), Value::Int(1));
        Some(Value::Compound(fields))
    }

    fn decode_pattern(value: &Value) -> Option<InstalledPattern> {
        let Value::Compound(fields) = value else {
            return None;
        };
        let stack = Stack::parse(&fastnbt::to_bytes(fields).ok()?)?;
        InstalledPattern::from_stack(&stack)
    }

    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.patterns.len() != PATTERN_SLOTS || self.speed_upgrades > MAX_SPEED_UPGRADES {
            return None;
        }
        let patterns = self
            .patterns
            .iter()
            .map(|pattern| match pattern {
                Some(pattern) => Self::encode_pattern(pattern),
                None => Some(Value::Compound(Compound::new())),
            })
            .collect::<Option<Vec<_>>>()?;
        root.insert(
            KEY.into(),
            Value::Compound(Compound::from([
                ("version".into(), Value::Int(1)),
                ("patterns".into(), Value::List(patterns)),
                (
                    "speedUpgrades".into(),
                    Value::Int(self.speed_upgrades as i32),
                ),
                ("mode".into(), Value::Int(self.mode as i32)),
                ("locked".into(), Value::Byte(i8::from(self.locked))),
                ("wasPowered".into(), Value::Byte(i8::from(self.was_powered))),
            ])),
        );
        Some(())
    }

    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(value) = root.get(KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(saved) = value else {
            return None;
        };
        if saved.get("version") != Some(&Value::Int(1)) {
            return None;
        }
        let (Some(Value::List(patterns)), Some(Value::Int(speed)), Some(Value::Int(mode))) = (
            saved.get("patterns"),
            saved.get("speedUpgrades"),
            saved.get("mode"),
        ) else {
            return None;
        };
        if patterns.len() != PATTERN_SLOTS {
            return None;
        }
        let speed_upgrades = u8::try_from(*speed).ok()?;
        if speed_upgrades > MAX_SPEED_UPGRADES {
            return None;
        }
        let mode = Mode::from_id(*mode)?;
        let flag = |key| match saved.get(key) {
            None => Some(false),
            Some(Value::Byte(value)) => Some(*value != 0),
            _ => None,
        };
        let locked = flag("locked")?;
        let was_powered = flag("wasPowered")?;
        if mode != Mode::PulseInsertsNextSet && (locked || was_powered) {
            return None;
        }
        let mut restored = Vec::with_capacity(PATTERN_SLOTS);
        for entry in patterns {
            let Value::Compound(fields) = entry else {
                return None;
            };
            if fields.is_empty() {
                restored.push(None);
            } else {
                let pattern = Self::decode_pattern(entry)?;
                if restored
                    .iter()
                    .flatten()
                    .any(|saved: &InstalledPattern| saved.same_pattern(&pattern))
                {
                    return None;
                }
                restored.push(Some(pattern));
            }
        }
        Some(Self {
            patterns: restored,
            speed_upgrades,
            mode,
            locked,
            was_powered,
        })
    }
}

pub fn load(world: World<'_>, site: Pos) -> Option<Crafter> {
    let snapshot = world.entity_snapshot(site)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Crafter::decode(&root)
}

pub fn update<T>(
    world: World<'_>,
    site: Pos,
    mutate: impl FnOnce(&mut Crafter) -> Option<T>,
) -> Option<T> {
    let snapshot = world.entity_snapshot(site)?;
    let mut root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    let mut crafter = Crafter::decode(&root)?;
    let result = mutate(&mut crafter)?;
    crafter.encode(&mut root)?;
    let bytes = fastnbt::to_bytes(&root).ok()?;
    world
        .compare_entity(site, &snapshot, &bytes)
        .then_some(result)
}

#[derive(Clone, Copy, Debug)]
pub struct CrafterNode {
    pub facing: u8,
}

/// Resolve the root container in the source's facing direction. Only a
/// connected Crafter on that face participates; loops fail closed instead of
/// recursing indefinitely. `network_nodes` is the already validated live graph.
pub fn root_container(
    crafters: &HashMap<Pos, CrafterNode>,
    network_nodes: &HashSet<Pos>,
    start: Pos,
) -> Option<Pos> {
    let mut current = start;
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(current) {
            return None;
        }
        let node = *crafters.get(&current)?;
        let next = crate::network::neighbor(current, node.facing)?;
        if network_nodes.contains(&next) && crafters.contains_key(&next) {
            current = next;
        } else {
            return Some(current);
        }
    }
}

/// A chained Crafter inherits the lock policy of its facing root container.
pub fn effective_lock(
    crafters: &HashMap<Pos, CrafterNode>,
    network_nodes: &HashSet<Pos>,
    states: &HashMap<Pos, Crafter>,
    powered: &HashMap<Pos, bool>,
    start: Pos,
) -> Option<bool> {
    let root = root_container(crafters, network_nodes, start)?;
    let state = states.get(&root)?;
    Some(state.is_locked(powered.get(&root).copied().unwrap_or(false)))
}

/// Processing use is attributed to the root container, matching the source's
/// `onUsedForProcessing` delegation for facing-connected Crafter chains.
pub fn mark_used_for_processing(
    crafters: &HashMap<Pos, CrafterNode>,
    network_nodes: &HashSet<Pos>,
    states: &mut HashMap<Pos, Crafter>,
    start: Pos,
) -> Option<Pos> {
    let root = root_container(crafters, network_nodes, start)?;
    states.get_mut(&root)?.on_used_for_processing();
    Some(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, count: i32) -> Stack {
        Stack::parse(
            &fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(count)),
            ]))
            .unwrap(),
        )
        .unwrap()
    }
    fn pattern(id: &str) -> ProcessingPattern {
        let mut value = ProcessingPattern::default();
        value.inputs[0] = Some(item("minecraft:iron_ingot", 4));
        value.outputs[0] = Some(item(id, 1));
        value
    }
    fn pos(x: i32) -> Pos {
        Pos { x, y: 80, z: 0 }
    }

    #[test]
    fn nine_single_pattern_slots_reject_duplicates_and_roundtrip_foreign_nbt() {
        let mut crafter = Crafter::default();
        let first = pattern("minecraft:iron_block");
        let first = InstalledPattern::Processing(first);
        assert!(crafter.install_pattern(first.clone()));
        assert!(!crafter.install_pattern(first.clone()));
        for i in 0..8 {
            assert!(
                crafter.install_pattern(InstalledPattern::Processing(pattern(&format!(
                    "test:result_{i}"
                ))))
            );
        }
        assert_eq!(crafter.patterns.iter().flatten().count(), 9);
        assert!(!crafter.install_pattern(InstalledPattern::Processing(pattern("test:overflow"))));
        let mut root = Compound::from([("foreign".into(), Value::Int(91))]);
        crafter.encode(&mut root).unwrap();
        let saved: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        assert_eq!(saved.get("foreign"), Some(&Value::Int(91)));
        let mut restored = Crafter::decode(&saved).unwrap();
        assert!(restored.eject_pattern(0).unwrap().same_pattern(&first));
        assert_eq!(restored.patterns.iter().flatten().count(), 8);
    }

    #[test]
    fn crafter_accepts_both_native_crafting_and_v1124_processing_pattern_items() {
        let mut grid = crate::craft::Pattern::default();
        grid.slots[0] = Some(item("minecraft:iron_ingot", 1));
        let recipe = moddedmc_api::crafting::RecipeMatch {
            result: item("minecraft:iron_block", 1).raw,
            id: Some("test:iron_block".into()),
            serializer: "minecraft:crafting_shaped".into(),
        };
        let crafting = RecipePattern::from_live(grid, &recipe).unwrap();
        let installed = InstalledPattern::Crafting(crafting);
        let stack = Stack::parse(&installed.item_stack().unwrap()).unwrap();
        let restored = InstalledPattern::from_stack(&stack).unwrap();
        assert!(installed.same_pattern(&restored));
        let mut crafter = Crafter::default();
        assert!(crafter.install_pattern(installed));
        assert_eq!(crafter.patterns.iter().flatten().count(), 1);
    }

    #[test]
    fn v1124_crafting_pattern_item_is_not_mistaken_for_processing_or_lost() {
        let mut value = pattern("minecraft:iron_block");
        value.exact = true;
        value.outputs.fill(None); // Source crafting PatternItems derive output from the live recipe.
        let item = CraftingPatternItem(value.clone());
        let bytes = item.item_stack().unwrap();
        let stack = Stack::parse(&bytes).unwrap();
        let restored = InstalledPattern::from_stack(&stack).unwrap();
        assert!(matches!(restored, InstalledPattern::V1124Crafting(_)));
        let mut crafter = Crafter::default();
        assert!(crafter.install_pattern(restored));
        assert_eq!(crafter.patterns.iter().flatten().count(), 1);
    }

    #[test]
    fn speed_upgrades_match_source_interval_and_batch_limit() {
        let mut crafter = Crafter::default();
        for (count, interval, batch) in [(0, 10, 1), (1, 8, 2), (2, 6, 3), (3, 4, 4), (4, 2, 5)] {
            assert_eq!(crafter.update_interval(), interval);
            assert_eq!(crafter.max_successful_updates(), batch);
            if count < 4 {
                assert!(crafter.install_speed_upgrade());
            }
        }
        assert!(!crafter.install_speed_upgrade());
    }

    #[test]
    fn all_four_source_redstone_modes_and_pulse_falling_edge_are_preserved() {
        let mut crafter = Crafter::default();
        assert!(!crafter.is_locked(false));
        assert!(!crafter.is_locked(true));
        crafter.set_mode(Mode::SignalUnlocks);
        assert!(crafter.is_locked(false));
        assert!(!crafter.is_locked(true));
        crafter.set_mode(Mode::SignalLocks);
        assert!(!crafter.is_locked(false));
        assert!(crafter.is_locked(true));
        crafter.set_mode(Mode::PulseInsertsNextSet);
        crafter.on_used_for_processing();
        assert!(crafter.is_locked(false));
        crafter.update_power(true);
        crafter.update_power(false);
        assert!(!crafter.is_locked(false));
        assert!(!crafter.was_powered);
    }

    #[test]
    fn pulse_lock_and_power_edge_survive_crafter_nbt_restart() {
        let mut crafter = Crafter::default();
        crafter.set_mode(Mode::PulseInsertsNextSet);
        crafter.on_used_for_processing();
        crafter.update_power(true);
        let mut root = Compound::new();
        crafter.encode(&mut root).unwrap();
        let restored_root: Compound =
            fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        let restored = Crafter::decode(&restored_root).unwrap();
        assert_eq!(restored.mode, Mode::PulseInsertsNextSet);
        assert!(restored.locked);
        assert!(restored.was_powered);
    }

    #[test]
    fn facing_chains_resolve_one_root_and_reject_loops_or_unconnected_crafters() {
        let crafters = HashMap::from([
            (pos(0), CrafterNode { facing: 5 }),
            (pos(1), CrafterNode { facing: 5 }),
            (pos(2), CrafterNode { facing: 5 }),
        ]);
        let network = HashSet::from([pos(0), pos(1), pos(2)]);
        assert_eq!(root_container(&crafters, &network, pos(0)), Some(pos(2)));
        let looped = HashMap::from([
            (pos(0), CrafterNode { facing: 5 }),
            (pos(1), CrafterNode { facing: 4 }),
        ]);
        assert_eq!(root_container(&looped, &network, pos(0)), None);
        assert_eq!(
            root_container(&crafters, &HashSet::from([pos(0)]), pos(0)),
            Some(pos(0))
        );
    }

    #[test]
    fn chained_crafters_inherit_root_lock_and_processing_pulses() {
        let nodes = HashMap::from([
            (pos(0), CrafterNode { facing: 5 }),
            (pos(1), CrafterNode { facing: 5 }),
        ]);
        let network = HashSet::from([pos(0), pos(1)]);
        let mut states =
            HashMap::from([(pos(0), Crafter::default()), (pos(1), Crafter::default())]);
        states
            .get_mut(&pos(1))
            .unwrap()
            .set_mode(Mode::PulseInsertsNextSet);
        assert_eq!(
            effective_lock(&nodes, &network, &states, &HashMap::new(), pos(0)),
            Some(false)
        );
        assert_eq!(
            mark_used_for_processing(&nodes, &network, &mut states, pos(0)),
            Some(pos(1))
        );
        assert_eq!(
            effective_lock(&nodes, &network, &states, &HashMap::new(), pos(0)),
            Some(true)
        );
    }
}
