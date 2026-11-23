//! FluidNetwork.tick's simulated and executed IFluidHandler transfers.
//! Topology discovery and the persistent world endpoints are provided by the caller.

use super::pipe_connection::FluidStack;
use std::collections::HashMap;

/// The subset of NeoForge IFluidHandler used by Create's FluidNetwork.tick.
/// A host adapter must preserve simulation semantics and fluid components.
pub trait FluidHandler {
    fn tank_count(&self) -> usize;
    fn fluid_in_tank(&self, tank: usize) -> Option<FluidStack>;
    fn drain(&mut self, request: &FluidStack, simulate: bool) -> Option<FluidStack>;
    fn drain_any(&mut self, amount: u32, simulate: bool) -> Option<FluidStack>;
    fn fill(&mut self, offered: &FluidStack, simulate: bool) -> u32;
    /// Identifies the underlying handler, not an adapter or queried face.
    /// All faces of one tank must share this ID for the duration of a tick;
    /// distinct tanks must have distinct IDs. The eventual host adapter must
    /// obtain this from the host (TODO(abi): #42), not its wrapper address.
    fn handler_identity(&self) -> u64;
}

/// FluidNetwork uses one half of inbound pressure (minimum 1 mB/tick).
pub fn transfer_speed(inbound_pressure: f32) -> u32 {
    (inbound_pressure / 2.0).max(1.0) as u32
}

fn choose_drain(
    source: &mut dyn FluidHandler,
    fluid: &FluidStack,
    amount: u32,
    simulate: bool,
) -> Option<FluidStack> {
    for tank in 0..source.tank_count() {
        let Some(contained) = source.fluid_in_tank(tank) else {
            continue;
        };
        if contained.is_empty() || !contained.same_fluid_same_components(fluid) {
            continue;
        }
        let mut request = contained;
        request.amount = amount;
        if let Some(drained) = source.drain(&request, simulate).filter(|s| !s.is_empty()) {
            return Some(drained);
        }
        break;
    }
    source
        .drain_any(amount, simulate)
        .filter(|s| !s.is_empty() && s.same_fluid_same_components(fluid))
}

/// FluidNetwork.tick's two-phase transfer after the flow graph finds its
/// source and targets. Returns the actual quantity accepted by endpoints.
///
/// As with Java Create, EXECUTE drains the source first and then fills the
/// targets. The caller must keep endpoint access sequential on the server tick.
pub fn transfer_tick(
    source: &mut dyn FluidHandler,
    targets: &mut [&mut dyn FluidHandler],
    flow_fluid: &FluidStack,
    inbound_pressure: f32,
) -> u32 {
    if flow_fluid.is_empty() || targets.is_empty() {
        return 0;
    }
    let mut speed = transfer_speed(inbound_pressure);
    let mut accumulated_fill = HashMap::<u64, u32>::new();
    let mut accepted = 0u32;

    for simulate in [true, false] {
        let Some(mut transfer) = choose_drain(source, flow_fluid, speed, simulate) else {
            return accepted;
        };
        if simulate {
            speed = transfer.amount;
        }
        let mut available: Vec<usize> = (0..targets.len()).collect();

        while !available.is_empty() && transfer.amount > 0 {
            let divided = transfer.amount / available.len() as u32;
            let mut remainder = transfer.amount % available.len() as u32;
            let mut still_available = Vec::with_capacity(available.len());

            for &index in &available {
                if transfer.amount == 0 {
                    break;
                }
                let mut to_transfer = divided;
                if remainder > 0 {
                    to_transfer += 1;
                    remainder -= 1;
                }

                let handler_identity = targets[index].handler_identity();
                let previously_filled = *accumulated_fill.get(&handler_identity).unwrap_or(&0);
                let simulated_amount = if simulate {
                    to_transfer.saturating_add(previously_filled)
                } else {
                    to_transfer
                };
                let mut divided_stack = transfer.clone();
                divided_stack.amount = simulated_amount;
                let raw_fill = targets[index]
                    .fill(&divided_stack, simulate)
                    .min(simulated_amount);
                let filled = if simulate {
                    accumulated_fill.insert(handler_identity, raw_fill.max(previously_filled));
                    raw_fill.saturating_sub(previously_filled).min(to_transfer)
                } else {
                    raw_fill
                };
                transfer.amount = transfer.amount.saturating_sub(filled);
                if !simulate {
                    accepted += filled;
                }

                if filled >= simulated_amount {
                    still_available.push(index);
                }
            }
            available = still_available;
        }
        speed = speed.saturating_sub(transfer.amount);
    }

    accepted
}

#[cfg(test)]
mod tests {
    use super::{FluidHandler, transfer_speed, transfer_tick};
    use crate::fluids::pipe_connection::FluidStack;

