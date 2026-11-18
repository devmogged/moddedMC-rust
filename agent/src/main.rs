//! `moddedmc-agent`: runs on every machine that hosts servers and lets the owner's app manage
//! them. See docs/DEVICES.md for the contract.

mod auth;
mod builds;
mod config;
mod device;
mod external;
mod pumpkin_config;
mod servers;

use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use tokio::net::TcpListener;
use tracing::{error, info, warn};

pub struct App {
    pub config: config::Config,
    pub servers: servers::Servers,
    pub auth: auth::Auth,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "moddedmc_agent=info".into()),
        )
        .init();

    let config = match config::Config::load_or_create().await {
        Ok(config) => config,
        Err(err) => {
            error!("cannot load agent config: {err}");
            std::process::exit(1);
        }
    };
    let app = Arc::new(App {
        servers: servers::Servers::new(config.servers_root.clone(), config.find_in.clone()),
        auth: auth::Auth::new(config.owner.clone()),
        config,
    });

    let router = Router::new()
        .route("/v1/device", get(device::get))
        .route("/v1/servers", get(servers::list).post(servers::create))
        .route("/v1/servers/{name}/start", post(servers::start))
        .route("/v1/servers/{name}/stop", post(servers::stop))
        .route("/v1/servers/{name}/restart", post(servers::restart))
        .route("/v1/servers/{name}/stats", get(servers::stats))
        .route("/v1/servers/{name}/console", get(servers::console))
        .route("/v1/servers/{name}/deploy", post(servers::deploy))
        .route("/v1/builds", get(builds::list))
        .route(
            "/v1/builds/{id}",
            post(builds::upload).layer(DefaultBodyLimit::max(builds::MAX_UPLOAD)),
        )
        .layer(middleware::from_fn_with_state(
            app.clone(),
            auth::require_owner,
        ))
        .with_state(app.clone());

    let mut addrs = vec![SocketAddr::from(([127, 0, 0, 1], app.config.port))];
    match auth::tailnet_ipv4().await {
        Some(ip) => addrs.push(SocketAddr::new(ip.into(), app.config.port)),
        None => info!(
            "no Tailscale address; listening on loopback only (use SSH to reach this machine)"
        ),
    }

    let mut tasks = Vec::new();
    for addr in addrs {
        let listener = match TcpListener::bind(addr).await {
            Ok(listener) => listener,
            Err(err) => {
                warn!("cannot listen on {addr}: {err}");
                continue;
            }
        };
        info!("listening on http://{addr}");
        let service = router
            .clone()
            .into_make_service_with_connect_info::<SocketAddr>();
        tasks.push(tokio::spawn(async move {
            if let Err(err) = axum::serve(listener, service).await {
                error!("listener {addr} stopped: {err}");
            }
        }));
    }
    if tasks.is_empty() {
        error!("no address to listen on");
        std::process::exit(1);
    }

    if tokio::signal::ctrl_c().await.is_ok() {
        info!("stopping managed servers");
        app.servers.stop_all().await;
    }
}
