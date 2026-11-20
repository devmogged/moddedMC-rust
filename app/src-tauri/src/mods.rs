//! The mod marketplace: `index.toml` from the public mods repo, and installing a listed `.mmc`
//! onto a server after checking it against the index's checksum.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::State;

use crate::{deploy::put_mod, devices::Tunnels, store::Store};

const INDEX_URL: &str = "https://raw.githubusercontent.com/devmogged/mods/main/index.toml";
const MAX_INDEX: usize = 4 << 20;
const MAX_MOD: u64 = 64 << 20;

/// One entry of `index.toml` (format: devmogged/mods README).
#[derive(Serialize, Deserialize, Clone)]
pub struct Mod {
    pub id: String,
    pub name: String,
    pub version: String,
    /// `drop-in` (an `.mmc` file) or `port` (compiled into pack servers).
    pub kind: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub upstream: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    /// The built `.mmc`, for drop-in mods.
    #[serde(default)]
    pub download: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}

#[derive(Deserialize)]
struct Index {
    #[serde(default)]
    mods: Vec<Mod>,
}

#[tauri::command]
pub async fn mods_index() -> Result<Vec<Mod>, String> {
    let offline = || "Can't load the mod list. Are you online?".to_string();
    let response = crate::agent::http()
        .get(INDEX_URL)
        .send()
        .await
        .map_err(|_| offline())?;
    if !response.status().is_success() {
        return Err(offline());
    }
    let bytes = response.bytes().await.map_err(|_| offline())?;
    if bytes.len() > MAX_INDEX {
        return Err("The mod list is too large.".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "The mod list is damaged.".to_string())?;
    let index: Index = toml::from_str(text).map_err(|_| "The mod list is damaged.".to_string())?;
    Ok(index.mods)
}

/// Download mod `id` from the index and put it in a stopped server's `mods/`.
#[tauri::command]
pub async fn mod_install(
    store: State<'_, Store>,
    tunnels: State<'_, Tunnels>,
    device: String,
    server: String,
    id: String,
) -> Result<(), String> {
    let index = mods_index().await?;
    let entry = index
        .into_iter()
        .find(|m| m.id == id)
        .ok_or("That mod isn't in the list anymore.")?;
    let (Some(url), Some(sha256)) = (entry.download.as_deref(), entry.sha256.as_deref()) else {
        return Err(format!("{} can't be installed on its own.", entry.name));
    };
    let response = crate::agent::http()
        .get(url)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("Downloading {}: {e}", entry.name))?;
    if response.content_length().is_some_and(|n| n > MAX_MOD) {
        return Err(format!("{} is too large.", entry.name));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Downloading {}: {e}", entry.name))?;
    if bytes.len() as u64 > MAX_MOD {
        return Err(format!("{} is too large.", entry.name));
    }
    let digest: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != sha256.to_ascii_lowercase() {
        return Err(format!(
            "{} didn't download correctly. Try again.",
            entry.name
        ));
    }
    let file = format!("{}.mmc", entry.id);
    put_mod(&store, &tunnels, &device, &server, &file, bytes.to_vec()).await
}
