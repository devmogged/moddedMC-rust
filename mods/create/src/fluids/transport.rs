//! The three-phase pipeline in Create 6.0.10's FluidTransportBehaviour.

use super::pipe_connection::{FluidStack, PipeConnection};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdatePhase {
    WaitForPumps,
    FlipFlows,
    Idle,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TickOutcome {
    WaitingForPumps,
    Flipped {
        changed: bool,
    },
    Collision {
        first: FluidStack,
        second: FluidStack,
    },
    Advanced {
        changed: bool,
    },
}

/// FluidTransportBehaviour.interfaces, with the connected faces specified by
/// the pipe's block state. The Source and FluidNetwork handlers are plugged
/// into the same tick pipeline when host fluid interfaces are available.
#[derive(Debug, Clone)]
pub struct Transport {
    pub phase: UpdatePhase,
    pub connections: [Option<PipeConnection>; 6],
}

impl Transport {
    pub fn new(connected_faces: impl IntoIterator<Item = u8>) -> Self {
        let mut connections = std::array::from_fn(|_| None);
        for face in connected_faces {
            if let Some(slot) = connections.get_mut(usize::from(face)) {
                *slot = Some(PipeConnection::new(face));
            }
        }
        Self {
            phase: UpdatePhase::WaitForPumps,
            connections,
        }
    }

    /// FluidTransportBehaviour.wipePressure, called when connectivity changes.
    pub fn wipe_pressure(&mut self, connected_faces: impl IntoIterator<Item = u8>) {
        let mut present = [false; 6];
        for face in connected_faces {
            if let Some(slot) = present.get_mut(usize::from(face)) {
                *slot = true;
            }
        }
        for (side, connection) in self.connections.iter_mut().enumerate() {
            if present[side] {
                connection
                    .get_or_insert_with(|| PipeConnection::new(side as u8))
                    .wipe_pressure();
            } else {
                *connection = None;
            }
        }
        self.phase = UpdatePhase::WaitForPumps;
    }

    /// The values come from FlowSource.provideFluid for each face. World
    /// mutations and fluid reactions remain at the caller's boundary.
    pub fn tick(&mut self, provided: &[Option<FluidStack>; 6]) -> TickOutcome {
        if self.phase == UpdatePhase::WaitForPumps {
            self.phase = UpdatePhase::FlipFlows;
            return TickOutcome::WaitingForPumps;
        }

        let mut changed = false;
        for connection in self.connections.iter_mut().flatten() {
            changed |= connection.flip_flows_if_pressure_reversed();
        }
        if self.phase == UpdatePhase::FlipFlows {
            self.phase = UpdatePhase::Idle;
            return TickOutcome::Flipped { changed };
        }

        let mut available: Option<FluidStack> = None;
        let mut single_source = None;
        for (face, connection) in self.connections.iter().enumerate() {
            let Some(inbound) = connection.as_ref().and_then(PipeConnection::provided_fluid) else {
                continue;
            };
            match &available {
                None => {
                    available = Some(inbound.clone());
                    single_source = Some(face);
                }
                Some(existing) if existing.same_fluid_same_components(inbound) => {
                    single_source = None;
                }
                Some(existing) => {
                    return TickOutcome::Collision {
                        first: existing.clone(),
                        second: inbound.clone(),
                    };
                }
            }
        }

        for (face, connection) in self.connections.iter_mut().enumerate() {
            let Some(connection) = connection else {
                continue;
            };
            let internal = (single_source != Some(face))
                .then_some(available.as_ref())
                .flatten();
            changed |= connection.manage_flows(provided[face].as_ref(), internal);
        }

        for connection in self.connections.iter_mut().flatten() {
            connection.tick_flow_progress();
        }
        TickOutcome::Advanced { changed }
    }
}

#[cfg(test)]
mod tests {
    use super::{TickOutcome, Transport, UpdatePhase};
    use crate::fluids::pipe_connection::{Flow, FluidStack};

    fn liquid(id: &str) -> FluidStack {
        FluidStack {
            id: id.to_owned(),
            amount: 1000,
            components: None,
        }
    }

    #[test]
    fn pump_phase_precedes_flow_and_flow_must_finish_before_outbound_starts() {
        let mut transport = Transport::new([2, 3]);
        transport.connections[2]
            .as_mut()
            .unwrap()
            .add_pressure(true, 128.0);
        transport.connections[3]
            .as_mut()
            .unwrap()
            .add_pressure(false, 128.0);
        let mut provided = std::array::from_fn(|_| None);
        provided[2] = Some(liquid("minecraft:water"));

        assert_eq!(transport.tick(&provided), TickOutcome::WaitingForPumps);
        assert_eq!(
            transport.tick(&provided),
            TickOutcome::Flipped { changed: false }
        );
        assert_eq!(transport.phase, UpdatePhase::Idle);
        assert_eq!(
            transport.tick(&provided),
            TickOutcome::Advanced { changed: true }
        );
        assert!(
            transport.connections[2]
                .as_ref()
                .and_then(|c| c.provided_fluid())
                .is_some()
        );
        assert!(
            transport.connections[3]
                .as_ref()
                .and_then(|c| c.provide_outbound_flow())
                .is_none()
        );

        assert_eq!(
            transport.tick(&provided),
            TickOutcome::Advanced { changed: true }
        );
        assert!(
            transport.connections[3]
                .as_ref()
                .and_then(|c| c.provide_outbound_flow())
                .is_some()
        );
    }

    #[test]
    fn a_single_inbound_face_does_not_feed_its_own_outbound_flow() {
        let mut transport = Transport::new([2]);
        transport.connections[2]
            .as_mut()
            .unwrap()
            .add_pressure(true, 128.0);
        let mut provided = std::array::from_fn(|_| None);
        provided[2] = Some(liquid("minecraft:water"));
        for _ in 0..5 {
            transport.tick(&provided);
        }
        assert!(
            transport.connections[2]
                .as_ref()
                .and_then(|c| c.provided_fluid())
                .is_some()
        );
        assert!(
            transport.connections[2]
                .as_ref()
                .and_then(|c| c.provide_outbound_flow())
                .is_none()
        );
    }

    #[test]
    fn two_different_completed_inbound_fluids_collide_before_flow_mutation() {
        let mut transport = Transport::new([2, 3, 4]);
        transport.phase = UpdatePhase::Idle;
        for (face, id) in [(2, "minecraft:water"), (3, "minecraft:lava")] {
            transport.connections[face]
                .as_mut()
                .unwrap()
                .add_pressure(true, 16.0);
            transport.connections[face].as_mut().unwrap().flow = Some(Flow {
                inbound: true,
                fluid: liquid(id),
                progress: 1.0,
                complete: true,
            });
        }
        let no_sources = std::array::from_fn(|_| None);
        assert_eq!(
            transport.tick(&no_sources),
            TickOutcome::Collision {
                first: liquid("minecraft:water"),
                second: liquid("minecraft:lava"),
            }
        );
        assert!(transport.connections[4].as_ref().unwrap().flow.is_none());
    }

    #[test]
    fn connectivity_change_wipes_pressure_and_restarts_two_wait_phases() {
        let mut transport = Transport::new([2]);
        transport.connections[2]
            .as_mut()
            .unwrap()
            .add_pressure(true, 16.0);
        transport.wipe_pressure([3]);
        assert!(transport.connections[2].is_none());
        assert_eq!(
            transport.connections[3].as_ref().unwrap().pressure,
            [0.0, 0.0]
        );
        assert_eq!(
            transport.tick(&std::array::from_fn(|_| None)),
            TickOutcome::WaitingForPumps
        );
    }
}
