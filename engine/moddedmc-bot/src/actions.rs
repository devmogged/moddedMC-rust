use std::collections::HashMap;

use pumpkin_data::packet::{CURRENT_MC_VERSION, PacketId, clientbound::play as p};
use pumpkin_protocol::{
    VarInt,
    codec::item_stack_seralizer::OptionalItemStackHash,
    java::server::play::{
        ActionType, SAttack, SClickSlot, SClientCommand, SClientTickEnd, SCloseContainer,
        SInteract, SPlayerPosition, SPlayerRotation, SSetHeldItem, SlotActionType,
    },
    ser::NetworkReadExt as _,
};
use pumpkin_util::math::vector3::Vector3;
use tokio::sync::Mutex;

use crate::{Writer, bot::send_framed_packet, inventory::Inventory, movement::Player, send};

#[derive(Default)]
pub struct State {
    window: i32,
    revisions: HashMap<i32, VarInt>,
    inventory: Inventory,
    inventory_error: Option<String>,
}

impl State {
    pub fn steering_item(&self) -> bool {
        self.inventory
            .holds(&pumpkin_data::item::Item::CARROT_ON_A_STICK)
    }

    pub fn observe(&mut self, kind: i32, mut payload: &[u8]) -> Result<(), String> {
        let inventory_payload = payload;
        match PacketId(kind) {
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
                self.window = 0;
                self.revisions.clear();
                self.inventory_error = None;
            }
            _ => {}
        }
        if let Err(error) = self.inventory.observe(kind, inventory_payload, self.window) {
            self.inventory_error = Some(error.to_string());
        }
        Ok(())
    }

    pub fn check_inventory(&self, args: &str) -> Result<String, String> {
        if let Some(error) = &self.inventory_error {
            return Err(format!("could not decode inventory: {error}"));
        }
        self.inventory.check(args, self.window)
    }

    fn click(&self, slot: i16, button: &str) -> Result<SClickSlot, String> {
        let (button, mode) = match button {
            "left" => (0, SlotActionType::Pickup),
            "right" => (1, SlotActionType::Pickup),
            "shift-left" => (0, SlotActionType::QuickMove),
            "shift-right" => (1, SlotActionType::QuickMove),
            "double" => (0, SlotActionType::PickupAll),
            "drop" => (0, SlotActionType::Throw),
            "drop-stack" => (1, SlotActionType::Throw),
            swap if swap.starts_with("swap-") => {
                (hotbar_slot(&swap[5..])? as i8, SlotActionType::Swap)
            }
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
            // Leave slot prediction to the server and predict an empty cursor.
            array_of_changed_slots: Vec::new(),
            carried_item: OptionalItemStackHash(None),
        })
    }

    fn close(&mut self) -> Result<SCloseContainer, String> {
        let id = self.window;
        if id == 0 {
            return Err("no container is open".into());
        }
        self.window = 0;
        self.revisions.remove(&id);
        self.inventory.close_menu();
        Ok(SCloseContainer {
            window_id: VarInt(id),
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
            let entity_id = player.lock().await.entities.target(name, anchor)?;
            send(writer, &SAttack { entity_id }).await
        }
        ("interact" | "use-entity", [name, x, y, z, hand]) => {
            let anchor = vector(&[x, y, z])?;
            use_entity(
                player,
                writer,
                name,
                anchor,
                crate::hand(hand)?,
                verb == "use-entity",
            )
            .await
        }
        ("inventory-click", [slot, button]) => {
            let slot = slot
                .parse::<i16>()
                .ok()
                .filter(|slot| *slot >= 0)
                .ok_or("invalid inventory slot")?;
            let mut writer = writer.lock().await;
            let packet = {
                let mut state = state.lock().await;
                let packet = state.click(slot, button)?;
                state.inventory.predict_empty_cursor();
                packet
            };
            send_framed_packet(&mut *writer, &packet).await
        }
        ("hotbar", [slot]) => {
            let slot = hotbar_slot(slot)?;
            state.lock().await.inventory.select(slot);
            send(writer, &SSetHeldItem { slot }).await
        }
        ("inventory-close", []) => {
            let packet = state.lock().await.close()?;
            send(writer, &packet).await
        }
        ("respawn", []) => {
            send(
                writer,
                &SClientCommand::new(SClientCommand::PERFORM_RESPAWN.into()),
            )
            .await
        }
        _ => Err(format!("invalid {verb} arguments")),
    }
}

async fn use_entity(
    player: &Mutex<Player>,
    writer: &Writer,
    name: &str,
    anchor: Vector3<f64>,
    hand: VarInt,
    nearby: bool,
) -> Result<(), String> {
    let (entity_id, sneaking) = {
        let player = player.lock().await;
        let id = if nearby {
            player.entities.nearby_target(name, anchor)?
        } else {
            player.entities.target(name, anchor)?
        };
        (id, player.controls.shift_pressed())
    };
    send(
        writer,
        &SInteract {
            entity_id,
            r#type: VarInt(ActionType::InteractAt as i32),
            target_position: Some(Vector3::default()),
            hand: Some(hand),
            sneaking,
        },
    )
    .await
}

fn hotbar_slot(value: &str) -> Result<i16, String> {
    value
        .parse::<i16>()
        .ok()
        .filter(|slot| (0..=8).contains(slot))
        .ok_or_else(|| format!("invalid hotbar slot {value}"))
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
        state.observe(p::CONTAINER_SET_CONTENT.0, &[7, 4, 3, 0, 0, 0, 0])?;
        state.observe(p::CONTAINER_SET_SLOT.0, &[7, 5, 0, 2, 0])?;
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
