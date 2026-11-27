//! Refined Storage 1.12.4 PatternItem processing-pattern representation.
//!
//! The source item stores nine independent item inputs, nine independent outputs,
//! a Processing flag, and an Exact flag. Fluids and allowed-tag alternatives are
//! intentionally not fabricated: patterns containing those source-only fields are
//! not accepted by this item-only native implementation.

use crate::stack::Stack;
use fastnbt::{ByteArray, Value};
use moddedmc_api::{Pos, content::World};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const VERSION: i32 = 1;
const SLOTS: usize = 9;
const MAX_PATTERN_BYTES: usize = moddedmc_api::interaction::MAX_STACK_DATA;
const EDITOR_KEY: &str = "refinedstorage_processing_editor";

#[derive(Clone, Debug)]
pub struct ProcessingPattern {
    pub inputs: [Option<Stack>; SLOTS],
    pub outputs: [Option<Stack>; SLOTS],
    pub exact: bool,
}

impl Default for ProcessingPattern {
    fn default() -> Self {
        Self {
            inputs: std::array::from_fn(|_| None),
            outputs: std::array::from_fn(|_| None),
            exact: false,
        }
    }
}

fn item_fields(stack: &Stack) -> Option<Compound> {
    let mut fields = stack.key.clone();
    fields.insert("count".into(), Value::Int(i32::try_from(stack.count).ok()?));
    Some(fields)
}

fn decode_item(value: &Value) -> Option<Stack> {
    let Value::Compound(fields) = value else {
        return None;
    };
    Stack::parse(&fastnbt::to_bytes(fields).ok()?)
}

impl ProcessingPattern {
    pub fn valid(&self) -> bool {
        self.outputs.iter().any(Option::is_some)
            && self.inputs.iter().flatten().all(|item| item.count > 0)
            && self.outputs.iter().flatten().all(|item| item.count > 0)
            && self
                .inputs
                .iter()
                .flatten()
                .chain(self.outputs.iter().flatten())
                .all(|item| item.id != "refinedstorage:pattern")
    }

    fn valid_crafting(&self) -> bool {
        self.inputs.iter().any(Option::is_some)
            && self.outputs.iter().flatten().count() <= 1
            && self
                .inputs
                .iter()
                .flatten()
                .chain(self.outputs.iter().flatten())
                .all(|item| item.count > 0 && item.id != "refinedstorage:pattern")
    }

    /// Source-compatible named fields: Version, Processing, Exact, Input_0..8,
    /// and Output_0..8. Missing item fields represent empty slots.
    pub fn item_data(&self) -> Option<Vec<u8>> {
        self.item_data_for(true)
    }

    pub fn item_data_for(&self, processing: bool) -> Option<Vec<u8>> {
        if (processing && !self.valid()) || (!processing && !self.valid_crafting()) {
            return None;
        }
        let mut fields = Compound::from([
            ("Version".into(), Value::Int(VERSION)),
            ("Processing".into(), Value::Byte(i8::from(processing))),
            ("Exact".into(), Value::Byte(i8::from(self.exact))),
        ]);
        for index in 0..SLOTS {
            if let Some(item) = &self.inputs[index] {
                fields.insert(
                    format!("Input_{index}"),
                    Value::Compound(item_fields(item)?),
                );
            }
            if let Some(item) = &self.outputs[index] {
                fields.insert(
                    format!("Output_{index}"),
                    Value::Compound(item_fields(item)?),
                );
            }
        }
        let data = fastnbt::to_bytes(&fields).ok()?;
        (data.len() <= MAX_PATTERN_BYTES).then_some(data)
    }

    pub fn from_item_data(data: &[u8]) -> Option<Self> {
        Self::decode_for(data, true)
    }

