//! Create train tracks, graph geometry and persisted global railway state.
mod bezier;
pub(super) mod bogey;
mod carriage;
pub(super) mod controls;
mod curve;
mod driving;
mod graph;
mod item;
mod math;
#[cfg(test)]
mod observer;
mod placement;
mod propagate;
mod railway;
mod reconcile;
mod runtime;
mod save;
mod sections;
pub(crate) mod signal;
pub(crate) mod station;
mod targeting;
mod track;
mod travelling_point;
mod visual;
mod world_graph;

pub(crate) use driving::TrainRuntime;
pub(crate) use item::use_on_block;
pub(crate) use railway::{
    RailUpdates, rebuild_at, register as register_saved, require as require_saved, summary,
};
pub(super) use runtime::{describe_state, placed_state, register};
