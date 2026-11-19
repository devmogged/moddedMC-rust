use std::{
    collections::BTreeMap,
    fs::{self, File},
    path::{Path, PathBuf},
};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Oracle {
    pub minecraft: String,
    pub java: Option<PathBuf>,
    pub args: Vec<String>,
    #[serde(default)]
    directories: BTreeMap<String, PathBuf>,
    #[serde(skip)]
    pub legacy_commands: bool,
}

impl Oracle {
    pub fn read(path: &Path) -> Result<Self, String> {
        let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut oracle: Self = serde_json::from_reader(file)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        oracle.legacy_commands = old_commands(&oracle.minecraft)?;
        if oracle.args.is_empty() {
            return Err("oracle needs Java launch arguments".into());
        }
        let base = path
            .parent()
            .ok_or("oracle descriptor has no parent directory")?;
        if let Some(java) = &mut oracle.java {
            *java = base.join(&*java);
            if !java.is_file() {
                return Err(format!("{} is not a Java executable file", java.display()));
            }
        }
        for (mount, directory) in &mut oracle.directories {
            if !matches!(mount.as_str(), "libraries" | "mods") {
                return Err(format!("unsupported oracle directory {mount}"));
            }
            *directory = base.join(&*directory);
            if !directory.is_dir() {
                return Err(format!("{} is not a directory", directory.display()));
            }
        }
        Ok(oracle)
    }

    pub fn mount(&self, dir: &Path) -> Result<(), String> {
        for (name, source) in &self.directories {
            let target = dir.join(name);
            link_dir(source, &target).map_err(|error| {
                format!(
                    "linking {} at {}: {error}",
                    source.display(),
                    target.display()
                )
            })?;
        }
        Ok(())
    }
}

fn old_commands(version: &str) -> Result<bool, String> {
    let parts = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("invalid Minecraft release {version}"))?;
    match parts.as_slice() {
        [1, 20, 3..=6] | [1, 21] | [1, 21, 0..=10] => Ok(true),
        [1, 21, patch] if *patch >= 11 => Ok(false),
        [major, _] | [major, _, _] if *major >= 26 => Ok(false),
        _ => Err(format!(
            "Minecraft {version} is unsupported for tick-exact oracles; the reference needs /tick"
        )),
    }
}

#[cfg(windows)]
use std::os::windows::fs::{symlink_dir as link_dir, symlink_file as link_file};

#[cfg(unix)]
use std::os::unix::fs::{symlink as link_dir, symlink as link_file};

pub fn stage_subject_mods(source: &Path, dir: &Path) -> Result<(), String> {
    let target = dir.join("mods");
    fs::create_dir(&target).map_err(|error| error.to_string())?;
    let mut count = 0;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "mmc") && path.is_file() {
            let output = target.join(name);
            link_file(&path, &output)
                .map_err(|error| format!("linking {}: {error}", path.display()))?;
            count += 1;
        }
    }
    if count == 0 {
        return Err(format!("no .mmc packs in {}", source.display()));
    }
    Ok(())
}

pub fn resource_id(id: &str) -> bool {
    let key_byte =
        |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'-');
    id.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace.bytes().all(key_byte)
            && path.bytes().all(|b| key_byte(b) || b == b'/')
    })
}

pub fn rewrite(text: &str, ids: &BTreeMap<String, String>, to_reference: bool) -> String {
    let token = |ch: char| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '/' | ':');
    let mut out = String::with_capacity(text.len());
    for part in text.split_inclusive(|ch| !token(ch)) {
        let delimiter_len = part
            .chars()
            .next_back()
            .filter(|&ch| !token(ch))
            .map_or(0, char::len_utf8);
        let (word, delimiter) = part.split_at(part.len() - delimiter_len);
        let replacement = if to_reference {
            ids.get(word)
        } else {
            ids.iter()
                .find(|(_, reference)| *reference == word)
                .map(|(subject, _)| subject)
        };
        out.push_str(replacement.map_or(word, String::as_str));
        out.push_str(delimiter);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_whole_resource_ids_in_both_directions() {
        let ids = BTreeMap::from([("minecraft:oak_boat".into(), "minecraft:boat".into())]);
        let current = r#"execute if entity @e[type=minecraft:oak_boat,nbt={id:"minecraft:oak_boat"}] # minecraft:oak_boat_extra"#;
        let old = r#"execute if entity @e[type=minecraft:boat,nbt={id:"minecraft:boat"}] # minecraft:oak_boat_extra"#;
        assert_eq!(rewrite(current, &ids, true), old);
        assert_eq!(rewrite(old, &ids, false), current);
        assert!(resource_id("example:sub/path"));
        assert!(!resource_id("example:bad:name"));
    }

    #[test]
    fn version_uses_the_expected_rule_names_and_requires_tick() {
        assert_eq!(old_commands("1.21.1"), Ok(true));
        assert_eq!(old_commands("1.21.11"), Ok(false));
        assert_eq!(old_commands("26.3"), Ok(false));
        assert!(matches!(old_commands("1.20.1"), Err(error) if error.contains("/tick")));
    }
}
