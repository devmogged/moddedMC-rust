//! Server-side selection and search options. Read once at mod startup.
use serde::Deserialize;
use std::{fs, io::ErrorKind, path::Path};

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
struct FileOptions {
    biome_blacklist: Vec<String>,
    structure_blacklist: Vec<String>,
    max_next_searches: Option<usize>,
    max_samples: Option<u32>,
    max_radius: Option<i32>,
    radius_modifier: Option<i32>,
    sample_space_modifier: Option<i32>,
    display_coordinates: Option<bool>,
    // Accept explicit disabled/default settings for migration from the
    // original General config. Non-default settings are rejected until
    // their required host capabilities exist; never silently ignore them.
    allow_teleport: Option<bool>,
    default_xp_levels: Option<u32>,
    per_biome_xp_levels: Vec<String>,
    per_structure_xp_levels: Vec<String>,
    compass_durability: Option<u32>,
}

pub struct Options {
    pub blacklist: Vec<String>,
    pub max_next: usize,
    pub max_samples: u32,
    pub radius: i32,
    pub step: i32,
    pub display_coordinates: bool,
    pub costs: SearchCosts,
}

/// Separate gameplay policy from host capability discovery. Costs must never
/// be activated unless the host can atomically authorize the stack update.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SearchCosts {
    default_xp: u8,
    overrides: Vec<(String, u8)>,
    pub durability: u16,
}

/// The source charges when a search is requested, including searches that
/// eventually exhaust. A new group search uses its set ID, while find-next
/// uses the actual structure member saved in the previous result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchIntent<'a> {
    Target(&'a str),
    Group(&'a str),
    FindNext(&'a str),
}

impl<'a> SearchIntent<'a> {
    fn billed_id(self) -> &'a str {
        match self {
            Self::Target(id) | Self::Group(id) | Self::FindNext(id) => id,
        }
    }
}

/// A quote describes the required debit, never proof that it was performed.
/// The v2 host interface must check available levels, debit, and replace the
/// exact compass state in one operation or do none of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchQuote {
    pub levels: u8,
}

impl SearchCosts {
    fn parse(raw: &FileOptions, nature: bool) -> Result<Self, &'static str> {
        let default_xp = raw.default_xp_levels.unwrap_or(0);
        if default_xp > 3 {
            return Err("defaultXpLevels must be 0..=3");
        }
        let durability = raw.compass_durability.unwrap_or(0);
        if durability > 10_000 {
            return Err("compassDurability must be 0..=10000");
        }
        if (nature && !raw.per_structure_xp_levels.is_empty())
            || (!nature && !raw.per_biome_xp_levels.is_empty())
        {
            return Err("XP override belongs to the other compass config");
        }
        let mut overrides: Vec<(String, u8)> = Vec::new();
        for entry in raw
            .per_biome_xp_levels
            .iter()
            .chain(&raw.per_structure_xp_levels)
        {
            let Some((pattern, levels)) = entry.split_once(',') else {
                return Err("XP override must be a pattern,levels pair");
            };
            if !valid_pattern(pattern) {
                return Err("XP override pattern must be a nonempty resource-ID glob");
            }
            let levels = levels
                .parse::<u8>()
                .map_err(|_| "XP override levels must be an integer 0..=3")?;
            if levels > 3 {
                return Err("XP override levels must be an integer 0..=3");
            }
            // As in the source map, a repeated identical pattern replaces its
            // previous value. For distinct overlapping globs the native JSON
            // contract uses explicit declaration order rather than Java's
            // unspecified HashMap iteration order.
            if let Some((_, cost)) = overrides.iter_mut().find(|(id, _)| id == pattern) {
                *cost = levels;
            } else {
                overrides.push((pattern.to_owned(), levels));
            }
        }
        Ok(Self {
            default_xp: default_xp as u8,
            overrides,
            durability: durability as u16,
        })
    }

    /// Resolve explicit group search against its group ID and find-next
    /// against the previously found actual member, regardless of visibility.
    pub fn quote(&self, intent: SearchIntent<'_>) -> SearchQuote {
        SearchQuote {
            levels: self.xp_for(intent.billed_id()),
        }
    }

    pub fn xp_for(&self, requested: &str) -> u8 {
        self.overrides
            .iter()
            .find(|(pattern, _)| glob(pattern.as_bytes(), requested.as_bytes()))
            .map_or(self.default_xp, |(_, cost)| *cost)
    }

    fn require_supported(&self, durable_state: bool, atomic_xp: bool) -> Result<(), &'static str> {
        if self.default_xp != 0 && !atomic_xp {
            return Err("nonzero defaultXpLevels requires atomic XP charging");
        }
        if self.durability != 0 && !durable_state {
            return Err("nonzero compassDurability requires player mode and atomic stack state");
        }
        if !atomic_xp && self.overrides.iter().any(|(_, cost)| *cost != 0) {
            return Err("nonzero per-target XP overrides require atomic XP charging");
        }
        Ok(())
    }
}

