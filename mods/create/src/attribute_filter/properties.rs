use std::cell::OnceCell;

use fastnbt::Value;
use moddedmc_api::stacks::StackInfo;

use crate::{
    STACKS, TAGS,
    filtering::{Compound, compound},
    fluids::item_drain::bucket_fluid,
    processing_recipes::{self, Process, item_id},
};

const SHULKER_SLOTS: usize = 27;
const HAS_ENCHANT: &str = "has_enchant";
const BOOK_AUTHOR: &str = "book_author";
const BOOK_COPY: &str = "book_copy";
const SHULKER_FILL: &str = "shulker_fill";
const HAS_NAME: &str = "has_name";
const COLOR: &str = "color";
const HAS_FLUID: &str = "has_fluid";

// Vanilla DyeColor names and their firework colors.
const DYE_COLORS: [(&str, i32); 16] = [
    ("white", 15_790_320),
    ("orange", 15_435_844),
    ("magenta", 12_801_229),
    ("light_blue", 6_719_955),
    ("yellow", 14_602_026),
    ("lime", 4_312_372),
    ("pink", 14_188_952),
    ("gray", 4_408_131),
    ("light_gray", 11_250_603),
    ("cyan", 2_651_799),
    ("purple", 8_073_150),
    ("blue", 2_437_522),
    ("brown", 5_320_730),
    ("green", 3_887_386),
    ("red", 11_743_532),
    ("black", 1_973_019),
];

