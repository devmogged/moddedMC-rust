//! The fluid transfer rules in Create 6.0.10's PipeConnection.

use fastnbt::Value;

/// A NeoForge FluidStack. The amount is not part of fluid/component identity.
#[derive(Clone, Debug, PartialEq)]
pub struct FluidStack {
    pub id: String,
    pub amount: u32,
    pub components: Option<Value>,
}

impl FluidStack {
    pub fn same_fluid_same_components(&self, other: &Self) -> bool {
        self.id == other.id && self.normalized_components() == other.normalized_components()
    }

    fn normalized_components(&self) -> Option<&Value> {
        match self.components.as_ref() {
            None => None,
            Some(Value::Compound(fields)) if fields.is_empty() => None,
            other => other,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.amount == 0 || self.id.is_empty() || self.id == "minecraft:empty"
    }
}

/// PipeConnection.Flow: one directional, progressing, component-bearing flow.
#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub inbound: bool,
    pub fluid: FluidStack,
    pub progress: f32,
    pub complete: bool,
}

impl Flow {
    pub fn new(inbound: bool, fluid: FluidStack) -> Self {
        Self {
            inbound,
            fluid,
            progress: 0.0,
            complete: false,
        }
    }
}

/// PipeConnection, before source discovery and the transient FluidNetwork are attached.
#[derive(Clone, Debug, Default)]
pub struct PipeConnection {
    pub side: u8,
    /// Inbound pressure (index 0) and outward pressure (index 1).
    pub pressure: [f32; 2],
    pub flow: Option<Flow>,
}

impl PipeConnection {
    pub fn new(side: u8) -> Self {
        Self {
            side,
            pressure: [0.0, 0.0],
            flow: None,
        }
    }

    pub fn compare_pressure(&self) -> f32 {
        self.pressure[1] - self.pressure[0]
    }

    pub fn has_pressure(&self) -> bool {
        self.pressure[0] != 0.0 || self.pressure[1] != 0.0
    }

    pub fn add_pressure(&mut self, inbound: bool, pressure: f32) {
        let index = usize::from(!inbound);
        self.pressure[index] += pressure;
    }

    /// FluidTransportBehaviour.wipePressure resets the connection's pressures,
    /// but does not discard its existing flow.
    pub fn wipe_pressure(&mut self) {
        self.pressure = [0.0, 0.0];
    }

    pub fn provided_fluid(&self) -> Option<&FluidStack> {
        let flow = self.flow.as_ref()?;
        (flow.inbound && flow.complete).then_some(&flow.fluid)
    }

    pub fn provide_outbound_flow(&self) -> Option<&FluidStack> {
        let flow = self.flow.as_ref()?;
        (!flow.inbound && flow.complete).then_some(&flow.fluid)
    }

    /// PipeConnection.flipFlowsIfPressureReversed: only one-sided pressure can flip
    /// a flow in place. A partial flow loses its progress when flipped.
    pub fn flip_flows_if_pressure_reversed(&mut self) -> bool {
        let comparison = self.compare_pressure();
        let single_pressure =
            comparison != 0.0 && (self.pressure[0] == 0.0 || self.pressure[1] == 0.0);
        let Some(flow) = self.flow.as_mut() else {
            return false;
        };
        if !single_pressure || (comparison < 0.0) == flow.inbound {
            return false;
        }
        flow.inbound = !flow.inbound;
        if !flow.complete {
            self.flow = None;
        }
        true
    }

    /// PipeConnection.manageFlows. `source_fluid` comes from FlowSource.provideFluid;
    /// `internal_fluid` comes from the other connections in the same pipe.
    /// The host-dependent FlowSource and FluidNetwork lifecycle is handled by
    /// the caller once the fluid handler and world interfaces are available.
    pub fn manage_flows(
        &mut self,
        source_fluid: Option<&FluidStack>,
        internal_fluid: Option<&FluidStack>,
    ) -> bool {
        if self.flow.is_none() {
            if !self.has_pressure() {
                return false;
            }
            let prioritize_inbound = self.compare_pressure() < 0.0;
            for inbound in [prioritize_inbound, !prioritize_inbound] {
                if self.pressure[usize::from(!inbound)] == 0.0 {
                    continue;
                }
                let provided = if inbound {
                    source_fluid
                } else {
                    internal_fluid
                };
                if let Some(stack) = provided.filter(|stack| !stack.is_empty()) {
                    self.flow = Some(Flow::new(inbound, stack.clone()));
                    return true;
                }
            }
            return false;
        }

        let inbound = self.flow.as_ref().is_some_and(|flow| flow.inbound);
        let provided = if inbound {
            source_fluid
        } else {
            internal_fluid
        };
        let valid = provided
            .filter(|stack| !stack.is_empty())
            .zip(self.flow.as_ref())
            .is_some_and(|(stack, flow)| stack.same_fluid_same_components(&flow.fluid));
        if !self.has_pressure() || !valid {
            self.flow = None;
            return true;
        }

        if inbound != (self.compare_pressure() < 0.0) {
            let reversed = !inbound;
            let fluid = if reversed {
                source_fluid
            } else {
                internal_fluid
            };
            if let Some(stack) = fluid.filter(|stack| !stack.is_empty()) {
                self.flow = Some(Flow::new(reversed, stack.clone()));
                return true;
            }
        }
        false
    }

