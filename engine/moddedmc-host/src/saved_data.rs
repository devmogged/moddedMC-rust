//! Save-global mod data. Writes are staged in memory and flushed through the world-save
//! lifecycle to a single atomic file per save (shared by all its dimensions).
//!
//! Keys are reserved during each mod's registration, never retained as raw mod memory,
//! and activate only after the full registration commits.

use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        LazyLock, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use moddedmc_api::abi::{self, Reg, Sink, Str, WorldRef, code};
use pumpkin_nbt::{Nbt, compound::NbtCompound, deserializer::NbtReadHelperJava, tag::NbtTag};

use crate::{content, native};

const MAX_TOTAL_BYTES: usize = 64 << 20;
const MAX_KEYS: usize = 4096;

#[derive(Clone)]
struct RegisteredKey {
    stored: String,
}

#[derive(Default)]
struct Save {
    values: HashMap<String, Vec<u8>>,
    dirty: bool,
    revision: u64,
}

enum Loaded {
    Ready(Save),
    Failed,
}

static KEYS: LazyLock<Mutex<HashMap<u64, RegisteredKey>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SAVES: LazyLock<Mutex<HashMap<PathBuf, Loaded>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_KEY: AtomicU64 = AtomicU64::new(1);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
// Serialize flush attempts so an older snapshot cannot overwrite a newer save.
static FLUSH: Mutex<()> = Mutex::new(());

fn keys() -> MutexGuard<'static, HashMap<u64, RegisteredKey>> {
    KEYS.lock().unwrap_or_else(PoisonError::into_inner)
}
fn saves() -> MutexGuard<'static, HashMap<PathBuf, Loaded>> {
    SAVES.lock().unwrap_or_else(PoisonError::into_inner)
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_.-/".contains(&c))
}

pub static SAVED_DATA: abi::SavedDataV1 = abi::SavedDataV1 {
    size: size_of::<abi::SavedDataV1>(),
    register_key,
    read,
    write,
};

unsafe extern "C" fn register_key(reg: Reg, name: Str, out: *mut u64) -> i32 {
    if out.is_null() {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: registration owns staging and the name throughout the call.
    let staging = unsafe { native::staging(reg) };
    // SAFETY: the caller keeps the borrowed name bytes alive for this registration call.
    let name = unsafe { name.str() };
    if !valid_name(name) {
        return code::INVALID_DEFINITION;
    }
    if staging.saved_keys.iter().any(|(_, other)| other == name) {
        return code::DUPLICATE_ID;
    }
    let handle = NEXT_KEY.fetch_add(1, Ordering::Relaxed);
    if handle == 0 {
        return code::INVALID_DEFINITION;
    }
    staging.saved_keys.push((handle, name.to_owned()));
    // SAFETY: caller provides an output pointer to one u64.
    unsafe { *out = handle };
    code::OK
}

pub fn register(owner: &str, staged: Vec<(u64, String)>) {
    let mut entries = keys();
    for (handle, name) in staged {
        entries.insert(
            handle,
            RegisteredKey {
                stored: format!("{owner}:{name}"),
            },
        );
    }
}

fn data_path(raw: WorldRef) -> Option<PathBuf> {
    // SAFETY: the world pointer is valid only during this synchronous callback.
    let world = unsafe { content::world(raw) }?;
    Some(
        world
            .level
            .level_folder
            .root_folder
            .join("moddedmc")
            .join("saved_data.nbt"),
    )
}

fn decode(bytes: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    if bytes.len() > MAX_TOTAL_BYTES {
        return Err("saved data exceeds maximum file size".into());
    }
    let mut cursor = Cursor::new(bytes);
    let nbt = Nbt::read(&mut NbtReadHelperJava::new(&mut cursor))
        .map_err(|e| format!("cannot parse saved data: {e}"))?;
    if cursor.position() != bytes.len() as u64 {
        return Err("trailing bytes in saved data".into());
    }
    if nbt.root_tag.child_tags.len() > MAX_KEYS {
        return Err("too many saved data keys".into());
    }
    let mut values = HashMap::new();
    let mut total = 0usize;
    for (key, tag) in nbt.root_tag.child_tags {
        let NbtTag::ByteArray(data) = tag else {
            return Err(format!("invalid saved data tag for {key}"));
        };
        if data.len() > abi::MAX_SAVED_DATA_BYTES {
            return Err(format!("oversized saved data key {key}"));
        }
        total += data.len();
        if total > MAX_TOTAL_BYTES {
            return Err("saved data exceeds total capacity".into());
        }
        values.insert(
            key.into_string(),
            data.into_iter().map(|b| b as u8).collect(),
        );
    }
    Ok(values)
}

fn load(path: &Path) -> Loaded {
    match fs::read(path) {
        Ok(bytes) => match decode(&bytes) {
            Ok(values) => Loaded::Ready(Save {
                values,
                ..Save::default()
            }),
            Err(error) => {
                tracing::error!("moddedmc: {}: {error}", path.display());
                Loaded::Failed
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Loaded::Ready(Save::default())
        }
        Err(error) => {
            tracing::error!("moddedmc: cannot read {}: {error}", path.display());
            Loaded::Failed
        }
    }
}

fn get_or_load<'a>(all: &'a mut HashMap<PathBuf, Loaded>, path: &Path) -> &'a mut Loaded {
    all.entry(path.to_owned()).or_insert_with(|| load(path))
}

