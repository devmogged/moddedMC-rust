//! Wireless Grid controller binding and installed transmitter range, saved in
//! named-root stack/block NBT. A transmitter must be in the same connected
//! controller graph and hold at least one consumed range upgrade.

use fastnbt::Value;
use moddedmc_api::{Pos, content::World};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const KEY: &str = "refinedstorage_wireless";
const MAX_UPGRADES: u8 = 4;
const RANGE_PER_UPGRADE: u32 = 16;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Transmitter {
    pub upgrades: u8,
}
impl Transmitter {
    pub fn range(self) -> u32 {
        u32::from(self.upgrades) * RANGE_PER_UPGRADE
    }
    pub fn install(&mut self) -> bool {
        if self.upgrades >= MAX_UPGRADES {
            return false;
        }
        self.upgrades += 1;
        true
    }
    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(value) = root.get(KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(data) = value else {
            return None;
        };
        let Some(Value::Byte(count)) = data.get("rangeUpgrades") else {
            return None;
        };
        let upgrades = u8::try_from(*count).ok()?;
        (upgrades <= MAX_UPGRADES).then_some(Self { upgrades })
    }
    pub fn encode(self, root: &mut Compound) -> Option<()> {
        if self.upgrades > MAX_UPGRADES {
            return None;
        }
        root.insert(
            KEY.into(),
            Value::Compound(Compound::from([(
                "rangeUpgrades".into(),
                Value::Byte(i8::try_from(self.upgrades).ok()?),
            )])),
        );
        Some(())
    }
}

pub fn load(world: World<'_>, site: Pos) -> Option<Transmitter> {
    let snapshot = world.entity_snapshot(site)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Transmitter::decode(&root)
}
pub fn update(world: World<'_>, site: Pos, change: impl FnOnce(&mut Transmitter) -> bool) -> bool {
    let Some(snapshot) = world.entity_snapshot(site) else {
        return false;
    };
    let Ok(mut root) = fastnbt::from_bytes::<Compound>(&snapshot.nbt) else {
        return false;
    };
    let Some(mut transmitter) = Transmitter::decode(&root) else {
        return false;
    };
    if !change(&mut transmitter) || transmitter.encode(&mut root).is_none() {
        return false;
    }
    let Ok(bytes) = fastnbt::to_bytes(&root) else {
        return false;
    };
    world.compare_entity(site, &snapshot, &bytes)
}

/// Network Grid item binding follows the source's explicit controller
/// ownership, and cannot be reused in another dimension.
pub fn bind(dimension: &str, root: Pos) -> Option<Vec<u8>> {
    if dimension.is_empty() || dimension.len() > 256 {
        return None;
    }
    fastnbt::to_bytes(&Compound::from([
        ("version".into(), Value::Int(1)),
        ("dimension".into(), Value::String(dimension.into())),
        ("x".into(), Value::Int(root.x)),
        ("y".into(), Value::Int(root.y)),
        ("z".into(), Value::Int(root.z)),
    ]))
    .ok()
}
pub fn bound(data: &[u8]) -> Option<(String, Pos)> {
    if data.is_empty() || data.len() > 2048 {
        return None;
    }
    let root: Compound = fastnbt::from_bytes(data).ok()?;
    if root.get("version") != Some(&Value::Int(1)) {
        return None;
    }
    let Value::String(dim) = root.get("dimension")? else {
        return None;
    };
    if dim.is_empty() || dim.len() > 256 {
        return None;
    }
    let coordinate = |key| match root.get(key) {
        Some(Value::Int(value)) => Some(*value),
        _ => None,
    };
    Some((
        dim.clone(),
        Pos {
            x: coordinate("x")?,
            y: coordinate("y")?,
            z: coordinate("z")?,
        },
    ))
}

pub fn in_range(player: [f64; 3], at: Pos, range: u32) -> bool {
    if range == 0 || player.iter().any(|value| !value.is_finite()) {
        return false;
    }
    let square = (player[0] - (f64::from(at.x) + 0.5)).powi(2)
        + (player[1] - (f64::from(at.y) + 0.5)).powi(2)
        + (player[2] - (f64::from(at.z) + 0.5)).powi(2);
    square.is_finite() && square <= f64::from(range).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bound_remote_has_full_dimension_and_restart_safe_coordinates() {
        let site = Pos {
            x: -45,
            y: 90,
            z: 1034,
        };
        let bytes = bind("minecraft:overworld", site).unwrap();
        let (dim, loaded) = bound(&bytes).unwrap();
        assert_eq!(dim, "minecraft:overworld");
        assert_eq!(loaded, site);
        assert!(bound(b"invalid").is_none());
        assert!(bind("", Pos { x: 0, y: 0, z: 0 }).is_none());
    }
    #[test]
    fn upgrades_and_range_are_real_saved_requirements() {
        let mut transmitter = Transmitter::default();
        assert!(!in_range(
            [1.5, 80.5, 1.5],
            Pos { x: 1, y: 80, z: 1 },
            transmitter.range()
        ));
        assert!(transmitter.install());
        assert_eq!(transmitter.range(), 16);
        assert!(in_range(
            [1.5, 80.5, 16.5],
            Pos { x: 1, y: 80, z: 1 },
            transmitter.range()
        ));
        assert!(!in_range(
            [1.5, 80.5, 18.5],
            Pos { x: 1, y: 80, z: 1 },
            transmitter.range()
        ));
        for _ in 0..3 {
            assert!(transmitter.install());
        }
        assert!(!transmitter.install());
        let mut root = Compound::from([("foreign".into(), Value::Int(6))]);
        transmitter.encode(&mut root).unwrap();
        let copy: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        assert_eq!(Transmitter::decode(&copy).unwrap(), transmitter);
        assert_eq!(copy.get("foreign"), Some(&Value::Int(6)));
        let Value::Compound(saved) = root.get_mut(KEY).unwrap() else {
            unreachable!()
        };
        saved.insert("rangeUpgrades".into(), Value::Byte(7));
        assert!(Transmitter::decode(&root).is_none());
    }
}
