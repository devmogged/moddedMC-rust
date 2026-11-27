//! A Crafting Grid configures a real 3x3 recipe grid. Results come only from
//! the world's recipe manager. Consuming ingredients and granting the result
//! occurs inside one storage drive block-entity CAS.

use crate::stack::Stack;
use fastnbt::Value;
use moddedmc_api::{Pos, content::World, crafting::RecipeMatch};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const KEY: &str = "refinedstorage_grid";

#[derive(Clone, Debug, Default)]
pub struct Pattern {
    pub slots: [Option<Stack>; 9],
    pub cursor: usize,
    selection: Option<RecipeSelection>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RecipeSelection {
    id: Option<String>,
    serializer: String,
    result: Vec<u8>,
}

impl RecipeSelection {
    fn from_match(recipe: &RecipeMatch) -> Option<Self> {
        if recipe.serializer.is_empty()
            || recipe.serializer.len() > 256
            || recipe
                .id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 256)
            || Stack::parse(&recipe.result).is_none()
        {
            return None;
        }
        Some(Self {
            id: recipe.id.clone(),
            serializer: recipe.serializer.clone(),
            result: recipe.result.clone(),
        })
    }

    fn matches(&self, recipe: &RecipeMatch) -> bool {
        if self.id != recipe.id || self.serializer != recipe.serializer {
            return false;
        }
        let (Some(saved), Some(current)) =
            (Stack::parse(&self.result), Stack::parse(&recipe.result))
        else {
            return false;
        };
        saved.same_item(&current) && saved.count == current.count
    }

    fn encode(&self) -> Option<Compound> {
        let result = fastnbt::from_bytes(&self.result).ok()?;
        let mut fields = Compound::from([
            ("serializer".into(), Value::String(self.serializer.clone())),
            ("result".into(), Value::Compound(result)),
        ]);
        if let Some(id) = &self.id {
            fields.insert("id".into(), Value::String(id.clone()));
        }
        Some(fields)
    }

    fn decode(fields: &Compound) -> Option<Self> {
        let (Some(Value::String(serializer)), Some(Value::Compound(result))) =
            (fields.get("serializer"), fields.get("result"))
        else {
            return None;
        };
        let id = match fields.get("id") {
            None => None,
            Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => Some(id.clone()),
            _ => return None,
        };
        if serializer.is_empty() || serializer.len() > 256 {
            return None;
        }
        let result = fastnbt::to_bytes(result).ok()?;
        Stack::parse(&result)?;
        Some(Self {
            id,
            serializer: serializer.clone(),
            result,
        })
    }
}