    #[derive(Debug)]
    struct Tank {
        fluid: FluidStack,
        capacity: u32,
        simulated: usize,
        executed: usize,
    }

    impl Tank {
        fn new(id: &str, amount: u32, capacity: u32) -> Self {
            Self {
                fluid: FluidStack {
                    id: id.into(),
                    amount,
                    components: None,
                },
                capacity,
                simulated: 0,
                executed: 0,
            }
        }
    }

    impl FluidHandler for Tank {
        fn handler_identity(&self) -> u64 {
            self as *const Self as usize as u64
        }

        fn tank_count(&self) -> usize {
            1
        }

        fn fluid_in_tank(&self, index: usize) -> Option<FluidStack> {
            (index == 0).then(|| self.fluid.clone())
        }

        fn drain(&mut self, request: &FluidStack, simulate: bool) -> Option<FluidStack> {
            if self.fluid.is_empty() || !self.fluid.same_fluid_same_components(request) {
                return None;
            }
            let amount = self.fluid.amount.min(request.amount);
            if simulate {
                self.simulated += 1;
            } else {
                self.executed += 1;
                self.fluid.amount -= amount;
            }
            Some(FluidStack {
                amount,
                ..self.fluid.clone()
            })
        }

        fn drain_any(&mut self, amount: u32, simulate: bool) -> Option<FluidStack> {
            let mut request = self.fluid.clone();
            request.amount = amount;
            self.drain(&request, simulate)
        }

        fn fill(&mut self, offered: &FluidStack, simulate: bool) -> u32 {
            if !self.fluid.same_fluid_same_components(offered) {
                return 0;
            }
            let accepted = offered.amount.min(self.capacity - self.fluid.amount);
            if simulate {
                self.simulated += 1;
            } else {
                self.executed += 1;
                self.fluid.amount += accepted;
            }
            accepted
        }
    }

    #[test]
    fn pressure_to_millibuckets_matches_create_limits() {
        assert_eq!(transfer_speed(0.0), 1);
        assert_eq!(transfer_speed(1.0), 1);
        assert_eq!(transfer_speed(63.0), 31);
        assert_eq!(transfer_speed(128.0), 64);
        assert_eq!(transfer_speed(256.0), 128);
    }

    #[test]
    fn full_target_redistributes_remainder_to_available_targets() {
        let mut source = Tank::new("minecraft:water", 100, 100);
        let mut narrow = Tank::new("minecraft:water", 0, 20);
        let mut wide = Tank::new("minecraft:water", 0, 100);
        let fluid = source.fluid.clone();

        let moved = transfer_tick(&mut source, &mut [&mut narrow, &mut wide], &fluid, 128.0);

        assert_eq!(moved, 64);
        assert_eq!(source.fluid.amount, 36);
        assert_eq!(narrow.fluid.amount, 20);
        assert_eq!(wide.fluid.amount, 44);
        assert!(source.simulated > 0 && source.executed > 0);
        assert!(wide.simulated > 0 && wide.executed > 0);
    }

    #[test]
    fn simulation_limits_source_drain_to_available_output_capacity() {
        let mut source = Tank::new("minecraft:water", 50, 100);
        let mut a = Tank::new("minecraft:water", 0, 5);
        let mut b = Tank::new("minecraft:water", 0, 5);
        let fluid = source.fluid.clone();

        let moved = transfer_tick(&mut source, &mut [&mut a, &mut b], &fluid, 128.0);

        assert_eq!(moved, 10);
        assert_eq!(source.fluid.amount, 40);
        assert_eq!(a.fluid.amount, 5);
        assert_eq!(b.fluid.amount, 5);
        assert!(source.simulated > 0 && source.executed > 0);
    }

    #[test]
    fn no_matching_fluid_or_no_targets_does_not_consume_source() {
        let mut source = Tank::new("minecraft:water", 30, 100);
        let mut sink = Tank::new("minecraft:lava", 0, 100);
        let water = source.fluid.clone();

        assert_eq!(
            transfer_tick(&mut source, &mut [&mut sink], &water, 256.0),
            0
        );
        assert_eq!(source.fluid.amount, 30);
        assert_eq!(transfer_tick(&mut source, &mut [], &water, 256.0), 0);
        assert_eq!(source.fluid.amount, 30);
    }
}

#[cfg(test)]
mod shared_handler_regression {
    use super::{FluidHandler, transfer_tick};
    use crate::fluids::pipe_connection::FluidStack;
    use std::{cell::RefCell, rc::Rc};