unsafe extern "C" fn read(raw: WorldRef, handle: u64, out: Sink) -> bool {
    let Some(key) = keys().get(&handle).cloned() else {
        return false;
    };
    let Some(path) = data_path(raw) else {
        return false;
    };
    let data = {
        let mut all = saves();
        match get_or_load(&mut all, &path) {
            Loaded::Ready(save) => save.values.get(&key.stored).cloned(),
            Loaded::Failed => None,
        }
    };
    let Some(data) = data else { return false };
    // SAFETY: the sink copies before the local bytes are dropped.
    unsafe { (out.put)(out.ctx, 0, Str::new(&data)) };
    true
}

unsafe extern "C" fn write(raw: WorldRef, handle: u64, value: Str) -> bool {
    // SAFETY: the ABI borrows the value for this call.
    unsafe { write_value(raw, handle, value, None) }
}

pub unsafe fn commit_claim(raw: WorldRef, owner: &str, handle: u64, value: Str) -> bool {
    // SAFETY: the caller keeps the world and value live during the claim.
    unsafe { write_value(raw, handle, value, Some(owner)) }
}

unsafe fn write_value(raw: WorldRef, handle: u64, value: Str, owner: Option<&str>) -> bool {
    if value.len > abi::MAX_SAVED_DATA_BYTES {
        return false;
    }
    let Some(key) = keys().get(&handle).cloned() else {
        return false;
    };
    if let Some(owner) = owner
        && !key
            .stored
            .split_once(':')
            .is_some_and(|(registered, _)| registered == owner)
    {
        return false;
    }
    let Some(path) = data_path(raw) else {
        return false;
    };
    // SAFETY: the supplied bytes live through this synchronous write.
    let value = unsafe { value.bytes() };
    let committed = owner.is_some();
    let _flush = committed.then(|| FLUSH.lock().unwrap_or_else(PoisonError::into_inner));
    let mut all = saves();
    let Loaded::Ready(save) = get_or_load(&mut all, &path) else {
        return false;
    };
    if !save.values.contains_key(&key.stored) && save.values.len() >= MAX_KEYS {
        return false;
    }
    let current = save.values.get(&key.stored).map_or(0, Vec::len);
    let new_entry_overhead = if save.values.contains_key(&key.stored) {
        0
    } else {
        key.stored.len() + 7 // tag ID + name length + array length
    };
    // Match the actual on-disk NBT byte bound, including metadata and root framing.
    if encoded_size(&save.values) - current + value.len() + new_entry_overhead > MAX_TOTAL_BYTES {
        return false;
    }
    if committed {
        if let Err(error) = replace_committed(&mut save.values, &path, key.stored, value.to_vec()) {
            tracing::error!("moddedmc: cannot commit {}: {error}", path.display());
            return false;
        }
    } else {
        save.values.insert(key.stored, value.to_vec());
    }
    save.dirty = !committed;
    save.revision = save.revision.wrapping_add(1);
    true
}

