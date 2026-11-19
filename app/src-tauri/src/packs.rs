//! Packs the owner builds from this machine: `pack.toml` files in a moddedMC-rust checkout.
//! Building runs `moddedmc-pack` there (docs/PACKS.md) and streams its output as events.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

use crate::store::Store;

#[derive(Serialize)]
pub struct PackMod {
    id: String,
    /// `path`, `mmc`, `git` or `index`: where the mod comes from.
    source: String,
}

#[derive(Serialize)]
pub struct PackTarget {
    name: String,
    triple: String,
    /// The build folder, when this target has been built.
    built: Option<String>,
}

#[derive(Serialize)]
pub struct Pack {
    path: String,
    id: String,
    name: String,
    version: String,
    mods: Vec<PackMod>,
    targets: Vec<PackTarget>,
    /// Why the file can't be used, instead of the fields above.
    error: Option<String>,
}

/// The checkout a pack builds in: the nearest folder above it with `engine/Cargo.toml`.
fn checkout(pack: &Path) -> Option<PathBuf> {
    pack.ancestors()
        .skip(1)
        .find(|d| d.join("engine").join("Cargo.toml").exists() && d.join("packer").exists())
        .map(Path::to_path_buf)
}

fn read_pack(path: &str) -> Pack {
    let mut pack = Pack {
        path: path.to_string(),
        id: String::new(),
        name: Path::new(path)
            .parent()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        version: String::new(),
        mods: Vec::new(),
        targets: Vec::new(),
        error: None,
    };
    let table: toml::Table = match std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| {
            t.parse()
                .map_err(|e: toml::de::Error| e.message().to_string())
        }) {
        Ok(table) => table,
        Err(err) => {
            pack.error = Some(format!("Can't read {path}: {err}"));
            return pack;
        }
    };
    let meta = table.get("pack").and_then(toml::Value::as_table);
    let field = |k: &str| {
        meta.and_then(|m| m.get(k))
            .and_then(toml::Value::as_str)
            .map(str::to_owned)
    };
    pack.id = field("id").unwrap_or_default();
    if let Some(name) = field("name") {
        pack.name = name;
    }
    pack.version = field("version").unwrap_or_default();
    if let Some(mods) = table.get("mods").and_then(toml::Value::as_table) {
        for (id, spec) in mods {
            let source = ["path", "mmc", "git"]
                .into_iter()
                .find(|k| spec.get(k).is_some())
                .unwrap_or("index");
            pack.mods.push(PackMod {
                id: id.clone(),
                source: source.to_string(),
            });
        }
    }
    let out = checkout(Path::new(path)).map(|c| c.join("target").join("packs").join(&pack.id));
    if let Some(targets) = table.get("targets").and_then(toml::Value::as_table) {
        for (name, triple) in targets {
            let dir = out.as_ref().map(|o| o.join(name));
            let built = dir
                .filter(|d| d.join("pack.lock").exists() && d.join(".build-key").exists())
                .map(|d| d.to_string_lossy().into_owned());
            pack.targets.push(PackTarget {
                name: name.clone(),
                triple: triple.as_str().unwrap_or_default().to_string(),
                built,
            });
        }
    }
    if pack.id.is_empty() {
        pack.error = Some("The pack has no [pack] id.".into());
    } else if checkout(Path::new(path)).is_none() {
        pack.error =
            Some("Packs build inside a moddedMC-rust checkout; this one isn't in one.".into());
    }
    pack
}

#[tauri::command]
pub fn packs_list(store: State<'_, Store>) -> Vec<Pack> {
    store.packs().iter().map(|p| read_pack(p)).collect()
}

#[tauri::command]
pub fn pack_add(store: State<'_, Store>, path: String) -> Result<Pack, String> {
    let pack = read_pack(&path);
    if pack.id.is_empty() {
        return Err(pack.error.unwrap_or_else(|| "Not a pack.toml.".into()));
    }
    store.add_pack(path)?;
    Ok(pack)
}

#[tauri::command]
pub fn pack_remove(store: State<'_, Store>, path: String) -> Result<(), String> {
    store.remove_pack(&path)
}

#[derive(Serialize, Clone)]
struct BuildLine {
    path: String,
    target: String,
    line: String,
}

