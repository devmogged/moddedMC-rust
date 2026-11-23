//! Create 6.0.10 fluid transport, pressure, pipes and machines.

pub mod blocks;
pub mod connection_nbt;
mod flow_runtime;
pub mod flow_source;
pub(crate) mod item_drain;
pub mod network;
pub mod network_graph;
pub mod open_end;
pub mod pipe_connection;
pub mod pipe_states;
pub mod pump_graph;
pub mod runtime;
mod smart_tank;
pub(crate) mod spout;
pub mod tank;
pub(crate) mod tank_runtime;
pub mod transport;
pub(crate) mod valve;
pub mod vanilla_targets;