    fn decode_for(data: &[u8], processing: bool) -> Option<Self> {
        if data.is_empty() || data.len() > MAX_PATTERN_BYTES {
            return None;
        }
        let fields: Compound = fastnbt::from_bytes(data).ok()?;
        if fields.get("Version") != Some(&Value::Int(VERSION))
            || fields.get("Processing") != Some(&Value::Byte(i8::from(processing)))
        {
            return None;
        }
        // This port has no fluid storage/handler or tag-expansion ABI. Refuse
        // source-only fields rather than silently widening item requirements.
        for key in fields.keys() {
            let matrix_slot = |prefix: &str| {
                key.strip_prefix(prefix)
                    .and_then(|index| index.parse::<usize>().ok())
                    .is_some_and(|index| index < SLOTS)
            };
            if key != "Version"
                && key != "Processing"
                && key != "Exact"
                && !matrix_slot("Input_")
                && !matrix_slot("Output_")
            {
                return None;
            }
        }
        let exact = match fields.get("Exact") {
            None => false,
            Some(Value::Byte(value)) => *value != 0,
            _ => return None,
        };
        let mut pattern = Self::default();
        pattern.exact = exact;
        for index in 0..SLOTS {
            pattern.inputs[index] = match fields.get(&format!("Input_{index}")) {
                Some(value) => Some(decode_item(value)?),
                None => None,
            };
            pattern.outputs[index] = match fields.get(&format!("Output_{index}")) {
                Some(value) => Some(decode_item(value)?),
                None => None,
            };
        }
        if (processing && pattern.valid()) || (!processing && pattern.valid_crafting()) {
            Some(pattern)
        } else {
            None
        }
    }

    pub fn item_stack(&self) -> Option<Vec<u8>> {
        self.item_stack_for(true)
    }

    pub fn item_stack_for(&self, processing: bool) -> Option<Vec<u8>> {
        let data = self.item_data_for(processing)?;
        let fields = Compound::from([
            ("id".into(), Value::String("refinedstorage:pattern".into())),
            ("count".into(), Value::Int(1)),
            (
                "components".into(),
                Value::Compound(Compound::from([(
                    "minecraft:custom_data".into(),
                    Value::Compound(Compound::from([(
                        "moddedmc:data".into(),
                        Value::ByteArray(ByteArray::new(
                            data.iter().map(|value| *value as i8).collect(),
                        )),
                    )])),
                )])),
            ),
        ]);
        fastnbt::to_bytes(&fields).ok()
    }

    pub fn from_stack(stack: &Stack) -> Option<Self> {
        if stack.id != "refinedstorage:pattern" || stack.count != 1 {
            return None;
        }
        let Value::Compound(components) = stack.key.get("components")? else {
            return None;
        };
        let Value::Compound(custom) = components.get("minecraft:custom_data")? else {
            return None;
        };
        let Value::ByteArray(bytes) = custom.get("moddedmc:data")? else {
            return None;
        };
        Self::from_item_data(&bytes.iter().map(|value| *value as u8).collect::<Vec<_>>())
    }

    pub fn same_pattern(&self, other: &Self) -> bool {
        self.exact == other.exact
            && (0..SLOTS).all(|index| match (&self.inputs[index], &other.inputs[index]) {
                (None, None) => true,
                (Some(a), Some(b)) => a.same_item(b) && a.count == b.count,
                _ => false,
            })
            && (0..SLOTS).all(
                |index| match (&self.outputs[index], &other.outputs[index]) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.same_item(b) && a.count == b.count,
                    _ => false,
                },
            )
    }
}

#[derive(Clone, Debug)]
pub struct CraftingPatternItem(pub ProcessingPattern);

