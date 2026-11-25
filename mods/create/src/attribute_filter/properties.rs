use std::cell::OnceCell;

use fastnbt::Value;
use moddedmc_api::stacks::StackInfo;

use crate::{
    STACKS,
    filtering::{Compound, compound},
    processing_recipes::{self, Process},
};

type Test = fn(&Stack<'_>) -> bool;

const PROPERTIES: &[(&str, Test)] = &[
    ("placeable", |stack| {
        stack.info().is_some_and(|info| info.block)
    }),
    ("consumable", |stack| stack.has("minecraft:food")),
    ("enchanted", |stack| {
        stack
            .component("minecraft:enchantments")
            .as_ref()
            .and_then(compound)
            .is_some_and(|enchantments| !enchantments.is_empty())
    }),
    ("renamed", |stack| stack.has("minecraft:custom_name")),
    ("damaged", |stack| stack.damage().is_some()),
    ("badly_damaged", |stack| {
        stack
            .damage()
            .is_some_and(|(damage, max)| i64::from(damage) * 4 > i64::from(max) * 3)
    }),
    ("not_stackable", |stack| {
        stack.info().is_some_and(|info| info.max_count <= 1) || stack.damage().is_some()
    }),
    ("equipable", |stack| {
        stack
            .component("minecraft:equippable")
            .as_ref()
            .and_then(compound)
            .and_then(|part| part.get("slot"))
            .is_some_and(|slot| matches!(slot, Value::String(slot) if !matches!(slot.as_str(), "mainhand" | "offhand")))
    }),
    ("furnace_fuel", |stack| {
        STACKS
            .get()
            .zip(stack.bytes())
            .is_some_and(|(stacks, bytes)| stacks.fuel(bytes).is_some())
    }),
    ("washable", |stack| stack.recipe(Process::Splashing)),
    ("hauntable", |stack| stack.recipe(Process::Haunting)),
    ("crushable", |stack| {
        stack.recipe(Process::Crushing) || stack.recipe(Process::Milling)
    }),
    ("smeltable", |stack| stack.recipe(Process::Smelting)),
    ("smokable", |stack| stack.recipe(Process::Smoking)),
    ("blastable", |stack| stack.recipe(Process::Blasting)),
    ("compostable", |stack| stack.has("minecraft:compostable")),
];

pub(super) struct Stack<'a> {
    item: &'a Value,
    bytes: OnceCell<Option<Vec<u8>>>,
    info: OnceCell<Option<StackInfo>>,
    damage: OnceCell<Option<(i32, i32)>>,
}

impl<'a> Stack<'a> {
    pub(super) fn new(item: &'a Value) -> Self {
        Self {
            item,
            bytes: OnceCell::new(),
            info: OnceCell::new(),
            damage: OnceCell::new(),
        }
    }

    fn bytes(&self) -> Option<&[u8]> {
        self.bytes
            .get_or_init(|| fastnbt::to_bytes(self.item).ok())
            .as_deref()
    }

    fn info(&self) -> Option<&StackInfo> {
        self.info
            .get_or_init(|| STACKS.get()?.info(self.bytes()?))
            .as_ref()
    }

    fn component_bytes(&self, key: &str) -> Option<Vec<u8>> {
        STACKS.get()?.component(self.bytes()?, key)
    }

    fn component(&self, key: &str) -> Option<Value> {
        let mut data: Compound = fastnbt::from_bytes(&self.component_bytes(key)?).ok()?;
        data.remove("value")
    }

    fn has(&self, key: &str) -> bool {
        self.component_bytes(key).is_some()
    }

    fn damage(&self) -> Option<(i32, i32)> {
        *self.damage.get_or_init(|| {
            let (Value::Int(damage), Value::Int(max)) = (
                self.component("minecraft:damage")?,
                self.component("minecraft:max_damage")?,
            ) else {
                return None;
            };
            (damage > 0 && max > 0 && !self.has("minecraft:unbreakable"))
                .then_some((damage.min(max), max))
        })
    }

    fn recipe(&self, process: Process) -> bool {
        processing_recipes::current().is_some_and(|book| book.find(process, self.item).is_some())
    }

    pub(super) fn applies(&self, name: &str) -> bool {
        PROPERTIES
            .iter()
            .find(|(property, _)| *property == name)
            .is_some_and(|(_, test)| test(self))
    }

    pub(super) fn selections(&self) -> impl Iterator<Item = &'static str> + '_ {
        PROPERTIES
            .iter()
            .filter(|(_, test)| test(self))
            .map(|(name, _)| *name)
    }
}
