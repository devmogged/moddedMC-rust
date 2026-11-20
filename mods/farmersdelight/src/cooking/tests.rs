use super::*;
use std::path::PathBuf;

fn fixture(name: &str) -> CookingPotRecipe {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/cooking")
        .join(format!("{name}.json"));
    CookingPotRecipe::from_json(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn every_original_cooking_recipe_parses_and_defaults_match_java() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cooking");
    let mut count = 0;
    let mut tabs = [0_usize; 3];
    let mut explicit_time = 0;
    let mut container_overrides = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(path.extension().unwrap(), "json");
        let original: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let recipe = CookingPotRecipe::from_json(&original.to_string()).unwrap();
        assert_eq!(recipe.group, original["group"].as_str().unwrap_or(""));
        assert!(
            (1..=6).contains(&recipe.ingredients.len()),
            "{}",
            path.display()
        );
        assert!(recipe.result.count >= 1);
        assert!(recipe.experience.is_finite());
        match recipe.recipe_book_tab {
            RecipeBookTab::Meals => tabs[0] += 1,
            RecipeBookTab::Drinks => tabs[1] += 1,
            RecipeBookTab::Misc => tabs[2] += 1,
        }
        explicit_time += usize::from(recipe.cookingtime != DEFAULT_COOK_TIME);
        container_overrides += usize::from(recipe.container.is_some());
        count += 1;
    }
    assert_eq!(count, 28);
    assert_eq!(tabs, [20, 2, 6]);
    assert_eq!(explicit_time, 4);
    assert_eq!(container_overrides, 4);
}

#[test]
fn duplicate_ingredients_require_distinct_slots_in_any_order() {
    let cider = fixture("apple_cider");
    assert!(
        cider
            .matching_slots(&[None; INPUT_SLOTS], |_, _| false)
            .is_none()
    );
    let one_apple = [
        Some("minecraft:apple"),
        Some("minecraft:sugar"),
        None,
        None,
        None,
        None,
    ];
    assert!(cider.matching_slots(&one_apple, |_, _| false).is_none());
    let shuffled = [
        Some("minecraft:sugar"),
        None,
        Some("minecraft:apple"),
        None,
        Some("minecraft:apple"),
        None,
    ];
    let matching = cider.matching_slots(&shuffled, |_, _| false).unwrap();
    assert_eq!(matching.len(), 3);
    assert_ne!(matching[0], matching[1]);
    assert_eq!(matching[0], 2);
    assert_eq!(matching[1], 4);
    assert_eq!(matching[2], 0);
    assert_eq!(cider.result.id, "farmersdelight:apple_cider");
    assert_eq!(cider.recipe_book_tab, RecipeBookTab::Drinks);
    assert_eq!(cider.cookingtime, DEFAULT_COOK_TIME);

    let extra = [
        Some("minecraft:sugar"),
        Some("minecraft:apple"),
        Some("minecraft:apple"),
        Some("minecraft:dirt"),
        None,
        None,
    ];
    assert!(cider.matching_slots(&extra, |_, _| false).is_none());
}

#[test]
fn overlapping_tags_use_backtracking_not_a_greedy_match() {
    let recipe = CookingPotRecipe::from_json(
        r#"{"type":"farmersdelight:cooking","ingredients":[{"tag":"test:fruit"},{"item":"minecraft:apple"}],"result":{"id":"test:result","count":1}}"#
    ).unwrap();
    let slots = [
        Some("minecraft:apple"),
        Some("minecraft:melon_slice"),
        None,
        None,
        None,
        None,
    ];
    let assigned = recipe
        .matching_slots(&slots, |item, tag| {
            tag == "test:fruit" && matches!(item, "minecraft:apple" | "minecraft:melon_slice")
        })
        .unwrap();
    assert_eq!(assigned, vec![1, 0]);

    let impossible = [
        Some("minecraft:melon_slice"),
        Some("minecraft:carrot"),
        None,
        None,
        None,
        None,
    ];
    assert!(recipe.matching_slots(&impossible, |_, _| true).is_none());
}

