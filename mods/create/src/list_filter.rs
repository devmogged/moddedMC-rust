//! The List Filter's ghost samples, allow/deny modes, and component matching.

use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    entities::ENTITY_SNEAKING,
    interaction::{ItemUse, ItemUsePhase, ItemUseResult},
    items::Item,
};

use crate::{
    ITEMS,
    belt_inventory::with_count,
    filtering::{Compound, matches_sample, same_item, settings, settings_mut},
    processing_recipes::item_id,
};

pub(crate) const ID: &str = "create:filter";
const SETTINGS: &str = "create:list_filter";
const ITEMS_TAG: &str = "Items";
const DENY: &str = "Deny";
const RESPECT_COMPONENTS: &str = "RespectComponents";
const MAX_ENTRIES: usize = 18;

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

fn flag(settings: &Compound, key: &str) -> bool {
    matches!(settings.get(key), Some(Value::Byte(value)) if *value != 0)
}

fn entries(settings: &Compound) -> &[Value] {
    match settings.get(ITEMS_TAG) {
        Some(Value::List(entries)) => &entries[..entries.len().min(MAX_ENTRIES)],
        _ => &[],
    }
}

pub(crate) fn matches(sample: &Value, stack: &Value, remaining: u8) -> Option<bool> {
    matches_entries(sample, |entry, respect| {
        matches_sample(entry, stack, respect, remaining)
    })
}

pub(crate) fn matches_entries(
    sample: &Value,
    mut matches: impl FnMut(&Value, bool) -> bool,
) -> Option<bool> {
    let settings = settings(sample, SETTINGS)?;
    let respect = flag(settings, RESPECT_COMPONENTS);
    let listed = entries(settings)
        .iter()
        .any(|entry| matches(entry, respect));
    Some(listed != flag(settings, DENY))
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
        let message = settings(&item, SETTINGS)
            .map_or_else(|| "Allow list 0/18 - Match item type".into(), describe);
        context.action_bar(&message);
        return ItemUseResult::Handled;
    }
    let Some(settings) = settings_mut(&mut item, SETTINGS) else {
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
