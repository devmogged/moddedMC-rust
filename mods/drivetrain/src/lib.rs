//! A Create-style drivetrain on `moddedmc:blocks/1`: motors, shafts, small and large cogs,
//! clutches and loads (see [`network`] for the rules). A stock client sees the parts as vanilla
//! look-alikes; a load lights up while it turns.
//!
//! The mod keeps only its motor positions (`config/drivetrain/motors.txt`). Networks are solved
//! again from the blocks around the motors whenever a part is placed, broken, its neighbors
//! change, and once a second (which also covers chunks loading and the server restarting).
//!
//! Commands: `/dtbuild` builds the test rig at your feet, `/dtspeeds` lists every part's speed.

mod network;

use std::{
    fs,
    sync::{Mutex, PoisonError},
};

use moddedmc_api::{
    Command, Mod, Pos, RegistrationError, Registry,
    blocks::{self, Definitions, Event, EventKind},
    data_dir, tick_count, warn, world,
};
use network::{Kind, Part, Status};

const AXES: [&str; 3] = ["x", "y", "z"];
const RUNNING: [&str; 2] = ["false", "true"];
/// Ticks between full solves.
const PERIOD: u32 = 20;

const fn name(kind: Kind) -> &'static str {
    match kind {
        Kind::Motor => "drivetrain:motor",
        Kind::Shaft => "drivetrain:shaft",
        Kind::Clutch => "drivetrain:clutch",
        Kind::Cog => "drivetrain:cog",
        Kind::LargeCog => "drivetrain:large_cog",
        Kind::Load => "drivetrain:load",
    }
}

const KINDS: [Kind; 6] = [
    Kind::Motor,
    Kind::Shaft,
    Kind::Clutch,
    Kind::Cog,
    Kind::LargeCog,
    Kind::Load,
];

/// `drivetrain:cog[axis=x]` to a part.
fn parse(state: &str) -> Option<Part> {
    let (block, props) = state.split_once('[')?;
    let kind = *KINDS.iter().find(|&&k| name(k) == block)?;
    let axis = props
        .trim_end_matches(']')
        .split(',')
        .find_map(|p| p.strip_prefix("axis="))?;
    Some(Part {
        kind,
        axis: AXES.iter().position(|a| *a == axis)?,
    })
}

/// Motors by dimension.
struct State {
    motors: Vec<(String, Pos)>,
    last_solve: u64,
    /// When the once-a-second solve is next due; keeps one periodic tick chain alive.
    next_period: u64,
    /// Loads found by the last solve.
    loads: Vec<(String, Pos)>,
}

struct Drivetrain {
    state: Mutex<State>,
}

fn motors_file() -> std::path::PathBuf {
    data_dir().join("motors.txt")
}

fn at(p: Pos) -> String {
    format!("{} {} {}", p.x, p.y, p.z)
}

impl Drivetrain {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn save(motors: &[(String, Pos)]) {
        let text: String = motors
            .iter()
            .map(|(d, p)| format!("{d} {}\n", at(*p)))
            .collect();
        if let Err(e) = fs::write(motors_file(), text) {
            warn(&format!("drivetrain: could not save motors: {e}"));
        }
    }

    /// Solves every network and lights the loads that turn. Returns the networks.
    fn solve(&self, dimension: &str) -> Vec<network::Network> {
        let (Some(world), Some(blocks)) = (world::get(), blocks::get()) else {
            return Vec::new();
        };
        let motors: Vec<Pos> = self
            .state()
            .motors
            .iter()
            .filter(|(d, _)| d == dimension)
            .map(|(_, p)| *p)
            .collect();
        let read = |ps: &[Pos]| {
            world
                .get_blocks(dimension, ps)
                .into_iter()
                .map(|s| parse(&world.state_name(s?)?))
                .collect()
        };
        let networks = network::solve(&motors, read, |p| {
            blocks.is_powered(dimension, p).unwrap_or(false)
        });
        let mut writes = Vec::new();
        // Loads no network reaches any more stop too; loads stay listed while they exist.
        let reached: Vec<Pos> = networks
            .iter()
            .flat_map(|n| &n.speeds)
            .filter(|(_, part, _)| part.kind == Kind::Load)
            .map(|(p, _, _)| *p)
            .collect();
        let cut_off: Vec<Pos> = {
            let mut state = self.state();
            let cut_off: Vec<Pos> = state
                .loads
                .iter()
                .filter(|(d, p)| d == dimension && !reached.contains(p))
                .map(|(_, p)| *p)
                .collect();
            for &p in &reached {
                if !state.loads.iter().any(|(d, q)| d == dimension && *q == p) {
                    state.loads.push((dimension.to_owned(), p));
                }
            }
            cut_off
        };
        let found = world.get_blocks(dimension, &cut_off);
        let mut gone = Vec::new();
        for (pos, s) in cut_off.into_iter().zip(found) {
            match s.and_then(|s| parse(&world.state_name(s)?)) {
                Some(part) if part.kind == Kind::Load => {
                    let name = format!("drivetrain:load[axis={},running=false]", AXES[part.axis]);
                    if let Some(state) = world.state_id(&name) {
                        writes.push((pos, state));
                    }
                }
                Some(_) | None if s.is_some() => gone.push(pos),
                _ => {} // unloaded: keep it listed
            }
        }
        self.state()
            .loads
            .retain(|(d, p)| !(d == dimension && gone.contains(p)));
        for n in &networks {
            for &(pos, part, speed) in &n.speeds {
                if part.kind != Kind::Load {
                    continue;
                }
                let name = format!(
                    "drivetrain:load[axis={},running={}]",
                    AXES[part.axis],
                    speed != 0
                );
                if let Some(state) = world.state_id(&name) {
                    writes.push((pos, state));
                }
            }
        }
        // Only write loads whose state changes: a write wakes the neighbors.
        let current = world.get_blocks(dimension, &writes.iter().map(|w| w.0).collect::<Vec<_>>());
        let changed: Vec<(Pos, u32)> = writes
            .into_iter()
            .zip(current)
            .filter(|((_, want), now)| *now != Some(*want))
            .map(|(w, _)| w)
            .collect();
        world.set_blocks(dimension, &changed);
        networks
    }

