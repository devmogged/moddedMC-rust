//! The server files this PC runs: the moddedMC server and the agent that manages it. They come
//! from the latest GitHub release (`runtime.json` lists them with checksums), live in the app's
//! local data folder at fixed paths, and are swapped in place when a newer release appears.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write as _;
use tauri::{AppHandle, Emitter, Manager};
use tokio::{io::AsyncWriteExt, net::TcpStream};

const MANIFEST_URL: &str =
    "https://github.com/devmogged/moddedMC-rust/releases/latest/download/runtime.json";
const MAX_MANIFEST: usize = 64 * 1024;
const AGENT_ADDR: &str = "127.0.0.1:25599";

#[derive(Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub version: String,
    pub files: Vec<RuntimeFile>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RuntimeFile {
    /// `server` or `agent`.
    pub role: String,
    /// `windows-x86_64`, `linux-x86_64`, `linux-aarch64`.
    pub target: String,
    pub url: String,
    /// Of the program itself, also when it is downloaded compressed.
    pub sha256: String,
    pub size: u64,
    /// The same program gzipped. Releases before v0.2.0 have only `url`.
    #[serde(default)]
    pub gz_url: Option<String>,
    #[serde(default)]
    pub gz_size: Option<u64>,
}

#[derive(Serialize)]
pub struct Status {
    pub installed: Option<String>,
    pub latest: Option<String>,
    /// Why the latest version couldn't be checked (offline, no release yet).
    pub error: Option<String>,
}

#[derive(Serialize, Clone)]
struct Progress {
    role: String,
    received: u64,
    total: u64,
}

fn target() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x86_64",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-aarch64",
        _ => "unsupported",
    }
}

fn dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("runtime"))
}

fn exe(dir: &Path, role: &str) -> PathBuf {
    dir.join(format!("moddedmc-{role}{}", std::env::consts::EXE_SUFFIX))
}

async fn installed(dir: &Path) -> Option<String> {
    let text = tokio::fs::read_to_string(dir.join("version.txt"))
        .await
        .ok()?;
    let version = text.trim().to_owned();
    (!version.is_empty() && exe(dir, "server").is_file() && exe(dir, "agent").is_file())
        .then_some(version)
}

