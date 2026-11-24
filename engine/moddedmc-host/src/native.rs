//! Loads mods from the server's `mods/` folder and implements the host side of the stable mod
//! ABI (`api/src/abi.rs`, docs/MOD-ABI.md).
//!
//! - `<name>.mmc`: a zip with `mod.json` and one library per platform (`windows-x86_64.dll`, ...).
//!   The library for this platform is extracted to `mods/.cache/` and loaded. A `data/` folder
//!   in the zip (recipes, loot tables, tags, ...) is extracted beside it and loads as a built-in
//!   datapack named `mod/<id>`. An `assets/` folder (models, textures) goes into the resource
//!   pack the server sends to players.
//! - `<name>.jar`: a Fabric mod. It is converted once by `recompiler/build-mod.ps1` into
//!   `mods/.cache/<name>-<hash>.<ext>` and loaded from there.
//! - A bare `.dll`/`.so`/`.dylib`: loaded as is (handy while developing a mod).

use std::{
    ffi::c_void,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, OnceLock},
};

use moddedmc_api::abi::{
    ABI_SYMBOL, ABI_VERSION, BlockCall, ENTRY_SYMBOL, HostApi, ID_SYMBOL, MenuButton, ModApi,
    PlayerRef, Pos, Reg, Sink, Str, TickEntry, code, level,
};
use pumpkin_core::{
    command::{
        argument_builder::{ArgumentBuilder, command},
        context::CommandContext,
        node::{CommandExecutor, CommandExecutorResult, detached::CommandDetachedNode},
    },
    entity::{EntityBase, player::Player},
    server::Server,
    world::World,
};
use pumpkin_protocol::java::client::dialog::{ActionButton, Dialog, DialogAction, DialogBody};
use pumpkin_util::{
    math::{position::BlockPos, vector3::Vector3},
    text::TextComponent,
};
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
    pub pre_world: bool,
}

thread_local! {
    static PRE_WORLD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn request_pre_world() {
    PRE_WORLD.set(true);
}

/// Loads mods in dependency order. Providers register before their dependents
/// run `Mod::start`, while standalone mods retain their old startup timing.
pub fn load_all(dir: &Path, mut on_loaded: impl FnMut(Native, bool) -> bool) -> usize {
    use std::collections::HashSet;

    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut files: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    files.sort();
    let mut pending = Vec::new();
    for path in files.into_iter().filter(|p| p.is_file()) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "mmc" | "jar") && ext != EXT {
            continue;
        }
        match crate::load_order::inspect(path.clone()) {
            Ok(candidate) => pending.push(candidate),
            Err(error) => tracing::error!("moddedmc: {}: {error}", path.display()),
        }
    }
    let providers: HashSet<String> = pending
        .iter()
        .flat_map(|candidate| candidate.dependencies.iter().cloned())
        .collect();
    let mut completed = HashSet::new();
    let mut seen = HashSet::new();
    let cache = dir.join(".cache");
    let mut loaded = 0usize;
    while !pending.is_empty() {
        let Some(index) = crate::load_order::next_ready(&pending, &completed) else {
            for candidate in pending {
                tracing::error!(
                    "moddedmc: skipped {}: dependencies missing, failed or cyclic: {}",
                    candidate.path.display(),
                    candidate.dependencies.join(", ")
                );
            }
            break;
        };
        let candidate = pending.remove(index);
        let path = &candidate.path;
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let extracted = match ext {
            "mmc" => extract(path, &cache),
            "jar" => convert(path, &cache).map(|lib| (lib, None)),
            e if e == EXT => Ok((path.clone(), None)),
            _ => continue,
        };
        match extracted.and_then(|(lib, resources)| Ok((load(&lib)?, resources))) {
            Ok((module, _)) if seen.contains(module.id) => {
                tracing::error!(
                    "moddedmc: duplicate mod id {} from {}",
                    module.id,
                    path.display()
                );
            }
            Ok((module, resources)) => {
                if candidate.id.as_deref().is_some_and(|id| id != module.id) {
                    tracing::error!(
                        "moddedmc: {}: manifest id disagrees with exported mod id {}",
                        path.display(),
                        module.id
                    );
                    continue;
                }
                seen.insert(module.id.to_owned());
                let eager = providers.contains(module.id);
                if !on_loaded(module, eager) {
                    tracing::error!(
                        "moddedmc: {}: registration failed, dependents skipped",
                        module.id
                    );
                    continue;
                }
                // SAFETY: the version string is static in the process-lifetime library.
                let version = unsafe { module.api.version.str() };
                tracing::info!(
                    "moddedmc: loaded {} {version} from {}",
                    module.id,
                    path.display()
                );
                if let Some(resources) = resources {
                    if resources.join("data").is_dir() {
                        pumpkin_core::data::datapack::register_builtin_datapack(
                            format!("mod/{}", module.id),
                            resources.clone(),
                        );
                    }
                    if resources.join("assets").is_dir() {
                        crate::content_pack::add_asset_root(resources);
                    }
                }
                completed.insert(module.id.to_owned());
                loaded += 1;
            }
            Err(error) => tracing::error!("moddedmc: {}: {error}", path.display()),
        }
    }
    loaded
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

