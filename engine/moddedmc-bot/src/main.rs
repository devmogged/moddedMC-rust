//! Headless player for scenario tests. Joins an offline-mode server, keeps the
//! connection alive, prints chat it receives, and runs steps read from stdin:
//!   cmd <command without slash>
//!   use <x> <y> <z>        right-click the top face of a block with the main hand
//!   break <x> <y> <z>      start digging the block (instant in Creative)
//!   click <action-id>      press a dialog button whose action is a custom click
//!   click-match <prefix> <suffix>  press the unique matching action in the latest dialog
//!   use-display <x> <y> <z> [block state]  use a block display via the companion channel
//!   action <id> <pressed|held|released>  send a companion semantic action
//!   rig-bone <name> <x> <y> <z> [count]  report a mounted rig's world-space bone
//!   quit
//! Usage: moddedmc-bot <host:port> <name>

mod actions;
mod attributes;
mod bot;
mod capture;
mod collision;
mod controls;
mod entities;
mod fluids;
mod inventory;
mod movement;
mod terrain;
mod vehicles;

use std::sync::Arc;

use pumpkin_data::{
    BlockDirection,
    packet::{
        CURRENT_MC_VERSION,
        clientbound::play::{CHUNK_BATCH_FINISHED, CUSTOM_PAYLOAD, SOUND, SOUND_ENTITY},
    },
    sound::Sound,
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
            SChatCommand, SChunkBatch, SCustomClickAction, SCustomPayload, SKeepAlive,
            SPlayerAction, SUseItem, SUseItemOn, Status,
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
const SCREEN_CHANNEL: &str = "moddedmc:pack_screen";
#[derive(Default)]
struct MenuState {
    dialog: Vec<DialogButton>,
    screen: Option<serde_json::Value>,
}

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
    let menus = Arc::new(Mutex::new(MenuState::default()));
    let mut last_action: Option<String> = None;

    let harness = std::env::var_os("MODDEDMC_BOT_HARNESS").is_some();
    if !harness {
        tokio::spawn(movement::realtime(
            player.clone(),
            writer.clone(),
            actions.clone(),
        ));
    }

    tokio::spawn(pump(
        bot.reader,
        writer.clone(),
        player.clone(),
        harness,
        bot.capture,
        Arc::clone(&actions),
        Arc::clone(&menus),
    ));

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut world_actions = WorldActions::default();
    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let result = match verb {
            "cmd" => send(&writer, &SChatCommand { command: rest }).await,
            "action" => companion_action(&writer, rest).await,
            "rig-bone" => {
                let mount = player.lock().await.vehicle;
                rig_bone(&writer, mount, rest).await
            }
            "screen-enable" => {
                send(
                    &writer,
                    &SCustomPayload {
                        channel: SCREEN_CHANNEL,
                        data: &[1, 0],
                    },
                )
                .await
            }
            "screen-click" => {
                let screen = menus.lock().await.screen.clone();
                screen_click(&writer, screen.as_ref(), rest).await
            }
            "use" | "place" | "use-item" | "release-item" | "break" | "dig" | "swap-hands"
            | "drop-item" => world_actions.perform(&player, &writer, verb, rest).await,
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
            "click-match" | "click-label" | "click-find" | "click-replay" => {
                let matched = if verb == "click-replay" {
                    last_action
                        .clone()
                        .ok_or_else(|| "no previous dialog action".into())
                } else {
                    let menus = menus.lock().await;
                    let buttons = &menus.dialog;
                    if verb == "click-match" {
                        match_dialog_action(buttons, rest)
                    } else {
                        match_dialog_label(buttons, rest).or_else(|error| {
                            if verb == "click-find" && error == "no matching dialog label" {
                                match_dialog_label(buttons, "Next page")
                            } else {
                                Err(error)
                            }
                        })
                    }
                };
                match matched {
                    Ok(action) => {
                        println!("[bot] matched dialog action: {action}");
                        last_action = Some(action.clone());
                        send(
                            &writer,
                            &SCustomClickAction {
                                action_id: &action,
                                payload: None,
                            },
                        )
                        .await
                    }
                    Err(error) => Err(error),
                }
            }
            "look" | "hotbar" | "attack" | "interact" | "use-entity" | "inventory-click"
            | "inventory-close" | "respawn" | "use-display" => {
                actions::perform(&actions, &player, &writer, verb, rest).await
            }
            "move" if harness => actions::perform(&actions, &player, &writer, verb, rest).await,
            "physics" if harness && rest == "on" => {
                Err("use physics manual in a frozen harness scenario".into())
            }
            "physics" => player.lock().await.set_mode(rest),
            "input" | "fly" => movement::control(&player, &writer, verb, rest).await,
            "physics-tick" => manual_ticks(&player, &writer, &actions, rest).await,
            "expect"
                if rest.starts_with("slot ")
                    || rest.starts_with("cursor ")
                    || rest.starts_with("tracker ") =>
            {
                actions
                    .lock()
                    .await
                    .check_inventory(rest)
                    .map(|result| println!("[bot] expect inventory ok {result}"))
            }
            "expect" if let Some(vehicle) = rest.strip_prefix("riding ") => {
                let player = player.lock().await;
                player
                    .check_vehicle(vehicle)
                    .map(|()| println!("[bot] expect riding ok {vehicle}"))
            }
            "expect" if let Some(entity) = rest.strip_prefix("entity ") => player
                .lock()
                .await
                .entities
                .check(entity)
                .map(|count| println!("[bot] expect entity ok {entity} ({count} visible)")),
            "expect" if let Some(block) = rest.strip_prefix("block ") => player
                .lock()
                .await
                .check_block(block)
                .map(|actual| println!("[bot] expect block ok {actual}")),
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

/// Exercise the same semantic action packet as the companion, including its play-channel hello.
async fn companion_action(writer: &Writer, args: &str) -> Result<(), String> {
    let (id, phase) = args.split_once(' ').ok_or("expected action ID and phase")?;
    let phase = match phase {
        "pressed" => 1,
        "held" => 2,
        "released" => 4,
        _ => return Err("invalid action phase".into()),
    };
    if id.is_empty() || id.len() > 128 || id.contains(char::is_whitespace) {
        return Err("invalid action ID".into());
    }
    let id_len = u8::try_from(id.len()).map_err(|_| "invalid action ID length")?;
    let channel = "moddedmc:player_actions";
    send(
        writer,
        &SCustomPayload {
            channel,
            data: &[1, 0],
        },
    )
    .await?;
    let mut data = vec![1, 2, phase, id_len];
    data.extend_from_slice(id.as_bytes());
    send(
        writer,
        &SCustomPayload {
            channel,
            data: &data,
        },
    )
    .await
}

async fn rig_bone(writer: &Writer, mount: Option<i32>, args: &str) -> Result<(), String> {
    let id = mount.ok_or("not riding a vehicle")?;
    let fields: Vec<_> = args.split_whitespace().collect();
    if !(4..=5).contains(&fields.len()) {
        return Err("expected bone name, x y z, optional repeat count".into());
    }
    let name = fields[0];
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
    {
        return Err("invalid bone name".into());
    }
    let count = fields
        .get(4)
        .map_or(Ok(1), |n| n.parse::<u8>())
        .map_err(|_| "invalid repeat count")?;
    if !(1..=60).contains(&count) {
        return Err("repeat count must be 1–60".into());
    }
    let mut data = vec![1, 6];
    data.extend_from_slice(&id.to_be_bytes());
    data.push(u8::try_from(name.len()).map_err(|_| "invalid bone name length")?);
    data.extend_from_slice(name.as_bytes());
    for field in &fields[1..4] {
        let coordinate = field.parse::<f64>().map_err(|_| "invalid coordinate")?;
        data.extend_from_slice(&coordinate.to_be_bytes());
    }
    let channel = "moddedmc:player_actions";
    send(
        writer,
        &SCustomPayload {
            channel,
            data: &[1, 0],
        },
    )
    .await?;
    for i in 0..count {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        send(
            writer,
            &SCustomPayload {
                channel,
                data: &data,
            },
        )
        .await?;
    }
    Ok(())
}

async fn screen_click(
    writer: &Writer,
    screen: Option<&serde_json::Value>,
    args: &str,
) -> Result<(), String> {
    let screen = screen.ok_or("no standalone screen")?;
    let (field, text) = args.split_once(' ').unwrap_or((args, ""));
    let action = screen["state"][field]
        .as_str()
        .ok_or("no matching screen action field")?;
    let token = screen["token"].as_str().ok_or("missing screen token")?;
    let mut data = vec![1, 2];
    data.extend_from_slice(
        &serde_json::to_vec(&serde_json::json!({"token": token, "action": action, "text": text}))
            .map_err(|e| e.to_string())?,
    );
    send(
        writer,
        &SCustomPayload {
            channel: SCREEN_CHANNEL,
            data: &data,
        },
    )
    .await
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
    menus: Arc<Mutex<MenuState>>,
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
            match dialog_buttons(&packet.payload) {
                Ok(buttons) => {
                    for button in &buttons {
                        println!("[bot] dialog button: {} -> {}", button.label, button.id);
                    }
                    let mut menus = menus.lock().await;
                    menus.dialog = buttons;
                    menus.screen = None;
                }
                Err(error) => {
                    menus.lock().await.dialog.clear();
                    println!("[bot] error: bad dialog packet: {error}");
                }
            }
            println!("[bot] dialog: {}", printable_text(&packet.payload));
        } else if packet.id == CUSTOM_PAYLOAD.0 {
            use pumpkin_protocol::ser::NetworkReadSliceExt as _;
            if payload.get_str_borrowed().ok() == Some(SCREEN_CHANNEL)
                && let Some(body) = payload.strip_prefix(&[1, 1])
                && let Ok(screen) = serde_json::from_slice::<serde_json::Value>(body)
            {
                println!("[bot] screen: {screen}");
                let mut menus = menus.lock().await;
                menus.dialog.clear();
                menus.screen = Some(screen);
            }
        } else if packet.id == SOUND.0 || packet.id == SOUND_ENTITY.0 {
            if let Err(error) = log_sound(payload, packet.id == SOUND_ENTITY.0) {
                println!("[bot] error: bad sound packet: {error}");
            }
        } else if trace {
            println!(
                "[bot] packet {:#x} ({} bytes)",
                packet.id,
                packet.payload.len()
            );
        }
    }
}

