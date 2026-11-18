use std::sync::Arc;

use axum::{Json, extract::State};
use serde::Serialize;
use sysinfo::{Disks, System};

use crate::App;

#[derive(Serialize)]
pub struct Device {
    name: String,
    os: String,
    arch: &'static str,
    cpus: usize,
    ram_total: u64,
    ram_available: u64,
    disk_free: u64,
    agent_version: &'static str,
    tailscale_name: Option<String>,
}

pub async fn get(State(app): State<Arc<App>>) -> Json<Device> {
    let (ram_total, ram_available) = System::new().map_or((0, 0), |mut system| {
        system.refresh_memory();
        (system.total_memory(), system.available_memory())
    });
    // The disk that holds the servers is the one that matters.
    let disk_free = Disks::new_with_refreshed_list().map_or(0, |disks| {
        (&disks)
            .into_iter()
            .filter(|disk| app.config.servers_root.starts_with(disk.mount_point()))
            .max_by_key(|disk| disk.mount_point().as_os_str().len())
            .map_or(0, sysinfo::Disk::available_space)
    });
    Json(Device {
        name: System::host_name().unwrap_or_default(),
        os: System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.into()),
        arch: std::env::consts::ARCH,
        cpus: std::thread::available_parallelism().map_or(1, usize::from),
        ram_total,
        ram_available,
        disk_free,
        agent_version: env!("CARGO_PKG_VERSION"),
        tailscale_name: crate::auth::self_dns_name().await,
    })
}
