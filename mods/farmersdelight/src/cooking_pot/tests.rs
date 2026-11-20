use super::*;

fn stack(id: &str, count: u32) -> ResultItem {
    ResultItem {
        id: id.to_owned(),
        count,
        components: None,
    }
}

fn recipe(input: &str, output: &str, output_count: u32, time: i32) -> CookingPotRecipe {
    CookingPotRecipe::from_json(&format!(
        r#"{{"type":"farmersdelight:cooking","ingredients":[{input}],"result":{{"id":"{output}","count":{output_count}}},"cookingtime":{time}}}"#
    )).unwrap()
}

fn same(a: &ResultItem, b: &ResultItem) -> bool {
    a.id == b.id && a.components == b.components
}

fn max_count(id: &str) -> u32 {
    if id == "minecraft:beetroot_soup" {
        1
    } else {
        64
    }
}

fn step(
    pot: &mut CookingPotState,
    heated: bool,
    recipe: Option<(&str, &CookingPotRecipe)>,
) -> CookingTick {
    pot.tick(heated, recipe, |_, _| false, max_count, |_| None, same)
}

#[test]
fn source_heatable_block_checks_direct_fire_lit_and_conduction() {
    let source = HeatBlock {
        source_tag: true,
        ..HeatBlock::default()
    };
    assert!(is_heated(source, HeatBlock::default(), false));
    let unlit = HeatBlock {
        source_tag: true,
        lit: Some(false),
        ..HeatBlock::default()
    };
    assert!(!is_heated(unlit, source, false));
    let conductor = HeatBlock {
        conductor_tag: true,
        ..HeatBlock::default()
    };
    assert!(is_heated(conductor, source, false));
    assert!(!is_heated(conductor, source, true));
    assert!(!is_heated(conductor, unlit, false));
    assert!(!is_heated(HeatBlock::default(), source, false));
    assert!(is_heated(
        HeatBlock {
            source_tag: true,
            lit: Some(true),
            ..HeatBlock::default()
        },
        HeatBlock::default(),
        true
    ));
}

#[test]
fn exact_tick_threshold_consumes_inputs_and_merges_output() {
    let custom = CookingPotRecipe::from_json(
        r#"{"type":"farmersdelight:cooking","ingredients":[{"item":"minecraft:carrot"},{"item":"minecraft:carrot"}],"result":{"id":"farmersdelight:salad","count":2},"cookingtime":3}"#
    ).unwrap();
    let mut pot = CookingPotState::default();
    assert!(pot.set_slot(0, Some(stack("minecraft:carrot", 2))));
    assert!(pot.set_slot(1, Some(stack("minecraft:carrot", 2))));
    assert!(!step(&mut pot, true, Some(("fd:test_salad", &custom))).cooked);
    assert_eq!((pot.cook_time, pot.cook_time_total), (1, 3));
    assert!(!step(&mut pot, true, Some(("fd:test_salad", &custom))).cooked);
    assert_eq!(pot.cook_time, 2);
    let cooked = step(&mut pot, true, Some(("fd:test_salad", &custom)));
    assert!(cooked.cooked && cooked.inventory_changed);
    assert_eq!(cooked.moved_to_output, 2);
    assert_eq!(pot.cook_time, 0);
    assert_eq!(pot.slot(0).unwrap().count, 1);
    assert_eq!(pot.slot(1).unwrap().count, 1);
    assert!(pot.slot(MEAL_DISPLAY_SLOT).is_none());
    assert_eq!(pot.slot(OUTPUT_SLOT).unwrap().count, 2);
    for _ in 0..2 {
        assert!(!step(&mut pot, true, Some(("fd:test_salad", &custom))).cooked);
    }
    assert!(step(&mut pot, true, Some(("fd:test_salad", &custom))).cooked);
    assert_eq!(pot.slot(OUTPUT_SLOT).unwrap().count, 4);
    assert!(pot.slot(0).is_none() && pot.slot(1).is_none());
    assert_eq!(pot.used_recipes["fd:test_salad"], 2);
}

