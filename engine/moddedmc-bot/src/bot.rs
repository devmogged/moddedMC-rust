//! Join flow copied from Pumpkin `tools/pumpkin-tester/src/bot.rs` (GPL-3.0) at the pinned commit.

use std::time::Duration;

use bytes::Bytes;
use pumpkin_core::net::java::{JavaReadHalf, JavaWriteHalf};
use pumpkin_data::packet::CURRENT_MC_VERSION;
use pumpkin_protocol::{
    ClientPacket, ConnectionState, RawPacket, ServerPacket,
    codec::var_int::VarInt,
    java::{
        client::{
            config::{
                CConfigAddResourcePack, CConfigPing, CFinishConfig, CKnownPacks, CPluginMessage,
            },
            login::CLoginSuccess,
            play::{CLogin, CPlayerPosition},
        },
        packet_decoder::TCPNetworkDecoder,
        packet_encoder::TCPNetworkEncoder,
        server::{
            config::{
                SAcknowledgeFinishConfig, SClientInformationConfig, SConfigPong,
                SConfigResourcePack, SKnownPacks, SPluginMessage,
            },
            handshake::SHandShake,
            login::{SLoginAcknowledged, SLoginStart},
            play::SPlayerLoaded,
        },
    },
    packet::MultiVersionJavaPacket,
    ser::{NetworkReadExt, NetworkReadSliceExt, NetworkWriteExt},
};
use tokio::{
    io::{AsyncWrite, BufReader, BufWriter},
    net::TcpStream,
    time::timeout,
};
use uuid::Uuid;

pub const PACKET_TIMEOUT: Duration = Duration::from_secs(60);

pub struct HeadlessBot {
    pub name: String,
    pub reader: TCPNetworkDecoder<BufReader<JavaReadHalf>>,
    pub writer: TCPNetworkEncoder<BufWriter<JavaWriteHalf>>,
    pub player: crate::movement::Player,
    pub capture: Option<crate::capture::Capture>,
    pub actions: crate::actions::State,
}

impl HeadlessBot {
    pub async fn send_packet<P: ClientPacket + MultiVersionJavaPacket>(
        &mut self,
        packet: &P,
    ) -> Result<(), String> {
        send_framed_packet(&mut self.writer, packet).await
    }

    pub async fn next_packet(&mut self) -> Result<RawPacket, String> {
        match timeout(PACKET_TIMEOUT, self.reader.get_raw_packet()).await {
            Ok(Ok(packet)) => {
                if std::env::var_os("MODDEDMC_BOT_TRACE").is_some() {
                    trace_packet(packet.id, packet.payload.len());
                }
                Ok(packet)
            }
            Ok(Err(err)) => Err(format!("Bot '{}' packet decode error: {err}", self.name)),
            Err(_) => Err(format!(
                "Bot '{}' timed out waiting for packet from server",
                self.name
            )),
        }
    }

    /// `addr` is `host:port`. Like a real client, the handshake carries the host it dialed;
    /// playit's shared Minecraft tunnels route on it.
    pub async fn connect_and_join(addr: &str, name: &str) -> Result<Self, String> {
        let (host, port) = addr
            .rsplit_once(':')
            .and_then(|(host, port)| Some((host, port.parse().ok()?)))
            .ok_or_else(|| format!("bad address {addr}, expected host:port"))?;
        let stream = TcpStream::connect(addr)
            .await
            .map_err(|err| format!("Bot '{name}' failed to connect to {addr}: {err}"))?;
        let _ = stream.set_nodelay(true);
        let (read_half, write_half) = stream.into_split();
        Self::join(Box::new(read_half), Box::new(write_half), host, port, name).await
    }

