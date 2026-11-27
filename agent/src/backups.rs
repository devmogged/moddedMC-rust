//! Snapshots of a server folder (docs/DEVICES.md, "Backups"). Files are cut into
//! content-defined chunks (FastCDC), and each chunk is stored once under its SHA-256, compressed
//! with zstd, in a store all servers share. A snapshot is a manifest of chunk lists, so it costs
//! only the chunks that no earlier snapshot has.

use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::{
    App,
    servers::{bad_name, error, server_key, valid_name},
};

/// Chunk sizes: small enough that a changed region of a world file costs little, large enough
/// that a big world doesn't turn into millions of blobs.
const MIN_CHUNK: usize = 16 << 10;
const AVG_CHUNK: usize = 64 << 10;
const MAX_CHUNK: usize = 256 << 10;
const ZSTD_LEVEL: i32 = 3;
/// How long a running server gets to finish `save-all flush`.
const SAVE_TIMEOUT: Duration = Duration::from_secs(120);
/// How often the agent checks whether a scheduled backup is due.
const SCHEDULE_TICK: Duration = Duration::from_secs(60);
const MAX_EVERY_MINUTES: u32 = 7 * 24 * 60;
const MAX_KEEP: u32 = 1000;

/// Taking and restoring snapshots read and add blobs; garbage collection removes the ones no
/// manifest names yet, so it must not run while a snapshot is half written.
static STORE: RwLock<()> = RwLock::const_new(());
/// Servers with a backup or restore in progress.
static BUSY: std::sync::Mutex<BTreeSet<String>> = std::sync::Mutex::new(BTreeSet::new());

pub struct Busy(String);

impl Busy {
    pub fn claim(server: &str) -> Result<Self, (StatusCode, String)> {
        let mut busy = BUSY
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = server_key(server);
        if busy.insert(key.clone()) {
            Ok(Self(key))
        } else {
            Err((
                StatusCode::CONFLICT,
                format!("a backup of {server} is already running"),
            ))
        }
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.0);
    }
}

/// Not part of a server's state: logs, live stats, lock and temporary files.
fn skipped(relative: &Path) -> bool {
    let mut parts = relative.components();
    let first = parts.next().map(|c| c.as_os_str().to_string_lossy());
    let name = relative.file_name().map(|n| n.to_string_lossy());
    matches!(first.as_deref(), Some("logs"))
        || matches!(name.as_deref(), Some("stats.json" | "session.lock"))
        || name.is_some_and(|n| n.starts_with('.') || n.ends_with(".partial"))
}

fn root() -> PathBuf {
    crate::config::home().join("backups")
}

fn blob_path(hash: &str) -> PathBuf {
    root()
        .join("blobs")
        .join(&hash[..2])
        .join(format!("{hash}.zst"))
}

fn snapshots_dir(server: &str) -> PathBuf {
    root().join("snapshots").join(server)
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    created: u64,
    files: Vec<FileEntry>,
    /// Size of the backed-up files.
    bytes: u64,
    /// What this snapshot added to the store, compressed.
    stored: u64,
    /// Taken by the schedule, so retention may remove it.
    #[serde(default)]
    scheduled: bool,
}

#[derive(Serialize, Deserialize)]
struct FileEntry {
    /// Relative to the server folder, `/`-separated.
    path: String,
    size: u64,
    chunks: Vec<String>,
}

#[derive(Serialize)]
pub struct Snapshot {
    id: String,
    created: u64,
    files: usize,
    bytes: u64,
    stored: u64,
    scheduled: bool,
}

impl Snapshot {
    fn new(id: String, m: &Manifest) -> Self {
        Self {
            id,
            created: m.created,
            files: m.files.len(),
            bytes: m.bytes,
            stored: m.stored,
            scheduled: m.scheduled,
        }
    }
}

/// When a server is backed up on its own, and how many of those backups stay.
#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct Schedule {
    every_minutes: u32,
    keep: u32,
}

fn schedules_path() -> PathBuf {
    root().join("schedules.json")
}

