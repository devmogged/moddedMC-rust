//! Loads mods from the server's `mods/` folder and implements the host side of the stable mod
//! ABI (`api/src/abi.rs`, docs/MOD-ABI.md).
//!
//! - `<name>.mmc`: a zip with `mod.json` and one library per platform (`windows-x86_64.dll`, ...).
//!   The library for this platform is extracted to `mods/.cache/` and loaded.
//! - `<name>.jar`: a Fabric mod. It is converted once by `recompiler/build-mod.ps1` into
//!   `mods/.cache/<name>-<hash>.<ext>` and loaded from there.
//! - A bare `.dll`/`.so`/`.dylib`: loaded as is (handy while developing a mod).

use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock},
};

use moddedmc_api::abi::{
    self, ABI_SYMBOL, ABI_VERSION, BiomesV1, BlockCall, BlockEvent, BlockWrite, BlocksV1, Defs,
    ENTRY_SYMBOL, EntitiesV1, EntityId, HostApi, ID_SYMBOL, MenuButton, MenusV1, ModApi, PlayerRef,
    Pos, Property, Reg, Sink, Str, TickEntry, Vec3, WorldV1, code, level,
};
use pumpkin_core::{
    command::{
        CommandSource,
        argument_builder::{ArgumentBuilder, command},
        context::CommandContext,
        node::{CommandExecutor, CommandExecutorResult, detached::CommandDetachedNode},
    },
    entity::{EntityBase, player::Player, r#type::from_type},
    server::Server,
    world::World,
};
use pumpkin_data::{
    Block, BlockStateId,
    entity::EntityType,
    modded::{self, BlockDef},
};
use pumpkin_protocol::java::client::dialog::{ActionButton, Dialog, DialogAction, DialogBody};
use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    text::TextComponent,
};
use pumpkin_world::world::BlockFlags;
use sha2::{Digest, Sha256};

use crate::Staging;

pub static SERVER: OnceLock<Arc<Server>> = OnceLock::new();

const PLATFORM: &str = if cfg!(windows) {
    "windows"
} else if cfg!(target_os = "macos") {
    "macos"
} else {
    "linux"
};
const ARCH: &str = std::env::consts::ARCH;
const EXT: &str = std::env::consts::DLL_EXTENSION;

/// A loaded mod. Its library is never unloaded: it lives until the process exits.
#[derive(Clone, Copy)]
pub struct Native {
    pub api: &'static ModApi,
    pub id: &'static str,
}

/// Loads every mod in `dir`. Problems with one mod are logged and skip only that mod.
pub fn load_all(dir: &Path) -> Vec<Native> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    files.sort();
    let cache = dir.join(".cache");
    let mut mods = Vec::new();
    for path in files.iter().filter(|p| p.is_file()) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let library = match ext {
            "mmc" => extract(path, &cache),
            "jar" => convert(path, &cache),
            e if e == EXT => Ok(path.clone()),
            _ => continue,
        };
        match library.and_then(|lib| load(&lib)) {
            Ok(m) if mods.iter().any(|o: &Native| o.id == m.id) => {
                tracing::error!(
                    "moddedmc: {} has the same id as another mod; skipped",
                    path.display()
                );
            }
            Ok(m) => {
                // SAFETY: the version string is static.
                let version = unsafe { m.api.version.str() };
                tracing::info!(
                    "moddedmc: loaded {} {version} from {}",
                    m.id,
                    path.display()
                );
                mods.push(m);
            }
            Err(e) => tracing::error!("moddedmc: {}: {e}", path.display()),
        }
    }
    mods
}

fn hash(bytes: &[u8]) -> String {
    use std::fmt::Write;
    Sha256::digest(bytes)
        .iter()
        .take(8)
        .fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "mod".into(), |s| s.to_string_lossy().into_owned())
}

/// Extracts this platform's library from a `.mmc` file, once per file content.
fn extract(path: &Path, cache: &Path) -> Result<PathBuf, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let out = cache.join(format!("{}-{}.{EXT}", stem(path), hash(&bytes)));
    if out.exists() {
        return Ok(out);
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut manifest = String::new();
    zip.by_name("mod.json")
        .map_err(|_| "not a mod package: no mod.json".to_owned())?
        .read_to_string(&mut manifest)
        .map_err(|e| e.to_string())?;
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest).map_err(|e| format!("mod.json: {e}"))?;
    if manifest["abi"].as_u64() != Some(u64::from(ABI_VERSION)) {
        return Err(format!(
            "built for mod ABI {}, this server has {ABI_VERSION}",
            manifest["abi"]
        ));
    }
    let name = format!("{PLATFORM}-{ARCH}.{EXT}");
    let mut library = Vec::new();
    zip.by_name(&name)
        .map_err(|_| format!("has no build for this platform ({name})"))?
        .read_to_end(&mut library)
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(cache).map_err(|e| e.to_string())?;
    fs::write(&out, library).map_err(|e| e.to_string())?;
    Ok(out)
}

/// Converts a Fabric `.jar` into a native mod, once per jar content.
fn convert(path: &Path, cache: &Path) -> Result<PathBuf, String> {
    let mut bytes = fs::read(path).map_err(|e| e.to_string())?;
    // A new server version may come with a new converter, so its output is cached per version.
    bytes.extend_from_slice(env!("CARGO_PKG_VERSION").as_bytes());
    let out = cache.join(format!("{}-{}.{EXT}", stem(path), hash(&bytes)));
    if out.exists() {
        return Ok(out);
    }
    let script = converter().ok_or(
        "Fabric mods need the converter: set MODDEDMC_CONVERTER to recompiler/build-mod.ps1",
    )?;
    tracing::info!(
        "moddedmc: converting {} to native code (first start with this jar only)",
        path.display()
    );
    // shortcut: the converter is a PowerShell script from the repo checkout; ship a converter bundle for servers without one.
    let status = Command::new(if cfg!(windows) { "powershell" } else { "pwsh" })
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-Jar")
        .arg(path)
        .arg("-OutFile")
        .arg(&out)
        .status()
        .map_err(|e| format!("cannot run {}: {e}", script.display()))?;
    if status.success() && out.exists() {
        Ok(out)
    } else {
        Err(format!("conversion failed ({status})"))
    }
}

/// `MODDEDMC_CONVERTER`, else `recompiler/build-mod.ps1` in a checkout above the server binary.
fn converter() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MODDEDMC_CONVERTER") {
        return Some(path.into());
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .map(|dir| dir.join("recompiler").join("build-mod.ps1"))
        .find(|p| p.exists())
}

