//! The ported-mod catalog: `index.toml` from the public mods repo.

use serde::{Deserialize, Serialize};

const INDEX_URL: &str = "https://raw.githubusercontent.com/devmogged/mods/main/index.toml";
const MAX_INDEX: usize = 4 << 20;

#[derive(Serialize, Deserialize)]
pub struct Mod {
    pub id: String,
    pub name: String,
    pub version: String,
    /// `drop-in` (an `.mmc` file) or `port` (compiled into pack servers).
    pub kind: String,
    #[serde(default)]
    pub minecraft: Option<String>,
    #[serde(default)]
    pub upstream: Option<String>,
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
