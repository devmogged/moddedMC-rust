use std::collections::HashMap;

use pumpkin_data::{
    entity::EntityType,
    packet::{CURRENT_MC_VERSION, PacketId, clientbound::play as p},
};
use pumpkin_protocol::{
    VarInt,
    codec::item_stack_seralizer::OptionalItemStackHash,
    java::{
        client::play::CSpawnEntity,
        server::play::{
            SAttack, SClickSlot, SClientTickEnd, SPlayerPosition, SPlayerRotation, SlotActionType,
        },
    },
    ser::NetworkReadExt as _,
};
use pumpkin_util::math::vector3::Vector3;
use tokio::sync::Mutex;

use crate::{Writer, bot::send_framed_packet, movement::Player, send};

#[derive(Default)]
pub struct State {
    entities: HashMap<i32, CSpawnEntity>,
    window: i32,
    revisions: HashMap<i32, VarInt>,
}

impl State {
    pub fn observe(&mut self, kind: i32, mut payload: &[u8]) -> Result<(), String> {
        match PacketId(kind) {
            p::ADD_ENTITY => {
                let spawn = CSpawnEntity::read_packet_data(payload, &CURRENT_MC_VERSION)
                    .map_err(|e| e.to_string())?;
                self.entities.insert(spawn.entity_id.0, spawn);
            }
            p::REMOVE_ENTITIES => {
                let count = read_id(&mut payload)?.0 as usize;
                if count > payload.len() {
                    return Err("invalid entity removal count".into());
                }
                for _ in 0..count {
                    self.entities.remove(&read_id(&mut payload)?.0);
                }
            }
            p::OPEN_SCREEN | p::MOUNT_SCREEN_OPEN => {
                self.window = read_id(&mut payload)?.0;
                self.revisions.remove(&self.window);
            }
            p::CONTAINER_CLOSE => {
                let id = read_id(&mut payload)?.0;
                if self.window == id {
                    self.window = 0;
                }
                self.revisions.remove(&id);
            }
            p::CONTAINER_SET_CONTENT | p::CONTAINER_SET_SLOT => {
                let id = payload
                    .get_container_id(&CURRENT_MC_VERSION)
                    .map_err(|e| e.to_string())?;
                self.revisions.insert(id.0, read_id(&mut payload)?);
            }
            p::RESPAWN => {
                self.entities.clear();
                self.window = 0;
                self.revisions.clear();
            }
            _ => {}
        }
        Ok(())
    }

    fn target(&self, name: &str, anchor: Vector3<f64>) -> Result<VarInt, String> {
        let kind =
            EntityType::from_name(name).ok_or_else(|| format!("unknown entity type {name}"))?;
        let mut targets = self.entities.values().filter(|entity| {
            entity.r#type.0 == i32::from(kind.id)
                && (entity.position.x - anchor.x).abs() <= 1e-6
                && (entity.position.y - anchor.y).abs() <= 1e-6
                && (entity.position.z - anchor.z).abs() <= 1e-6
        });
        let entity = targets
            .next()
            .ok_or_else(|| format!("no visible {name} spawned at {anchor:?}"))?;
        if targets.next().is_some() {
            return Err(format!("multiple visible {name} spawned at {anchor:?}"));
        }
        Ok(entity.entity_id)
    }

    fn click(&self, slot: i16, button: &str) -> Result<SClickSlot, String> {
        let (button, mode) = match button {
            "left" => (0, SlotActionType::Pickup),
            "right" => (1, SlotActionType::Pickup),
            "shift-left" => (0, SlotActionType::QuickMove),
            "shift-right" => (1, SlotActionType::QuickMove),
            _ => return Err(format!("invalid inventory button {button}")),
        };
        let revision = self
            .revisions
            .get(&self.window)
            .copied()
            .or((self.window == 0).then_some(VarInt(0)))
            .ok_or_else(|| format!("no inventory revision received for window {}", self.window))?;
        Ok(SClickSlot {
            sync_id: self.window.into(),
            revision,
            slot,
            button,
            mode,
            length_of_array: 0.into(),
            // The server resends any slots and cursor state affected by the click.
            array_of_changed_slots: Vec::new(),
            carried_item: OptionalItemStackHash(None),
        })
    }
}

pub async fn perform(
    state: &Mutex<State>,
    player: &Mutex<Player>,
    writer: &Writer,
    verb: &str,
    args: &str,
) -> Result<(), String> {
    let parts = args.split_whitespace().collect::<Vec<_>>();
    match (verb, parts.as_slice()) {
        ("move", coords) => {
            let position = vector(coords)?;
            let mut writer = writer.lock().await;
            player.lock().await.reposition(position);
            send_framed_packet(
                &mut *writer,
                &SPlayerPosition {
                    position,
                    collision: 0,
                },
            )
            .await?;
            send_framed_packet(&mut *writer, &SClientTickEnd).await
        }
        ("look", [yaw, pitch]) => {
            let yaw: f32 = yaw.parse().map_err(|_| "invalid yaw")?;
            let pitch: f32 = pitch.parse().map_err(|_| "invalid pitch")?;
            if !yaw.is_finite() || !(-90.0..=90.0).contains(&pitch) {
                return Err("look requires finite yaw and pitch from -90 to 90".into());
            }
            let mut writer = writer.lock().await;
            let (ground, horizontal_collision) = player.lock().await.look(yaw, pitch);
            send_framed_packet(
                &mut *writer,
                &SPlayerRotation {
                    yaw,
                    pitch,
                    ground,
                    horizontal_collision,
                },
            )
            .await?;
            send_framed_packet(&mut *writer, &SClientTickEnd).await
        }
        ("attack", [name, coords @ ..]) => {
            let anchor = vector(coords)?;
            let entity_id = state.lock().await.target(name, anchor)?;
            send(writer, &SAttack { entity_id }).await
        }
        ("inventory-click", [slot, button]) => {
            let slot = slot
                .parse::<i16>()
                .ok()
                .filter(|slot| *slot >= 0)
                .ok_or("invalid inventory slot")?;
            let packet = state.lock().await.click(slot, button)?;
            send(writer, &packet).await
        }
        _ => Err(format!("invalid {verb} arguments")),
    }
}

fn read_id(payload: &mut &[u8]) -> Result<VarInt, String> {
    payload.get_var_int().map_err(|e| e.to_string())
}

fn vector(parts: &[&str]) -> Result<Vector3<f64>, String> {
    let [x, y, z] = parts else {
        return Err("expected three coordinates".into());
    };
    let coordinate = |text: &str| {
        text.parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| format!("invalid coordinate {text}"))
    };
    Ok(Vector3::new(coordinate(x)?, coordinate(y)?, coordinate(z)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_uses_latest_active_window_revision() -> Result<(), String> {
        let mut state = State::default();
        state.observe(p::OPEN_SCREEN.0, &[7])?;
        assert!(state.click(2, "left").is_err());
        state.observe(p::CONTAINER_SET_CONTENT.0, &[7, 4])?;
        state.observe(p::CONTAINER_SET_SLOT.0, &[7, 5])?;
        let click = state.click(2, "shift-right")?;
        assert_eq!(
            (click.sync_id.0, click.revision.0, click.slot, click.button),
            (7, 5, 2, 1)
        );
        assert_eq!(click.mode, SlotActionType::QuickMove);
        state.observe(p::CONTAINER_CLOSE.0, &[7])?;
        assert_eq!(state.click(2, "left")?.sync_id.0, 0);
        Ok(())
    }
}