/// Extracts this platform's library and the `data/` and `assets/` folders from a `.mmc` file,
/// once per file content. Returns the library and the folder holding the other two.
fn extract(path: &Path, cache: &Path) -> Result<(PathBuf, Option<PathBuf>), String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let key = format!("{}-{}", stem(path), hash(&bytes));
    let out = cache.join(format!("{key}.{EXT}"));
    let pack = cache.join(&key);
    if out.exists() && pack.join(EXTRACTED).exists() {
        return Ok((out, Some(pack)));
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
    extract_resources(&mut zip, &pack)?;
    Ok((out, Some(pack)))
}

/// Marks fully extracted resources, so an interrupted extraction is redone.
const EXTRACTED: &str = ".extracted";

fn extract_resources<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    pack: &Path,
) -> Result<(), String> {
    if pack.exists() {
        fs::remove_dir_all(pack).map_err(|e| e.to_string())?;
    }
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let Some(relative) = entry.enclosed_name() else {
            let name = entry.name().unwrap_or_default();
            return Err(format!("unsafe path in mod package: {name}"));
        };
        if !(relative.starts_with("data") || relative.starts_with("assets")) || entry.is_dir() {
            continue;
        }
        let target = pack.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = fs::File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(pack).map_err(|e| e.to_string())?;
    fs::write(pack.join(EXTRACTED), "").map_err(|e| e.to_string())
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
            .get::<unsafe extern "C" fn(*const HostApi) -> *const ModApi>(ENTRY_SYMBOL)
            .map_err(|_| "not a moddedMC mod (no moddedmc_mod)".to_owned())?;
        PRE_WORLD.set(false);
        let api = entry(Box::leak(Box::new(host_api(id))));
        let pre_world = PRE_WORLD.replace(false);
        if api.is_null() {
            return Err("the mod refused to start".into());
        }
        Native {
            api: &*api,
            id,
            pre_world,
        }
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
        get_interface: crate::interfaces::get_interface,
    }
}

/// # Safety
/// `s` is a live borrowed string.
pub unsafe fn text(s: Str) -> String {
    // SAFETY: the caller's contract.
    String::from_utf8_lossy(unsafe { s.bytes() }).into_owned()
}

/// # Safety
/// `p` is null or a player reference the host handed out for the current call.
pub const unsafe fn player<'a>(p: PlayerRef) -> Option<&'a Player> {
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
pub const unsafe fn staging<'a>(reg: Reg) -> &'a mut Staging<'a> {
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
    // SAFETY: `source` is the live context of the command being run.
    unsafe {
        if let Some(context) = source.cast::<CommandContext<'_>>().as_ref() {
            context
                .source
                .send_message(TextComponent::text(text(message)));
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

pub unsafe extern "C" fn dimension(index: usize) -> Str {
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
    install_command(server, &node);
}

pub fn install_command(server: &Server, node: &CommandDetachedNode) {
    server.command_dispatcher.rcu(|dispatcher| {
        let mut new = (**dispatcher).clone();
        new.register(node.clone());
        Arc::new(new)
    });
}

pub struct Forward {
    pub module: Native,
    pub tag: u64,
}

impl CommandExecutor for Forward {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let player = context.source.as_player();
        let call = moddedmc_api::abi::CommandCall {
            source: std::ptr::from_ref(context).cast(),
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

    pub fn menu_click(self, player: &Arc<Player>, action: &str) {
        // SAFETY: see above; the player is kept alive by the caller.
        unsafe { (self.api.menu_click)(self.api.instance, player_ref(player), Str::from(action)) };
    }
}
