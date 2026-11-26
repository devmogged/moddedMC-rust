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
        }
    }
    pub fn limit(item: &str) -> usize {
        if item == NATURE {
            MAX_NEXT_BIOME
        } else {
            MAX_NEXT_STRUCTURE
        }
    }
    pub fn encode(&self, item: &str) -> Option<Vec<u8>> {
        if self.previous.len() > Self::limit(item) + 1 || (item == NATURE && self.is_group) {
            return None;
        }
        // Retain the old encoding for ordinary stacks. The extended encoding
        // adds one signed byte per Explorer position (vanilla locate_offset
        // components are bounded to ±16) and a group flag in the status byte.
        let extended =
            self.is_group || (item == EXPLORER && self.previous.iter().any(|p| p.y != 0));
        let mut out = Vec::new();
        out.extend_from_slice(if extended { b"MCc2" } else { b"MCc1" });
        out.extend_from_slice(&self.nonce.to_le_bytes());
        let status = match self.status {
            Status::Idle => 0,
            Status::Searching => 1,
            Status::Found => 2,
            Status::Failed => 3,
        };
        out.push(status | if self.is_group { 0x80 } else { 0 });
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
        let extended = match take(&mut data, 4)? {
            b"MCc1" => false,
            b"MCc2" if item == EXPLORER => true,
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
        if !data.is_empty()
            || (status == Status::Found && previous.is_empty())
            || (status != Status::Idle && (target.is_empty() || dimension.is_empty()))
        {
            return None;
        }
        Some(Self {
            nonce,
            target,
            is_group,
            dimension,
            status,
            previous,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
