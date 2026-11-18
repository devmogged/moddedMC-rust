//! Where each device's agent is reachable. Local and Tailscale devices are direct; SSH
//! devices get an `ssh -N -L` tunnel to the agent's loopback port, opened on first use.

use std::{collections::HashMap, net::TcpListener, process::Stdio, time::Duration};

use serde::Deserialize;
use tauri::State;
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

use crate::store::{self, Device, Route, Store};

#[derive(Default)]
pub struct Tunnels {
    open: Mutex<HashMap<String, (u16, Child)>>,
}

impl Tunnels {
    /// The `host:port` that reaches this device's agent from here.
    pub async fn endpoint(&self, device: &Device) -> Result<String, String> {
        match device.route {
            Route::Local => Ok(format!("127.0.0.1:{}", device.port)),
            Route::Tailscale => Ok(format!("{}:{}", device.host, device.port)),
            Route::Ssh => self
                .ssh(device)
                .await
                .map(|port| format!("127.0.0.1:{port}")),
        }
    }

    async fn ssh(&self, device: &Device) -> Result<u16, String> {
        let mut open = self.open.lock().await;
        if let Some((port, child)) = open.get_mut(&device.id) {
            if matches!(child.try_wait(), Ok(None)) {
                return Ok(*port);
            }
            open.remove(&device.id);
        }
        let port = free_port()?;
        let mut command = Command::new("ssh");
        command
            .args([
                "-N",
                "-o",
                "BatchMode=yes",
                "-o",
                "ExitOnForwardFailure=yes",
            ])
            .args(["-o", "ServerAliveInterval=15", "-L"])
            .arg(format!("{port}:127.0.0.1:{}", device.port))
            .arg(&device.host)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        let mut child = command
            .spawn()
            .map_err(|_| "Can't run ssh. Is OpenSSH installed?".to_string())?;
        // Wait for the forward to accept connections, or for ssh to give up.
        for _ in 0..80 {
            if let Ok(Some(_)) = child.try_wait() {
                return Err(format!(
                    "Can't open SSH to {}. Check that key login works.",
                    device.host
                ));
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", port))
                .await
                .is_ok()
            {
                open.insert(device.id.clone(), (port, child));
                return Ok(port);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(format!("SSH to {} timed out.", device.host))
    }

    pub async fn close(&self, id: &str) {
        self.open.lock().await.remove(id);
    }
}

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn free_port() -> Result<u16, String> {
    TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map(|a| a.port())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn devices_list(store: State<'_, Store>) -> Vec<Device> {
    store.devices()
}

#[derive(Deserialize)]
pub struct NewDevice {
    name: String,
    route: Route,
    host: String,
    port: Option<u16>,
}

#[tauri::command]
pub fn device_add(store: State<'_, Store>, device: NewDevice) -> Result<Device, String> {
    let name = device.name.trim();
    let host = device.host.trim();
    if name.is_empty() || host.is_empty() {
        return Err("A device needs a name and an address.".into());
    }
    if device.route == Route::Local {
        return Err("This PC is already listed.".into());
    }
    let mut id = store::slug(name);
    if id.is_empty() || id == "local" {
        id = format!("device-{}", store::slug(host));
    }
    let device = Device {
        id,
        name: name.to_string(),
        route: device.route,
        host: host.to_string(),
        port: device.port.unwrap_or_else(store::agent_port),
    };
    store.add_device(device.clone())?;
    Ok(device)
}

#[tauri::command]
pub async fn device_remove(
    store: State<'_, Store>,
    tunnels: State<'_, Tunnels>,
    id: String,
) -> Result<(), String> {
    tunnels.close(&id).await;
    store.remove_device(&id)
}
