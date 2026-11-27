//! Server folders under the servers root, and the processes the agent runs for them.

use std::{
    collections::{HashMap, VecDeque},
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{
        Path as UrlPath, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::{Child, ChildStdin},
    sync::{Mutex, broadcast},
};
use tracing::{info, warn};

use crate::App;

/// Log lines a console client gets when it connects.
const BACKLOG_LINES: usize = 500;
/// How long `stop` waits for the server to exit after sending `stop` before killing it.
const STOP_TIMEOUT: Duration = Duration::from_secs(60);

/// Console clients get plain text; Pumpkin colors its log even when piped.
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        // CSI: ESC [ parameters... final byte in @..~
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
}

/// `<server>/server.json`.
#[derive(Serialize, Deserialize, Clone)]
pub struct ServerFile {
    /// The server binary, absolute or relative to the server folder.
    pub exe: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub pack: Option<String>,
    #[serde(default)]
    pub port: Option<u16>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ServerState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Crashed,
}

struct Running {
    pid: u32,
    stdin: ChildStdin,
    started: Instant,
}

struct Managed {
    state: ServerState,
    running: Option<Running>,
    backlog: VecDeque<String>,
    lines: broadcast::Sender<String>,
    exit_code: Option<i32>,
    start_error: Option<String>,
}

impl Managed {
    fn new() -> Self {
        Self {
            state: ServerState::Stopped,
            running: None,
            backlog: VecDeque::with_capacity(BACKLOG_LINES),
            lines: broadcast::channel(1024).0,
            exit_code: None,
            start_error: None,
        }
    }

    fn push_line(&mut self, line: String) {
        let line = strip_ansi(&line);
        if self.backlog.len() == BACKLOG_LINES {
            self.backlog.pop_front();
        }
        self.backlog.push_back(line.clone());
        // No receivers is fine: nobody is watching the console.
        let _ = self.lines.send(line);
    }
}

pub struct Servers {
    root: PathBuf,
    find_in: Vec<PathBuf>,
    managed: Mutex<HashMap<String, Arc<Mutex<Managed>>>>,
    java_ports: Arc<Mutex<HashMap<u16, String>>>,
}

#[derive(Serialize)]
pub struct ServerInfo {
    pub name: String,
    state: ServerState,
    pack: Option<String>,
    pub port: Option<u16>,
    pid: Option<u32>,
    uptime_secs: Option<u64>,
    exit_code: Option<i32>,
    start_error: Option<String>,
    health: Option<Health>,
    /// Running on this machine, but not started by the agent: shown, not controlled.
    external: bool,
    /// The external server's program (`java`, `pack-buja`).
    program: Option<String>,
    /// The external server's folder.
    folder: Option<String>,
}

/// Tick health and players, from the `stats.json` the pack server writes every two seconds.
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Health {
    mspt: f64,
    mspt_max: f64,
    tps: f64,
    tickrate: f64,
    players: u32,
    max_players: u32,
}

/// Older than this, `stats.json` is from a server that stopped writing it.
const HEALTH_STALE_SECS: u64 = 6;

async fn read_health(dir: &Path) -> Option<Health> {
    #[derive(Deserialize)]
    struct File {
        at: u64,
        #[serde(flatten)]
        health: Health,
    }
    let text = tokio::fs::read_to_string(dir.join("stats.json"))
        .await
        .ok()?;
    let file: File = serde_json::from_str(&text).ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    (now.saturating_sub(file.at) <= HEALTH_STALE_SECS).then_some(file.health)
}

/// A folder name is a server name: letters, digits, `-`, `_`, `.`, no path tricks.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && (!cfg!(windows) || !name.ends_with('.'))
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

pub fn server_key(name: &str) -> String {
    if cfg!(windows) {
        name.to_ascii_lowercase()
    } else {
        name.to_owned()
    }
}

type ServerError = (StatusCode, String);

