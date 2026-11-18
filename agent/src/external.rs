//! Minecraft servers on this machine that the agent didn't start: a Java or Pumpkin process
//! listening on a TCP port that answers the game's status ping. Read-only; the agent can't
//! control them, only show them.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::LazyLock,
    time::{Duration, Instant},
};

use serde::Serialize;
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
    for (pid, (name, program)) in candidates {
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
                });
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name).then(a.port.cmp(&b.port)));
    out
}

/// Java and Pumpkin processes: pid to (folder name, program).
fn candidates() -> HashMap<u32, (String, String)> {
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
        let name = process
            .cwd()
            .and_then(Path::file_name)
            .map_or_else(|| program.to_string(), |n| n.to_string_lossy().into_owned());
        out.insert(pid.as_u32(), (name, program.to_string()));
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
