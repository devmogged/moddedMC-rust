//! Create's mechanical crafting recipe type.
use super::{Json, ResultStack, Value, read_ingredient};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

pub(crate) const MAX_GRID_SIDE: usize = 9;

#[derive(Deserialize)]
struct RawRecipe {
    pattern: Vec<String>,
    key: HashMap<char, Json>,
    accept_mirrored: bool,
    result: ResultStack,
}

pub(super) struct MechanicalRecipe {
    grid: Vec<Vec<Option<HashSet<String>>>>,
    accept_mirrored: bool,
    result: Value,
}

impl MechanicalRecipe {
    pub(super) fn parse(bytes: &[u8], tags: &mut impl FnMut(&str) -> Vec<String>) -> Option<Self> {
        let raw: RawRecipe = serde_json::from_slice(bytes).ok()?;
        let rows: Vec<Vec<_>> = raw
            .pattern
            .iter()
            .map(|line| line.chars().collect())
            .collect();
        let width = rows.first()?.len();
        if width > MAX_GRID_SIDE
            || rows.len() > MAX_GRID_SIDE
            || rows.iter().any(|row| row.len() != width)
        {
            return None;
        }

        let mut left = width;
        let mut right = 0;
        let mut top = rows.len();
        let mut bottom = 0;
        for (y, row) in rows.iter().enumerate() {
            for (x, &ingredient) in row.iter().enumerate() {
                if ingredient != ' ' {
                    left = left.min(x);
                    right = right.max(x + 1);
                    top = top.min(y);
                    bottom = bottom.max(y + 1);
                }
            }
        }
        let keys: HashMap<_, _> = raw
            .key
            .into_iter()
            .map(|(symbol, value)| {
                let mut ingredients = HashSet::new();
                read_ingredient(&value, tags, &mut ingredients);
                (symbol, ingredients)
            })
            .collect();
        let grid = rows
            .get(top..bottom)?
            .iter()
            .map(|row| {
                row.get(left..right)?
                    .iter()
                    .map(|symbol| {
                        if *symbol == ' ' {
                            Some(None)
                        } else {
                            keys.get(symbol).cloned().map(Some)
                        }
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self {
            grid,
            accept_mirrored: raw.accept_mirrored,
            result: raw.result.into_nbt()?,
        })
    }

    pub(super) fn matching_result(&self, grid: &[Vec<Option<&str>>]) -> Option<&Value> {
        if self.matches(grid, false) || self.accept_mirrored && self.matches(grid, true) {
            Some(&self.result)
        } else {
            None
        }
    }

    fn matches(&self, grid: &[Vec<Option<&str>>], mirrored: bool) -> bool {
        self.grid.len() == grid.len()
            && self.grid.iter().zip(grid).all(|(recipe, row)| {
                recipe.len() == row.len()
                    && recipe.iter().enumerate().all(|(x, required)| {
                        let x = if mirrored { row.len() - 1 - x } else { x };
                        match (required, row[x]) {
                            (None, None) => true,
                            (Some(allowed), Some(item)) => allowed.contains(item),
                            _ => false,
                        }
                    })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanical_patterns_trim_edges_and_mirror_only_when_enabled() {
        let raw = r#"{
            "pattern": ["     ", " A B "],
            "key": {"A": "minecraft:stone", "B": {"tag": "c:rods/wooden"}},
            "accept_mirrored": false,
            "result": {"id": "minecraft:stick", "count": 2}
        }"#;
        let recipe = |json: &str| {
            MechanicalRecipe::parse(json.as_bytes(), &mut |tag| {
                assert_eq!(tag, "c:rods/wooden");
                vec!["minecraft:stick".to_owned()]
            })
            .expect("mechanical pattern")
        };
        let original = vec![Some("minecraft:stone"), None, Some("minecraft:stick")];
        let reversed = vec![Some("minecraft:stick"), None, Some("minecraft:stone")];
        assert!(recipe(raw).matching_result(&[original]).is_some());
        assert!(
            recipe(raw)
                .matching_result(std::slice::from_ref(&reversed))
                .is_none()
        );
        assert!(
            recipe(&raw.replace("false", "true"))
                .matching_result(&[reversed])
                .is_some()
        );
        assert!(
            recipe(raw)
                .matching_result(&[vec![Some("minecraft:stone"), Some("minecraft:stick")]])
                .is_none()
        );
    }
}
