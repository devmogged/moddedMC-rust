use moddedmc_api::Pos;
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const NATURE: &str = "naturescompass:naturescompass";
pub const EXPLORER: &str = "explorerscompass:explorerscompass";
pub const MAX_NEXT_BIOME: usize = 25;
pub const MAX_NEXT_STRUCTURE: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Idle,
    Searching,
    Found,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub nonce: u128,
    pub target: String,
    /// Explorer group searches store the actual found member in target and
    /// recover its first registered group when searching for the next result.
    pub is_group: bool,
    pub dimension: String,
    pub status: Status,
    pub previous: Vec<Pos>,
    /// Successful non-creative searches since the last fresh-item repair.
    /// Separate from search history: reset/failure must never repair a compass.
    pub wear: u16,
}

pub fn nonce() -> u128 {
    static SEQ: AtomicU64 = AtomicU64::new(1);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    (time << 32) ^ u128::from(SEQ.fetch_add(1, Ordering::Relaxed))
}

impl State {
    pub fn blank() -> Self {
        Self {
            nonce: nonce(),
            target: String::new(),
            is_group: false,
            dimension: String::new(),
            status: Status::Idle,
            previous: Vec::new(),
            wear: 0,
        }
    }

    /// Reset the needle/history and retire any old search, preserving wear.
    pub fn reset_search(&mut self) {
        let wear = self.wear;
        *self = Self::blank();
        self.wear = wear;
    }

    pub fn broken(&self, durability: u16) -> bool {
        durability != 0 && self.wear >= durability
    }

    /// Compute the next wear value for a successful result. The host must
    /// publish it together with the exact-stack result CAS and visible damage.
    pub fn successful_wear(&self, durability: u16, infinite_materials: bool) -> u16 {
        if durability == 0 || infinite_materials || self.broken(durability) {
            self.wear
        } else {
            self.wear + 1
        }
    }
    pub fn limit(item: &str) -> usize {
        if item == NATURE {
            MAX_NEXT_BIOME
        } else {
            MAX_NEXT_STRUCTURE
        }
    }

    /// A searching stack is durable, but its worker is tied to a live
    /// player and world. Recover a lost worker as a failed search. Rotate
    /// the nonce so any delayed result or dialog cannot revive it.
    pub fn interrupt(&mut self) {
        self.fail();
        self.nonce = nonce();
    }

    pub fn fail(&mut self) {
        self.status = Status::Failed;
        self.is_group = false;
        self.previous.clear();
    }

    /// The sole source of truth for the vanilla client's needle.
    /// A saved result remains tied to the dimension where it was located;
    /// the current player's dimension must never retarget the component.
    pub fn tracker(&self) -> Option<(&str, Pos)> {
        if self.status != Status::Found {
            return None;
        }
        self.previous
            .last()
            .copied()
            .map(|p| (self.dimension.as_str(), p))
    }

