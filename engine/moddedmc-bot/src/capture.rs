use pumpkin_data::packet::{CURRENT_MC_VERSION, clientbound::play as p};
use pumpkin_protocol::{RawPacket, ser::NetworkReadSliceExt as _};

const MAX_WINDOW: usize = 32 * 1024 * 1024;

#[derive(Default)]
pub struct Capture {
    name: String,
    bytes: Vec<u8>,
}

impl Capture {
    pub fn start(name: &str) -> Option<Self> {
        std::env::var_os("MODDEDMC_BOT_CAPTURE").map(|_| Self {
            name: name.into(),
            bytes: Vec::new(),
        })
    }

    pub async fn receive(&mut self, packet: &RawPacket) -> Result<Option<String>, String> {
        if packet.id == p::SYSTEM_CHAT.0 {
            let mut payload = &packet.payload[..];
            if let Ok(component) = payload.get_component(&CURRENT_MC_VERSION) {
                let text = component.get_text();
                if let Some(index) = text.strip_prefix("moddedmc_harness_packet_fence_")
                    && index.parse::<u32>().is_ok()
                {
                    tokio::fs::write(format!("bot-{}.packets", self.name), &self.bytes)
                        .await
                        .map_err(|e| e.to_string())?;
                    self.bytes.clear();
                    return Ok(Some(text));
                }
            }
        }
        self.record(packet)?;
        Ok(None)
    }

    pub fn record(&mut self, packet: &RawPacket) -> Result<(), String> {
        if [
            p::KEEP_ALIVE,
            p::BUNDLE_DELIMITER,
            p::PING,
            p::PONG_RESPONSE,
            p::LEVEL_CHUNK_WITH_LIGHT,
            p::CHUNK_BATCH_START,
            p::CHUNK_BATCH_FINISHED,
            p::CHUNKS_BIOMES,
            p::UPDATE_RECIPES,
            p::UPDATE_TAGS,
        ]
        .iter()
        .any(|kind| kind.0 == packet.id)
        {
            return Ok(());
        }
        if self.bytes.len() + 8 + packet.payload.len() > MAX_WINDOW {
            return Err("packet capture window exceeds 32 MiB".into());
        }
        self.bytes.extend_from_slice(&packet.id.to_be_bytes());
        self.bytes
            .extend_from_slice(&(packet.payload.len() as u32).to_be_bytes());
        self.bytes.extend_from_slice(&packet.payload);
        Ok(())
    }
}
