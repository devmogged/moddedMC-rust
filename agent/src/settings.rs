//! `GET`/`PUT /v1/servers/{name}/config`: the `pumpkin.toml` settings the app edits, and the
//! server-list icon. Missing keys read as Pumpkin's defaults; saving writes only these keys and
//! leaves the rest of the file alone. Changes apply on the next start.

use std::{path::Path, sync::Arc};

use axum::{
    Json,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::{
    App,
    pumpkin_config::{FILE, section, write},
    servers::{bad_name, error},
};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Settings {
    pub java: bool,
    pub bedrock: bool,
    pub online_mode: bool,
    pub max_players: u32,
    pub motd: String,
    pub view_distance: u8,
    pub simulation_distance: u8,
    pub difficulty: String,
    pub gamemode: String,
    pub hardcore: bool,
    pub white_list: bool,
    pub allow_nether: bool,
    pub allow_end: bool,
    pub spawn_protection: u32,
    /// The server-list icon, a 64x64 PNG, kept as `icon.png` in the server folder.
    pub icon: Option<Vec<u8>>,
}

const ICON: &str = "icon.png";
const MAX_ICON: usize = 256 * 1024;
const DIFFICULTIES: [&str; 4] = ["Peaceful", "Easy", "Normal", "Hard"];
const GAMEMODES: [&str; 4] = ["Survival", "Creative", "Adventure", "Spectator"];

pub async fn get(State(app): State<Arc<App>>, UrlPath(name): UrlPath<String>) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let Some(dir) = app.servers.folder(&name).await else {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    };
    match read(&dir).await {
        Ok(settings) => Json(settings).into_response(),
        Err(err) => error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

pub async fn put(
    State(app): State<Arc<App>>,
    UrlPath(name): UrlPath<String>,
    Json(settings): Json<Settings>,
) -> Response {
    if let Some(response) = bad_name(&name) {
        return response;
    }
    let Some(dir) = app.servers.folder(&name).await else {
        return error(StatusCode::NOT_FOUND, format!("no server named {name}"));
    };
    if let Err(message) = validate(&settings) {
        return error(StatusCode::BAD_REQUEST, message);
    }
    match save(&dir, &settings).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(err) => error(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

fn lookup<'a>(table: &'a Table, path: &[&str]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut current = table;
    for key in parents {
        current = current.get(*key)?.as_table()?;
    }
    current.get(*last)
}

fn boolean(table: &Table, path: &[&str], default: bool) -> bool {
    lookup(table, path)
        .and_then(Value::as_bool)
        .unwrap_or(default)
}

fn number<T: TryFrom<i64>>(table: &Table, path: &[&str], default: T) -> T {
    lookup(table, path)
        .and_then(Value::as_integer)
        .and_then(|n| T::try_from(n).ok())
        .unwrap_or(default)
}

fn text(table: &Table, path: &[&str], default: &str) -> String {
    lookup(table, path)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned()
}

async fn read_table(dir: &Path) -> std::io::Result<Table> {
    match tokio::fs::read_to_string(dir.join(FILE)).await {
        Ok(text) => text
            .parse::<Table>()
            .map_err(|err| std::io::Error::other(format!("can't parse {FILE}: {err}"))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Table::new()),
        Err(err) => Err(err),
    }
}

pub async fn read(dir: &Path) -> std::io::Result<Settings> {
    let t = read_table(dir).await?;
    let icon = match tokio::fs::read(dir.join(ICON)).await {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(err),
    };
    let java = |key| ["networking", "java", key];
    Ok(Settings {
        java: boolean(&t, &java("enabled"), true),
        bedrock: boolean(&t, &["networking", "bedrock", "enabled"], false),
        online_mode: boolean(&t, &java("online_mode"), true),
        max_players: number(&t, &java("max_players"), 1000),
        motd: text(&t, &java("motd"), "A blazingly fast Pumpkin server!"),
        view_distance: number(&t, &java("view_distance"), 16),
        simulation_distance: number(&t, &java("simulation_distance"), 10),
        difficulty: text(&t, &["default_difficulty"], "Normal"),
        gamemode: text(&t, &["default_gamemode"], "Survival"),
        hardcore: boolean(&t, &["hardcore"], false),
        white_list: boolean(&t, &["white_list"], false),
        allow_nether: boolean(&t, &["allow_nether"], true),
        allow_end: boolean(&t, &["allow_end"], true),
        spawn_protection: number(&t, &["spawn_protection"], 16),
        icon,
    })
}

/// What Pumpkin would reject or a client can't show.
pub fn validate(s: &Settings) -> Result<(), String> {
    if !s.java && !s.bedrock {
        return Err("turn on Java, Bedrock or both".into());
    }
    if !(2..=32).contains(&s.view_distance) || !(2..=32).contains(&s.simulation_distance) {
        return Err("view and simulation distance go from 2 to 32 chunks".into());
    }
    if s.max_players == 0 {
        return Err("allow at least one player".into());
    }
    if s.motd.chars().count() > 256 {
        return Err("the message of the day is at most 256 characters".into());
    }
    if !DIFFICULTIES.contains(&s.difficulty.as_str()) || !GAMEMODES.contains(&s.gamemode.as_str()) {
        return Err("unknown difficulty or game mode".into());
    }
    if let Some(icon) = &s.icon
        && (icon.len() > MAX_ICON || !icon.starts_with(b"\x89PNG\r\n\x1a\n"))
    {
        return Err("the icon must be a PNG under 256 KiB".into());
    }
    Ok(())
}

pub async fn save(dir: &Path, s: &Settings) -> std::io::Result<()> {
    let mut t = read_table(dir).await?;
    let java = section(&mut t, &["networking", "java"]);
    java.insert("enabled".into(), Value::Boolean(s.java));
    java.insert("online_mode".into(), Value::Boolean(s.online_mode));
    java.insert("max_players".into(), Value::Integer(s.max_players.into()));
    java.insert("motd".into(), Value::String(s.motd.clone()));
    java.insert(
        "view_distance".into(),
        Value::Integer(s.view_distance.into()),
    );
    java.insert(
        "simulation_distance".into(),
        Value::Integer(s.simulation_distance.into()),
    );
    let bedrock = section(&mut t, &["networking", "bedrock"]);
    bedrock.insert("enabled".into(), Value::Boolean(s.bedrock));
    bedrock.insert("motd".into(), Value::String(s.motd.clone()));
    bedrock.insert("max_players".into(), Value::Integer(s.max_players.into()));
    t.insert(
        "default_difficulty".into(),
        Value::String(s.difficulty.clone()),
    );
    t.insert("default_gamemode".into(), Value::String(s.gamemode.clone()));
    t.insert("hardcore".into(), Value::Boolean(s.hardcore));
    t.insert("white_list".into(), Value::Boolean(s.white_list));
    t.insert("allow_nether".into(), Value::Boolean(s.allow_nether));
    t.insert("allow_end".into(), Value::Boolean(s.allow_end));
    t.insert(
        "spawn_protection".into(),
        Value::Integer(s.spawn_protection.into()),
    );
    match &s.icon {
        Some(icon) => {
            tokio::fs::write(dir.join(ICON), icon).await?;
            t.insert("use_favicon".into(), Value::Boolean(true));
            t.insert("favicon_path".into(), Value::String(ICON.into()));
        }
        None => {
            if t.get("favicon_path").and_then(Value::as_str) == Some(ICON) {
                t.remove("favicon_path");
            }
            if let Err(err) = tokio::fs::remove_file(dir.join(ICON)).await
                && err.kind() != std::io::ErrorKind::NotFound
            {
                return Err(err);
            }
        }
    }
    write(dir, &t).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_keeps_other_keys() {
        let dir = std::env::temp_dir().join(format!("agent-settings-{}", std::process::id()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(
            dir.join(FILE),
            "seed = \"42\"\n[networking.java]\naddress = \"0.0.0.0:25570\"\n",
        )
        .await
        .unwrap();
        let mut s = read(&dir).await.unwrap();
        assert!(s.java && !s.bedrock && s.icon.is_none());
        s.bedrock = true;
        s.online_mode = false;
        s.motd = "hi".into();
        s.icon = Some(b"\x89PNG\r\n\x1a\nrest".to_vec());
        validate(&s).unwrap();
        save(&dir, &s).await.unwrap();
        assert_eq!(read(&dir).await.unwrap(), s);
        let text = tokio::fs::read_to_string(dir.join(FILE)).await.unwrap();
        assert!(text.contains("seed = \"42\"") && text.contains("0.0.0.0:25570"));
        s.icon = None;
        save(&dir, &s).await.unwrap();
        assert!(!dir.join(ICON).exists());
        s.java = false;
        s.bedrock = false;
        assert!(validate(&s).is_err());
        tokio::fs::remove_dir_all(&dir).await.unwrap();
    }
}