impl CraftingPatternItem {
    pub fn from_item_data(data: &[u8]) -> Option<Self> {
        let pattern = ProcessingPattern::decode_for(data, false)?;
        // This port cannot expand source ingredient alternatives. Exact=true is
        // therefore mandatory, and the crafting factory yields one live recipe output.
        (pattern.exact && pattern.valid_crafting()).then_some(Self(pattern))
    }
    pub fn item_data(&self) -> Option<Vec<u8>> {
        if !self.0.exact || !self.0.valid_crafting() {
            return None;
        }
        self.0.item_data_for(false)
    }
    pub fn item_stack(&self) -> Option<Vec<u8>> {
        if !self.0.exact || !self.0.valid_crafting() {
            return None;
        }
        self.0.item_stack_for(false)
    }
    pub fn same_pattern(&self, other: &Self) -> bool {
        self.0.same_pattern(&other.0)
    }
    pub fn valid(&self) -> bool {
        self.0.exact && self.0.valid_crafting()
    }
}

#[derive(Clone, Debug)]
pub struct Editor {
    pub enabled: bool,
    pub pattern: ProcessingPattern,
    pub input_cursor: usize,
    pub output_cursor: usize,
    pub editing_outputs: bool,
    pub processing: bool,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            enabled: false,
            pattern: ProcessingPattern::default(),
            input_cursor: 0,
            output_cursor: 0,
            editing_outputs: false,
            processing: true,
        }
    }
}

impl Editor {
    pub fn encoded_item_data(&self) -> Option<Vec<u8>> {
        if self.processing {
            self.pattern.item_data()
        } else {
            CraftingPatternItem(self.pattern.clone()).item_data()
        }
    }
    pub fn select_input(&mut self, index: usize) -> bool {
        if index >= SLOTS {
            return false;
        }
        self.input_cursor = index;
        true
    }
    pub fn select_output(&mut self, index: usize) -> bool {
        if index >= SLOTS {
            return false;
        }
        self.output_cursor = index;
        true
    }
    pub fn assign(&mut self, item: &Stack) -> bool {
        if item.id == "refinedstorage:pattern" || item.count == 0 {
            return false;
        }
        let Some(raw) = item.with_count(item.count) else {
            return false;
        };
        let Some(item) = Stack::parse(&raw) else {
            return false;
        };
        if self.output_cursor >= SLOTS || self.input_cursor >= SLOTS {
            return false;
        }
        // Source Pattern Grid configures the input and output matrices separately.
        // The selected side is stored in `editing_outputs` by the UI state below.
        if self.editing_outputs {
            self.pattern.outputs[self.output_cursor] = Some(item);
        } else {
            self.pattern.inputs[self.input_cursor] = Some(item);
        }
        true
    }
    pub fn clear_inputs(&mut self) {
        self.pattern.inputs.fill(None);
    }
    pub fn clear_outputs(&mut self) {
        self.pattern.outputs.fill(None);
    }
    pub fn toggle_exact(&mut self) {
        self.pattern.exact = !self.pattern.exact;
    }
    pub fn toggle_processing(&mut self) {
        self.processing = !self.processing;
    }
    pub fn toggle_side(&mut self) {
        self.editing_outputs = !self.editing_outputs;
    }
    pub fn editing_outputs(&self) -> bool {
        self.editing_outputs
    }
    fn encode_slots(slots: &[Option<Stack>; SLOTS]) -> Option<Value> {
        Some(Value::List(
            slots
                .iter()
                .map(|slot| match slot {
                    Some(item) => item_fields(item).map(Value::Compound),
                    None => Some(Value::Compound(Compound::new())),
                })
                .collect::<Option<Vec<_>>>()?,
        ))
    }
    fn decode_slots(value: &Value) -> Option<[Option<Stack>; SLOTS]> {
        let Value::List(entries) = value else {
            return None;
        };
        if entries.len() != SLOTS {
            return None;
        }
        let mut slots = Vec::with_capacity(SLOTS);
        for entry in entries {
            let Value::Compound(fields) = entry else {
                return None;
            };
            slots.push(if fields.is_empty() {
                None
            } else {
                let item = Stack::parse(&fastnbt::to_bytes(fields).ok()?)?;
                if item.id == "refinedstorage:pattern" {
                    return None;
                }
                Some(item)
            });
        }
        slots.try_into().ok()
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.input_cursor >= SLOTS || self.output_cursor >= SLOTS {
            return None;
        }
        root.insert(
            EDITOR_KEY.into(),
            Value::Compound(Compound::from([
                ("version".into(), Value::Int(1)),
                ("enabled".into(), Value::Byte(i8::from(self.enabled))),
                ("exact".into(), Value::Byte(i8::from(self.pattern.exact))),
                ("processing".into(), Value::Byte(i8::from(self.processing))),
                (
                    "editingOutputs".into(),
                    Value::Byte(i8::from(self.editing_outputs)),
                ),
                ("inputCursor".into(), Value::Int(self.input_cursor as i32)),
                ("outputCursor".into(), Value::Int(self.output_cursor as i32)),
                ("inputs".into(), Self::encode_slots(&self.pattern.inputs)?),
                ("outputs".into(), Self::encode_slots(&self.pattern.outputs)?),
            ])),
        );
        Some(())
    }
    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(value) = root.get(EDITOR_KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(saved) = value else {
            return None;
        };
        if saved.get("version") != Some(&Value::Int(1)) {
            return None;
        }
        let flag = |key| match saved.get(key) {
            Some(Value::Byte(value)) => Some(*value != 0),
            _ => None,
        };
        let cursor = |key| match saved.get(key) {
            Some(Value::Int(value)) => usize::try_from(*value).ok().filter(|index| *index < SLOTS),
            _ => None,
        };
        Some(Self {
            enabled: flag("enabled")?,
            pattern: ProcessingPattern {
                inputs: Self::decode_slots(saved.get("inputs")?)?,
                outputs: Self::decode_slots(saved.get("outputs")?)?,
                exact: flag("exact")?,
            },
            processing: flag("processing")?,
            input_cursor: cursor("inputCursor")?,
            output_cursor: cursor("outputCursor")?,
            editing_outputs: flag("editingOutputs")?,
        })
    }
}

