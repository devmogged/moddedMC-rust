//! `moddedmc:biomes/1`: biomes mods add and paint into the world (see
//! [`crate::abi::BiomesV1`]). Define them in [`crate::Mod::define_blocks`] with
//! [`crate::blocks::Definitions::biome`].

use crate::{
    abi::{self, BiomesV1, Pos},
    interface, s, with_sink,
};

pub(crate) fn table() -> Option<&'static BiomesV1> {
    interface::<BiomesV1>(abi::BIOMES_V1)
}

/// The biomes interface, if the server offers it.
#[must_use]
pub fn get() -> Option<Biomes> {
    table().map(Biomes)
}

#[derive(Clone, Copy)]
pub struct Biomes(&'static BiomesV1);

impl Biomes {
    /// Sets the biome of the 4x4x4 cell holding `pos`, e.g. from `chunk_generated`. False if the
    /// biome is unknown or the chunk is not loaded.
    pub fn set(&self, dimension: &str, pos: Pos, biome: &str) -> bool {
        // SAFETY: borrowed for the call.
        unsafe { (self.0.set_biome)(s(dimension), pos, s(biome)) }
    }

    /// The biome at `pos` (`minecraft:plains`); `None` if the chunk is not loaded.
    #[must_use]
    pub fn at(&self, dimension: &str, pos: Pos) -> Option<String> {
        let mut name = None;
        // SAFETY: borrowed for the call; the sink is live for it.
        with_sink(
            |sink| unsafe { (self.0.biome_at)(s(dimension), pos, sink) },
            |_, b| name = Some(String::from_utf8_lossy(b).into_owned()),
        );
        name
    }
}
