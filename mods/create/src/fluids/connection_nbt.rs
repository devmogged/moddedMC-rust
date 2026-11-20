//! Create 6.0.10 PipeConnection.serializeNBT/deserializeNBT.
//! Optional OpenEnd data is kept opaque until the host world adapter is available.

use std::collections::HashMap;

use fastnbt::Value;

use super::pipe_connection::{Flow, FluidStack, PipeConnection};

const SIDE_NAMES: [&str; 6] = ["down", "up", "north", "south", "west", "east"];

#[derive(Debug, Clone)]
pub struct SavedConnection {
    pub pipe: PipeConnection,
    pub open_end: Option<Value>,
    /// LerpedFloat NBT including its chase speed and target.
    pub progress: Option<Value>,
}

fn compound(value: &Value) -> Option<&HashMap<String, Value>> {
    match value {
        Value::Compound(tag) => Some(tag),
        _ => None,
    }
}

fn float(value: Option<&Value>) -> f32 {
    match value {
        Some(Value::Float(v)) => *v,
        _ => 0.0,
    }
}

fn fluid_stack(tag: Option<&Value>) -> FluidStack {
    let data = tag.and_then(compound);
    FluidStack {
        id: data
            .and_then(|data| data.get("id"))
            .and_then(|id| match id {
                Value::String(id) => Some(id.clone()),
                _ => None,
            })
            .unwrap_or_default(),
        amount: data
            .and_then(|data| data.get("amount"))
            .and_then(|amount| match amount {
                Value::Int(amount) if *amount > 0 => Some(*amount as u32),
                _ => None,
            })
            .unwrap_or(0),
        components: data.and_then(|data| data.get("components")).cloned(),
    }
}

fn saved_fluid(fluid: &FluidStack) -> Value {
    let mut compound = HashMap::new();
    if !fluid.is_empty()
        && let Ok(amount) = i32::try_from(fluid.amount)
    {
        compound.insert("id".into(), Value::String(fluid.id.clone()));
        compound.insert("amount".into(), Value::Int(amount));
        if let Some(components) = &fluid.components {
            compound.insert("components".into(), components.clone());
        }
    }
    Value::Compound(compound)
}

fn saved_progress(value: f32, previous: Option<&Value>) -> Value {
    let mut data = previous.and_then(compound).cloned().unwrap_or_default();
    data.entry("Speed".into()).or_insert(Value::Float(0.0));
    data.entry("Target".into()).or_insert(Value::Float(0.0));
    data.insert("Value".into(), Value::Float(value));
    Value::Compound(data)
}

impl SavedConnection {
    pub fn new(pipe: PipeConnection) -> Self {
        Self {
            pipe,
            open_end: None,
            progress: None,
        }
    }

    /// Write this side's nested CompoundTag into its parent PipeConnection map.
    pub fn write(&self, parent: &mut HashMap<String, Value>) {
        let Some(side) = SIDE_NAMES.get(usize::from(self.pipe.side)) else {
            return;
        };
        let mut connection = HashMap::new();
        if self.pipe.has_pressure() {
            connection.insert(
                "Pressure".into(),
                Value::List(self.pipe.pressure.into_iter().map(Value::Float).collect()),
            );
        }
        if let Some(open_end) = &self.open_end {
            connection.insert("OpenEnd".into(), open_end.clone());
        }
        if let Some(flow) = &self.pipe.flow {
            let mut data = HashMap::from([
                ("Fluid".into(), saved_fluid(&flow.fluid)),
                ("In".into(), Value::Byte(i8::from(flow.inbound))),
            ]);
            if !flow.complete {
                data.insert(
                    "Progress".into(),
                    saved_progress(flow.progress, self.progress.as_ref()),
                );
            }
            connection.insert("Flow".into(), Value::Compound(data));
        }
        parent.insert((*side).into(), Value::Compound(connection));
    }

