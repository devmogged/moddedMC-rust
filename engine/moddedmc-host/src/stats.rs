//! Tick-time percentiles and content-host stage timings for benchmarks.
//! Enabled by `MODDEDMC_TICK_STATS=<window ticks>`; logs one `tick-stats` and one
//! `host-stats` line per window.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::Instant,
};

use pumpkin_core::{
    plugin::{BoxFuture, EventHandler, EventPriority, server::server_tick_end::ServerTickEndEvent},
    server::Server,
};

/// Time spent in each content-host stage since the last report.
pub struct Timing {
    discover_ns: AtomicU64,
    collect_ns: AtomicU64,
    call_ns: AtomicU64,
    apply_ns: AtomicU64,
    entries: AtomicU64,
}

pub static TIMING: Timing = Timing {
    discover_ns: AtomicU64::new(0),
    collect_ns: AtomicU64::new(0),
    call_ns: AtomicU64::new(0),
    apply_ns: AtomicU64::new(0),
    entries: AtomicU64::new(0),
};

fn elapsed_ns(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

impl Timing {
    pub fn add_discover(&self, started: Instant) {
        self.discover_ns.fetch_add(elapsed_ns(started), Relaxed);
    }
    pub fn add_collect(&self, started: Instant, entries: usize) {
        self.collect_ns.fetch_add(elapsed_ns(started), Relaxed);
        self.entries.fetch_add(entries as u64, Relaxed);
    }
    pub fn add_call(&self, started: Instant) {
        self.call_ns.fetch_add(elapsed_ns(started), Relaxed);
    }
    pub fn add_apply(&self, started: Instant) {
        self.apply_ns.fetch_add(elapsed_ns(started), Relaxed);
    }
}

struct StatsHandler {
    window: usize,
    samples: Mutex<Vec<i64>>,
}

pub fn attach(server: &Arc<Server>) {
    let Some(window) = std::env::var("MODDEDMC_TICK_STATS")
        .ok()
        .and_then(|v| v.parse().ok())
    else {
        return;
    };
    let handler = StatsHandler {
        window,
        samples: Mutex::new(Vec::with_capacity(window)),
    };
    server.plugin_manager.register::<ServerTickEndEvent, _>(
        Arc::new(handler),
        EventPriority::Lowest,
        true,
    );
}

impl EventHandler<ServerTickEndEvent> for StatsHandler {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut ServerTickEndEvent,
    ) -> BoxFuture<'a, ()> {
        let mut samples = self
            .samples
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        samples.push(event.duration_nanos);
        if samples.len() >= self.window {
            samples.sort_unstable();
            let ms = |q: f64| {
                let i = ((samples.len() - 1) as f64 * q).round() as usize;
                samples[i] as f64 / 1e6
            };
            let mean = samples.iter().sum::<i64>() as f64 / samples.len() as f64 / 1e6;
            tracing::info!(
                "tick-stats ticks={} mean_ms={mean:.3} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3}",
                samples.len(),
                ms(0.5),
                ms(0.95),
                ms(0.99),
                ms(1.0)
            );
            let per_tick = |ns: &AtomicU64| ns.swap(0, Relaxed) as f64 / samples.len() as f64 / 1e6;
            tracing::info!(
                "host-stats per_tick_ms discover={:.3} collect={:.3} call={:.3} apply={:.3} entries_per_tick={:.0}",
                per_tick(&TIMING.discover_ns),
                per_tick(&TIMING.collect_ns),
                per_tick(&TIMING.call_ns),
                per_tick(&TIMING.apply_ns),
                TIMING.entries.swap(0, Relaxed) as f64 / samples.len() as f64
            );
            samples.clear();
        }
        Box::pin(async {})
    }
}