impl Pattern {
    pub fn add(&mut self, item: &Stack) -> bool {
        if self.cursor >= self.slots.len() {
            return false;
        }
        let Some(raw) = item.with_count(1) else {
            return false;
        };
        let Some(ghost) = Stack::parse(&raw) else {
            return false;
        };
        self.slots[self.cursor] = Some(ghost);
        self.selection = None;
        self.cursor += 1;
        true
    }
    /// Select any crafting cell in the stock menu. The next sneak-held use
    /// writes that exact cell, so shaped recipes can have holes and offsets.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.slots.len() {
            return false;
        }
        self.cursor = index;
        true
    }
    /// Empty-hand sneak-use advances past an intentionally blank cell.
    pub fn skip(&mut self) -> bool {
        if self.cursor >= self.slots.len() {
            return false;
        }
        self.slots[self.cursor] = None;
        self.selection = None;
        self.cursor += 1;
        true
    }
    pub fn clear(&mut self) {
        self.slots.fill(None);
        self.cursor = 0;
        self.selection = None;
    }
    pub fn grid(&self) -> Vec<Vec<u8>> {
        self.slots
            .iter()
            .map(|slot| slot.as_ref().map_or_else(Vec::new, |item| item.raw.clone()))
            .collect()
    }

    /// Resolve the host's effective item remainders once per occupied recipe
    /// cell. Repeated ingredients in separate cells must produce separate
    /// remainders, and the returned stack NBT (including count/components)
    /// must be kept intact for the storage transaction.
    pub fn remainders(
        &self,
        mut remainder: impl FnMut(&[u8]) -> Option<Vec<u8>>,
    ) -> Option<Vec<Stack>> {
        let mut outputs = Vec::new();
        for ingredient in self.slots.iter().flatten() {
            if let Some(raw) = remainder(&ingredient.raw) {
                outputs.push(Stack::parse(&raw)?);
            }
        }
        Some(outputs)
    }

    /// Resolve a saved Polymorph choice against the current live recipe list.
    /// A missing/reloaded recipe falls back to the engine's first live match.
    pub fn selected_recipe<'a>(&self, choices: &'a [RecipeMatch]) -> Option<&'a RecipeMatch> {
        self.selection
            .as_ref()
            .and_then(|saved| choices.iter().find(|choice| saved.matches(choice)))
            .or_else(|| choices.first())
    }

    /// Cycle the selected recipe while retaining its identity across restarts
    /// and recipe-list reordering.
    pub fn cycle_recipe(&mut self, choices: &[RecipeMatch]) -> bool {
        if choices.len() < 2 {
            return false;
        }
        let current = self
            .selection
            .as_ref()
            .and_then(|saved| choices.iter().position(|choice| saved.matches(choice)))
            .unwrap_or(0);
        let next = (current + 1) % choices.len();
        let Some(selection) = RecipeSelection::from_match(&choices[next]) else {
            return false;
        };
        self.selection = Some(selection);
        true
    }

    pub fn remember_recipe(&mut self, recipe: &RecipeMatch) -> bool {
        let Some(selection) = RecipeSelection::from_match(recipe) else {
            return false;
        };
        self.selection = Some(selection);
        true
    }

    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(saved) = root.get(KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(saved) = saved else {
            return None;
        };
        let (Some(Value::Int(cursor)), Some(Value::List(entries))) =
            (saved.get("cursor"), saved.get("slots"))
        else {
            return None;
        };
        let cursor = usize::try_from(*cursor).ok()?;
        if cursor > 9 || entries.len() != 9 {
            return None;
        }
        let mut slots: [Option<Stack>; 9] = std::array::from_fn(|_| None);
        for (index, entry) in entries.iter().enumerate() {
            let Value::Compound(fields) = entry else {
                return None;
            };
            if fields.is_empty() {
                continue;
            }
            let stack = Stack::parse(&fastnbt::to_bytes(fields).ok()?)?;
            if stack.count != 1 {
                return None;
            }
            slots[index] = Some(stack);
        }
        let selection = match saved.get("selection") {
            None => None,
            Some(Value::Compound(fields)) => Some(RecipeSelection::decode(fields)?),
            _ => return None,
        };
        Some(Self {
            slots,
            cursor,
            selection,
        })
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.cursor > 9 {
            return None;
        }
        let slots = self
            .slots
            .iter()
            .map(|item| {
                let fields = item.as_ref().map_or_else(Compound::new, |stack| {
                    let mut fields = stack.key.clone();
                    fields.insert("count".into(), Value::Int(1));
                    fields
                });
                Value::Compound(fields)
            })
            .collect();
        let mut saved = Compound::from([
            (
                "cursor".into(),
                Value::Int(i32::try_from(self.cursor).ok()?),
            ),
            ("slots".into(), Value::List(slots)),
        ]);
        if let Some(selection) = &self.selection {
            saved.insert("selection".into(), Value::Compound(selection.encode()?));
        }
        root.insert(KEY.into(), Value::Compound(saved));
        Some(())
    }
}