fn load(path: &Path) -> Result<Native, String> {
    // SAFETY: loading a mod runs its code; mods are trusted like server plugins.
    let library = unsafe { libloading::Library::new(path) }.map_err(|e| e.to_string())?;
    // SAFETY: the symbols have the types the ABI documents.
    let api = unsafe {
        let abi = library
            .get::<*const u32>(ABI_SYMBOL)
            .map_err(|_| "not a moddedMC mod (no MODDEDMC_ABI)".to_owned())?;
        let abi = **abi;
        if abi != ABI_VERSION {
            return Err(format!(
                "built for mod ABI {abi}, this server has {ABI_VERSION}"
            ));
        }
        let id = library
            .get::<*const Str>(ID_SYMBOL)
            .map_err(|_| "not a moddedMC mod (no MODDEDMC_ID)".to_owned())?;
        let id = (**id).str();
        if !crate::valid_namespace(id) {
            return Err(format!("invalid mod id {id:?}"));
        }
        let entry = library
            .get::<extern "C" fn(&'static HostApi) -> *const ModApi>(ENTRY_SYMBOL)
            .map_err(|_| "not a moddedMC mod (no moddedmc_mod)".to_owned())?;
        let api = entry(Box::leak(Box::new(host_api(id))));
        if api.is_null() {
            return Err("the mod refused to start (wrong ABI or a panic)".into());
        }
        Native { api: &*api, id }
    };
    // Never unloaded: the mod's code and statics must outlive every call into it.
    std::mem::forget(library);
    Ok(api)
}

// ---------------------------------------------------------------- host functions

/// The host table handed to one mod.
fn host_api(id: &str) -> HostApi {
    let dir = Path::new("config").join(id);
    // Mods used to be Pumpkin plugins with their data in plugins/<id>; keep it.
    let old = Path::new("plugins").join(id);
    if !dir.exists() && old.is_dir() {
        let _ = fs::create_dir_all("config");
        match fs::rename(&old, &dir) {
            Ok(()) => tracing::info!("moddedmc: moved {} to {}", old.display(), dir.display()),
            Err(e) => tracing::warn!("moddedmc: could not move {}: {e}", old.display()),
        }
    }
    let _ = fs::create_dir_all(&dir);
    let dir: &'static str = Box::leak(dir.to_string_lossy().into_owned().into_boxed_str());
    HostApi {
        abi_version: ABI_VERSION,
        size: size_of::<HostApi>(),
        log,
        data_dir: Str::from(dir),
        add_block_entity,
        add_block,
        add_command,
        player_name,
        player_dimension,
        player_position,
        send_message,
        reply,
        give_block,
        teleport,
        show_menu,
        tick_count,
        dimension,
        query_interface,
    }
}

/// # Safety
/// `s` is a live borrowed string.
unsafe fn text(s: Str) -> String {
    // SAFETY: the caller's contract.
    String::from_utf8_lossy(unsafe { s.bytes() }).into_owned()
}

/// # Safety
/// `p` is null or a player reference the host handed out for the current call.
const unsafe fn player<'a>(p: PlayerRef) -> Option<&'a Player> {
    // SAFETY: the caller's contract; the host keeps the `Arc<Player>` alive for the call.
    unsafe { p.0.cast::<Player>().as_ref() }
}

pub fn player_ref(p: &Arc<Player>) -> PlayerRef {
    PlayerRef(Arc::as_ptr(p).cast())
}

unsafe extern "C" fn log(level: u32, message: Str) {
    // SAFETY: borrowed for the call.
    let message = unsafe { text(message) };
    match level {
        level::ERROR => tracing::error!("{message}"),
        level::WARN => tracing::warn!("{message}"),
        _ => tracing::info!("{message}"),
    }
}

/// # Safety
/// `reg` is the `Staging` passed to the current `register` call.
const unsafe fn staging<'a>(reg: Reg) -> &'a mut Staging<'a> {
    // SAFETY: the caller's contract.
    unsafe { &mut *reg.cast::<Staging<'a>>() }
}

unsafe extern "C" fn add_block_entity(
    reg: Reg,
    id: Str,
    state_version: u32,
    initial_state: Str,
    tick_interval: u32,
    out_index: *mut u32,
) -> i32 {
    // SAFETY: live registration and borrowed arguments.
    unsafe {
        match staging(reg).add_block_entity(
            text(id),
            state_version,
            initial_state.bytes().to_vec(),
            (tick_interval > 0).then_some(tick_interval),
        ) {
            Ok(index) => {
                if !out_index.is_null() {
                    *out_index = index;
                }
                code::OK
            }
            Err(e) => e,
        }
    }
}

unsafe extern "C" fn add_block(
    reg: Reg,
    id: Str,
    display_name: Str,
    stand_in: Str,
    block_entity: Str,
) -> i32 {
    // SAFETY: live registration and borrowed arguments.
    unsafe {
        staging(reg)
            .add_block(
                text(id),
                text(display_name),
                &text(stand_in),
                &text(block_entity),
            )
            .err()
            .unwrap_or(code::OK)
    }
}

unsafe extern "C" fn add_command(reg: Reg, name: Str, description: Str, tag: u64) -> i32 {
    // SAFETY: borrowed arguments.
    let (name, description) = unsafe { (text(name), text(description)) };
    if name.is_empty() || name.contains(char::is_whitespace) {
        return code::INVALID_DEFINITION;
    }
    // SAFETY: live registration.
    unsafe { staging(reg) }
        .commands
        .push((name, description, tag));
    code::OK
}

unsafe extern "C" fn player_name(p: PlayerRef) -> Str {
    // SAFETY: a live player; the name lives as long as it.
    unsafe { player(p) }.map_or(Str::EMPTY, |p| Str::from(p.gameprofile.name.as_str()))
}

unsafe extern "C" fn player_dimension(p: PlayerRef) -> Str {
    // SAFETY: a live player; dimension names are static.
    unsafe { player(p) }.map_or(Str::EMPTY, |p| {
        Str::from(p.world().dimension.minecraft_name)
    })
}

unsafe extern "C" fn player_position(p: PlayerRef, out: *mut [f64; 5]) {
    // SAFETY: a live player.
    let Some(p) = (unsafe { player(p) }) else {
        return;
    };
    let entity = &p.living_entity.entity;
    let pos = entity.pos.load();
    let values = [
        pos.x,
        pos.y,
        pos.z,
        f64::from(entity.yaw.load()),
        f64::from(entity.pitch.load()),
    ];
    if !out.is_null() {
        // SAFETY: the mod passes room for 5 doubles.
        unsafe { *out = values };
    }
}

unsafe extern "C" fn send_message(p: PlayerRef, message: Str, action_bar: bool) {
    // SAFETY: a live player and a borrowed string.
    unsafe {
        if let Some(p) = player(p) {
            p.send_system_message_raw(&TextComponent::text(text(message)), action_bar);
        }
    }
}

unsafe extern "C" fn reply(source: *const c_void, message: Str) {
    // SAFETY: `source` is the CommandSource of the command being run.
    unsafe {
        if let Some(source) = source.cast::<CommandSource>().as_ref() {
            source.send_message(TextComponent::text(text(message)));
        }
    }
}

unsafe extern "C" fn give_block(p: PlayerRef, block_id: Str) -> bool {
    // SAFETY: a live player and a borrowed string.
    unsafe { player(p) }.is_some_and(|p| crate::give_block(p, &unsafe { text(block_id) }))
}

unsafe extern "C" fn teleport(
    p: PlayerRef,
    dimension: Str,
    x: f64,
    y: f64,
    z: f64,
    yaw: f32,
    pitch: f32,
) -> bool {
    // SAFETY: a live player and a borrowed string.
    let (Some(p), dimension) = (unsafe { player(p) }, unsafe { text(dimension) }) else {
        return false;
    };
    let Some(world) = SERVER.get().and_then(|s| {
        s.worlds
            .load()
            .iter()
            .find(|w| w.dimension.minecraft_name == dimension)
            .cloned()
    }) else {
        return false;
    };
    let keep = |v: f32| (!v.is_nan()).then_some(v);
    p.teleport(Vector3::new(x, y, z), keep(yaw), keep(pitch), world);
    true
}

unsafe extern "C" fn show_menu(
    p: PlayerRef,
    title: Str,
    body: Str,
    buttons: *const MenuButton,
    count: usize,
) {
    // SAFETY: a live player and `count` borrowed buttons.
    unsafe {
        let Some(p) = player(p) else { return };
        let buttons = if count == 0 || buttons.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(buttons, count)
        };
        let dialog = Dialog {
            r#type: "minecraft:multi_action".into(),
            title: TextComponent::text(text(title)),
            body: vec![DialogBody::PlainMessage {
                contents: TextComponent::text(text(body)),
            }],
            inputs: vec![],
            buttons: buttons
                .iter()
                .map(|b| ActionButton {
                    text: TextComponent::text(text(b.label)),
                    tooltip: (b.tooltip.len > 0).then(|| TextComponent::text(text(b.tooltip))),
                    width: None,
                    action: DialogAction::Custom {
                        id: text(b.action),
                        payload: None,
                    },
                })
                .collect(),
            links: vec![],
            exit_action: None,
            after_action: Some("pop".into()),
            can_close_with_escape: true,
            external_title: None,
        };
        crate::show_dialog(p, &dialog);
    }
}