    fn water(amount: u32) -> FluidStack {
        FluidStack {
            id: "minecraft:water".into(),
            amount,
            components: None,
        }
    }
    struct Source {
        remaining: u32,
    }
    impl FluidHandler for Source {
        fn handler_identity(&self) -> u64 {
            self as *const Self as usize as u64
        }
        fn tank_count(&self) -> usize {
            1
        }
        fn fluid_in_tank(&self, tank: usize) -> Option<FluidStack> {
            (tank == 0).then(|| water(self.remaining))
        }
        fn drain(&mut self, request: &FluidStack, simulate: bool) -> Option<FluidStack> {
            let count = self.remaining.min(request.amount);
            if !simulate {
                self.remaining -= count;
            }
            Some(water(count))
        }
        fn drain_any(&mut self, amount: u32, simulate: bool) -> Option<FluidStack> {
            self.drain(&water(amount), simulate)
        }
        fn fill(&mut self, _offered: &FluidStack, _simulate: bool) -> u32 {
            0
        }
    }
    struct AliasTarget {
        shared: Rc<RefCell<u32>>,
    }
    impl FluidHandler for AliasTarget {
        fn handler_identity(&self) -> u64 {
            Rc::as_ptr(&self.shared) as usize as u64
        }
        fn tank_count(&self) -> usize {
            1
        }
        fn fluid_in_tank(&self, _tank: usize) -> Option<FluidStack> {
            Some(water(*self.shared.borrow()))
        }
        fn drain(&mut self, _request: &FluidStack, _simulate: bool) -> Option<FluidStack> {
            None
        }
        fn drain_any(&mut self, _amount: u32, _simulate: bool) -> Option<FluidStack> {
            None
        }
        fn fill(&mut self, offered: &FluidStack, simulate: bool) -> u32 {
            let accepted = offered.amount.min(40 - *self.shared.borrow());
            if !simulate {
                *self.shared.borrow_mut() += accepted;
            }
            accepted
        }
    }
    #[test]
    fn same_handler_referenced_by_two_endpoint_faces_does_not_lose_fluid() {
        let shared = Rc::new(RefCell::new(0u32));
        let mut left = AliasTarget {
            shared: Rc::clone(&shared),
        };
        let mut right = AliasTarget {
            shared: Rc::clone(&shared),
        };
        let mut source = Source { remaining: 64 };
        let moved = transfer_tick(&mut source, &mut [&mut left, &mut right], &water(64), 128.0);
        assert_eq!(
            moved,
            *shared.borrow(),
            "reported transfer must equal delivered"
        );
        assert_eq!(
            source.remaining,
            64 - moved,
            "source fluid must be conserved"
        );
    }

    #[test]
    fn aliased_faces_preserve_mass_at_multiple_pressures() {
        for face_count in 1..=6 {
            for initial in [0u32, 1, 20, 39, 40] {
                for pressure in [1.0f32, 16.0, 80.0, 128.0, 256.0] {
                    let shared = Rc::new(RefCell::new(initial));
                    let mut adapters = (0..face_count)
                        .map(|_| AliasTarget {
                            shared: Rc::clone(&shared),
                        })
                        .collect::<Vec<_>>();
                    let mut targets = adapters
                        .iter_mut()
                        .map(|target| target as &mut dyn FluidHandler)
                        .collect::<Vec<_>>();
                    let mut source = Source { remaining: 128 };
                    let moved = transfer_tick(&mut source, &mut targets, &water(128), pressure);
                    let stored = *shared.borrow();

                    assert_eq!(
                        source.remaining,
                        128 - moved,
                        "source loss with {face_count} faces, {initial} initially, {pressure} pressure"
                    );
                    assert_eq!(
                        stored,
                        initial + moved,
                        "sink loss with {face_count} faces, {initial} initially, {pressure} pressure"
                    );
                    assert_eq!(moved, (40 - initial).min(super::transfer_speed(pressure)));
                }
            }
        }
    }

    #[test]
    fn distinct_handlers_do_not_share_one_capacity_budget() {
        let first = Rc::new(RefCell::new(0u32));
        let second = Rc::new(RefCell::new(0u32));
        let mut left_face = AliasTarget {
            shared: Rc::clone(&first),
        };
        let mut right_face = AliasTarget {
            shared: Rc::clone(&first),
        };
        let mut other = AliasTarget {
            shared: Rc::clone(&second),
        };
        let mut source = Source { remaining: 128 };

        let moved = transfer_tick(
            &mut source,
            &mut [&mut left_face, &mut right_face, &mut other],
            &water(128),
            256.0,
        );

        assert_eq!(moved, 80);
        assert_eq!(source.remaining, 48);
        assert_eq!(*first.borrow(), 40);
        assert_eq!(*second.borrow(), 40);
    }
}
