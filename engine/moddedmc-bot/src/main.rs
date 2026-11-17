//! Headless player for scenario tests. Joins an offline-mode server, keeps the
//! connection alive, prints chat it receives, and runs steps read from stdin:
//!   cmd <command without slash>
//!   use <x> <y> <z>        right-click the top face of a block with the main hand
//!   quit
//! Usage: moddedmc-bot <addr> <name>

mod bot;

use std::{net::SocketAddr, sync::Arc};

use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_protocol::{
    ServerPacket,
    codec::var_int::VarInt,
    java::{
        client::play::{CKeepAlive, CPlayerPosition, CSystemChatMessage},
        packet_encoder::TCPNetworkEncoder,
        server::play::{SChatCommand, SConfirmTeleport, SKeepAlive, SPlayerPosition, SUseItemOn},
    },
    packet::MultiVersionJavaPacket,
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use tokio::{
    io::{AsyncBufReadExt, BufReader, BufWriter},
    sync::Mutex,
};

type Writer = Arc<Mutex<TCPNetworkEncoder<BufWriter<pumpkin_core::net::java::JavaWriteHalf>>>>;

#[expect(clippy::print_stdout, reason = "the bot reports to the scenario runner on stdout")]
#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, addr, name] = args.as_slice() else {
        return Err("usage: moddedmc-bot <addr> <name>".into());
    };
    let addr: SocketAddr = addr.parse().map_err(|e| format!("bad address: {e}"))?;
    let bot = bot::HeadlessBot::connect_and_join(addr, name).await?;
    println!("[bot] joined as {name} at {:?}", bot.position);
    let writer: Writer = Arc::new(Mutex::new(bot.writer));
    let mut reader = bot.reader;
    let position = Arc::new(Mutex::new(bot.position));

    // Like a real client, report our position every second; the server loads chunks from it.
    let (tick_writer, tick_position) = (writer.clone(), position.clone());
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let current = *tick_position.lock().await;
            let _ = send(&tick_writer, &SPlayerPosition { position: current, collision: 0 }).await;
        }
    });

    let (pump_writer, pump_position) = (writer.clone(), position.clone());
    tokio::spawn(async move {
        let keep_alive = CKeepAlive::to_id(CURRENT_MC_VERSION);
        let position = CPlayerPosition::to_id(CURRENT_MC_VERSION);
        let system_chat = CSystemChatMessage::to_id(CURRENT_MC_VERSION);
        loop {
            let Ok(packet) = reader.get_raw_packet().await else {
                println!("[bot] disconnected");
                std::process::exit(2);
            };
            let mut payload = &packet.payload[..];
            if packet.id == keep_alive {
                let Some(id) = payload.get(..8).and_then(|b| b.try_into().ok()) else { continue };
                let reply = SKeepAlive { keep_alive_id: i64::from_be_bytes(id) };
                let _ = send(&pump_writer, &reply).await;
            } else if packet.id == position {
                if let Ok(teleport) = CPlayerPosition::read(&mut payload, &CURRENT_MC_VERSION) {
                    let confirm = SConfirmTeleport {
                        teleport_id: teleport.teleport_id,
                        position: teleport.position,
                        yaw: teleport.yaw,
                        pitch: teleport.pitch,
                    };
                    let _ = send(&pump_writer, &confirm).await;
                    // Settle slightly, as a falling client would: Pumpkin only reloads the
                    // player's chunks when a move actually changes the position.
                    let mut settled = teleport.position;
                    settled.y -= 0.1;
                    *pump_position.lock().await = settled;
                    let moved = SPlayerPosition { position: settled, collision: 0 };
                    let _ = send(&pump_writer, &moved).await;
                    println!("[bot] teleported to {:?}", teleport.position);
                }
            } else if packet.id == system_chat {
                println!("[bot] chat: {}", printable_text(&packet.payload));
            }
        }
    });

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut sequence = 0;
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let result = match verb {
            "cmd" => send(&writer, &SChatCommand { command: rest }).await,
            "use" => match parse_pos(rest) {
                Some(position) => {
                    sequence += 1;
                    send(&writer, &use_item_on(position, sequence)).await
                }
                None => Err(format!("bad position: {rest}")),
            },
            "quit" => break,
            _ => Err(format!("unknown step: {line}")),
        };
        match result {
            Ok(()) => println!("[bot] did: {line}"),
            Err(error) => println!("[bot] error: {error}"),
        }
    }
    Ok(())
}

async fn send<P: pumpkin_protocol::ClientPacket + MultiVersionJavaPacket>(
    writer: &Writer,
    packet: &P,
) -> Result<(), String> {
    bot::send_framed_packet(&mut *writer.lock().await, packet).await
}

const fn use_item_on(position: BlockPos, sequence: i32) -> SUseItemOn {
    SUseItemOn {
        hand: VarInt(0),
        position,
        face: VarInt(1),
        cursor_pos: Vector3::new(0.5, 1.0, 0.5),
        inside_block: false,
        is_against_world_border: false,
        sequence: VarInt(sequence),
    }
}

fn parse_pos(text: &str) -> Option<BlockPos> {
    let mut parts = text.split_whitespace().map(str::parse::<i32>);
    let (Some(Ok(x)), Some(Ok(y)), Some(Ok(z)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    Some(BlockPos(Vector3::new(x, y, z)))
}

/// Chat arrives as network NBT; the readable strings are enough for test assertions.
fn printable_text(payload: &[u8]) -> String {
    payload
        .split(|b| !(b.is_ascii_graphic() || *b == b' '))
        .filter(|run| run.len() >= 3)
        .map(String::from_utf8_lossy)
        .collect::<Vec<_>>()
        .join(" ")
}