unsafe extern "C" fn tick_count() -> u64 {
    SERVER.get().map_or(0, |s| {
        u64::try_from(s.tick_count.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(0)
    })
}

unsafe extern "C" fn dimension(index: usize) -> Str {
    SERVER
        .get()
        .and_then(|s| {
            s.worlds
                .load()
                .get(index)
                .map(|w| w.dimension.minecraft_name)
        })
        .map_or(Str::EMPTY, Str::from)
}

// ---------------------------------------------------------------- interfaces

unsafe extern "C" fn query_interface(name: Str) -> *const c_void {
    // SAFETY: borrowed for the call.
    match unsafe { name.str() } {
        abi::WORLD_V1 => std::ptr::from_ref(&WORLD).cast(),
        abi::BLOCKS_V1 => std::ptr::from_ref(&BLOCKS).cast(),
        abi::ENTITIES_V1 => std::ptr::from_ref(&ENTITIES).cast(),
        abi::MENUS_V1 => std::ptr::from_ref(&MENUS).cast(),
        abi::BIOMES_V1 => std::ptr::from_ref(&BIOMES).cast(),
        _ => std::ptr::null(),
    }
}

static WORLD: WorldV1 = WorldV1 {
    size: size_of::<WorldV1>(),
    get_blocks,
    set_blocks,
    state_id,
    state_name,
    place_structure,
};

unsafe extern "C" fn place_structure(dimension: Str, origin: Pos, template: Str) -> i64 {
    use pumpkin_world::generation::structure::template::StructureTemplate;
    // SAFETY: borrowed for the call.
    let (dimension, bytes) = unsafe { (dimension.str(), template.bytes()) };
    let parsed = if bytes.starts_with(&[0x1f, 0x8b]) {
        StructureTemplate::from_nbt_bytes(bytes).ok()
    } else {
        let mut reader =
            pumpkin_nbt::deserializer::NbtReadHelperJava::new(std::io::Cursor::new(bytes));
        pumpkin_nbt::Nbt::read(&mut reader)
            .ok()
            .and_then(|nbt| StructureTemplate::from_nbt_compound(&nbt.root_tag).ok())
    };
    let (Some(world), Some(template)) = (world(dimension), parsed) else {
        return -1;
    };
    let Some(palette) = template.palettes.first() else {
        return 0;
    };
    let mut written = 0;
    for block in palette.blocks() {
        let props: Vec<String> = block
            .state
            .properties
            .iter()
            .map(|p| format!("{}={}", p.key_str(), p.value_str()))
            .collect();
        let name = if props.is_empty() {
            block.state.name.clone()
        } else {
            format!("{}[{}]", block.state.name, props.join(","))
        };
        let Some(state) = parse_state(&name) else {
            continue;
        };
        let pos = BlockPos::new(
            origin.x + block.pos.x,
            origin.y + block.pos.y,
            origin.z + block.pos.z,
        );
        if world.get_block_state_id_if_loaded(&pos).is_some() {
            world.set_block_state(&pos, state, BlockFlags::NOTIFY_ALL);
            written += 1;
        }
    }
    written
}

pub fn world(dimension: &str) -> Option<Arc<World>> {
    SERVER.get().and_then(|s| {
        s.worlds
            .load()
            .iter()
            .find(|w| w.dimension.minecraft_name == dimension)
            .cloned()
    })
}

const fn block_pos(p: &Pos) -> BlockPos {
    BlockPos(Vector3::new(p.x, p.y, p.z))
}

unsafe extern "C" fn get_blocks(
    dimension: Str,
    positions: *const Pos,
    count: usize,
    out: *mut u32,
) -> bool {
    // SAFETY: borrowed for the call.
    let Some(world) = world(unsafe { dimension.str() }) else {
        return false;
    };
    if count == 0 {
        return true;
    }
    // SAFETY: the mod passes `count` positions and room for `count` ids.
    let (positions, out) = unsafe {
        (
            std::slice::from_raw_parts(positions, count),
            std::slice::from_raw_parts_mut(out, count),
        )
    };
    for (pos, out) in positions.iter().zip(out) {
        *out = world
            .get_block_state_id_if_loaded(&block_pos(pos))
            .map_or(abi::NO_STATE, |s| u32::from(s.as_u16()));
    }
    true
}

unsafe extern "C" fn set_blocks(dimension: Str, writes: *const BlockWrite, count: usize) -> usize {
    // SAFETY: borrowed for the call.
    let Some(world) = world(unsafe { dimension.str() }) else {
        return 0;
    };
    if count == 0 {
        return 0;
    }
    // SAFETY: the mod passes `count` writes.
    let writes = unsafe { std::slice::from_raw_parts(writes, count) };
    let mut written = 0;
    for write in writes {
        let pos = block_pos(&write.pos);
        let Some(state) = raw_state(write.state) else {
            continue;
        };
        if world.get_block_state_id_if_loaded(&pos).is_none() {
            continue;
        }
        world.set_block_state(&pos, state, BlockFlags::NOTIFY_ALL);
        written += 1;
    }
    written
}

/// `minecraft:oak_log[axis=x]` to a state id.
fn parse_state(name: &str) -> Option<BlockStateId> {
    let (block, props) = match name.split_once('[') {
        Some((block, rest)) => (block, rest.strip_suffix(']')?),
        None => (name, ""),
    };
    let block = Block::from_name(block.strip_prefix("minecraft:").unwrap_or(block))?;
    if modded::is_modded(block.default_state.id) {
        let pairs = props.split(',').filter(|p| !p.is_empty()).map(|p| {
            p.split_once('=')
                .map_or(("", ""), |(k, v)| (k.trim(), v.trim()))
        });
        return modded::state_with(block, pairs);
    }
    let props = props
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (key, value) = p.split_once('=')?;
            block.parse_property(key.trim(), value.trim()).ok()
        })
        .collect::<Option<Vec<_>>>()?;
    Some(block.apply_properties(block.default_state.id, &props))
}

