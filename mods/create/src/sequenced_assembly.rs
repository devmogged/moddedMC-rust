//! Ordered press and spout steps from Create's sequenced assembly recipes.

use super::{
    Json, ResultStack, is_registered_output, item_id, next_int, read_fluid_ingredient,
    read_ingredient,
};
use fastnbt::Value;
use moddedmc_api::tags::Tags;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

const COMPONENT: &str = "minecraft:custom_data";
const PROGRESS: &str = "create:sequenced_assembly";

#[derive(Deserialize)]
struct Definition {
    ingredient: Json,
    transitional_item: ResultStack,
    #[serde(default = "super::one")]
    loops: i32,
    sequence: Vec<StepDefinition>,
    results: Vec<ResultStack>,
}

#[derive(Deserialize)]
struct StepDefinition {
    #[serde(rename = "type")]
    kind: String,
    ingredients: Vec<Json>,
}

pub(super) enum Step {
    Press,
    Fill {
        fluids: HashSet<String>,
        amount: u32,
    },
}

pub(super) struct Assembly {
    id: String,
    inputs: HashSet<String>,
    intermediate: Value,
    steps: Vec<Step>,
    length: i32,
    results: Vec<(Value, f32)>,
    total_weight: f32,
}

impl Assembly {
    pub(super) fn parse(
        path: &str,
        bytes: &[u8],
        tags: &mut impl FnMut(&str) -> Vec<String>,
        tag_api: Tags,
    ) -> Option<Self> {
        let definition: Definition = serde_json::from_slice(bytes).ok()?;
        let (namespace, path) = path.split_once(":recipe/")?;
        let path = path.strip_suffix(".json")?;
        let mut inputs = HashSet::new();
        read_ingredient(&definition.ingredient, tags, &mut inputs);
        let steps: Vec<_> = definition
            .sequence
            .into_iter()
            .map(
                |step| match (step.kind.as_str(), step.ingredients.as_slice()) {
                    ("create:pressing", [_]) => Some(Step::Press),
                    ("create:filling", [_, fluid]) => {
                        let (fluids, amount) = read_fluid_ingredient(fluid, tag_api)?;
                        Some(Step::Fill { fluids, amount })
                    }
                    _ => None,
                },
            )
            .collect::<Option<_>>()?;
        let length = i32::try_from(steps.len())
            .ok()?
            .checked_mul(definition.loops)?;
        let intermediate = definition.transitional_item.into_nbt()?;
        if inputs.is_empty() || length <= 0 || !is_registered_output(&intermediate) {
            return None;
        }
        let results: Vec<_> = definition
            .results
            .into_iter()
            .filter(|item| item.chance > 0.0)
            .map(|item| {
                let weight = item.chance;
                let stack = item.into_nbt().filter(is_registered_output)?;
                Some((stack, weight))
            })
            .collect::<Option<_>>()?;
        let total_weight: f32 = results.iter().map(|(_, weight)| weight).sum();
        if !total_weight.is_finite() || total_weight <= 0.0 {
            return None;
        }
        Some(Self {
            id: format!("{namespace}:{path}"),
            inputs,
            intermediate,
            steps,
            length,
            results,
            total_weight,
        })
    }

    pub(super) fn next(&self, item: &Value) -> Option<(&Step, Advance<'_>)> {
        let id = item_id(item)?;
        let step = match field(item, "components")
            .and_then(|item| field(item, COMPONENT))
            .and_then(|item| field(item, PROGRESS))
        {
            Some(progress) => {
                if !matches!(field(progress, "id"), Some(Value::String(id)) if id == &self.id)
                    || item_id(&self.intermediate) != Some(id)
                {
                    return None;
                }
                let Value::Int(step) = field(progress, "step")? else {
                    return None;
                };
                *step
            }
            None if self.inputs.contains(id) => 0,
            None => return None,
        };
        if !(0..self.length).contains(&step) {
            return None;
        }
        let stage = &self.steps[step as usize % self.steps.len()];
        Some((
            stage,
            Advance {
                assembly: self,
                step: step + 1,
            },
        ))
    }
}

fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    let Value::Compound(fields) = value else {
        return None;
    };
    fields.get(name)
}

pub struct Advance<'a> {
    assembly: &'a Assembly,
    step: i32,
}

impl Advance<'_> {
    pub(super) fn finish(self) -> Value {
        let Self { assembly, step } = self;
        if step == assembly.length {
            let mut number = next_int(1 << 24) as f32 / (1 << 24) as f32 * assembly.total_weight;
            for (result, weight) in &assembly.results {
                number -= weight;
                if number < 0.0 {
                    return result.clone();
                }
            }
            return assembly.results.last().map_or_else(
                || Value::Compound(HashMap::new()),
                |(result, _)| result.clone(),
            );
        }
        let mut item = assembly.intermediate.clone();
        if let Value::Compound(fields) = &mut item {
            fields.insert("count".to_owned(), Value::Int(1));
            let progress = Value::Compound(HashMap::from([
                ("id".to_owned(), Value::String(assembly.id.clone())),
                ("step".to_owned(), Value::Int(step)),
                (
                    "progress".to_owned(),
                    Value::Float(step as f32 / assembly.length as f32),
                ),
            ]));
            let custom = Value::Compound(HashMap::from([(PROGRESS.to_owned(), progress)]));
            fields.insert(
                "components".to_owned(),
                Value::Compound(HashMap::from([(COMPONENT.to_owned(), custom)])),
            );
        }
        item
    }
}
