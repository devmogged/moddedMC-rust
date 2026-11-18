//! `moddedmc run`: starts Pumpkin in a server folder, forwards the console, and opens a tunnel.
//!
//!     moddedmc run [--dir server] [--server path\to\pumpkin.exe] [--tunnel none|playit|tailscale]
//!
//! Mods: copy `.wasm` files into `<dir>\plugins`.
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::{env, fs, thread};

// shortcut: assumes Pumpkin's default Java port, read networking.java.address once users change it.
const PORT: u16 = 25565;
// Written only into a folder without a config; Pumpkin fills in every other default on start.
const NEW_CONFIG: &str = "# Written by moddedmc. Telemetry is off unless you turn it on.\n[telemetry]\nenabled = false\n";

fn main() {
    if let Err(err) = run(env::args().skip(1).collect()) {
        eprintln!("moddedmc: {err}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    let usage = "usage: moddedmc run [--dir DIR] [--server PUMPKIN_EXE] [--tunnel none|playit|tailscale]";
    let mut args = args.into_iter();
    if args.next().as_deref() != Some("run") {
        return Err(usage.into());
    }
    let (mut dir, mut server, mut tunnel) = (PathBuf::from("server"), None, String::from("none"));
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(usage)?;
        match flag.as_str() {
            "--dir" => dir = value.into(),
            "--server" => server = Some(PathBuf::from(value)),
            "--tunnel" => tunnel = value,
            _ => return Err(usage.into()),
        }
    }
    let server = match server {
        Some(path) => path,
        None => env::current_exe()
            .map_err(|e| e.to_string())?
            .with_file_name(format!("pumpkin{}", env::consts::EXE_SUFFIX)),
    };
    if !server.is_file() {
        return Err(format!("no Pumpkin server at {} (pass --server)", server.display()));
    }

    fs::create_dir_all(dir.join("plugins")).map_err(|e| format!("{}: {e}", dir.display()))?;
    let config = dir.join("pumpkin.toml");
    if !config.exists() {
        fs::write(&config, NEW_CONFIG).map_err(|e| format!("{}: {e}", config.display()))?;
    }

    // Start the tunnel first so a bad setup fails before the world loads.
    let mut agent = match tunnel.as_str() {
        "none" => None,
        "playit" => Some(start_playit(&dir)?),
        "tailscale" => {
            println!("[moddedmc] players on your tailnet join {}:{PORT}", tailscale_ip()?);
            None
        }
        other => return Err(format!("unknown tunnel '{other}'")),
    };

    let mut pumpkin = Command::new(&server)
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting {}: {e}", server.display()))?;
    let mut console = pumpkin.stdin.take().expect("piped stdin");
    // Raw bytes, so any input encoding passes through. Ctrl+C reaches Pumpkin and playit
    // directly (same console).
    thread::spawn(move || io::copy(&mut io::stdin(), &mut console));
    let status = pumpkin.wait().map_err(|e| e.to_string());
    if let Some(agent) = agent.as_mut() {
        let _ = agent.kill();
        let _ = agent.wait();
    }
    match status? {
        s if s.success() => Ok(()),
        s => Err(format!("server exited with {s}")),
    }
}

fn tool(name: &str, windows_default: &str) -> PathBuf {
    let default = Path::new(windows_default);
    if cfg!(windows) && default.is_file() { default.into() } else { name.into() }
}

fn output(exe: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new(exe).args(args).output().map_err(|e| format!("{}: {e}", exe.display()))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{} {}: {}", exe.display(), args.join(" "), err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn tailscale_ip() -> Result<String, String> {
    let exe = tool("tailscale", r"C:\Program Files\Tailscale\tailscale.exe");
    let ips = output(&exe, &["ip", "-4"]).map_err(|e| format!("{e}\nsign in to Tailscale first (tailscale up)"))?;
    ips.lines().next().map(str::to_owned).ok_or_else(|| "Tailscale has no IPv4 address".into())
}

fn start_playit(dir: &Path) -> Result<Child, String> {
    let exe = tool("playit", r"C:\Program Files\playit_gg\bin\playit.exe");
    // A claimed agent can list its account's tunnels; an unclaimed one fails here.
    let list = output(&exe, &["tunnels", "list"]).map_err(|e| format!("{e}\nrun `playit` once to link this machine to your playit.gg account"))?;
    let address = playit_address(&list)?;
    println!("[moddedmc] players join {address}");
    let log = dir.join("logs").join("playit.log");
    fs::create_dir_all(dir.join("logs")).map_err(|e| e.to_string())?;
    Command::new(&exe)
        .args(["-l".as_ref(), log.as_os_str(), "start".as_ref()])
        .stdout(Stdio::null())
        .spawn()
        .map_err(|e| format!("starting playit: {e}"))
}

/// The public address of the account's allocated Minecraft Java tunnel that points at our port.
fn playit_address(list: &str) -> Result<String, String> {
    let json: serde_json::Value = serde_json::from_str(list).map_err(|e| format!("playit tunnels list: {e}"))?;
    let tunnels = json["tunnels"].as_array().map(Vec::as_slice).unwrap_or_default();
    let tunnel = tunnels.iter().find(|t| {
        t["tunnel_type"] == "minecraft-java"
            && t["origin"]["data"]["local_port"] == PORT
            && t["alloc"]["status"] == "allocated"
    });
    let alloc = &tunnel.ok_or_else(|| {
        format!("no Minecraft Java tunnel to 127.0.0.1:{PORT}; add one at https://playit.gg/account/tunnels")
    })?["alloc"]["data"];
    // Minecraft-java tunnels publish an SRV record, so the bare domain works in the client.
    match (alloc["assigned_domain"].as_str(), alloc["static_ip4"].as_str(), alloc["port_start"].as_u64()) {
        (Some(domain), Some(ip), Some(port)) => Ok(format!("{domain} (or {ip}:{port})")),
        (Some(domain), ..) => Ok(domain.to_owned()),
        _ => Err("playit tunnel has no address yet".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tunnel(kind: &str, port: u16, domain: &str) -> String {
        format!(
            r#"{{"tunnel_type":"{kind}","origin":{{"data":{{"local_port":{port}}}}},
               "alloc":{{"status":"allocated","data":{{"assigned_domain":"{domain}","static_ip4":"192.0.2.1","port_start":4000}}}}}}"#
        )
    }

    #[test]
    fn picks_the_minecraft_tunnel_for_our_port() {
        let list = format!(
            r#"{{"tunnels":[{},{},{}]}}"#,
            tunnel("minecraft-bedrock", PORT, "bedrock.example"),
            tunnel("minecraft-java", 25566, "other.example"),
            tunnel("minecraft-java", PORT, "ours.example"),
        );
        assert_eq!(playit_address(&list).unwrap(), "ours.example (or 192.0.2.1:4000)");
        assert!(playit_address(r#"{"tunnels":[]}"#).unwrap_err().contains("no Minecraft Java tunnel"));
    }
}
