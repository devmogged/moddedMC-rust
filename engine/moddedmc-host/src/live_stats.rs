//! Live server health for the device agent: with `MODDEDMC_STATS=1`, writes `stats.json` in
//! the server folder every two seconds. A thread of its own, so the tick never waits on disk.

use std::{
    sync::{Arc, PoisonError, Weak},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use pumpkin_core::server::Server;
use serde_json::json;

const INTERVAL: Duration = Duration::from_secs(2);
const FILE: &str = "stats.json";

pub fn attach(server: &Arc<Server>) {
    if std::env::var_os("MODDEDMC_STATS").is_none_or(|v| v != "1") {
        return;
    }
    let server = Arc::downgrade(server);
    let spawned = thread::Builder::new()
        .name("moddedmc-stats".into())
        .spawn(move || run(&server));
    if let Err(err) = spawned {
        tracing::warn!("moddedmc: no live stats: {err}");
    }
}

fn run(server: &Weak<Server>) {
    while let Some(server) = server.upgrade() {
        let snapshot = snapshot(&server);
        drop(server);
        let tmp = format!("{FILE}.tmp");
        let written =
            std::fs::write(&tmp, snapshot.to_string()).and_then(|()| std::fs::rename(&tmp, FILE));
        if let Err(err) = written {
            tracing::warn!("moddedmc: can't write {FILE}: {err}");
            return;
        }
        thread::sleep(INTERVAL);
    }
}

fn snapshot(server: &Server) -> serde_json::Value {
    let max_tick_ns = server
        .tick_times_nanos
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .copied()
        .max()
        .unwrap_or(0);
    let mspt = server.get_mspt();
    let tickrate = f64::from(server.tick_rate_manager.tickrate());
    // A tick shorter than its budget sleeps out the rest, so TPS tops out at the tick rate.
    let tps = if mspt > 0.0 {
        (1000.0 / mspt).min(tickrate)
    } else {
        0.0
    };
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    json!({
        "at": at,
        "mspt": mspt,
        "mspt_max": max_tick_ns as f64 / 1e6,
        "tps": tps,
        "tickrate": tickrate,
        "players": server.get_player_count(),
        "max_players": server.max_players(),
    })
}
