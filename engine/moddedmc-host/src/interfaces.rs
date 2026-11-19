//! Named versioned tables exposed to drop-in mods.

use std::{
    ffi::c_void,
    ptr,
    sync::{Arc, Mutex},
};

use moddedmc_api::abi::{self, ChunkCompletion, Reg, StopCallback, Str, code};
use pumpkin_core::{
    command::{
        argument_builder::{ArgumentBuilder, argument, command, literal},
        argument_types::core::integer::IntegerArgumentType,
        context::CommandContext,
        node::detached::DetachedNode,
    },
    server::Server,
};
use pumpkin_util::{
    PermissionLvl,
    math::vector2::Vector2,
    permission::{Permission, PermissionDefault},
    text::TextComponent,
};

use crate::{
    lock,
    native::{self, Forward, Native, SERVER, staging, text},
};

static WORLD: abi::WorldV1 = abi::WorldV1 {
    size: size_of::<abi::WorldV1>(),
    dimension: native::dimension,
    command_dimension,
};
static CHUNKS: abi::ChunksV1 = abi::ChunksV1 {
    size: size_of::<abi::ChunksV1>(),
    is_generated,
    ensure_generated,
};
static COMMANDS: abi::CommandsV1 = abi::CommandsV1 {
    size: size_of::<abi::CommandsV1>(),
    add: add_command,
    integer,
    feedback,
};
static LIFECYCLE: abi::LifecycleV1 = abi::LifecycleV1 {
    size: size_of::<abi::LifecycleV1>(),
    on_stopping,
};

pub unsafe extern "C" fn get_interface(name: Str, version: u32) -> *const c_void {
    // SAFETY: the name is borrowed for this call.
    match (unsafe { name.str() }, version) {
        (abi::CONTENT, 1) => {
            native::request_pre_world();
            ptr::from_ref(&crate::content::CONTENT).cast()
        }
        (abi::ITEMS, 1) => {
            native::request_pre_world();
            ptr::from_ref(&crate::content::ITEMS).cast()
        }
        (abi::BLOCK_WORLD, 1) => ptr::from_ref(&crate::content::WORLD).cast(),
        (abi::ENTITIES, 1) => ptr::from_ref(&crate::content::ENTITIES).cast(),
        (abi::ITEM_STACKS, 1) => ptr::from_ref(&crate::stacks::ITEM_STACKS).cast(),
        (abi::DISPLAYS, 1) => ptr::from_ref(&crate::stacks::DISPLAYS_TABLE).cast(),
        (abi::EFFECTS, 1) => ptr::from_ref(&crate::effects::EFFECTS).cast(),
        (abi::RESOURCES, 1) => ptr::from_ref(&crate::resources::RESOURCES).cast(),
        (abi::CONTAINERS, 1) => ptr::from_ref(&crate::containers::CONTAINERS).cast(),
        (abi::TAGS, 1) => ptr::from_ref(&crate::tags::TAGS).cast(),
        (abi::INTERACTION, 1) => ptr::from_ref(&crate::interaction::INTERACTION).cast(),
        (abi::WORLD, 1) => ptr::from_ref(&WORLD).cast(),
        (abi::CHUNKS, 1) => ptr::from_ref(&CHUNKS).cast(),
        (abi::COMMANDS, 1) => ptr::from_ref(&COMMANDS).cast(),
        (abi::LIFECYCLE, 1) => ptr::from_ref(&LIFECYCLE).cast(),
        _ => ptr::null(),
    }
}

