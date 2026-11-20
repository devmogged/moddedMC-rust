use std::collections::BTreeMap;

use pumpkin_data::{
    attributes::Attributes,
    entity::{EntityPose, EntityType},
    meta_data_type::MetaDataType as Meta,
    packet::{CURRENT_MC_VERSION, PacketId, clientbound::play as p},
    tracked_data::{entity, living_entity, shulker},
};
use pumpkin_protocol::{
    ServerPacket as _, VarInt,
    codec::lp_vector_3d::LpVector3d,
    java::client::play::{CSetEquipment, CSpawnEntity},
    ser::{NetworkReadExt as _, NetworkReadSliceExt as _, ReadingError},
};
use pumpkin_util::math::{
    boundingbox::{BoundingBox, EntityDimensions},
    vector3::Vector3,
};

use crate::{attributes::Values, controls::yaw_sin_cos};

#[derive(Default)]
pub struct Entities {
    tracked: BTreeMap<i32, Tracked>,
}

pub struct Tracked {
    pub kind: &'static EntityType,
    anchor: Vector3<f64>,
    pub position: Vector3<f64>,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Vector3<f64>,
    pub on_ground: bool,
    pub saddled: bool,
    attributes: Values,
    pose: EntityPose,
    health: f32,
    attach_face: i32,
    peek_target: f32,
    peek: f32,
}

impl Tracked {
    pub fn attribute(&self, attribute: &Attributes, fallback: f64) -> f64 {
        let base = self
            .kind
            .attributes
            .iter()
            .find_map(|(key, value)| (key.id == attribute.id).then_some(*value));
        self.attributes.get(attribute, base.unwrap_or(fallback))
    }

    pub fn dimensions(&self) -> EntityDimensions {
        self.kind.pose_dimensions(self.pose, false).map_or_else(
            || {
                EntityDimensions::new(
                    self.kind.dimension[0],
                    self.kind.dimension[1],
                    self.kind.eye_height,
                )
            },
            |pose| pose.dimensions,
        )
    }

    fn bounds(&self) -> BoundingBox {
        let body = BoundingBox::new_from_pos(
            self.position.x,
            self.position.y,
            self.position.z,
            &self.dimensions(),
        );
        if self.kind.id != EntityType::SHULKER.id {
            return body;
        }
        let lid = f64::from(0.5 - yaw_sin_cos((0.5 + self.peek) * 180.0).0 as f32 * 0.5);
        let (x, y, z) = match self.attach_face {
            1 => (0.0, -lid, 0.0),
            2 => (0.0, 0.0, lid),
            3 => (0.0, 0.0, -lid),
            4 => (lid, 0.0, 0.0),
            5 => (-lid, 0.0, 0.0),
            _ => (0.0, lid, 0.0),
        };
        body.expand_towards(x, y, z)
    }

