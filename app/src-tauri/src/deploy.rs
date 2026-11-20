//! Deploying a pack build (a `moddedmc-pack` output folder) to a server: zip it, upload it to
//! the device's agent once, then point the server at it.

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::Serialize;
use tauri::State;

use crate::{agent, devices::Tunnels, store::Store};

#[derive(Serialize)]
pub struct Build {
    /// `<pack>-<first 12 of the build key>`: the same build always gets the same id.
    id: String,
    pack: String,
    version: String,
    path: String,
}

/// Reads a build folder: `pack.lock`, `.build-key` and the pack server must be there.
fn read_build(dir: &Path) -> Result<Build, String> {
    let not_a_build = || format!("{} isn't a pack build folder.", dir.display());
    let lock = std::fs::read_to_string(dir.join("pack.lock")).map_err(|_| not_a_build())?;
    let lock: toml::Table = lock.parse().map_err(|_| not_a_build())?;
    let pack = lock
        .get("pack")
        .and_then(toml::Value::as_table)
        .ok_or_else(not_a_build)?;
    let field = |k: &str| pack.get(k).and_then(toml::Value::as_str).map(str::to_owned);
    let (Some(id), Some(version)) = (field("id"), field("version")) else {
        return Err(not_a_build());
    };
    let key = std::fs::read_to_string(dir.join(".build-key")).map_err(|_| not_a_build())?;
    let key: String = key.trim().chars().take(12).collect();
    if key.len() < 12 || !key.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(not_a_build());
    }
    Ok(Build {
        id: format!("{id}-{key}"),
        pack: id,
        version,
        path: dir.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn build_inspect(path: String) -> Result<Build, String> {
    read_build(&PathBuf::from(path))
}

fn zip_dir(dir: &Path) -> Result<Vec<u8>, String> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(true);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|e| e.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(dir)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
            out.start_file(relative, options)
                .map_err(|e| e.to_string())?;
            out.write_all(&bytes).map_err(|e| e.to_string())?;
        }
    }
    out.finish()
        .map(std::io::Cursor::into_inner)
        .map_err(|e| e.to_string())
}

/// Uploads the build if the device doesn't have it yet, then deploys it to the server.
#[tauri::command]
pub async fn deploy(
    store: State<'_, Store>,
    tunnels: State<'_, Tunnels>,
    device: String,
    server: String,
    path: String,
) -> Result<Build, String> {
    if server.contains(['/', '\\', '?', '#']) {
        return Err("bad server name".into());
    }
    let dir = PathBuf::from(&path);
    let build = read_build(&dir)?;
    let (endpoint, name) = agent::endpoint(&store, &tunnels, &device).await?;
    let unreachable = |_| format!("Can't reach {name}. Is the agent running there?");

    let zipped = tauri::async_runtime::spawn_blocking(move || zip_dir(&dir))
        .await
        .map_err(|e| e.to_string())??;
    let response = agent::http()
        .post(format!("http://{endpoint}/v1/builds/{}", build.id))
        .timeout(Duration::from_secs(600))
        .body(zipped)
        .send()
        .await
        .map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(response.text().await.unwrap_or_default());
    }

    let response = agent::http()
        .post(format!("http://{endpoint}/v1/servers/{server}/deploy"))
        .json(&serde_json::json!({ "build": build.id }))
        .send()
        .await
        .map_err(unreachable)?;
    if !response.status().is_success() {
        return Err(response.text().await.unwrap_or_default());
    }
    Ok(build)
}

/// Uploads one drop-in mod (`.mmc`) into a stopped server's `mods/`.
#[tauri::command]
pub async fn mod_upload(
    store: State<'_, Store>,
    tunnels: State<'_, Tunnels>,
    device: String,
    server: String,
    path: String,
) -> Result<(), String> {
    if server.contains(['/', '\\', '?', '#']) {
        return Err("bad server name".into());
    }
    let path = PathBuf::from(path);
    let file = path
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .filter(|f| f.ends_with(".mmc"))
        .ok_or("Pick a .mmc file.")?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| format!("Can't read {}: {e}", path.display()))?;
    put_mod(&store, &tunnels, &device, &server, &file, bytes).await
}

/// Puts `bytes` as `mods/<file>` on a stopped server.
pub async fn put_mod(
    store: &Store,
    tunnels: &Tunnels,
    device: &str,
    server: &str,
    file: &str,
    bytes: Vec<u8>,
) -> Result<(), String> {
    if server.contains(['/', '\\', '?', '#']) || file.contains(['/', '\\', '?', '#']) {
        return Err("bad server or file name".into());
    }
    let (endpoint, name) = agent::endpoint(store, tunnels, device).await?;
    let response = agent::http()
        .put(format!("http://{endpoint}/v1/servers/{server}/mods/{file}"))
        .timeout(Duration::from_secs(120))
        .body(bytes)
        .send()
        .await
        .map_err(|_| format!("Can't reach {name}. Is the agent running there?"))?;
    if !response.status().is_success() {
        return Err(response.text().await.unwrap_or_default());
    }
    Ok(())
}