    fn valid(&self, item: &str) -> bool {
        fn resource_id(id: &str) -> bool {
            let Some((namespace, path)) = id.split_once(':') else {
                return false;
            };
            let common = |c: u8| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'-' | b'.')
            };
            !namespace.is_empty()
                && !path.is_empty()
                && namespace.bytes().all(common)
                && path.bytes().all(|c| common(c) || c == b'/')
        }
        if !matches!(item, NATURE | EXPLORER)
            || self.previous.len() > Self::limit(item) + 1
            || (item == NATURE && self.is_group)
            || self.wear > 10_000
        {
            return false;
        }
        match self.status {
            Status::Idle => {
                self.target.is_empty()
                    && self.dimension.is_empty()
                    && !self.is_group
                    && self.previous.is_empty()
            }
            Status::Failed | Status::Found | Status::Searching => {
                resource_id(&self.target)
                    && resource_id(&self.dimension)
                    && self.dimension.len() <= 256
                    && match self.status {
                        Status::Failed => !self.is_group && self.previous.is_empty(),
                        Status::Found => !self.previous.is_empty(),
                        _ => true,
                    }
                    && self.previous.iter().all(|p| {
                        p.x.unsigned_abs() <= 30_000_000
                            && p.z.unsigned_abs() <= 30_000_000
                            && (-2048..=2047).contains(&p.y)
                    })
            }
        }
    }

    pub fn encode(&self, item: &str) -> Option<Vec<u8>> {
        if !self.valid(item) {
            return None;
        }
        // Retain the old encoding for ordinary stacks. The extended encoding
        // adds one signed byte per Explorer position (vanilla locate_offset
        // components are bounded to ±16) and a group flag in the status byte.
        // MCc3 stores wear as a u16 and uses the same compact XYZ layout
        // as MCc2 for Explorer. Keep old versions for pristine stacks.
        let worn = self.wear != 0;
        let extended =
            item == EXPLORER && (worn || self.is_group || self.previous.iter().any(|p| p.y != 0));
        let mut out = Vec::new();
        out.extend_from_slice(if worn {
            b"MCc3"
        } else if extended {
            b"MCc2"
        } else {
            b"MCc1"
        });
        out.extend_from_slice(&self.nonce.to_le_bytes());
        let status = match self.status {
            Status::Idle => 0,
            Status::Searching => 1,
            Status::Found => 2,
            Status::Failed => 3,
        };
        out.push(status | if self.is_group { 0x80 } else { 0 });
        if worn {
            out.extend_from_slice(&self.wear.to_le_bytes());
        }
        for s in [&self.target, &self.dimension] {
            out.extend_from_slice(&u16::try_from(s.len()).ok()?.to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        }
        out.push(u8::try_from(self.previous.len()).ok()?);
        for p in &self.previous {
            out.extend_from_slice(&p.x.to_le_bytes());
            if item == NATURE {
                out.extend_from_slice(&p.y.to_le_bytes());
            } else if extended {
                if !(-16..=16).contains(&p.y) {
                    return None;
                }
                out.extend_from_slice(&i8::try_from(p.y).ok()?.to_le_bytes());
            }
            out.extend_from_slice(&p.z.to_le_bytes());
        }
        (out.len() <= moddedmc_api::interaction::MAX_STACK_DATA).then_some(out)
    }
    pub fn decode(item: &str, mut data: &[u8]) -> Option<Self> {
        fn take<'a>(data: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
            let (head, rest) = data.split_at_checked(n)?;
            *data = rest;
            Some(head)
        }
        fn string(data: &mut &[u8]) -> Option<String> {
            let len = u16::from_le_bytes(take(data, 2)?.try_into().ok()?) as usize;
            Some(std::str::from_utf8(take(data, len)?).ok()?.to_owned())
        }
        fn i32_field(data: &mut &[u8]) -> Option<i32> {
            Some(i32::from_le_bytes(take(data, 4)?.try_into().ok()?))
        }
        if data.len() > moddedmc_api::interaction::MAX_STACK_DATA {
            return None;
        }
        let (extended, worn) = match take(&mut data, 4)? {
            b"MCc1" => (false, false),
            b"MCc2" if item == EXPLORER => (true, false),
            b"MCc3" => (item == EXPLORER, true),
            _ => return None,
        };
        let nonce = u128::from_le_bytes(take(&mut data, 16)?.try_into().ok()?);
        let status_byte = take(&mut data, 1)?[0];
        let is_group = status_byte & 0x80 != 0;
        if is_group && !extended {
            return None;
        }
        let status = match status_byte & 0x7f {
            0 => Status::Idle,
            1 => Status::Searching,
            2 => Status::Found,
            3 => Status::Failed,
            _ => return None,
        };
        let wear = if worn {
            let wear = u16::from_le_bytes(take(&mut data, 2)?.try_into().ok()?);
            if !(1..=10_000).contains(&wear) {
                return None;
            }
            wear
        } else {
            0
        };
        let target = string(&mut data)?;
        let dimension = string(&mut data)?;
        let n = take(&mut data, 1)?[0] as usize;
        if n > Self::limit(item) + 1 {
            return None;
        }
        let mut previous = Vec::with_capacity(n);
        for _ in 0..n {
            let x = i32_field(&mut data)?;
            let y = if item == NATURE {
                i32_field(&mut data)?
            } else if extended {
                let y = i32::from(i8::from_le_bytes(take(&mut data, 1)?.try_into().ok()?));
                if !(-16..=16).contains(&y) {
                    return None;
                }
                y
            } else {
                0
            };
            let z = i32_field(&mut data)?;
            previous.push(Pos { x, y, z });
        }
        if !data.is_empty() {
            return None;
        }
        let state = Self {
            nonce,
            target,
            is_group,
            dimension,
            status,
            previous,
            wear,
        };
        state.valid(item).then_some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wear_is_persistent_across_reset_failure_and_history_migration() {
        for item in [NATURE, EXPLORER] {
            let mut state = State::blank();
            let legacy = state.encode(item).unwrap();
            assert_eq!(State::decode(item, &legacy).unwrap().wear, 0);
            state.target = "test:member".into();
            state.dimension = "minecraft:overworld".into();
            state.status = Status::Found;
            state.is_group = item == EXPLORER;
            state.previous.push(Pos {
                x: -287,
                y: 7,
                z: 6,
            });
            assert_eq!(state.successful_wear(2, true), 0);
            assert_eq!(state.successful_wear(0, false), 0);
            state.wear = state.successful_wear(2, false);
            let bytes = state.encode(item).unwrap();
            assert_eq!(&bytes[..4], b"MCc3");
            assert_eq!(State::decode(item, &bytes), Some(state.clone()));
            for len in 0..bytes.len() {
                assert!(State::decode(item, &bytes[..len]).is_none());
            }
            let mut invalid = bytes.clone();
            invalid[21..23].copy_from_slice(&10_001_u16.to_le_bytes());
            assert!(State::decode(item, &invalid).is_none());
            state.fail();
            assert_eq!(state.wear, 1);
            state.reset_search();
            assert_eq!(state.wear, 1);
            assert_eq!(state.status, Status::Idle);
            assert_eq!(state.tracker(), None);
            assert_ne!(state.nonce, State::decode(item, &bytes).unwrap().nonce);
            assert!(!state.broken(2));
            state.wear = state.successful_wear(2, false);
            assert!(state.broken(2));
            assert!(!state.broken(0));
            assert_eq!(state.successful_wear(2, false), 2);
            assert_eq!(
                State::decode(item, &state.encode(item).unwrap()),
                Some(state)
            );
            assert_eq!(State::blank().wear, 0);
        }
    }

    #[test]
    fn max_default_history_and_wear_fit_in_one_stack() {
        let mut state = State::blank();
        state.status = Status::Found;
        state.target = "minecraft:buried_treasure".into();
        state.dimension = "minecraft:overworld".into();
        state.is_group = true;
        state.wear = 10_000;
        state.previous = (0..101)
            .map(|x| Pos {
                x: x * 16 + 9,
                y: 7,
                z: -x * 16 + 9,
            })
            .collect();
        let bytes = state.encode(EXPLORER).unwrap();
        assert_eq!(State::decode(EXPLORER, &bytes), Some(state));
    }

    #[test]
    fn full_structure_history_and_malformed_state() {
        let mut state = State::blank();
        state.status = Status::Found;
        state.target = "minecraft:stronghold".into();
        state.dimension = "minecraft:overworld".into();
        state.previous = (0..101)
            .map(|x| Pos {
                x: x * 16,
                y: 0,
                z: -x * 16,
            })
            .collect();
        let bytes = state.encode(EXPLORER).unwrap();
        assert_eq!(&bytes[..4], b"MCc1");
        assert_eq!(State::decode(EXPLORER, &bytes), Some(state));
        for len in 0..bytes.len() {
            assert!(State::decode(EXPLORER, &bytes[..len]).is_none());
        }
        let mut invalid = bytes;
        invalid.push(1);
        assert!(State::decode(EXPLORER, &invalid).is_none());
    }

    #[test]
    fn group_and_locate_offset_history_round_trip_within_stack_limit() {
        let mut state = State::blank();
        state.status = Status::Found;
        state.target = "minecraft:buried_treasure".into();
        state.dimension = "minecraft:overworld".into();
        state.is_group = true;
        state.previous = (0..101)
            .map(|x| Pos {
                x: x * 16 + 9,
                y: x % 33 - 16,
                z: -x * 16 + 9,
            })
            .collect();
        let bytes = state.encode(EXPLORER).unwrap();
        assert_eq!(&bytes[..4], b"MCc2");
        assert_eq!(State::decode(EXPLORER, &bytes), Some(state.clone()));
        assert!(State::decode(NATURE, &bytes).is_none());
        assert!(state.encode(NATURE).is_none());
        state.previous[0].y = 17;
        assert!(state.encode(EXPLORER).is_none());
    }

    #[test]
    fn interrupted_group_search_invalidates_old_worker_and_history() {
        let mut state = State::blank();
        state.status = Status::Searching;
        state.target = "minecraft:villages".into();
        state.dimension = "minecraft:overworld".into();
        state.is_group = true;
        state.previous.push(Pos { x: 16, y: 7, z: 32 });
        let saved = state.encode(EXPLORER).unwrap();
        let mut recovered = State::decode(EXPLORER, &saved).unwrap();
        recovered.interrupt();
        assert_ne!(recovered.nonce, state.nonce);
        assert_eq!(recovered.status, Status::Failed);
        assert_eq!(recovered.target, state.target);
        assert_eq!(recovered.dimension, state.dimension);
        assert!(!recovered.is_group);
        assert!(recovered.previous.is_empty());
        assert_eq!(
            State::decode(EXPLORER, &recovered.encode(EXPLORER).unwrap()),
            Some(recovered)
        );
    }

    #[test]
    fn tracker_projection_uses_saved_dimension_and_only_found_state() {
        let mut state = State::blank();
        assert_eq!(state.tracker(), None);
        state.target = "test:member_a".into();
        state.dimension = "test:other_world".into();
        state.previous.push(Pos {
            x: 16,
            y: 7,
            z: -32,
        });
        state.status = Status::Searching;
        assert_eq!(state.tracker(), None);
        state.status = Status::Found;
        let bytes = state.encode(EXPLORER).unwrap();
        let restored = State::decode(EXPLORER, &bytes).unwrap();
        assert_eq!(
            restored.tracker(),
            Some((
                "test:other_world",
                Pos {
                    x: 16,
                    y: 7,
                    z: -32
                }
            ))
        );
        state.fail();
        assert_eq!(state.tracker(), None);
    }

    #[test]
    fn malformed_metadata_cannot_supply_a_tracker_or_stale_history() {
        let mut state = State::blank();
        state.status = Status::Found;
        state.target = "test:target".into();
        state.dimension = "test:world".into();
        state.previous.push(Pos { x: 16, y: 0, z: 32 });
        let bytes = state.encode(EXPLORER).unwrap();
        // Replace the first namespace colon with an invalid character.
        let mut bad_id = bytes.clone();
        bad_id[27] = b'!';
        assert!(State::decode(EXPLORER, &bad_id).is_none());
        // The state byte must be consistent with the stored history.
        let mut bad_status = bytes.clone();
        bad_status[20] = 3;
        assert!(State::decode(EXPLORER, &bad_status).is_none());
        let mut bad_pos = bytes.clone();
        let x_offset = bad_pos.len() - 8;
        bad_pos[x_offset..x_offset + 4].copy_from_slice(&30_000_001_i32.to_le_bytes());
        assert!(State::decode(EXPLORER, &bad_pos).is_none());
        state.previous[0].x = 30_000_001;
        assert!(state.encode(EXPLORER).is_none());
        state.fail();
        assert!(state.encode(EXPLORER).is_some());
        state.status = Status::Found;
        state.previous.push(Pos {
            x: 0,
            y: 2048,
            z: 0,
        });
        assert!(state.encode(NATURE).is_none());
    }
}
