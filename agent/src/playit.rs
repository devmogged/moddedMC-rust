//! playit.gg, so friends can join from anywhere without port forwarding. The agent keeps its own
//! copy of playit's daemon (`playitd`) and sign-in in `~/.moddedmc/playit/`: it signs in through
//! playit's claim link, creates a Minecraft Java tunnel for a server on request, and runs playitd
//! while servers are up. A playit sign-in that already exists on this machine is reused.
//! API shapes: playit-cloud/playit-agent, packages/api_client/src/api.rs.

use std::{
    hash::{BuildHasher, RandomState},
    net::{IpAddr, UdpSocket},
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
use tracing::{info, warn};

use crate::App;

const API: &str = "https://api.playit.gg";
const DAEMON_URL: &str = "https://github.com/playit-cloud/playit-agent/releases/latest/download/playitd-windows-x86_64-signed.exe";
const MAX_DAEMON: usize = 64 << 20;

fn dir() -> PathBuf {
    crate::config::home().join("playit")
}

fn secret_path() -> PathBuf {
    dir().join("playit.toml")
}

/// A secret file holds the hex key, bare or as `secret_key = "..."`.
fn parse_secret(text: &str) -> Option<String> {
    let text = text.trim();
    let key = match text.parse::<toml::Table>() {
        Ok(table) => table.get("secret_key")?.as_str()?.trim().to_owned(),
        Err(_) => text.to_owned(),
    };
    (!key.is_empty() && key.len() % 2 == 0 && key.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some(key)
}

/// This machine's playit key: ours, or the one playit's own app saved, copied over once. After
/// `sign-out` our file exists but is empty, so the old key isn't copied back.
async fn secret() -> Option<String> {
    if let Ok(text) = tokio::fs::read_to_string(secret_path()).await {
        return parse_secret(&text);
    }
    let existing = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("playit_gg/playit.toml"))
    } else {
        std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config/playit_gg/playit.toml"))
    };
    let key = parse_secret(&tokio::fs::read_to_string(existing?).await.ok()?)?;
    save_secret(&key).await;
    Some(key)
}

async fn save_secret(key: &str) {
    let _ = tokio::fs::create_dir_all(dir()).await;
    if let Err(e) = tokio::fs::write(secret_path(), format!("secret_key = \"{key}\"\n")).await {
        warn!("cannot save the playit key: {e}");
    }
}

fn http() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_default()
}

/// One playit API call. Replies are `{"status": "success" | "fail" | "error", "data": ...}`.
async fn api<T: DeserializeOwned>(path: &str, body: Value, key: Option<&str>) -> Result<T, String> {
    let mut request = http().post(format!("{API}{path}")).json(&body);
    if let Some(key) = key {
        request = request.header("Authorization", format!("Agent-Key {key}"));
    }
    let reply: Value = request
        .send()
        .await
        .map_err(|_| "Can't reach playit.gg. Are you online?".to_string())?
        .json()
        .await
        .map_err(|_| "playit.gg sent an unexpected reply.".to_string())?;
    if reply["status"] != "success" {
        return Err(format!("playit.gg: {}", reply["data"]));
    }
    serde_json::from_value(reply["data"].clone())
        .map_err(|_| "playit.gg sent an unexpected reply.".to_string())
}

#[derive(Deserialize)]
struct RunData {
    agent_id: String,
    tunnels: Vec<AgentTunnel>,
}

#[derive(Deserialize)]
struct AgentTunnel {
    display_address: String,
    #[serde(default)]
    disabled_reason: Option<String>,
    agent_config: Fields,
}

#[derive(Deserialize)]
struct Fields {
    fields: Vec<Field>,
}

#[derive(Deserialize)]
struct Field {
    name: String,
    value: String,
}

async fn run_data(key: &str) -> Result<RunData, String> {
    api("/v1/agents/rundata", json!({}), Some(key)).await
}

/// Public addresses of the enabled tunnels that point at local `port`.
fn tunnels_to(data: &RunData, port: u16) -> Vec<String> {
    data.tunnels
        .iter()
        .filter(|t| t.disabled_reason.is_none())
        .filter(|t| {
            t.agent_config
                .fields
                .iter()
                .any(|f| f.name == "local_port" && f.value == port.to_string())
        })
        .map(|t| t.display_address.clone())
        .collect()
}

fn running() -> bool {
    let Ok(mut system) = System::new() else {
        return false;
    };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system.processes().values().any(|p| {
        let name = p.name().to_string_lossy().to_lowercase();
        matches!(name.trim_end_matches(".exe"), "playit" | "playitd")
    })
}