fn read_schedules() -> BTreeMap<String, Schedule> {
    std::fs::read(schedules_path())
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Every file under `dir` that belongs in a snapshot, relative and sorted.
fn walk(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        for entry in std::fs::read_dir(dir.join(&relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            if skipped(&path) {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Writes a blob unless the store has it; returns the compressed bytes written.
fn store_chunk(hash: &str, data: &[u8]) -> std::io::Result<u64> {
    let path = blob_path(hash);
    if path.exists() {
        return Ok(0);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let compressed = zstd::bulk::compress(data, ZSTD_LEVEL)?;
    let partial = path.with_extension("partial");
    std::fs::write(&partial, &compressed)?;
    std::fs::rename(&partial, &path)?;
    Ok(compressed.len() as u64)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn take(dir: &Path, scheduled: bool) -> std::io::Result<Manifest> {
    let mut manifest = Manifest {
        created: now(),
        files: Vec::new(),
        bytes: 0,
        stored: 0,
        scheduled,
    };
    for relative in walk(dir)? {
        let file = std::fs::File::open(dir.join(&relative))?;
        let mut entry = FileEntry {
            path: relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/"),
            size: 0,
            chunks: Vec::new(),
        };
        for chunk in fastcdc::v2020::StreamCDC::new(file, MIN_CHUNK, AVG_CHUNK, MAX_CHUNK) {
            let chunk = chunk.map_err(std::io::Error::other)?;
            let hash = hex(&Sha256::digest(&chunk.data));
            manifest.stored += store_chunk(&hash, &chunk.data)?;
            entry.size += chunk.length as u64;
            entry.chunks.push(hash);
        }
        manifest.bytes += entry.size;
        manifest.files.push(entry);
    }
    Ok(manifest)
}

/// A manifest path as a safe relative path: no roots, no `..`.
fn relative_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    path.components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(path)
}

/// Puts the folder back as the manifest has it: rewrites changed files and removes files the
/// snapshot didn't have (outside the skipped ones).
fn restore(dir: &Path, manifest: &Manifest) -> std::io::Result<()> {
    let bad = |what: String| std::io::Error::new(std::io::ErrorKind::InvalidData, what);
    let mut keep = HashSet::new();
    for entry in &manifest.files {
        let relative =
            relative_path(&entry.path).ok_or_else(|| bad(format!("bad path {}", entry.path)))?;
        let target = dir.join(&relative);
        keep.insert(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let partial = target.with_file_name(format!(
            ".{}.partial",
            target
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ));
        let mut out = std::io::BufWriter::new(std::fs::File::create(&partial)?);
        for hash in &entry.chunks {
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(bad(format!("bad chunk id {hash}")));
            }
            let mut compressed = Vec::new();
            std::fs::File::open(blob_path(hash))?.read_to_end(&mut compressed)?;
            let data = zstd::bulk::decompress(&compressed, MAX_CHUNK)?;
            if hex(&Sha256::digest(&data)) != *hash {
                return Err(bad(format!("chunk {hash} is damaged")));
            }
            out.write_all(&data)?;
        }
        out.into_inner().map_err(|e| e.into_error())?.sync_all()?;
        std::fs::rename(&partial, &target)?;
    }
    for relative in walk(dir)? {
        if !keep.contains(&relative) {
            std::fs::remove_file(dir.join(relative))?;
        }
    }
    Ok(())
}

fn read_manifest(server: &str, id: &str) -> Option<Manifest> {
    let bytes = std::fs::read(snapshots_dir(server).join(format!("{id}.json"))).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Removes blobs no snapshot of any server refers to.
fn collect_garbage() -> std::io::Result<u64> {
    let mut used = HashSet::new();
    for server in std::fs::read_dir(root().join("snapshots"))?.flatten() {
        for snapshot in std::fs::read_dir(server.path())?.flatten() {
            let bytes = std::fs::read(snapshot.path())?;
            let manifest: Manifest = serde_json::from_slice(&bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            used.extend(manifest.files.into_iter().flat_map(|f| f.chunks));
        }
    }
    let mut freed = 0;
    for prefix in std::fs::read_dir(root().join("blobs"))?.flatten() {
        for blob in std::fs::read_dir(prefix.path())?.flatten() {
            let name = blob.file_name().to_string_lossy().into_owned();
            let hash = name.trim_end_matches(".zst");
            if !used.contains(hash) {
                freed += blob.metadata().map_or(0, |m| m.len());
                std::fs::remove_file(blob.path())?;
            }
        }
    }
    Ok(freed)
}

fn internal(err: impl std::fmt::Display) -> Response {
    error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

/// Every snapshot of a server, newest first.
fn snapshots(server: &str) -> Vec<Snapshot> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(snapshots_dir(server))
        .into_iter()
        .flatten()
        .flatten()
    {
        let file = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = file.strip_suffix(".json") else {
            continue;
        };
        if let Some(m) = read_manifest(server, id) {
            out.push(Snapshot::new(id.to_string(), &m));
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then(b.id.cmp(&a.id)));
    out
}

pub async fn list(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if app.servers.folder(&name).await.is_none() {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    }
    let found = tokio::task::spawn_blocking(move || snapshots(&name))
        .await
        .unwrap_or_default();
    Json(found).into_response()
}

fn internal_error(err: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
}

/// Takes a snapshot. A running server saves first and doesn't write while the files are read.
async fn back_up(app: &App, name: &str, scheduled: bool) -> Result<Snapshot, (StatusCode, String)> {
    let _busy = Busy::claim(name)?;
    let dir = app
        .servers
        .folder(name)
        .await
        .ok_or((StatusCode::NOT_FOUND, format!("no server named {name}")))?;
    let running = app.servers.is_running(name).await;
    if running {
        app.servers.command(name, "save-off").await?;
        let saved = app
            .servers
            .command_until(name, "save-all flush", "Saved the game", SAVE_TIMEOUT)
            .await;
        if let Err(err) = saved {
            let _ = app.servers.command(name, "save-on").await;
            return Err(err);
        }
    }
    let store = STORE.read().await;
    let taken = tokio::task::spawn_blocking(move || take(&dir, scheduled)).await;
    if running {
        let _ = app.servers.command(name, "save-on").await;
    }
    let manifest = taken.map_err(internal_error)?.map_err(internal_error)?;
    let server = name.to_string();
    let snapshot = tokio::task::spawn_blocking(move || {
        let dir = snapshots_dir(&server);
        std::fs::create_dir_all(&dir)?;
        let mut id = manifest.created.to_string();
        let mut n = 1;
        while dir.join(format!("{id}.json")).exists() {
            n += 1;
            id = format!("{}-{n}", manifest.created);
        }
        let json = serde_json::to_vec_pretty(&manifest).map_err(std::io::Error::other)?;
        let partial = dir.join(format!(".{id}.partial"));
        std::fs::write(&partial, json)?;
        std::fs::rename(&partial, dir.join(format!("{id}.json")))?;
        Ok::<_, std::io::Error>(Snapshot::new(id, &manifest))
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)?;
    drop(store);
    info!(
        "{name}: backup {} ({} files, {} bytes, {} new)",
        snapshot.id, snapshot.files, snapshot.bytes, snapshot.stored
    );
    Ok(snapshot)
}

pub async fn create(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    match back_up(&app, &name, false).await {
        Ok(snapshot) => (StatusCode::CREATED, Json(snapshot)).into_response(),
        Err((status, message)) => error(status, message),
    }
}

pub async fn restore_one(
    State(app): State<Arc<App>>,
    UrlPath((name, id)): UrlPath<(String, String)>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if !valid_name(&id) {
        return error(StatusCode::BAD_REQUEST, format!("bad backup id {id:?}"));
    }
    let _busy = match Busy::claim(&name) {
        Ok(busy) => busy,
        Err((status, message)) => return error(status, message),
    };
    let _store = STORE.read().await;
    let result = app
        .servers
        .with_stopped(&name, async |dir: &Path| {
            let (dir, server, id) = (dir.to_path_buf(), name.clone(), id.clone());
            tokio::task::spawn_blocking(move || {
                let manifest = read_manifest(&server, &id).ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::NotFound, format!("no backup {id}"))
                })?;
                restore(&dir, &manifest)
            })
            .await
            .map_err(std::io::Error::other)?
        })
        .await;
    match result {
        Ok(()) => {
            info!("{name}: restored backup {id}");
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, message)) => error(status, message),
    }
}

/// Removes this server's backup data while it is locked against start and folder changes.
pub async fn purge_server(name: &str) -> Result<(), (StatusCode, String)> {
    let _store = STORE.write().await;
    let name = name.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut schedules = read_schedules();
        let before = schedules.len();
        let key = server_key(&name);
        schedules.retain(|server, _| server_key(server) != key);
        if schedules.len() != before {
            write_schedules(&schedules)?;
        }
        match std::fs::remove_dir_all(snapshots_dir(&name)) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
        if root().join("snapshots").try_exists()? && root().join("blobs").try_exists()? {
            collect_garbage()?;
        }
        Ok(())
    })
    .await
    .map_err(internal_error)?
    .map_err(internal_error)
}

fn write_schedules(schedules: &BTreeMap<String, Schedule>) -> std::io::Result<()> {
    std::fs::create_dir_all(root())?;
    let json = serde_json::to_vec_pretty(schedules).map_err(std::io::Error::other)?;
    let partial = root().join(".schedules.partial");
    std::fs::write(&partial, json)?;
    std::fs::rename(&partial, schedules_path())
}

/// Removes snapshots, then the blobs nothing names any more.
async fn remove(server: String, ids: Vec<String>) -> std::io::Result<u64> {
    let _store = STORE.write().await;
    tokio::task::spawn_blocking(move || {
        for id in &ids {
            std::fs::remove_file(snapshots_dir(&server).join(format!("{id}.json")))?;
        }
        collect_garbage()
    })
    .await
    .map_err(std::io::Error::other)?
}

pub async fn delete(
    State(app): State<Arc<App>>,
    UrlPath((name, id)): UrlPath<(String, String)>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if !valid_name(&id) {
        return error(StatusCode::BAD_REQUEST, format!("bad backup id {id:?}"));
    }
    if app.servers.folder(&name).await.is_none() {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    }
    match remove(name, vec![id]).await {
        Ok(freed) => {
            info!("removed a backup, freed {freed} bytes");
            StatusCode::NO_CONTENT.into_response()
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            error(StatusCode::NOT_FOUND, "no such backup".into())
        }
        Err(err) => {
            warn!("removing a backup: {err}");
            internal(err)
        }
    }
}

pub async fn get_schedule(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if app.servers.folder(&name).await.is_none() {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    }
    let schedules = tokio::task::spawn_blocking(read_schedules)
        .await
        .unwrap_or_default();
    Json(schedules.get(&name).copied()).into_response()
}

/// Sets a server's schedule; `null` turns scheduled backups off.
pub async fn put_schedule(
    State(app): State<Arc<App>>,
    UrlPath(name): UrlPath<String>,
    Json(schedule): Json<Option<Schedule>>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let _busy = match Busy::claim(&name) {
        Ok(busy) => busy,
        Err((status, message)) => return error(status, message),
    };
    if app.servers.folder(&name).await.is_none() {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    }
    if let Some(s) = schedule
        && (!(1..=MAX_EVERY_MINUTES).contains(&s.every_minutes)
            || !(1..=MAX_KEEP).contains(&s.keep))
    {
        return error(
            StatusCode::BAD_REQUEST,
            format!("every_minutes must be 1 to {MAX_EVERY_MINUTES} and keep 1 to {MAX_KEEP}"),
        );
    }
    let _store = STORE.write().await;
    let written = tokio::task::spawn_blocking(move || {
        let mut schedules = read_schedules();
        match schedule {
            Some(s) => schedules.insert(name, s),
            None => schedules.remove(&name),
        };
        write_schedules(&schedules)
    })
    .await;
    match written {
        Ok(Ok(())) => Json(schedule).into_response(),
        Ok(Err(err)) => internal(err),
        Err(err) => internal(err),
    }
}

/// Backs up running servers on their schedule and keeps the newest `keep` scheduled backups.
/// Backups taken by hand are never removed here.
pub async fn run_schedules(app: Arc<App>) {
    let mut tick = tokio::time::interval(SCHEDULE_TICK);
    loop {
        tick.tick().await;
        let schedules = tokio::task::spawn_blocking(read_schedules)
            .await
            .unwrap_or_default();
        for (name, schedule) in schedules {
            if !app.servers.is_running(&name).await {
                continue;
            }
            let server = name.clone();
            let scheduled: Vec<Snapshot> = tokio::task::spawn_blocking(move || {
                snapshots(&server)
                    .into_iter()
                    .filter(|s| s.scheduled)
                    .collect()
            })
            .await
            .unwrap_or_default();
            let due = scheduled.first().is_none_or(|last| {
                now().saturating_sub(last.created) >= u64::from(schedule.every_minutes) * 60
            });
            if !due {
                continue;
            }
            let taken = match back_up(&app, &name, true).await {
                Ok(snapshot) => snapshot,
                Err((_, message)) => {
                    warn!("{name}: scheduled backup failed: {message}");
                    continue;
                }
            };
            let keep = usize::try_from(schedule.keep).unwrap_or(usize::MAX);
            let old: Vec<String> = std::iter::once(taken.id)
                .chain(scheduled.into_iter().map(|s| s.id))
                .skip(keep)
                .collect();
            if old.is_empty() {
                continue;
            }
            match remove(name.clone(), old).await {
                Ok(freed) => info!("{name}: retention freed {freed} bytes"),
                Err(err) => warn!("{name}: retention: {err}"),
            }
        }
    }
}
