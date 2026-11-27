//! Persist absolute game-time deadlines in the same archive as claims.
//! Policy follows Lootr's global, loot-table, namespace and dimension filters.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq, Default, Deserialize, Serialize)]
pub(crate) struct Schedule {
    #[serde(default)]
    pub refresh_at: Option<i64>,
    #[serde(default)]
    pub decay_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Policy {
    pub all: bool,
    pub ticks: i64,
    pub tables: BTreeSet<String>,
    pub namespaces: BTreeSet<String>,
    pub dimensions: BTreeSet<String>,
    pub perform_while_ticking: bool,
    pub start_while_ticking: bool,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            all: false,
            ticks: 6000,
            tables: BTreeSet::new(),
            namespaces: BTreeSet::new(),
            dimensions: BTreeSet::new(),
            perform_while_ticking: true,
            start_while_ticking: false,
        }
    }
}

impl Policy {
    pub fn matches(&self, table: Option<&str>, dimension: &str) -> bool {
        self.all
            || self.dimensions.contains(dimension)
            || table.is_some_and(|table| {
                self.tables.contains(table)
                    || table
                        .split_once(':')
                        .is_some_and(|(namespace, _)| self.namespaces.contains(namespace))
            })
    }

    pub fn enabled(&self) -> bool {
        self.all
            || !self.tables.is_empty()
            || !self.namespaces.is_empty()
            || !self.dimensions.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Config {
    pub refresh: Policy,
    pub decay: Policy,
    pub replace_when_decayed: bool,
}

impl Config {
    /// Port-specific, strict key/value syntax. Unknown settings are rejected
    /// so administrators cannot unknowingly enable an unsupported behavior.
    pub fn parse(input: &str) -> Option<Self> {
        let mut config = Self::default();
        let mut seen = BTreeSet::new();
        for line in input.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            let (key, value) = (key.trim(), value.trim());
            if !seen.insert(key) {
                return None;
            }
            if key == "replace_when_decayed" {
                config.replace_when_decayed = parse_bool(value)?;
                continue;
            }
            let (prefix, field) = key.split_once('_')?;
            let policy = match prefix {
                "refresh" => &mut config.refresh,
                "decay" => &mut config.decay,
                _ => return None,
            };
            match field {
                "all" => policy.all = parse_bool(value)?,
                "value" => {
                    let ticks = value.parse::<i64>().ok()?;
                    if !(1..=i64::from(i32::MAX)).contains(&ticks) {
                        return None;
                    }
                    policy.ticks = ticks;
                }
                "loot_tables" => policy.tables = list(value, true)?,
                "loot_table_modids" => policy.namespaces = list(value, false)?,
                "dimensions" => policy.dimensions = list(value, true)?,
                "perform_while_ticking" => policy.perform_while_ticking = parse_bool(value)?,
                "start_while_ticking" => policy.start_while_ticking = parse_bool(value)?,
                _ => return None,
            }
        }
        Some(config)
    }

    pub fn enabled(&self) -> bool {
        self.refresh.enabled() || self.decay.enabled()
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn list(value: &str, resource: bool) -> Option<BTreeSet<String>> {
    if value.is_empty() {
        return Some(BTreeSet::new());
    }
    let mut entries = BTreeSet::new();
    for item in value.split(',').map(str::trim) {
        let legal = |part: &str, path: bool| {
            !part.is_empty()
                && part.bytes().all(|b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || matches!(b, b'_' | b'-' | b'.')
                        || (path && b == b'/')
                })
        };
        if resource {
            let (namespace, path) = item.split_once(':')?;
            if !legal(namespace, false) || !legal(path, true) {
                return None;
            }
        } else if !legal(item, false) {
            return None;
        }
        entries.insert(item.into());
    }
    Some(entries)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    None,
    Refresh,
    Decay,
}

impl Schedule {
    pub fn valid(&self) -> bool {
        self.refresh_at.is_none_or(|at| at >= 0) && self.decay_at.is_none_or(|at| at >= 0)
    }

    /// Decay has precedence over refresh, as in the Java open and tick paths.
    pub fn due(&self, config: &Config, now: i64, ticking: bool) -> Action {
        if self.decay_at.is_some_and(|at| now >= at)
            && (!ticking || config.decay.perform_while_ticking)
        {
            Action::Decay
        } else if self.refresh_at.is_some_and(|at| now >= at)
            && (!ticking || config.refresh.perform_while_ticking)
        {
            Action::Refresh
        } else {
            Action::None
        }
    }

    /// Start only on open by default. An already-running deadline is never
    /// extended by repeated opens, nor by a restart or a config duration edit.
    pub fn begin(
        &mut self,
        config: &Config,
        now: i64,
        table: Option<&str>,
        dimension: &str,
        ticking: bool,
    ) -> bool {
        let previous = self.clone();
        for (deadline, policy) in [
            (&mut self.decay_at, &config.decay),
            (&mut self.refresh_at, &config.refresh),
        ] {
            if deadline.is_none()
                && (!ticking || policy.start_while_ticking)
                && policy.matches(table, dimension)
            {
                *deadline = now.checked_add(policy.ticks).filter(|at| *at >= 0);
            }
        }
        *self != previous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_survive_serialization_and_are_not_extended() {
        let config = Config::parse(
            "refresh_all=true\nrefresh_value=20\ndecay_loot_table_modids=minecraft\ndecay_value=40",
        )
        .unwrap();
        let mut schedule = Schedule::default();
        assert!(schedule.begin(
            &config,
            100,
            Some("minecraft:chests/test"),
            "minecraft:overworld",
            false
        ));
        assert!(!schedule.begin(
            &config,
            110,
            Some("minecraft:chests/test"),
            "minecraft:overworld",
            false
        ));
        let restored: Schedule =
            fastnbt::from_bytes(&fastnbt::to_bytes(&schedule).unwrap()).unwrap();
        assert_eq!(restored.due(&config, 119, true), Action::None);
        assert_eq!(restored.due(&config, 120, true), Action::Refresh);
        assert_eq!(restored.due(&config, 140, false), Action::Decay);
    }

    #[test]
    fn defaults_and_filters_match_source_policy() {
        let default = Config::default();
        assert!(!default.enabled());
        let config = Config::parse("refresh_dimensions=minecraft:the_nether\nrefresh_start_while_ticking=true\ndecay_all=true\ndecay_perform_while_ticking=false").unwrap();
        let mut schedule = Schedule::default();
        assert!(schedule.begin(&config, 3, None, "minecraft:the_nether", true));
        assert_eq!(schedule.refresh_at, Some(6003));
        assert_eq!(schedule.decay_at, None);
        assert!(schedule.begin(&config, 5, None, "minecraft:the_nether", false));
        assert_eq!(schedule.due(&config, 6005, true), Action::Refresh);
        assert_eq!(schedule.due(&config, 6005, false), Action::Decay);
        assert!(Config::parse("refresh_value=0").is_none());
        assert!(Config::parse("refresh_all=true\nrefresh_all=false").is_none());
        assert!(Config::parse("refresh_loot_tables=invalid").is_none());
    }
}
