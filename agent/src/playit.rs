//! Addresses friends can join a server on, and keeping playit.gg running while a server it
//! tunnels to is up. playit is the program the user installed and signed in to; its CLI prints
//! the account's tunnels (`playit tunnels list`, JSON), and `playit --stdout start` runs it
//! headless.

use std::{
    net::{IpAddr, UdpSocket},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tracing::{info, warn};

use crate::App;

#[derive(Serialize)]
pub struct Address {
    /// `playit`, `tailscale` or `lan`.
    kind: &'static str,
    address: String,
}

#[derive(Serialize)]
pub struct Addresses {
    addresses: Vec<Address>,
    playit: PlayitState,
}

#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum PlayitState {
    /// The playit program isn't installed here.
    NotInstalled,
    /// Installed but not signed in (no secret yet).
    NotSetUp,
    /// Signed in, but no tunnel points at this server's port.
    NoTunnel,
    /// A tunnel points here, but playit isn't running.
    Stopped,
    Running,
}

fn program() -> PathBuf {
    let installed = PathBuf::from(r"C:\Program Files\playit_gg\bin\playit.exe");
    if cfg!(windows) && installed.is_file() {
        installed
    } else {
        PathBuf::from("playit")
    }
}

#[derive(Deserialize)]
struct TunnelList {
    tunnels: Vec<Tunnel>,
}

#[derive(Deserialize)]
struct Tunnel {
    #[serde(default)]
    disabled_reason: Option<String>,
    alloc: Alloc,
    origin: Origin,
}

#[derive(Deserialize)]
struct Alloc {
    #[serde(default)]
    data: Option<AllocData>,
}

#[derive(Deserialize)]
struct AllocData {
    assigned_domain: String,
}

#[derive(Deserialize)]
struct Origin {
    #[serde(default)]
    data: Option<OriginData>,
}

#[derive(Deserialize)]
struct OriginData {
    local_port: u16,
}

/// The public addresses of enabled tunnels that point at `port`, or the state that explains why
/// there are none.
async fn tunnels_to(port: u16) -> Result<Vec<String>, PlayitState> {
    let output = crate::command(program())
        .args(["tunnels", "list"])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|_| PlayitState::NotInstalled)?;
    if !output.status.success() {
        return Err(PlayitState::NotSetUp);
    }
    let list: TunnelList =
        serde_json::from_slice(&output.stdout).map_err(|_| PlayitState::NotSetUp)?;
    let found: Vec<String> = list
        .tunnels
        .into_iter()
        .filter(|t| t.disabled_reason.is_none())
        .filter(|t| t.origin.data.as_ref().is_some_and(|o| o.local_port == port))
        .filter_map(|t| t.alloc.data.map(|a| a.assigned_domain))
        .collect();
    if found.is_empty() {
        Err(PlayitState::NoTunnel)
    } else {
        Ok(found)
    }
}

fn playit_running() -> bool {
    let Ok(mut system) = System::new() else {
        return false;
    };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system.processes().values().any(|p| {
        p.name()
            .to_string_lossy()
            .to_lowercase()
            .trim_end_matches(".exe")
            == "playit"
    })
}

/// Starts playit if a tunnel points at `port` and it isn't running yet. It keeps running after
/// the server stops, like playit's own app; its log is `playit.log` in the agent's folder.
pub async fn ensure_for(port: u16) {
    if tunnels_to(port).await.is_err() || playit_running() {
        return;
    }
    let log = std::env::current_dir()
        .unwrap_or_default()
        .join("playit.log");
    let (Ok(out), Ok(err)) = (
        std::fs::File::create(&log),
        std::fs::File::create(log.with_extension("err.log")),
    ) else {
        warn!("cannot write {}", log.display());
        return;
    };
    match crate::command(program())
        .args(["--stdout", "start"])
        .stdin(Stdio::null())
        .stdout(out)
        .stderr(err)
        .spawn()
    {
        Ok(child) => info!("started playit (pid {:?})", child.id()),
        Err(e) => warn!("cannot start playit: {e}"),
    }
}

/// This machine's address on the local network: the source address of a route to the internet.
/// Nothing is sent.
fn lan_ip() -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("1.1.1.1:80").ok()?;
    Some(socket.local_addr().ok()?.ip())
}

pub async fn port_of(app: &App, name: &str) -> Option<u16> {
    app.servers
        .list()
        .await
        .into_iter()
        .find(|s| s.name == name)
        .map(|s| s.port.unwrap_or(25565))
}

pub async fn get(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = crate::servers::bad_name(&name) {
        return response;
    }
    let Some(port) = port_of(&app, &name).await else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            format!("no server {name}"),
        )
            .into_response();
    };
    let mut addresses = Vec::new();
    let playit = match tunnels_to(port).await {
        Ok(found) => {
            addresses.extend(found.into_iter().map(|address| Address {
                kind: "playit",
                address,
            }));
            if playit_running() {
                PlayitState::Running
            } else {
                PlayitState::Stopped
            }
        }
        Err(state) => state,
    };
    if let Some(ip) = crate::auth::tailnet_ipv4().await {
        addresses.push(Address {
            kind: "tailscale",
            address: format!("{ip}:{port}"),
        });
    }
    if let Some(ip) = lan_ip() {
        addresses.push(Address {
            kind: "lan",
            address: format!("{ip}:{port}"),
        });
    }
    Json(Addresses { addresses, playit }).into_response()
}

/// `POST /v1/servers/{name}/playit`: start playit for this server's tunnel now.
pub async fn start(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = crate::servers::bad_name(&name) {
        return response;
    }
    match port_of(&app, &name).await {
        Some(port) => {
            ensure_for(port).await;
            axum::http::StatusCode::OK.into_response()
        }
        None => (
            axum::http::StatusCode::NOT_FOUND,
            format!("no server {name}"),
        )
            .into_response(),
    }
}