fn internal(err: impl std::fmt::Display) -> ServerError {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

fn missing(name: &str) -> ServerError {
    (StatusCode::NOT_FOUND, format!("no server named {name}"))
}

fn blocked_port(port: u16, owner: Option<&str>) -> String {
    let reason = owner.map_or_else(
        || "is already in use".to_string(),
        |name| format!("is in use by {name}"),
    );
    format!(
        "Server cannot start. Java port {port} {reason}. Choose another port in Settings or free this port."
    )
}

async fn check_port(address: SocketAddr) -> Result<(), ServerError> {
    let port = address.port();
    // Winsock can bind a wildcard even with a listener on one of its interfaces.
    #[cfg(windows)]
    if crate::external::listening_ports()
        .await
        .values()
        .any(|ports| ports.contains(&port))
    {
        return Err((StatusCode::CONFLICT, blocked_port(port, None)));
    }
    TcpListener::bind(address).await.map(drop).map_err(|err| {
        if err.kind() == io::ErrorKind::AddrInUse {
            (StatusCode::CONFLICT, blocked_port(port, None))
        } else {
            internal(format!("Cannot use Java port {port}: {err}"))
        }
    })
}

async fn write_server_file(dir: &Path, file: &ServerFile) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(file).map_err(io::Error::other)?;
    let tmp = dir.join(".server.json.tmp");
    tokio::fs::write(&tmp, json).await?;
    tokio::fs::rename(tmp, dir.join("server.json")).await
}

async fn read_server_file(dir: &Path) -> Option<ServerFile> {
    let text = tokio::fs::read_to_string(dir.join("server.json"))
        .await
        .ok()?;
    serde_json::from_str(&text).ok()
}

impl Servers {
    pub fn new(root: PathBuf, find_in: Vec<PathBuf>) -> Self {
        Self {
            root,
            find_in,
            managed: Mutex::new(HashMap::new()),
            java_ports: Arc::default(),
        }
    }

    async fn entry(&self, name: &str) -> Arc<Mutex<Managed>> {
        self.managed
            .lock()
            .await
            .entry(server_key(name))
            .or_insert_with(|| Arc::new(Mutex::new(Managed::new())))
            .clone()
    }

    /// A managed server's folder, when there is one by that name.
    pub async fn folder(&self, name: &str) -> Option<PathBuf> {
        let dir = self.root.join(name);
        read_server_file(&dir).await.map(|_| dir)
    }

    /// Runs `change` on a stopped server's folder, holding the server so it can't start halfway
    /// through.
    pub async fn with_stopped(
        &self,
        name: &str,
        change: impl AsyncFnOnce(&Path) -> std::io::Result<()>,
    ) -> Result<(), (StatusCode, String)> {
        self.change_folder(name, true, change).await
    }

    pub async fn with_folder(
        &self,
        name: &str,
        change: impl AsyncFnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), ServerError> {
        self.change_folder(name, false, change).await
    }

