//! Chunky-style world pregeneration through the moddedMC interfaces.

use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::PathBuf,
    sync::{
        Arc, Condvar, LazyLock, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    chunks::{self, Chunks},
    commands::{self, Argument, Commands, Node},
    lifecycle::{self, Lifecycle},
    world::{self, World},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ChunkPos(i32, i32);

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
    fn chunks(self) -> impl Iterator<Item = ChunkPos> {
        let (cx, cz) = (self.center_x >> 4, self.center_z >> 4);
        (0..=self.radius_chunks())
            .flat_map(move |ring| ring_chunks(cx, cz, ring))
            .filter(move |c| self.contains(*c))
    }

    /// Chunky's chunk-aligned shapes: a square is every chunk in the rings; a circle keeps
    /// chunks whose center is within `(2 * radius_chunks + 1) * 8` blocks of the center chunk's.
    fn contains(self, chunk: ChunkPos) -> bool {
        match self.shape {
            Shape::Square => true,
            Shape::Circle => {
                let dx = f64::from(chunk.0 - (self.center_x >> 4)) * 16.0;
                let dz = f64::from(chunk.1 - (self.center_z >> 4)) * 16.0;
                dx.hypot(dz) <= f64::from(2 * self.radius_chunks() + 1) * 8.0
            }
        }
    }

    fn total(self) -> u64 {
        let radius = i64::from(self.radius_chunks());
        let width = (2 * radius + 1) as u64;
        if self.shape == Shape::Square {
            return width * width;
        }

        // Sum the rows of the same circle as contains(), using half-chunk units.
        let limit = (2 * radius + 1).pow(2);
        let mut x = radius;
        let mut count = width;
        for z in 1..=radius {
            while 4 * (x * x + z * z) > limit {
                x -= 1;
            }
            count += 2 * (2 * x + 1) as u64;
        }
        count
    }
}

/// The square ring `ring` chunks away from (cx, cz), walked once.
fn ring_chunks(cx: i32, cz: i32, ring: i32) -> Box<dyn Iterator<Item = ChunkPos> + Send> {
    if ring == 0 {
        return Box::new(std::iter::once(ChunkPos(cx, cz)));
    }
    let (lo_x, hi_x, lo_z, hi_z) = (cx - ring, cx + ring, cz - ring, cz + ring);
    let top = (lo_x..=hi_x).map(move |x| ChunkPos(x, lo_z));
    let right = (lo_z + 1..=hi_z).map(move |z| ChunkPos(hi_x, z));
    let bottom = (lo_x..hi_x).rev().map(move |x| ChunkPos(x, hi_z));
    let left = (lo_z + 1..hi_z).rev().map(move |z| ChunkPos(lo_x, z));
    Box::new(top.chain(right).chain(bottom).chain(left))
}

struct Task {
    world: String,
    selection: Selection,
    total: AtomicU64,
    done: AtomicU64,
    /// Chunks a resumed task counted before the restart; not part of this run's rate.
    resumed_at: u64,
    paused: AtomicBool,
    cancelled: AtomicBool,
    /// The server is stopping: end the run but keep the saved task.
    stopping: AtomicBool,
    events: Sender<Event>,
    started: Instant,
    /// Last chunk handed out, for the progress line.
    current: Mutex<ChunkPos>,
}

