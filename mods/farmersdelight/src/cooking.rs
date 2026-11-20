//! Farmer's Delight 1.3.4 CookingPotRecipe matching and metadata.
//! Source: common/crafting/CookingPotRecipe.java, 1.21.1 NeoForge.
//! Mod-owned exact shapeless 1-to-1 matching; live pot integration deferred.

use crate::cutting::ResultItem;

/// Vanilla item/tag ingredient or NeoForge compound/difference ingredient.
/// The latter are not simple tags: Compound is OR and Difference is base
/// minus subtracted, per NeoForge 1.21.1 CompoundIngredient/DifferenceIngredient.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum CookingIngredient {
    Item {
        item: String,
    },
    Tag {
        tag: String,
    },
    Compound {
        #[serde(rename = "type")]
        kind: String,
        children: Vec<CookingIngredient>,
    },
    Difference {
        #[serde(rename = "type")]
        kind: String,
        base: Box<CookingIngredient>,
        subtracted: Box<CookingIngredient>,
    },
}

impl CookingIngredient {
    fn valid(&self) -> bool {
        match self {
            Self::Item { item } => !item.is_empty(),
            Self::Tag { tag } => !tag.is_empty(),
            Self::Compound { kind, children } => {
                kind == "neoforge:compound"
                    && !children.is_empty()
                    && children.iter().all(Self::valid)
            }
            Self::Difference {
                kind,
                base,
                subtracted,
            } => kind == "neoforge:difference" && base.valid() && subtracted.valid(),
        }
    }

    pub fn matches(&self, item_id: &str, has_tag: &impl Fn(&str, &str) -> bool) -> bool {
        match self {
            Self::Item { item } => item == item_id,
            Self::Tag { tag } => has_tag(item_id, tag),
            Self::Compound { children, .. } => {
                children.iter().any(|child| child.matches(item_id, has_tag))
            }
            Self::Difference {
                base, subtracted, ..
            } => base.matches(item_id, has_tag) && !subtracted.matches(item_id, has_tag),
        }
    }
}
use serde::Deserialize;

pub const INPUT_SLOTS: usize = 6;
pub const DEFAULT_COOK_TIME: u32 = 200;

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RecipeBookTab {
    Meals,
    Drinks,
    #[default]
    Misc,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CookingPotRecipe {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub recipe_book_tab: RecipeBookTab,
    pub ingredients: Vec<CookingIngredient>,
    pub result: ResultItem,
    /// Original explicit container override; default crafting remainder is
    /// resolved via host item-component data when gameplay is integrated.
    #[serde(default)]
    pub container: Option<ResultItem>,
    #[serde(default)]
    pub experience: f32,
    #[serde(default = "default_cook_time")]
    pub cookingtime: u32,
}

const fn default_cook_time() -> u32 {
    DEFAULT_COOK_TIME
}

impl CookingPotRecipe {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let recipe: Self = serde_json::from_str(json)?;
        let valid = CookingIngredient::valid;
        if recipe.kind != "farmersdelight:cooking"
            || recipe.ingredients.is_empty()
            || !recipe.ingredients.iter().all(valid)
            || recipe.result.id.is_empty()
            || recipe.result.count == 0
            || recipe
                .container
                .as_ref()
                .is_some_and(|c| c.id.is_empty() || c.count == 0)
        {
            return Err(<serde_json::Error as serde::de::Error>::custom(
                "invalid cooking-pot recipe definition",
            ));
        }
        Ok(recipe)
    }

    /// Return which unique inventory slot supplies each source ingredient.
    /// Original matches() collects 0..6 nonempty slots, checks exact count,
    /// and requires RecipeMatcher.findMatches to return a valid assignment.
    /// Backtracking avoids greedy false negatives for overlapping tags.
    pub fn matching_slots(
        &self,
        slots: &[Option<&str>; INPUT_SLOTS],
        has_tag: impl Fn(&str, &str) -> bool,
    ) -> Option<Vec<usize>> {
        let nonempty: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter_map(|(index, item)| item.map(|_| index))
            .collect();
        if nonempty.len() != self.ingredients.len() {
            return None;
        }
        let mut assigned = Vec::with_capacity(self.ingredients.len());
        let mut used = [false; INPUT_SLOTS];

        fn backtrack(
            index: usize,
            recipe: &[CookingIngredient],
            slots: &[Option<&str>; INPUT_SLOTS],
            nonempty: &[usize],
            used: &mut [bool; INPUT_SLOTS],
            assigned: &mut Vec<usize>,
            has_tag: &impl Fn(&str, &str) -> bool,
        ) -> bool {
            if index == recipe.len() {
                return true;
            }
            for &slot in nonempty {
                if used[slot] {
                    continue;
                }
                let Some(item_id) = slots[slot] else {
                    continue;
                };
                if recipe[index].matches(item_id, has_tag) {
                    used[slot] = true;
                    assigned.push(slot);
                    if backtrack(index + 1, recipe, slots, nonempty, used, assigned, has_tag) {
                        return true;
                    }
                    assigned.pop();
                    used[slot] = false;
                }
            }
            false
        }

        backtrack(
            0,
            &self.ingredients,
            slots,
            &nonempty,
            &mut used,
            &mut assigned,
            &has_tag,
        )
        .then_some(assigned)
    }
}

#[cfg(test)]
mod tests;
