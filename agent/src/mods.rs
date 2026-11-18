//! A server's drop-in mods: the `.mmc` files in its `mods/` folder (docs/DEVICES.md).

use std::{path::Path, sync::Arc};

use axum::{
    Json,
    body::Bytes,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    App,
    servers::{bad_name, error, valid_name},
};

/// Largest `.mmc` one upload may be.
pub const MAX_MOD: usize = 256 << 20;
/// `mod.json` is a few lines; anything bigger isn't one.
const MAX_MOD_JSON: u64 = 64 << 10;

#[derive(Serialize)]
pub struct ModFile {
    file: String,
    id: Option<String>,
    version: Option<String>,
    bytes: u64,
}

#[derive(Deserialize)]
struct ModJson {
    id: Option<String>,
    version: Option<String>,
}

/// `id` and `version` from an `.mmc`'s `mod.json`; `None` when it isn't a mod archive.
fn mod_json(reader: impl std::io::Read + std::io::Seek) -> Option<ModJson> {
    let mut archive = zip::ZipArchive::new(reader).ok()?;
    let entry = archive.by_name("mod.json").ok()?;
    if entry.size() > MAX_MOD_JSON {
        return None;
    }
    serde_json::from_reader(std::io::Read::take(entry, MAX_MOD_JSON)).ok()
}

fn valid_file(file: &str) -> bool {
    valid_name(file) && file.ends_with(".mmc")
}

pub async fn list(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let Some(dir) = app.servers.folder(&name).await else {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    };
    let mods = dir.join("mods");
    let found = tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&mods).into_iter().flatten().flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            if !valid_file(&file) {
                continue;
            }
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let json = std::fs::File::open(entry.path()).ok().and_then(mod_json);
            out.push(ModFile {
                file,
                id: json.as_ref().and_then(|j| j.id.clone()),
                version: json.and_then(|j| j.version),
                bytes,
            });
        }
        out.sort_by(|a, b| a.file.cmp(&b.file));
        out
    })
    .await
    .unwrap_or_default();
    Json(found).into_response()
}

pub async fn put(
    State(app): State<Arc<App>>,
    UrlPath((name, file)): UrlPath<(String, String)>,
    body: Bytes,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if !valid_file(&file) {
        return error(StatusCode::BAD_REQUEST, format!("bad mod file {file:?}"));
    }
    if mod_json(std::io::Cursor::new(&body[..])).is_none() {
        return error(
            StatusCode::BAD_REQUEST,
            format!("{file} isn't a mod: no mod.json inside"),
        );
    }
    let result = app
        .servers
        .with_stopped(&name, async |dir: &Path| {
            let mods = dir.join("mods");
            tokio::fs::create_dir_all(&mods).await?;
            let partial = mods.join(format!(".{file}.partial"));
            tokio::fs::write(&partial, &body).await?;
            tokio::fs::rename(&partial, mods.join(&file)).await
        })
        .await;
    match result {
        Ok(()) => {
            info!("{name}: added {file}");
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, message)) => error(status, message),
    }
}

pub async fn delete(
    State(app): State<Arc<App>>,
    UrlPath((name, file)): UrlPath<(String, String)>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    if !valid_file(&file) {
        return error(StatusCode::BAD_REQUEST, format!("bad mod file {file:?}"));
    }
    let result = app
        .servers
        .with_stopped(&name, async |dir: &Path| {
            let mods = dir.join("mods");
            match tokio::fs::remove_file(mods.join(&file)).await {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            }
        })
        .await;
    match result {
        Ok(()) => {
            info!("{name}: removed {file}");
            StatusCode::NO_CONTENT.into_response()
        }
        Err((status, message)) => error(status, message),
    }
}