impl Task {
    fn progress_line(&self, state: &str) -> String {
        let done = self.done.load(Relaxed);
        let total = self.total.load(Relaxed);
        let percent = if total == 0 {
            100.0
        } else {
            done as f64 * 100.0 / total as f64
        };
        let elapsed = self.started.elapsed().as_secs_f64().max(0.001);
        let rate = done.saturating_sub(self.resumed_at) as f64 / elapsed;
        let eta = if rate > 0.0 {
            (total.saturating_sub(done)) as f64 / rate
        } else {
            0.0
        };
        let current = *lock(&self.current);
        format!(
            "[Chunky] Task {state} for {}. Processed: {done} chunks ({percent:.2}%), ETA: {}, Rate: {rate:.1} cps, Current: {}, {}",
            self.world,
            clock(eta),
            current.0,
            current.1
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

struct Tasks {
    stopping: bool,
    running: HashMap<String, Arc<Task>>,
}

static TASKS: LazyLock<Mutex<Tasks>> = LazyLock::new(|| {
    Mutex::new(Tasks {
        stopping: false,
        running: HashMap::new(),
    })
});
static DRAINED: Condvar = Condvar::new();

enum Event {
    Chunk { index: u64, generated: bool },
    Wake,
}

fn launch(chunks: Chunks, world: &str, selection: Selection, skip: u64) -> bool {
    let mut tasks = lock(&TASKS);
    if tasks.stopping || tasks.running.contains_key(world) {
        return false;
    }
    let (events, receive) = mpsc::channel();
    let task = Arc::new(Task {
        world: world.to_owned(),
        selection,
        total: AtomicU64::new(0),
        done: AtomicU64::new(skip),
        resumed_at: skip,
        paused: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
        events,
        started: Instant::now(),
        current: Mutex::new(ChunkPos(selection.center_x >> 4, selection.center_z >> 4)),
    });
    let worker = task.clone();
    if let Err(err) = thread::Builder::new().name("chunky".into()).spawn(move || {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(chunks, &worker, &receive)
        }))
        .is_err()
        {
            moddedmc_api::error(&format!("[Chunky] Task panicked for {}.", worker.world));
        }
        lock(&TASKS).running.remove(&worker.world);
        DRAINED.notify_all();
    }) {
        moddedmc_api::warn(&format!("[Chunky] can't start the task for {world}: {err}"));
        return false;
    }
    tasks.running.insert(world.to_owned(), task);
    true
}

fn task_file(world: &str) -> PathBuf {
    moddedmc_api::data_dir()
        .join("tasks")
        .join(format!("{}.properties", world.replace(':', "_")))
}

/// Chunky-style properties: the selection and how many chunks are done.
fn task_text(task: &Task, done: u64) -> String {
    let s = task.selection;
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
    let text = fs::read_to_string(task_file(world)).ok()?;
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

fn save_task(task: &Task, prefix: u64) {
    let file = task_file(&task.world);
    let temp = file.with_extension("tmp");
    let result = (|| {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&temp, task_text(task, prefix))?;
        fs::rename(temp, file)
    })();
    if let Err(err) = result {
        moddedmc_api::warn(&format!(
            "[Chunky] can't save the task for {}: {err}",
            task.world
        ));
    }
}

fn run(chunks: Chunks, task: &Task, receive: &Receiver<Event>) {
    let total = task.selection.total();
    let mut sent = task.resumed_at.min(total);
    task.done.store(sent, Relaxed);
    task.total.store(total, Relaxed);
    let mut positions = task.selection.chunks().skip(sent as usize);
    let limit = in_flight();
    let mut pending = BTreeSet::new();
    let mut failed_at = u64::MAX;
    let mut report_at = Instant::now() + PROGRESS_EVERY;
    let ended = || task.cancelled.load(Relaxed) || task.stopping.load(Relaxed);
    loop {
        while sent < total && pending.len() < limit && !task.paused.load(Relaxed) && !ended() {
            let Some(pos) = positions.next() else {
                task.stopping.store(true, Relaxed);
                break;
            };
            let index = sent;
            let events = task.events.clone();
            if !chunks.ensure_generated(&task.world, pos.0, pos.1, move |generated| {
                let _ = events.send(Event::Chunk { index, generated });
            }) {
                task.stopping.store(true, Relaxed);
                break;
            }
            *lock(&task.current) = pos;
            pending.insert(sent);
            sent += 1;
        }
        if pending.is_empty() && (sent == total || ended()) {
            break;
        }

        let now = Instant::now();
        if now >= report_at {
            if !task.paused.load(Relaxed) && !ended() {
                moddedmc_api::info(&task.progress_line("running"));
            }
            // Only a completed prefix can be skipped on resume.
            save_task(
                task,
                pending.first().copied().unwrap_or(sent).min(failed_at),
            );
            report_at = now + PROGRESS_EVERY;
        }
        if let Ok(Event::Chunk { index, generated }) =
            receive.recv_timeout(report_at.saturating_duration_since(Instant::now()))
        {
            pending.remove(&index);
            if generated {
                task.done.fetch_add(1, Relaxed);
            } else {
                failed_at = failed_at.min(index);
                task.stopping.store(true, Relaxed);
            }
        }
    }

    if task.stopping.load(Relaxed) && !task.cancelled.load(Relaxed) {
        save_task(task, sent.min(failed_at));
        moddedmc_api::info(&format!(
            "[Chunky] Task stopped for {}; /chunky continue resumes it.",
            task.world
        ));
    } else {
        let file = task_file(&task.world);
        if let Err(err) = fs::remove_file(file)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            moddedmc_api::warn(&format!(
                "[Chunky] can't clear the task for {}: {err}",
                task.world
            ));
        }
        if task.cancelled.load(Relaxed) {
            moddedmc_api::info(&format!("[Chunky] Task cancelled for {}.", task.world));
        } else {
            moddedmc_api::info(&format!(
                "[Chunky] Task finished for {}. Processed: {} chunks (100.00%), Total time: {}",
                task.world,
                task.done.load(Relaxed),
                clock(task.started.elapsed().as_secs_f64())
            ));
        }
    }
}