#[test]
fn original_tag_recipe_and_explicit_container_are_preserved() {
    let soup = fixture("beetroot_soup");
    assert_eq!(soup.result.id, "minecraft:beetroot_soup");
    assert_eq!(soup.container.as_ref().unwrap().id, "minecraft:bowl");
    assert_eq!(soup.recipe_book_tab, RecipeBookTab::Meals);
    assert_eq!(soup.experience, 1.0_f32);
    assert_eq!(soup.cookingtime, DEFAULT_COOK_TIME);
    let slots = [
        Some("minecraft:beetroot"),
        None,
        Some("minecraft:beetroot"),
        None,
        Some("minecraft:beetroot"),
        None,
    ];
    assert!(
        soup.matching_slots(&slots, |item, tag| {
            item == "minecraft:beetroot" && tag == "c:crops/beetroot"
        })
        .is_some()
    );
    assert_eq!(fixture("apple_cider").container, None);
}

#[test]
fn invalid_type_tab_or_empty_ingredients_fail() {
    let wrong_type = r#"{"type":"minecraft:crafting_shaped","ingredients":[{"item":"minecraft:carrot"}],"result":{"id":"test:out"}}"#;
    assert!(CookingPotRecipe::from_json(wrong_type).is_err());
    let invalid_tab = r#"{"type":"farmersdelight:cooking","recipe_book_tab":"unknown","ingredients":[{"item":"minecraft:carrot"}],"result":{"id":"test:out"}}"#;
    assert!(CookingPotRecipe::from_json(invalid_tab).is_err());
    let empty = r#"{"type":"farmersdelight:cooking","ingredients":[],"result":{"id":"test:out"}}"#;
    assert!(CookingPotRecipe::from_json(empty).is_err());
    let invalid_ability = r#"{"type":"farmersdelight:cooking","ingredients":[{"type":"farmersdelight:item_ability","action":"knife_dig"}],"result":{"id":"test:out"}}"#;
    assert!(CookingPotRecipe::from_json(invalid_ability).is_err());
}

#[test]
fn original_compound_ingredient_matches_any_child_item_or_tag() {
    let recipe = fixture("bone_broth");
    let alternative = &recipe.ingredients[1];
    assert!(alternative.matches("minecraft:glow_berries", &|_, _| false));
    assert!(alternative.matches("minecraft:hanging_roots", &|_, _| false));
    assert!(alternative.matches("minecraft:red_mushroom", &|item, tag| {
        item == "minecraft:red_mushroom" && tag == "c:mushrooms"
    }));
    assert!(!alternative.matches("minecraft:stone", &|_, _| false));

    let another = fixture("mushroom_rice");
    assert!(another.ingredients[3].matches("minecraft:carrot", &|_, _| false));
    assert!(another.ingredients[3].matches("minecraft:potato", &|_, _| false));
    assert!(!another.ingredients[3].matches("minecraft:oak_log", &|_, _| false));
}

#[test]
fn original_difference_ingredient_subtracts_even_when_base_matches() {
    let soup = fixture("chicken_soup");
    let vegetable = &soup.ingredients[3];
    let tag = |item: &str, category: &str| {
        category == "c:foods/vegetable"
            && matches!(item, "minecraft:carrot" | "minecraft:melon_slice")
    };
    assert!(vegetable.matches("minecraft:carrot", &tag));
    assert!(!vegetable.matches("minecraft:melon_slice", &tag));
    assert!(!vegetable.matches("minecraft:cooked_beef", &tag));
    for name in ["ratatouille", "stuffed_pumpkin_block", "vegetable_noodles"] {
        let recipe = fixture(name);
        let difference = recipe
            .ingredients
            .iter()
            .find(|item| matches!(item, CookingIngredient::Difference { .. }))
            .unwrap();
        assert!(difference.matches("minecraft:carrot", &tag), "{name}");
        assert!(!difference.matches("minecraft:melon_slice", &tag), "{name}");
    }
}

#[test]
fn invalid_nested_ingredients_fail_closed() {
    for ingredient in [
        r#"{"type":"neoforge:compound","children":[]}"#,
        r#"{"type":"neoforge:compound","children":[{"item":""}]}"#,
        r#"{"type":"neoforge:invalid","children":[{"item":"minecraft:apple"}]}"#,
        r#"{"type":"neoforge:difference","base":{"item":"minecraft:apple"},"subtracted":{"tag":""}}"#,
        r#"{"type":"neoforge:invalid","base":{"item":"minecraft:apple"},"subtracted":{"item":"minecraft:stone"}}"#,
    ] {
        let document = format!(
            r#"{{"type":"farmersdelight:cooking","ingredients":[{ingredient}],"result":{{"id":"test:output"}}}}"#
        );
        assert!(
            CookingPotRecipe::from_json(&document).is_err(),
            "{ingredient}"
        );
    }
}
