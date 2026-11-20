//! Create train tracks, graph geometry and persisted global railway state.
mod bezier;
mod curve;
mod graph;
mod item;
mod math;
mod propagate;
mod railway;
#[cfg(test)]
mod reconcile;
mod runtime;
mod save;
mod track;
mod world_graph;

pub(crate) use item::use_on_block;
pub(crate) use railway::{
    rebuild_at, register as register_saved, require as require_saved, summary,
};
pub(super) use runtime::{describe_state, placed_state, register};