fn replace_committed(
    values: &mut HashMap<String, Vec<u8>>,
    path: &Path,
    key: String,
    bytes: Vec<u8>,
) -> std::io::Result<()> {
    let previous = values.insert(key.clone(), bytes);
    if let Err(error) = persist(path, values) {
        match previous {
            Some(previous) => values.insert(key, previous),
            None => values.remove(&key),
        };
        return Err(error);
    }
    Ok(())
}

fn encoded_size(values: &HashMap<String, Vec<u8>>) -> usize {
    4 + values
        .iter()
        .map(|(key, data)| key.len() + 7 + data.len())
        .sum::<usize>()
}

fn encode(values: &HashMap<String, Vec<u8>>) -> Vec<u8> {
    let mut compound = NbtCompound::new();
    for (name, bytes) in values {
        compound.child_tags.insert(
            name.clone().into(),
            NbtTag::ByteArray(bytes.iter().map(|&b| b as i8).collect::<Vec<_>>().into()),
        );
    }
    Nbt::from(compound).write().to_vec()
}

fn persist(path: &Path, values: &HashMap<String, Vec<u8>>) -> std::io::Result<()> {
    let data = encode(values);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let tmp = path.with_extension(format!("nbt.{}.{}.tmp", std::process::id(), nonce));
    let result = (|| {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(&data)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Flush dirty snapshots on world save and shutdown; concurrent writes stay marked dirty.
pub fn flush_all() {
    let _guard = FLUSH.lock().unwrap_or_else(PoisonError::into_inner);
    let snapshots: Vec<_> = {
        let all = saves();
        all.iter()
            .filter_map(|(path, loaded)| match loaded {
                Loaded::Ready(save) if save.dirty => {
                    Some((path.clone(), save.revision, save.values.clone()))
                }
                Loaded::Ready(_) | Loaded::Failed => None,
            })
            .collect()
    };
    for (path, revision, values) in snapshots {
        match persist(&path, &values) {
            Ok(()) => {
                let mut all = saves();
                if let Some(Loaded::Ready(save)) = all.get_mut(&path)
                    && save.revision == revision
                {
                    save.dirty = false;
                }
            }
            Err(error) => tracing::error!("moddedmc: cannot save {}: {error}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, encoded_size, replace_committed, valid_name};
    use std::collections::HashMap;

    #[test]
    fn keys_are_bounded_and_safe() {
        assert!(valid_name("create_tracks"));
        assert!(valid_name("railway/nether.graph-1"));
        for bad in [
            "",
            "../escape",
            "abc//def",
            "abc/./def",
            "OtherMod",
            "bad:key",
        ] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(!valid_name(&"x".repeat(129)));
    }

    #[test]
    fn nbt_round_trip_preserves_binary_and_namespace() {
        let values = HashMap::from([
            ("a:graph".to_owned(), vec![0, 128, 255, 42]),
            ("b:graph".to_owned(), Vec::new()),
        ]);
        let encoded = encode(&values);
        assert_eq!(encoded_size(&values), encoded.len());
        assert_eq!(decode(&encoded).ok(), Some(values));
        let mut trailing = encoded;
        trailing.push(1);
        assert!(decode(&trailing).is_err());
    }

    #[test]
    fn failed_disk_commit_restores_the_previous_value() {
        let blocker =
            std::env::temp_dir().join(format!("moddedmc-save-blocker-{}", uuid::Uuid::new_v4()));
        std::fs::File::create_new(&blocker).expect("create a file that blocks the save directory");
        let original = HashMap::from([("fixture:claim".to_owned(), vec![1, 2])]);
        let mut values = original.clone();
        let result = replace_committed(
            &mut values,
            &blocker.join("data.nbt"),
            "fixture:claim".to_owned(),
            vec![3],
        );
        std::fs::remove_file(blocker).expect("remove the temporary blocking file");
        assert!(result.is_err());
        assert_eq!(values, original);
    }
}