unsafe extern "C" fn state_id(name: Str) -> u32 {
    // SAFETY: borrowed for the call.
    parse_state(unsafe { name.str() }).map_or(abi::NO_STATE, |s| u32::from(s.as_u16()))
}

/// A state's full name, e.g. `minecraft:oak_log[axis=x]`.
fn full_name(state: BlockStateId) -> String {
    let block = Block::from_state_id(state);
    if let Some(name) = modded::state_name(state) {
        return name;
    }
    let props: Vec<String> = block
        .properties(state)
        .map_or_else(Vec::new, |p| p.to_props())
        .iter()
        .map(|p| format!("{}={}", p.key().to_str(), p.value_str()))
        .collect();
    if props.is_empty() {
        format!("minecraft:{}", block.name)
    } else {
        format!("minecraft:{}[{}]", block.name, props.join(","))
    }
}

/// A vanilla or modded state id from a mod.
fn raw_state(state: u32) -> Option<BlockStateId> {
    u16::try_from(state).ok().and_then(modded::state_id)
}

unsafe extern "C" fn state_name(state: u32, out: Sink) -> bool {
    let Some(state) = raw_state(state) else {
        return false;
    };
    let name = full_name(state);
    // SAFETY: the mod's sink copies before returning.
    unsafe { (out.put)(out.ctx, 0, Str::from(name.as_str())) };
    true
}

// ---------------------------------------------------------------- blocks/1

static BLOCKS: BlocksV1 = BlocksV1 {
    size: size_of::<BlocksV1>(),
    define_block,
    schedule_tick,
    is_powered,
};

unsafe extern "C" fn schedule_tick(dimension: Str, pos: Pos, delay: u32) -> bool {
    // SAFETY: borrowed for the call.
    let Some(world) = world(unsafe { dimension.str() }) else {
        return false;
    };
    crate::schedule_tick(&world, block_pos(&pos), delay.max(1));
    true
}

unsafe extern "C" fn is_powered(dimension: Str, pos: Pos) -> i32 {
    // SAFETY: borrowed for the call.
    let Some(world) = world(unsafe { dimension.str() }) else {
        return -1;
    };
    let pos = block_pos(&pos);
    if world.get_block_state_id_if_loaded(&pos).is_none() {
        return -1;
    }
    i32::from(pumpkin_core::block::blocks::redstone::block_receives_redstone_power(&world, &pos))
}

/// Modded block -> index of the mod that defined it, set once by [`define_blocks`].
static OWNERS: OnceLock<HashMap<u16, usize>> = OnceLock::new();

/// Definitions collected from the mod being asked, plus what earlier mods took.
struct Defining {
    taken: HashSet<String>,
    states: usize,
    defs: Vec<BlockDef>,
    biomes: Vec<crate::biome::Def>,
}

/// Most states mods can add: the client sees `u16` ids, vanilla uses the rest.
const STATE_BUDGET: usize = u16::MAX as usize - BlockStateId::COUNT as usize;

unsafe extern "C" fn define_block(
    defs: Defs,
    name: Str,
    properties: *const Property,
    property_count: usize,
    appearances: *const Str,
    appearance_count: usize,
) -> i32 {
    // SAFETY: `defs` is the `Defining` of the `define_blocks` call in progress.
    let Some(defining) = (unsafe { defs.cast::<Defining>().as_mut() }) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: the mod passes `property_count` properties and `appearance_count` names, each with
    // its values, all borrowed for the call.
    let (name, properties, looks) = unsafe {
        let properties: Vec<(String, Vec<String>)> = slice(properties, property_count)
            .iter()
            .map(|p| {
                (
                    text(p.name),
                    slice(p.values, p.count).iter().map(|v| text(*v)).collect(),
                )
            })
            .collect();
        let looks: Vec<String> = slice(appearances, appearance_count)
            .iter()
            .map(|a| text(*a))
            .collect();
        (text(name), properties, looks)
    };
    if !crate::valid_id(&name) {
        return code::INVALID_DEFINITION;
    }
    if defining.taken.contains(&name) || defining.defs.iter().any(|d| d.name == name) {
        return code::DUPLICATE_ID;
    }
    let mut keys = HashSet::new();
    let well_formed = properties.iter().all(|(key, values)| {
        let mut seen = HashSet::new();
        !key.is_empty()
            && key.chars().all(crate::ns_char)
            && keys.insert(key.as_str())
            && !values.is_empty()
            && values
                .iter()
                .all(|v| !v.is_empty() && v.chars().all(crate::ns_char) && seen.insert(v.as_str()))
    });
    let count: usize = properties.iter().map(|(_, v)| v.len()).product();
    let Some(appearance) = looks
        .iter()
        .map(|l| parse_state(l).filter(|s| !modded::is_modded(*s)))
        .collect::<Option<Vec<_>>>()
    else {
        return code::INVALID_DEFINITION;
    };
    let states: usize = defining.states
        + defining
            .defs
            .iter()
            .map(|d| d.appearance.len())
            .sum::<usize>();
    if !well_formed || appearance.len() != count || states + count > STATE_BUDGET {
        return code::INVALID_DEFINITION;
    }
    defining.defs.push(BlockDef {
        name,
        properties,
        appearance,
    });
    code::OK
}

/// # Safety
/// `ptr` points to `count` live values, or `count` is 0.
const unsafe fn slice<'a, T>(ptr: *const T, count: usize) -> &'a [T] {
    if count == 0 {
        &[]
    } else {
        // SAFETY: the caller's contract.
        unsafe { std::slice::from_raw_parts(ptr, count) }
    }
}