#[test]
fn unheated_or_invalid_recipe_progress_decrements_two_ticks() {
    let custom = recipe(r#"{"item":"minecraft:apple"}"#, "test:cider", 1, 200);
    let mut pot = CookingPotState {
        cook_time: 7,
        cook_time_total: 200,
        ..CookingPotState::default()
    };
    pot.set_slot(0, Some(stack("minecraft:carrot", 1)));
    assert!(!step(&mut pot, true, Some(("test:cider", &custom))).cooked);
    assert_eq!(pot.cook_time, 5);
    step(&mut pot, false, Some(("test:cider", &custom)));
    assert_eq!(pot.cook_time, 3);
    step(&mut pot, false, None);
    assert_eq!(pot.cook_time, 1);
    step(&mut pot, false, None);
    assert_eq!(pot.cook_time, 0);
    assert!(pot.used_recipes.is_empty());
    assert_eq!(pot.slot(0).unwrap().count, 1);
}

#[test]
fn wrong_meal_or_full_buffer_prevents_cooking_without_duplication() {
    let custom = recipe(r#"{"item":"minecraft:carrot"}"#, "test:stew", 1, 1);
    let mut pot = CookingPotState::default();
    pot.set_slot(0, Some(stack("minecraft:carrot", 3)));
    pot.set_slot(MEAL_DISPLAY_SLOT, Some(stack("test:other_meal", 4)));
    pot.set_slot(OUTPUT_SLOT, Some(stack("test:other_meal", 64)));
    assert!(!pot.can_cook(&custom, &max_count, &same));
    assert!(!step(&mut pot, true, Some(("test:stew", &custom))).cooked);
    assert_eq!(pot.slot(0).unwrap().count, 3);
    pot.set_slot(MEAL_DISPLAY_SLOT, Some(stack("test:stew", 64)));
    assert!(!pot.can_cook(&custom, &max_count, &same));
    assert!(!step(&mut pot, true, Some(("test:stew", &custom))).cooked);
    assert_eq!(pot.slot(0).unwrap().count, 3);
    assert_eq!(pot.slot(MEAL_DISPLAY_SLOT).unwrap().count, 64);
    assert!(pot.used_recipes.is_empty());
}

#[test]
fn cooking_ejects_crafting_remainders_and_source_overrides() {
    let custom = recipe(r#"{"item":"minecraft:cod_bucket"}"#, "test:broth", 1, 1);
    let mut pot = CookingPotState::default();
    pot.set_slot(0, Some(stack("minecraft:cod_bucket", 1)));
    let event = step(&mut pot, true, Some(("test:broth", &custom)));
    assert!(event.cooked);
    assert_eq!(event.ejected, vec![stack("minecraft:bucket", 1)]);
    assert!(pot.slot(0).is_none());
    assert_eq!(pot.slot(OUTPUT_SLOT).unwrap().id, "test:broth");
    assert_eq!(
        CookingPotState::ingredient_remainder_override("minecraft:experience_bottle"),
        Some("minecraft:glass_bottle")
    );
    assert_eq!(
        CookingPotState::ingredient_remainder_override("minecraft:beetroot_soup"),
        Some("minecraft:bowl")
    );
    assert_eq!(
        CookingPotState::ingredient_remainder_override("minecraft:stone"),
        None
    );

    let mut pot = CookingPotState::default();
    pot.set_slot(0, Some(stack("minecraft:cod_bucket", 1)));
    let event = pot.tick(
        true,
        Some(("test:broth", &custom)),
        |_, _| false,
        max_count,
        |input| (input.id == "minecraft:cod_bucket").then(|| stack("test:canonical_remainder", 1)),
        same,
    );
    assert_eq!(event.ejected, vec![stack("test:canonical_remainder", 1)]);
}

#[test]
fn explicit_bowl_requirement_uses_only_matching_container_and_output_space() {
    let soup = CookingPotRecipe::from_json(
        r#"{"type":"farmersdelight:cooking","ingredients":[{"item":"minecraft:beetroot"}],"result":{"id":"minecraft:beetroot_soup","count":1},"container":{"id":"minecraft:bowl","count":1},"cookingtime":1}"#
    ).unwrap();
    let mut pot = CookingPotState::default();
    pot.set_slot(0, Some(stack("minecraft:beetroot", 2)));
    pot.set_slot(CONTAINER_SLOT, Some(stack("minecraft:glass_bottle", 2)));
    let event = step(&mut pot, true, Some(("test:beetroot_soup", &soup)));
    assert!(event.cooked);
    assert_eq!(event.containers_used, 0);
    assert_eq!(event.moved_to_output, 0);
    assert_eq!(
        pot.slot(MEAL_DISPLAY_SLOT).unwrap().id,
        "minecraft:beetroot_soup"
    );
    pot.set_slot(CONTAINER_SLOT, Some(stack("minecraft:bowl", 2)));
    let event = step(&mut pot, true, Some(("test:beetroot_soup", &soup)));
    assert!(event.cooked);
    assert_eq!(event.containers_used, 1);
    assert_eq!(pot.slot(CONTAINER_SLOT).unwrap().count, 1);
    assert_eq!(pot.slot(OUTPUT_SLOT).unwrap().count, 1);
    assert_eq!(pot.slot(MEAL_DISPLAY_SLOT).unwrap().count, 1);
    let event = step(&mut pot, false, None);
    assert_eq!(event.containers_used, 0); // single soup output slot is full
    assert_eq!(pot.slot(CONTAINER_SLOT).unwrap().count, 1);
    assert_eq!(pot.used_recipes["test:beetroot_soup"], 2);
    assert!(pot.set_slot(OUTPUT_SLOT, None));
    let event = step(&mut pot, false, None);
    assert_eq!(event.containers_used, 1);
    assert!(pot.slot(CONTAINER_SLOT).is_none());
    assert!(pot.slot(MEAL_DISPLAY_SLOT).is_none());
}

#[test]
fn held_container_serving_consumes_one_only_if_item_matches() {
    let soup = CookingPotRecipe::from_json(
        r#"{"type":"farmersdelight:cooking","ingredients":[{"item":"minecraft:beetroot"}],"result":{"id":"minecraft:beetroot_soup"},"container":{"id":"minecraft:bowl"},"cookingtime":1}"#
    ).unwrap();
    let mut pot = CookingPotState::default();
    pot.set_slot(0, Some(stack("minecraft:beetroot", 2)));
    assert!(step(&mut pot, true, Some(("test:soup", &soup))).cooked);
    let mut wrong = stack("minecraft:glass_bottle", 2);
    assert!(pot.use_held_container(&mut wrong).is_none());
    assert_eq!(wrong.count, 2);
    let mut bowl = stack("minecraft:bowl", 2);
    let portion = pot.use_held_container(&mut bowl).unwrap();
    assert_eq!(portion.id, "minecraft:beetroot_soup");
    assert_eq!(portion.count, 1);
    assert_eq!(bowl.count, 1);
    assert!(pot.slot(MEAL_DISPLAY_SLOT).is_none());
}

#[test]
fn cooking_remainder_of_output_requires_container_and_preserves_input() {
    let custom = recipe(r#"{"item":"minecraft:carrot"}"#, "test:meal", 2, 1);
    let mut pot = CookingPotState::default();
    pot.set_slot(0, Some(stack("minecraft:carrot", 1)));
    let event = pot.tick(
        true,
        Some(("test:meal", &custom)),
        |_, _| false,
        max_count,
        |stack| (stack.id == "test:meal").then(|| stack_from("minecraft:bowl", 1)),
        same,
    );
    assert!(event.cooked);
    assert_eq!(event.moved_to_output, 0);
    assert_eq!(pot.slot(MEAL_DISPLAY_SLOT).unwrap().count, 2);
    assert!(pot.slot(OUTPUT_SLOT).is_none());
    assert_eq!(pot.meal_container.as_ref().unwrap().id, "minecraft:bowl");
}

fn stack_from(id: &str, count: u32) -> ResultItem {
    stack(id, count)
}
