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
}

impl Options {
    fn parse(raw: FileOptions, nature: bool) -> Result<Self, &'static str> {
        if raw.allow_teleport == Some(true) {
            return Err("allowTeleport=true requires permission-checked teleport support");
        }
        if raw.default_xp_levels.unwrap_or(0) != 0 {
            return Err("nonzero defaultXpLevels requires atomic XP charging");
        }
        if raw.compass_durability.unwrap_or(0) != 0 {
            return Err("nonzero compassDurability requires atomic damage and repair support");
        }
        if (nature && !raw.per_structure_xp_levels.is_empty())
            || (!nature && !raw.per_biome_xp_levels.is_empty())
        {
            return Err("XP override belongs to the other compass config");
        }
        // The source supports "pattern,levels" entries. Zero-cost overrides
        // are already represented by our default zero-cost search. Reject
        // malformed or nonzero entries rather than silently dropping costs.
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
                .parse::<u32>()
                .map_err(|_| "XP override levels must be an integer 0..=3")?;
            if levels > 3 {
                return Err("XP override levels must be an integer 0..=3");
            }
            if levels != 0 {
                return Err("nonzero per-target XP overrides require atomic XP charging");
            }
        }
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
        })
    }

    pub fn load(dir: &Path, nature: bool) -> Result<Self, String> {
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
        Self::parse(raw, nature).map_err(|e| format!("{}: {e}", path.display()))
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
}
