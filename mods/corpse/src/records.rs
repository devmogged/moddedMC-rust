use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    time::{SystemTime, UNIX_EPOCH},
};

use fastnbt::ByteArray;
use moddedmc_api::player_death::Death;
use serde::{Deserialize, Serialize};

pub fn blob(bytes: &[u8]) -> ByteArray {
    ByteArray::new(bytes.iter().copied().map(u8::cast_signed).collect())
}

pub fn unblob(bytes: &ByteArray) -> Vec<u8> {
    bytes.iter().copied().map(i8::cast_unsigned).collect()
}

fn new_actor_uuid() -> [u8; 16] {
    let salt = RandomState::new();
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&salt.hash_one(0_u8).to_be_bytes());
    bytes[8..].copy_from_slice(&salt.hash_one(1_u8).to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    bytes
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SavedStack {
    pub slot: i32,
    bytes: ByteArray,
}

impl SavedStack {
    pub fn new(slot: i32, bytes: &[u8]) -> Self {
        Self {
            slot,
            bytes: blob(bytes),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        unblob(&self.bytes)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Grave {
    pub id: i64,
    actor_uuid: ByteArray,
    player_uuid: ByteArray,
    pub owner: String,
    pub dimension: String,
    pub position: [f64; 3],
    pub body_position: [f64; 3],
    pub yaw: f32,
    created_at: i64,
    profile: ByteArray,
    pub pending: bool,
    pub present: bool,
    pub items: Vec<SavedStack>,
    pub original: Vec<SavedStack>,
    pub overflow: Vec<ByteArray>,
}

impl Grave {
    pub fn new(id: i64, dimension: &str, death: &Death<'_>, profile: Vec<u8>) -> Self {
        let items: Vec<_> = death
            .drops()
            .map(|stack| SavedStack::new(stack.slot as i32, stack.stack))
            .collect();
        Self {
            id,
            actor_uuid: blob(&new_actor_uuid()),
            player_uuid: blob(&death.player_uuid),
            owner: death.player.name().to_owned(),
            dimension: dimension.to_owned(),
            position: death.position,
            body_position: death.position,
            yaw: death.player.position().yaw,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |time| {
                    i64::try_from(time.as_millis()).unwrap_or(i64::MAX)
                }),
            profile: blob(&profile),
            pending: true,
            present: true,
            original: items.clone(),
            items,
            overflow: Vec::new(),
        }
    }

    pub fn uuid(&self) -> Option<[u8; 16]> {
        unblob(&self.actor_uuid).try_into().ok()
    }

    pub fn owns(&self, uuid: &[u8; 16]) -> bool {
        self.player_uuid.len() == uuid.len()
            && self
                .player_uuid
                .iter()
                .copied()
                .map(i8::cast_unsigned)
                .eq(uuid.iter().copied())
    }

    pub fn profile(&self) -> Vec<u8> {
        unblob(&self.profile)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.overflow.is_empty()
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Archive {
    pub next_id: i64,
    pub deaths: Vec<Grave>,
}

impl Archive {
    pub fn get(&self, id: i64) -> Option<&Grave> {
        let index = self
            .deaths
            .binary_search_by_key(&id, |grave| grave.id)
            .ok()?;
        Some(&self.deaths[index])
    }

    pub fn get_mut(&mut self, id: i64) -> Option<&mut Grave> {
        let index = self
            .deaths
            .binary_search_by_key(&id, |grave| grave.id)
            .ok()?;
        Some(&mut self.deaths[index])
    }
}