unsafe extern "C" fn command_dimension(source: *const c_void) -> Str {
    // SAFETY: source is the live context passed by Forward.
    unsafe { source.cast::<CommandContext<'_>>().as_ref() }
        .and_then(|context| context.source.world.as_ref())
        .map_or(Str::EMPTY, |world| {
            Str::from(world.dimension.minecraft_name)
        })
}

unsafe extern "C" fn is_generated(
    dimension: Str,
    x: i32,
    z: i32,
    ctx: *mut c_void,
    completed: ChunkCompletion,
) -> bool {
    // SAFETY: the world name is borrowed for the call.
    submit(unsafe { dimension.str() }, x, z, ctx, completed, false)
}

unsafe extern "C" fn ensure_generated(
    dimension: Str,
    x: i32,
    z: i32,
    ctx: *mut c_void,
    completed: ChunkCompletion,
) -> bool {
    // SAFETY: the world name is borrowed for the call.
    submit(unsafe { dimension.str() }, x, z, ctx, completed, true)
}

// Closing the admission gate and submitting a tracked task must be ordered at shutdown.
static STOP: Mutex<Option<Vec<StopCallback>>> = Mutex::new(Some(Vec::new()));

fn submit(
    dimension: &str,
    x: i32,
    z: i32,
    ctx: *mut c_void,
    completed: ChunkCompletion,
    generate: bool,
) -> bool {
    // Leave room for neighboring chunk coordinates to be converted to blocks.
    const LIMIT: u32 = (i32::MAX as u32 >> 4) - 32;
    let Some(server) = SERVER.get() else {
        return false;
    };
    if x.unsigned_abs() > LIMIT || z.unsigned_abs() > LIMIT {
        return false;
    }
    let worlds = server.worlds.load();
    let Some(world) = worlds
        .iter()
        .find(|world| world.dimension.minecraft_name == dimension)
    else {
        return false;
    };
    let level = world.level.clone();
    let gate = lock(&STOP);
    if gate.is_none() || server.stop_token.is_cancelled() {
        return false;
    }
    let mut completion = PendingCompletion {
        ctx,
        completed,
        generated: false,
    };
    let pos = Vector2::new(x, z);
    server.spawn_task(async move {
        let exists = level.is_chunk_generated(pos).await;
        if generate && !exists {
            level.get_or_fetch_chunk(pos, |_| ()).await;
        }
        completion.generated = exists || generate;
        drop(completion);
    });
    drop(gate);
    true
}

struct PendingCompletion {
    ctx: *mut c_void,
    completed: ChunkCompletion,
    generated: bool,
}

// SAFETY: accepting the request transfers its callback context to a server task.
unsafe impl Send for PendingCompletion {}

impl Drop for PendingCompletion {
    fn drop(&mut self) {
        // SAFETY: the mod keeps the context alive until this one completion.
        unsafe { (self.completed)(self.ctx, self.generated) };
    }
}

unsafe extern "C" fn on_stopping(reg: Reg, callback: StopCallback) -> i32 {
    // SAFETY: live staging for this registration.
    let slot = &mut unsafe { staging(reg) }.on_stopping;
    if slot.is_some() {
        return code::DUPLICATE_ID;
    }
    *slot = Some(callback);
    code::OK
}

pub fn register_stopping(callback: StopCallback) {
    if let Some(callbacks) = lock(&STOP).as_mut() {
        callbacks.push(callback);
    }
}

pub fn watch_shutdown(server: &Arc<Server>) {
    let stop = server.stop_token.clone();
    server.spawn_task(async move {
        stop.cancelled().await;
        let callbacks = lock(&STOP).take().unwrap_or_default();
        if !callbacks.is_empty()
            && let Err(error) = tokio::task::spawn_blocking(move || {
                for callback in callbacks {
                    // SAFETY: successfully registered callbacks live until the process exits.
                    unsafe { callback() };
                }
            })
            .await
        {
            tracing::error!("moddedmc: stopping callbacks failed: {error}");
        }
    });
}

pub struct CommandTree {
    name: String,
    description: String,
    level: PermissionLvl,
    nodes: Vec<TreeNode>,
}

struct TreeNode {
    name: String,
    parent: Option<usize>,
    bounds: Option<(i32, i32)>,
    tag: Option<u64>,
}

fn valid_word(name: &str) -> bool {
    !name.is_empty() && !name.contains(char::is_whitespace)
}

unsafe extern "C" fn add_command(
    reg: Reg,
    name: Str,
    description: Str,
    op_level: u8,
    nodes: *const abi::CommandNode,
    count: usize,
) -> i32 {
    // Bound packet construction and the recursive command-tree walk.
    if !(1..=128).contains(&count) || nodes.is_null() {
        return code::INVALID_DEFINITION;
    }
    let level = match op_level {
        0 => PermissionLvl::Zero,
        1 => PermissionLvl::One,
        2 => PermissionLvl::Two,
        3 => PermissionLvl::Three,
        4 => PermissionLvl::Four,
        _ => return code::INVALID_DEFINITION,
    };
    // SAFETY: all definitions are borrowed for the call.
    let (name, description, nodes) = unsafe {
        (
            text(name),
            text(description),
            std::slice::from_raw_parts(nodes, count),
        )
    };
    if !valid_word(&name) {
        return code::INVALID_DEFINITION;
    }
    let mut tree: Vec<TreeNode> = Vec::with_capacity(count);
    for (index, node) in nodes.iter().enumerate() {
        let parent = if node.parent == abi::COMMAND_ROOT {
            None
        } else if (node.parent as usize) < index {
            Some(node.parent as usize)
        } else {
            return code::INVALID_DEFINITION;
        };
        let bounds = match node.kind {
            abi::COMMAND_LITERAL => None,
            abi::COMMAND_INTEGER if node.min <= node.max => Some((node.min, node.max)),
            _ => return code::INVALID_DEFINITION,
        };
        // SAFETY: node names are borrowed for the call.
        let name = unsafe { text(node.name) };
        if !valid_word(&name) {
            return code::INVALID_DEFINITION;
        }
        if tree
            .iter()
            .any(|node| node.parent == parent && node.name == name)
        {
            return code::DUPLICATE_ID;
        }
        tree.push(TreeNode {
            name,
            parent,
            bounds,
            tag: node.executable.then_some(node.tag),
        });
    }
    // SAFETY: live staging for this registration.
    unsafe { staging(reg) }.command_trees.push(CommandTree {
        name,
        description,
        level,
        nodes: tree,
    });
    code::OK
}

pub fn register_tree(server: &Server, module: Native, tree: CommandTree) {
    let permission = format!("{}:command.{}", module.id, tree.name);
    if let Err(error) = server
        .permission_manager
        .register_permission(Permission::new(
            &permission,
            &tree.description,
            PermissionDefault::Op(tree.level),
        ))
    {
        tracing::error!(
            "moddedmc: cannot register /{} ({}): {error}",
            tree.name,
            module.id
        );
        return;
    }
    let mut root = command(tree.name.clone(), tree.description).requires(permission);
    let mut children = vec![Vec::new(); tree.nodes.len()];
    for (index, node) in tree.nodes.into_iter().enumerate().rev() {
        let descendants = std::mem::take(&mut children[index]);
        let exec = node.tag.map(|tag| Forward { module, tag });
        let built: DetachedNode = if let Some((min, max)) = node.bounds {
            let mut arg = argument(node.name, IntegerArgumentType::new(min, max));
            if let Some(exec) = exec {
                arg = arg.executes(exec);
            }
            descendants
                .into_iter()
                .fold(arg, ArgumentBuilder::then)
                .into()
        } else {
            let mut arg = literal(node.name);
            if let Some(exec) = exec {
                arg = arg.executes(exec);
            }
            descendants
                .into_iter()
                .fold(arg, ArgumentBuilder::then)
                .into()
        };
        if let Some(parent) = node.parent {
            children[parent].push(built);
        } else {
            root = root.then(built);
        }
    }
    tracing::info!(
        "moddedmc: registered command /{} ({})",
        tree.name,
        module.id
    );
    native::install_command(server, &root.into());
}

unsafe extern "C" fn integer(source: *const c_void, name: Str, out: *mut i32) -> bool {
    // SAFETY: source and name live throughout the call, out may be null or room for one i32.
    unsafe {
        if let Some(context) = source.cast::<CommandContext<'_>>().as_ref()
            && let Ok(value) = IntegerArgumentType::get(context, name.str())
            && let Some(out) = out.as_mut()
        {
            *out = value;
            return true;
        }
    }
    false
}

unsafe extern "C" fn feedback(source: *const c_void, message: Str) {
    // SAFETY: both the context and message are live for the call.
    unsafe {
        if let Some(context) = source.cast::<CommandContext<'_>>().as_ref() {
            context
                .source
                .send_feedback(TextComponent::text(text(message)), false);
        }
    }
}
