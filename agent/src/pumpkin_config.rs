//! The few `pumpkin.toml` settings the agent owns: the Java port (from `server.json`) and the
//! defaults a new managed server starts with. Everything else stays the owner's to edit.

use std::path::Path;

use toml::{Table, Value};
use tracing::warn;

pub const FILE: &str = "pumpkin.toml";

/// Settings for a new server: its port, offline mode, telemetry off (like `host`), and no
/// Bedrock listener, so several servers on one machine don't fight over Bedrock's fixed ports.
pub async fn seed(dir: &Path, port: Option<u16>) -> std::io::Result<()> {
    let mut table = Table::new();
    let java = section(&mut table, &["networking", "java"]);
    java.insert("online_mode".into(), Value::Boolean(false));
    if let Some(port) = port {
        java.insert("address".into(), Value::String(address(port)));
    }
    section(&mut table, &["networking", "bedrock"]).insert("enabled".into(), Value::Boolean(false));
    section(&mut table, &["telemetry"]).insert("enabled".into(), Value::Boolean(false));
    write(dir, &table).await
}

/// Makes the Java listener use `port` before a start. Leaves a file it can't read alone.
pub async fn apply_port(dir: &Path, port: u16) {
    let path = dir.join(FILE);
    let mut table = match tokio::fs::read_to_string(&path).await {
        Ok(text) => match text.parse::<Table>() {
            Ok(table) => table,
            Err(err) => {
                warn!(
                    "not setting the port: can't parse {}: {err}",
                    path.display()
                );
                return;
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Table::new(),
        Err(err) => {
            warn!("not setting the port: can't read {}: {err}", path.display());
            return;
        }
    };
    let wanted = address(port);
    let java = section(&mut table, &["networking", "java"]);
    if java.get("address").and_then(Value::as_str) == Some(wanted.as_str()) {
        return;
    }
    java.insert("address".into(), Value::String(wanted));
    if let Err(err) = write(dir, &table).await {
        warn!("can't write {}: {err}", path.display());
    }
}

fn address(port: u16) -> String {
    format!("0.0.0.0:{port}")
}

pub fn section<'a>(table: &'a mut Table, path: &[&str]) -> &'a mut Table {
    let mut current = table;
    for key in path {
        let entry = current
            .entry(key.to_string())
            .or_insert_with(|| Value::Table(Table::new()));
        if !entry.is_table() {
            *entry = Value::Table(Table::new());
        }
        current = match entry {
            Value::Table(table) => table,
            _ => unreachable!("replaced with a table above"),
        };
    }
    current
}

pub async fn write(dir: &Path, table: &Table) -> std::io::Result<()> {
    let text = toml::to_string_pretty(table).map_err(std::io::Error::other)?;
    let tmp = dir.join("pumpkin.toml.tmp");
    tokio::fs::write(&tmp, text).await?;
    tokio::fs::rename(&tmp, dir.join(FILE)).await
}