    /// Solves at most once per server tick.
    fn solve_now(&self, dimension: &str) {
        let tick = tick_count();
        {
            let mut state = self.state();
            if state.last_solve == tick + 1 {
                return;
            }
            state.last_solve = tick + 1;
        }
        self.solve(dimension);
    }

    fn build(dimension: &str, base: Pos) -> usize {
        let Some(world) = world::get() else { return 0 };
        let p = |x, y, z| Pos {
            x: base.x + x,
            y: base.y + y,
            z: base.z + z,
        };
        let mut rig = vec![(p(0, 0, 0), "drivetrain:motor[axis=x]")];
        rig.extend((1..10).map(|x| (p(x, 0, 0), "drivetrain:shaft[axis=x]")));
        rig.extend([
            (p(10, 0, 0), "drivetrain:cog[axis=x]"),
            (p(10, 0, 1), "drivetrain:cog[axis=x]"),
            (p(10, 1, 2), "drivetrain:large_cog[axis=x]"),
            (p(11, 1, 2), "drivetrain:shaft[axis=x]"),
            (p(12, 1, 2), "drivetrain:clutch[axis=x]"),
            (p(13, 1, 2), "drivetrain:shaft[axis=x]"),
            (p(14, 1, 2), "drivetrain:load[axis=x,running=false]"),
        ]);
        let writes: Vec<(Pos, u32)> = rig
            .iter()
            .filter_map(|(pos, n)| Some((*pos, world.state_id(n)?)))
            .collect();
        world.set_blocks(dimension, &writes)
    }

    fn add_motor(&self, dimension: &str, pos: Pos) {
        let mut state = self.state();
        if !state
            .motors
            .iter()
            .any(|(d, p)| d == dimension && *p == pos)
        {
            state.motors.push((dimension.to_owned(), pos));
            Self::save(&state.motors);
        }
        drop(state);
        if let Some(blocks) = blocks::get() {
            blocks.schedule_tick(dimension, pos, 1);
        }
    }
}

