//! Headless player for scenario tests. Joins an offline-mode server, keeps the
//! connection alive, prints chat it receives, and runs steps read from stdin:
//!   cmd <command without slash>
//!   use <x> <y> <z>        right-click the top face of a block with the main hand
//!   click <action-id>      press a dialog button whose action is a custom click
//!   quit
//! Usage: moddedmc-bot <host:port> <name>

mod bot;

use std::sync::Arc;

use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_protocol::{
    ServerPacket,
    codec::var_int::VarInt,
    java::{
        client::play::{
            CChunkBatchEnd, CKeepAlive, COpenScreen, CPlayShowDialog, CPlayerPosition,
            CSpawnEntity, CSystemChatMessage,
        },
        packet_encoder::TCPNetworkEncoder,
        server::play::{
            SAttack, SChatCommand, SClientTickEnd, SConfirmTeleport, SCustomClickAction, SInteract,
            SChunkBatch, SClickSlot, SCloseContainer, SKeepAlive, SlotActionType, SPlayerPosition, SUseItemOn,
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

    tokio::spawn(pump(bot.reader, writer.clone(), position.clone()));

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
            // The last armor stand the bot saw spawn (mods' creatures in the scenarios).
            "interact" => {
                let entity_id = VarInt(LAST_STAND.load(std::sync::atomic::Ordering::Relaxed));
                let interact = SInteract {
                    entity_id,
                    r#type: VarInt(2),
                    target_position: Some(Vector3::new(0.0, 1.0, 0.0)),
                    hand: Some(VarInt(0)),
                    sneaking: false,
                };
                send(&writer, &interact).await
            }
            "attack" => {
                let entity_id = VarInt(LAST_STAND.load(std::sync::atomic::Ordering::Relaxed));
                send(&writer, &SAttack { entity_id }).await
            }
            // Shift-click a slot of the open screen.
            "shiftclick" => match rest.parse::<i16>() {
                Ok(slot) => {
                    let click = SClickSlot {
                        sync_id: VarInt(LAST_SCREEN.load(std::sync::atomic::Ordering::Relaxed)),
                        revision: VarInt(0),
                        slot,
                        button: 0,
                        mode: SlotActionType::QuickMove,
                        length_of_array: VarInt(0),
                        array_of_changed_slots: Vec::new(),
                        carried_item: pumpkin_protocol::codec::item_stack_seralizer::OptionalItemStackHash(None),
                    };
                    send(&writer, &click).await
                }
                Err(_) => Err(format!("bad slot: {rest}")),
            },
            "close" => {
                let window_id = VarInt(LAST_SCREEN.load(std::sync::atomic::Ordering::Relaxed));
                send(&writer, &SCloseContainer { window_id }).await
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
) {
    let keep_alive_id = CKeepAlive::to_id(CURRENT_MC_VERSION);
    let position_id = CPlayerPosition::to_id(CURRENT_MC_VERSION);
    let system_chat = CSystemChatMessage::to_id(CURRENT_MC_VERSION);
    let spawn_entity = CSpawnEntity::to_id(CURRENT_MC_VERSION);
    let batch_end = CChunkBatchEnd::to_id(CURRENT_MC_VERSION);
    let open_screen = COpenScreen::to_id(CURRENT_MC_VERSION);
    let show_dialog = CPlayShowDialog::to_id(CURRENT_MC_VERSION);
    let trace = std::env::var_os("MODDEDMC_BOT_TRACE").is_some();
    loop {
        let Ok(packet) = reader.get_raw_packet().await else {
            println!("[bot] disconnected");
            std::process::exit(2);
        };
        let mut payload = &packet.payload[..];
        if packet.id == keep_alive_id {
            let Some(id) = payload.get(..8).and_then(|b| b.try_into().ok()) else {
                continue;
            };
            let reply = SKeepAlive {
                keep_alive_id: i64::from_be_bytes(id),
            };
            let _ = send(&writer, &reply).await;
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
        } else if packet.id == open_screen {
            use pumpkin_protocol::ser::NetworkReadExt;
            if let Ok(id) = payload.get_var_int() {
                LAST_SCREEN.store(id.0, std::sync::atomic::Ordering::Relaxed);
                println!("[bot] screen opened: {}", printable_text(&packet.payload));
            }
        } else if packet.id == batch_end {
            // Ack like a client: unacked, Pumpkin stops sending chunks, and with them entities.
            let _ = send(&writer, &SChunkBatch { chunks_per_tick: 64.0 }).await;
        } else if packet.id == spawn_entity {
            // id, uuid, type
            use pumpkin_protocol::ser::NetworkReadExt;
            if let (Ok(id), Ok(_), Ok(kind)) = (
                payload.get_var_int(),
                payload.get_uuid(),
                payload.get_var_int(),
            ) && kind.0 == i32::from(pumpkin_data::entity::EntityType::ARMOR_STAND.id)
            {
                LAST_STAND.store(id.0, std::sync::atomic::Ordering::Relaxed);
                println!("[bot] armor stand spawned");
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

/// Window id of the last screen the server opened.
static LAST_SCREEN: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Network id of the last armor stand that spawned near the bot.
static LAST_STAND: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

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
