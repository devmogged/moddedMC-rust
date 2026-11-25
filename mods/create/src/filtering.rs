//! Create's filter settings and the stock-client value plate interaction.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{Pos, content::World, interaction::UseBlock, stacks::ItemDisplay};
use serde::{Deserialize, Serialize};

use crate::{
    DISPLAYS, STACKS, WRENCH, attribute_filter,
    belt_inventory::{TransportedItemStack, remove_display, render_stack, with_count},
    list_filter,
    processing_recipes::item_id,
};

const MAX_FILTER_AMOUNT: i32 = 64;
const MAX_NESTED_FILTERS: u8 = 16;
const COMPONENTS: &str = "components";
const CUSTOM_DATA: &str = "minecraft:custom_data";

pub(crate) type Compound = HashMap<String, Value>;

pub(crate) fn compound(value: &Value) -> Option<&Compound> {
    if let Value::Compound(fields) = value {
        Some(fields)
    } else {
        None
    }
}

fn components(item: &Value) -> Option<&Compound> {
    compound(compound(item)?.get(COMPONENTS)?).filter(|parts| !parts.is_empty())
}

pub(crate) fn settings<'a>(item: &'a Value, key: &str) -> Option<&'a Compound> {
    let data = compound(components(item)?.get(CUSTOM_DATA)?)?;
    compound(data.get(key)?)
}

fn ensure_compound<'a>(fields: &'a mut Compound, key: &str) -> Option<&'a mut Compound> {
    if let Value::Compound(inner) = fields
        .entry(key.into())
        .or_insert_with(|| Value::Compound(HashMap::new()))
    {
        Some(inner)
    } else {
        None
    }
}

pub(crate) fn settings_mut<'a>(item: &'a mut Value, key: &str) -> Option<&'a mut Compound> {
    let Value::Compound(fields) = item else {
        return None;
    };
    ensure_compound(
        ensure_compound(ensure_compound(fields, COMPONENTS)?, CUSTOM_DATA)?,
        key,
    )
}

pub(crate) fn same_item(sample: &Value, stack: &Value, exact: bool) -> bool {
    item_id(sample).is_some_and(|id| item_id(stack) == Some(id))
        && (!exact || components(sample) == components(stack))
}

fn is_filter_item(id: &str) -> bool {
    matches!(id, list_filter::ID | attribute_filter::ID)
}

pub(crate) fn matches_sample(sample: &Value, stack: &Value, exact: bool, depth: u8) -> bool {
    if depth == 0 {
        return false;
    }
    let matched = match item_id(sample) {
        Some(list_filter::ID) => list_filter::matches(sample, stack, depth - 1),
        Some(attribute_filter::ID) => attribute_filter::matches(sample, stack),
        _ => None,
    };
    matched.unwrap_or_else(|| same_item(sample, stack, exact))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub(crate) struct Filter {
    #[serde(rename = "Filter", skip_serializing_if = "Option::is_none")]
    pub(crate) stack: Option<Value>,
    #[serde(rename = "FilterFace", skip_serializing_if = "Option::is_none")]
    face: Option<u8>,
    filter_amount: i32,
    up_to: i8,
}

impl Default for Filter {
    fn default() -> Self {
        Self {
            stack: None,
            face: None,
            filter_amount: MAX_FILTER_AMOUNT,
            up_to: 1,
        }
    }
}

pub(crate) struct Extraction<'a> {
    pub(crate) amount: i32,
    pub(crate) exact: bool,
    pub(crate) filter: Option<&'a Filter>,
}

impl Extraction<'_> {
    pub(crate) fn up_to(amount: i32) -> Self {
        Self {
            amount,
            exact: false,
            filter: None,
        }
    }
}

