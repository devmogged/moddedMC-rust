use std::{
    fs::{self, File},
    io::{self, BufRead, BufReader, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::scenario::Scenario;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(45);
const BOOT_TIMEOUT: Duration = Duration::from_secs(180);
const POLL: Duration = Duration::from_millis(25);
const STORAGE: &str = "moddedmc_harness:sync";

#[derive(Clone)]
pub enum ServerProgram {
    Vanilla(PathBuf),
    Pumpkin(PathBuf),
}

impl ServerProgram {
    pub fn parse(value: &str) -> Result<Self, String> {
        if let Some(jar) = value.strip_prefix("vanilla:") {
            Ok(Self::Vanilla(file(jar)?))
        } else {
            Ok(Self::Pumpkin(file(value)?))
        }
    }

    pub const fn is_vanilla(&self) -> bool {
        matches!(self, Self::Vanilla(_))
    }

    fn command(&self) -> Command {
        match self {
            Self::Vanilla(jar) => {
                let mut command = Command::new("java");
                command
                    .args([
                        "-Xmx1G",
                        "-XX:ActiveProcessorCount=2",
                        "-Duser.language=en",
                        "-Duser.country=US",
                        "-Dfile.encoding=UTF-8",
                        "-jar",
                    ])
                    .arg(jar)
                    .arg("nogui");
                command
            }
            Self::Pumpkin(exe) => {
                let mut command = Command::new(exe);
                command
                    .env("RAYON_NUM_THREADS", "2")
                    .env("TOKIO_WORKER_THREADS", "2");
                command
            }
        }
    }
}

pub fn file(path: impl AsRef<Path>) -> Result<PathBuf, String> {
    let path = path.as_ref();
    let full = std::path::absolute(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if !full.is_file() {
        return Err(format!("{} is not a file", full.display()));
    }
    Ok(full)
}

pub struct Process {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<String>,
    reader: Option<JoinHandle<io::Result<()>>>,
    log: PathBuf,
}

impl Process {
    pub fn spawn(mut command: Command, dir: &Path, name: &str) -> Result<Self, String> {
        let log = dir.join(format!("{name}.log"));
        let mut output = File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
        let (source, sink) = io::pipe().map_err(|e| e.to_string())?;
        command
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(sink.try_clone().map_err(|e| e.to_string())?)
            .stderr(sink);
        hide_window(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("starting {name}: {e}"))?;
        let (send, events) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut incoming = BufReader::new(source);
            let mut bytes = Vec::new();
            while incoming.read_until(b'\n', &mut bytes)? != 0 {
                output.write_all(&bytes)?;
                let text = String::from_utf8_lossy(&bytes);
                let _ = send.send(text.trim_end_matches(['\r', '\n']).into());
                bytes.clear();
            }
            Ok(())
        });
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{name} has no stdin"));
        };
        Ok(Self {
            child,
            stdin,
            events,
            reader: Some(reader),
            log,
        })
    }

    pub fn send(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.stdin, "{line}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("writing {}: {e}", self.log.display()))
    }

    pub fn until(
        &self,
        label: &str,
        timeout: Duration,
        mut done: impl FnMut(&str) -> bool,
    ) -> Result<Vec<String>, String> {
        let deadline = Instant::now() + timeout;
        let mut lines = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self
                .events
                .recv_timeout(remaining)
                .map_err(|e| format!("waiting for {label} in {}: {e}", self.log.display()))?;
            if done(&line) {
                return Ok(lines);
            }
            lines.push(line);
        }
    }

    fn finish(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + BOOT_TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().map_err(|e| e.to_string())?
                && self.reader.as_ref().is_none_or(JoinHandle::is_finished)
            {
                if let Some(reader) = self.reader.take() {
                    reader
                        .join()
                        .map_err(|_| "log reader panicked")?
                        .map_err(|e| e.to_string())?;
                }
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!("{} exited with {status}", self.log.display()))
                };
            }
            if Instant::now() >= deadline {
                return Err(format!("shutdown timed out: {}", self.log.display()));
            }
            thread::sleep(POLL);
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None) | Err(_)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        // Windows descendants can retain a redirected pipe after a forced kill.
        // Only join the reader on the bounded, graceful shutdown path.
    }
}

fn hide_window(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = command;
}

pub struct Server {
    process: Process,
    vanilla: bool,
    pub dir: PathBuf,
    pub port: u16,
    pub origin: u64,
    fence: u64,
    pub history: Vec<String>,
    pub expect_cursor: usize,
}