pub fn load_editor(world: World<'_>, site: Pos) -> Option<Editor> {
    let snapshot = world.entity_snapshot(site)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Editor::decode(&root)
}

pub fn update_editor<T>(
    world: World<'_>,
    site: Pos,
    mutate: impl FnOnce(&mut Editor) -> Option<T>,
) -> Option<T> {
    let snapshot = world.entity_snapshot(site)?;
    let mut root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    let mut editor = Editor::decode(&root)?;
    let result = mutate(&mut editor)?;
    editor.encode(&mut root)?;
    let bytes = fastnbt::to_bytes(&root).ok()?;
    world
        .compare_entity(site, &snapshot, &bytes)
        .then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, count: i32, damage: i32) -> Stack {
        Stack::parse(
            &fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(count)),
                (
                    "components".into(),
                    Value::Compound(Compound::from([(
                        "minecraft:damage".into(),
                        Value::Int(damage),
                    )])),
                ),
            ]))
            .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn v1124_processing_pattern_roundtrips_all_nine_input_and_output_slots() {
        let mut pattern = ProcessingPattern::default();
        for index in 0..9 {
            pattern.inputs[index] = Some(item(
                "minecraft:iron_ingot",
                (index + 1) as i32,
                index as i32,
            ));
            pattern.outputs[index] = Some(item("minecraft:gold_ingot", (index + 2) as i32, 0));
        }
        pattern.exact = true;
        let bytes = pattern.item_data().unwrap();
        let restored = ProcessingPattern::from_item_data(&bytes).unwrap();
        assert!(pattern.same_pattern(&restored));
        let stack = Stack::parse(&pattern.item_stack().unwrap()).unwrap();
        assert!(
            ProcessingPattern::from_stack(&stack)
                .unwrap()
                .same_pattern(&pattern)
        );
    }

    #[test]
    fn empty_outputs_and_unsupported_fluid_slots_fail_closed() {
        assert!(ProcessingPattern::default().item_data().is_none());
        let mut pattern = ProcessingPattern::default();
        pattern.outputs[0] = Some(item("minecraft:diamond", 4, 0));
        let mut fields: Compound = fastnbt::from_bytes(&pattern.item_data().unwrap()).unwrap();
        fields.insert("FluidInput_0".into(), Value::Compound(Compound::new()));
        assert!(ProcessingPattern::from_item_data(&fastnbt::to_bytes(&fields).unwrap()).is_none());
        fields.remove("FluidInput_0");
        fields.insert("AllowedTags".into(), Value::Compound(Compound::new()));
        assert!(ProcessingPattern::from_item_data(&fastnbt::to_bytes(&fields).unwrap()).is_none());
    }

    #[test]
    fn exact_flag_and_component_sensitive_input_identity_are_preserved() {
        let mut pattern = ProcessingPattern::default();
        pattern.inputs[0] = Some(item("minecraft:iron_ingot", 2, 1));
        pattern.outputs[0] = Some(item("minecraft:iron_block", 1, 0));
        let restored = ProcessingPattern::from_item_data(&pattern.item_data().unwrap()).unwrap();
        assert!(!restored.exact);
        assert!(pattern.same_pattern(&restored));
        let mut different = restored;
        different.inputs[0] = Some(item("minecraft:iron_ingot", 2, 2));
        assert!(!pattern.same_pattern(&different));
    }

    #[test]
    fn v1124_crafting_mode_requires_exact_true_and_one_live_recipe_output() {
        let mut pattern = ProcessingPattern::default();
        pattern.inputs[0] = Some(item("minecraft:stick", 1, 0));
        pattern.inputs[1] = Some(item("minecraft:coal", 1, 0));
        pattern.outputs[0] = Some(item("minecraft:torch", 4, 0));
        assert!(
            CraftingPatternItem::from_item_data(&pattern.item_data_for(false).unwrap()).is_none()
        );
        pattern.outputs.fill(None);
        assert!(
            CraftingPatternItem::from_item_data(&pattern.item_data_for(false).unwrap()).is_some()
        );
        pattern.exact = true;
        let encoded = pattern.item_data_for(false).unwrap();
        assert!(CraftingPatternItem::from_item_data(&encoded).is_some());
        pattern.outputs[1] = Some(item("minecraft:torch", 4, 0));
        assert!(
            CraftingPatternItem::from_item_data(&pattern.item_data_for(false).unwrap()).is_none()
        );
    }

    #[test]
    fn pattern_grid_editor_persists_selected_input_output_and_exact_mode() {
        let mut editor = Editor::default();
        editor.enabled = true;
        editor.select_input(2);
        assert!(editor.assign(&item("minecraft:iron_ingot", 4, 0)));
        editor.toggle_side();
        editor.select_output(5);
        assert!(editor.assign(&item("minecraft:iron_block", 2, 0)));
        editor.toggle_exact();
        let mut root = Compound::from([("foreign".into(), Value::Int(55))]);
        editor.encode(&mut root).unwrap();
        let saved: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        let restored = Editor::decode(&saved).unwrap();
        assert_eq!(saved.get("foreign"), Some(&Value::Int(55)));
        assert!(restored.enabled);
        assert!(restored.processing);
        assert!(restored.pattern.exact);
        assert_eq!(restored.input_cursor, 2);
        assert_eq!(restored.output_cursor, 5);
        assert!(restored.editing_outputs());
        assert_eq!(restored.pattern.inputs[2].as_ref().unwrap().count, 4);
        assert_eq!(restored.pattern.outputs[5].as_ref().unwrap().count, 2);
        let crafting_item =
            CraftingPatternItem::from_item_data(&restored.pattern.item_data_for(false).unwrap())
                .unwrap();
        assert!(crafting_item.0.same_pattern(&restored.pattern));
        assert!(ProcessingPattern::from_item_data(&crafting_item.item_data().unwrap()).is_none());
    }
}