    /// Load exactly the side named in PipeConnection.side. Missing or wrong
    /// types have the same empty/default behavior as Java CompoundTag reads.
    pub fn read(side: u8, parent: &HashMap<String, Value>) -> Self {
        let Some(data) = SIDE_NAMES
            .get(usize::from(side))
            .and_then(|name| parent.get(*name))
            .and_then(compound)
        else {
            return Self::new(PipeConnection::new(side));
        };
        let mut pipe = PipeConnection::new(side);
        if let Some(Value::List(pressure)) = data.get("Pressure") {
            pipe.pressure[0] = float(pressure.first());
            pipe.pressure[1] = float(pressure.get(1));
        }
        let progress = data
            .get("Flow")
            .and_then(compound)
            .and_then(|flow| flow.get("Progress"))
            .cloned();
        if let Some(flow) = data.get("Flow").and_then(compound) {
            let complete = !flow.contains_key("Progress");
            let inbound = matches!(flow.get("In"), Some(Value::Byte(v)) if *v != 0);
            pipe.flow = Some(Flow {
                inbound,
                fluid: fluid_stack(flow.get("Fluid")),
                progress: if complete {
                    1.0
                } else {
                    float(
                        progress
                            .as_ref()
                            .and_then(compound)
                            .and_then(|p| p.get("Value")),
                    )
                },
                complete,
            });
        }
        Self {
            pipe,
            open_end: data.get("OpenEnd").cloned(),
            progress,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use fastnbt::Value;

    use super::SavedConnection;
    use crate::fluids::pipe_connection::{Flow, FluidStack, PipeConnection};

    fn stack() -> FluidStack {
        FluidStack {
            id: "minecraft:water".into(),
            amount: 1000,
            components: Some(Value::Compound(HashMap::from([(
                "example:color".into(),
                Value::Int(7),
            )]))),
        }
    }

    #[test]
    fn partial_flow_reads_java_pressure_and_chase_fields() {
        let input = HashMap::from([(
            "north".into(),
            Value::Compound(HashMap::from([
                (
                    "Pressure".into(),
                    Value::List(vec![Value::Float(64.0), Value::Float(0.0)]),
                ),
                (
                    "Flow".into(),
                    Value::Compound(HashMap::from([
                        (
                            "Fluid".into(),
                            Value::Compound(HashMap::from([
                                ("id".into(), Value::String("minecraft:water".into())),
                                ("amount".into(), Value::Int(1000)),
                                ("components".into(), stack().components.unwrap()),
                            ])),
                        ),
                        ("In".into(), Value::Byte(1)),
                        (
                            "Progress".into(),
                            Value::Compound(HashMap::from([
                                ("Value".into(), Value::Float(0.25)),
                                ("Speed".into(), Value::Float(0.2)),
                                ("Target".into(), Value::Float(0.5)),
                            ])),
                        ),
                    ])),
                ),
            ])),
        )]);
        let mut loaded = SavedConnection::read(2, &input);
        assert_eq!(loaded.pipe.pressure, [64.0, 0.0]);
        assert_eq!(
            loaded.pipe.flow.as_ref().map(|flow| flow.progress),
            Some(0.25)
        );
        assert_eq!(
            loaded.pipe.flow.as_ref().map(|flow| flow.complete),
            Some(false)
        );
        assert_eq!(
            loaded.pipe.flow.as_ref().map(|flow| flow.fluid.amount),
            Some(1000)
        );
        loaded.pipe.tick_flow_progress();
        let mut output = HashMap::new();
        loaded.write(&mut output);
        let tag = output.get("north").and_then(super::compound).unwrap();
        let flow = tag.get("Flow").and_then(super::compound).unwrap();
        let chase = flow.get("Progress").and_then(super::compound).unwrap();
        assert_eq!(chase.get("Speed"), Some(&Value::Float(0.2)));
        assert_eq!(chase.get("Target"), Some(&Value::Float(0.5)));
        assert_eq!(chase.get("Value"), Some(&Value::Float(0.765625)));
        assert_eq!(
            flow.get("Fluid")
                .and_then(super::compound)
                .and_then(|s| s.get("components")),
            Some(&Value::Compound(HashMap::from([(
                "example:color".into(),
                Value::Int(7)
            )])))
        );
    }

    #[test]
    fn complete_flow_omits_progress_and_reloads_with_full_progress() {
        let mut pipe = PipeConnection::new(5);
        pipe.flow = Some(Flow {
            inbound: false,
            fluid: stack(),
            progress: 1.0,
            complete: true,
        });
        let saved = SavedConnection::new(pipe);
        let mut parent = HashMap::new();
        saved.write(&mut parent);
        let flow_tag = parent
            .get("east")
            .and_then(super::compound)
            .and_then(|tag| tag.get("Flow"))
            .and_then(super::compound)
            .unwrap();
        assert!(!flow_tag.contains_key("Progress"));
        let loaded = SavedConnection::read(5, &parent);
        assert_eq!(
            loaded.pipe.flow.as_ref().map(|flow| flow.progress),
            Some(1.0)
        );
        assert_eq!(
            loaded.pipe.flow.as_ref().map(|flow| flow.inbound),
            Some(false)
        );
    }

    #[test]
    fn empty_connection_emits_only_its_side_and_absent_fields_reset_on_load() {
        let saved = SavedConnection::new(PipeConnection::new(0));
        let mut parent = HashMap::new();
        saved.write(&mut parent);
        assert_eq!(parent.get("down"), Some(&Value::Compound(HashMap::new())));
        let loaded = SavedConnection::read(0, &parent);
        assert_eq!(loaded.pipe.pressure, [0.0, 0.0]);
        assert!(loaded.pipe.flow.is_none());
    }

    #[test]
    fn open_end_tag_survives_nbt_binary_round_trip() {
        let mut saved = SavedConnection::new(PipeConnection::new(3));
        saved.open_end = Some(Value::Compound(HashMap::from([(
            "Pulling".into(),
            Value::Byte(1),
        )])));
        let mut root = HashMap::new();
        saved.write(&mut root);
        let bytes = fastnbt::to_bytes(&root).unwrap();
        let parsed: HashMap<String, Value> = fastnbt::from_bytes(&bytes).unwrap();
        let restored = SavedConnection::read(3, &parsed);
        assert_eq!(restored.open_end, saved.open_end);
    }
}