impl Server {
    pub fn start(
        program: &ServerProgram,
        scenario: &Scenario,
        dir: PathBuf,
    ) -> Result<Self, String> {
        let socket = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
        let port = socket
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        configure(scenario, &dir, port, program.is_vanilla())?;
        let command = program.command();
        drop(socket);
        let process = Process::spawn(command, &dir, "console")?;
        let mut server = Self {
            process,
            vanilla: program.is_vanilla(),
            dir,
            port,
            origin: 0,
            fence: 0,
            history: Vec::new(),
            expect_cursor: 0,
        };
        let vanilla = server.vanilla;
        server.history = server.process.until("server ready", BOOT_TIMEOUT, |line| {
            if vanilla {
                line.contains("Done (") && line.contains("For help")
            } else {
                line.contains("Started server; took")
            }
        })?;
        if !server.history.iter().any(|line| {
            if vanilla {
                line.ends_with("Starting minecraft server version 26.3")
            } else {
                line.contains("Java Minecraft (Protocol 777)")
            }
        }) {
            return Err(format!(
                "{} did not advertise Java 26.3 at startup",
                server.dir.display()
            ));
        }
        server.require("tick freeze", "The game is frozen")?;
        server.origin = server.clock()?;
        Ok(server)
    }

    pub fn prepare(&mut self, scenario: &Scenario) -> Result<(), String> {
        for (rule, value) in [
            ("spawn_mobs", "false"),
            ("advance_time", "false"),
            ("advance_weather", "false"),
            ("random_tick_speed", "0"),
            ("respawn_radius", "0"),
        ] {
            self.require(
                &format!("gamerule minecraft:{rule} {value}"),
                &format!("is now set to {value}"),
            )?;
        }
        self.require("setworldspawn 0 150 0", "Set the world spawn point")?;
        let [min_x, _, min_z] = scenario.area.min;
        let [max_x, _, max_z] = scenario.area.max;
        self.require(
            &format!("forceload add {min_x} {min_z} {max_x} {max_z}"),
            "force loaded",
        )?;
        let deadline = Instant::now() + BOOT_TIMEOUT;
        for (chunk_x, chunk_z) in scenario.area.chunks() {
            let query = format!("execute if loaded {} 0 {}", chunk_x * 16, chunk_z * 16);
            loop {
                let out = self.command(&query)?;
                if out.iter().any(|s| s.starts_with("Test passed")) {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(format!(
                        "{query} did not succeed in {}: {out:?}",
                        self.dir.display()
                    ));
                }
                thread::sleep(POLL);
            }
        }
        self.verify_clock(0)?;
        self.expect_cursor = self.history.len();
        Ok(())
    }

    pub fn command(&mut self, command: &str) -> Result<Vec<String>, String> {
        self.fence += 1;
        let marker = format!("harness_fence_{}", self.fence);
        self.process.send(command)?;
        self.process
            .send(&format!("data get storage {STORAGE} {marker}"))?;
        let marker = format!("Found no elements matching {marker}");
        let vanilla = self.vanilla;
        let raw = self.process.until(command, COMMAND_TIMEOUT, |line| {
            feedback(line, vanilla).is_some_and(|text| text == marker)
        })?;
        let mut output = Vec::new();
        let mut continuation = false;
        for line in raw {
            if let Some(text) = feedback(&line, vanilla) {
                output.push(text.into());
                continuation = vanilla;
            } else if continuation && !line.starts_with('[') {
                output.push(line.trim().into());
            } else {
                continuation = false;
            }
            self.history.push(line);
        }
        Ok(output)
    }

    pub fn require(&mut self, command: &str, expected: &str) -> Result<(), String> {
        let output = self.command(command)?;
        if !output.iter().any(|line| line.contains(expected)) {
            return Err(format!(
                "{} rejected {command:?}: {output:?}",
                self.dir.display()
            ));
        }
        Ok(())
    }

    fn clock(&mut self) -> Result<u64, String> {
        self.command("time query gametime")?
            .iter()
            .find_map(|line| {
                line.strip_prefix("The game time is ")?
                    .strip_suffix(" tick(s)")?
                    .parse()
                    .ok()
            })
            .ok_or_else(|| format!("game clock missing in {}", self.dir.display()))
    }

    pub fn verify_clock(&mut self, elapsed: u64) -> Result<(), String> {
        let observed = self.clock()?;
        if self.origin + elapsed != observed {
            return Err(format!(
                "{} game clock: expected {} ({}+{elapsed}), observed {observed}",
                self.dir.display(),
                self.origin + elapsed,
                self.origin
            ));
        }
        Ok(())
    }

    pub fn begin_step(&mut self, count: u32) -> Result<(), String> {
        self.require(
            &format!("tick step {count}"),
            &format!("Stepping {count} tick(s)"),
        )
    }

