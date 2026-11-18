//! Friend mode: the servers you play on, and the server list ping that fills in their rows
//! (description, players, latency, icon), the same exchange the game's multiplayer screen does.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

use crate::store::{self, Friend, Store};

const DEFAULT_PORT: u16 = 25565;
/// A status response is one JSON string; a favicon makes it tens of KB at most.
const MAX_PACKET: usize = 1 << 20;
const PING_TIMEOUT: Duration = Duration::from_secs(4);

#[tauri::command]
pub fn friends_list(store: State<'_, Store>) -> Vec<Friend> {
    store.friends()
}

#[derive(Deserialize)]
pub struct NewFriend {
    name: String,
    address: String,
}

#[tauri::command]
pub fn friend_add(store: State<'_, Store>, friend: NewFriend) -> Result<Friend, String> {
    let address = friend.address.trim();
    if address.is_empty() {
        return Err("A server needs an address.".into());
    }
    split_address(address)?;
    let name = match friend.name.trim() {
        "" => address,
        name => name,
    };
    let friend = Friend {
        id: store::slug(address),
        name: name.to_string(),
        address: address.to_string(),
    };
    store.add_friend(friend.clone())?;
    Ok(friend)
}

#[tauri::command]
pub fn friend_remove(store: State<'_, Store>, id: String) -> Result<(), String> {
    store.remove_friend(&id)
}

#[derive(Serialize)]
pub struct Status {
    latency_ms: u32,
    motd: String,
    version: Option<String>,
    players_online: Option<u32>,
    players_max: Option<u32>,
    /// A `data:image/png;base64,...` URL, as the server sends it.
    favicon: Option<String>,
}

#[tauri::command]
pub async fn ping(address: String) -> Result<Status, String> {
    let (host, port) = split_address(address.trim())?;
    timeout(PING_TIMEOUT, status(&host, port))
        .await
        .map_err(|_| format!("{address} didn't answer."))?
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::ConnectionRefused => format!("{address} is offline."),
            _ => format!("Can't reach {address}."),
        })
}

fn split_address(address: &str) -> Result<(String, u16), String> {
    let bad = || format!("{address} isn't a server address.");
    if let Some(rest) = address.strip_prefix('[') {
        let (host, port) = rest.split_once(']').ok_or_else(bad)?;
        let port = match port.strip_prefix(':') {
            Some(port) => port.parse().map_err(|_| bad())?,
            None if port.is_empty() => DEFAULT_PORT,
            None => return Err(bad()),
        };
        return Ok((host.to_string(), port));
    }
    match address.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => {
            Ok((host.to_string(), port.parse().map_err(|_| bad())?))
        }
        _ if address.is_empty() || address.contains(char::is_whitespace) => Err(bad()),
        _ => Ok((address.to_string(), DEFAULT_PORT)),
    }
}

async fn status(host: &str, port: u16) -> std::io::Result<Status> {
    let mut stream = TcpStream::connect((host, port)).await?;
    stream.set_nodelay(true)?;

    // Handshake (next state 1 = status), then the status request.
    let mut handshake = Vec::new();
    write_varint(&mut handshake, 0x00);
    write_varint(&mut handshake, -1);
    write_varint(&mut handshake, host.len() as i32);
    handshake.extend_from_slice(host.as_bytes());
    handshake.extend_from_slice(&port.to_be_bytes());
    write_varint(&mut handshake, 1);
    send(&mut stream, &handshake).await?;
    send(&mut stream, &[0x00]).await?;

    let body = receive(&mut stream).await?;
    let mut cursor = body.as_slice();
    if read_varint(&mut cursor)? != 0x00 {
        return Err(invalid("not a status response"));
    }
    let len = read_varint(&mut cursor)? as usize;
    let json = cursor.get(..len).ok_or_else(|| invalid("short status"))?;
    let value: Value = serde_json::from_slice(json).map_err(|_| invalid("bad status json"))?;

    // Latency is the ping round trip, like the multiplayer screen measures it.
    let started = Instant::now();
    let mut ping = vec![0x01];
    ping.extend_from_slice(&0i64.to_be_bytes());
    send(&mut stream, &ping).await?;
    receive(&mut stream).await?;
    let latency_ms = started.elapsed().as_millis().min(u32::MAX as u128) as u32;

    let players = &value["players"];
    Ok(Status {
        latency_ms,
        motd: plain_text(&value["description"]),
        version: value["version"]["name"].as_str().map(str::to_owned),
        players_online: players["online"].as_u64().map(|n| n as u32),
        players_max: players["max"].as_u64().map(|n| n as u32),
        favicon: value["favicon"]
            .as_str()
            .filter(|f| f.starts_with("data:image/png;base64,"))
            .map(str::to_owned),
    })
}

/// A text component flattened to plain text, with legacy `§` formatting codes removed.
fn plain_text(component: &Value) -> String {
    fn walk(value: &Value, out: &mut String) {
        match value {
            Value::String(text) => out.push_str(text),
            Value::Array(parts) => parts.iter().for_each(|part| walk(part, out)),
            Value::Object(map) => {
                if let Some(text) = map.get("text") {
                    walk(text, out);
                }
                if let Some(extra) = map.get("extra") {
                    walk(extra, out);
                }
            }
            _ => {}
        }
    }
    let mut raw = String::new();
    walk(component, &mut raw);
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

fn invalid(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

async fn send(stream: &mut TcpStream, body: &[u8]) -> std::io::Result<()> {
    let mut packet = Vec::with_capacity(body.len() + 5);
    write_varint(&mut packet, body.len() as i32);
    packet.extend_from_slice(body);
    stream.write_all(&packet).await
}

async fn receive(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut len = 0u32;
    for shift in 0..5 {
        let byte = stream.read_u8().await?;
        len |= u32::from(byte & 0x7f) << (7 * shift);
        if byte & 0x80 == 0 {
            let len = len as usize;
            if len > MAX_PACKET {
                return Err(invalid("status too large"));
            }
            let mut body = vec![0; len];
            stream.read_exact(&mut body).await?;
            return Ok(body);
        }
    }
    Err(invalid("bad length"))
}

fn write_varint(out: &mut Vec<u8>, value: i32) {
    let mut value = value as u32;
    loop {
        if value & !0x7f == 0 {
            out.push(value as u8);
            return;
        }
        out.push((value as u8 & 0x7f) | 0x80);
        value >>= 7;
    }
}

fn read_varint(cursor: &mut &[u8]) -> std::io::Result<i32> {
    let mut value = 0u32;
    for shift in 0..5 {
        let (&byte, rest) = cursor
            .split_first()
            .ok_or_else(|| invalid("short varint"))?;
        *cursor = rest;
        value |= u32::from(byte & 0x7f) << (7 * shift);
        if byte & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(invalid("varint too long"))
}