    /// PipeConnection.tickFlowProgress uses the pressure in the flow direction,
    /// independently of the network's transfer rate.
    pub fn tick_flow_progress(&mut self) {
        let Some(flow) = self.flow.as_mut() else {
            return;
        };
        if flow.fluid.is_empty() {
            return;
        }
        let pressure = self.pressure[usize::from(!flow.inbound)];
        let flow_speed = 1.0 / 32.0 + (pressure / 128.0).clamp(0.0, 1.0) * 31.0 / 32.0;
        flow.progress = (flow.progress + flow_speed).min(1.0);
        if flow.progress >= 1.0 {
            flow.complete = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Flow, FluidStack, PipeConnection};

    fn water() -> FluidStack {
        FluidStack {
            id: "minecraft:water".to_owned(),
            amount: 1000,
            components: None,
        }
    }

    #[test]
    fn flow_progress_respects_original_tick_boundaries() {
        let mut slow = PipeConnection::new(2);
        slow.flow = Some(Flow::new(true, water()));
        slow.add_pressure(true, 0.0);
        for _ in 0..31 {
            slow.tick_flow_progress();
        }
        assert!(!slow.flow.as_ref().is_some_and(|flow| flow.complete));
        slow.tick_flow_progress();
        assert!(slow.flow.as_ref().is_some_and(|flow| flow.complete));

        let mut fast = PipeConnection::new(2);
        fast.flow = Some(Flow::new(true, water()));
        fast.add_pressure(true, 128.0);
        fast.tick_flow_progress();
        assert!(fast.flow.as_ref().is_some_and(|flow| flow.complete));
        assert!(fast.provided_fluid().is_some());
        assert!(fast.provide_outbound_flow().is_none());
    }

    #[test]
    fn reversing_pressure_discards_partial_but_keeps_complete_flow() {
        let mut partial = PipeConnection::new(2);
        partial.flow = Some(Flow::new(true, water()));
        partial.add_pressure(false, 32.0);
        assert!(partial.flip_flows_if_pressure_reversed());
        assert!(partial.flow.is_none());

        let mut complete = PipeConnection::new(2);
        complete.flow = Some(Flow {
            inbound: true,
            fluid: water(),
            progress: 1.0,
            complete: true,
        });
        complete.add_pressure(false, 32.0);
        assert!(complete.flip_flows_if_pressure_reversed());
        assert!(complete.provided_fluid().is_none());
        assert_eq!(
            complete.provide_outbound_flow().map(|fluid| fluid.amount),
            Some(1000)
        );
        assert!(!complete.flip_flows_if_pressure_reversed());
    }

    #[test]
    fn opposing_pressures_do_not_flip_a_complete_flow() {
        let mut connection = PipeConnection::new(2);
        connection.flow = Some(Flow {
            inbound: true,
            fluid: water(),
            progress: 1.0,
            complete: true,
        });
        connection.add_pressure(true, 16.0);
        connection.add_pressure(false, 32.0);
        assert!(!connection.flip_flows_if_pressure_reversed());
        assert!(connection.provided_fluid().is_some());
    }

    #[test]
    fn source_fluid_identity_includes_components_but_not_amount() {
        let a = water();
        let mut b = water();
        b.amount = 1;
        assert!(a.same_fluid_same_components(&b));
        b.components = Some(fastnbt::Value::Int(42));
        assert!(!a.same_fluid_same_components(&b));

        let mut pipe = PipeConnection::new(2);
        pipe.add_pressure(true, 64.0);
        assert!(pipe.manage_flows(Some(&a), None));
        assert_eq!(pipe.flow.as_ref().map(|flow| flow.inbound), Some(true));
        assert!(pipe.manage_flows(Some(&b), None));
        assert!(pipe.flow.is_none());
    }

    #[test]
    fn pressure_wipe_keeps_flow_until_manage_flows_clears_it() {
        let mut pipe = PipeConnection::new(2);
        pipe.add_pressure(true, 16.0);
        assert!(pipe.manage_flows(Some(&water()), None));
        pipe.wipe_pressure();
        assert!(pipe.flow.is_some());
        assert!(pipe.manage_flows(Some(&water()), None));
        assert!(pipe.flow.is_none());
    }
}

#[cfg(test)]
mod fluid_identity_regressions {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn absent_and_explicit_empty_component_patch_have_same_identity() {
        let absent = FluidStack {
            id: "minecraft:water".into(),
            amount: 1000,
            components: None,
        };
        let empty = FluidStack {
            id: "minecraft:water".into(),
            amount: 250,
            components: Some(Value::Compound(HashMap::new())),
        };
        assert!(absent.same_fluid_same_components(&empty));
        assert!(empty.same_fluid_same_components(&absent));
        let mut connection = PipeConnection::new(0);
        connection.add_pressure(true, 32.0);
        assert!(connection.manage_flows(Some(&absent), None));
        assert!(connection.flow.is_some());
        assert!(!connection.manage_flows(Some(&empty), None));
        assert!(connection.flow.is_some());
    }

    #[test]
    fn positive_amount_empty_fluid_is_still_empty() {
        assert!(
            FluidStack {
                id: "minecraft:empty".into(),
                amount: 1000,
                components: None
            }
            .is_empty()
        );
    }
}