    async fn change_folder(
        &self,
        name: &str,
        stopped: bool,
        change: impl AsyncFnOnce(&Path) -> io::Result<()>,
    ) -> Result<(), ServerError> {
        let managed = self.entry(name).await;
        let guard = managed.lock().await;
        let dir = self.root.join(name);
        read_server_file(&dir).await.ok_or_else(|| missing(name))?;
        if stopped && guard.running.is_some() {
            return Err((StatusCode::CONFLICT, format!("Stop {name} first.")));
        }
        change(&dir).await.map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                (StatusCode::NOT_FOUND, err.to_string())
            } else {
                internal(err)
            }
        })
    }

    async fn set_port(&self, name: &str, port: u16) -> Result<(), ServerError> {
        let managed = self.entry(name).await;
        let mut guard = managed.lock().await;
        let dir = self.root.join(name);
        let mut file = read_server_file(&dir).await.ok_or_else(|| missing(name))?;
        if guard.running.is_some() {
            return Err((
                StatusCode::CONFLICT,
                format!("Stop {name} before changing its port."),
            ));
        }
        file.port = Some(port);
        write_server_file(&dir, &file).await.map_err(internal)?;
        guard.start_error = None;
        Ok(())
    }

    async fn delete(&self, name: &str) -> Result<(), ServerError> {
        let managed = self.entry(name).await;
        let mut guard = managed.lock().await;
        let dir = self.root.join(name);
        let metadata = tokio::fs::symlink_metadata(&dir).await.map_err(|err| {
            if err.kind() == io::ErrorKind::NotFound {
                missing(name)
            } else {
                internal(err)
            }
        })?;
        #[cfg(windows)]
        let is_redirect = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
        };
        #[cfg(not(windows))]
        let is_redirect = metadata.is_symlink();
        if !metadata.is_dir() || is_redirect {
            return Err((
                StatusCode::CONFLICT,
                "The server folder must be a local directory.".into(),
            ));
        }
        let file = read_server_file(&dir).await.ok_or_else(|| missing(name))?;
        if guard.running.is_some() {
            return Err((
                StatusCode::CONFLICT,
                format!("Stop {name} before deleting it."),
            ));
        }
        let _backup = crate::backups::Busy::claim(name)?;
        crate::backups::purge_server(name).await?;
        if let Err(err) = tokio::fs::remove_dir_all(&dir).await {
            // Keep a partially removed server visible so deletion can be retried.
            if matches!(
                tokio::fs::try_exists(dir.join("server.json")).await,
                Ok(false)
            ) {
                let _ = write_server_file(&dir, &file).await;
            }
            return Err(internal(format!("Could not remove {name}: {err}")));
        }
        *guard = Managed::new();
        info!("deleted {name}");
        Ok(())
    }

    /// The console lines the agent keeps for a server, oldest first.
    pub async fn backlog(&self, name: &str) -> Option<Vec<String>> {
        let managed = self.existing(name).await?;
        let guard = managed.lock().await;
        Some(guard.backlog.iter().cloned().collect())
    }

    pub async fn is_running(&self, name: &str) -> bool {
        match self.existing(name).await {
            Some(managed) => managed.lock().await.running.is_some(),
            None => false,
        }
    }

    /// Sends a console command to a running server.
    pub async fn command(&self, name: &str, line: &str) -> Result<(), (StatusCode, String)> {
        self.command_inner(name, line).await.map(drop)
    }

    /// Sends a console command and waits for a log line containing `expect`.
    pub async fn command_until(
        &self,
        name: &str,
        line: &str,
        expect: &str,
        wait: std::time::Duration,
    ) -> Result<(), (StatusCode, String)> {
        let mut lines = self.command_inner(name, line).await?;
        let found = async {
            loop {
                match lines.recv().await {
                    Ok(l) if l.contains(expect) => return true,
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return false,
                }
            }
        };
        match tokio::time::timeout(wait, found).await {
            Ok(true) => Ok(()),
            _ => Err((
                StatusCode::GATEWAY_TIMEOUT,
                format!("{name} didn't answer {line:?} in time"),
            )),
        }
    }

    /// Subscribes to the log, then writes the command, so the answer can't be missed.
    async fn command_inner(
        &self,
        name: &str,
        line: &str,
    ) -> Result<broadcast::Receiver<String>, (StatusCode, String)> {
        let managed = self
            .existing(name)
            .await
            .ok_or((StatusCode::NOT_FOUND, format!("no server named {name}")))?;
        let mut guard = managed.lock().await;
        let lines = guard.lines.subscribe();
        let running = guard
            .running
            .as_mut()
            .ok_or((StatusCode::CONFLICT, format!("{name} isn't running")))?;
        running
            .stdin
            .write_all(format!("{line}\n").as_bytes())
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(lines)
    }

    /// The managed entry for a server folder that exists, so unknown names make no entries.
    async fn existing(&self, name: &str) -> Option<Arc<Mutex<Managed>>> {
        read_server_file(&self.root.join(name)).await?;
        Some(self.entry(name).await)
    }

    pub async fn list(&self) -> Vec<ServerInfo> {
        let mut out = Vec::new();
        let Ok(mut dir) = tokio::fs::read_dir(&self.root).await else {
            return out;
        };
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !valid_name(&name) {
                continue;
            }
            let managed = self.entry(&name).await;
            let managed = managed.lock().await;
            let Some(file) = read_server_file(&entry.path()).await else {
                continue;
            };
            let port = if file.port.is_some() {
                file.port
            } else {
                crate::pumpkin_config::read(&entry.path())
                    .await
                    .ok()
                    .and_then(|table| crate::pumpkin_config::java_address(&table).ok())
                    .map(|address| address.port())
            };
            let health = if managed.state == ServerState::Running {
                read_health(&entry.path()).await
            } else {
                None
            };
            out.push(ServerInfo {
                name,
                state: managed.state,
                pack: file.pack,
                port,
                pid: managed.running.as_ref().map(|r| r.pid),
                uptime_secs: managed
                    .running
                    .as_ref()
                    .map(|r| r.started.elapsed().as_secs()),
                exit_code: managed.exit_code,
                start_error: managed.start_error.clone(),
                health,
                external: false,
                program: None,
                folder: None,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));

        let ours: std::collections::HashSet<u32> = out.iter().filter_map(|s| s.pid).collect();
        let mut names: std::collections::HashSet<String> =
            out.iter().map(|s| s.name.clone()).collect();
        let running = crate::external::scan(&ours).await;
        let installed = crate::external::installed(&running, &self.find_in, &self.root).await;
        // Names key the app's rows, so a clash gets the port (or a number) added.
        let mut unique = |name: String, port: Option<u16>| {
            let mut out = name.clone();
            let mut n = 1;
            while names.contains(&out) {
                n += 1;
                out = match port {
                    Some(port) if n == 2 => format!("{name}:{port}"),
                    _ => format!("{name}-{n}"),
                };
            }
            names.insert(out.clone());
            out
        };
        for found in running {
            let name = unique(found.name, Some(found.port));
            out.push(ServerInfo {
                name,
                state: ServerState::Running,
                // The version the server reports ("Paper 1.21.4") stands in for a pack.
                pack: found.version,
                port: Some(found.port),
                pid: Some(found.pid),
                uptime_secs: None,
                exit_code: None,
                start_error: None,
                health: None,
                external: true,
                program: Some(found.program),
                folder: found.dir.map(|d| d.display().to_string()),
            });
        }
        for found in installed {
            out.push(ServerInfo {
                name: unique(found.name, found.port),
                state: ServerState::Stopped,
                pack: found.version,
                port: found.port,
                pid: None,
                uptime_secs: None,
                exit_code: None,
                start_error: None,
                health: None,
                external: true,
                program: found.program,
                folder: Some(found.dir.display().to_string()),
            });
        }
        out
    }

    async fn start(&self, name: &str) -> Result<Option<u16>, ServerError> {
        let dir = self.root.join(name);
        let managed = self.entry(name).await;
        let mut guard = managed.lock().await;
        let file = read_server_file(&dir).await.ok_or_else(|| missing(name))?;
        if guard.running.is_some() {
            return Err((StatusCode::CONFLICT, format!("{name} is already running")));
        }
        guard.start_error = None;
        let address = crate::pumpkin_config::prepare_java(&dir, file.port)
            .await
            .map_err(internal)?;
        let mut ports = self.java_ports.lock().await;
        if let Some(address) = address {
            let port = address.port();
            let available = if let Some(owner) = ports.get(&port) {
                Err((StatusCode::CONFLICT, blocked_port(port, Some(owner))))
            } else {
                check_port(address).await
            };
            if let Err((status, message)) = available {
                guard.state = ServerState::Stopped;
                guard.exit_code = None;
                guard.start_error = Some(message.clone());
                return Err((status, message));
            }
        }
        // Health from the last run would look live until it goes stale.
        let _ = tokio::fs::remove_file(dir.join("stats.json")).await;
        let exe = if file.exe.is_absolute() {
            file.exe.clone()
        } else {
            dir.join(&file.exe)
        };
        let mut child: Child = crate::command(&exe)
            .args(&file.args)
            .current_dir(&dir)
            .env("MODDEDMC_STATS", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("cannot start {}: {e}", exe.display()),
                )
            })?;
        let (Some(stdin), Some(stdout), Some(stderr), Some(pid)) = (
            child.stdin.take(),
            child.stdout.take(),
            child.stderr.take(),
            child.id(),
        ) else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "child has no pipes".into(),
            ));
        };
        info!("started {name} (pid {pid})");
        guard.state = ServerState::Starting;
        guard.exit_code = None;
        guard.running = Some(Running {
            pid,
            stdin,
            started: Instant::now(),
        });
        let port = address.map(|address| address.port());
        if let Some(port) = port {
            ports.insert(port, name.to_owned());
        }
        drop(ports);
        drop(guard);

        let readers = [
            Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Send + Unpin>,
            Box::new(stderr),
        ]
        .map(|stream| {
            let managed = managed.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut managed = managed.lock().await;
                    // Pumpkin logs this once it accepts players.
                    if managed.state == ServerState::Starting {
                        if line.contains("Started server") {
                            managed.state = ServerState::Running;
                        } else if let Some(port) = port
                            && line.contains(
                                "Make sure another instance of the server isn't already running",
                            )
                        {
                            managed.start_error = Some(blocked_port(port, None));
                        }
                    }
                    managed.push_line(line);
                }
            })
        });
        let ports = self.java_ports.clone();
        let name = name.to_owned();
        tokio::spawn(async move {
            let status = child.wait().await;
            // The bind error may still be in a pipe when the process exits.
            for reader in readers {
                let _ = reader.await;
            }
            let mut managed = managed.lock().await;
            let code = status.ok().and_then(|s| s.code());
            if let Some(port) = port {
                ports.lock().await.remove(&port);
            }
            let clean = managed.state == ServerState::Stopping || code == Some(0);
            let blocked = managed.start_error.is_some();
            managed.state = if clean || blocked {
                ServerState::Stopped
            } else {
                ServerState::Crashed
            };
            managed.running = None;
            managed.exit_code = if blocked { None } else { code };
            managed.push_line(format!("[agent] {name} exited with {code:?}"));
            info!("{name} exited with {code:?}");
        });
        Ok(port)
    }

    async fn stop(&self, name: &str) -> Result<(), (StatusCode, String)> {
        let managed = self.entry(name).await;
        {
            let mut guard = managed.lock().await;
            let Some(running) = guard.running.as_mut() else {
                return Err((StatusCode::CONFLICT, format!("{name} is not running")));
            };
            if running.stdin.write_all(b"stop\n").await.is_err() {
                warn!("{name}: console closed; waiting for exit");
            }
            guard.state = ServerState::Stopping;
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        loop {
            let pid = managed.lock().await.running.as_ref().map(|r| r.pid);
            let Some(pid) = pid else {
                return Ok(());
            };
            if Instant::now() >= deadline {
                warn!("{name} did not stop in {STOP_TIMEOUT:?}; killing pid {pid}");
                let pid = Pid::from_u32(pid);
                if let Ok(mut system) = System::new() {
                    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
                    if let Some(process) = system.process(pid) {
                        process.kill();
                    }
                }
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub async fn stop_all(&self) {
        let names: Vec<String> = self.managed.lock().await.keys().cloned().collect();
        for name in names {
            let _ = self.stop(&name).await;
        }
    }

    /// Points a stopped server at a build and replaces its drop-in mods with the build's.
    async fn deploy(
        &self,
        name: &str,
        build: &Path,
        exe: PathBuf,
        pack: String,
    ) -> Result<(), (StatusCode, String)> {
        let dir = self.root.join(name);
        let managed = self.entry(name).await;
        // Held to the end, so nobody starts the server halfway through.
        let guard = managed.lock().await;
        let mut file = read_server_file(&dir).await.ok_or_else(|| missing(name))?;
        if guard.running.is_some() {
            return Err((StatusCode::CONFLICT, format!("stop {name} first")));
        }
        let mods = dir.join("mods");
        tokio::fs::create_dir_all(&mods).await.map_err(internal)?;
        let mut old = tokio::fs::read_dir(&mods).await.map_err(internal)?;
        while let Some(entry) = old.next_entry().await.map_err(internal)? {
            if entry.path().extension().is_some_and(|e| e == "mmc") {
                tokio::fs::remove_file(entry.path())
                    .await
                    .map_err(internal)?;
            }
        }
        if let Ok(mut new) = tokio::fs::read_dir(build.join("mods")).await {
            while let Some(entry) = new.next_entry().await.map_err(internal)? {
                if entry.path().extension().is_some_and(|e| e == "mmc") {
                    tokio::fs::copy(entry.path(), mods.join(entry.file_name()))
                        .await
                        .map_err(internal)?;
                }
            }
        }
        file.exe = exe;
        file.pack = Some(pack);
        write_server_file(&dir, &file).await.map_err(internal)?;
        drop(guard);
        info!("{name} now runs {}", build.display());
        Ok(())
    }
}

pub fn error(status: StatusCode, message: String) -> Response {
    (status, message).into_response()
}

#[derive(Deserialize)]
pub struct Deploy {
    build: String,
}

pub async fn deploy(
    State(app): State<Arc<App>>,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<Deploy>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if !valid_name(&body.build) {
        return error(
            StatusCode::BAD_REQUEST,
            format!("bad build id {:?}", body.build),
        );
    }
    let build = crate::builds::root().join(&body.build);
    let Some((pack, version, exe)) = crate::builds::describe(&build).await else {
        return error(
            StatusCode::NOT_FOUND,
            format!("no build named {}", body.build),
        );
    };
    match app
        .servers
        .deploy(&name, &build, exe, format!("{pack} {version}"))
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub async fn list(State(app): State<Arc<App>>) -> Json<Vec<ServerInfo>> {
    Json(app.servers.list().await)
}

#[derive(Deserialize)]
pub struct CreateServer {
    name: String,
    #[serde(flatten)]
    file: ServerFile,
}

pub async fn create(State(app): State<Arc<App>>, Json(body): Json<CreateServer>) -> Response {
    if !valid_name(&body.name) {
        return error(
            StatusCode::BAD_REQUEST,
            format!("bad server name {:?}", body.name),
        );
    }
    if body.file.port == Some(0) {
        return error(
            StatusCode::BAD_REQUEST,
            "Choose a port from 1 to 65535.".into(),
        );
    }
    let managed = app.servers.entry(&body.name).await;
    let _guard = managed.lock().await;
    let dir = app.servers.root.join(&body.name);
    match tokio::fs::try_exists(dir.join("server.json")).await {
        Ok(true) => {
            return error(
                StatusCode::CONFLICT,
                format!("{} already exists", body.name),
            );
        }
        Ok(false) => {}
        Err(err) => return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    // A folder that already has a config (an imported server) keeps it.
    if !tokio::fs::try_exists(dir.join("pumpkin.toml"))
        .await
        .unwrap_or(true)
        && let Err(e) = crate::pumpkin_config::seed(&dir, body.file.port).await
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    if let Err(err) = write_server_file(&dir, &body.file).await {
        return error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string());
    }
    StatusCode::CREATED.into_response()
}

#[derive(Deserialize)]
pub struct PortSetting {
    port: u16,
}

pub async fn set_port(
    State(app): State<Arc<App>>,
    UrlPath(name): UrlPath<String>,
    Json(body): Json<PortSetting>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if body.port == 0 {
        return error(
            StatusCode::BAD_REQUEST,
            "Choose a port from 1 to 65535.".into(),
        );
    }
    match app.servers.set_port(&name, body.port).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub async fn delete(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let result = tokio::spawn(async move { app.servers.delete(&name).await })
        .await
        .map_err(internal)
        .and_then(|result| result);
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub fn bad_name(name: &str) -> Option<Response> {
    if valid_name(name) {
        None
    } else {
        Some(error(
            StatusCode::BAD_REQUEST,
            format!("bad server name {name:?}"),
        ))
    }
}

pub async fn start(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    match app.servers.start(&name).await {
        Ok(port) => started(port),
        Err((status, message)) => error(status, message),
    }
}

pub async fn stop(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    match app.servers.stop(&name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub async fn restart(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    // Not running is fine for a restart.
    let _ = app.servers.stop(&name).await;
    match app.servers.start(&name).await {
        Ok(port) => started(port),
        Err((status, message)) => error(status, message),
    }
}

fn started(port: Option<u16>) -> Response {
    // Friends joining through playit need it running too.
    if let Some(port) = port {
        tokio::spawn(crate::playit::ensure_for(port));
    }
    StatusCode::ACCEPTED.into_response()
}

#[derive(Serialize)]
pub struct Stats {
    state: ServerState,
    rss_bytes: Option<u64>,
    cpu_percent: Option<f32>,
    uptime_secs: Option<u64>,
    health: Option<Health>,
}

pub async fn stats(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let Some(managed) = app.servers.existing(&name).await else {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    };
    let (state, pid, uptime) = {
        let managed = managed.lock().await;
        (
            managed.state,
            managed.running.as_ref().map(|r| r.pid),
            managed
                .running
                .as_ref()
                .map(|r| r.started.elapsed().as_secs()),
        )
    };
    let (rss_bytes, cpu_percent) = match pid {
        Some(pid) => 'sample: {
            let pid = Pid::from_u32(pid);
            let Ok(mut system) = System::new() else {
                break 'sample (None, None);
            };
            let kind = ProcessRefreshKind::nothing().with_memory().with_cpu();
            // CPU use needs two samples.
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL).await;
            system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), true, kind);
            system
                .process(pid)
                .map_or((None, None), |p| (Some(p.memory()), Some(p.cpu_usage())))
        }
        None => (None, None),
    };
    let health = if state == ServerState::Running {
        read_health(&app.servers.root.join(&name)).await
    } else {
        None
    };
    Json(Stats {
        state,
        rss_bytes,
        cpu_percent,
        uptime_secs: uptime,
        health,
    })
    .into_response()
}

pub async fn console(
    State(app): State<Arc<App>>,
    UrlPath(name): UrlPath<String>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let Some(managed) = app.servers.existing(&name).await else {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    };
    upgrade.on_upgrade(move |socket| console_session(socket, managed))
}

async fn console_session(mut socket: WebSocket, managed: Arc<Mutex<Managed>>) {
    let (backlog, mut lines) = {
        let managed = managed.lock().await;
        (managed.backlog.clone(), managed.lines.subscribe())
    };
    for line in backlog {
        if socket.send(Message::Text(line.into())).await.is_err() {
            return;
        }
    }
    loop {
        tokio::select! {
            line = lines.recv() => match line {
                Ok(line) => {
                    if socket.send(Message::Text(line.into())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    let note = format!("[agent] skipped {skipped} lines");
                    if socket.send(Message::Text(note.into())).await.is_err() {
                        return;
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return,
            },
            message = socket.recv() => match message {
                Some(Ok(Message::Text(command))) => {
                    let mut managed = managed.lock().await;
                    if let Some(running) = managed.running.as_mut() {
                        let line = format!("{}\n", command.trim_end());
                        let _ = running.stdin.write_all(line.as_bytes()).await;
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn busy_java_port_blocks_a_stopped_server_and_can_be_changed() {
        let root = std::env::temp_dir().join(format!("agent-server-port-{}", std::process::id()));
        let dir = root.join("test");
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let file = ServerFile {
            exe: "missing-server".into(),
            args: Vec::new(),
            pack: None,
            port: Some(port),
        };
        write_server_file(&dir, &file).await.unwrap();
        let servers = Servers::new(root.clone(), Vec::new());

        let (status, message) = servers.start("test").await.unwrap_err();
        assert_eq!(status, StatusCode::CONFLICT, "{message}");
        assert!(message.contains(&format!("Java port {port} is already in use")));
        let managed = servers.entry("test").await;
        {
            let guard = managed.lock().await;
            assert!(guard.state == ServerState::Stopped && guard.running.is_none());
            assert_eq!(guard.start_error.as_deref(), Some(message.as_str()));
        }

        drop(listener);
        let new_port = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        servers.set_port("test", new_port).await.unwrap();
        assert_eq!(read_server_file(&dir).await.unwrap().port, Some(new_port));
        assert!(managed.lock().await.start_error.is_none());
        assert_eq!(
            servers.start("test").await.unwrap_err().0,
            StatusCode::INTERNAL_SERVER_ERROR
        );
        let table = crate::pumpkin_config::read(&dir).await.unwrap();
        assert_eq!(
            crate::pumpkin_config::java_address(&table).unwrap().port(),
            new_port
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
