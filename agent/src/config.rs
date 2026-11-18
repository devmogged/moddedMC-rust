use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tracing::info;

pub const DEFAULT_PORT: u16 = 25599;

/// `~/.moddedmc/agent.toml`.
#[derive(Serialize, Deserialize)]
pub struct Config {
    /// The Tailscale login allowed to control this machine over the tailnet. Loopback (and so
    /// SSH tunnels) is always allowed. `None` refuses every tailnet caller.
    pub owner: Option<String>,
    #[serde(default = "default_port")]
    pub port: u16,
    pub servers_root: PathBuf,
    /// Folders that hold servers the agent didn't make; each subfolder with a
    /// `server.properties` is listed as an external server while stopped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub find_in: Vec<PathBuf>,
}

const fn default_port() -> u16 {
    DEFAULT_PORT
}

pub fn home() -> PathBuf {
    std::env::var_os("MODDEDMC_HOME").map_or_else(
        || {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map_or_else(|| PathBuf::from("."), PathBuf::from)
                .join(".moddedmc")
        },
        PathBuf::from,
    )
}

impl Config {
    pub async fn load_or_create() -> Result<Self, String> {
        let home = home();
        let path = home.join("agent.toml");
        if let Ok(text) = tokio::fs::read_to_string(&path).await {
            return toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()));
        }
        // First run: the owner is whoever is logged in to Tailscale on this machine.
        let owner = crate::auth::self_login().await;
        let config = Self {
            owner,
            port: DEFAULT_PORT,
            servers_root: home.join("servers"),
            find_in: Vec::new(),
        };
        tokio::fs::create_dir_all(&config.servers_root)
            .await
            .map_err(|e| format!("{}: {e}", config.servers_root.display()))?;
        let text = toml::to_string_pretty(&config).map_err(|e| e.to_string())?;
        tokio::fs::write(&path, text)
            .await
            .map_err(|e| format!("{}: {e}", path.display()))?;
        info!(
            "wrote {} (owner: {})",
            path.display(),
            config.owner.as_deref().unwrap_or("none, loopback only")
        );
        Ok(config)
    }
}