    pub fn finish_step(&mut self, elapsed: u64, count: u32) -> Result<(), String> {
        let expected = self.origin + elapsed;
        let deadline =
            Instant::now() + Duration::from_millis(u64::from(count) * 125) + COMMAND_TIMEOUT;
        loop {
            self.require("tick query", "The game is frozen")?;
            let observed = self.clock()?;
            match observed.cmp(&expected) {
                std::cmp::Ordering::Equal => return Ok(()),
                std::cmp::Ordering::Greater => {
                    return Err(format!(
                        "{} stepped beyond {expected} to {observed}",
                        self.dir.display()
                    ));
                }
                std::cmp::Ordering::Less if Instant::now() >= deadline => {
                    return Err(format!(
                        "{} stopped advancing at {observed}, expected {expected}",
                        self.dir.display()
                    ));
                }
                std::cmp::Ordering::Less => thread::sleep(POLL),
            }
        }
    }

    pub fn flush(&mut self) -> Result<(), String> {
        self.process.send("save-all flush")?;
        let vanilla = self.vanilla;
        let mut saved = false;
        let raw = self.process.until("save-all flush", BOOT_TIMEOUT, |line| {
            let Some(text) = feedback(line, vanilla) else {
                return false;
            };
            saved = text == "Saved the game";
            saved || text == "Saving failed"
        })?;
        if !saved || raw.iter().any(|line| line.contains("Failed to save")) {
            return Err(format!("save failed in {}", self.dir.display()));
        }
        self.history.extend(raw);
        Ok(())
    }

    pub fn request_stop(&mut self) -> Result<(), String> {
        self.process.send("stop")
    }

    pub fn finish_stop(&mut self) -> Result<(), String> {
        self.process.finish()?;
        let log = fs::read_to_string(&self.process.log).map_err(|e| e.to_string())?;
        if log.lines().any(|line| {
            line.contains("Failed to save")
                || line.contains("Failed to flush")
                || line.contains("Failed to write chunk")
                || line.contains("Failed to write region")
        }) {
            return Err(format!(
                "save or flush failed in {}",
                self.process.log.display()
            ));
        }
        Ok(())
    }

    pub const fn is_vanilla(&self) -> bool {
        self.vanilla
    }
}

fn feedback(line: &str, vanilla: bool) -> Option<&str> {
    if vanilla {
        let (_, text) = line.split_once("]: ")?;
        text.strip_prefix("System chat: ")
    } else if line.as_bytes().get(2) == Some(&b':')
        || line.starts_with('[') && line.contains(" INFO ")
    {
        None
    } else {
        Some(line.trim())
    }
}

fn configure(scenario: &Scenario, dir: &Path, port: u16, vanilla: bool) -> Result<(), String> {
    let get = |key: &str, fallback: &'static str| {
        scenario
            .properties
            .get(key)
            .map_or(fallback, String::as_str)
    };
    let seed = scenario.seed;
    let difficulty = get("difficulty", "normal");
    let gamemode = get("gamemode", "survival");
    let hardcore = get("hardcore", "false");
    let view = get("view-distance", "3");
    let simulation = get("simulation-distance", "3");
    let protection = get("spawn-protection", "0");
    let players = get("max-players", "20");
    let files = if vanilla {
        vec![
            ("eula.txt", "eula=true\n".into()),
            (
                "server.properties",
                format!(
                    "\
server-ip=127.0.0.1\nserver-port={port}\nlevel-name=world\nlevel-seed={seed}\n\
difficulty={difficulty}\ngamemode={gamemode}\nhardcore={hardcore}\nview-distance={view}\n\
simulation-distance={simulation}\nspawn-protection={protection}\nmax-players={players}\n\
online-mode=false\nnetwork-compression-threshold=-1\npause-when-empty-seconds=-1\n\
white-list=false\nenforce-whitelist=false\nenable-rcon=false\nenable-query=false\n"
                ),
            ),
        ]
    } else {
        let difficulty = upper_first(difficulty);
        let gamemode = upper_first(gamemode);
        vec![(
            "pumpkin.toml",
            format!(
                "\
seed = \"{seed}\"\ndefault_difficulty = \"{difficulty}\"\ndefault_gamemode = \"{gamemode}\"\n\
hardcore = {hardcore}\nspawn_protection = {protection}\nwhite_list = false\nenforce_whitelist = false\n[logging]\ncolor = false\n\
[world]\nautosave_ticks = 0\n[world.chunk]\ntype = \"anvil\"\n\
[networking.java]\naddress = \"127.0.0.1:{port}\"\nonline_mode = false\nencryption = false\n\
view_distance = {view}\nsimulation_distance = {simulation}\nmax_players = {players}\n\
[networking.java.compression]\nenabled = false\n[networking.bedrock]\nenabled = false\n\
[telemetry]\nenabled = false\n"
            ),
        )]
    };
    for (name, content) in files {
        fs::write(dir.join(name), content)
            .map_err(|e| format!("writing {name} in {}: {e}", dir.display()))?;
    }
    Ok(())
}

fn upper_first(text: &str) -> String {
    let mut value = text.to_owned();
    if let Some(first) = value.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    value
}
