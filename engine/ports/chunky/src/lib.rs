//! A port of Chunky's world pregeneration: pick a center, radius and shape per world, then
//! `/chunky start` generates every chunk in it, nearest first, and saves them. Progress goes to
//! the console every few seconds as `[Chunky] Task running for <world>. Processed: ...`, which
//! the device agent and the app read.
//!
//! Chunks that already exist are counted and skipped, like Chunky does. A task's progress is
//! saved every few seconds, so `/chunky continue` after a restart picks it up again.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    },
    time::{Duration, Instant},
};

use pumpkin_core::{
    command::{
        argument_builder::{ArgumentBuilder, argument, command, literal},
        argument_types::core::integer::IntegerArgumentType,
        context::command_context::CommandContext,
        node::{CommandExecutor, CommandExecutorResult},
    },
    plugin::{Context, Plugin, PluginFuture, PluginMetadata},
    world::World,
};
use pumpkin_util::{
    PermissionLvl,
    math::vector2::Vector2,
    permission::{Permission, PermissionDefault},
    text::TextComponent,
};
use tokio::sync::{Notify, Semaphore};

const PERMISSION: &str = "port-chunky:command.chunky";
/// Chunky's defaults: a 500 block square around 0, 0.
const DEFAULT_RADIUS: i32 = 500;
const PROGRESS_EVERY: Duration = Duration::from_secs(5);
/// The world border's limit; nothing generates past it.
const MAX_RADIUS: i32 = 29_999_984;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Square,
    Circle,
}

#[derive(Clone, Copy)]
struct Selection {
    center_x: i32,
    center_z: i32,
    radius: i32,
    shape: Shape,
}

impl Default for Selection {
    fn default() -> Self {
        Self {
            center_x: 0,
            center_z: 0,
            radius: DEFAULT_RADIUS,
            shape: Shape::Square,
        }
    }
}

impl Selection {
    /// Chunky's `Selection.radiusChunksX`: the radius rounded up to whole chunks.
    const fn radius_chunks(self) -> i32 {
        (self.radius + 15) >> 4
    }

    /// Chunks in the selection, ring by ring outwards from the center chunk (Chunky's default
    /// ring pattern), filtered by the shape like `GenerationTask` does.
    fn chunks(self) -> impl Iterator<Item = Vector2<i32>> {
        let (cx, cz) = (self.center_x >> 4, self.center_z >> 4);
        (0..=self.radius_chunks())
            .flat_map(move |ring| ring_chunks(cx, cz, ring))
            .filter(move |c| self.contains(*c))
    }

    /// Chunky's chunk-aligned shapes: a square is every chunk in the rings; a circle keeps
    /// chunks whose center is within `(2 * radius_chunks + 1) * 8` blocks of the center chunk's.
    fn contains(self, chunk: Vector2<i32>) -> bool {
        match self.shape {
            Shape::Square => true,
            Shape::Circle => {
                let dx = f64::from(chunk.x - (self.center_x >> 4)) * 16.0;
                let dz = f64::from(chunk.y - (self.center_z >> 4)) * 16.0;
                dx.hypot(dz) <= f64::from(2 * self.radius_chunks() + 1) * 8.0
            }
        }
    }

    fn total(self) -> u64 {
        self.chunks().count() as u64
    }
}

/// The square ring `ring` chunks away from (cx, cz), walked once.
fn ring_chunks(cx: i32, cz: i32, ring: i32) -> Box<dyn Iterator<Item = Vector2<i32>> + Send> {
    if ring == 0 {
        return Box::new(std::iter::once(Vector2::new(cx, cz)));
    }
    let (lo_x, hi_x, lo_z, hi_z) = (cx - ring, cx + ring, cz - ring, cz + ring);
    let top = (lo_x..=hi_x).map(move |x| Vector2::new(x, lo_z));
    let right = (lo_z + 1..=hi_z).map(move |z| Vector2::new(hi_x, z));
    let bottom = (lo_x..hi_x).rev().map(move |x| Vector2::new(x, hi_z));
    let left = (lo_z + 1..hi_z).rev().map(move |z| Vector2::new(lo_x, z));
    Box::new(top.chain(right).chain(bottom).chain(left))
}

struct Task {
    world: String,
    selection: Selection,
    total: u64,
    done: AtomicU64,
    /// Chunks a resumed task counted before the restart; not part of this run's rate.
    resumed_at: u64,
    paused: AtomicBool,
    cancelled: AtomicBool,
    /// The server is stopping: end the run but keep the saved task.
    stopping: AtomicBool,
    resume: Notify,
    started: Instant,
    /// Last chunk handed out, for the progress line.
    current: Mutex<Vector2<i32>>,
}