type Test = fn(&Stack<'_>) -> bool;

const PROPERTIES: &[(&str, Test)] = &[
    ("placeable", |stack| {
        stack.info().is_some_and(|info| info.block)
    }),
    ("consumable", |stack| stack.has("minecraft:food")),
    ("fluid_container", |stack| stack.fluid().is_some()),
    ("enchanted", |stack| {
        stack
            .component("minecraft:enchantments")
            .as_ref()
            .and_then(compound)
            .is_some_and(|enchantments| !enchantments.is_empty())
    }),
    ("max_enchanted", |stack| {
        STACKS
            .get()
            .zip(stack.bytes())
            .is_some_and(|(stacks, bytes)| stacks.has_maxed_enchantment(bytes))
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

    fn enchants(&self) -> Option<Compound> {
        let key = if item_id(self.item) == Some("minecraft:enchanted_book") {
            "minecraft:stored_enchantments"
        } else {
            "minecraft:enchantments"
        };
        match self.component(key)? {
            Value::Compound(enchants) => Some(enchants),
            _ => None,
        }
    }

    fn book(&self) -> Option<(String, i32)> {
        let Value::Compound(mut book) = self.component("minecraft:written_book_content")? else {
            return None;
        };
        let (Some(Value::String(author)), Some(Value::Int(generation))) =
            (book.remove("author"), book.remove("generation"))
        else {
            return None;
        };
        Some((author, generation))
    }

    fn name(&self) -> Option<String> {
        let Value::String(name) = self.component("minecraft:custom_name")? else {
            return None;
        };
        (!name.is_empty()).then_some(name)
    }

    fn fluid(&self) -> Option<&'static str> {
        item_id(self.item).and_then(bucket_fluid)
    }

    fn colors(&self) -> Vec<&'static str> {
        let Some(id) = item_id(self.item) else {
            return Vec::new();
        };
        if let Some(tags) = TAGS.get()
            && tags.contains("item", "c:dyes", id)
            && let Some(dye) = DYE_COLORS
                .iter()
                .map(|&(name, _)| name)
                .find(|name| tags.contains("item", &format!("c:dyes/{name}"), id))
        {
            return vec![dye];
        }
        let path = id.split_once(':').map_or(id, |(_, path)| path);
        let mut colors: Vec<_> = DYE_COLORS
            .iter()
            .map(|&(name, _)| name)
            .filter(|name| {
                path.strip_prefix(name)
                    .is_some_and(|suffix| suffix.starts_with('_'))
            })
            .collect();
        if !matches!(id, "minecraft:firework_rocket" | "minecraft:firework_star") {
            return colors;
        }
        let fireworks = self.component("minecraft:fireworks");
        let Some(Value::List(explosions)) = fireworks
            .as_ref()
            .and_then(compound)
            .and_then(|fields| fields.get("explosions"))
        else {
            return colors;
        };
        for explosion in explosions.iter().filter_map(compound) {
            for key in ["colors", "fade_colors"] {
                if let Some(Value::IntArray(rgb)) = explosion.get(key) {
                    for (dye, value) in DYE_COLORS {
                        if rgb.contains(&value) && !colors.contains(&dye) {
                            colors.push(dye);
                        }
                    }
                }
            }
        }
        colors
    }

    fn shulker_fill(&self) -> Option<&'static str> {
        let id = item_id(self.item)?;
        if !TAGS.get()?.contains("item", "minecraft:shulker_boxes", id) {
            return None;
        }
        let Some(Value::List(items)) = self.component("minecraft:container") else {
            return Some("empty");
        };
        if items.is_empty() {
            return Some("empty");
        }
        if self.has("minecraft:container_loot") {
            return None;
        }
        if items.len() != SHULKER_SLOTS {
            return Some("partial");
        }
        let stacks = STACKS.get()?;
        let mut full = [false; SHULKER_SLOTS];
        for item in &items {
            let Some(fields) = compound(item) else {
                continue;
            };
            let Some(Value::Int(slot)) = fields.get("slot") else {
                continue;
            };
            if let Some(full) = usize::try_from(*slot)
                .ok()
                .and_then(|slot| full.get_mut(slot))
            {
                *full = fields
                    .get("item")
                    .and_then(|item| fastnbt::to_bytes(item).ok())
                    .and_then(|bytes| stacks.info(&bytes))
                    .is_some_and(|info| info.count == info.max_count);
            }
        }
        Some(if full.into_iter().all(|full| full) {
            "full"
        } else {
            "partial"
        })
    }

    pub(super) fn applies(&self, name: &str) -> bool {
        if let Some((property, expected)) = name.split_once('=') {
            return match property {
                HAS_ENCHANT => self
                    .enchants()
                    .is_some_and(|enchants| enchants.contains_key(expected)),
                BOOK_AUTHOR => self.book().is_some_and(|(author, _)| author == expected),
                BOOK_COPY => self
                    .book()
                    .is_some_and(|(_, generation)| expected.parse::<i32>() == Ok(generation)),
                SHULKER_FILL => self.shulker_fill() == Some(expected),
                HAS_NAME => self.name().is_some_and(|name| name == expected),
                COLOR => self.colors().contains(&expected),
                HAS_FLUID => self.fluid() == Some(expected),
                _ => false,
            };
        }
        PROPERTIES
            .iter()
            .find(|(property, _)| *property == name)
            .is_some_and(|(_, test)| test(self))
    }

    pub(super) fn add_selections(&self, options: &mut Vec<String>) {
        options.extend(
            PROPERTIES
                .iter()
                .filter(|(_, test)| test(self))
                .map(|(name, _)| (*name).to_owned()),
        );
        if let Some(enchants) = self.enchants() {
            let mut ids: Vec<_> = enchants.keys().collect();
            ids.sort_unstable();
            options.extend(ids.into_iter().map(|id| format!("{HAS_ENCHANT}={id}")));
        }
        if let Some((author, generation)) = self.book() {
            if !author.is_empty() {
                options.push(format!("{BOOK_AUTHOR}={author}"));
            }
            options.push(format!("{BOOK_COPY}={generation}"));
        }
        if let Some(level) = self.shulker_fill() {
            options.push(format!("{SHULKER_FILL}={level}"));
        }
        if let Some(name) = self.name() {
            options.push(format!("{HAS_NAME}={name}"));
        }
        if let Some(fluid) = self.fluid() {
            options.push(format!("{HAS_FLUID}={fluid}"));
        }
        options.extend(
            self.colors()
                .into_iter()
                .map(|color| format!("{COLOR}={color}")),
        );
    }
}