/// Asks every mod for its blocks and installs them. Call once, before any world loads.
pub fn define_blocks(mods: &[Native]) {
    let mut defining = Defining {
        taken: HashSet::new(),
        states: 0,
        defs: Vec::new(),
        biomes: Vec::new(),
    };
    let mut biomes = Vec::new();
    let mut all = Vec::new();
    let mut owners = Vec::new();
    for (idx, module) in mods.iter().enumerate() {
        if module.api.size < abi::MOD_API_V1_0_SIZE + 2 * size_of::<usize>() {
            continue; // built before blocks/1
        }
        let defs = std::ptr::from_mut(&mut defining).cast();
        // SAFETY: the mod's own instance; `defining` outlives the call.
        let result = unsafe { (module.api.define_blocks)(module.api.instance, defs) };
        let defined = std::mem::take(&mut defining.defs);
        let defined_biomes = std::mem::take(&mut defining.biomes);
        if result != code::OK {
            tracing::error!(
                "moddedmc: {} failed to define its blocks (code {result}); they are disabled",
                module.id
            );
            continue;
        }
        for def in defined {
            tracing::info!(
                "moddedmc: defined block {} ({} states)",
                def.name,
                def.appearance.len()
            );
            defining.taken.insert(def.name.clone());
            defining.states += def.appearance.len();
            owners.push((def.name.clone(), idx));
            all.push(def);
        }
        biomes.extend(defined_biomes);
    }
    crate::biome::install(biomes);
    // Blocks of mods that are gone stay defined, inert, so their chunks keep them.
    let known = fs::read_to_string(KNOWN_BLOCKS).unwrap_or_default();
    let mut missing = Vec::new();
    for def in known.lines().filter_map(decode) {
        let fits = defining.states + def.appearance.len() <= STATE_BUDGET;
        if fits && defining.taken.insert(def.name.clone()) {
            missing.push(def.name.clone());
            tracing::warn!(
                "moddedmc: no mod defines {}; its blocks are kept as they are until it is back",
                def.name
            );
            defining.states += def.appearance.len();
            all.push(def);
        }
    }
    let text: String = all.iter().map(|d| encode(d) + "\n").collect();
    let saved = Path::new(KNOWN_BLOCKS)
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(KNOWN_BLOCKS, text));
    if let Err(e) = saved {
        tracing::warn!("moddedmc: could not write {KNOWN_BLOCKS}: {e}");
    }
    snapshot_once(&missing);
    // Must be on disk before any chunk is saved with these ids.
    let saved: Vec<String> = fs::read_to_string(SAVED_STATES)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    let had_modded = !saved.is_empty();
    let saved = match modded::install(all, saved) {
        Ok(saved) => saved,
        Err(e) if had_modded => {
            // Without the table, saved modded blocks would load as air and be saved that way.
            tracing::error!(
                "moddedmc: could not install modded blocks ({e}); refusing to start, because \
                 the worlds contain modded blocks that would be lost"
            );
            std::process::exit(1);
        }
        Err(e) => {
            tracing::error!("moddedmc: could not install modded blocks: {e}");
            return;
        }
    };
    if let Err(e) = fs::write(SAVED_STATES, saved.join("\n")) {
        tracing::error!("moddedmc: could not write {SAVED_STATES}: {e}");
    }
    let owners = owners
        .into_iter()
        .filter_map(|(name, idx)| Some((Block::from_name(&name)?.id.as_u16(), idx)))
        .collect();
    let _ = OWNERS.set(owners);
}

/// Snapshot of the worlds, taken before the first start with a new set of missing blocks.
const SNAPSHOT_KEY: &str = "config/moddedmc/snapshot.txt";

/// Copies `world/` to `backups/moddedmc-<unix time>/` the first time blocks in `missing` have no
/// mod, so a removal can be undone even if something goes wrong while the mod is away.
// shortcut: assumes the default world folder `world`, until the host reads the level name.
fn snapshot_once(missing: &[String]) {
    if missing.is_empty() {
        return;
    }
    let mut key = missing.to_vec();
    key.sort();
    let key = key.join("\n");
    if fs::read_to_string(SNAPSHOT_KEY).is_ok_and(|k| k == key) || !Path::new("world").is_dir() {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let to = PathBuf::from(format!("backups/moddedmc-{secs}"));
    match copy_dir(Path::new("world"), &to) {
        Ok(()) => {
            tracing::warn!(
                "moddedmc: blocks without a mod; saved a copy of the world to {}",
                to.display()
            );
            if let Err(e) = fs::write(SNAPSHOT_KEY, key) {
                tracing::warn!("moddedmc: could not write {SNAPSHOT_KEY}: {e}");
            }
        }
        Err(e) => tracing::error!(
            "moddedmc: could not copy the world to {}: {e}",
            to.display()
        ),
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Every modded block this server has had: name, properties and look-alikes, one per line.
const KNOWN_BLOCKS: &str = "config/moddedmc/blocks.txt";
/// Modded state names behind the ids `.pump` worlds store; append-only (see `modded::install`).
const SAVED_STATES: &str = "config/moddedmc/states.txt";

/// `name key=v1,v2;key2=a,b look1;look2`
fn encode(def: &BlockDef) -> String {
    let props: Vec<String> = def
        .properties
        .iter()
        .map(|(k, v)| format!("{k}={}", v.join(",")))
        .collect();
    let looks: Vec<String> = def.appearance.iter().map(|&s| full_name(s)).collect();
    format!("{} {} {}", def.name, props.join(";"), looks.join(";"))
}

fn decode(line: &str) -> Option<BlockDef> {
    let mut parts = line.split(' ');
    let (name, props, looks) = (parts.next()?, parts.next()?, parts.next()?);
    let properties = props
        .split(';')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.to_owned(), v.split(',').map(str::to_owned).collect()))
        })
        .collect::<Option<Vec<(String, Vec<String>)>>>()?;
    let appearance = looks
        .split(';')
        .map(parse_state)
        .collect::<Option<Vec<_>>>()?;
    let count: usize = properties.iter().map(|(_, v)| v.len()).product();
    (crate::valid_id(name) && appearance.len() == count).then(|| BlockDef {
        name: name.to_owned(),
        properties,
        appearance,
    })
}

/// The index of the mod that owns modded `state`'s block.
pub fn owner(state: BlockStateId) -> Option<usize> {
    if !modded::is_modded(state) {
        return None;
    }
    OWNERS
        .get()?
        .get(&Block::from_state_id(state).id.as_u16())
        .copied()
}

// ---------------------------------------------------------------- entities/1

// ---------------------------------------------------------------- biomes/1

static BIOMES: BiomesV1 = BiomesV1 {
    size: size_of::<BiomesV1>(),
    define_biome,
    set_biome,
    biome_at,
};

