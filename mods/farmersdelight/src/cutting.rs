//! Farmer's Delight 1.3.4 CuttingBoardRecipe and ChanceResult.
//! Mirrors source method boundaries; runtime board integration is separate.

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Ingredient {
    Item {
        item: String,
    },
    Tag {
        tag: String,
    },
    ItemAbility {
        #[serde(rename = "type")]
        kind: String,
        action: String,
    },
}

impl Ingredient {
    fn valid(&self) -> bool {
        match self {
            Self::Item { item } => !item.is_empty(),
            Self::Tag { tag } => !tag.is_empty(),
            Self::ItemAbility { kind, action } => {
                kind == "farmersdelight:item_ability" && !action.is_empty()
            }
        }
    }

    /// Original ingredient predicate: vanilla id, tag, or mod item ability.
    pub fn matches(
        &self,
        item_id: &str,
        has_tag: &impl Fn(&str, &str) -> bool,
        has_ability: &impl Fn(&str, &str) -> bool,
    ) -> bool {
        match self {
            Self::Item { item } => item == item_id,
            Self::Tag { tag } => has_tag(item_id, tag),
            Self::ItemAbility { action, .. } => has_ability(item_id, action),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ResultItem {
    pub id: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default)]
    pub components: Option<serde_json::Value>,
}

const fn one() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct ChanceResult {
    pub item: ResultItem,
    #[serde(default = "default_chance")]
    pub chance: f32,
}

const fn default_chance() -> f32 {
    1.0
}

impl ChanceResult {
    /// Java ChanceResult.rollOutput: one random float per source stack item,
    /// even for guaranteed outcomes. Java promotes chance float to double.
    pub fn roll_output(
        &self,
        fortune_level: u32,
        fortune_bonus: f64,
        next_float: &mut impl FnMut() -> f32,
    ) -> Option<ResultItem> {
        let threshold = f64::from(self.chance) + fortune_bonus * f64::from(fortune_level);
        let mut count = 0;
        for _ in 0..self.item.count {
            if f64::from(next_float()) <= threshold {
                count += 1;
            }
        }
        (count > 0).then(|| ResultItem {
            count,
            ..self.item.clone()
        })
    }
}

#[derive(Clone, Debug, Deserialize)]
struct SoundEvent {
    sound_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CuttingBoardRecipe {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    pub group: String,
    pub ingredients: Vec<Ingredient>,
    pub tool: Vec<Ingredient>,
    pub result: Vec<ChanceResult>,
    sound: Option<SoundEvent>,
}

impl CuttingBoardRecipe {
    /// Source Serializer.CODEC: one item input, tool alternatives, at most 4 outputs.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let recipe: Self = serde_json::from_str(json)?;
        if recipe.kind != "farmersdelight:cutting"
            || recipe.ingredients.len() != 1
            || recipe.tool.is_empty()
            || recipe.result.len() > 4
            || !recipe.ingredients.iter().all(Ingredient::valid)
            || !recipe.tool.iter().all(Ingredient::valid)
            || !recipe
                .result
                .iter()
                .all(|r| !r.item.id.is_empty() && r.item.count > 0)
        {
            return Err(<serde_json::Error as serde::de::Error>::custom(
                "invalid cutting recipe definition",
            ));
        }
        Ok(recipe)
    }

    pub fn matches(
        &self,
        input_id: &str,
        tool_id: &str,
        has_tag: impl Fn(&str, &str) -> bool,
        has_ability: impl Fn(&str, &str) -> bool,
    ) -> bool {
        self.ingredients[0].matches(input_id, &has_tag, &has_ability)
            && self
                .tool
                .iter()
                .any(|alternative| alternative.matches(tool_id, &has_tag, &has_ability))
    }

    pub fn sound(&self) -> Option<&str> {
        self.sound.as_ref().map(|sound| sound.sound_id.as_str())
    }

    pub fn roll_results(
        &self,
        fortune_level: u32,
        fortune_bonus: f64,
        mut next_float: impl FnMut() -> f32,
    ) -> Vec<ResultItem> {
        self.result
            .iter()
            .filter_map(|result| result.roll_output(fortune_level, fortune_bonus, &mut next_float))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> CuttingBoardRecipe {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/cutting")
            .join(format!("{name}.json"));
        CuttingBoardRecipe::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn every_original_cutting_fixture_parses_and_preserves_source_fields() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cutting");
        let mut count = 0;
        let mut optional_sounds = 0;
        let mut partial_chances = 0;
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            assert_eq!(path.extension().unwrap(), "json");
            let source: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let recipe = CuttingBoardRecipe::from_json(&source.to_string()).unwrap();
            assert_eq!(recipe.ingredients.len(), 1, "{}", path.display());
            assert!((1..=4).contains(&recipe.result.len()), "{}", path.display());
            assert_eq!(recipe.tool.len(), 2, "{}", path.display());
            assert!(recipe.result.iter().all(|r| r.item.count >= 1));
            optional_sounds += usize::from(recipe.sound().is_some());
            partial_chances += recipe.result.iter().filter(|r| r.chance < 1.0).count();
            count += 1;
        }
        assert_eq!(count, 72);
        assert_eq!(optional_sounds, 21);
        assert_eq!(partial_chances, 8);
    }

