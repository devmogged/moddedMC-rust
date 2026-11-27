//! Bounded reads from the active world generator, independent of loaded chunks.
pub use crate::worldgen_groups::{StructureGroup, StructureGroups, require_groups};
use crate::{InterfaceUnavailable, Pos, abi, content::World, require_interface};
use std::ffi::c_void;

#[derive(Clone, Copy)]
pub struct WorldgenLookup(&'static abi::WorldgenLookupV1);

pub fn require() -> Result<WorldgenLookup, InterfaceUnavailable> {
    // SAFETY: this is the published table for worldgen_lookup v1.
    unsafe { require_interface(abi::WORLDGEN_LOOKUP, 1) }.map(WorldgenLookup)
}

unsafe extern "C" fn collect(ctx: *mut c_void, key: u64, value: abi::Str) {
    // SAFETY: the synchronous host callback borrows this vector and the identifier.
    let entries = unsafe { &mut *ctx.cast::<Vec<(u64, String)>>() };
    entries.push((key, unsafe { value.str() }.to_owned()));
}

fn sink(entries: &mut Vec<(u64, String)>) -> abi::Sink {
    abi::Sink {
        ctx: std::ptr::from_mut(entries).cast(),
        put: collect,
    }
}

impl WorldgenLookup {
    pub fn height_bounds(self, world: World<'_>) -> Option<(i32, i32)> {
        let (mut bottom, mut top) = (0, 0);
        // SAFETY: live world and writable outputs.
        unsafe { (self.0.height_bounds)(world.raw(), &mut bottom, &mut top) }
            .then_some((bottom, top))
    }
    pub fn biomes(self, world: World<'_>) -> Option<Vec<String>> {
        let mut entries = Vec::new();
        // SAFETY: world and sink are live for this synchronous call.
        unsafe { (self.0.biomes)(world.raw(), sink(&mut entries)) }
            .then(|| entries.into_iter().map(|(_, id)| id).collect())
    }

    /// Each returned identifier corresponds to the same index in the input slice.
    pub fn noise_biomes(self, world: World<'_>, quart: &[Pos]) -> Option<Vec<String>> {
        if quart.len() > 256 {
            return None;
        }
        let mut entries = Vec::new();
        // SAFETY: all arguments are live for this synchronous call.
        if !unsafe {
            (self.0.noise_biomes)(world.raw(), quart.as_ptr(), quart.len(), sink(&mut entries))
        } {
            return None;
        }
        entries.sort_unstable_by_key(|(index, _)| *index);
        if entries.len() != quart.len()
            || entries
                .iter()
                .enumerate()
                .any(|(i, (key, id))| *key != i as u64 || id.is_empty())
        {
            return None;
        }
        Some(entries.into_iter().map(|(_, id)| id).collect())
    }
}

#[derive(Clone, Copy)]
pub struct Structures(&'static abi::WorldgenStructuresV1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lookup<T> {
    Found(T),
    Absent,
    Pending,
    Unavailable,
}

pub fn require_structures() -> Result<Structures, InterfaceUnavailable> {
    // SAFETY: this is the published table for worldgen_structures v1.
    unsafe { require_interface(abi::WORLDGEN_STRUCTURES, 1) }.map(Structures)
}

unsafe extern "C" fn collect_bytes(ctx: *mut c_void, key: u64, value: abi::Str) {
    // SAFETY: synchronous callback borrows the vector and bytes.
    unsafe { &mut *ctx.cast::<Vec<(u64, Vec<u8>)>>() }
        .push((key, unsafe { value.bytes() }.to_vec()));
}

fn byte_sink(entries: &mut Vec<(u64, Vec<u8>)>) -> abi::Sink {
    abi::Sink {
        ctx: std::ptr::from_mut(entries).cast(),
        put: collect_bytes,
    }
}

impl Structures {
    pub fn structures(self, world: World<'_>) -> Option<Vec<String>> {
        let mut entries = Vec::new();
        // SAFETY: world and sink are live for this call.
        unsafe { (self.0.structures)(world.raw(), sink(&mut entries)) }
            .then(|| entries.into_iter().map(|(_, id)| id).collect())
    }

    /// Placement index and spacing in chunks (zero means concentric rings).
    pub fn placements(self, world: World<'_>, target: &str) -> Option<Vec<(u32, i32)>> {
        let mut entries = Vec::new();
        // SAFETY: all arguments are borrowed for the call.
        if !unsafe {
            (self.0.placements)(world.raw(), abi::Str::from(target), byte_sink(&mut entries))
        } {
            return None;
        }
        entries
            .into_iter()
            .map(|(key, bytes)| {
                let spacing = i32::from_le_bytes(bytes.try_into().ok()?);
                (spacing >= 0).then_some((u32::try_from(key).ok()?, spacing))
            })
            .collect()
    }

    pub fn spread_candidate(self, world: World<'_>, placement: u32, x: i32, z: i32) -> Option<Pos> {
        let mut out = Pos::default();
        // SAFETY: live world and writable output.
        unsafe { (self.0.spread_candidate)(world.raw(), placement, x, z, &mut out) }.then_some(out)
    }

    pub fn ring_chunks(self, world: World<'_>, placement: u32) -> Lookup<Vec<Pos>> {
        let mut entries = Vec::new();
        // SAFETY: live world and synchronous sink.
        match unsafe { (self.0.ring_chunks)(world.raw(), placement, byte_sink(&mut entries)) } {
            1 => {
                let decoded: Option<Vec<_>> = entries
                    .into_iter()
                    .map(|(_, bytes)| {
                        let [a, b, c, d, e, f, g, h] = <[u8; 8]>::try_from(bytes).ok()?;
                        Some(Pos {
                            x: i32::from_le_bytes([a, b, c, d]),
                            y: 0,
                            z: i32::from_le_bytes([e, f, g, h]),
                        })
                    })
                    .collect();
                decoded.map_or(Lookup::Unavailable, Lookup::Found)
            }
            2 => Lookup::Pending,
            _ => Lookup::Unavailable,
        }
    }

    pub fn structure_start(
        self,
        world: World<'_>,
        placement: u32,
        target: &str,
        x: i32,
        z: i32,
    ) -> Lookup<Pos> {
        let mut out = Pos::default();
        // SAFETY: live world and borrowed identifier/output.
        match unsafe {
            (self.0.structure_start)(
                world.raw(),
                placement,
                abi::Str::from(target),
                x,
                z,
                &mut out,
            )
        } {
            1 => Lookup::Found(out),
            0 => Lookup::Absent,
            2 => Lookup::Pending,
            _ => Lookup::Unavailable,
        }
    }
}
