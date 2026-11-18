//! `pack.toml` and the mods it names, resolved to something buildable. See docs/PACKS.md.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use toml::{Table, Value};

#[derive(Deserialize)]
pub struct Pack {
    pub pack: Meta,
    #[serde(default)]
    pub mods: BTreeMap<String, ModSpec>,
    #[serde(default)]
    pub targets: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct Meta {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
pub enum ModSpec {
    /// A crate on disk: a drop-in mod (cdylib) or a compiled-in port.
    Path { path: PathBuf },
    /// A built `.mmc` file.
    File { mmc: PathBuf },
    /// A crate at a git revision.
    Git { git: String, rev: String },
    /// A mod from the devmogged mod index.
    Published { version: String, source: String },
}

pub enum Kind {
    /// Built as a library and shipped as `<id>.mmc` in the server's `mods/`.
    DropIn { package: String, lib: String },
    /// Linked into the pack server; `lib::builtin()` registers it.
    Port { package: String, lib: String },
    /// A `.mmc` someone else built.
    Prebuilt,
}

pub struct Resolved {
    pub id: String,
    pub version: String,
    pub kind: Kind,
    /// The crate directory or the `.mmc` file.
    pub path: PathBuf,
    /// SHA-256 of the sources (or the file), so the lock changes when the mod does.
    pub hash: String,
}

pub fn load(path: &Path) -> Result<Pack, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    let pack: Pack = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if pack.pack.id.is_empty() || !pack.pack.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(format!("pack id {:?} must be letters, digits and dashes", pack.pack.id));
    }
    if pack.targets.is_empty() {
        return Err("the pack has no [targets]".into());
    }
    Ok(pack)
}

pub fn resolve(base: &Path, id: &str, spec: &ModSpec) -> Result<Resolved, String> {
    match spec {
        ModSpec::Path { path } => resolve_crate(id, &base.join(path)),
        ModSpec::File { mmc } => {
            let path = base.join(mmc);
            let bytes = fs::read(&path).map_err(|e| format!("{id}: can't read {}: {e}", path.display()))?;
            let version = mmc_version(&bytes).map_err(|e| format!("{id}: {}: {e}", path.display()))?;
            Ok(Resolved {
                id: id.to_string(),
                version,
                kind: Kind::Prebuilt,
                hash: hex(&Sha256::digest(&bytes)),
                path,
            })
        }
        ModSpec::Git { git, rev } => Err(format!(
            "{id}: git sources ({git} at {rev}) aren't supported yet; check it out and use {{ path = \"...\" }}"
        )),
        ModSpec::Published { version, source } => Err(format!(
            "{id} {version}: nothing is published in the {source:?} index yet; use {{ path = \"...\" }}"
        )),
    }
}

fn resolve_crate(id: &str, dir: &Path) -> Result<Resolved, String> {
    let manifest = dir.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).map_err(|e| format!("{id}: can't read {}: {e}", manifest.display()))?;
    let table: Table = text.parse().map_err(|e| format!("{}: {e}", manifest.display()))?;
    let package = table.get("package").and_then(Value::as_table).ok_or_else(|| format!("{}: no [package]", manifest.display()))?;
    let name = package.get("name").and_then(Value::as_str).ok_or_else(|| format!("{}: no package name", manifest.display()))?;
    let version = package
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{}: give the package an explicit version, packs lock it", manifest.display()))?;
    let lib_table = table.get("lib").and_then(Value::as_table);
    let lib = lib_table
        .and_then(|t| t.get("name"))
        .and_then(Value::as_str)
        .map_or_else(|| name.replace('-', "_"), str::to_owned);
    let is_port = package
        .get("metadata")
        .and_then(|m| m.get("moddedmc"))
        .and_then(|m| m.get("kind"))
        .and_then(Value::as_str)
        == Some("port");
    let is_cdylib = lib_table
        .and_then(|t| t.get("crate-type"))
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|t| t.as_str() == Some("cdylib")));
    let kind = if is_port {
        Kind::Port { package: name.to_string(), lib }
    } else if is_cdylib {
        Kind::DropIn { package: name.to_string(), lib }
    } else {
        return Err(format!(
            "{id}: {} is neither a drop-in mod (crate-type cdylib) nor a port ([package.metadata.moddedmc] kind = \"port\")",
            manifest.display()
        ));
    };
    Ok(Resolved {
        id: id.to_string(),
        version: version.to_string(),
        kind,
        hash: hash_tree(dir)?,
        path: dir.to_path_buf(),
    })
}

/// The version in a `.mmc`'s `mod.json`.
fn mmc_version(bytes: &[u8]) -> Result<String, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a .mmc: {e}"))?;
    let entry = archive.by_name("mod.json").map_err(|_| "no mod.json".to_string())?;
    let manifest: serde_json::Value = serde_json::from_reader(entry).map_err(|e| format!("bad mod.json: {e}"))?;
    manifest["version"].as_str().map(str::to_owned).ok_or_else(|| "mod.json has no version".into())
}

/// Hash of every file under `dir` except build output, in a stable order.
pub fn hash_tree(dir: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for relative in files {
        let bytes = fs::read(dir.join(&relative)).map_err(|e| format!("can't read {relative}: {e}"))?;
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex(&hasher.finalize()))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("can't list {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name();
        if name == "target" || name.to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else if let Ok(relative) = path.strip_prefix(root) {
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