impl Filter {
    pub(crate) fn extraction(&self, count_visible: bool) -> Extraction<'_> {
        Extraction {
            amount: if count_visible {
                self.amount()
            } else {
                MAX_FILTER_AMOUNT
            },
            exact: count_visible && self.exact(),
            filter: self.stack.is_some().then_some(self),
        }
    }

    pub(crate) fn side(&self, default: u8) -> u8 {
        self.face
            .filter(|face| (2..=5).contains(face))
            .unwrap_or(default)
    }

    pub(crate) fn amount(&self) -> i32 {
        if self.filter_amount <= 0 {
            MAX_FILTER_AMOUNT
        } else {
            self.filter_amount.min(MAX_FILTER_AMOUNT)
        }
    }

    pub(crate) fn exact(&self) -> bool {
        self.filter_amount > 0 && self.up_to == 0
    }

    pub(crate) fn matches(&self, stack: &Value) -> bool {
        self.stack.as_ref().is_none_or(|sample| {
            item_id(sample).is_none() || matches_sample(sample, stack, false, MAX_NESTED_FILTERS)
        })
    }

    fn installed_item(&self) -> Option<Vec<u8>> {
        let stack = self
            .stack
            .as_ref()
            .filter(|stack| item_id(stack).is_some_and(is_filter_item))?;
        fastnbt::to_bytes(stack).ok()
    }

    pub(crate) fn drop_item(&self, world: World<'_>, pos: Pos) {
        if let (Some(&stacks), Some(bytes)) = (STACKS.get(), self.installed_item()) {
            let center = [pos.x as f64 + 0.5, pos.y as f64 + 0.5, pos.z as f64 + 0.5];
            stacks.spawn(world, center, [0.0; 3], &bytes);
        }
    }

    fn max_amount(&self) -> i32 {
        self.stack
            .as_ref()
            .and_then(|item| fastnbt::to_bytes(item).ok())
            .and_then(|bytes| STACKS.get()?.info(&bytes))
            .map_or(MAX_FILTER_AMOUNT, |info| {
                info.max_count.min(MAX_FILTER_AMOUNT as u32) as i32
            })
            .max(1)
    }

    pub(crate) fn configure(
        &mut self,
        context: &mut UseBlock<'_>,
        item: Option<&Value>,
        side: f32,
        count_visible: bool,
    ) -> bool {
        if !context.main_hand
            || item.and_then(item_id) == Some(WRENCH)
            || !context.player.is_some_and(|player| player.may_build())
        {
            return false;
        }
        if (2..=5).contains(&context.face) {
            self.face = Some(context.face);
        }
        if item.is_some() || context.sneaking || !count_visible {
            if let Some(bytes) = self.installed_item()
                && !context.give(&bytes)
            {
                return true;
            }
            self.stack = item.map(|stack| with_count(stack, 1));
            if let Some(item) = item {
                if !context.creative && item_id(item).is_some_and(is_filter_item) {
                    context.stack.shrink(1);
                }
                if self.exact() {
                    self.filter_amount = self.amount().min(self.max_amount());
                }
            }
        } else if side.abs() < 0.05 {
            self.up_to = i8::from(self.up_to == 0);
            self.filter_amount = self.amount().min(self.max_amount());
        } else {
            let step = if side.abs() >= 0.13 { 16 } else { 1 };
            let delta = if side > 0.0 { step } else { -step };
            self.filter_amount = (self.amount() + delta).clamp(1, self.max_amount());
        }
        let filter = self.stack.as_ref().and_then(item_id).unwrap_or("any item");
        let mode = if self.exact() { "Exactly" } else { "Up to" };
        if count_visible {
            context.action_bar(&format!("{filter} - {mode} {}", self.amount()));
        } else {
            context.action_bar(&format!("Filter: {filter}"));
        }
        true
    }
}

pub(crate) const fn preview_angle(side: u8) -> i32 {
    match side {
        2 => 180,
        4 => -90,
        5 => 90,
        _ => 0,
    }
}

pub(crate) fn slot_column(face: u8, hit: [f32; 3]) -> Option<f32> {
    let lateral = match face {
        2 => 0.5 - hit[0],
        3 => hit[0] - 0.5,
        4 => hit[2] - 0.5,
        5 => 0.5 - hit[2],
        _ => return None,
    };
    (lateral.abs() <= 0.19).then_some(lateral)
}

pub(crate) fn render(
    world: World<'_>,
    sample: Option<&Value>,
    visible: &mut Option<TransportedItemStack>,
    shown: ItemDisplay,
) {
    let Some(&displays) = DISPLAYS.get() else {
        return;
    };
    if sample != visible.as_ref().map(|shown| &shown.item) {
        if let Some(display) = visible.take() {
            remove_display(world, &display);
        }
        *visible = sample.cloned().map(TransportedItemStack::new);
    }
    if let Some(item) = visible {
        render_stack(world, displays, item, shown);
    }
}