    pub async fn join(
        read_half: JavaReadHalf,
        write_half: JavaWriteHalf,
        host: &str,
        port: u16,
        name: &str,
    ) -> Result<Self, String> {
        let mut bot = Self {
            name: name.to_string(),
            reader: TCPNetworkDecoder::new(BufReader::new(read_half)),
            writer: TCPNetworkEncoder::new(BufWriter::new(write_half)),
            player: crate::movement::Player::default(),
            capture: crate::capture::Capture::start(name),
            actions: crate::actions::State::default(),
        };

        // 1. Handshake -> Login
        bot.send_packet(&SHandShake {
            protocol_version: VarInt(CURRENT_MC_VERSION.protocol_version()),
            server_address: host.into(),
            server_port: port,
            next_state: ConnectionState::Login,
        })
        .await?;

        bot.send_packet(&SLoginStart {
            name: name.into(),
            uuid: Uuid::new_v4(),
        })
        .await?;

        let login_success_id = CLoginSuccess::to_id(CURRENT_MC_VERSION);
        loop {
            let pkt = bot.next_packet().await?;
            if pkt.id == login_success_id {
                break;
            }
        }

        bot.send_packet(&SLoginAcknowledged).await?;

        bot.configure(host).await?;

        // 3. Play state: wait for CLogin and initial CPlayerPosition synchronize packet
        let play_login_id = CLogin::to_id(CURRENT_MC_VERSION);
        let player_pos_id = CPlayerPosition::to_id(CURRENT_MC_VERSION);
        let mut saw_play_login = false;
        let mut saw_initial_pos = false;

        while !(saw_play_login && saw_initial_pos) {
            let pkt = bot.next_packet().await?;
            if let Some(capture) = &mut bot.capture {
                capture.record(&pkt)?;
            }
            bot.actions.observe(pkt.id, &pkt.payload)?;
            bot.player.observe(&pkt)?;
            if pkt.id == play_login_id {
                saw_play_login = true;
            } else if pkt.id == player_pos_id {
                let mut slice = &pkt.payload[..];
                let pos_pkt = CPlayerPosition::read(&mut slice, &CURRENT_MC_VERSION)
                    .map_err(|err| format!("Failed to read CPlayerPosition: {err}"))?;
                let confirm = bot.player.teleport(&pos_pkt);
                bot.send_packet(&confirm).await?;
                bot.send_packet(&SPlayerLoaded).await?;
                saw_initial_pos = true;
            }
        }

        Ok(bot)
    }

    async fn configure(&mut self, host: &str) -> Result<(), String> {
        self.send_packet(&SClientInformationConfig {
            locale: "en_us",
            view_distance: 4,
            chat_mode: VarInt(0),
            chat_colors: true,
            skin_parts: 0x7F,
            main_hand: VarInt(1),
            text_filtering: false,
            server_listing: true,
            particle_status: VarInt(0),
        })
        .await?;

        let known_packs_id = CKnownPacks::to_id(CURRENT_MC_VERSION);
        let finish_config_id = CFinishConfig::to_id(CURRENT_MC_VERSION);
        let pack_id = CConfigAddResourcePack::to_id(CURRENT_MC_VERSION);
        let config_ping_id = CConfigPing::to_id(CURRENT_MC_VERSION);
        let companion_version = std::env::var("MODDEDMC_BOT_COMPANION")
            .ok()
            .and_then(|value| value.parse::<u16>().ok());
        let mut received_answer = false;
        loop {
            let pkt = self.next_packet().await?;
            if pkt.id == CPluginMessage::to_id(CURRENT_MC_VERSION)
                && let Some(version) = companion_version
            {
                received_answer |= self.companion_message(&pkt.payload, version).await?;
            } else if pkt.id == pack_id {
                let uuid = download_pack(&pkt.payload, host).await?;
                self.send_packet(&SConfigResourcePack {
                    uuid,
                    result: VarInt(0),
                })
                .await?;
            } else if pkt.id == config_ping_id {
                // Fabric waits for this pong before it finishes configuration.
                let id = pkt
                    .payload
                    .get(..4)
                    .and_then(|b| b.try_into().ok())
                    .map_or(0, i32::from_be_bytes);
                self.send_packet(&SConfigPong { id }).await?;
            } else if pkt.id == known_packs_id {
                self.send_packet(&SKnownPacks {
                    known_packs: Vec::new(),
                })
                .await?;
            } else if pkt.id == finish_config_id {
                if companion_version.is_some() && !received_answer {
                    return Err("No companion configuration answer".into());
                }
                return self.send_packet(&SAcknowledgeFinishConfig).await;
            }
        }
    }

