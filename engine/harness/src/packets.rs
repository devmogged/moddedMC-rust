use std::{collections::HashMap, fmt::Write as _};

use pumpkin_data::{
    entity::EntityType,
    packet::{CURRENT_MC_VERSION, PacketId, clientbound::play as p},
};
use pumpkin_protocol::{
    java::client::play::CSpawnEntity,
    ser::{NetworkReadExt as _, NetworkReadSliceExt as _},
};
use sha2::{Digest as _, Sha256};

use crate::diff::Check;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entity {
    SelfPlayer,
    Spawn {
        kind: i32,
        pos: [u64; 3],
        index: usize,
    },
    Unseen(i32),
    Absent,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Packet {
    kind: i32,
    entities: Vec<Entity>,
    data: Vec<u8>,
}

#[derive(Default)]
pub struct Decoder {
    ids: HashMap<i32, Entity>,
    spawns: HashMap<(i32, [u64; 3]), usize>,
}

impl Decoder {
    pub fn window(&mut self, mut bytes: &[u8]) -> Result<Vec<Packet>, String> {
        let mut packets = Vec::new();
        while !bytes.is_empty() {
            let kind = bytes.get_i32_be().map_err(|e| e.to_string())?;
            let length = bytes.get_u32_be().map_err(|e| e.to_string())? as usize;
            let body = bytes
                .read_slice_borrowed(length)
                .map_err(|e| e.to_string())?;
            if let Some(packet) = self
                .packet(kind, body)
                .map_err(|e| format!("{}: {e}", kind_name(kind)))?
            {
                packets.push(packet);
            }
        }
        Ok(packets)
    }

    fn packet(&mut self, kind: i32, mut body: &[u8]) -> Result<Option<Packet>, String> {
        let mut entities = Vec::new();
        let mut data = Vec::new();
        match PacketId(kind) {
            p::LOGIN => {
                self.ids.insert(
                    body.get_i32_be().map_err(|e| e.to_string())?,
                    Entity::SelfPlayer,
                );
                return Ok(None);
            }
            p::ADD_ENTITY => return self.spawn(body).map(Some),
            p::ENTITY_EVENT => {
                entities.push(self.entity(body.get_i32_be().map_err(|e| e.to_string())?));
            }
            p::SET_ENTITY_LINK => {
                entities.push(self.entity(body.get_i32_be().map_err(|e| e.to_string())?));
                let holder = body.get_i32_be().map_err(|e| e.to_string())?;
                entities.push(if holder == 0 {
                    Entity::Absent
                } else {
                    self.entity(holder)
                });
            }
            p::TAKE_ITEM_ENTITY => {
                entities.push(self.entity(var(&mut body)?));
                entities.push(self.entity(var(&mut body)?));
            }
            p::SET_PASSENGERS | p::REMOVE_ENTITIES => {
                if kind == p::SET_PASSENGERS.0 {
                    entities.push(self.entity(var(&mut body)?));
                }
                entities.extend(self.entity_list(&mut body, kind == p::REMOVE_ENTITIES.0)?);
            }
            p::DAMAGE_EVENT => {
                entities.push(self.entity(var(&mut body)?));
                data.extend_from_slice(&var(&mut body)?.to_be_bytes());
                for _ in 0..2 {
                    let source = var(&mut body)?;
                    entities.push(if source == 0 {
                        Entity::Absent
                    } else {
                        self.entity(source - 1)
                    });
                }
            }
            p::PLAYER_LOOK_AT => {
                data.extend_from_slice(&var(&mut body)?.to_be_bytes());
                data.extend_from_slice(body.read_slice_borrowed(24).map_err(|e| e.to_string())?);
                let has_entity = body.get_u8().map_err(|e| e.to_string())?;
                data.push(has_entity);
                if has_entity != 0 {
                    entities.push(self.entity(var(&mut body)?));
                }
            }
            p::MOUNT_SCREEN_OPEN => {
                for _ in 0..2 {
                    data.extend_from_slice(&var(&mut body)?.to_be_bytes());
                }
                entities.push(self.entity(body.get_i32_be().map_err(|e| e.to_string())?));
            }
            p::SOUND | p::SOUND_ENTITY => return self.sound(kind, body).map(Some),
            p::PLAYER_POSITION => {
                var(&mut body)?;
            } // teleport acknowledgement token
            p::SET_TIME => {
                body.get_i64_be().map_err(|e| e.to_string())?;
                if body == [0] {
                    return Ok(None);
                }
            } // absolute game clock
            p::ANIMATE
            | p::BLOCK_DESTRUCTION
            | p::DEBUG_ENTITY_VALUE
            | p::ENTITY_POSITION_SYNC
            | p::HURT_ANIMATION
            | p::MOVE_ENTITY_POS
            | p::MOVE_ENTITY_POS_ROT
            | p::MOVE_ENTITY_ROT
            | p::MOVE_MINECART_ALONG_TRACK
            | p::PROJECTILE_POWER
            | p::REMOVE_MOB_EFFECT
            | p::ROTATE_HEAD
            | p::SET_CAMERA
            | p::SET_ENTITY_DATA
            | p::SET_ENTITY_MOTION
            | p::SET_EQUIPMENT
            | p::SWING_ANIMATION
            | p::TELEPORT_ENTITY
            | p::UPDATE_ATTRIBUTES
            | p::UPDATE_MOB_EFFECT => entities.push(self.entity(var(&mut body)?)),
            _ => {}
        }
        data.extend_from_slice(body);
        Ok(Some(Packet {
            kind,
            entities,
            data,
        }))
    }

    fn entity_list(&mut self, body: &mut &[u8], remove: bool) -> Result<Vec<Entity>, String> {
        let count = var(body)? as usize;
        if count > body.len() {
            return Err("invalid entity list size".into());
        }
        (0..count)
            .map(|_| {
                let id = var(body)?;
                Ok(if remove {
                    self.ids.remove(&id).unwrap_or(Entity::Unseen(id))
                } else {
                    self.entity(id)
                })
            })
            .collect()
    }

    fn sound(&self, kind: i32, mut body: &[u8]) -> Result<Packet, String> {
        let mut entities = Vec::new();
        let mut data = Vec::new();
        let original = body;
        if var(&mut body)? == 0 {
            body.get_str_borrowed().map_err(|e| e.to_string())?;
            if body.get_u8().map_err(|e| e.to_string())? != 0 {
                body.get_f32_be().map_err(|e| e.to_string())?;
            }
        }
        var(&mut body)?; // sound category
        data.extend_from_slice(&original[..original.len() - body.len()]);
        if kind == p::SOUND_ENTITY.0 {
            entities.push(self.entity(var(&mut body)?));
        }
        let size = if kind == p::SOUND_ENTITY.0 { 16 } else { 28 };
        if body.len() != size {
            return Err("invalid sound packet size".into());
        }
        body = &body[..size - 8]; // sound variant random seed
        data.extend_from_slice(body);
        Ok(Packet {
            kind,
            entities,
            data,
        })
    }

    fn spawn(&mut self, body: &[u8]) -> Result<Packet, String> {
        let mut rest = body;
        let spawn = CSpawnEntity::read_packet_data(&mut rest, &CURRENT_MC_VERSION)
            .map_err(|e| e.to_string())?;
        if !rest.is_empty() {
            return Err("trailing bytes in spawn packet".into());
        }
        let pos = [spawn.position.x, spawn.position.y, spawn.position.z].map(f64::to_bits);
        let kind = spawn.r#type.0;
        let index = self.spawns.entry((kind, pos)).or_default();
        *index += 1;
        let identity = Entity::Spawn {
            kind,
            pos,
            index: *index,
        };
        self.ids.insert(spawn.entity_id.0, identity.clone());
        let mut entities = vec![identity];

        let mut prefix = body;
        var(&mut prefix)?;
        prefix.get_uuid().map_err(|e| e.to_string())?;
        let mut data = prefix.to_vec();
        if spawn.data.0 != 0
            && [
                EntityType::ARROW,
                EntityType::SPECTRAL_ARROW,
                EntityType::TRIDENT,
                EntityType::FISHING_BOBBER,
                EntityType::SNOWBALL,
                EntityType::EGG,
                EntityType::ENDER_PEARL,
                EntityType::EXPERIENCE_BOTTLE,
                EntityType::SPLASH_POTION,
                EntityType::LINGERING_POTION,
                EntityType::FIREBALL,
                EntityType::SMALL_FIREBALL,
                EntityType::DRAGON_FIREBALL,
                EntityType::WITHER_SKULL,
                EntityType::WIND_CHARGE,
                EntityType::BREEZE_WIND_CHARGE,
                EntityType::LLAMA_SPIT,
                EntityType::SHULKER_BULLET,
                EntityType::FIREWORK_ROCKET,
            ]
            .iter()
            .any(|ty| i32::from(ty.id) == kind)
        {
            let mut tail = &data[..];
            var(&mut tail)?;
            tail.read_slice_borrowed(24).map_err(|e| e.to_string())?;
            pumpkin_protocol::codec::lp_vector_3d::LpVector3d::read(&mut tail)
                .map_err(|e| e.to_string())?;
            tail.read_slice_borrowed(3).map_err(|e| e.to_string())?;
            data.truncate(data.len() - tail.len());
            entities.push(self.entity(spawn.data.0));
        }
        Ok(Packet {
            kind: p::ADD_ENTITY.0,
            entities,
            data,
        })
    }

    fn entity(&self, id: i32) -> Entity {
        self.ids.get(&id).cloned().unwrap_or(Entity::Unseen(id))
    }
}

fn var(input: &mut &[u8]) -> Result<i32, String> {
    input.get_var_int().map(|v| v.0).map_err(|e| e.to_string())
}

fn kind_name(kind: i32) -> String {
    macro_rules! names {
        ($($name:ident),+) => {
            match PacketId(kind) {
                $(p::$name => stringify!($name).to_ascii_lowercase(),)+
                _ => format!("{kind:#04x}"),
            }
        };
    }
    names!(
        ADD_ENTITY,
        ANIMATE,
        BLOCK_CHANGED_ACK,
        BLOCK_DESTRUCTION,
        BLOCK_ENTITY_DATA,
        BLOCK_EVENT,
        BLOCK_UPDATE,
        CONTAINER_CLOSE,
        CONTAINER_SET_CONTENT,
        CONTAINER_SET_DATA,
        CONTAINER_SET_SLOT,
        DAMAGE_EVENT,
        ENTITY_EVENT,
        ENTITY_POSITION_SYNC,
        EXPLODE,
        GAME_EVENT,
        HURT_ANIMATION,
        LEVEL_EVENT,
        LEVEL_PARTICLES,
        LIGHT_UPDATE,
        LOGIN,
        MOUNT_SCREEN_OPEN,
        MOVE_ENTITY_POS,
        MOVE_ENTITY_POS_ROT,
        MOVE_ENTITY_ROT,
        MOVE_MINECART_ALONG_TRACK,
        OPEN_SCREEN,
        PLAYER_LOOK_AT,
        PLAYER_POSITION,
        PLAYER_ROTATION,
        PROJECTILE_POWER,
        REMOVE_ENTITIES,
        ROTATE_HEAD,
        SECTION_BLOCKS_UPDATE,
        SET_CAMERA,
        SET_ENTITY_DATA,
        SET_ENTITY_LINK,
        SET_ENTITY_MOTION,
        SET_EQUIPMENT,
        SET_PASSENGERS,
        SET_TIME,
        SOUND,
        SOUND_ENTITY,
        STOP_SOUND,
        SWING_ANIMATION,
        SYSTEM_CHAT,
        TAKE_ITEM_ENTITY,
        TELEPORT_ENTITY,
        TICKING_STATE,
        TICKING_STEP,
        UPDATE_ATTRIBUTES,
        UPDATE_MOB_EFFECT
    )
}

fn render(packet: &Packet) -> String {
    let kind = kind_name(packet.kind);
    let data = &packet.data;
    let head = &data[..data.len().min(32)];
    let suffix = if data.len() > head.len() {
        let mut hash = String::with_capacity(64);
        for byte in Sha256::digest(data) {
            let _ = write!(hash, "{byte:02x}");
        }
        format!(" ({} bytes, SHA-256 {hash})", data.len())
    } else {
        String::new()
    };
    format!("{kind} {:?} {head:02x?}{suffix}", packet.entities)
}

pub fn compare(check: &mut Check<'_>, bot: &str, reference: &[Packet], subject: &[Packet]) {
    for index in 0..reference.len().max(subject.len()) {
        let a = reference.get(index);
        let b = subject.get(index);
        if a != b {
            check.add(
                None,
                format!("packet.{bot}.{index}"),
                a.map(render),
                b.map(render),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use pumpkin_protocol::{ClientPacket as _, VarInt};
    use pumpkin_util::math::vector3::Vector3;

    use super::*;

    fn entity(id: i32, x: f64, uuid: u128) -> Result<Vec<u8>, String> {
        let spawn = CSpawnEntity::new(
            VarInt(id),
            uuid::Uuid::from_u128(uuid),
            VarInt(1),
            Vector3::new(x, 151., 1.),
            0.,
            0.,
            0.,
            0.into(),
            Vector3::new(0., 0., 0.),
        );
        let mut bytes = Vec::new();
        spawn
            .write_packet_data(&mut bytes, &CURRENT_MC_VERSION)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    }

    #[test]
    fn maps_spawns_and_references_across_windows_without_hiding_shifted_targets()
    -> Result<(), String> {
        let (mut a, mut b) = (Decoder::default(), Decoder::default());
        assert_eq!(
            a.packet(p::ADD_ENTITY.0, &entity(1, 1., 1)?)?,
            b.packet(p::ADD_ENTITY.0, &entity(111, 1., 2)?)?,
        );
        assert_eq!(
            a.packet(p::ANIMATE.0, &[1, 2])?,
            b.packet(p::ANIMATE.0, &[111, 2])?,
        );
        b.packet(p::ADD_ENTITY.0, &entity(112, 2., 3)?)?;
        assert_ne!(
            a.packet(p::ANIMATE.0, &[1, 2])?,
            b.packet(p::ANIMATE.0, &[112, 2])?,
        );
        assert_ne!(
            a.packet(p::ANIMATE.0, &[1, 2])?,
            b.packet(p::ANIMATE.0, &[111, 3])?,
        );
        assert!(b.window(&[0, 0, 0, 2, 0, 0, 0, 3, 111]).is_err());
        assert!(b.packet(p::SET_TIME.0, &[0; 9])?.is_none());
        Ok(())
    }
}