async fn manifest() -> Result<Manifest, String> {
    // MODDEDMC_RUNTIME_MANIFEST points at a test copy of a release (scripts/fresh-install-test.ps1).
    let url = std::env::var("MODDEDMC_RUNTIME_MANIFEST").unwrap_or_else(|_| MANIFEST_URL.into());
    let response = crate::agent::http()
        .get(url)
        .send()
        .await
        .map_err(|_| "Can't reach GitHub. Are you online?".to_string())?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Err("No release has been published yet.".into());
    }
    if !response.status().is_success() {
        return Err(format!("GitHub answered {}.", response.status()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_MANIFEST {
        return Err("The release list is too large.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "The release list is damaged.".into())
}

/// Development builds with no release to download from install the server and agent built in
/// this checkout instead (`MODDEDMC_DEV_SERVER` / `MODDEDMC_DEV_AGENT` override the paths).
#[cfg(debug_assertions)]
async fn install_local(dir: &Path) -> Option<String> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target = option_env!("CARGO_TARGET_DIR").map_or_else(|| repo.join("target"), PathBuf::from);
    let pick = |var: &str, candidates: Vec<PathBuf>| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .into_iter()
            .chain(candidates)
            .find(|p| p.is_file())
    };
    let server = pick(
        "MODDEDMC_DEV_SERVER",
        vec![
            repo.join("engine/target/pack/pack-dev.exe"),
            repo.join("engine/target/release/pack-dev.exe"),
            target.join("../engine/target/pack/pack-dev.exe"),
        ],
    )?;
    let agent = pick(
        "MODDEDMC_DEV_AGENT",
        vec![target.join("debug/moddedmc-agent.exe")],
    )?;
    tokio::fs::create_dir_all(dir).await.ok()?;
    tokio::fs::copy(server, exe(dir, "server")).await.ok()?;
    tokio::fs::copy(agent, exe(dir, "agent")).await.ok()?;
    tokio::fs::write(dir.join("version.txt"), "dev")
        .await
        .ok()?;
    Some("dev".into())
}

#[tauri::command]
pub async fn runtime_status(app: AppHandle) -> Result<Status, String> {
    let dir = dir(&app)?;
    #[allow(unused_mut)]
    let mut installed = installed(&dir).await;
    let latest = manifest().await;
    #[cfg(debug_assertions)]
    if installed.is_none() && latest.is_err() {
        installed = install_local(&dir).await;
    }
    Ok(match latest {
        Ok(m) => Status {
            installed,
            latest: Some(m.version),
            error: None,
        },
        Err(error) => Status {
            installed,
            latest: None,
            error: Some(error),
        },
    })
}

/// Download this PC's server and agent from the latest release, verify them and put them in
/// place. A running copy keeps running; the new one is used from its next start.
#[tauri::command]
pub async fn runtime_install(app: AppHandle) -> Result<String, String> {
    let dir = dir(&app)?;
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;
    let manifest = manifest().await?;
    for role in ["server", "agent"] {
        let file = manifest
            .files
            .iter()
            .find(|f| f.role == role && f.target == target())
            .ok_or_else(|| format!("This release has no {role} for {}.", target()))?;
        let dest = exe(&dir, role);
        if sha256_of(&dest).await.as_deref() != Some(&*file.sha256.to_ascii_lowercase()) {
            download(&app, file, &dest).await?;
        }
    }
    tokio::fs::write(dir.join("version.txt"), &manifest.version)
        .await
        .map_err(|e| e.to_string())?;
    Ok(manifest.version)
}

/// The hex SHA-256 of an installed file, or `None` if it's missing.
async fn sha256_of(path: &Path) -> Option<String> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut file = std::fs::File::open(path).ok()?;
        let mut hash = Sha256::new();
        std::io::copy(&mut file, &mut hash).ok()?;
        Some(hex(&hash.finalize()))
    })
    .await
    .ok()?
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Turns downloaded chunks into the program's bytes: gunzips them when the download is the .gz.
enum Unpack {
    Plain,
    Gzip(Box<flate2::write::GzDecoder<Vec<u8>>>),
}

impl Unpack {
    fn feed(&mut self, chunk: &[u8]) -> std::io::Result<Vec<u8>> {
        match self {
            Unpack::Plain => Ok(chunk.to_vec()),
            Unpack::Gzip(gz) => {
                gz.write_all(chunk)?;
                Ok(std::mem::take(gz.get_mut()))
            }
        }
    }

    fn finish(self) -> std::io::Result<Vec<u8>> {
        match self {
            Unpack::Plain => Ok(Vec::new()),
            Unpack::Gzip(gz) => gz.finish(),
        }
    }
}

