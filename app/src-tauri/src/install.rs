//! Installing the device agent on a Linux machine over SSH (docs/DEVICES.md, "Install"):
//! copy the binary for its architecture, write `agent.toml` with this user as the owner, and
//! run it as a systemd user service that keeps running without a login session.

use std::{path::PathBuf, process::Stdio};

use tokio::process::Command;

/// The agent binary for a Linux architecture. Release builds will bundle these; from a
/// checkout they come from `cargo zigbuild --release --target <triple> -p moddedmc-agent`.
fn agent_binary(triple: &str) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("MODDEDMC_AGENT_DIR") {
        let path = PathBuf::from(dir).join(triple).join("moddedmc-agent");
        if path.exists() {
            return Ok(path);
        }
    }
    let checkout = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let path = checkout
        .join("target")
        .join(triple)
        .join("release")
        .join("moddedmc-agent");
    if path.exists() {
        Ok(path)
    } else {
        Err(format!(
            "No agent build for {triple}. Build it with: cargo zigbuild --release --target {triple} -p moddedmc-agent"
        ))
    }
}

const UNIT: &str = "[Unit]
Description=moddedMC device agent
After=network-online.target

[Service]
ExecStart=%h/.moddedmc/bin/moddedmc-agent
Restart=on-failure

[Install]
WantedBy=default.target
";

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    command
}

async fn ssh(destination: &str, script: &str) -> Result<String, String> {
    let output = command("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            destination,
            script,
        ])
        .output()
        .await
        .map_err(|_| "Can't run ssh. Is OpenSSH installed?".to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr
            .lines()
            .last()
            .unwrap_or("ssh failed")
            .trim()
            .to_string())
    }
}

/// This machine's Tailscale login, which becomes the agent's owner.
async fn tailscale_login() -> Option<String> {
    let output = command("tailscale")
        .args(["status", "--json", "--peers=false"])
        .output()
        .await
        .ok()?;
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let user = value["Self"]["UserID"].as_u64()?;
    value["User"][user.to_string()]["LoginName"]
        .as_str()
        .map(str::to_owned)
}

/// A TOML basic string: the login is the only value that comes from outside.
fn toml_string(value: &str) -> String {
    toml::Value::String(value.to_string()).to_string()
}

#[tauri::command]
pub async fn install_agent(destination: String) -> Result<String, String> {
    let destination = destination.trim().to_string();
    if destination.is_empty()
        || destination.starts_with('-')
        || destination.contains(char::is_whitespace)
    {
        return Err("That isn't an SSH destination.".into());
    }
    let system = ssh(&destination, "uname -sm")
        .await
        .map_err(|e| format!("Can't log in to {destination} with a key: {e}"))?;
    let triple = match system.as_str() {
        "Linux aarch64" => "aarch64-unknown-linux-gnu",
        "Linux x86_64" => "x86_64-unknown-linux-gnu",
        other => {
            return Err(format!(
                "{destination} runs {other}; the agent installs on Linux only."
            ));
        }
    };
    let binary = agent_binary(triple)?;

    ssh(
        &destination,
        "mkdir -p ~/.moddedmc/bin ~/.config/systemd/user",
    )
    .await?;
    let output = command("scp")
        .args(["-q", "-o", "BatchMode=yes"])
        .arg(&binary)
        .arg(format!("{destination}:.moddedmc/bin/moddedmc-agent.new"))
        .output()
        .await
        .map_err(|_| "Can't run scp. Is OpenSSH installed?".to_string())?;
    if !output.status.success() {
        return Err(format!(
            "Copying the agent failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    // An existing agent.toml stays: it may name a different owner on purpose.
    let config = match tailscale_login().await {
        Some(login) => format!(
            "test -e ~/.moddedmc/agent.toml || printf 'owner = %s\\nport = 25599\\nservers_root = \"%s\"\\n' '{}' \"$HOME/.moddedmc/servers\" > ~/.moddedmc/agent.toml",
            toml_string(&login).replace('\'', "")
        ),
        None => String::from(":"),
    };
    let script = format!(
        "set -e
cd ~/.moddedmc/bin && chmod +x moddedmc-agent.new && mv moddedmc-agent.new moddedmc-agent
{config}
cat > ~/.config/systemd/user/moddedmc-agent.service <<'UNIT'
{UNIT}UNIT
loginctl enable-linger \"$USER\" 2>/dev/null || echo no-linger
systemctl --user daemon-reload
systemctl --user enable moddedmc-agent >/dev/null 2>&1
systemctl --user restart moddedmc-agent"
    );
    let result = ssh(&destination, &script).await?;
    Ok(if result.contains("no-linger") {
        "Installed. It stops when you log out there: run `sudo loginctl enable-linger $USER` once."
            .into()
    } else {
        "Installed and running.".into()
    })
}
