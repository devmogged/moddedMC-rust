// WS-3: track block content can be placed without train-entity host APIs.
mod math;
mod runtime;
mod track;

// The remaining railway systems are tested against Java source fixtures but
// not loaded until the world-wide saved-data host interface is available.
#[cfg(test)]
mod bezier;
#[cfg(test)]
mod curve;
#[cfg(test)]
mod graph;
mod item;
#[cfg(test)]
mod propagate;
#[cfg(test)]
mod save;

pub(crate) use item::use_on_block;
pub(super) use runtime::{describe_state, placed_state, register};
