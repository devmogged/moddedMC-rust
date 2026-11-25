//! The List Filter's ghost samples, allow/deny modes, and component matching.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    entities::ENTITY_SNEAKING,
    interaction::{ItemUse, ItemUsePhase, ItemUseResult},
    items::Item,
};

use crate::{ITEMS, belt_inventory::with_count, processing_recipes::item_id};

pub(crate) const ID: &str = "create:filter";
const COMPONENTS: &str = "components";
const CUSTOM_DATA: &str = "minecraft:custom_data";
const SETTINGS: &str = "create:list_filter";
const ITEMS_TAG: &str = "Items";
const DENY: &str = "Deny";
const RESPECT_COMPONENTS: &str = "RespectComponents";
const MAX_ENTRIES: usize = 18;
const MAX_DEPTH: u8 = 16;

type Compound = HashMap<String, Value>;

pub(crate) fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: ID,
                display_name: "List Filter",
                carrier: "minecraft:paper",
                model: "create:item/filter",
            },
        )
}

fn compound(value: &Value) -> Option<&Compound> {
    if let Value::Compound(fields) = value {
        Some(fields)
    } else {
        None
    }
}

fn named_compound<'a>(fields: &'a Compound, key: &str) -> Option<&'a Compound> {
    compound(fields.get(key)?)
}

fn components(item: &Value) -> Option<&Compound> {
    named_compound(compound(item)?, COMPONENTS).filter(|parts| !parts.is_empty())
}

fn settings(item: &Value) -> Option<&Compound> {
    named_compound(named_compound(components(item)?, CUSTOM_DATA)?, SETTINGS)
}

fn flag(settings: &Compound, key: &str) -> bool {
    matches!(settings.get(key), Some(Value::Byte(value)) if *value != 0)
}

fn entries(settings: &Compound) -> &[Value] {
    match settings.get(ITEMS_TAG) {
        Some(Value::List(entries)) => &entries[..entries.len().min(MAX_ENTRIES)],
        _ => &[],
    }
}

fn same_item(sample: &Value, stack: &Value, exact: bool) -> bool {
    item_id(sample).is_some_and(|id| item_id(stack) == Some(id))
        && (!exact || components(sample) == components(stack))
}

pub(crate) fn matches(sample: &Value, stack: &Value) -> bool {
    test(sample, stack, false, MAX_DEPTH)
}

fn test(sample: &Value, stack: &Value, exact: bool, depth: u8) -> bool {
    if depth == 0 {
        return false;
    }
    if item_id(sample) == Some(ID)
        && let Some(settings) = settings(sample)
    {
        let respect = flag(settings, RESPECT_COMPONENTS);
        let listed = entries(settings)
            .iter()
            .any(|entry| test(entry, stack, respect, depth - 1));
        return listed != flag(settings, DENY);
    }
    same_item(sample, stack, exact)
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

fn settings_mut(item: &mut Value) -> Option<&mut Compound> {
    let Value::Compound(fields) = item else {
        return None;
    };
    ensure_compound(
        ensure_compound(ensure_compound(fields, COMPONENTS)?, CUSTOM_DATA)?,
        SETTINGS,
    )
}

fn describe(settings: &Compound) -> String {
    let mode = if flag(settings, DENY) {
        "Deny"
    } else {
        "Allow"
    };
    let matching = if flag(settings, RESPECT_COMPONENTS) {
        "Match components"
    } else {
        "Match item type"
    };
    let entries = entries(settings);
    let mut message = format!("{mode} list {}/{} - {matching}", entries.len(), MAX_ENTRIES);
    let mut ids = entries.iter().filter_map(item_id);
    for id in ids.by_ref().take(4) {
        let name = id.split_once(':').map_or(id, |(_, name)| name);
        message.push_str(" - ");
        message.push_str(&name.replace('_', " "));
    }
    if ids.next().is_some() {
        message.push_str(" - ...");
    }
    message
}

pub(crate) fn use_item(context: &mut ItemUse<'_>) -> ItemUseResult {
    if !context.main_hand || context.phase != ItemUsePhase::Start {
        return ItemUseResult::Pass;
    }
    let Some(mut item) = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
    else {
        return ItemUseResult::Handled;
    };
    let sneak = context
        .player
        .is_some_and(|player| player.flags() & ENTITY_SNEAKING != 0);
    let other = context
        .other_hand
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
        .filter(|sample| item_id(sample).is_some());
    if !sneak && other.is_none() {
        let message =
            settings(&item).map_or_else(|| "Allow list 0/18 - Match item type".into(), describe);
        context.action_bar(&message);
        return ItemUseResult::Handled;
    }
    let Some(settings) = settings_mut(&mut item) else {
        return ItemUseResult::Handled;
    };
    if sneak {
        let key = if other.is_some() {
            RESPECT_COMPONENTS
        } else {
            DENY
        };
        let value = !flag(settings, key);
        settings.insert(key.into(), Value::Byte(i8::from(value)));
    } else if let Some(sample) = other {
        let mut list = entries(settings).to_vec();
        if let Some(index) = list
            .iter()
            .position(|entry| same_item(entry, &sample, true))
        {
            list.remove(index);
        } else if list.len() < MAX_ENTRIES {
            list.push(with_count(&sample, 1));
        } else {
            context.action_bar("List Filter is full");
            return ItemUseResult::Handled;
        }
        settings.insert(ITEMS_TAG.into(), Value::List(list));
    }
    let message = describe(settings);
    if fastnbt::to_bytes(&item).is_ok_and(|bytes| context.stack.write(&bytes)) {
        context.action_bar(&message);
    } else {
        context.action_bar("List Filter data is too large");
    }
    ItemUseResult::Handled
}