/// Runs the builder for one target. Output lines arrive as `pack-build` events; the result is
/// the build folder.
#[tauri::command]
pub async fn pack_build(app: AppHandle, path: String, target: String) -> Result<String, String> {
    let repo = checkout(Path::new(&path)).ok_or("This pack isn't in a moddedMC-rust checkout.")?;
    let mut command = Command::new("cargo");
    command
        .args([
            "run",
            "-q",
            "--release",
            "-p",
            "moddedmc-pack",
            "--",
            "build",
        ])
        .arg(&path)
        .args(["--target", &target])
        .current_dir(&repo)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .spawn()
        .map_err(|_| "Can't run cargo. Is Rust installed?".to_string())?;
    let mut readers = Vec::new();
    for stream in [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
    ]
    .into_iter()
    .flatten()
    {
        let app = app.clone();
        let (path, target) = (path.clone(), target.clone());
        readers.push(tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            let mut last = String::new();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app.emit(
                    "pack-build",
                    BuildLine {
                        path: path.clone(),
                        target: target.clone(),
                        line: line.clone(),
                    },
                );
                if !line.trim().is_empty() {
                    last = line;
                }
            }
            last
        }));
    }
    let status = child.wait().await.map_err(|e| e.to_string())?;
    let mut last = String::new();
    for reader in readers {
        if let Ok(line) = reader.await
            && !line.is_empty()
        {
            last = line;
        }
    }
    if !status.success() {
        return Err(if last.is_empty() {
            "The build failed.".into()
        } else {
            last
        });
    }
    let pack = read_pack(&path);
    pack.targets
        .into_iter()
        .find(|t| t.name == target)
        .and_then(|t| t.built)
        .ok_or_else(|| "The build finished but left no output.".into())
}

/// `to` relative to the folder `from`, `/`-separated; absolute when they share no root.
fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return to.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = vec!["..".into(); from.len() - common];
    parts.extend(
        to_parts[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// The id a mod file or crate declares: `mod.json` in a `.mmc`, `[package] name` in a crate.
fn mod_id(source: &Path) -> Result<(String, &'static str), String> {
    if source.is_dir() {
        let manifest = source.join("Cargo.toml");
        let text = std::fs::read_to_string(&manifest)
            .map_err(|_| "Pick a .mmc file or a mod crate folder.".to_string())?;
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let name = table
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(toml::Value::as_str)
            .ok_or("That folder's Cargo.toml has no package name.")?;
        return Ok((name.to_string(), "path"));
    }
    let file = std::fs::File::open(source).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "That file isn't a .mmc mod.")?;
    let json: serde_json::Value = {
        let entry = zip
            .by_name("mod.json")
            .map_err(|_| "That .mmc has no mod.json.")?;
        serde_json::from_reader(entry).map_err(|e| format!("mod.json: {e}"))?
    };
    let id = json
        .get("id")
        .and_then(serde_json::Value::as_str)
        .ok_or("That .mmc's mod.json has no id.")?;
    Ok((id.to_string(), "mmc"))
}

fn edit_pack(
    path: &str,
    change: impl FnOnce(&mut toml_edit::Table) -> Result<(), String>,
) -> Result<Pack, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Can't read {path}: {e}"))?;
    let mut doc: toml_edit::DocumentMut = text
        .parse()
        .map_err(|e: toml_edit::TomlError| e.to_string())?;
    let mods = doc
        .entry("mods")
        .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()))
        .as_table_mut()
        .ok_or("The pack's [mods] isn't a table.")?;
    change(mods)?;
    let partial = format!("{path}.partial");
    std::fs::write(&partial, doc.to_string()).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())?;
    Ok(read_pack(path))
}

/// Adds a drop-in mod to a pack: a `.mmc` file or a mod crate folder.
#[tauri::command]
pub fn pack_add_mod(path: String, source: String) -> Result<Pack, String> {
    let source = PathBuf::from(source);
    let (id, key) = mod_id(&source)?;
    let dir = Path::new(&path).parent().ok_or("Bad pack path.")?;
    let value = relative(dir, &source);
    edit_pack(&path, |mods| {
        if mods.contains_key(&id) {
            return Err(format!("The pack already has {id}."));
        }
        let mut spec = toml_edit::InlineTable::new();
        spec.insert(key, value.into());
        mods.insert(&id, toml_edit::value(spec));
        Ok(())
    })
}

#[tauri::command]
pub fn pack_remove_mod(path: String, id: String) -> Result<Pack, String> {
    edit_pack(&path, |mods| {
        mods.remove(&id)
            .map(drop)
            .ok_or(format!("The pack has no {id}."))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        let pack = Path::new("N:/repo/packs/survival");
        assert_eq!(
            relative(pack, Path::new("N:/repo/mods/chunky")),
            "../../mods/chunky"
        );
        assert_eq!(
            relative(pack, Path::new("N:/repo/packs/survival/m.mmc")),
            "m.mmc"
        );
        assert_eq!(relative(pack, Path::new("D:/mods/x.mmc")), "D:/mods/x.mmc");
    }

    #[test]
    fn edits_keep_comments() {
        let dir = std::env::temp_dir().join("moddedmc-pack-edit-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pack.toml");
        std::fs::write(
            &path,
            "# keep me\n[pack]\nid = \"t\"\nname = \"T\"\nversion = \"1\"\n\n[mods]\na = { path = \"../a\" }\n\n[targets]\nlaptop = \"x86_64-pc-windows-msvc\"\n",
        )
        .unwrap();
        let path = path.to_string_lossy().into_owned();
        edit_pack(&path, |mods| {
            mods.remove("a");
            Ok(())
        })
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# keep me\n"));
        assert!(!text.contains("../a"));
        assert!(text.contains("[targets]"));
    }
}