unsafe extern "C" fn define_biome(defs: Defs, name: Str, look_alike: Str, json: Str) -> i32 {
    // SAFETY: `defs` is the `Defining` of the `define_blocks` call in progress; the strings are
    // borrowed for the call.
    let (Some(defining), name, look_alike, json) = (
        unsafe { defs.cast::<Defining>().as_mut() },
        unsafe { text(name) },
        unsafe { text(look_alike) },
        unsafe { text(json) },
    ) else {
        return code::INVALID_DEFINITION;
    };
    if !crate::biome::valid(&name, &look_alike, &json) {
        return code::INVALID_DEFINITION;
    }
    if !defining.taken.insert(name.clone()) {
        return code::DUPLICATE_ID;
    }
    defining.biomes.push(crate::biome::Def {
        name,
        look_alike,
        json,
    });
    code::OK
}

unsafe extern "C" fn set_biome(dimension: Str, pos: Pos, name: Str) -> bool {
    // SAFETY: borrowed for the call.
    let (dimension, name) = unsafe { (dimension.str(), name.str()) };
    crate::biome::set(dimension, pos.x, pos.y, pos.z, name)
}

unsafe extern "C" fn biome_at(dimension: Str, pos: Pos, out: Sink) -> bool {
    // SAFETY: borrowed for the call.
    let Some(name) = crate::biome::at(unsafe { dimension.str() }, pos.x, pos.y, pos.z) else {
        return false;
    };
    // SAFETY: the mod's sink copies before returning.
    unsafe { (out.put)(out.ctx, 0, Str::from(name.as_str())) };
    true
}

// ---------------------------------------------------------------- menus/1

static MENUS: MenusV1 = MenusV1 {
    size: size_of::<MenusV1>(),
    open: open_menu,
    item: menu_item,
    describe: describe_item,
    open_machine,
    set_property,
    restrict_slot,
};

unsafe extern "C" fn restrict_slot(
    p: PlayerRef,
    key: Str,
    slot: u32,
    items: *const Str,
    count: usize,
) -> bool {
    // SAFETY: a live player and `count` item ids, all borrowed for the call.
    unsafe {
        let Some(p) = player(p) else { return false };
        let allowed = slice(items, count).iter().map(|i| text(*i)).collect();
        crate::menu::restrict_slot(p, &text(key), slot, allowed)
    }
}

unsafe extern "C" fn open_machine(
    p: PlayerRef,
    kind: Str,
    title: Str,
    key: Str,
    items: *const Str,
    count: usize,
) -> bool {
    // SAFETY: a live player and `count` items, all borrowed for the call.
    unsafe {
        let Some(p) = player(p) else { return false };
        let items: Vec<&[u8]> = slice(items, count).iter().map(|i| i.bytes()).collect();
        crate::menu::open_machine(p, &text(kind), &text(title), &text(key), &items)
    }
}

unsafe extern "C" fn set_property(p: PlayerRef, key: Str, property: u32, value: i32) -> bool {
    // SAFETY: a live player and a borrowed key.
    unsafe { player(p) }
        .is_some_and(|p| crate::menu::set_property(p, &unsafe { text(key) }, property, value))
}

unsafe extern "C" fn open_menu(
    p: PlayerRef,
    title: Str,
    rows: u32,
    key: Str,
    items: *const Str,
    count: usize,
) -> bool {
    // SAFETY: a live player and `count` items, all borrowed for the call.
    unsafe {
        let Some(p) = player(p) else { return false };
        let items = if count == 0 || items.is_null() {
            &[]
        } else {
            std::slice::from_raw_parts(items, count)
        };
        let items: Vec<&[u8]> = items.iter().map(|i| i.bytes()).collect();
        crate::menu::open(p, &text(title), rows, &text(key), &items)
    }
}

unsafe extern "C" fn menu_item(id: Str, count: u32, out: Sink) -> bool {
    // SAFETY: borrowed for the call.
    let Some(bytes) = crate::menu::item(&unsafe { text(id) }, count) else {
        return false;
    };
    // SAFETY: the mod's sink copies before returning.
    unsafe { (out.put)(out.ctx, 0, Str::new(&bytes)) };
    true
}

unsafe extern "C" fn describe_item(item: Str, out: Sink) -> u32 {
    // SAFETY: borrowed for the call.
    let (id, count) = crate::menu::describe(unsafe { item.bytes() });
    if count > 0 {
        // SAFETY: the mod's sink copies before returning.
        unsafe { (out.put)(out.ctx, 0, Str::from(id.as_str())) };
    }
    count
}

static ENTITIES: EntitiesV1 = EntitiesV1 {
    size: size_of::<EntitiesV1>(),
    spawn,
    position,
    move_to,
    set_name,
    remove,
    set_ai,
    walk_to,
    set_target,
    add_goal,
};

/// A live entity of a dimension by id.
unsafe fn entity(dimension: Str, id: EntityId) -> Option<(Arc<World>, Arc<dyn EntityBase>)> {
    // SAFETY: the caller's contract: borrowed for the call.
    let world = world(unsafe { dimension.str() })?;
    let entity = world.get_entity_by_uuid(uuid::Uuid::from_bytes(id.0))?;
    Some((world, entity))
}

unsafe extern "C" fn spawn(dimension: Str, kind: Str, at: Vec3, out: *mut EntityId) -> bool {
    // SAFETY: borrowed for the call.
    let (dimension, kind) = unsafe { (dimension.str(), kind.str()) };
    let (Some(world), Some(kind)) = (
        world(dimension),
        EntityType::from_name(kind.strip_prefix("minecraft:").unwrap_or(kind)),
    ) else {
        return false;
    };
    let id = uuid::Uuid::new_v4();
    let entity = from_type(kind, Vector3::new(at.x, at.y, at.z), &world, id);
    if !world.spawn_entity(entity) {
        return false;
    }
    // SAFETY: the mod passes a writable id.
    unsafe { out.write(EntityId(id.into_bytes())) };
    true
}