async fn download(app: &AppHandle, file: &RuntimeFile, dest: &Path) -> Result<(), String> {
    let (url, total, mut unpack) = match (&file.gz_url, file.gz_size) {
        (Some(url), Some(size)) => (
            url,
            size,
            Unpack::Gzip(Box::new(flate2::write::GzDecoder::new(Vec::new()))),
        ),
        _ => (&file.url, file.size, Unpack::Plain),
    };
    let mut response = crate::agent::http()
        .get(url)
        .timeout(Duration::from_secs(30 * 60))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("Downloading the {}: {e}", file.role))?;
    let part = dest.with_extension("part");
    let mut out = tokio::fs::File::create(&part)
        .await
        .map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut written = 0u64;
    let damaged = || format!("The {} download is damaged. Try again.", file.role);
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|e| format!("Downloading the {}: {e}", file.role))?;
        let bytes = match &chunk {
            Some(chunk) => {
                received += chunk.len() as u64;
                unpack.feed(chunk)
            }
            None => std::mem::replace(&mut unpack, Unpack::Plain).finish(),
        }
        .map_err(|_| damaged())?;
        written += bytes.len() as u64;
        if received > total || written > file.size {
            return Err(format!(
                "The {} download is larger than expected.",
                file.role
            ));
        }
        hash.update(&bytes);
        out.write_all(&bytes).await.map_err(|e| e.to_string())?;
        if chunk.is_none() {
            break;
        }
        let _ = app.emit(
            "runtime-progress",
            Progress {
                role: file.role.clone(),
                received,
                total,
            },
        );
    }
    out.flush().await.map_err(|e| e.to_string())?;
    drop(out);
    if hex(&hash.finalize()) != file.sha256.to_ascii_lowercase() {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(damaged());
    }
    // Windows can rename a running program but not overwrite it: move the old one aside first.
    let old = dest.with_extension("old");
    let _ = tokio::fs::remove_file(&old).await;
    if dest.exists() {
        tokio::fs::rename(dest, &old)
            .await
            .map_err(|e| e.to_string())?;
    }
    tokio::fs::rename(&part, dest)
        .await
        .map_err(|e| e.to_string())?;
    let _ = tokio::fs::remove_file(&old).await;
    Ok(())
}

/// The installed server program, for new servers on this PC.
#[tauri::command]
pub async fn runtime_server_exe(app: AppHandle) -> Result<String, String> {
    let dir = dir(&app)?;
    if installed(&dir).await.is_none() {
        return Err("The server files aren't installed yet.".into());
    }
    Ok(exe(&dir, "server").to_string_lossy().into_owned())
}

async fn agent_up() -> bool {
    tokio::time::timeout(Duration::from_millis(500), TcpStream::connect(AGENT_ADDR))
        .await
        .is_ok_and(|r| r.is_ok())
}

/// Start this PC's agent if it isn't running. It keeps running after the app closes, so servers
/// stay up; its log goes to `runtime/agent.log`.
#[tauri::command]
pub async fn agent_ensure(app: AppHandle) -> Result<(), String> {
    if agent_up().await {
        return Ok(());
    }
    let dir = dir(&app)?;
    let agent = exe(&dir, "agent");
    if !agent.is_file() {
        return Err("The server files aren't installed yet.".into());
    }
    let log = std::fs::File::create(dir.join("agent.log")).map_err(|e| e.to_string())?;
    let err = log.try_clone().map_err(|e| e.to_string())?;
    let mut command = std::process::Command::new(&agent);
    command
        .current_dir(&dir)
        .stdin(std::process::Stdio::null())
        .stdout(log)
        .stderr(err);
    if let Ok(resources) = app.path().resource_dir() {
        let playit = resources.join("playitd.exe");
        if playit.is_file() {
            command.env("MODDEDMC_PLAYIT_DAEMON", playit);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|e| format!("Can't start the agent: {e}"))?;
    for _ in 0..40 {
        if agent_up().await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Err("The agent started but isn't answering. See runtime/agent.log.".into())
}

/// Ask this PC's agent to stop every server (and the playit it started) and exit. Used when the
/// app quits, so nothing keeps running behind it.
pub async fn agent_shutdown() {
    let _ = crate::agent::http()
        .post(format!("http://{AGENT_ADDR}/v1/shutdown"))
        .timeout(Duration::from_secs(3))
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_download_unpacks_across_chunks() {
        let program: Vec<u8> = (0..200_000u32)
            .flat_map(|i| (i % 251).to_le_bytes())
            .collect();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        gz.write_all(&program).unwrap();
        let packed = gz.finish().unwrap();
        let mut unpack = Unpack::Gzip(Box::new(flate2::write::GzDecoder::new(Vec::new())));
        let mut out = Vec::new();
        for chunk in packed.chunks(1000) {
            out.extend(unpack.feed(chunk).unwrap());
        }
        out.extend(unpack.finish().unwrap());
        assert_eq!(out, program);
    }

    #[tokio::test]
    async fn sha256_of_matches_and_misses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a");
        assert_eq!(sha256_of(&path).await, None);
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_of(&path).await.unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