impl Options {
    #[cfg(test)]
    fn parse(raw: FileOptions, nature: bool) -> Result<Self, &'static str> {
        Self::parse_with_durability(raw, nature, false)
    }

    #[cfg(test)]
    fn parse_with_durability(
        raw: FileOptions,
        nature: bool,
        durable_state: bool,
    ) -> Result<Self, &'static str> {
        Self::parse_with_capabilities(raw, nature, durable_state, false)
    }

    fn parse_with_capabilities(
        raw: FileOptions,
        nature: bool,
        durable_state: bool,
        atomic_xp: bool,
    ) -> Result<Self, &'static str> {
        if raw.allow_teleport == Some(true) {
            return Err("allowTeleport=true requires permission-checked teleport support");
        }
        let costs = SearchCosts::parse(&raw, nature)?;
        costs.require_supported(durable_state, atomic_xp)?;
        let limit = if nature { 25 } else { 100 };
        let max_next = raw.max_next_searches.unwrap_or(limit);
        let max_samples = raw
            .max_samples
            .unwrap_or(if nature { 50_000 } else { 100_000 });
        let radius = if nature {
            if raw.max_radius.is_some() || !raw.structure_blacklist.is_empty() {
                return Err("Explorer-only option in Nature's Compass config");
            }
            let radius = raw.radius_modifier.unwrap_or(2500);
            if !(0..=1_000_000).contains(&radius) {
                return Err("radiusModifier must be 0..=1000000");
            }
            radius * 4
        } else {
            if raw.radius_modifier.is_some()
                || raw.sample_space_modifier.is_some()
                || !raw.biome_blacklist.is_empty()
            {
                return Err("Nature-only option in Explorer's Compass config");
            }
            let radius = raw.max_radius.unwrap_or(10_000);
            if !(0..=1_000_000).contains(&radius) {
                return Err("maxRadius must be 0..=1000000");
            }
            radius
        };
        let step = raw.sample_space_modifier.unwrap_or(16);
        if !(0..=1_000_000).contains(&step) {
            return Err("sampleSpaceModifier must be 0..=1000000");
        }
        if max_next > limit || max_samples > if nature { 1_000_000 } else { 100_000_000 } {
            return Err("search limits exceed supported range");
        }
        let blacklist = if nature {
            raw.biome_blacklist
        } else {
            raw.structure_blacklist
        };
        if blacklist.iter().any(|p| !valid_pattern(p)) {
            return Err("blacklist patterns must be nonempty resource-ID globs");
        }
        Ok(Self {
            blacklist,
            max_next,
            max_samples,
            radius,
            step: step * 4,
            display_coordinates: raw.display_coordinates.unwrap_or(true),
            costs,
        })
    }

    pub fn load(
        dir: &Path,
        nature: bool,
        durable_state: bool,
        atomic_xp: bool,
    ) -> Result<Self, String> {
        let path = dir.join(if nature {
            "naturescompass.json"
        } else {
            "explorerscompass.json"
        });
        let raw = match fs::read_to_string(&path) {
            Ok(text) => {
                serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            Err(e) if e.kind() == ErrorKind::NotFound => FileOptions::default(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::parse_with_capabilities(raw, nature, durable_state, atomic_xp)
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn blocked(&self, id: &str) -> bool {
        self.blacklist
            .iter()
            .any(|p| glob(p.as_bytes(), id.as_bytes()))
    }
}

/// The documented upstream wildcard language covers resource IDs, '*' and
/// '?'. Reject accidental Java-regex metacharacters rather than interpreting
/// them as unsupported regex or silently accepting an ineffective blacklist.
fn valid_pattern(pattern: &str) -> bool {
    !pattern.is_empty()
        && pattern.bytes().all(|c| {
            c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || matches!(c, b'_' | b'-' | b'.' | b'/' | b':' | b'*' | b'?')
        })
}

/// Whole-ID matching with upstream's documented '*' and '?' wildcards.
/// Registry identifiers are ASCII; no regex compilation on the menu path.
fn glob(pattern: &[u8], value: &[u8]) -> bool {
    let (mut p, mut v) = (0, 0);
    let mut star = None;
    let mut resume = 0;
    while v < value.len() {
        if pattern.get(p) == Some(&b'*') {
            star = Some(p);
            p += 1;
            resume = v;
        } else if pattern.get(p).is_some_and(|&c| c == b'?' || c == value[v]) {
            p += 1;
            v += 1;
        } else if let Some(s) = star {
            resume += 1;
            v = resume;
            p = s + 1;
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cost_policy_handles_group_and_member_costs_and_zero_override() {
        let raw = serde_json::from_str(
            r#"{
            "defaultXpLevels":2,
            "perStructureXpLevels":[
                "test:pair,3","test:member_b,0","test:member_?,1","test:pair,2"
            ],
            "compassDurability":3
        }"#,
        )
        .unwrap();
        let costs = SearchCosts::parse(&raw, false).unwrap();
        assert_eq!(costs.xp_for("test:pair"), 2);
        assert_eq!(costs.xp_for("test:member_b"), 0);
        assert_eq!(costs.xp_for("test:member_a"), 1);
        assert_eq!(costs.xp_for("minecraft:stronghold"), 2);
        assert_eq!(costs.durability, 3);
        assert!(costs.require_supported(false, false).is_err());
    }

    #[test]
    fn wildcard_matches_full_id_and_literal_punctuation() {
        assert!(glob(b"minecraft:*village*", b"minecraft:village_plains"));
        assert!(glob(b"test:member_?", b"test:member_a"));
        assert!(!glob(b"test:member_?", b"test:member_ab"));
        assert!(!glob(b"minecraft:forest", b"minecraft:birch_forest"));
        assert!(glob(b"*:*", b"test:member_a"));
        assert!(!glob(b"test:a.b", b"test:axb"));
    }
    #[test]
    fn settings_default_and_reject_unrepresentable_history() {
        let n = Options::parse(FileOptions::default(), true).unwrap();
        let e = Options::parse(FileOptions::default(), false).unwrap();
        assert_eq!(
            (n.radius, n.step, n.max_samples, n.max_next),
            (10000, 64, 50000, 25)
        );
        assert_eq!((e.radius, e.max_samples, e.max_next), (10000, 100000, 100));
        let raw = serde_json::from_str(r#"{"maxNextSearches":0,"structureBlacklist":["test:*"]}"#)
            .unwrap();
        let e = Options::parse(raw, false).unwrap();
        assert_eq!(e.max_next, 0);
        assert!(e.blocked("test:member_a"));
        assert!(!e.blocked("minecraft:stronghold"));
        let raw = serde_json::from_str(r#"{"maxNextSearches":101}"#).unwrap();
        assert!(Options::parse(raw, false).is_err());
    }

    #[test]
    fn disabled_unsupported_settings_are_explicit_not_silently_ignored() {
        for nature in [true, false] {
            let raw = serde_json::from_str(
                r#"{"allowTeleport":false,"defaultXpLevels":0,"compassDurability":0}"#,
            )
            .unwrap();
            assert!(Options::parse(raw, nature).is_ok());
            for text in [
                r#"{"allowTeleport":true}"#,
                r#"{"defaultXpLevels":1}"#,
                r#"{"compassDurability":100}"#,
                r#"{"perBiomeXpLevels":["minecraft:*ocean*,1"]}"#,
                r#"{"perStructureXpLevels":["minecraft:*village*,2"]}"#,
            ] {
                let raw = serde_json::from_str(text).unwrap();
                assert!(Options::parse(raw, nature).is_err());
            }
        }
    }

    #[test]
    fn invalid_source_limits_are_distinct_from_unimplemented_valid_settings() {
        for nature in [true, false] {
            for (json, expected) in [
                (r#"{"defaultXpLevels":4}"#, "defaultXpLevels must be 0..=3"),
                (
                    r#"{"defaultXpLevels":3}"#,
                    "nonzero defaultXpLevels requires atomic XP charging",
                ),
                (
                    r#"{"compassDurability":10001}"#,
                    "compassDurability must be 0..=10000",
                ),
                (
                    r#"{"compassDurability":10000}"#,
                    "nonzero compassDurability requires player mode and atomic stack state",
                ),
            ] {
                let raw = serde_json::from_str(json).unwrap();
                assert_eq!(Options::parse(raw, nature).err(), Some(expected));
            }
        }
    }

    #[test]
    fn zero_cost_overrides_and_documented_glob_syntax() {
        for (nature, text) in [
            (
                true,
                r#"{"perBiomeXpLevels":["minecraft:*ocean*,0","test:foo?,0"]}"#,
            ),
            (
                false,
                r#"{"perStructureXpLevels":["minecraft:*village*,0"]}"#,
            ),
        ] {
            let raw = serde_json::from_str(text).unwrap();
            assert!(Options::parse(raw, nature).is_ok());
        }
        for text in [
            r#"{"perBiomeXpLevels":["minecraft:forest"]}"#,
            r#"{"perBiomeXpLevels":["minecraft:forest,-1"]}"#,
            r#"{"perBiomeXpLevels":["minecraft:forest,4"]}"#,
            r#"{"perBiomeXpLevels":["minecraft:forest,0,0"]}"#,
            r#"{"biomeBlacklist":["minecraft:(forest|plains)"]}"#,
            r#"{"biomeBlacklist":["minecraft:Forest"]}"#,
        ] {
            let raw = serde_json::from_str(text).unwrap();
            assert!(Options::parse(raw, true).is_err(), "{text}");
        }
        for pattern in ["*", "*:*", "minecraft:*_forest", "mod:dir/a.b-?"] {
            assert!(valid_pattern(pattern));
        }
    }

    #[test]
    fn durable_stack_state_enables_wear_without_enabling_xp_charges() {
        for nature in [true, false] {
            let raw = serde_json::from_str(r#"{"compassDurability":2}"#).unwrap();
            assert!(Options::parse_with_durability(raw, nature, true).is_ok());
            let raw =
                serde_json::from_str(r#"{"compassDurability":2,"defaultXpLevels":1}"#).unwrap();
            assert_eq!(
                Options::parse_with_durability(raw, nature, true).err(),
                Some("nonzero defaultXpLevels requires atomic XP charging")
            );
        }
    }

    #[test]
    fn paid_config_requires_atomic_stack_and_xp_capability() {
        for nature in [true, false] {
            let key = if nature {
                "perBiomeXpLevels"
            } else {
                "perStructureXpLevels"
            };
            let json = format!(
                r#"{{"defaultXpLevels":2,"{key}":["test:free,0","test:paid,3"],"compassDurability":4}}"#
            );
            let raw: FileOptions = serde_json::from_str(&json).unwrap();
            assert_eq!(
                Options::parse_with_capabilities(raw, nature, true, false).err(),
                Some("nonzero defaultXpLevels requires atomic XP charging")
            );
            let raw: FileOptions = serde_json::from_str(&json).unwrap();
            let supported = Options::parse_with_capabilities(raw, nature, true, true).unwrap();
            assert_eq!(supported.costs.durability, 4);
            assert_eq!(supported.costs.xp_for("test:paid"), 3);
            assert_eq!(supported.costs.xp_for("test:free"), 0);
            let raw: FileOptions = serde_json::from_str(&json).unwrap();
            assert_eq!(
                Options::parse_with_capabilities(raw, nature, false, true).err(),
                Some("nonzero compassDurability requires player mode and atomic stack state")
            );
        }
    }

    #[test]
    fn quoted_group_then_actual_member_next_charges_distinct_targets() {
        let raw: FileOptions = serde_json::from_str(
            r#"{"defaultXpLevels":3,"perStructureXpLevels":[
                "test:pair,2","test:a,1","test:b,0","test:pair,2"]}"#,
        )
        .unwrap();
        let costs = SearchCosts::parse(&raw, false).unwrap();
        let first = costs.quote(SearchIntent::Group("test:pair"));
        let second = costs.quote(SearchIntent::FindNext("test:b"));
        let third = costs.quote(SearchIntent::FindNext("test:a"));
        assert_eq!(first.levels, 2);
        assert_eq!(second.levels, 0);
        assert_eq!(third.levels, 1);
        assert_eq!(costs.quote(SearchIntent::Target("test:other")).levels, 3);
        // No forged extra comma or resource ID is accepted as a cost override.
        for malformed in ["test:pair,1,2", "test:PAIR,1", "test:pair,-1"] {
            let json = format!(r#"{{"perStructureXpLevels":["{malformed}"]}}"#);
            let raw = serde_json::from_str(&json).unwrap();
            assert!(SearchCosts::parse(&raw, false).is_err());
        }
    }
}
