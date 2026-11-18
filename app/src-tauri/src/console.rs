//! Console streams: one WebSocket per open console, lines forwarded as `console` events.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU32, Ordering},
};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, mpsc};
use tokio_tungstenite::tungstenite::Message;

use crate::{agent::endpoint, devices::Tunnels, store::Store};

#[derive(Default)]
pub struct Consoles {
    next: AtomicU32,
    open: Mutex<HashMap<u32, mpsc::UnboundedSender<String>>>,
}

#[derive(Serialize, Clone)]
struct ConsoleEvent {
    id: u32,
    /// `None` when the stream ended.
    line: Option<String>,
}

#[tauri::command]
pub async fn console_open(
    app: AppHandle,
    store: State<'_, Store>,
    tunnels: State<'_, Tunnels>,
    consoles: State<'_, Consoles>,
    device: String,
    server: String,
) -> Result<u32, String> {
    if server.contains(['/', '\\', '?', '#']) {
        return Err("bad server name".into());
    }
    let (endpoint, name) = endpoint(&store, &tunnels, &device).await?;
    let url = format!("ws://{endpoint}/v1/servers/{server}/console");
    let (socket, _) = tokio_tungstenite::connect_async(url)
        .await
        .map_err(|_| format!("Can't open the console on {name}."))?;
    let id = consoles.next.fetch_add(1, Ordering::Relaxed);
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    consoles.open.lock().await.insert(id, tx);
    let (mut sink, mut stream) = socket.split();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                message = stream.next() => match message {
                    Some(Ok(Message::Text(text))) => {
                        let line = Some(text.to_string());
                        let _ = app.emit("console", ConsoleEvent { id, line });
                    }
                    Some(Ok(_)) => {}
                    _ => break,
                },
                command = rx.recv() => match command {
                    Some(command) => {
                        if sink.send(Message::Text(command.into())).await.is_err() {
                            break;
                        }
                    }
                    None => {
                        let _ = sink.close().await;
                        return;
                    }
                },
            }
        }
        let _ = app.emit("console", ConsoleEvent { id, line: None });
    });
    Ok(id)
}

#[tauri::command]
pub async fn console_send(
    consoles: State<'_, Consoles>,
    id: u32,
    line: String,
) -> Result<(), String> {
    let open = consoles.open.lock().await;
    let tx = open.get(&id).ok_or("The console closed.")?;
    tx.send(line).map_err(|_| "The console closed.".to_string())
}

#[tauri::command]
pub async fn console_close(consoles: State<'_, Consoles>, id: u32) -> Result<(), String> {
    consoles.open.lock().await.remove(&id);
    Ok(())
}