/// The playitd this agent started, stopped again when the app quits. A playit the user runs
/// themselves is left alone.
static STARTED: Mutex<Option<tokio::process::Child>> = Mutex::new(None);

pub fn stop_started() {
    if let Some(mut child) = STARTED.lock().unwrap_or_else(|e| e.into_inner()).take() {
        let _ = child.start_kill();
    }
}

/// playitd: ours, downloaded on first use on Windows; elsewhere the one on PATH.
async fn daemon() -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Ok(PathBuf::from("playitd"));
    }
    let path = dir().join("playitd.exe");
    if path.is_file() {
        return Ok(path);
    }
    info!("downloading playitd");
    let bytes = http()
        .get(DAEMON_URL)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("Downloading playit: {e}"))?
        .bytes()
        .await
        .map_err(|e| format!("Downloading playit: {e}"))?;
    if bytes.len() > MAX_DAEMON {
        return Err("The playit download is larger than expected.".into());
    }
    tokio::fs::create_dir_all(dir())
        .await
        .map_err(|e| e.to_string())?;
    let part = path.with_extension("part");
    tokio::fs::write(&part, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&part, &path)
        .await
        .map_err(|e| e.to_string())?;
    Ok(path)
}

async fn start_daemon() -> Result<(), String> {
    let program = daemon().await?;
    let socket = if cfg!(windows) {
        r"\\.\pipe\moddedmc-playitd".to_owned()
    } else {
        dir().join("playitd.sock").to_string_lossy().into_owned()
    };
    let child = crate::command(program)
        .arg("--secret-path")
        .arg(secret_path())
        .args(["--socket-path", &socket, "--log-path"])
        .arg(dir().join("playitd.log"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Can't start playit: {e}"))?;
    info!("started playitd (pid {:?})", child.id());
    *STARTED.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
    Ok(())
}

/// Starts playit if this machine is signed in, a tunnel points at `port` and playit isn't
/// running yet. It keeps running after the server stops, until the app quits.
pub async fn ensure_for(port: u16) {
    let Some(key) = secret().await else { return };
    match run_data(&key).await {
        Ok(data) if !tunnels_to(&data, port).is_empty() && !running() => {
            if let Err(e) = start_daemon().await {
                warn!("{e}");
            }
        }
        Ok(_) => {}
        Err(e) => warn!("{e}"),
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

#[derive(Serialize)]
pub struct Address {
    /// `playit`, `tailscale` or `lan`.
    kind: &'static str,
    address: String,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum PlayitState {
    /// Not signed in to playit on this machine.
    SignedOut,
    /// Signed in, but no tunnel points at this server.
    NoTunnel,
    /// A tunnel points here, but playit isn't running.
    Stopped,
    Running,
    /// playit.gg couldn't be asked.
    Unknown,
}

/// `GET /v1/servers/{name}/addresses`: where friends can join this server.
pub async fn get(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = crate::servers::bad_name(&name) {
        return response;
    }
    let Some(port) = port_of(&app, &name).await else {
        return (StatusCode::NOT_FOUND, format!("no server {name}")).into_response();
    };
    let mut addresses = Vec::new();
    let playit = match secret().await {
        None => PlayitState::SignedOut,
        Some(key) => match run_data(&key).await {
            Err(_) => PlayitState::Unknown,
            Ok(data) => {
                let found = tunnels_to(&data, port);
                let state = if found.is_empty() {
                    PlayitState::NoTunnel
                } else if running() {
                    PlayitState::Running
                } else {
                    PlayitState::Stopped
                };
                addresses.extend(found.into_iter().map(|address| Address {
                    kind: "playit",
                    address,
                }));
                state
            }
        },
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
    Json(json!({ "addresses": addresses, "playit": playit })).into_response()
}

/// `POST /v1/servers/{name}/playit`: give this server a playit address (create a Minecraft Java
/// tunnel to its port if none points there) and start playit.
pub async fn start(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = crate::servers::bad_name(&name) {
        return response;
    }
    let Some(port) = port_of(&app, &name).await else {
        return (StatusCode::NOT_FOUND, format!("no server {name}")).into_response();
    };
    let Some(key) = secret().await else {
        return (StatusCode::CONFLICT, "Sign in to playit first.").into_response();
    };
    let result = async {
        let data = run_data(&key).await?;
        if tunnels_to(&data, port).is_empty() {
            // The agent-key endpoint playit's own CLI uses (`ReqTunnelsCreate`).
            let _: Value = api(
                "/tunnels/create",
                json!({
                    "name": format!("moddedMC {name}"),
                    "tunnel_type": "minecraft-java",
                    "port_type": "tcp",
                    "port_count": 1,
                    "origin": { "type": "agent", "data": {
                        "agent_id": data.agent_id,
                        "local_ip": "127.0.0.1",
                        "local_port": port,
                    } },
                    "enabled": true,
                    "alloc": null,
                    "firewall_id": null,
                    "proxy_protocol": null,
                }),
                Some(&key),
            )
            .await
            .map_err(|e| {
                if e.contains("NotAllowedWithReadOnly") {
                    // Keys from playit's own app can't make tunnels; one from our sign-in can.
                    READ_ONLY.to_owned()
                } else {
                    e
                }
            })?;
            info!("created a playit tunnel for {name} (port {port})");
        }
        if !running() {
            start_daemon().await?;
        }
        Ok::<_, String>(())
    }
    .await;
    match result {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

/// The error the app answers by signing in with moddedMC (sign-out, then claim).
const READ_ONLY: &str = "This playit sign-in can only use tunnels made on playit.gg. Sign in with moddedMC to let it make them.";

/// `POST /v1/playit/sign-out`: forget this machine's playit key (ours; playit's own app keeps
/// its copy) and stop the playit we started.
pub async fn sign_out() -> StatusCode {
    stop_started();
    let _ = tokio::fs::create_dir_all(dir()).await;
    match tokio::fs::write(secret_path(), "").await {
        Ok(()) => StatusCode::OK,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// A sign-in in progress: the link the user approves, or why it failed.
#[derive(Clone, Serialize)]
struct Claim {
    url: String,
    error: Option<String>,
}

static CLAIM: Mutex<Option<Claim>> = Mutex::new(None);

fn set_claim(claim: Option<Claim>) {
    *CLAIM.lock().unwrap_or_else(|e| e.into_inner()) = claim;
}

/// `GET /v1/playit`: whether this machine is signed in, and the pending sign-in link if any.
pub async fn status() -> Json<Value> {
    let claim = CLAIM.lock().unwrap_or_else(|e| e.into_inner()).clone();
    Json(json!({ "signed_in": secret().await.is_some(), "claim": claim, "running": running() }))
}

/// `POST /v1/playit/claim`: start signing in. The app opens the returned link; once the user
/// approves this machine on playit.gg, the key is saved and `signed_in` turns true.
pub async fn claim() -> Response {
    if secret().await.is_some() {
        return Json(json!({ "url": null })).into_response();
    }
    // A claim code is 5 random bytes in hex, like playit's own CLI makes.
    let code = format!(
        "{:010x}",
        RandomState::new().hash_one(std::process::id()) & 0xff_ffff_ffff
    );
    let url = format!("https://playit.gg/claim/{code}");
    set_claim(Some(Claim {
        url: url.clone(),
        error: None,
    }));
    tokio::spawn(async move {
        let fail = |error: String| {
            set_claim(Some(Claim {
                url: format!("https://playit.gg/claim/{code}"),
                error: Some(error),
            }));
        };
        let setup = json!({
            "code": code,
            "agent_type": "self-managed",
            "version": format!("moddedMC agent {}", env!("CARGO_PKG_VERSION")),
        });
        // The link stays valid for a while; give the user 15 minutes to approve it.
        for _ in 0..450 {
            match api::<String>("/claim/setup", setup.clone(), None)
                .await
                .as_deref()
            {
                Ok("UserAccepted") => {
                    match api::<Value>("/claim/exchange", json!({ "code": code }), None).await {
                        Ok(v) => match v["secret_key"].as_str().and_then(parse_secret) {
                            Some(key) => {
                                save_secret(&key).await;
                                info!("signed in to playit");
                                set_claim(None);
                            }
                            None => fail("playit.gg sent an unexpected reply.".into()),
                        },
                        Err(e) => fail(e),
                    }
                    return;
                }
                Ok("UserRejected") => return fail("The sign-in was declined on playit.gg.".into()),
                Ok(_) => {}
                Err(e) => warn!("playit sign-in: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        fail("The sign-in link expired. Try again.".into());
    });
    Json(json!({ "url": url })).into_response()
}

#[cfg(test)]
mod tests {
    use super::parse_secret;

    #[test]
    fn secret_files() {
        assert_eq!(
            parse_secret("secret_key = \"ab12\"\n").as_deref(),
            Some("ab12")
        );
        assert_eq!(parse_secret(" ab12\n").as_deref(), Some("ab12"));
        assert_eq!(parse_secret("secret_key = \"nothex\""), None);
        assert_eq!(parse_secret(""), None);
    }
}
