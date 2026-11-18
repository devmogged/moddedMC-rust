//! Snapshots of a server folder (docs/DEVICES.md, "Backups"). Files are cut into
//! content-defined chunks (FastCDC), and each chunk is stored once under its SHA-256, compressed
//! with zstd, in a store all servers share. A snapshot is a manifest of chunk lists, so it costs
//! only the chunks that no earlier snapshot has.

use std::{
    collections::HashSet,
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
use tracing::{info, warn};

use crate::{
    App,
    servers::{bad_name, error, valid_name},
};

/// Chunk sizes: small enough that a changed region of a world file costs little, large enough
/// that a big world doesn't turn into millions of blobs.
const MIN_CHUNK: usize = 16 << 10;
const AVG_CHUNK: usize = 64 << 10;
const MAX_CHUNK: usize = 256 << 10;
const ZSTD_LEVEL: i32 = 3;
/// How long a running server gets to finish `save-all flush`.
const SAVE_TIMEOUT: Duration = Duration::from_secs(120);

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

fn take(dir: &Path) -> std::io::Result<Manifest> {
    let mut manifest = Manifest {
        created: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        files: Vec::new(),
        bytes: 0,
        stored: 0,
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

pub async fn list(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if app.servers.folder(&name).await.is_none() {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    }
    let found = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(snapshots_dir(&name))
            .into_iter()
            .flatten()
            .flatten()
        {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(id) = file.strip_suffix(".json") else {
                continue;
            };
            if let Some(m) = read_manifest(&name, id) {
                out.push(Snapshot {
                    id: id.to_string(),
                    created: m.created,
                    files: m.files.len(),
                    bytes: m.bytes,
                    stored: m.stored,
                });
            }
        }
        out.sort_by(|a, b| b.created.cmp(&a.created).then(b.id.cmp(&a.id)));
        out
    })
    .await
    .unwrap_or_default();
    Json(found).into_response()
}

/// Takes a snapshot. A running server saves first and doesn't write while the files are read.
pub async fn create(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let Some(dir) = app.servers.folder(&name).await else {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    };
    let running = app.servers.is_running(&name).await;
    if running {
        if let Err((status, message)) = app.servers.command(&name, "save-off").await {
            return error(status, message);
        }
        let saved = app
            .servers
            .command_until(&name, "save-all flush", "Saved the game", SAVE_TIMEOUT)
            .await;
        if let Err((status, message)) = saved {
            let _ = app.servers.command(&name, "save-on").await;
            return error(status, message);
        }
    }
    let taken = tokio::task::spawn_blocking(move || take(&dir)).await;
    if running {
        let _ = app.servers.command(&name, "save-on").await;
    }
    let manifest = match taken {
        Ok(Ok(manifest)) => manifest,
        Ok(Err(err)) => return internal(err),
        Err(err) => return internal(err),
    };
    let server = name.clone();
    let written = tokio::task::spawn_blocking(move || {
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
        Ok::<_, std::io::Error>(Snapshot {
            id,
            created: manifest.created,
            files: manifest.files.len(),
            bytes: manifest.bytes,
            stored: manifest.stored,
        })
    })
    .await;
    match written {
        Ok(Ok(snapshot)) => {
            info!(
                "{name}: backup {} ({} files, {} bytes, {} new)",
                snapshot.id, snapshot.files, snapshot.bytes, snapshot.stored
            );
            (StatusCode::CREATED, Json(snapshot)).into_response()
        }
        Ok(Err(err)) => internal(err),
        Err(err) => internal(err),
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
    let result = tokio::task::spawn_blocking(move || {
        std::fs::remove_file(snapshots_dir(&name).join(format!("{id}.json")))?;
        collect_garbage()
    })
    .await;
    match result {
        Ok(Ok(freed)) => {
            info!("removed a backup, freed {freed} bytes");
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(err)) if err.kind() == std::io::ErrorKind::NotFound => {
            error(StatusCode::NOT_FOUND, "no such backup".into())
        }
        Ok(Err(err)) => {
            warn!("removing a backup: {err}");
            internal(err)
        }
        Err(err) => internal(err),
    }
}
