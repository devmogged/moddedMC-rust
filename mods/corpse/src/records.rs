use std::{collections::hash_map::RandomState, hash::BuildHasher};

use fastnbt::ByteArray;
use moddedmc_api::player_death::Death;
use serde::{Deserialize, Serialize};

use crate::settings::{now_millis, settings};

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
    pub created_at: i64,
    #[serde(
        default = "history_default",
        skip_serializing_if = "history_is_default"
    )]
    recorded: bool,
    profile: ByteArray,
    pub pending: bool,
    pub present: bool,
    pub items: Vec<SavedStack>,
    pub original: Vec<SavedStack>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    overflow: Vec<ByteArray>,
    #[serde(default, skip_serializing_if = "is_zero")]
    next_page: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pages: Vec<OverflowPage>,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

const fn history_default() -> bool {
    true
}

const fn history_is_default(recorded: &bool) -> bool {
    *recorded
}

pub const PAGE_SLOTS: usize = 45;

#[derive(Clone, Serialize, Deserialize)]
pub struct OverflowPage {
    pub id: i32,
    pub slots: Vec<ByteArray>,
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
            created_at: now_millis(),
            recorded: settings().history_days != 0,
            profile: blob(&profile),
            pending: true,
            present: true,
            original: items.clone(),
            items,
            overflow: Vec::new(),
            next_page: 0,
            pages: Vec::new(),
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
        self.items.is_empty() && self.pages.is_empty() && self.overflow.is_empty()
    }

    pub fn is_recorded(&self, now: i64) -> bool {
        self.recorded && settings().keeps_history_at(self.created_at, now)
    }

    pub fn prune_history(&mut self, now: i64) {
        if !self.is_recorded(now) {
            self.recorded = false;
            if !self.present {
                // The carrier still needs its immutable equipment while the body exists.
                self.original.clear();
                self.profile = ByteArray::new(Vec::new());
            }
        }
    }

    pub fn migrate_overflow(&mut self) -> Option<()> {
        if self.overflow.is_empty() {
            return Some(());
        }
        let old = std::mem::take(&mut self.overflow);
        self.append_overflow(old)
    }

    pub fn append_overflow(&mut self, stacks: Vec<ByteArray>) -> Option<()> {
        // Existing pages can be open elsewhere. Only new pages receive new stacks.
        for slots in stacks.chunks(PAGE_SLOTS) {
            self.next_page = self.next_page.checked_add(1)?;
            self.pages.push(OverflowPage {
                id: self.next_page,
                slots: slots.to_vec(),
            });
        }
        Some(())
    }

    pub fn save_page(&mut self, page: i32, mut slots: Vec<ByteArray>) -> Option<()> {
        let index = self.pages.iter().position(|current| current.id == page)?;
        slots.truncate(
            slots
                .iter()
                .rposition(|stack| !stack.is_empty())
                .map_or(0, |index| index + 1),
        );
        if slots.is_empty() {
            self.pages.remove(index);
        } else {
            self.pages[index].slots = slots;
        }
        Some(())
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
