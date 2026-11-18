//! Calls to a device's agent. Going through the backend keeps the webview free of CORS and
//! lets SSH devices share one tunnel.

use std::{sync::LazyLock, time::Duration};

use serde_json::Value;
use tauri::State;

use crate::{devices::Tunnels, store::Store};

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(15))
        .build()
        .unwrap_or_default()
});

pub fn http() -> &'static reqwest::Client {
    &HTTP
}

/// The agent's `host:port` for a device, and the device's display name for errors.
pub async fn endpoint(
    store: &Store,
    tunnels: &Tunnels,
    device: &str,
) -> Result<(String, String), String> {
    let device = store.device(device).ok_or("That device is gone.")?;
    let endpoint = tunnels.endpoint(&device).await?;
    Ok((endpoint, device.name))
}

/// `GET` without a body, `POST` with one. Returns the agent's JSON, or `null` for an empty body.
#[tauri::command]
pub async fn agent(
    store: State<'_, Store>,
    tunnels: State<'_, Tunnels>,
    device: String,
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    if !path.starts_with("/v1/") || path.contains("..") {
        return Err("bad agent path".into());
    }
    let (endpoint, name) = endpoint(&store, &tunnels, &device).await?;
    let url = format!("http://{endpoint}{path}");
    let request = match method.as_str() {
        "GET" => HTTP.get(&url),
        "POST" => HTTP.post(&url).json(&body.unwrap_or(Value::Null)),
        _ => return Err("bad agent method".into()),
    };
    let response = request.send().await.map_err(|err| {
        if err.is_connect() || err.is_timeout() {
            format!("Can't reach {name}. Is the agent running there?")
        } else {
            err.to_string()
        }
    })?;
    let status = response.status();
    let text = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(if text.is_empty() {
            status.to_string()
        } else {
            text
        });
    }
    if text.is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)))
}
