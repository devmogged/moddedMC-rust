//! Headless player for scenario tests. Joins an offline-mode server, keeps the
//! connection alive, prints chat it receives, and runs steps read from stdin:
//!   cmd <command without slash>
//!   use <x> <y> <z>        right-click the top face of a block with the main hand
//!   break <x> <y> <z>      start digging the block (instant in Creative)
//!   click <action-id>      press a dialog button whose action is a custom click
//!   quit
//! Usage: moddedmc-bot <host:port> <name>

mod bot;
mod capture;

use std::sync::Arc;

use pumpkin_data::packet::{CURRENT_MC_VERSION, clientbound::play::CHUNK_BATCH_FINISHED};
use pumpkin_protocol::{
    ServerPacket,
    codec::var_int::VarInt,
    java::{
        client::play::{
            CAcknowledgeBlockChange, CKeepAlive, CPlayShowDialog, CPlayerPosition,
            CSystemChatMessage,
        },
        packet_encoder::TCPNetworkEncoder,
        server::play::{
            SChatCommand, SChunkBatch, SClientTickEnd, SConfirmTeleport, SCustomClickAction,
            SKeepAlive, SPlayerAction, SPlayerPosition, SUseItemOn, Status,
        },
    },
    packet::MultiVersionJavaPacket,
};
use pumpkin_util::math::{position::BlockPos, vector3::Vector3};
use tokio::{
    io::{AsyncBufReadExt, BufReader, BufWriter},
    sync::Mutex,
};

type Writer = Arc<Mutex<TCPNetworkEncoder<BufWriter<pumpkin_core::net::java::JavaWriteHalf>>>>;

#[expect(
    clippy::print_stdout,
    reason = "the bot reports to the scenario runner on stdout"
)]
#[tokio::main]
async fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    let [_, addr, name] = args.as_slice() else {
        return Err("usage: moddedmc-bot <host:port> <name>".into());
    };
    let bot = bot::HeadlessBot::connect_and_join(addr, name).await?;
    println!("[bot] joined as {name} at {:?}", bot.position);
    let writer: Writer = Arc::new(Mutex::new(bot.writer));
    let position = Arc::new(Mutex::new(bot.position));

    // Like a real client, report our position every second; the server loads chunks from it.
    let (tick_writer, tick_position) = (writer.clone(), position.clone());
    let harness = std::env::var_os("MODDEDMC_BOT_HARNESS").is_some();
    if !harness {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                let current = *tick_position.lock().await;
                let _ = send(
                    &tick_writer,
                    &SPlayerPosition {
                        position: current,
                        collision: 0,
                    },
                )
                .await;
                // Vanilla allows one position per client tick and resets that on tick end.
                let _ = send(&tick_writer, &SClientTickEnd).await;
            }
        });
    }

    tokio::spawn(pump(
        bot.reader,
        writer.clone(),
        position.clone(),
        harness,
        bot.capture,
    ));

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut sequence = 0;
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let result = match verb {
            "cmd" => send(&writer, &SChatCommand { command: rest }).await,
            "use" | "place" => match parse_pos(rest) {
                Some(position) => {
                    sequence += 1;
                    send(&writer, &use_item_on(position, sequence)).await
                }
                None => Err(format!("bad position: {rest}")),
            },
            "break" => match parse_pos(rest) {
                Some(position) => {
                    sequence += 1;
                    send(
                        &writer,
                        &SPlayerAction {
                            status: VarInt(Status::StartedDigging as i32),
                            position,
                            face: 1,
                            sequence: VarInt(sequence),
                        },
                    )
                    .await
                }
                None => Err(format!("bad position: {rest}")),
            },
            "click" => {
                send(
                    &writer,
                    &SCustomClickAction {
                        action_id: rest,
                        payload: None,
                    },
                )
                .await
            }
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

/// Answers keep-alives and teleports and prints what scenarios check (chat, dialogs).
#[expect(
    clippy::print_stdout,
    reason = "the bot reports to the scenario runner on stdout"
)]
async fn pump(
    mut reader: pumpkin_protocol::java::packet_decoder::TCPNetworkDecoder<
        BufReader<pumpkin_core::net::java::JavaReadHalf>,
    >,
    writer: Writer,
    position: Arc<Mutex<Vector3<f64>>>,
    harness: bool,
    mut capture: Option<capture::Capture>,
) {
    let keep_alive_id = CKeepAlive::to_id(CURRENT_MC_VERSION);
    let position_id = CPlayerPosition::to_id(CURRENT_MC_VERSION);
    let system_chat = CSystemChatMessage::to_id(CURRENT_MC_VERSION);
    let show_dialog = CPlayShowDialog::to_id(CURRENT_MC_VERSION);
    let ack = CAcknowledgeBlockChange::to_id(CURRENT_MC_VERSION);
    let trace = std::env::var_os("MODDEDMC_BOT_TRACE").is_some();
    loop {
        let Ok(packet) = reader.get_raw_packet().await else {
            println!("[bot] disconnected");
            std::process::exit(2);
        };
        if let Some(capture) = &mut capture {
            match capture.receive(&packet).await {
                Ok(Some(marker)) => {
                    println!("[bot] {marker}");
                    continue;
                }
                Ok(None) => {}
                Err(error) => {
                    println!("[bot] error: {error}");
                    std::process::exit(2);
                }
            }
        }
        let mut payload = &packet.payload[..];
        if packet.id == keep_alive_id {
            let Some(id) = payload.get(..8).and_then(|b| b.try_into().ok()) else {
                continue;
            };
            let reply = SKeepAlive {
                keep_alive_id: i64::from_be_bytes(id),
            };
            let _ = send(&writer, &reply).await;
        } else if harness && packet.id == CHUNK_BATCH_FINISHED.0 {
            let _ = send(
                &writer,
                &SChunkBatch {
                    chunks_per_tick: 9.0,
                },
            )
            .await;
        } else if packet.id == position_id {
            match CPlayerPosition::read(&mut payload, &CURRENT_MC_VERSION) {
                Ok(teleport) => {
                    let confirm = SConfirmTeleport {
                        teleport_id: teleport.teleport_id,
                        position: teleport.position,
                        yaw: teleport.yaw,
                        pitch: teleport.pitch,
                    };
                    let _ = send(&writer, &confirm).await;
                    // Settle slightly, as a falling client would: Pumpkin only reloads the
                    // player's chunks when a move actually changes the position. The 1 s
                    // reporter sends it; a second sender can put two moves in one tick,
                    // which vanilla 26.3 kicks for.
                    let mut settled = teleport.position;
                    settled.y -= 0.1;
                    *position.lock().await = settled;
                    println!("[bot] teleported to {:?}", teleport.position);
                }
                Err(error) => println!("[bot] error: bad teleport packet: {error}"),
            }
        } else if harness && packet.id == ack {
            use pumpkin_protocol::ser::NetworkReadExt as _;
            if let Ok(sequence) = payload.get_var_int() {
                println!("[bot] block ack {}", sequence.0);
            }
        } else if packet.id == system_chat {
            println!("[bot] chat: {}", printable_text(&packet.payload));
        } else if packet.id == show_dialog {
            println!("[bot] dialog: {}", printable_text(&packet.payload));
        } else if trace {
            println!(
                "[bot] packet {:#x} ({} bytes)",
                packet.id,
                packet.payload.len()
            );
        }
    }
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