fn stopping() {
    let mut tasks = lock(&TASKS);
    tasks.stopping = true;
    for task in tasks.running.values() {
        task.stopping.store(true, Relaxed);
        let _ = task.events.send(Event::Wake);
    }
    drop(DRAINED.wait_while(tasks, |tasks| !tasks.running.is_empty()));
}

const CENTER: u64 = 0;
const RADIUS: u64 = 1;
const SQUARE: u64 = 2;
const CIRCLE: u64 = 3;
const START: u64 = 4;
const PAUSE: u64 = 5;
const CONTINUE: u64 = 6;
const CANCEL: u64 = 7;
const PROGRESS: u64 = 8;

const fn lit(parent: Option<u32>, name: &str, tag: Option<u64>) -> Node<'_> {
    Node {
        parent,
        name,
        argument: Argument::Literal,
        tag,
    }
}

const fn int(parent: u32, name: &str, min: i32, tag: Option<u64>) -> Node<'_> {
    Node {
        parent: Some(parent),
        name,
        argument: Argument::Integer {
            min,
            max: MAX_RADIUS,
        },
        tag,
    }
}

struct Interfaces {
    world: World,
    chunks: Chunks,
    commands: Commands,
    lifecycle: Lifecycle,
}

#[derive(Default)]
struct Chunky(OnceLock<Interfaces>);