impl Task {
    fn progress_line(&self, state: &str) -> String {
        let done = self.done.load(Relaxed);
        let percent = if self.total == 0 {
            100.0
        } else {
            done as f64 * 100.0 / self.total as f64
        };
        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let rate = done.saturating_sub(self.resumed_at) as f64 / elapsed;
        let eta = if rate > 0.0 {
            (self.total.saturating_sub(done)) as f64 / rate
        } else {
            0.0
        };
        let current = *lock(&self.current);
        format!(
            "[Chunky] Task {state} for {}. Processed: {done} chunks ({percent:.2}%), ETA: {}, Rate: {rate:.1} cps, Current: {}, {}",
            self.world,
            clock(eta),
            current.x,
            current.y
        )
    }
}

fn clock(secs: f64) -> String {
    let secs = secs as u64;
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Selections and running tasks, by world name.
static SELECTIONS: LazyLock<Mutex<HashMap<String, Selection>>> = LazyLock::new(Default::default);
static TASKS: LazyLock<Mutex<HashMap<String, Arc<Task>>>> = LazyLock::new(Default::default);

fn world_of(context: &CommandContext) -> Option<Arc<World>> {
    context
        .source
        .world
        .clone()
        .or_else(|| context.server().worlds.load().first().cloned())
}

fn world_name(world: &World) -> String {
    world.dimension.minecraft_name.to_string()
}

fn reply(context: &CommandContext, message: String) {
    context
        .source
        .send_feedback(TextComponent::text(message), false);
}

fn selection(world: &str) -> Selection {
    lock(&SELECTIONS).get(world).copied().unwrap_or_default()
}

fn update_selection(context: &CommandContext, change: impl FnOnce(&mut Selection)) -> i32 {
    let Some(world) = world_of(context) else {
        return 0;
    };
    let name = world_name(&world);
    let mut selections = lock(&SELECTIONS);
    let selection = selections.entry(name.clone()).or_default();
    change(selection);
    let s = *selection;
    drop(selections);
    reply(
        context,
        format!(
            "[Chunky] {name}: center {}, {}, radius {}, {}",
            s.center_x,
            s.center_z,
            s.radius,
            if s.shape == Shape::Square {
                "square"
            } else {
                "circle"
            }
        ),
    );
    1
}

struct Center;
impl CommandExecutor for Center {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let x = IntegerArgumentType::get(context, "x")?;
        let z = IntegerArgumentType::get(context, "z")?;
        Ok(update_selection(context, |s| {
            s.center_x = x;
            s.center_z = z;
        }))
    }
}

struct Radius;
impl CommandExecutor for Radius {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let radius = IntegerArgumentType::get(context, "radius")?;
        Ok(update_selection(context, |s| s.radius = radius))
    }
}

struct SetShape(Shape);
impl CommandExecutor for SetShape {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let shape = self.0;
        Ok(update_selection(context, |s| s.shape = shape))
    }
}

/// Starts a task for `world`, `skip` chunks in when resuming. `None` when one is running.
fn launch(
    context: &CommandContext,
    world: Arc<World>,
    selection: Selection,
    skip: u64,
) -> Option<String> {
    let name = world_name(&world);
    let task = {
        let mut tasks = lock(&TASKS);
        if tasks.contains_key(&name) {
            return None;
        }
        let task = Arc::new(Task {
            world: name.clone(),
            selection,
            total: selection.total(),
            done: AtomicU64::new(skip),
            resumed_at: skip,
            paused: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            stopping: AtomicBool::new(false),
            resume: Notify::new(),
            started: Instant::now(),
            current: Mutex::new(Vector2::new(
                selection.center_x >> 4,
                selection.center_z >> 4,
            )),
        });
        tasks.insert(name.clone(), task.clone());
        task
    };
    context.server().spawn_task(run(world, task));
    Some(name)
}

struct Start;
impl CommandExecutor for Start {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let Some(world) = world_of(context) else {
            return Ok(0);
        };
        let selection = selection(&world_name(&world));
        let Some(name) = launch(context, world.clone(), selection, 0) else {
            reply(
                context,
                format!(
                    "[Chunky] A task is already running for {}.",
                    world_name(&world)
                ),
            );
            return Ok(0);
        };
        reply(
            context,
            format!(
                "[Chunky] Task started for {name} at {}, {} with radius {}.",
                selection.center_x, selection.center_z, selection.radius
            ),
        );
        Ok(1)
    }
}

/// Where the port keeps saved tasks: its plugin data folder.
static DATA: OnceLock<PathBuf> = OnceLock::new();

