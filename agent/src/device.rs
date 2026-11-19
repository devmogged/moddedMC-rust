use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

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
    disk_total: u64,
    /// Whole-machine CPU load since the previous request, 0 to 100.
    cpu_percent: Option<f32>,
    agent_version: &'static str,
    tailscale_name: Option<String>,
}

/// CPU load is a difference between two samples, so the agent keeps the last one.
static CPU: Mutex<Option<System>> = Mutex::new(None);

fn sample_cpu() -> Option<f32> {
    let mut cpu = CPU.lock().unwrap_or_else(PoisonError::into_inner);
    match cpu.as_mut() {
        Some(system) => {
            system.refresh_cpu_usage();
            Some(system.global_cpu_usage())
        }
        None => {
            let mut system = System::new().ok()?;
            system.refresh_cpu_usage();
            *cpu = Some(system);
            None
        }
    }
}

async fn cpu_percent() -> Option<f32> {
    if let Some(load) = sample_cpu() {
        return Some(load);
    }
    // First request: take a second sample after the shortest interval sysinfo can measure.
    tokio::time::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(Duration::from_millis(200))).await;
    sample_cpu()
}

pub async fn get(State(app): State<Arc<App>>) -> Json<Device> {
    let (ram_total, ram_available) = System::new().map_or((0, 0), |mut system| {
        system.refresh_memory();
        (system.total_memory(), system.available_memory())
    });
    // The disk that holds the servers is the one that matters.
    let (disk_free, disk_total) = Disks::new_with_refreshed_list().map_or((0, 0), |disks| {
        (&disks)
            .into_iter()
            .filter(|disk| app.config.servers_root.starts_with(disk.mount_point()))
            .max_by_key(|disk| disk.mount_point().as_os_str().len())
            .map_or((0, 0), |disk| (disk.available_space(), disk.total_space()))
    });
    Json(Device {
        name: System::host_name().unwrap_or_default(),
        os: System::long_os_version().unwrap_or_else(|_| std::env::consts::OS.into()),
        arch: std::env::consts::ARCH,
        cpus: std::thread::available_parallelism().map_or(1, usize::from),
        ram_total,
        ram_available,
        disk_free,
        disk_total,
        cpu_percent: cpu_percent().await,
        agent_version: env!("CARGO_PKG_VERSION"),
        tailscale_name: crate::auth::self_dns_name().await,
    })
}
