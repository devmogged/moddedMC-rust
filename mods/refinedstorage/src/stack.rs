//! Lossless vanilla named-root ItemStack NBT for routing and aggregation. Network comparisons
//! include every component; two swords with different damage or enchantments never merge.

use fastnbt::Value;
use std::collections::HashMap;

type Compound = HashMap<String, Value>;

#[derive(Clone, Debug)]
pub struct Stack {
    pub raw: Vec<u8>,
    pub id: String,
    pub count: u32,
    pub key: Compound,
}

fn integer(value: &Value) -> Option<i64> {
    match value {
        Value::Byte(v) => Some(i64::from(*v)),
        Value::Short(v) => Some(i64::from(*v)),
        Value::Int(v) => Some(i64::from(*v)),
        Value::Long(v) => Some(*v),
        _ => None,
    }
}

impl Stack {
    pub fn parse(raw: &[u8]) -> Option<Self> {
        if raw.is_empty() || raw.len() > 64 * 1024 {
            return None;
        }
        let mut key: Compound = fastnbt::from_bytes(raw).ok()?;
        let id = match key.get("id")? {
            Value::String(id) if id.contains(':') => id.clone(),
            _ => return None,
        };
        let count = u32::try_from(integer(key.get("count")?)?).ok()?;
        if count == 0 || count > 99 * 64 {
            return None;
        }
        key.remove("count");
        Some(Self {
            raw: raw.to_vec(),
            id,
            count,
            key,
        })
    }

    /// Rewrite only the count. Preserve components and every unknown vanilla/modded key.
    pub fn with_count(&self, count: u32) -> Option<Vec<u8>> {
        if count == 0 || count > 99 * 64 {
            return None;
        }
        let mut nbt = self.key.clone();
        nbt.insert("count".into(), Value::Int(i32::try_from(count).ok()?));
        fastnbt::to_bytes(&nbt).ok()
    }

    pub fn same_item(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub sample: Stack,
    pub total: u64,
}

/// The original UI cycles AMOUNT -> NAME -> MOD, with a separate downwards toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Amount,
    Name,
    Mod,
}

impl Sort {
    pub fn next(self) -> Self {
        match self {
            Self::Amount => Self::Name,
            Self::Name => Self::Mod,
            Self::Mod => Self::Amount,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Amount => "Amount",
            Self::Name => "Name",
            Self::Mod => "Mod",
        }
    }
}

pub fn sort_requests(requests: &mut [Request], mode: Sort, downwards: bool) {
    requests.sort_by(|a, b| {
        let ordering = match mode {
            Sort::Amount => a.total.cmp(&b.total),
            Sort::Name => a.sample.id.cmp(&b.sample.id),
            Sort::Mod => namespace(&a.sample.id).cmp(namespace(&b.sample.id)),
        };
        let ordering = if downwards {
            ordering.reverse()
        } else {
            ordering
        };
        ordering
            .then_with(|| a.sample.id.cmp(&b.sample.id))
            .then_with(|| a.sample.raw.cmp(&b.sample.raw))
    });
}

fn namespace(id: &str) -> &str {
    id.split_once(':').map_or(id, |(namespace, _)| namespace)
}

/// Item identity groups preserve component distinctions. Sorted output is stable independent of
/// the order in which link cables and chests were discovered.
pub fn aggregate(stacks: impl IntoIterator<Item = Stack>) -> Vec<Request> {
    // Different item IDs can never match, so avoid a global quadratic scan for a catalogue
    // spanning many modded item types. Compare component NBT only inside the matching ID bucket.
    let mut by_id: HashMap<String, Vec<Request>> = HashMap::new();
    for stack in stacks {
        let bucket = by_id.entry(stack.id.clone()).or_default();
        if let Some(entry) = bucket.iter_mut().find(|e| e.sample.same_item(&stack)) {
            entry.total = entry.total.saturating_add(u64::from(stack.count));
        } else {
            bucket.push(Request {
                total: u64::from(stack.count),
                sample: stack,
            });
        }
    }
    let mut result: Vec<Request> = by_id.into_values().flatten().collect();
    result.sort_by(|left, right| {
        left.sample
            .id
            .cmp(&right.sample.id)
            .then_with(|| left.sample.raw.cmp(&right.sample.raw))
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(id: &str, count: i32, damage: i32) -> Vec<u8> {
        fastnbt::to_bytes(&Compound::from([
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
        .unwrap()
    }

    #[test]
    fn component_differences_are_distinct_and_count_is_lossless() {
        let first = Stack::parse(&encoded("minecraft:diamond_sword", 1, 7)).unwrap();
        let changed = Stack::parse(&encoded("minecraft:diamond_sword", 1, 8)).unwrap();
        assert!(!first.same_item(&changed));
        let new = Stack::parse(&first.with_count(4).unwrap()).unwrap();
        assert_eq!(new.count, 4);
        assert_eq!(new.key, first.key);
        assert!(Stack::parse(&encoded("minecraft:stone", 0, 0)).is_none());
        assert!(Stack::parse(&encoded("minecraft:stone", -1, 0)).is_none());
    }

    #[test]
    fn aggregation_separates_components_and_uses_exact_totals() {
        let a = Stack::parse(&encoded("minecraft:iron_ingot", 17, 0)).unwrap();
        let b = Stack::parse(&encoded("minecraft:iron_ingot", 30, 0)).unwrap();
        let c = Stack::parse(&encoded("minecraft:iron_ingot", 5, 1)).unwrap();
        let result = aggregate([a, b, c]);
        assert_eq!(result.len(), 2);
        assert_eq!(result.iter().map(|request| request.total).sum::<u64>(), 52);
        assert!(result.iter().any(|request| request.total == 47));
    }

    #[test]
    fn all_three_source_sort_modes_have_deterministic_tie_breakers() {
        let a = Stack::parse(&encoded("alpha:copper", 1, 0)).unwrap();
        let b = Stack::parse(&encoded("beta:iron", 7, 0)).unwrap();
        let c = Stack::parse(&encoded("alpha:diamond", 3, 0)).unwrap();
        let mut requests = aggregate([a, b, c]);
        sort_requests(&mut requests, Sort::Amount, true);
        assert_eq!(requests[0].sample.id, "beta:iron");
        sort_requests(&mut requests, Sort::Name, false);
        assert_eq!(requests[0].sample.id, "alpha:copper");
        sort_requests(&mut requests, Sort::Mod, false);
        assert!(requests[0].sample.id.starts_with("alpha:"));
        assert!(requests[1].sample.id.starts_with("alpha:"));
        assert_eq!(Sort::Mod.next(), Sort::Amount);
    }
}