unsafe extern "C" fn position(dimension: Str, id: EntityId, out: *mut Vec3) -> bool {
    // SAFETY: borrowed for the call.
    let Some((_, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    let p = entity.get_entity().pos.load();
    let at = Vec3 {
        x: p.x,
        y: p.y,
        z: p.z,
    };
    // SAFETY: the mod passes a writable position.
    unsafe { out.write(at) };
    true
}

unsafe extern "C" fn move_to(dimension: Str, id: EntityId, at: Vec3, yaw: f32, pitch: f32) -> bool {
    // SAFETY: borrowed for the call.
    let Some((world, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    let keep = |a: f32| (!a.is_nan()).then_some(a);
    entity.teleport(
        Vector3::new(at.x, at.y, at.z),
        keep(yaw),
        keep(pitch),
        world,
    );
    true
}

unsafe extern "C" fn set_name(dimension: Str, id: EntityId, name: Str) -> bool {
    // SAFETY: borrowed for the call.
    let (Some((_, entity)), name) = (unsafe { entity(dimension, id) }, unsafe { text(name) })
    else {
        return false;
    };
    let e = entity.get_entity();
    e.set_custom_name_visible(!name.is_empty());
    e.set_custom_name(TextComponent::text(name));
    true
}

unsafe extern "C" fn set_ai(dimension: Str, id: EntityId, enabled: bool) -> bool {
    // SAFETY: borrowed for the call.
    let Some((_, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    let Some(mob) = entity.get_mob() else {
        return false;
    };
    mob.get_mob_entity().set_no_ai(!enabled);
    true
}

unsafe extern "C" fn set_target(dimension: Str, id: EntityId, target: EntityId) -> bool {
    // SAFETY: borrowed for the call.
    let Some((world, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    let Some(mob) = entity.get_mob() else {
        return false;
    };
    if target == EntityId::default() {
        mob.apply_mob_target(None);
        return true;
    }
    let Some(target) = world.get_entity_by_uuid(uuid::Uuid::from_bytes(target.0)) else {
        return false;
    };
    mob.apply_mob_target(Some(target));
    true
}

unsafe extern "C" fn add_goal(
    dimension: Str,
    id: EntityId,
    priority: u32,
    controls: u32,
    key: Str,
) -> bool {
    // SAFETY: borrowed for the call.
    let Some((_, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    let Some(mob) = entity.get_mob() else {
        return false;
    };
    // SAFETY: borrowed for the call.
    let key = unsafe { text(key) };
    crate::goal::add(mob.get_mob_entity(), priority, controls, key)
}

/// A walk in progress: dimension, entity, target, speed.
type Walk = (String, uuid::Uuid, Vector3<f64>, f64);

static WALKS: std::sync::Mutex<Vec<Walk>> = std::sync::Mutex::new(Vec::new());

unsafe extern "C" fn walk_to(dimension: Str, id: EntityId, at: Vec3, speed: f64) -> bool {
    use pumpkin_core::entity::ai::goal::Controls;
    // SAFETY: borrowed for the call.
    let Some((_, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    let Some(mob) = entity.get_mob() else {
        return false;
    };
    let mob = mob.get_mob_entity();
    let uuid = uuid::Uuid::from_bytes(id.0);
    let mut walks = crate::lock(&WALKS);
    walks.retain(|(_, w, _, _)| *w != uuid);
    if at.x.is_nan() {
        mob.steered().navigator().stop();
        mob.goals_selector.enable_control(Controls::MOVE);
        return true;
    }
    // As a leash does: goals that move the mob pause, its navigator still runs.
    mob.goals_selector.disable_control(Controls::MOVE);
    let to = Vector3::new(at.x, at.y, at.z);
    walk(mob, to, speed);
    // SAFETY: borrowed for the call.
    walks.push((unsafe { dimension.str() }.to_owned(), uuid, to, speed));
    true
}

fn walk(mob: &pumpkin_core::entity::mob::MobEntity, to: Vector3<f64>, speed: f64) {
    use pumpkin_core::entity::ai::pathfinder::NavigatorGoal;
    let from = mob.living_entity.entity.pos.load();
    mob.steered()
        .navigator()
        .set_progress(NavigatorGoal::new(from, to, speed));
}

/// Keeps walks going until released: a goal stopping as it loses control, or anything else,
/// clears the navigator, so an idle walker that has not arrived is sent again; one that has
/// arrived is held there. Gone walkers drop.
pub fn tick_walks() {
    crate::lock(&WALKS).retain(|(dimension, id, to, speed)| {
        let Some(entity) = world(dimension).and_then(|w| w.get_entity_by_uuid(*id)) else {
            return false;
        };
        let Some(mob) = entity.get_mob() else {
            return false;
        };
        let mob = mob.get_mob_entity();
        let at = mob.living_entity.entity.pos.load();
        let steered = mob.steered();
        let mut navigator = steered.navigator();
        // The navigator counts a path done a little over a block short of its end.
        if (at.x - to.x).hypot(at.z - to.z) < 1.5 {
            if !navigator.is_idle() {
                navigator.stop();
            }
        } else if navigator.is_idle() {
            drop(navigator);
            walk(mob, *to, *speed);
        }
        true
    });
}

unsafe extern "C" fn remove(dimension: Str, id: EntityId) -> bool {
    // SAFETY: borrowed for the call.
    let Some((_, entity)) = (unsafe { entity(dimension, id) }) else {
        return false;
    };
    entity.get_entity().remove();
    true
}

// ---------------------------------------------------------------- commands

/// Registers a mod's root command; running it calls the mod's `command`.
pub fn register_command(
    server: &Server,
    module: Native,
    name: String,
    description: String,
    tag: u64,
) {
    let node: CommandDetachedNode = command(name, description)
        .executes(Forward { module, tag })
        .into();
    server.command_dispatcher.rcu(|dispatcher| {
        let mut new = (**dispatcher).clone();
        new.register(node.clone());
        Arc::new(new)
    });
}

struct Forward {
    module: Native,
    tag: u64,
}

impl CommandExecutor for Forward {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let player = context.source.as_player();
        let call = moddedmc_api::abi::CommandCall {
            source: Arc::as_ptr(&context.source).cast(),
            player: player
                .as_ref()
                .map_or(PlayerRef(std::ptr::null()), player_ref),
            input: Str::from(context.input.as_str()),
        };
        // SAFETY: the call borrows `context` and `player`, both alive until it returns.
        Ok(unsafe {
            (self.module.api.command)(self.module.api.instance, self.tag, &raw const call)
        })
    }
}

// ---------------------------------------------------------------- calls into a mod

/// Bytes a mod handed back through a [`Sink`], by key.
pub type Returned = Vec<(u64, Vec<u8>)>;

unsafe extern "C" fn collect(ctx: *mut c_void, key: u64, value: Str) {
    // SAFETY: `ctx` is the `Returned` of the call in progress; `value` is borrowed for this call.
    unsafe { (*ctx.cast::<Returned>()).push((key, value.bytes().to_vec())) };
}

fn sink(out: &mut Returned) -> Sink {
    Sink {
        ctx: std::ptr::from_mut(out).cast(),
        put: collect,
    }
}

/// One of a mod's blocks, for use, place and break calls.
pub struct BlockArgs<'a> {
    pub entity_type: usize,
    pub pos: BlockPos,
    pub world: &'a World,
    pub state: &'a [u8],
    pub player: Option<&'a Arc<Player>>,
}

impl BlockArgs<'_> {
    fn call(&self) -> BlockCall {
        BlockCall {
            entity_type: u32::try_from(self.entity_type).unwrap_or(u32::MAX),
            pos: pos(self.pos),
            dimension: Str::from(self.world.dimension.minecraft_name),
            state: Str::new(self.state),
            player: self.player.map_or(PlayerRef(std::ptr::null()), player_ref),
        }
    }
}

pub const fn pos(p: BlockPos) -> Pos {
    Pos {
        x: p.0.x,
        y: p.0.y,
        z: p.0.z,
    }
}

// The mod's functions get its own instance and arguments borrowed for the call; the SDK catches
// panics on the mod's side.
impl Native {
    pub fn register(self, staging: &mut Staging<'_>) -> i32 {
        let reg = std::ptr::from_mut(staging).cast();
        // SAFETY: see above; `reg` is live for the call.
        unsafe { (self.api.register)(self.api.instance, reg) }
    }

    pub fn tick(self, tick: u64, world: &World, entries: &[TickEntry]) -> Returned {
        let mut out = Returned::new();
        let dimension = Str::from(world.dimension.minecraft_name);
        let (ptr, len) = (entries.as_ptr(), entries.len());
        // SAFETY: see above; `entries` and `out` outlive the call.
        unsafe { (self.api.tick)(self.api.instance, tick, dimension, ptr, len, sink(&mut out)) };
        out
    }

    pub fn use_block(self, args: &BlockArgs<'_>) -> Returned {
        let mut out = Returned::new();
        let call = args.call();
        // SAFETY: see above; `call` and `out` outlive the call.
        unsafe { (self.api.use_block)(self.api.instance, &raw const call, sink(&mut out)) };
        out
    }

    pub fn place_block(self, args: &BlockArgs<'_>) -> Returned {
        let mut out = Returned::new();
        let call = args.call();
        // SAFETY: see above.
        unsafe { (self.api.place_block)(self.api.instance, &raw const call, sink(&mut out)) };
        out
    }

    pub fn break_block(self, args: &BlockArgs<'_>) {
        let call = args.call();
        // SAFETY: see above.
        unsafe { (self.api.break_block)(self.api.instance, &raw const call) };
    }

    /// Sends a `blocks/1` event. Mods define blocks only if they have `block_event`.
    pub fn block_event(
        self,
        kind: u32,
        world: &World,
        pos: BlockPos,
        state: BlockStateId,
        player: Option<&Arc<Player>>,
    ) -> bool {
        let event = BlockEvent {
            kind,
            dimension: Str::from(world.dimension.minecraft_name),
            pos: self::pos(pos),
            state: u32::from(state.as_u16()),
            player: player.map_or(PlayerRef(std::ptr::null()), player_ref),
        };
        // SAFETY: see above; `event` outlives the call.
        unsafe { (self.api.block_event)(self.api.instance, &raw const event) }
    }

    /// Sends an entity event (`player` for uses and attacks, `damage` for damage); true if the mod
    /// handled it.
    pub fn entity_event(
        self,
        kind: u32,
        world: &World,
        id: uuid::Uuid,
        player: Option<&Arc<Player>>,
        (damage, cause): (f32, &str),
    ) -> bool {
        if self.api.size < abi::MOD_API_V1_0_SIZE + 4 * size_of::<usize>() {
            return false;
        }
        let event = abi::EntityEvent {
            kind,
            dimension: Str::from(world.dimension.minecraft_name),
            id: EntityId(id.into_bytes()),
            player: player.map_or(PlayerRef(std::ptr::null()), player_ref),
            damage,
            cause: Str::from(cause),
        };
        // SAFETY: see above; `event` outlives the call.
        unsafe { (self.api.entity_event)(self.api.instance, &raw const event) }
    }

    /// Asks or tells the mod about a goal; its answer for `CAN_START` and `CONTINUE`.
    pub fn goal_event(self, phase: u32, key: &str, dimension: &str, id: uuid::Uuid) -> bool {
        if self.api.size < abi::MOD_API_V1_0_SIZE + 6 * size_of::<usize>() {
            return false;
        }
        let event = abi::GoalEvent {
            phase,
            key: Str::from(key),
            dimension: Str::from(dimension),
            id: EntityId(id.into_bytes()),
        };
        // SAFETY: see above; `event` outlives the call.
        unsafe { (self.api.goal_event)(self.api.instance, &raw const event) }
    }

    /// Hands a closed menu's contents to the mod.
    pub fn menu_closed(self, player: &Arc<Player>, key: &str, items: &[Vec<u8>]) {
        if self.api.size < abi::MOD_API_V1_0_SIZE + 5 * size_of::<usize>() {
            return;
        }
        let items: Vec<Str> = items.iter().map(|i| Str::new(i)).collect();
        // SAFETY: see above; everything outlives the call.
        unsafe {
            (self.api.menu_closed)(
                self.api.instance,
                player_ref(player),
                Str::from(key),
                items.as_ptr(),
                items.len(),
            );
        }
    }

    /// Whether this mod was built with `chunk_generated`.
    pub const fn has_worldgen(self) -> bool {
        self.api.size >= abi::MOD_API_V1_0_SIZE + 3 * size_of::<usize>()
    }

    pub fn chunk_generated(self, world: &World, chunk_x: i32, chunk_z: i32) {
        let dimension = Str::from(world.dimension.minecraft_name);
        // SAFETY: see above.
        unsafe { (self.api.chunk_generated)(self.api.instance, dimension, chunk_x, chunk_z) };
    }

    pub fn menu_click(self, player: &Arc<Player>, action: &str) {
        // SAFETY: see above; the player is kept alive by the caller.
        unsafe { (self.api.menu_click)(self.api.instance, player_ref(player), Str::from(action)) };
    }
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, full_name, parse_state};

    #[test]
    fn known_blocks_round_trip() {
        let line = "demo:shaft axis=x,y,z;lit=false,true \
            minecraft:end_rod[facing=east];minecraft:end_rod[facing=up];minecraft:end_rod[facing=south];\
            minecraft:stone;minecraft:stone;minecraft:oak_log[axis=x]";
        let def = decode(line);
        assert!(def.is_some());
        assert_eq!(def.as_ref().map(encode).as_deref(), Some(line));
        // Wrong state count, bad id, unknown look-alike.
        assert!(decode("demo:shaft axis=x,y minecraft:stone").is_none());
        assert!(decode("shaft  minecraft:stone").is_none());
        assert!(decode("demo:shaft  minecraft:nope").is_none());
        assert!(decode("demo:plain  minecraft:stone").is_some());
    }

    #[test]
    fn state_names_round_trip() {
        let log = parse_state("minecraft:oak_log[axis=x]");
        assert!(log.is_some());
        assert_eq!(
            log.map(full_name).as_deref(),
            Some("minecraft:oak_log[axis=x]")
        );
        // Left-out properties take the default; the prefix is optional.
        assert_eq!(
            parse_state("oak_log"),
            parse_state("minecraft:oak_log[axis=y]")
        );
        assert_eq!(
            parse_state("minecraft:stone").map(full_name).as_deref(),
            Some("minecraft:stone")
        );
        assert_eq!(parse_state("minecraft:oak_log[axis=q]"), None);
        assert_eq!(parse_state("minecraft:no_such_block"), None);
    }
}
