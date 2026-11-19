//! `moddedmc:resources` v1: the merged datapack files.

use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use moddedmc_api::abi::{self, MAX_RESOURCE_BYTES, Sink, Str};
use pumpkin_core::data::datapack::LoadedDatapack;

use crate::native::SERVER;

/// How deep `list` walks below the directory; vanilla has no limit, real packs stay far below it.
const MAX_DEPTH: usize = 16;
/// Ids one `list` call returns, so a huge pack can't stall the tick indefinitely.
const MAX_LISTED: usize = 1 << 16;

/// Vanilla `ResourceLocation.isAllowedInResourceLocation` for paths.
fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
}

/// Datapacks in priority order, highest last. `None` before they first load.
fn packs() -> Option<Vec<LoadedDatapack>> {
    let manager = &SERVER.get()?.datapack_manager;
    (manager.generation() != 0).then(|| manager.get_loaded_packs())
}

fn walk(dir: &Path, prefix: &str, depth: usize, out: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_LISTED {
            return;
        }
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let id = format!("{prefix}/{name}");
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if depth < MAX_DEPTH && valid_path(&name) {
                walk(&entry.path(), &id, depth + 1, out);
            }
        } else if kind.is_file() && valid_path(&name) {
            out.insert(id);
        }
    }
}

unsafe extern "C" fn list(directory: Str, out: Sink) -> bool {
    // SAFETY: the directory is borrowed for this call.
    let directory = unsafe { directory.str() };
    let (true, Some(packs)) = (valid_path(directory), packs()) else {
        return false;
    };
    let mut ids = BTreeSet::new();
    for pack in &packs {
        let Ok(namespaces) = fs::read_dir(pack.root_path.join("data")) else {
            continue;
        };
        for namespace in namespaces.flatten() {
            let Ok(name) = namespace.file_name().into_string() else {
                continue;
            };
            if valid_namespace(&name) {
                let root = namespace.path().join(directory);
                walk(&root, &format!("{name}:{directory}"), 0, &mut ids);
            }
        }
    }
    for id in &ids {
        // SAFETY: the id is live until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, 0, Str::new(id.as_bytes())) };
    }
    true
}

fn resolve(packs: &[LoadedDatapack], id: &str) -> Option<PathBuf> {
    let (namespace, path) = id.split_once(':')?;
    if !valid_namespace(namespace) || !valid_path(path) {
        return None;
    }
    packs.iter().rev().find_map(|pack| {
        let file = pack.root_path.join("data").join(namespace).join(path);
        file.is_file().then_some(file)
    })
}

unsafe extern "C" fn read(id: Str, out: Sink) -> bool {
    // SAFETY: the id is borrowed for this call.
    let id = unsafe { id.str() };
    let Some(file) = packs().and_then(|packs| resolve(&packs, id)) else {
        return false;
    };
    let Ok(handle) = fs::File::open(file) else {
        return false;
    };
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_RESOURCE_BYTES).unwrap_or(u64::MAX);
    if handle.take(limit + 1).read_to_end(&mut bytes).is_err() || bytes.len() > MAX_RESOURCE_BYTES {
        return false;
    }
    // SAFETY: the bytes are live until the synchronous sink returns.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn generation() -> u64 {
    SERVER
        .get()
        .map_or(0, |server| server.datapack_manager.generation())
}

pub static RESOURCES: abi::ResourcesV1 = abi::ResourcesV1 {
    size: size_of::<abi::ResourcesV1>(),
    list,
    read,
    generation,
};
