//! The few `pumpkin.toml` settings the agent owns: the Java port (from `server.json`) and the
//! defaults a new managed server starts with. Everything else stays the owner's to edit.

use std::{io, net::SocketAddr, path::Path};

use toml::{Table, Value};

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

pub async fn read(dir: &Path) -> io::Result<Table> {
    match tokio::fs::read_to_string(dir.join(FILE)).await {
        Ok(text) => text
            .parse()
            .map_err(|err| io::Error::other(format!("can't parse {FILE}: {err}"))),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Table::new()),
        Err(err) => Err(err),
    }
}

pub fn java_address(table: &Table) -> io::Result<SocketAddr> {
    table
        .get("networking")
        .and_then(|value| value.get("java"))
        .and_then(|value| value.get("address"))
        .map_or(Ok(SocketAddr::from(([0, 0, 0, 0], 25565))), |value| {
            value
                .as_str()
                .and_then(|address| address.parse().ok())
                .ok_or_else(|| io::Error::other(format!("invalid Java address in {FILE}")))
        })
}

/// Applies the managed port and returns the address Pumpkin will bind, if Java is enabled.
pub async fn prepare_java(dir: &Path, port: Option<u16>) -> io::Result<Option<SocketAddr>> {
    let mut table = read(dir).await?;
    let java = section(&mut table, &["networking", "java"]);
    let enabled = java.get("enabled").and_then(Value::as_bool).unwrap_or(true);
    if let Some(port) = port {
        let wanted = address(port);
        if java.get("address").and_then(Value::as_str) != Some(wanted.as_str()) {
            java.insert("address".into(), Value::String(wanted));
            write(dir, &table).await?;
        }
    }
    if enabled {
        java_address(&table).map(Some)
    } else {
        Ok(None)
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