impl Mod for Chunky {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let interfaces = Interfaces {
            world: world::require()?,
            chunks: chunks::require()?,
            commands: commands::require()?,
            lifecycle: lifecycle::require()?,
        };
        let _ = self.0.set(interfaces);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let api = self.0.get().ok_or(RegistrationError::InvalidDefinition)?;
        api.lifecycle.on_stopping(registry, stopping)?;
        api.commands.add(
            registry,
            "chunky",
            "Pregenerates the world around a center.",
            4,
            &[
                lit(None, "center", None),
                int(0, "x", -MAX_RADIUS, None),
                int(1, "z", -MAX_RADIUS, Some(CENTER)),
                lit(None, "radius", None),
                int(3, "radius", 1, Some(RADIUS)),
                lit(None, "shape", None),
                lit(Some(5), "square", Some(SQUARE)),
                lit(Some(5), "circle", Some(CIRCLE)),
                lit(None, "start", Some(START)),
                lit(None, "pause", Some(PAUSE)),
                lit(None, "continue", Some(CONTINUE)),
                lit(None, "cancel", Some(CANCEL)),
                lit(None, "progress", Some(PROGRESS)),
            ],
        )
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let Some(api) = self.0.get() else { return 0 };
        let Some(name) = api
            .world
            .command_dimension(&command)
            .or_else(|| api.world.dimensions().next())
        else {
            return 0;
        };
        let (message, result) = match tag {
            CENTER => {
                let (Some(x), Some(z)) = (
                    api.commands.integer(&command, "x"),
                    api.commands.integer(&command, "z"),
                ) else {
                    return 0;
                };
                (
                    update_selection(name, |s| {
                        s.center_x = x;
                        s.center_z = z;
                    }),
                    1,
                )
            }
            RADIUS => {
                let Some(radius) = api.commands.integer(&command, "radius") else {
                    return 0;
                };
                (update_selection(name, |s| s.radius = radius), 1)
            }
            SQUARE | CIRCLE => (
                update_selection(name, |s| {
                    s.shape = if tag == SQUARE {
                        Shape::Square
                    } else {
                        Shape::Circle
                    };
                }),
                1,
            ),
            START => {
                let selection = lock(&SELECTIONS).get(name).copied().unwrap_or_default();
                if launch(api.chunks, name, selection, 0) {
                    (
                        format!(
                            "[Chunky] Task started for {name} at {}, {} with radius {}.",
                            selection.center_x, selection.center_z, selection.radius
                        ),
                        1,
                    )
                } else {
                    (format!("[Chunky] A task is already running for {name}."), 0)
                }
            }
            PAUSE | CONTINUE | CANCEL | PROGRESS => task_command(api.chunks, name, tag),
            _ => return 0,
        };
        api.commands.feedback(&command, &message);
        result
    }
}

fn update_selection(name: &str, change: impl FnOnce(&mut Selection)) -> String {
    let mut selections = lock(&SELECTIONS);
    let s = selections.entry(name.to_owned()).or_default();
    change(s);
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
    )
}

fn task_command(chunks: Chunks, name: &str, tag: u64) -> (String, i32) {
    let task = lock(&TASKS).running.get(name).cloned();
    if let Some(task) = task {
        let message = match tag {
            PAUSE => {
                task.paused.store(true, Relaxed);
                format!("[Chunky] Task paused for {name}.")
            }
            CONTINUE => {
                task.paused.store(false, Relaxed);
                let _ = task.events.send(Event::Wake);
                format!("[Chunky] Task continuing for {name}.")
            }
            CANCEL => {
                task.cancelled.store(true, Relaxed);
                let _ = task.events.send(Event::Wake);
                format!("[Chunky] Cancelling the task for {name}.")
            }
            _ => task.progress_line(if task.paused.load(Relaxed) {
                "paused"
            } else {
                "running"
            }),
        };
        return (message, 1);
    }
    if tag == CONTINUE
        && let Some((selection, done)) = read_task(name)
        && launch(chunks, name, selection, done)
    {
        return (format!("[Chunky] Task continuing for {name}."), 1);
    }
    (format!("[Chunky] No task is running for {name}."), 0)
}

moddedmc_api::export_mod!(Chunky::default());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_totals_match_the_selected_chunks() {
        for radius in [1, 17, 100, 500] {
            let selection = Selection {
                radius,
                shape: Shape::Circle,
                ..Selection::default()
            };
            assert_eq!(selection.total(), selection.chunks().count() as u64);
        }
    }

    #[test]
    fn rings_cover_the_square_once() {
        let selection = Selection {
            center_x: 40,
            center_z: -100,
            radius: 100,
            shape: Shape::Square,
        };
        let chunks: Vec<_> = selection.chunks().collect();
        let unique: std::collections::HashSet<_> = chunks.iter().copied().collect();
        assert_eq!(unique.len(), chunks.len());
        // Chunky: center chunks 2 and -7, ceil(100 / 16) = 7 rings, so 15 by 15 chunks.
        let xs = chunks.iter().map(|c| c.0);
        let zs = chunks.iter().map(|c| c.1);
        assert_eq!((xs.clone().min(), xs.max()), (Some(-5), Some(9)));
        assert_eq!((zs.clone().min(), zs.max()), (Some(-14), Some(0)));
        assert_eq!(chunks.len(), 15 * 15);
        assert_eq!(chunks[0], ChunkPos(2, -7));
    }
}