fn task_file(world: &str) -> Option<PathBuf> {
    Some(
        DATA.get()?
            .join("tasks")
            .join(format!("{}.properties", world.replace(':', "_"))),
    )
}

/// Chunky-style properties: the selection and how many chunks are done.
fn task_text(task: &Task) -> String {
    let s = task.selection;
    // Chunks still generating when the server stops aren't saved; start that far back.
    let done = task.done.load(Relaxed).saturating_sub(in_flight() as u64);
    format!(
        "center-x={}
center-z={}
radius={}
shape={}
chunks={done}
",
        s.center_x,
        s.center_z,
        s.radius,
        if s.shape == Shape::Square {
            "square"
        } else {
            "circle"
        }
    )
}

fn read_task(world: &str) -> Option<(Selection, u64)> {
    let text = std::fs::read_to_string(task_file(world)?).ok()?;
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
            .map(str::trim)
    };
    let number = |key: &str| value(key)?.parse::<i32>().ok();
    let selection = Selection {
        center_x: number("center-x")?,
        center_z: number("center-z")?,
        radius: number("radius").filter(|r| (1..=MAX_RADIUS).contains(r))?,
        shape: if value("shape")? == "circle" {
            Shape::Circle
        } else {
            Shape::Square
        },
    };
    Some((selection, value("chunks")?.parse().ok()?))
}

/// Chunks generating at once. Each holds a full chunk and its lower-status neighbours in memory,
/// so this bounds RAM as much as it uses the generation threads.
fn in_flight() -> usize {
    std::thread::available_parallelism()
        .map_or(4, |n| n.get() * 2)
        .clamp(4, 64)
}

async fn run(world: Arc<World>, task: Arc<Task>) {
    let level = world.level.clone();
    let permits = Arc::new(Semaphore::new(in_flight()));
    let reporter = {
        let task = task.clone();
        tokio::spawn(async move {
            let mut every = tokio::time::interval(PROGRESS_EVERY);
            every.tick().await;
            loop {
                every.tick().await;
                if !task.paused.load(Relaxed) {
                    tracing::info!("{}", task.progress_line("running"));
                }
                if let Some(file) = task_file(&task.world) {
                    if let Some(dir) = file.parent() {
                        let _ = tokio::fs::create_dir_all(dir).await;
                    }
                    if let Err(err) = tokio::fs::write(&file, task_text(&task)).await {
                        tracing::warn!("[Chunky] can't save the task for {}: {err}", task.world);
                    }
                }
            }
        })
    };
    for pos in task.selection.chunks().skip(task.resumed_at as usize) {
        let ended = || task.cancelled.load(Relaxed) || task.stopping.load(Relaxed);
        while task.paused.load(Relaxed) && !ended() {
            task.resume.notified().await;
        }
        if ended() {
            break;
        }
        let Ok(permit) = permits.clone().acquire_owned().await else {
            break;
        };
        *lock(&task.current) = pos;
        let (level, task) = (level.clone(), task.clone());
        tokio::spawn(async move {
            if !level.is_chunk_generated(pos).await {
                level.get_or_fetch_chunk(pos, |_| ()).await;
            }
            task.done.fetch_add(1, Relaxed);
            drop(permit);
        });
    }
    // Wait for the chunks still generating.
    let _ = permits.acquire_many(in_flight() as u32).await;
    reporter.abort();
    lock(&TASKS).remove(&task.world);
    if task.stopping.load(Relaxed) && !task.cancelled.load(Relaxed) {
        if let Some(file) = task_file(&task.world) {
            if let Some(dir) = file.parent() {
                let _ = tokio::fs::create_dir_all(dir).await;
            }
            let _ = tokio::fs::write(file, task_text(&task)).await;
        }
        tracing::info!(
            "[Chunky] Task stopped for {}; /chunky continue resumes it.",
            task.world
        );
        return;
    }
    if let Some(file) = task_file(&task.world) {
        let _ = tokio::fs::remove_file(file).await;
    }
    if task.cancelled.load(Relaxed) {
        tracing::info!("[Chunky] Task cancelled for {}.", task.world);
    } else {
        tracing::info!(
            "[Chunky] Task finished for {}. Processed: {} chunks (100.00%), Total time: {}",
            task.world,
            task.done.load(Relaxed),
            clock(task.started.elapsed().as_secs_f64())
        );
    }
}

/// Applies `f` to the running task of the sender's world.
fn with_task(context: &CommandContext, f: impl FnOnce(&Task) -> String) -> i32 {
    let Some(world) = world_of(context) else {
        return 0;
    };
    let name = world_name(&world);
    let task = lock(&TASKS).get(&name).cloned();
    let (message, result) = task.map_or_else(
        || (format!("[Chunky] No task is running for {name}."), 0),
        |task| (f(&task), 1),
    );
    reply(context, message);
    result
}

