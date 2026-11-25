//! Attribute Filter rules selected from a sample's tags, source, and properties.

use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    entities::ENTITY_SNEAKING,
    interaction::{ItemUse, ItemUsePhase, ItemUseResult},
    items::Item,
};

use crate::{
    ITEMS, TAGS,
    filtering::{Compound, compound, settings, settings_mut},
    processing_recipes::item_id,
};

mod properties;

pub(crate) const ID: &str = "create:attribute_filter";
const MODE: &str = "Mode";
const RULES: &str = "Rules";
const ATTRIBUTE: &str = "Attribute";
const INVERTED: &str = "Inverted";
const SELECTION: &str = "Selection";

#[derive(Clone, Copy)]
enum Mode {
    Any,
    All,
    Deny,
}

impl Mode {
    fn from_settings(settings: &Compound) -> Self {
        match settings.get(MODE) {
            Some(Value::Byte(1)) => Self::All,
            Some(Value::Byte(2)) => Self::Deny,
            _ => Self::Any,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Any => "Allow any",
            Self::All => "Allow all",
            Self::Deny => "Deny any",
        }
    }

    fn next(self) -> i8 {
        match self {
            Self::Any => 1,
            Self::All => 2,
            Self::Deny => 0,
        }
    }
}

pub(crate) fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: ID,
                display_name: "Attribute Filter",
                carrier: "minecraft:paper",
                model: "create:item/attribute_filter",
            },
        )
}

fn rules(settings: &Compound) -> &[Value] {
    if let Some(Value::List(rules)) = settings.get(RULES) {
        rules
    } else {
        &[]
    }
}

fn rule(value: &Value) -> Option<(&str, bool)> {
    let fields = compound(value)?;
    let Value::String(attribute) = fields.get(ATTRIBUTE)? else {
        return None;
    };
    Some((
        attribute,
        matches!(fields.get(INVERTED), Some(Value::Byte(1))),
    ))
}

fn applies(attribute: &str, id: &str, stack: &properties::Stack<'_>) -> bool {
    if let Some(tag) = attribute.strip_prefix('#') {
        TAGS.get()
            .is_some_and(|tags| tags.contains("item", tag, id))
    } else if let Some(source) = attribute.strip_prefix('@') {
        id.split_once(':')
            .is_some_and(|(namespace, _)| namespace == source)
    } else {
        stack.applies(attribute)
    }
}

pub(crate) fn matches(filter: &Value, stack: &Value) -> Option<bool> {
    let settings = settings(filter, ID)?;
    let id = item_id(stack)?;
    let mut rules = rules(settings).iter().filter_map(rule).peekable();
    rules.peek()?;
    let stack = properties::Stack::new(stack);
    let applies = |(attribute, inverted)| applies(attribute, id, &stack) != inverted;
    Some(match Mode::from_settings(settings) {
        Mode::Any => rules.any(applies),
        Mode::All => rules.all(applies),
        Mode::Deny => !rules.any(applies),
    })
}

fn options(sample: &Value, id: &str) -> Vec<String> {
    let mut options: Vec<_> = TAGS
        .get()
        .and_then(|tags| tags.memberships("item", id))
        .into_iter()
        .flatten()
        .map(|tag| format!("#{tag}"))
        .collect();
    if let Some((namespace, _)) = id.split_once(':') {
        options.push(format!("@{namespace}"));
    }
    properties::Stack::new(sample).add_selections(&mut options);
    options
}

fn describe(settings: &Compound) -> String {
    let mode = Mode::from_settings(settings).label();
    let rules = rules(settings);
    let mut message = format!("{mode} - {} rules", rules.len());
    for (attribute, inverted) in rules.iter().filter_map(rule).take(4) {
        message.push_str(if inverted { " - not " } else { " - " });
        message.push_str(attribute);
    }
    if rules.len() > 4 {
        message.push_str(" - ...");
    }
    message
}

fn edit(settings: &mut Compound, selection: &str) -> &'static str {
    let mut rules = rules(settings).to_vec();
    let mut entry = Compound::from([(ATTRIBUTE.into(), Value::String(selection.into()))]);
    let status = match rules
        .iter()
        .position(|value| rule(value).is_some_and(|(attribute, _)| attribute == selection))
    {
        Some(index) if rule(&rules[index]).is_some_and(|(_, inverted)| inverted) => {
            rules.remove(index);
            "Removed"
        }
        Some(index) => {
            entry.insert(INVERTED.into(), Value::Byte(1));
            rules[index] = Value::Compound(entry);
            "Inverted"
        }
        None => {
            rules.push(Value::Compound(entry));
            "Added"
        }
    };
    settings.insert(RULES.into(), Value::List(rules));
    status
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
    let sample = context
        .other_hand
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok());
    let sample = sample
        .as_ref()
        .and_then(|item| Some((item, item_id(item)?)));
    if !sneak && sample.is_none() {
        let message = settings(&item, ID).map_or_else(|| "Allow any - 0 rules".into(), describe);
        context.action_bar(&message);
        return ItemUseResult::Handled;
    }
    let Some(settings) = settings_mut(&mut item, ID) else {
        return ItemUseResult::Handled;
    };
    let message = if let Some((sample, id)) = sample {
        let options = options(sample, id);
        if options.is_empty() {
            context.action_bar("No attributes for this sample");
            return ItemUseResult::Handled;
        }
        let current = match settings.get(SELECTION) {
            Some(Value::String(choice)) => options.iter().position(|candidate| candidate == choice),
            _ => None,
        };
        let index = current.map_or(0, |index| (index + usize::from(sneak)) % options.len());
        let selection = &options[index];
        settings.insert(SELECTION.into(), Value::String(selection.clone()));
        if sneak {
            format!("Attribute {}/{} - {selection}", index + 1, options.len())
        } else {
            format!(
                "{} {selection} - {}",
                edit(settings, selection),
                describe(settings)
            )
        }
    } else {
        settings.insert(
            MODE.into(),
            Value::Byte(Mode::from_settings(settings).next()),
        );
        describe(settings)
    };
    if fastnbt::to_bytes(&item).is_ok_and(|bytes| context.stack.write(&bytes)) {
        context.action_bar(&message);
    } else {
        context.action_bar("Attribute Filter data is too large");
    }
    ItemUseResult::Handled
}
