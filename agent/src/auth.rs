//! Who may call the agent: loopback always (that includes SSH tunnels), tailnet peers only if
//! Tailscale says they are the owner. No tokens.

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::{process::Command, sync::Mutex};
use tracing::warn;

use crate::App;

const WHOIS_TTL: Duration = Duration::from_secs(60);

pub struct Auth {
    owner: Option<String>,
    whois: Mutex<HashMap<IpAddr, (Option<String>, Instant)>>,
}

impl Auth {
    pub fn new(owner: Option<String>) -> Self {
        Self {
            owner,
            whois: Mutex::new(HashMap::new()),
        }
    }

    async fn is_owner(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() {
            return true;
        }
        let Some(owner) = &self.owner else {
            return false;
        };
        let mut cache = self.whois.lock().await;
        let login = match cache.get(&ip) {
            Some((login, at)) if at.elapsed() < WHOIS_TTL => login.clone(),
            _ => {
                let login = whois_login(ip).await;
                cache.insert(ip, (login.clone(), Instant::now()));
                login
            }
        };
        login.as_deref() == Some(owner.as_str())
    }
}

pub async fn require_owner(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    if app.auth.is_owner(peer.ip()).await {
        next.run(request).await
    } else {
        warn!("refused {} (not the owner)", peer.ip());
        (
            StatusCode::FORBIDDEN,
            "only the owner can control this machine",
        )
            .into_response()
    }
}

async fn tailscale(args: &[&str]) -> Option<String> {
    let output = Command::new("tailscale").args(args).output().await.ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// The Tailscale login of the machine at `ip`, if it is a tailnet peer.
async fn whois_login(ip: IpAddr) -> Option<String> {
    let json = tailscale(&["whois", "--json", &ip.to_string()]).await?;
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    value["UserProfile"]["LoginName"]
        .as_str()
        .map(str::to_owned)
}

pub async fn tailnet_ipv4() -> Option<Ipv4Addr> {
    tailscale(&["ip", "-4"])
        .await?
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

/// The Tailscale login of this machine.
pub async fn self_login() -> Option<String> {
    whois_login(tailnet_ipv4().await?.into()).await
}

/// This machine's name on the tailnet.
pub async fn self_dns_name() -> Option<String> {
    let json = tailscale(&["status", "--json", "--self", "--peers=false"]).await?;
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    value["Self"]["DNSName"]
        .as_str()
        .map(|name| name.trim_end_matches('.').to_owned())
}
