//! Pack builds pushed by the app (`moddedmc-pack` output, zipped): `~/.moddedmc/builds/<id>/`
//! holds `pack-<pack>[.exe]`, `mods/*.mmc` and `pack.lock`. Servers run a build in place.

use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Json,
    body::Bytes,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::App;

/// A pack server binary is tens of MB; this leaves room for a big pack's mods.
pub const MAX_UPLOAD: usize = 1 << 30;
/// Most files one build may unpack, so a hostile zip can't fill the disk with entries.
const MAX_ENTRIES: usize = 4096;

pub fn root() -> PathBuf {
    crate::config::home().join("builds")
}

#[derive(Serialize)]
pub struct BuildInfo {
    id: String,
    pack: String,
    version: String,
}

#[derive(Deserialize)]
struct Lock {
    pack: LockPack,
}

#[derive(Deserialize)]
struct LockPack {
    id: String,
    version: String,
}

/// The pack id and version a build's `pack.lock` names, and its server binary.
pub async fn describe(dir: &Path) -> Option<(String, String, PathBuf)> {
    let text = tokio::fs::read_to_string(dir.join("pack.lock"))
        .await
        .ok()?;
    let lock: Lock = toml::from_str(&text).ok()?;
    let exe = dir.join(format!(
        "pack-{}{}",
        lock.pack.id,
        std::env::consts::EXE_SUFFIX
    ));
    tokio::fs::try_exists(&exe)
        .await
        .unwrap_or(false)
        .then_some((lock.pack.id, lock.pack.version, exe))
}

pub async fn list() -> Json<Vec<BuildInfo>> {
    let mut out = Vec::new();
    if let Ok(mut dir) = tokio::fs::read_dir(root()).await {
        while let Ok(Some(entry)) = dir.next_entry().await {
            let id = entry.file_name().to_string_lossy().into_owned();
            if let Some((pack, version, _)) = describe(&entry.path()).await {
                out.push(BuildInfo { id, pack, version });
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Json(out)
}

pub async fn upload(
    State(_app): State<Arc<App>>,
    UrlPath(id): UrlPath<String>,
    body: Bytes,
) -> Response {
    if !crate::servers::valid_name(&id) {
        return (StatusCode::BAD_REQUEST, format!("bad build id {id:?}")).into_response();
    }
    let dir = root().join(&id);
    if describe(&dir).await.is_some() {
        return (StatusCode::OK, "already uploaded").into_response();
    }
    let tmp = root().join(format!(".{id}.partial"));
    let target = tmp.clone();
    let unpacked = tokio::task::spawn_blocking(move || unpack(&body, &target)).await;
    let result = match unpacked {
        Ok(result) => result,
        Err(err) => Err(err.to_string()),
    };
    if let Err(err) = result {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        return (StatusCode::BAD_REQUEST, err).into_response();
    }
    if describe(&tmp).await.is_none() {
        let _ = tokio::fs::remove_dir_all(&tmp).await;
        return (
            StatusCode::BAD_REQUEST,
            "not a pack build: needs pack.lock and its pack server",
        )
            .into_response();
    }
    let _ = tokio::fs::remove_dir_all(&dir).await;
    if let Err(err) = tokio::fs::rename(&tmp, &dir).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response();
    }
    info!("received build {id}");
    StatusCode::CREATED.into_response()
}

fn unpack(bytes: &[u8], dir: &Path) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| format!("not a zip: {e}"))?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!("more than {MAX_ENTRIES} files"));
    }
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // `enclosed_name` rejects absolute paths and `..`, so nothing lands outside `dir`.
        let Some(relative) = entry.enclosed_name() else {
            return Err(format!("unsafe path {:?}", entry.name()));
        };
        let path = dir.join(relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut data = Vec::new();
        // Bound what one zip can expand to, whatever its headers claim.
        let limit = (MAX_UPLOAD as u64 * 4).saturating_sub(total);
        entry
            .by_ref()
            .take(limit + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        total += data.len() as u64;
        if total > MAX_UPLOAD as u64 * 4 {
            return Err("unpacks too large".into());
        }
        std::fs::write(&path, &data).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("pack-"))
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
