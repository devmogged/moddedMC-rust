//! Server folders under the servers root, and the processes the agent runs for them.

use std::{
    collections::{HashMap, VecDeque},
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
    process::{Child, ChildStdin, Command},
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
}

impl Managed {
    fn new() -> Self {
        Self {
            state: ServerState::Stopped,
            running: None,
            backlog: VecDeque::with_capacity(BACKLOG_LINES),
            lines: broadcast::channel(1024).0,
            exit_code: None,
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
    managed: Mutex<HashMap<String, Arc<Mutex<Managed>>>>,
}

#[derive(Serialize)]
pub struct ServerInfo {
    name: String,
    state: ServerState,
    pack: Option<String>,
    port: Option<u16>,
    pid: Option<u32>,
    uptime_secs: Option<u64>,
    exit_code: Option<i32>,
    health: Option<Health>,
    /// Running on this machine, but not started by the agent: shown, not controlled.
    external: bool,
    /// The external server's program (`java`, `pack-buja`).
    program: Option<String>,
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
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

async fn read_server_file(dir: &Path) -> Option<ServerFile> {
    let text = tokio::fs::read_to_string(dir.join("server.json"))
        .await
        .ok()?;
    serde_json::from_str(&text).ok()
}

impl Servers {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            managed: Mutex::new(HashMap::new()),
        }
    }

    async fn entry(&self, name: &str) -> Arc<Mutex<Managed>> {
        self.managed
            .lock()
            .await
            .entry(name.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(Managed::new())))
            .clone()
    }

    /// The managed entry for a server folder that exists, so unknown names make no entries.
    async fn existing(&self, name: &str) -> Option<Arc<Mutex<Managed>>> {
        read_server_file(&self.root.join(name)).await?;
        Some(self.entry(name).await)
    }

    async fn list(&self) -> Vec<ServerInfo> {
        let mut out = Vec::new();
        let Ok(mut dir) = tokio::fs::read_dir(&self.root).await else {
            return out;
        };
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !valid_name(&name) {
                continue;
            }
            let Some(file) = read_server_file(&entry.path()).await else {
                continue;
            };
            let managed = self.entry(&name).await;
            let managed = managed.lock().await;
            let health = if managed.state == ServerState::Running {
                read_health(&entry.path()).await
            } else {
                None
            };
            out.push(ServerInfo {
                name,
                state: managed.state,
                pack: file.pack,
                port: file.port,
                pid: managed.running.as_ref().map(|r| r.pid),
                uptime_secs: managed
                    .running
                    .as_ref()
                    .map(|r| r.started.elapsed().as_secs()),
                exit_code: managed.exit_code,
                health,
                external: false,
                program: None,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));

        let ours: std::collections::HashSet<u32> = out.iter().filter_map(|s| s.pid).collect();
        let mut names: std::collections::HashSet<String> =
            out.iter().map(|s| s.name.clone()).collect();
        for found in crate::external::scan(&ours).await {
            // Names key the app's rows, so a clash with a managed server gets the port added.
            let name = if names.contains(&found.name) {
                format!("{}:{}", found.name, found.port)
            } else {
                found.name
            };
            names.insert(name.clone());
            out.push(ServerInfo {
                name,
                state: ServerState::Running,
                // The version the server reports ("Paper 1.21.4") stands in for a pack.
                pack: found.version,
                port: Some(found.port),
                pid: Some(found.pid),
                uptime_secs: None,
                exit_code: None,
                health: None,
                external: true,
                program: Some(found.program),
            });
        }
        out
    }

    async fn start(&self, name: &str) -> Result<(), (StatusCode, String)> {
        let dir = self.root.join(name);
        let file = read_server_file(&dir)
            .await
            .ok_or((StatusCode::NOT_FOUND, format!("no server named {name}")))?;
        let managed = self.entry(name).await;
        let mut guard = managed.lock().await;
        if guard.running.is_some() {
            return Err((StatusCode::CONFLICT, format!("{name} is already running")));
        }
        if let Some(port) = file.port {
            crate::pumpkin_config::apply_port(&dir, port).await;
        }
        // Health from the last run would look live until it goes stale.
        let _ = tokio::fs::remove_file(dir.join("stats.json")).await;
        let exe = if file.exe.is_absolute() {
            file.exe.clone()
        } else {
            dir.join(&file.exe)
        };
        let mut child: Child = Command::new(&exe)
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
        drop(guard);

        for stream in [
            Box::new(stdout) as Box<dyn tokio::io::AsyncRead + Send + Unpin>,
            Box::new(stderr),
        ] {
            let managed = managed.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut managed = managed.lock().await;
                    // Pumpkin logs this once it accepts players.
                    if managed.state == ServerState::Starting && line.contains("Started server") {
                        managed.state = ServerState::Running;
                    }
                    managed.push_line(line);
                }
            });
        }
        let name = name.to_owned();
        tokio::spawn(async move {
            let status = child.wait().await;
            let mut managed = managed.lock().await;
            let code = status.ok().and_then(|s| s.code());
            let clean = managed.state == ServerState::Stopping || code == Some(0);
            managed.state = if clean {
                ServerState::Stopped
            } else {
                ServerState::Crashed
            };
            managed.running = None;
            managed.exit_code = code;
            managed.push_line(format!("[agent] {name} exited with {code:?}"));
            info!("{name} exited with {code:?}");
        });
        Ok(())
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
        let mut file = read_server_file(&dir)
            .await
            .ok_or((StatusCode::NOT_FOUND, format!("no server named {name}")))?;
        let managed = self.entry(name).await;
        // Held to the end, so nobody starts the server halfway through.
        let guard = managed.lock().await;
        if guard.running.is_some() {
            return Err((StatusCode::CONFLICT, format!("stop {name} first")));
        }
        let internal = |e: std::io::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
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
        let json = serde_json::to_string_pretty(&file)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        tokio::fs::write(dir.join("server.json"), json)
            .await
            .map_err(internal)?;
        drop(guard);
        info!("{name} now runs {}", build.display());
        Ok(())
    }
}

fn error(status: StatusCode, message: String) -> Response {
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
    let dir = app.servers.root.join(&body.name);
    if tokio::fs::try_exists(dir.join("server.json"))
        .await
        .unwrap_or(false)
    {
        return error(
            StatusCode::CONFLICT,
            format!("{} already exists", body.name),
        );
    }
    let json = match serde_json::to_string_pretty(&body.file) {
        Ok(json) => json,
        Err(e) => return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    if let Err(e) = tokio::fs::write(dir.join("server.json"), json).await {
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
    StatusCode::CREATED.into_response()
}

fn bad_name(name: &str) -> Option<Response> {
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
        Ok(()) => StatusCode::ACCEPTED.into_response(),
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
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err((status, message)) => error(status, message),
    }
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
