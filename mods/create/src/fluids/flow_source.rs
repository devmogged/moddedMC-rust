//! Fluid source resolution from Create 6.0.10 PipeConnection and FlowSource.

use super::network::FluidHandler;
use super::pipe_connection::{FluidStack, PipeConnection};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    OpenEnd,
    FluidHandler,
    OtherPipe,
    Blocked,
}

/// PipeConnection.determineSource chooses the first applicable source type.
/// An unloaded neighbor cannot be a source yet; querying it must not load it.
pub fn determine_source(
    neighbor_loaded: bool,
    open_end: bool,
    has_fluid_handler: bool,
    has_transport: bool,
) -> Option<SourceKind> {
    if !neighbor_loaded {
        return None;
    }
    if open_end {
        Some(SourceKind::OpenEnd)
    } else if has_fluid_handler {
        Some(SourceKind::FluidHandler)
    } else if has_transport {
        Some(SourceKind::OtherPipe)
    } else {
        Some(SourceKind::Blocked)
    }
}

/// FlowSource.provideFluid: prefer a simulated one-mB generic drain; only if
/// the extraction predicate rejects it inspect each fluid tank in order.
/// Predicate decisions on an empty generic drain matter: Java returns it.
pub fn provide_fluid_from_handler(
    handler: &mut dyn FluidHandler,
    predicate: impl Fn(&FluidStack) -> bool,
) -> Option<FluidStack> {
    let immediate = handler.drain_any(1, true);
    let empty = FluidStack {
        id: String::new(),
        amount: 0,
        components: None,
    };
    if predicate(immediate.as_ref().unwrap_or(&empty)) {
        return immediate.filter(|fluid| !fluid.is_empty());
    }

    for tank in 0..handler.tank_count() {
        let Some(contained) = handler.fluid_in_tank(tank) else {
            continue;
        };
        if contained.is_empty() || !predicate(&contained) {
            continue;
        }
        let mut request = contained;
        request.amount = 1;
        return handler
            .drain(&request, true)
            .filter(|fluid| !fluid.is_empty());
    }
    None
}

/// FlowSource.OtherPipe.provideFluid uses the neighbor's outward completed
/// flow, and then applies the requesting pipe's extraction predicate.
pub fn provide_from_other_pipe(
    outward: Option<&PipeConnection>,
    predicate: impl Fn(&FluidStack) -> bool,
) -> Option<FluidStack> {
    outward
        .and_then(PipeConnection::provide_outbound_flow)
        .filter(|fluid| !fluid.is_empty() && predicate(fluid))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::{
        SourceKind, determine_source, provide_fluid_from_handler, provide_from_other_pipe,
    };
    use crate::fluids::network::FluidHandler;
    use crate::fluids::pipe_connection::{Flow, FluidStack, PipeConnection};

    fn fluid(id: &str, amount: u32) -> FluidStack {
        FluidStack {
            id: id.into(),
            amount,
            components: None,
        }
    }

    #[derive(Default)]
    struct Source {
        tanks: Vec<FluidStack>,
        generic: Option<FluidStack>,
        requests: Vec<(String, bool)>,
    }

    impl FluidHandler for Source {
        fn tank_count(&self) -> usize {
            self.tanks.len()
        }

        fn fluid_in_tank(&self, index: usize) -> Option<FluidStack> {
            self.tanks.get(index).cloned()
        }

        fn drain(&mut self, requested: &FluidStack, simulate: bool) -> Option<FluidStack> {
            self.requests.push((requested.id.clone(), simulate));
            self.tanks
                .iter()
                .find(|stack| stack.same_fluid_same_components(requested) && !stack.is_empty())
                .map(|stack| fluid(&stack.id, stack.amount.min(requested.amount)))
        }

        fn drain_any(&mut self, amount: u32, simulate: bool) -> Option<FluidStack> {
            self.requests.push(("*".into(), simulate));
            self.generic
                .as_ref()
                .map(|stack| fluid(&stack.id, stack.amount.min(amount)))
        }

        fn fill(&mut self, _offered: &FluidStack, _simulate: bool) -> u32 {
            0
        }
    }

    #[test]
    fn source_detection_respects_loaded_gate_and_priority() {
        assert_eq!(determine_source(false, true, true, true), None);
        assert_eq!(
            determine_source(true, true, true, true),
            Some(SourceKind::OpenEnd)
        );
        assert_eq!(
            determine_source(true, false, true, true),
            Some(SourceKind::FluidHandler)
        );
        assert_eq!(
            determine_source(true, false, false, true),
            Some(SourceKind::OtherPipe)
        );
        assert_eq!(
            determine_source(true, false, false, false),
            Some(SourceKind::Blocked)
        );
    }

    #[test]
    fn handler_falls_back_to_component_matching_tank_only_after_predicate_rejection() {
        let mut handler = Source {
            generic: Some(fluid("minecraft:lava", 1000)),
            tanks: vec![fluid("minecraft:lava", 1000), fluid("minecraft:water", 500)],
            ..Default::default()
        };
        let extracted =
            provide_fluid_from_handler(&mut handler, |stack| stack.id == "minecraft:water");
        assert_eq!(extracted, Some(fluid("minecraft:water", 1)));
        assert_eq!(
            handler.requests,
            [("*".into(), true), ("minecraft:water".into(), true)]
        );
    }

    #[test]
    fn handler_does_not_inspect_tanks_when_generic_drain_passes_predicate() {
        let mut handler = Source {
            generic: Some(fluid("minecraft:water", 900)),
            tanks: vec![fluid("minecraft:lava", 800)],
            ..Default::default()
        };
        let extracted = provide_fluid_from_handler(&mut handler, |_| true);
        assert_eq!(extracted, Some(fluid("minecraft:water", 1)));
        assert_eq!(handler.requests, [("*".into(), true)]);
    }

    #[test]
    fn other_pipe_provides_only_complete_outbound_flow_with_matching_fluid() {
        let mut connection = PipeConnection::new(5);
        connection.flow = Some(Flow {
            fluid: fluid("minecraft:water", 1000),
            inbound: false,
            complete: false,
            progress: 0.5,
        });
        assert_eq!(provide_from_other_pipe(Some(&connection), |_| true), None);
        connection.flow.as_mut().unwrap().complete = true;
        assert_eq!(
            provide_from_other_pipe(Some(&connection), |stack| stack.id == "minecraft:water"),
            Some(fluid("minecraft:water", 1000))
        );
        assert_eq!(
            provide_from_other_pipe(Some(&connection), |stack| stack.id == "minecraft:lava"),
            None
        );
        connection.flow.as_mut().unwrap().inbound = true;
        assert_eq!(provide_from_other_pipe(Some(&connection), |_| true), None);
    }
}
