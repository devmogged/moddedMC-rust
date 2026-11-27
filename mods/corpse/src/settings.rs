use std::{
    collections::HashSet,
    io::ErrorKind,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::rules::LifetimePolicy;

const PATH: &str = "config/corpse-synced.toml";
const DAY_MILLIS: i64 = 86_400_000;

#[derive(Clone, Copy)]
pub struct Settings {
    pub life: LifetimePolicy,
    pub show_equipment: bool,
    pub fall_into_void: bool,
    pub history_days: i32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            life: LifetimePolicy::default(),
            show_equipment: true,
            fall_into_void: false,
            history_days: -1,
        }
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

pub fn settings() -> &'static Settings {
    SETTINGS.get_or_init(|| match std::fs::read_to_string(PATH) {
        Ok(contents) => Settings::parse(&contents),
        Err(error) if error.kind() == ErrorKind::NotFound => Settings::default(),
        Err(error) => {
            moddedmc_api::warn(&format!(
                "Could not read {PATH}. Using Corpse defaults. {error}"
            ));
            Settings::default()
        }
    })
}

impl Settings {
    fn parse(contents: &str) -> Self {
        let mut config = Self::default();
        let mut section = "";
        let mut seen = HashSet::new();
        for (index, raw) in contents.trim_start_matches('\u{feff}').lines().enumerate() {
            let line = raw.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            if let Some(table) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                section = table.trim();
                continue;
            }
            let valid = line.split_once('=').is_some_and(|(key, value)| {
                let key = key.trim();
                seen.insert((section, key)) && config.set(section, key, value.trim()).is_some()
            });
            if !valid {
                moddedmc_api::warn(&format!(
                    "Ignored invalid Corpse setting in {PATH} at line {}.",
                    index + 1
                ));
            }
        }
        config
    }

    fn set(&mut self, section: &str, key: &str, value: &str) -> Option<()> {
        let bounded = |min| {
            value
                .replace('_', "")
                .parse::<i32>()
                .ok()
                .filter(|&number| number >= min)
        };
        match (section, key) {
            ("", "death_storage_duration") => self.history_days = bounded(-1)?,
            ("corpse", "show_equipment") => self.show_equipment = value.parse().ok()?,
            ("corpse", "fall_into_void") => self.fall_into_void = value.parse().ok()?,
            ("corpse", "skeleton_time") => {
                self.life.skeleton_after = u32::try_from(bounded(0)?).ok()?
            }
            ("corpse.despawn", "time") => {
                self.life.empty_despawn_after = u32::try_from(bounded(20)?).ok()?
            }
            ("corpse.despawn", "force_time") => {
                self.life.force_despawn_after =
                    u32::try_from(bounded(-1)?).ok().filter(|&ticks| ticks > 0)
            }
            ("corpse", "lie_on_face" | "lava_damage")
            | ("corpse.access", "only_owner" | "skeleton") => {
                if value.parse::<bool>().ok()? {
                    moddedmc_api::warn(&format!(
                        "Corpse setting {section}.{key} is pending on this server."
                    ));
                }
            }
            _ => return None,
        }
        Some(())
    }

    pub fn keeps_history_at(self, timestamp: i64, now: i64) -> bool {
        self.history_days == -1
            || (self.history_days > 0
                && now.saturating_sub(timestamp) < i64::from(self.history_days) * DAY_MILLIS)
    }
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| {
            i64::try_from(time.as_millis()).unwrap_or(i64::MAX)
        })
}
