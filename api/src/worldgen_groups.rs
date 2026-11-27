//! Active structure registry groups in their declaration order.
//! A group may contain members unavailable in the current dimension.
use crate::{InterfaceUnavailable, abi, content::World, require_interface};
use std::{collections::HashSet, ffi::c_void};

#[derive(Clone, Copy)]
pub struct StructureGroups(&'static abi::WorldgenStructureGroupsV1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureGroup {
    pub id: String,
    pub members: Vec<String>,
}

pub fn require_groups() -> Result<StructureGroups, InterfaceUnavailable> {
    // SAFETY: this is the published structure groups v1 table.
    unsafe { require_interface(abi::WORLDGEN_STRUCTURE_GROUPS, 1) }.map(StructureGroups)
}

unsafe extern "C" fn collect(ctx: *mut c_void, key: u64, value: abi::Str) {
    // SAFETY: the synchronous sink borrows the vector and bytes.
    unsafe { &mut *ctx.cast::<Vec<(u64, Vec<u8>)>>() }
        .push((key, unsafe { value.bytes() }.to_vec()));
}

fn decode(mut entries: Vec<(u64, Vec<u8>)>) -> Option<Vec<StructureGroup>> {
    entries.sort_unstable_by_key(|(key, _)| *key);
    let mut seen = HashSet::new();
    entries
        .into_iter()
        .enumerate()
        .map(|(i, (key, data))| {
            if key != i as u64 {
                return None;
            }
            let text = String::from_utf8(data).ok()?;
            let mut fields = text.split('\0');
            let id = fields.next()?.to_owned();
            let members: Vec<_> = fields.map(str::to_owned).collect();
            if id.is_empty()
                || members.is_empty()
                || members.iter().any(String::is_empty)
                || !seen.insert(id.clone())
            {
                return None;
            }
            Some(StructureGroup { id, members })
        })
        .collect()
}

impl StructureGroups {
    pub fn groups(self, world: World<'_>) -> Option<Vec<StructureGroup>> {
        let mut entries: Vec<(u64, Vec<u8>)> = Vec::new();
        let sink = abi::Sink {
            ctx: std::ptr::from_mut(&mut entries).cast(),
            put: collect,
        };
        // SAFETY: the world and sink live through this synchronous call.
        if !unsafe { (self.0.groups)(world.raw(), sink) } {
            return None;
        }
        decode(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_group_and_member_order() {
        let groups = decode(vec![
            (1, b"b:group\0b:z\0b:a".to_vec()),
            (0, b"a:group\0b:z\0a:a".to_vec()),
        ])
        .unwrap();
        assert_eq!(groups[0].id, "a:group");
        assert_eq!(groups[1].members, ["b:z", "b:a"]);
    }

    #[test]
    fn rejects_ambiguous_or_malformed_records() {
        for records in [
            vec![(0, b"a:group".to_vec())],
            vec![(0, b"a:group\0".to_vec())],
            vec![(1, b"a:group\0a:member".to_vec())],
            vec![(0, vec![0xff])],
            vec![(0, b"a:group\0a:a".to_vec()), (1, b"a:group\0a:b".to_vec())],
        ] {
            assert!(decode(records).is_none());
        }
    }
}