#[expect(
    clippy::print_stdout,
    reason = "the bot reports to the scenario runner on stdout"
)]
fn log_sound(
    mut payload: &[u8],
    entity_bound: bool,
) -> Result<(), pumpkin_protocol::ser::ReadingError> {
    use pumpkin_protocol::ser::NetworkReadExt;

    let id = payload.get_var_int()?.0;
    let custom;
    let name = if id == 0 {
        custom = payload.get_str()?;
        payload.get_option(NetworkReadExt::get_f32)?;
        custom.as_ref()
    } else {
        let Some(name) = usize::try_from(id)
            .ok()
            .and_then(|id| id.checked_sub(1))
            .and_then(|id| Sound::NAMES.get(id))
        else {
            return Ok(());
        };
        name
    };
    if entity_bound {
        payload.get_var_int()?;
        let entity = payload.get_var_int()?.0;
        println!("[bot] entity sound: {name} (entity {entity})");
    } else {
        println!("[bot] sound: {name}");
    }
    Ok(())
}

async fn manual_ticks(
    player: &Mutex<movement::Player>,
    writer: &Writer,
    actions: &Mutex<actions::State>,
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
        let has_carrot = actions.lock().await.steering_item();
        movement::tick(player, writer, movement::Mode::Manual, false, has_carrot).await?;
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
            "swap-hands" | "drop-item" if args.is_empty() => {
                let status = if verb == "swap-hands" {
                    Status::SwapItem
                } else {
                    Status::DropItem
                };
                send(
                    writer,
                    &player_action(status, BlockPos::new(0, 0, 0), BlockDirection::Down, 0),
                )
                .await
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
    let (face, hand, hit) = match options {
        [] => ("up", "main", None),
        [face] => (*face, "main", None),
        [face, hand] => (*face, *hand, None),
        [face, hand, x, y, z] => (*face, *hand, Some([*x, *y, *z])),
        _ => return Err("use requires x y z [face [hand [hit-x hit-y hit-z]]]".into()),
    };
    let (face, mut cursor_pos) = face_hit(face)?;
    if let Some(hit) = hit {
        let [Ok(x), Ok(y), Ok(z)] = hit.map(str::parse::<f32>) else {
            return Err("use hit coordinates must be numbers between 0 and 1".into());
        };
        if ![x, y, z].iter().all(|part| (0.0..=1.0).contains(part)) {
            return Err("use hit coordinates must be numbers between 0 and 1".into());
        }
        cursor_pos = Vector3::new(x, y, z);
    }
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

#[derive(Debug, PartialEq, Eq)]
struct DialogButton {
    label: String,
    id: String,
}

fn dialog_buttons(mut payload: &[u8]) -> Result<Vec<DialogButton>, String> {
    use pumpkin_protocol::ser::{NetworkReadExt, NetworkReadSliceExt};
    if payload.get_var_int().map_err(|e| e.to_string())?.0 != 0 {
        return Ok(Vec::new());
    }
    let tag = payload
        .get_nbt(&CURRENT_MC_VERSION)
        .map_err(|e| e.to_string())?
        .ok_or("missing dialog NBT")?;
    let root = tag.extract_compound().ok_or("dialog is not a compound")?;
    let mut buttons = Vec::new();
    let mut add = |button: &pumpkin_nbt::compound::NbtCompound| {
        let Some(action) = button.get_compound("action") else {
            return;
        };
        if action.get_string("type") != Some("minecraft:custom") {
            return;
        }
        let Some(id) = action.get_string("id") else {
            return;
        };
        let Some(label_tag) = button.get("label") else {
            return;
        };
        let Some(label) = label_tag
            .extract_string()
            .or_else(|| label_tag.extract_compound()?.get_string("text"))
        else {
            return;
        };
        buttons.push(DialogButton {
            label: label.to_owned(),
            id: id.to_owned(),
        });
    };
    if let Some(list) = root.get_list("actions") {
        for button in list.iter().filter_map(|tag| tag.extract_compound()) {
            add(button);
        }
    }
    for key in ["action", "yes", "no"] {
        if let Some(button) = root.get_compound(key) {
            add(button);
        }
    }
    Ok(buttons)
}

fn unique_dialog_action<'a>(
    mut matches: impl Iterator<Item = &'a DialogButton>,
    missing: &str,
) -> Result<String, String> {
    let first = matches.next().ok_or(missing)?;
    if matches.next().is_some() {
        return Err("ambiguous dialog action".into());
    }
    Ok(first.id.clone())
}

fn match_dialog_action(buttons: &[DialogButton], args: &str) -> Result<String, String> {
    let parts: Vec<_> = args.split_whitespace().collect();
    let [prefix, suffix] = parts.as_slice() else {
        return Err("click-match requires prefix suffix".into());
    };
    unique_dialog_action(
        buttons
            .iter()
            .filter(|b| b.id.starts_with(prefix) && b.id.ends_with(suffix)),
        "no matching dialog action",
    )
}

fn match_dialog_label(buttons: &[DialogButton], label: &str) -> Result<String, String> {
    unique_dialog_action(
        buttons.iter().filter(|b| b.label == label),
        "no matching dialog label",
    )
}

#[cfg(test)]
mod dialog_tests {
    use super::*;

    #[test]
    fn parses_wire_dialog_without_confusing_body_or_tooltip_for_actions() {
        use pumpkin_protocol::{
            ClientPacket, IdOr,
            java::client::dialog::{ActionButton, Dialog, DialogAction, DialogBody, DialogNBT},
        };
        use pumpkin_util::text::TextComponent;
        let dialog = Dialog {
            r#type: "minecraft:multi_action".into(),
            title: TextComponent::text("Compass"),
            body: vec![DialogBody::PlainMessage {
                contents: TextComponent::text("compasses:pick/decoy/3"),
            }],
            inputs: vec![],
            buttons: vec![
                ActionButton {
                    text: TextComponent::text("minecraft:birch_forest"),
                    tooltip: Some(TextComponent::text("compasses:pick/tooltip/3")),
                    width: None,
                    action: DialogAction::Custom {
                        id: "compasses:pick/actual/3".into(),
                        payload: None,
                    },
                },
                ActionButton {
                    text: TextComponent::text("Help"),
                    tooltip: None,
                    width: None,
                    action: DialogAction::OpenUrl {
                        url: "https://example.com".into(),
                    },
                },
            ],
            links: vec![],
            exit_action: None,
            after_action: None,
            can_close_with_escape: true,
            external_title: None,
        };
        let mut payload = Vec::new();
        CPlayShowDialog::new(IdOr::Value(DialogNBT::from_dialog(&dialog)))
            .write_packet_data(&mut payload, &CURRENT_MC_VERSION)
            .unwrap();
        let buttons = dialog_buttons(&payload).unwrap();
        assert_eq!(buttons.len(), 1);
        assert_eq!(
            match_dialog_label(&buttons, "minecraft:birch_forest").unwrap(),
            "compasses:pick/actual/3"
        );
        assert_eq!(
            match_dialog_action(&buttons, "compasses:pick/ /3").unwrap(),
            "compasses:pick/actual/3"
        );
        assert!(dialog_buttons(&[]).is_err());
        assert!(dialog_buttons(&[1]).unwrap().is_empty());
    }

    #[test]
    fn matches_dynamic_action_and_exact_label() {
        let buttons = vec![
            DialogButton {
                label: "minecraft:plains".into(),
                id: "compasses:pick/abc/1".into(),
            },
            DialogButton {
                label: "minecraft:forest".into(),
                id: "compasses:pick/abc/11".into(),
            },
        ];
        assert_eq!(
            match_dialog_action(&buttons, "compasses:pick/ /1").unwrap(),
            "compasses:pick/abc/1"
        );
        assert!(match_dialog_action(&buttons, "compasses:pick/ 1").is_err());
        assert_eq!(
            match_dialog_label(&buttons, "minecraft:forest").unwrap(),
            "compasses:pick/abc/11"
        );
        assert!(match_dialog_label(&buttons, "forest").is_err());
    }
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