    #[expect(
        clippy::print_stdout,
        reason = "the bot reports configuration evidence to stdout"
    )]
    async fn companion_message(&mut self, mut data: &[u8], version: u16) -> Result<bool, String> {
        const CHANNEL: &str = "moddedmc:negotiate";
        if data.get_str_borrowed().map_err(|error| error.to_string())? != CHANNEL {
            return Ok(false);
        }
        if data.is_empty() {
            let mut hello = version.to_be_bytes().to_vec();
            hello.extend_from_slice(&1u32.to_be_bytes());
            self.send_packet(&SPluginMessage {
                channel: CHANNEL,
                data: &hello,
            })
            .await?;
            return Ok(false);
        }
        match data {
            [0, 1, 0, 0, 0, 0] => println!("[bot] companion protocol 1, active capabilities 0"),
            [0, 1, 0, 0, 0, 1, rest @ ..] if rest.len() == 52 && version == 1 => {
                use std::fmt::Write as _;
                let mut manifest = String::with_capacity(64);
                for byte in &rest[..32] {
                    let _ = write!(manifest, "{byte:02x}");
                }
                println!(
                    "[bot] companion protocol 1, active capabilities 1, asset manifest {manifest}"
                );
            }
            _ => return Err(format!("Unexpected companion answer {data:?}")),
        }
        Ok(true)
    }
}

#[expect(
    clippy::print_stdout,
    reason = "the bot reports to the scenario runner on stdout"
)]
fn trace_packet(id: i32, len: usize) {
    println!("[bot] packet {id:#x} ({len} bytes)");
}

pub async fn send_framed_packet<P: ClientPacket + MultiVersionJavaPacket, W: AsyncWrite + Unpin>(
    writer: &mut TCPNetworkEncoder<BufWriter<W>>,
    packet: &P,
) -> Result<(), String> {
    let packet_id = P::to_id(CURRENT_MC_VERSION);
    let mut buf = Vec::new();
    buf.write_var_int(&VarInt(packet_id))
        .map_err(|err| format!("Failed to write packet id {packet_id}: {err:?}"))?;
    packet
        .write_packet_data(&mut buf, &CURRENT_MC_VERSION)
        .map_err(|err| format!("Failed to serialize packet {packet_id}: {err:?}"))?;
    writer
        .write_packet(Bytes::from(buf))
        .await
        .map_err(|err| format!("Failed to write packet to socket: {err}"))?;
    writer
        .flush()
        .await
        .map_err(|err| format!("Failed to flush socket: {err}"))
}

#[expect(
    clippy::print_stdout,
    reason = "the bot reports to the scenario runner on stdout"
)]
async fn download_pack(mut payload: &[u8], host: &str) -> Result<Uuid, String> {
    use sha1::{Digest, Sha1};
    const MAX_PACK: u64 = 32 * 1024 * 1024;
    let uuid = payload.get_uuid().map_err(|e| e.to_string())?;
    let address = payload
        .get_str_bounded_borrowed(512)
        .map_err(|e| e.to_string())?;
    let hash = payload
        .get_str_bounded_borrowed(40)
        .map_err(|e| e.to_string())?;
    let url = reqwest::Url::parse(address).map_err(|e| e.to_string())?;
    if url.scheme() != "http"
        || url.host_str() != Some(host)
        || url
            .path()
            .strip_prefix("/resource-pack-")
            .and_then(|name| name.strip_suffix(".zip"))
            != Some(hash)
        || hash.len() != 40
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("unexpected content resource-pack offer".into());
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PACKET_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.to_string())?;
    if response.content_length().is_none_or(|size| size > MAX_PACK) {
        return Err("invalid content resource-pack size".into());
    }
    let mut remaining = MAX_PACK;
    let mut digest = Sha1::new();
    while let Some(bytes) = response.chunk().await.map_err(|e| e.to_string())? {
        remaining = remaining
            .checked_sub(bytes.len() as u64)
            .ok_or("invalid content resource-pack size")?;
        digest.update(&bytes);
    }
    if !digest
        .finalize()
        .iter()
        .zip(hash.as_bytes().as_chunks::<2>().0.iter())
        .all(|(&byte, pair)| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                == Some(byte)
        })
    {
        return Err("content resource pack SHA-1 mismatch".into());
    }
    println!("[bot] fetched content resource pack SHA-1 {hash}");
    Ok(uuid)
}
