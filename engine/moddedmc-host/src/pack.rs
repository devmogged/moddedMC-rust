//! The mods' resource pack. Every `.mmc` may carry an `assets/` folder; at startup the host merges
//! them (later files in name order win), writes one pack to `mods/.cache/`, serves it over HTTP
//! and offers it to each joining player. A stock client then shows the mods' textures, models,
//! sounds and translations.
//!
//! `config/moddedmc/pack.txt` sets `bind=` (where the HTTP server listens, default
//! `0.0.0.0:25580`) and `url=` (what players download, default `http://127.0.0.1:25580/pack.zip`;
//! set it to an address players can reach).

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    sync::{Arc, OnceLock},
};

use pumpkin_core::{
    entity::player::Player,
    plugin::{BoxFuture, EventHandler, player::player_join::PlayerJoinEvent},
    server::Server,
};
use pumpkin_protocol::java::client::play::CAddResourcePack;
use pumpkin_util::text::TextComponent;
use sha1::{Digest, Sha1};

const SETTINGS: &str = "config/moddedmc/pack.txt";

struct Pack {
    url: String,
    sha1: String,
    id: uuid::Uuid,
}

static PACK: OnceLock<Pack> = OnceLock::new();

/// Builds and serves the pack if any mod in `dir` has assets.
pub fn build(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut mmcs: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "mmc"))
        .collect();
    mmcs.sort();
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for path in &mmcs {
        if let Err(e) = add_assets(path, &mut files) {
            tracing::warn!("moddedmc: assets of {}: {e}", path.display());
        }
    }
    if files.is_empty() {
        return;
    }
    let zip = match write_zip(&files) {
        Ok(zip) => zip,
        Err(e) => {
            tracing::error!("moddedmc: could not build the resource pack: {e}");
            return;
        }
    };
    let digest = Sha1::digest(&zip);
    let sha1 = hex::encode(digest);
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    let (bind, url) = settings();
    let listener = match TcpListener::bind(&bind) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("moddedmc: resource pack server cannot listen on {bind}: {e}");
            return;
        }
    };
    let _ = fs::write(dir.join(".cache").join("pack.zip"), &zip);
    tracing::info!(
        "moddedmc: resource pack with {} files ({} bytes, sha1 {sha1}) at {url}",
        files.len(),
        zip.len()
    );
    let zip = Arc::new(zip);
    std::thread::spawn(move || serve(&listener, &zip));
    let _ = PACK.set(Pack {
        url,
        sha1,
        id: uuid::Uuid::from_bytes(id),
    });
}

fn settings() -> (String, String) {
    let text = fs::read_to_string(SETTINGS).unwrap_or_default();
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('=').map(str::trim))
            .map(str::to_owned)
    };
    let bind = get("bind").unwrap_or_else(|| "0.0.0.0:25580".into());
    let url = get("url").unwrap_or_else(|| "http://127.0.0.1:25580/pack.zip".into());
    if text.is_empty() {
        let _ = fs::create_dir_all("config/moddedmc");
        let _ = fs::write(SETTINGS, format!("bind={bind}\nurl={url}\n"));
        tracing::warn!("moddedmc: set url= in {SETTINGS} to an address players can reach");
    }
    (bind, url)
}

fn add_assets(mmc: &Path, files: &mut BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(fs::File::open(mmc).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        if !name.starts_with("assets/") || entry.is_dir() || name.contains("..") {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if files.insert(name.clone(), bytes).is_some() {
            tracing::warn!(
                "moddedmc: {} replaces {name} from an earlier mod",
                mmc.display()
            );
        }
    }
    Ok(())
}

fn write_zip(files: &BTreeMap<String, Vec<u8>>) -> zip::result::ZipResult<Vec<u8>> {
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    // Accepted by every client version the server speaks; newer ones read the format range.
    out.start_file("pack.mcmeta", options)?;
    out.write_all(
        br#"{"pack":{"description":"moddedMC mods","pack_format":75,"supported_formats":[0,999],"min_format":0,"max_format":999}}"#,
    )?;
    for (name, bytes) in files {
        out.start_file(name.as_str(), options)?;
        out.write_all(bytes)?;
    }
    Ok(out.finish()?.into_inner())
}

/// A minimal HTTP/1.0 server: every GET gets the pack.
fn serve(listener: &TcpListener, zip: &[u8]) {
    for stream in listener.incoming().flatten() {
        let mut stream = stream;
        let mut request = [0u8; 1024];
        let n = stream.read(&mut request).unwrap_or(0);
        let response = if request[..n].starts_with(b"GET ") {
            let head = format!(
                "HTTP/1.0 200 OK\r\nContent-Type: application/zip\r\nContent-Length: {}\r\n\r\n",
                zip.len()
            );
            [head.as_bytes(), zip].concat()
        } else {
            b"HTTP/1.0 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n".to_vec()
        };
        let _ = stream.write_all(&response);
    }
}

pub struct JoinHandler;

impl EventHandler<PlayerJoinEvent> for JoinHandler {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerJoinEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move { offer(&event.player) })
    }
}

fn offer(player: &Player) {
    let Some(pack) = PACK.get() else { return };
    player.try_send_client_packet(&CAddResourcePack::new(
        &pack.id,
        &pack.url,
        &pack.sha1,
        false,
        Some(TextComponent::text("Textures for this server's mods")),
    ));
    tracing::info!(
        "moddedmc: offered the resource pack to {}",
        player.gameprofile.name
    );
}