impl Mod for Drivetrain {
    fn define_blocks(&self, blocks: &mut Definitions<'_>) -> Result<(), RegistrationError> {
        let by_axis = |f: &str| -> Vec<String> {
            ["east", "up", "south"]
                .iter()
                .map(|d| f.replace("{}", d))
                .collect()
        };
        let logs = |wood: &str| -> Vec<String> {
            AXES.iter()
                .map(|a| format!("minecraft:{wood}[axis={a}]"))
                .collect()
        };
        let axis: &[(&str, &[&str])] = &[("axis", &AXES)];
        let parts: [(Kind, Vec<String>); 5] = [
            (Kind::Motor, by_axis("minecraft:observer[facing={}]")),
            (Kind::Shaft, by_axis("minecraft:end_rod[facing={}]")),
            (Kind::Clutch, by_axis("minecraft:lightning_rod[facing={}]")),
            (Kind::Cog, logs("stripped_oak_log")),
            (Kind::LargeCog, logs("stripped_spruce_log")),
        ];
        for (kind, looks) in &parts {
            let looks: Vec<&str> = looks.iter().map(String::as_str).collect();
            blocks.block(name(*kind), axis, &looks)?;
        }
        let lamps: Vec<&str> = AXES
            .iter()
            .flat_map(|_| {
                [
                    "minecraft:redstone_lamp[lit=false]",
                    "minecraft:redstone_lamp[lit=true]",
                ]
            })
            .collect();
        blocks.block(
            name(Kind::Load),
            &[("axis", &AXES), ("running", &RUNNING)],
            &lamps,
        )
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        registry.add_command("dtbuild", "Build the drivetrain test rig at your feet", 0)?;
        registry.add_command("dtspeeds", "List the speed of every drivetrain part", 1)?;
        registry.add_command("dtlamps", "Show which loads are lit, without solving", 2)?;
        // Restart: the motors tick again, which solves their networks.
        let motors = fs::read_to_string(motors_file()).unwrap_or_default();
        let mut state = self.state();
        state.motors = motors
            .lines()
            .filter_map(|l| {
                let mut w = l.split(' ');
                let d = w.next()?.to_owned();
                let mut n = || w.next()?.parse::<i32>().ok();
                Some((
                    d,
                    Pos {
                        x: n()?,
                        y: n()?,
                        z: n()?,
                    },
                ))
            })
            .collect();
        if let Some(blocks) = blocks::get() {
            for (d, p) in &state.motors {
                blocks.schedule_tick(d, *p, 1);
            }
        }
        Ok(())
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let Some(player) = command.player else {
            return 0;
        };
        let dimension = player.dimension();
        if tag == 0 {
            let here = player.position();
            #[expect(clippy::cast_possible_truncation, reason = "block coordinates fit i32")]
            let base = Pos {
                x: here.x.floor() as i32,
                y: here.y.floor() as i32,
                z: here.z.floor() as i32,
            };
            let written = Self::build(dimension, base);
            self.add_motor(dimension, base);
            command.reply(&format!("dtbuild: {written} parts at {}", at(base)));
            return i32::try_from(written).unwrap_or(0);
        }
        if tag == 2 {
            let Some(world) = world::get() else { return 0 };
            let loads: Vec<Pos> = self
                .state()
                .loads
                .iter()
                .filter(|(d, _)| d == dimension)
                .map(|(_, p)| *p)
                .collect();
            let lit: Vec<String> = world
                .get_blocks(dimension, &loads)
                .into_iter()
                .zip(&loads)
                .map(|(s, p)| {
                    let name = s.and_then(|s| world.state_name(s)).unwrap_or_default();
                    format!("{},{},{}={}", p.x, p.y, p.z, name.contains("running=true"))
                })
                .collect();
            command.reply(&format!("dtlamps: {}", lit.join(" ")));
            return i32::try_from(lit.len()).unwrap_or(0);
        }
        let networks = self.solve(dimension);
        let mut lines = Vec::new();
        for n in &networks {
            let status = match n.status {
                Status::Running => "running",
                Status::Jammed => "jammed",
                Status::Overstressed => "overstressed",
            };
            let parts: Vec<String> = n
                .speeds
                .iter()
                .map(|(p, part, s)| {
                    let kind = name(part.kind).trim_start_matches("drivetrain:");
                    format!("{kind}@{},{},{}={s}", p.x, p.y, p.z)
                })
                .collect();
            lines.push(format!("{status} {}", parts.join(" ")));
        }
        command.reply(&format!("dtspeeds: {}", lines.join(" | ")));
        i32::try_from(networks.len()).unwrap_or(0)
    }

    fn block_event(&self, event: Event<'_>) -> bool {
        let Some(world) = world::get() else {
            return false;
        };
        let part = world.state_name(event.state).as_deref().and_then(parse);
        match event.kind {
            EventKind::Place if part.is_some_and(|p| p.kind == Kind::Motor) => {
                self.add_motor(event.dimension, event.pos);
            }
            EventKind::Break if part.is_some_and(|p| p.kind == Kind::Motor) => {
                let mut state = self.state();
                state
                    .motors
                    .retain(|(d, p)| !(d == event.dimension && *p == event.pos));
                Self::save(&state.motors);
            }
            EventKind::Tick if part.is_some_and(|p| p.kind == Kind::Motor) => {
                self.solve_now(event.dimension);
                let now = tick_count();
                let periodic = {
                    let mut state = self.state();
                    let due = now >= state.next_period;
                    if due {
                        state.next_period = now + u64::from(PERIOD);
                    }
                    due
                };
                if periodic && let Some(blocks) = blocks::get() {
                    blocks.schedule_tick(event.dimension, event.pos, PERIOD);
                }
                return false;
            }
            EventKind::Use => return false,
            _ => {}
        }
        // Something changed: solve on the next tick, once, through any motor.
        let motor = self
            .state()
            .motors
            .iter()
            .find(|(d, _)| d == event.dimension)
            .map(|(_, p)| *p);
        if let (Some(motor), Some(blocks)) = (motor, blocks::get()) {
            blocks.schedule_tick(event.dimension, motor, 1);
        }
        false
    }
}

moddedmc_api::export_mod!(Drivetrain {
    state: Mutex::new(State {
        motors: Vec::new(),
        last_solve: 0,
        next_period: 0,
        loads: Vec::new(),
    }),
});
