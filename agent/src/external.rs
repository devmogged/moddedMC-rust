//! Minecraft servers on this machine that the agent didn't start: a Java or Pumpkin process
//! listening on a TCP port that answers the game's status ping, and, while stopped, the folders
//! such servers ran in before or that sit in the configured `find_in` folders. Read-only; the
//! agent can't control them, only show them.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::LazyLock,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    sync::Mutex,
    time::timeout,
};

/// The app polls every two seconds; a scan (process list, socket table, pings) costs more.
const CACHE_FOR: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_millis(800);
/// Status responses carry a favicon at most; anything bigger isn't a Minecraft server.
const MAX_STATUS: usize = 1 << 20;

#[derive(Serialize, Clone)]
pub struct External {
    /// The folder the process runs in, which is what people call their server.
    pub name: String,
    pub pid: u32,
    pub port: u16,
    pub program: String,
    pub players: Option<u32>,
    pub max_players: Option<u32>,
    pub version: Option<String>,
    /// Where it runs, when the OS tells us.
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

/// The last scan and when it ran.
type Scan = (Instant, Vec<External>);

static CACHE: LazyLock<Mutex<Option<Scan>>> = LazyLock::new(|| Mutex::new(None));

/// External servers, skipping the processes the agent runs itself.
pub async fn scan(managed: &HashSet<u32>) -> Vec<External> {
    let mut cache = CACHE.lock().await;
    if let Some((at, list)) = cache.as_ref()
        && at.elapsed() < CACHE_FOR
    {
        return list
            .iter()
            .filter(|e| !managed.contains(&e.pid))
            .cloned()
            .collect();
    }
    let list = scan_now().await;
    *cache = Some((Instant::now(), list.clone()));
    list.into_iter()
        .filter(|e| !managed.contains(&e.pid))
        .collect()
}

async fn scan_now() -> Vec<External> {
    let candidates = tokio::task::spawn_blocking(candidates)
        .await
        .unwrap_or_default();
    if candidates.is_empty() {
        return Vec::new();
    }
    let ports = listening_ports().await;
    let mut out = Vec::new();
    for (pid, (name, program, dir)) in candidates {
        for &port in ports.get(&pid).into_iter().flatten() {
            if let Some(status) = status(port).await {
                out.push(External {
                    name: name.clone(),
                    pid,
                    port,
                    program: program.clone(),
                    players: status.0,
                    max_players: status.1,
                    version: status.2,
                    dir: dir.clone(),
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.port.cmp(&b.port)));
    out
}

/// Java and Pumpkin processes: pid to (folder name, program, folder).
fn candidates() -> HashMap<u32, (String, String, Option<PathBuf>)> {
    let Ok(mut system) = System::new() else {
        return HashMap::new();
    };
    let kind = ProcessRefreshKind::nothing()
        .with_cmd(UpdateKind::OnlyIfNotSet)
        .with_exe(UpdateKind::OnlyIfNotSet)
        .with_cwd(UpdateKind::OnlyIfNotSet);
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, kind);
    let mut out = HashMap::new();
    for (pid, process) in system.processes() {
        let program = process.name().to_string_lossy().to_lowercase();
        let program = program.trim_end_matches(".exe");
        let args: Vec<String> = process
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().to_lowercase())
            .collect();
        let java_server = matches!(program, "java" | "javaw")
            && args.iter().any(|a| a.ends_with(".jar") || a == "nogui");
        let pumpkin = program.starts_with("pack-") || program == "pumpkin";
        if !java_server && !pumpkin {
            continue;
        }
        let dir = process.cwd().map(Path::to_path_buf);
        let name = dir
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(|| program.to_string(), |n| n.to_string_lossy().into_owned());
        out.insert(pid.as_u32(), (name, program.to_string(), dir));
    }
    out
}

/// Listening TCP ports by pid, from the OS socket table.
async fn listening_ports() -> HashMap<u32, Vec<u16>> {
    let mut out: HashMap<u32, Vec<u16>> = HashMap::new();
    if cfg!(windows) {
        let Some(text) = run("netstat", &["-ano", "-p", "TCP"]).await else {
            return out;
        };
        // "  TCP    0.0.0.0:25565    0.0.0.0:0    LISTENING    1234"
        for line in text.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() == 5
                && fields[3] == "LISTENING"
                && let (Some(port), Ok(pid)) = (port_of(fields[1]), fields[4].parse())
            {
                out.entry(pid).or_default().push(port);
            }
        }
    } else {
        let Some(text) = run("ss", &["-Hltnp"]).await else {
            return out;
        };
        // "LISTEN 0 4096 *:25565 *:* users:(("java",pid=1234,fd=45))"
        for line in text.lines() {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let Some(port) = fields.get(3).and_then(|a| port_of(a)) else {
                continue;
            };
            for part in line.split("pid=").skip(1) {
                if let Ok(pid) = part.split(',').next().unwrap_or_default().parse() {
                    out.entry(pid).or_default().push(port);
                }
            }
        }
    }
    for ports in out.values_mut() {
        ports.sort_unstable();
        ports.dedup();
    }
    out
}

/// A server folder found while its server isn't running.
pub struct Installed {
    pub name: String,
    pub dir: PathBuf,
    pub port: Option<u16>,
    pub program: Option<String>,
    pub version: Option<String>,
}

/// A folder an external server ran in, remembered in `~/.moddedmc/external.json`.
#[derive(Serialize, Deserialize, Clone, PartialEq)]
struct Seen {
    dir: PathBuf,
    program: String,
    version: Option<String>,
}

/// Remembered folders beyond this drop the oldest.
const MAX_SEEN: usize = 64;

fn seen_path() -> PathBuf {
    crate::config::home().join("external.json")
}

fn read_seen() -> Vec<Seen> {
    std::fs::read(seen_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Adds or updates the folders of running external servers; writes only on a change.
fn remember(seen: &mut Vec<Seen>, found: Vec<Seen>) {
    let mut changed = false;
    for entry in found {
        match seen.iter_mut().find(|s| s.dir == entry.dir) {
            Some(s) if *s == entry => {}
            Some(s) => {
                *s = entry;
                changed = true;
            }
            None => {
                seen.push(entry);
                changed = true;
            }
        }
    }
    if changed {
        let excess = seen.len().saturating_sub(MAX_SEEN);
        seen.drain(..excess);
        if let Ok(json) = serde_json::to_vec_pretty(&seen) {
            let _ = std::fs::write(seen_path(), json);
        }
    }
}

/// An absolute path without Windows' `\\?\` prefix, so paths from the OS and config compare.
fn plain(path: &Path) -> PathBuf {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    match path.to_str().and_then(|p| p.strip_prefix(r"\\?\")) {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// Stopped servers: folders external servers ran in before, and server folders directly inside
/// `find_in`, minus the agent's own servers and the ones running now. Remembers the folders of
/// the running ones (`running` excludes the agent's processes) for when they stop.
pub async fn installed(running: &[External], find_in: &[PathBuf], own: &Path) -> Vec<Installed> {
    let running: Vec<Seen> = running
        .iter()
        .filter_map(|e| {
            Some(Seen {
                dir: e.dir.clone()?,
                program: e.program.clone(),
                version: e.version.clone(),
            })
        })
        .collect();
    let (find_in, own) = (find_in.to_vec(), own.to_path_buf());
    tokio::task::spawn_blocking(move || {
        let own = plain(&own);
        let running: Vec<Seen> = running
            .into_iter()
            .map(|s| Seen {
                dir: plain(&s.dir),
                ..s
            })
            .collect();
        let live: HashSet<PathBuf> = running.iter().map(|s| s.dir.clone()).collect();
        let mut seen = read_seen();
        remember(
            &mut seen,
            running
                .into_iter()
                .filter(|s| !s.dir.starts_with(&own))
                .collect(),
        );
        let mut dirs: Vec<(PathBuf, Option<Seen>)> =
            seen.into_iter().map(|s| (s.dir.clone(), Some(s))).collect();
        for root in &find_in {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            dirs.extend(
                entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.join("server.properties").is_file())
                    .map(|p| (p, None)),
            );
        }
        let mut done = HashSet::new();
        let mut out = Vec::new();
        for (dir, seen) in dirs {
            let dir = plain(&dir);
            if !dir.is_dir()
                || dir.starts_with(&own)
                || live.contains(&dir)
                || !done.insert(dir.clone())
            {
                continue;
            }
            let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            out.push(Installed {
                name,
                port: server_port(&dir),
                version: seen
                    .as_ref()
                    .and_then(|s| s.version.clone())
                    .or_else(|| server_jar(&dir)),
                program: seen.map(|s| s.program),
                dir,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    })
    .await
    .unwrap_or_default()
}

/// `server-port` from `server.properties`; vanilla's default when the file leaves it out.
fn server_port(dir: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(dir.join("server.properties")).ok()?;
    Some(
        text.lines()
            .find_map(|l| l.strip_prefix("server-port="))
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(DEFAULT_GAME_PORT),
    )
}

const DEFAULT_GAME_PORT: u16 = 25565;

/// The server jar's name (`paper-1.21.4-120`), which says what it runs.
fn server_jar(dir: &Path) -> Option<String> {
    let mut jars: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".jar").map(str::to_owned)
        })
        .collect();
    jars.sort();
    jars.into_iter().next()
}

fn port_of(address: &str) -> Option<u16> {
    address.rsplit_once(':')?.1.parse().ok()
}

async fn run(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().await.ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The server-list status exchange on loopback: (players, max, version) when it answers.
async fn status(port: u16) -> Option<(Option<u32>, Option<u32>, Option<String>)> {
    let exchange = async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.ok()?;
        let host = b"127.0.0.1";
        let mut handshake = vec![0x00];
        varint(&mut handshake, -1);
        varint(&mut handshake, host.len() as i32);
        handshake.extend_from_slice(host);
        handshake.extend_from_slice(&port.to_be_bytes());
        varint(&mut handshake, 1);
        let mut out = Vec::new();
        varint(&mut out, handshake.len() as i32);
        out.extend_from_slice(&handshake);
        out.extend_from_slice(&[1, 0x00]);
        stream.write_all(&out).await.ok()?;

        let len = read_varint(&mut stream).await? as usize;
        if len > MAX_STATUS {
            return None;
        }
        let mut body = vec![0; len];
        stream.read_exact(&mut body).await.ok()?;
        let mut cursor = body.as_slice();
        if take_varint(&mut cursor)? != 0 {
            return None;
        }
        let json_len = take_varint(&mut cursor)? as usize;
        let value: serde_json::Value = serde_json::from_slice(cursor.get(..json_len)?).ok()?;
        let players = &value["players"];
        let number = |v: &serde_json::Value| v.as_u64().and_then(|n| u32::try_from(n).ok());
        Some((
            number(&players["online"]),
            number(&players["max"]),
            value["version"]["name"].as_str().map(str::to_owned),
        ))
    };
    timeout(PING_TIMEOUT, exchange).await.ok().flatten()
}

fn varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if value & !0x7f == 0 {
            out.push(value as u8);
            return;
        }
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
}

async fn read_varint(stream: &mut TcpStream) -> Option<i32> {
    let mut value = 0u32;
    for shift in 0..5 {
        let byte = stream.read_u8().await.ok()?;
        value |= u32::from(byte & 0x7f) << (7 * shift);
        if byte & 0x80 == 0 {
            return Some(value as i32);
        }
    }
    None
}

fn take_varint(cursor: &mut &[u8]) -> Option<i32> {
    let mut value = 0u32;
    for shift in 0..5 {
        let (&byte, rest) = cursor.split_first()?;
        *cursor = rest;
        value |= u32::from(byte & 0x7f) << (7 * shift);
        if byte & 0x80 == 0 {
            return Some(value as i32);
        }
    }
    None
}
