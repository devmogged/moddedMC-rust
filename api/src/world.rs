//! `moddedmc:world/1`: batched block reads and writes (see [`crate::abi::WorldV1`]).

use crate::{
    Pos,
    abi::{BlockWrite, NO_STATE, Str, WorldV1},
    interface,
};

/// The world interface, if the server offers it.
#[must_use]
pub fn get() -> Option<World> {
    interface::<WorldV1>(crate::abi::WORLD_V1).map(World)
}

/// Batched block access. Block states are vanilla state ids, valid for this Minecraft version only:
/// keep ids in memory, save names.
#[derive(Clone, Copy)]
pub struct World(&'static WorldV1);

impl World {
    /// The state ids at `positions` in `dimension`, `None` where the chunk is not loaded or the
    /// position is outside the world. Empty if there is no such dimension.
    #[must_use]
    pub fn get_blocks(&self, dimension: &str, positions: &[Pos]) -> Vec<Option<u32>> {
        let mut out = vec![NO_STATE; positions.len()];
        // SAFETY: `out` has room for one id per position; everything is borrowed for the call.
        let found = unsafe {
            (self.0.get_blocks)(
                Str::from(dimension),
                positions.as_ptr(),
                positions.len(),
                out.as_mut_ptr(),
            )
        };
        if !found {
            return Vec::new();
        }
        out.into_iter()
            .map(|s| (s != NO_STATE).then_some(s))
            .collect()
    }

    /// Writes blocks in order, with neighbor and client updates. Returns how many were written.
    pub fn set_blocks(&self, dimension: &str, writes: &[(Pos, u32)]) -> usize {
        let writes: Vec<BlockWrite> = writes
            .iter()
            .map(|&(pos, state)| BlockWrite { pos, state })
            .collect();
        // SAFETY: borrowed for the call.
        unsafe { (self.0.set_blocks)(Str::from(dimension), writes.as_ptr(), writes.len()) }
    }

    /// `minecraft:oak_log[axis=x]` to a state id. Properties left out take the block's default.
    #[must_use]
    pub fn state_id(&self, name: &str) -> Option<u32> {
        // SAFETY: borrowed for the call.
        let id = unsafe { (self.0.state_id)(Str::from(name)) };
        (id != NO_STATE).then_some(id)
    }

    /// A state id's full name, e.g. `minecraft:oak_log[axis=x]`.
    #[must_use]
    pub fn state_name(&self, state: u32) -> Option<String> {
        let mut name = None;
        crate::with_sink(
            |sink| {
                // SAFETY: the sink is live for the call.
                unsafe { (self.0.state_name)(state, sink) }
            },
            |_, bytes| name = Some(String::from_utf8_lossy(bytes).into_owned()),
        );
        name
    }
}
