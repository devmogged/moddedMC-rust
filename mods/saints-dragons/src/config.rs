//! Server-side settings from SaintsDragonsConfig, read at server startup.
use std::sync::OnceLock;
use toml::Value;

#[derive(Default)]
struct Settings {
    spawning: Option<Value>,
    server: Option<Value>,
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

fn read(name: &str) -> Option<Value> {
    let path = moddedmc_api::data_dir().join("server").join(name);
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str(&text) {
            Ok(value) => Some(value),
            Err(err) => {
                moddedmc_api::warn(&format!(
                    "Saint's Dragons: invalid {}: {err}",
                    path.display()
                ));
                None
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            moddedmc_api::warn(&format!(
                "Saint's Dragons: cannot read {}: {err}",
                path.display()
            ));
            None
        }
    }
}

pub fn load() {
    SETTINGS.get_or_init(|| Settings {
        spawning: read("spawning.toml"),
        server: read("servercommon.toml"),
    });
}

fn boolean(root: Option<&Value>, key: &str, default: bool) -> bool {
    root.and_then(|root| root.get(key))
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn number(root: Option<&Value>, key: &str, default: i32, min: i32, max: i32) -> i32 {
    root.and_then(|root| root.get(key))
        .and_then(Value::as_integer)
        .and_then(|v| i32::try_from(v).ok())
        .filter(|v| (min..=max).contains(v))
        .unwrap_or(default)
}

fn spawning() -> Option<&'static Value> {
    SETTINGS.get()?.spawning.as_ref()?.get("spawning")
}

pub fn custom_spawning(kind: &str) -> bool {
    let name = kind.strip_prefix("saintsdragons:").unwrap_or(kind);
    boolean(spawning(), &format!("{name}CustomSpawningEnabled"), true)
}

pub fn gameplay(key: &str) -> bool {
    boolean(
        SETTINGS
            .get()
            .and_then(|s| s.server.as_ref()?.get("gameplay")),
        key,
        true,
    )
}

pub fn restock_interval() -> i64 {
    i64::from(number(
        SETTINGS.get().and_then(|s| s.server.as_ref()?.get("ivy")),
        "ivyRestockInterval",
        24000,
        20,
        72000,
    ))
}

pub fn spawn_settings(kind: &str, weight: i32, min: i32, max: i32) -> Option<(i32, i32, i32)> {
    spawn_settings_from(spawning(), kind, weight, min, max)
}

fn spawn_settings_from(
    root: Option<&Value>,
    kind: &str,
    weight: i32,
    min: i32,
    max: i32,
) -> Option<(i32, i32, i32)> {
    let name = kind.strip_prefix("saintsdragons:").unwrap_or(kind);
    if !boolean(root, &format!("{name}SpawningEnabled"), true) {
        return None;
    }
    let weight = number(root, &format!("{name}SpawnWeight"), weight, 0, 5000);
    let max = number(root, &format!("{name}MaxGroupSize"), max, 1, 10);
    let min = number(root, &format!("{name}MinGroupSize"), min, 1, 10).min(max);
    (weight > 0).then_some((weight, min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawning_settings_validate_and_disable() {
        let root: Value = toml::from_str("[spawning]\ncindervaneSpawnWeight = 50\ncindervaneMinGroupSize = 6\ncindervaneMaxGroupSize = 2\nmoopSpawningEnabled = false\natroxiiaSpawnWeight = 5001").unwrap();
        let root = root.get("spawning").unwrap();
        assert_eq!(
            spawn_settings_from(Some(root), "saintsdragons:cindervane", 8, 1, 2),
            Some((50, 2, 2))
        );
        assert_eq!(
            spawn_settings_from(Some(root), "saintsdragons:moop", 4, 1, 1),
            None
        );
        assert_eq!(
            spawn_settings_from(Some(root), "saintsdragons:atroxiia", 6, 1, 1),
            Some((6, 1, 1))
        );
    }
}