    fn update_metadata(&mut self, mut bytes: &[u8]) -> Result<(), ReadingError> {
        let version = CURRENT_MC_VERSION;
        loop {
            let field = bytes.get_u8()?;
            if field == 0xff {
                return Ok(());
            }
            let kind = Meta {
                id: bytes.get_var_int()?.0,
            };
            match kind {
                Meta::BYTE => {
                    let value = bytes.get_u8()?;
                    if self.kind.id == EntityType::SHULKER.id
                        && field == shulker::DATA_PEEK_ID.id.get(&version)
                    {
                        self.peek_target = f32::from(value.min(100)) * 0.01;
                    }
                }
                Meta::INT | Meta::DIRECTION | Meta::POSE | Meta::OPTIONAL_UNSIGNED_INT => {
                    let value = bytes.get_var_int()?.0;
                    if field == entity::DATA_POSE.id.get(&version) {
                        self.pose = match value {
                            1 => EntityPose::FallFlying,
                            2 => EntityPose::Sleeping,
                            3 => EntityPose::Swimming,
                            4 => EntityPose::SpinAttack,
                            5 => EntityPose::Crouching,
                            7 => EntityPose::Dying,
                            _ => EntityPose::Standing,
                        };
                    } else if self.kind.id == EntityType::SHULKER.id
                        && field == shulker::DATA_ATTACH_FACE_ID.id.get(&version)
                    {
                        self.attach_face = value;
                    }
                }
                Meta::FLOAT => {
                    let value = bytes.get_f32_be()?;
                    if (self.kind.mob || self.kind.id == EntityType::PLAYER.id)
                        && field == living_entity::DATA_HEALTH_ID.id.get(&version)
                    {
                        self.health = value;
                    }
                }
                Meta::BOOLEAN => {
                    bytes.get_bool()?;
                }
                Meta::COMPONENT => {
                    bytes.get_nbt(&version)?;
                }
                Meta::OPTIONAL_COMPONENT => {
                    if bytes.get_bool()? {
                        bytes.get_nbt(&version)?;
                    }
                }
                Meta::PARTICLES => {
                    if bytes.get_var_int()?.0 != 0 {
                        return Ok(());
                    }
                }
                Meta::OPTIONAL_GLOBAL_POS => {
                    if bytes.get_bool()? {
                        bytes.get_str_borrowed()?;
                        bytes.get_block_pos(&version)?;
                    }
                }
                // shortcut: collision-relevant base fields only. Extend when a fixture needs later metadata.
                _ => return Ok(()),
            }
        }
    }
}