struct Pause;
impl CommandExecutor for Pause {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        Ok(with_task(context, |task| {
            task.paused.store(true, Relaxed);
            format!("[Chunky] Task paused for {}.", task.world)
        }))
    }
}

struct Continue;
impl CommandExecutor for Continue {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        let Some(world) = world_of(context) else {
            return Ok(0);
        };
        let name = world_name(&world);
        if !lock(&TASKS).contains_key(&name)
            && let Some((selection, done)) = read_task(&name)
            && launch(context, world, selection, done).is_some()
        {
            reply(context, format!("[Chunky] Task continuing for {name}."));
            return Ok(1);
        }
        Ok(with_task(context, |task| {
            task.paused.store(false, Relaxed);
            task.resume.notify_waiters();
            format!("[Chunky] Task continuing for {}.", task.world)
        }))
    }
}

struct Cancel;
impl CommandExecutor for Cancel {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        Ok(with_task(context, |task| {
            task.cancelled.store(true, Relaxed);
            task.resume.notify_waiters();
            format!("[Chunky] Cancelling the task for {}.", task.world)
        }))
    }
}

struct Progress;
impl CommandExecutor for Progress {
    fn execute(&self, context: &CommandContext) -> CommandExecutorResult {
        Ok(with_task(context, |task| {
            task.progress_line(if task.paused.load(Relaxed) {
                "paused"
            } else {
                "running"
            })
        }))
    }
}

struct Chunky;

impl Plugin for Chunky {
    fn on_load(&self, context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async move {
            let _ = DATA.set(context.get_data_folder());
            context.register_permission(Permission::new(
                PERMISSION,
                "Pregenerate the world.",
                PermissionDefault::Op(PermissionLvl::Four),
            ))?;
            let node = command("chunky", "Pregenerates the world around a center.")
                .then(
                    literal("center").then(
                        argument("x", IntegerArgumentType::new(-MAX_RADIUS, MAX_RADIUS)).then(
                            argument("z", IntegerArgumentType::new(-MAX_RADIUS, MAX_RADIUS))
                                .executes(Center),
                        ),
                    ),
                )
                .then(literal("radius").then(
                    argument("radius", IntegerArgumentType::new(1, MAX_RADIUS)).executes(Radius),
                ))
                .then(
                    literal("shape")
                        .then(literal("square").executes(SetShape(Shape::Square)))
                        .then(literal("circle").executes(SetShape(Shape::Circle))),
                )
                .then(literal("start").executes(Start))
                .then(literal("pause").executes(Pause))
                .then(literal("continue").executes(Continue))
                .then(literal("cancel").executes(Cancel))
                .then(literal("progress").executes(Progress));
            context.register_command(node, PERMISSION);
            Ok(())
        })
    }

    fn on_unload(&self, _context: Arc<Context>) -> PluginFuture<'_, Result<(), String>> {
        Box::pin(async {
            // The server waits for running tasks before it saves the worlds; end them first.
            for task in lock(&TASKS).values() {
                task.stopping.store(true, Relaxed);
                task.resume.notify_waiters();
            }
            Ok(())
        })
    }
}

/// What the generated pack server registers: `launch.builtin_plugins.push(port_chunky::builtin())`.
#[must_use]
pub fn builtin() -> (PluginMetadata, Arc<dyn Plugin>) {
    let metadata = PluginMetadata {
        name: "port-chunky".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        authors: vec!["devmogged".into()],
        description: "Chunky-style world pregeneration.".into(),
        dependencies: Vec::new(),
        permissions: Vec::new(),
    };
    (metadata, Arc::new(Chunky))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rings_cover_the_square_once() {
        let selection = Selection {
            center_x: 40,
            center_z: -100,
            radius: 100,
            shape: Shape::Square,
        };
        let chunks: Vec<_> = selection.chunks().collect();
        let unique: std::collections::HashSet<_> = chunks.iter().map(|c| (c.x, c.y)).collect();
        assert_eq!(unique.len(), chunks.len());
        // Chunky: center chunks 2 and -7, ceil(100 / 16) = 7 rings, so 15 by 15 chunks.
        let xs = chunks.iter().map(|c| c.x);
        let zs = chunks.iter().map(|c| c.y);
        assert_eq!((xs.clone().min(), xs.max()), (Some(-5), Some(9)));
        assert_eq!((zs.clone().min(), zs.max()), (Some(-14), Some(0)));
        assert_eq!(chunks.len(), 15 * 15);
        assert_eq!(chunks[0], Vector2::new(2, -7));
    }
}