    #[test]
    fn original_item_tag_and_ability_matching() {
        let acacia = fixture("acacia_log");
        assert_eq!(acacia.sound(), Some("minecraft:item.axe.strip"));
        assert!(acacia.matches(
            "minecraft:acacia_log",
            "minecraft:iron_axe",
            |item, tag| item == "minecraft:iron_axe" && tag == "minecraft:axes",
            |_, _| false,
        ));
        assert!(acacia.matches(
            "minecraft:acacia_log",
            "test:custom_axe",
            |_, _| false,
            |item, action| item == "test:custom_axe" && action == "axe_strip",
        ));
        assert!(!acacia.matches(
            "minecraft:spruce_log",
            "minecraft:iron_axe",
            |_, _| true,
            |_, _| true,
        ));
        assert!(!acacia.matches(
            "minecraft:acacia_log",
            "minecraft:iron_pickaxe",
            |_, _| false,
            |_, _| false,
        ));

        let allium = fixture("allium");
        assert_eq!(allium.sound(), None);
        assert!(allium.matches(
            "minecraft:allium",
            "test:knife",
            |item, tag| item == "test:knife" && tag == "c:tools/knife",
            |_, _| false,
        ));
        assert!(!allium.matches(
            "minecraft:allium",
            "minecraft:iron_axe",
            |_, _| false,
            |_, _| false,
        ));
    }

    #[test]
    fn each_chance_result_consumes_one_random_draw_per_item() {
        let gravel = fixture("gravel");
        let mut draws = [0.7f32, 0.1].into_iter();
        let produced = gravel.roll_results(0, 0.1, || draws.next().unwrap());
        assert_eq!(produced.len(), 2);
        assert_eq!(produced[0].id, "minecraft:gravel");
        assert_eq!(produced[1].id, "minecraft:flint");
        assert_eq!(draws.next(), None);

        let mut draws = [0.7f32, 0.11].into_iter();
        let produced = gravel.roll_results(0, 0.1, || draws.next().unwrap());
        assert_eq!(produced.len(), 1);
        assert_eq!(produced[0].id, "minecraft:gravel");
        assert_eq!(draws.next(), None);

        let chance = ChanceResult {
            item: ResultItem {
                id: "minecraft:gravel".to_owned(),
                count: 4,
                components: None,
            },
            chance: 0.5,
        };
        let mut draws = [0.4f32, 0.6, 0.5, 0.7].into_iter();
        let rolled = chance
            .roll_output(0, 0.1, &mut || draws.next().unwrap())
            .unwrap();
        assert_eq!(rolled.count, 2);
        assert_eq!(draws.next(), None);
    }

    #[test]
    fn fortune_bonus_and_java_float_to_double_promotion() {
        let gravel = fixture("gravel");
        let mut draws = [0.7f32, 0.15].into_iter();
        let produced = gravel.roll_results(1, 0.1, || draws.next().unwrap());
        assert_eq!(produced.len(), 2);
        assert_eq!(produced[1].id, "minecraft:flint");

        // The source's Float 0.1 is promoted to a Double, so a value
        // encoded as exactly 0.1f32 is on the included boundary.
        let mut draws = [0.7f32, 0.1f32].into_iter();
        assert_eq!(
            gravel.roll_results(0, 0.1, || draws.next().unwrap()).len(),
            2
        );

        let mut draws = [0.7f32, 0.21].into_iter();
        assert_eq!(
            gravel.roll_results(1, 0.1, || draws.next().unwrap()).len(),
            1
        );
    }

    #[test]
    fn validate_recipe_type_and_structural_limits() {
        let original = std::fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cutting/gravel.json"),
        )
        .unwrap();
        let mut data: serde_json::Value = serde_json::from_str(&original).unwrap();
        data["type"] = serde_json::json!("minecraft:crafting_shaped");
        assert!(CuttingBoardRecipe::from_json(&data.to_string()).is_err());
        data["type"] = serde_json::json!("farmersdelight:cutting");
        data["ingredients"] = serde_json::json!([]);
        assert!(CuttingBoardRecipe::from_json(&data.to_string()).is_err());
        data["ingredients"] = serde_json::json!([{"item":"minecraft:gravel"}]);
        data["tool"] = serde_json::json!([]);
        assert!(CuttingBoardRecipe::from_json(&data.to_string()).is_err());
        data["tool"] = serde_json::json!([{"type":"farmersdelight:invalid","action":"shovel_dig"}]);
        assert!(CuttingBoardRecipe::from_json(&data.to_string()).is_err());
        data["tool"] = serde_json::json!([{"tag":"minecraft:shovels"}]);
        let first = data["result"][0].clone();
        data["result"] = serde_json::json!([
            first.clone(),
            first.clone(),
            first.clone(),
            first.clone(),
            first
        ]);
        assert!(CuttingBoardRecipe::from_json(&data.to_string()).is_err());
    }
}
