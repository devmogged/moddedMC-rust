//! Headless player for scenario tests. Joins an offline-mode server, keeps the
//! connection alive, prints chat it receives, and runs steps read from stdin:
//!   cmd <command without slash>
//!   use <x> <y> <z>        right-click the top face of a block with the main hand
//!   break <x> <y> <z>      start digging the block (instant in Creative)
//!   click <action-id>      press a dialog button whose action is a custom click
//!   quit
//! Usage: moddedmc-bot <host:port> <name>

mod actions;
mod bot;
mod capture;
mod collision;
mod controls;
mod entities;
mod fluids;
mod inventory;
mod movement;
mod terrain;

use std::sync::Arc;

use pumpkin_data::{
    BlockDirection,
    packet::{CURRENT_MC_VERSION, clientbound::play::CHUNK_BATCH_FINISHED},
};
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
            SChatCommand, SChunkBatch, SCustomClickAction, SKeepAlive, SPlayerAction, SUseItem,
            SUseItemOn, Status,
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
    println!("[bot] joined as {name} at {:?}", bot.player.position);
    let writer: Writer = Arc::new(Mutex::new(bot.writer));
    let player = Arc::new(Mutex::new(bot.player));
    let actions = Arc::new(Mutex::new(bot.actions));

    let harness = std::env::var_os("MODDEDMC_BOT_HARNESS").is_some();
    if !harness {
        tokio::spawn(movement::realtime(player.clone(), writer.clone()));
    }

    tokio::spawn(pump(
        bot.reader,
        writer.clone(),
        player.clone(),
        harness,
        bot.capture,
        Arc::clone(&actions),
    ));

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut world_actions = WorldActions::default();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let result = match verb {
            "cmd" => send(&writer, &SChatCommand { command: rest }).await,
            "use" | "place" | "use-item" | "release-item" | "break" | "dig" => {
                world_actions.perform(&player, &writer, verb, rest).await
            }
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
            "look" | "hotbar" | "attack" | "interact" | "inventory-click" | "inventory-close" => {
                actions::perform(&actions, &player, &writer, verb, rest).await
            }
            "move" if harness => actions::perform(&actions, &player, &writer, verb, rest).await,
            "physics" if harness && rest == "on" => {
                Err("use physics manual in a frozen harness scenario".into())
            }
            "physics" => player.lock().await.set_mode(rest),
            "input" | "fly" => movement::control(&player, &writer, verb, rest).await,
            "physics-tick" => manual_ticks(&player, &writer, rest).await,
            "expect" if rest.starts_with("slot ") || rest.starts_with("cursor ") => actions
                .lock()
                .await
                .check_inventory(rest)
                .map(|result| println!("[bot] expect inventory ok {result}")),
            "expect" if let Some(vehicle) = rest.strip_prefix("riding ") => {
                let player = player.lock().await;
                player
                    .check_vehicle(vehicle)
                    .map(|()| println!("[bot] expect riding ok {vehicle}"))
            }
            "expect" => player
                .lock()
                .await
                .check(rest)
                .map(|(kind, actual)| println!("[bot] expect {kind} ok {actual:?}")),
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
    player: Arc<Mutex<movement::Player>>,
    harness: bool,
    mut capture: Option<capture::Capture>,
    actions: Arc<Mutex<actions::State>>,
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
        let observed_actions = actions.lock().await.observe(packet.id, &packet.payload);
        if let Err(error) = observed_actions {
            println!("[bot] error: {error}");
            std::process::exit(2);
        }
        let observed = player.lock().await.observe(&packet);
        if let Err(error) = observed {
            println!("[bot] error: {error}");
            std::process::exit(2);
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
        } else if packet.id == CHUNK_BATCH_FINISHED.0 {
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
                    let mut writer = writer.lock().await;
                    let confirm = player.lock().await.teleport(&teleport);
                    let _ = bot::send_framed_packet(&mut *writer, &confirm).await;
                    println!("[bot] teleported to {:?}", confirm.position);
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

async fn manual_ticks(
    player: &Mutex<movement::Player>,
    writer: &Writer,
    count: &str,
) -> Result<(), String> {
    let count = count
        .parse::<u16>()
        .ok()
        .filter(|count| (1..=movement::MAX_MANUAL_TICKS).contains(count))
        .ok_or("physics-tick requires a count from 1 to 1200")?;
    if player.lock().await.mode != movement::Mode::Manual {
        return Err("physics-tick requires physics manual".into());
    }
    for _ in 0..count {
        movement::tick(player, writer, movement::Mode::Manual, false).await?;
    }
    Ok(())
}

async fn send<P: pumpkin_protocol::ClientPacket + MultiVersionJavaPacket>(
    writer: &Writer,
    packet: &P,
) -> Result<(), String> {
    bot::send_framed_packet(&mut *writer.lock().await, packet).await
}

#[derive(Default)]
struct WorldActions {
    sequence: i32,
    digging: Option<(BlockPos, BlockDirection)>,
}

impl WorldActions {
    async fn perform(
        &mut self,
        player: &Mutex<movement::Player>,
        writer: &Writer,
        verb: &str,
        args: &str,
    ) -> Result<(), String> {
        let next = self
            .sequence
            .checked_add(1)
            .ok_or("block action sequence exhausted")?;
        match verb {
            "use" | "place" => {
                let packet = use_item_on(args, next)?;
                self.sequence = next;
                send(writer, &packet).await
            }
            "use-item" => {
                let hand = hand(args)?;
                let (yaw, pitch) = player.lock().await.rotation();
                self.sequence = next;
                send(
                    writer,
                    &SUseItem {
                        hand,
                        sequence: VarInt(next),
                        yaw,
                        pitch,
                    },
                )
                .await
            }
            "break" => {
                let coords = args.split_whitespace().collect::<Vec<_>>();
                let position = parse_block(&coords)?;
                self.sequence = next;
                send(
                    writer,
                    &player_action(Status::StartedDigging, position, BlockDirection::Up, next),
                )
                .await
            }
            "dig" => {
                let (status, position, face) = self.dig(args)?;
                self.sequence = next;
                send(writer, &player_action(status, position, face, next)).await
            }
            "release-item" if args.is_empty() => {
                send(
                    writer,
                    &player_action(
                        Status::ReleaseItemInUse,
                        BlockPos::new(0, 0, 0),
                        BlockDirection::Down,
                        0,
                    ),
                )
                .await
            }
            _ => Err(format!("unknown block action {verb}")),
        }
    }

    fn dig(&mut self, args: &str) -> Result<(Status, BlockPos, BlockDirection), String> {
        let args = args.split_whitespace().collect::<Vec<_>>();
        match args.as_slice() {
            ["start", x, y, z, face] => {
                if self.digging.is_some() {
                    return Err("finish or abort the previous dig before starting another".into());
                }
                let position = parse_block(&[x, y, z])?;
                let (face, _) = face_hit(face)?;
                self.digging = Some((position, face));
                Ok((Status::StartedDigging, position, face))
            }
            [phase @ ("finish" | "abort")] => {
                let (position, face) = self.digging.take().ok_or("no dig is active")?;
                if *phase == "finish" {
                    Ok((Status::FinishedDigging, position, face))
                } else {
                    Ok((Status::CancelledDigging, position, BlockDirection::Down))
                }
            }
            _ => Err("dig requires start x y z face, finish or abort".into()),
        }
    }
}

const fn player_action(
    status: Status,
    position: BlockPos,
    face: BlockDirection,
    sequence: i32,
) -> SPlayerAction {
    SPlayerAction {
        status: VarInt(status as i32),
        position,
        face: face as u8,
        sequence: VarInt(sequence),
    }
}

fn use_item_on(args: &str, sequence: i32) -> Result<SUseItemOn, String> {
    let args = args.split_whitespace().collect::<Vec<_>>();
    let [x, y, z, options @ ..] = args.as_slice() else {
        return Err("use requires x y z with optional face and hand".into());
    };
    let position = parse_block(&[x, y, z])?;
    let (face, hand) = match options {
        [] => ("up", "main"),
        [face] => (*face, "main"),
        [face, hand] => (*face, *hand),
        _ => return Err("use requires x y z with optional face and hand".into()),
    };
    let (face, cursor_pos) = face_hit(face)?;
    Ok(SUseItemOn {
        hand: self::hand(hand)?,
        position,
        face: VarInt(face as i32),
        cursor_pos,
        inside_block: false,
        is_against_world_border: false,
        sequence: VarInt(sequence),
    })
}

fn face_hit(face: &str) -> Result<(BlockDirection, Vector3<f32>), String> {
    Ok(match face {
        "down" => (BlockDirection::Down, Vector3::new(0.5, 0.0, 0.5)),
        "up" => (BlockDirection::Up, Vector3::new(0.5, 1.0, 0.5)),
        "north" => (BlockDirection::North, Vector3::new(0.5, 0.5, 0.0)),
        "south" => (BlockDirection::South, Vector3::new(0.5, 0.5, 1.0)),
        "west" => (BlockDirection::West, Vector3::new(0.0, 0.5, 0.5)),
        "east" => (BlockDirection::East, Vector3::new(1.0, 0.5, 0.5)),
        _ => return Err(format!("invalid block face {face}")),
    })
}

fn hand(name: &str) -> Result<VarInt, String> {
    match name {
        "main" => Ok(VarInt(0)),
        "off" => Ok(VarInt(1)),
        _ => Err(format!("invalid hand {name}")),
    }
}

fn parse_block(parts: &[&str]) -> Result<BlockPos, String> {
    if let [x, y, z] = parts
        && let [Ok(x), Ok(y), Ok(z)] = [x, y, z].map(|value| value.parse::<i32>())
    {
        return Ok(BlockPos::new(x, y, z));
    }
    Err(format!(
        "expected three integer block coordinates, got {}",
        parts.join(" ")
    ))
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