pub fn load(world: World<'_>, pos: Pos) -> Option<Pattern> {
    let snapshot = world.entity_snapshot(pos)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Pattern::decode(&root)
}
pub fn update(world: World<'_>, pos: Pos, apply: impl FnOnce(&mut Pattern) -> bool) -> bool {
    let Some(snapshot) = world.entity_snapshot(pos) else {
        return false;
    };
    let Ok(mut root) = fastnbt::from_bytes::<Compound>(&snapshot.nbt) else {
        return false;
    };
    let Some(mut pattern) = Pattern::decode(&root) else {
        return false;
    };
    if !apply(&mut pattern) || pattern.encode(&mut root).is_none() {
        return false;
    }
    let Ok(bytes) = fastnbt::to_bytes(&root) else {
        return false;
    };
    world.compare_entity(pos, &snapshot, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> Stack {
        Stack::parse(
            &fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(1)),
            ]))
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn nine_ghost_ingredients_round_trip_and_clear() {
        let mut pattern = Pattern::default();
        let diamond = item("minecraft:diamond");
        for _ in 0..9 {
            assert!(pattern.add(&diamond));
        }
        assert!(!pattern.add(&diamond));
        assert_eq!(pattern.grid().len(), 9);
        let mut nbt = Compound::from([("foreign".into(), Value::Byte(7))]);
        pattern.encode(&mut nbt).unwrap();
        let copy: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&nbt).unwrap()).unwrap();
        assert_eq!(copy.get("foreign"), Some(&Value::Byte(7)));
        let mut restored = Pattern::decode(&copy).unwrap();
        assert_eq!(restored.slots.iter().flatten().count(), 9);
        restored.clear();
        assert_eq!(restored.cursor, 0);
        assert!(restored.grid().iter().all(Vec::is_empty));
    }

    #[test]
    fn noncontiguous_shaped_grid_and_menu_cursor_survive_serialization() {
        let plank = item("minecraft:oak_planks");
        let stick = item("minecraft:stick");
        let mut pattern = Pattern::default();
        for _ in 0..3 {
            assert!(pattern.add(&plank));
        }
        assert!(pattern.skip());
        assert!(pattern.add(&stick));
        assert!(pattern.skip());
        assert!(pattern.skip());
        assert!(pattern.add(&stick));
        assert!(pattern.skip());
        assert_eq!(pattern.cursor, 9);
        assert_eq!(pattern.slots[0].as_ref().unwrap().id, plank.id);
        assert_eq!(pattern.slots[4].as_ref().unwrap().id, stick.id);
        assert!(pattern.slots[3].is_none());
        assert!(pattern.slots[5].is_none());
        assert!(pattern.slots[8].is_none());
        assert!(!pattern.skip());
        assert!(pattern.select(8));
        assert!(pattern.add(&plank));
        assert!(!pattern.select(9));
        let mut nbt = Compound::new();
        pattern.encode(&mut nbt).unwrap();
        let persisted: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&nbt).unwrap()).unwrap();
        let restored = Pattern::decode(&persisted).unwrap();
        assert_eq!(restored.slots[4].as_ref().unwrap().id, stick.id);
        assert_eq!(restored.slots[8].as_ref().unwrap().id, plank.id);
        assert!(restored.slots[3].is_none());
    }

    #[test]
    fn effective_remainders_are_resolved_per_cell_and_keep_components_and_counts() {
        let bucket = item("minecraft:milk_bucket");
        let mut pattern = Pattern::default();
        assert!(pattern.add(&bucket));
        assert!(pattern.select(4));
        assert!(pattern.add(&bucket));

        let outputs = pattern
            .remainders(|ingredient| {
                let parsed = Stack::parse(ingredient)?;
                (parsed.id == "minecraft:milk_bucket").then(|| {
                    fastnbt::to_bytes(&Compound::from([
                        ("id".into(), Value::String("minecraft:bucket".into())),
                        ("count".into(), Value::Int(2)),
                        (
                            "components".into(),
                            Value::Compound(Compound::from([(
                                "minecraft:custom_data".into(),
                                Value::Compound(Compound::from([(
                                    "source".into(),
                                    Value::String("recipe-remainder".into()),
                                )])),
                            )])),
                        ),
                    ]))
                    .unwrap()
                })
            })
            .unwrap();

        assert_eq!(outputs.len(), 2);
        assert!(outputs.iter().all(|stack| {
            stack.id == "minecraft:bucket"
                && stack.count == 2
                && stack.key.contains_key("components")
        }));
    }

    #[test]
    fn polymorph_choice_persists_by_recipe_identity_across_reordering() {
        let recipe = |id: &str, result: &str| RecipeMatch {
            result: item(result).raw,
            id: Some(id.into()),
            serializer: "minecraft:crafting_shaped".into(),
        };
        let oak = recipe("test:oak_result", "minecraft:oak_planks");
        let birch = recipe("test:birch_result", "minecraft:birch_planks");
        let choices = vec![oak, birch.clone()];
        let mut pattern = Pattern::default();

        assert!(pattern.cycle_recipe(&choices));
        assert_eq!(
            pattern.selected_recipe(&choices).unwrap().id.as_deref(),
            Some("test:birch_result")
        );

        let mut root = Compound::new();
        pattern.encode(&mut root).unwrap();
        let restored =
            Pattern::decode(&fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap())
                .unwrap();
        let reordered = vec![birch, choices[0].clone()];
        assert_eq!(
            restored.selected_recipe(&reordered).unwrap().id.as_deref(),
            Some("test:birch_result")
        );
        assert_eq!(
            restored
                .selected_recipe(&[choices[0].clone()])
                .unwrap()
                .id
                .as_deref(),
            Some("test:oak_result")
        );
        let mut cleared = restored;
        cleared.clear();
        assert!(cleared.selection.is_none());
    }
}