impl Entities {
    pub fn observe(&mut self, packet: i32, mut bytes: &[u8]) -> Result<(), ReadingError> {
        match PacketId(packet) {
            p::ADD_ENTITY => {
                let spawn = CSpawnEntity::read_packet_data(bytes, &CURRENT_MC_VERSION)?;
                if let Some(kind) = u16::try_from(spawn.r#type.0)
                    .ok()
                    .and_then(EntityType::from_raw)
                {
                    self.tracked.insert(
                        spawn.entity_id.0,
                        Tracked {
                            kind,
                            anchor: spawn.position,
                            position: spawn.position,
                            yaw: unpack_degrees(spawn.yaw),
                            pitch: unpack_degrees(spawn.pitch),
                            velocity: spawn.velocity.0,
                            on_ground: false,
                            saddled: false,
                            attributes: Values::default(),
                            pose: EntityPose::Standing,
                            health: 1.0,
                            attach_face: 0,
                            peek_target: 0.0,
                            peek: 0.0,
                        },
                    );
                }
            }
            p::REMOVE_ENTITIES => {
                for _ in 0..count(&mut bytes, 1)? {
                    self.tracked.remove(&bytes.get_var_int()?.0);
                }
            }
            p::RESPAWN => self.tracked.clear(),
            p::SET_EQUIPMENT => {
                let mut header = bytes;
                let id = header.get_var_int()?.0;
                if let Some(entity) = self.tracked.get_mut(&id)
                    && [EntityType::HORSE.id, EntityType::PIG.id].contains(&entity.kind.id)
                {
                    let equipment = CSetEquipment::read(&mut bytes, &CURRENT_MC_VERSION)?;
                    if let Some((_, saddle)) =
                        equipment.equipment.iter().find(|(slot, _)| *slot == 7)
                    {
                        entity.saddled = !saddle.0.is_empty();
                    }
                }
            }
            p::MOVE_ENTITY_POS
            | p::MOVE_ENTITY_POS_ROT
            | p::MOVE_ENTITY_ROT
            | p::ENTITY_POSITION_SYNC
            | p::TELEPORT_ENTITY
            | p::SET_ENTITY_MOTION
            | p::UPDATE_ATTRIBUTES
            | p::SET_ENTITY_DATA => {
                let id = bytes.get_var_int()?.0;
                if let Some(entity) = self.tracked.get_mut(&id) {
                    update_entity(entity, PacketId(packet), bytes)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub fn kind(&self, id: i32) -> Option<&'static EntityType> {
        self.tracked.get(&id).map(|entity| entity.kind)
    }

    pub fn get(&self, id: i32) -> Option<&Tracked> {
        self.tracked.get(&id)
    }

    pub fn target(&self, name: &str, anchor: Vector3<f64>) -> Result<VarInt, String> {
        let kind =
            EntityType::from_name(name).ok_or_else(|| format!("unknown entity type {name}"))?;
        let mut targets = self.tracked.iter().filter(|(_, entity)| {
            entity.kind.id == kind.id
                && (entity.anchor.x - anchor.x).abs() <= 1e-6
                && (entity.anchor.y - anchor.y).abs() <= 1e-6
                && (entity.anchor.z - anchor.z).abs() <= 1e-6
        });
        let (&id, _) = targets
            .next()
            .ok_or_else(|| format!("no visible {name} spawned at {anchor:?}"))?;
        if targets.next().is_some() {
            return Err(format!("multiple visible {name} spawned at {anchor:?}"));
        }
        Ok(VarInt(id))
    }

    pub fn tick(&mut self) {
        for entity in self.tracked.values_mut() {
            entity.peek += (entity.peek_target - entity.peek).clamp(-0.05, 0.05);
        }
    }

    pub fn solid_shapes(&self, area: BoundingBox) -> Vec<BoundingBox> {
        self.solid_shapes_except(area, None)
    }

    pub fn solid_shapes_except(
        &self,
        area: BoundingBox,
        excluded: Option<i32>,
    ) -> Vec<BoundingBox> {
        self.tracked
            .iter()
            .filter(|(id, entity)| {
                Some(**id) != excluded
                    && entity.health > 0.0
                    && (entity.kind.boat.is_some() || entity.kind.id == EntityType::SHULKER.id)
            })
            .map(|(_, entity)| entity.bounds())
            .filter(|bounds| bounds.intersects(&area))
            .collect()
    }

    pub fn push_at(&self, position: Vector3<f64>, body: BoundingBox) -> Vector3<f64> {
        let mut total = Vector3::default();
        for entity in self.tracked.values() {
            if entity.health <= 0.0 || entity.kind.id == EntityType::SHULKER.id {
                continue;
            }
            let bounds = entity.bounds();
            let peer = if entity.kind.boat.is_some() {
                body.min.y <= bounds.min.y
                    && bounds
                        .expand(f64::from(0.2f32), -f64::from(0.01f32), f64::from(0.2f32))
                        .intersects(&body)
            } else {
                (entity.kind.id == EntityType::PLAYER.id || entity.kind.mob)
                    && bounds.intersects(&body)
            };
            if !peer {
                continue;
            }
            let dx = position.x - entity.position.x;
            let dz = position.z - entity.position.z;
            let distance = dx.abs().max(dz.abs());
            if distance >= f64::from(0.01f32) {
                let root = distance.sqrt();
                let strength = (1.0 / root).min(1.0);
                total.x += dx / root * strength * f64::from(0.05f32);
                total.z += dz / root * strength * f64::from(0.05f32);
            }
        }
        total
    }
}

fn update_entity(
    entity: &mut Tracked,
    kind: PacketId,
    mut bytes: &[u8],
) -> Result<(), ReadingError> {
    match kind {
        p::MOVE_ENTITY_POS | p::MOVE_ENTITY_POS_ROT => {
            let packed = bytes.get_var_int()?.0 as u32;
            entity.on_ground = packed & 1 != 0;
            let steps = (packed >> 1) as usize;
            if steps == 0 {
                entity.position = decode_delta(entity.position, &mut bytes)?;
            } else {
                if steps > bytes.len() / 7 {
                    return invalid_path();
                }
                for _ in 0..steps {
                    bytes.get_var_int()?;
                    entity.position = decode_delta(entity.position, &mut bytes)?;
                }
            }
            if kind == p::MOVE_ENTITY_POS_ROT {
                entity.yaw = unpack_degrees(bytes.get_u8()?);
                entity.pitch = unpack_degrees(bytes.get_u8()?);
            }
        }
        p::MOVE_ENTITY_ROT => {
            entity.on_ground = bytes.get_bool()?;
            entity.yaw = unpack_degrees(bytes.get_u8()?);
            entity.pitch = unpack_degrees(bytes.get_u8()?);
        }
        p::ENTITY_POSITION_SYNC => {
            match bytes.get_var_int()?.0 {
                0 => entity.position = vector(&mut bytes)?,
                1 => {
                    let steps = count(&mut bytes, 25)?;
                    if steps == 0 {
                        return invalid_path();
                    }
                    for _ in 0..steps {
                        entity.position = vector(&mut bytes)?;
                        bytes.get_var_int()?;
                    }
                }
                _ => return invalid_path(),
            }
            entity.yaw = bytes.get_f32_be()?;
            entity.pitch = bytes.get_f32_be()?;
            entity.on_ground = bytes.get_bool()?;
        }
        p::TELEPORT_ENTITY => {
            let pos = vector(&mut bytes)?;
            let velocity = vector(&mut bytes)?;
            let yaw = bytes.get_f32_be()?;
            let pitch = bytes.get_f32_be()?;
            let flags = bytes.get_i32_be()?;
            entity.on_ground = bytes.get_bool()?;
            let absolute = |old, target, flag| {
                if flags & flag == 0 {
                    target
                } else {
                    old + target
                }
            };
            entity.position = Vector3::new(
                absolute(entity.position.x, pos.x, 1),
                absolute(entity.position.y, pos.y, 2),
                absolute(entity.position.z, pos.z, 4),
            );
            entity.yaw = yaw + if flags & 8 != 0 { entity.yaw } else { 0.0 };
            entity.pitch = pitch + if flags & 16 != 0 { entity.pitch } else { 0.0 };
            entity.velocity = Vector3::new(
                absolute(entity.velocity.x, velocity.x, 32),
                absolute(entity.velocity.y, velocity.y, 64),
                absolute(entity.velocity.z, velocity.z, 128),
            );
        }
        p::SET_ENTITY_MOTION => entity.velocity = LpVector3d::read(&mut bytes)?.0,
        p::UPDATE_ATTRIBUTES => entity.attributes.update(bytes)?,
        p::SET_ENTITY_DATA
            if entity.kind.boat.is_some()
                || entity.kind.mob
                || entity.kind.id == EntityType::PLAYER.id =>
        {
            entity.update_metadata(bytes)?;
        }
        _ => {}
    }
    Ok(())
}

fn decode_delta(base: Vector3<f64>, bytes: &mut &[u8]) -> Result<Vector3<f64>, ReadingError> {
    let add = |base: f64, delta: i16| {
        if delta == 0 {
            base
        } else {
            ((base * 4096.0 + 0.5).floor() + f64::from(delta)) / 4096.0
        }
    };
    Ok(Vector3::new(
        add(base.x, bytes.get_i16_be()?),
        add(base.y, bytes.get_i16_be()?),
        add(base.z, bytes.get_i16_be()?),
    ))
}

fn vector(bytes: &mut &[u8]) -> Result<Vector3<f64>, ReadingError> {
    Ok(Vector3::new(
        bytes.get_f64_be()?,
        bytes.get_f64_be()?,
        bytes.get_f64_be()?,
    ))
}

fn count(bytes: &mut &[u8], minimum_size: usize) -> Result<usize, ReadingError> {
    let count = bytes.get_var_int()?.0 as usize;
    if count > bytes.len() / minimum_size {
        return invalid_path();
    }
    Ok(count)
}

fn invalid_path<T>() -> Result<T, ReadingError> {
    Err(ReadingError::Message(
        "invalid entity movement path length or type".into(),
    ))
}

fn unpack_degrees(angle: u8) -> f32 {
    f32::from(angle as i8) * (360.0 / 256.0)
}
