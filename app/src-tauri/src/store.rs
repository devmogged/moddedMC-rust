//! Devices and friends' servers, kept as one JSON file in the app data dir.

use std::{path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    Local,
    Tailscale,
    Ssh,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub route: Route,
    /// Tailscale name or IP, or the SSH destination (`user@host`).
    #[serde(default)]
    pub host: String,
    #[serde(default = "agent_port")]
    pub port: u16,
}

pub fn agent_port() -> u16 {
    25599
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Friend {
    pub id: String,
    pub name: String,
    pub address: String,
}

#[derive(Serialize, Deserialize, Default)]
struct Data {
    #[serde(default)]
    devices: Vec<Device>,
    #[serde(default)]
    friends: Vec<Friend>,
}

pub struct Store {
    path: PathBuf,
    data: Mutex<Data>,
}

impl Store {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("app.json");
        let data = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    fn data(&self) -> std::sync::MutexGuard<'_, Data> {
        self.data.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, data: &Data) -> Result<(), String> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_vec_pretty(data).map_err(|e| e.to_string())?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }

    /// The local machine first, then the devices the owner added.
    pub fn devices(&self) -> Vec<Device> {
        let mut devices = vec![Device {
            id: "local".into(),
            name: "This PC".into(),
            route: Route::Local,
            host: String::new(),
            port: agent_port(),
        }];
        devices.extend(self.data().devices.iter().cloned());
        devices
    }

    pub fn device(&self, id: &str) -> Option<Device> {
        self.devices().into_iter().find(|d| d.id == id)
    }

    pub fn add_device(&self, device: Device) -> Result<(), String> {
        let mut data = self.data();
        data.devices.retain(|d| d.id != device.id);
        data.devices.push(device);
        self.save(&data)
    }

    pub fn remove_device(&self, id: &str) -> Result<(), String> {
        let mut data = self.data();
        data.devices.retain(|d| d.id != id);
        self.save(&data)
    }

    pub fn friends(&self) -> Vec<Friend> {
        self.data().friends.clone()
    }

    pub fn add_friend(&self, friend: Friend) -> Result<(), String> {
        let mut data = self.data();
        data.friends.retain(|f| f.id != friend.id);
        data.friends.push(friend);
        self.save(&data)
    }

    pub fn remove_friend(&self, id: &str) -> Result<(), String> {
        let mut data = self.data();
        data.friends.retain(|f| f.id != id);
        self.save(&data)
    }
}

/// A stable id from a display name: lowercase letters and digits joined by `-`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}
