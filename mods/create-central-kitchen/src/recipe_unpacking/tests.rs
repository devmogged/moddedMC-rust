use super::*;

#[derive(Clone)]
struct Slot {
    stack: Stack,
    limit: i32,
    valid: bool,
}

struct TestInventory {
    slots: Vec<Slot>,
}

impl TestInventory {
    fn empty(size: usize) -> Self {
        Self {
            slots: vec![
                Slot {
                    stack: Stack::empty(),
                    limit: 64,
                    valid: true,
                };
                size
            ],
        }
    }
}

impl ItemHandler for TestInventory {
    fn slots(&self) -> usize {
        self.slots.len()
    }

    fn stack_in_slot(&self, slot: usize) -> Stack {
        self.slots[slot].stack.clone()
    }

    fn slot_limit(&self, slot: usize) -> i32 {
        self.slots[slot].limit
    }

    fn is_item_valid(&self, slot: usize, _: &Stack) -> bool {
        self.slots[slot].valid
    }

    fn insert_item(&mut self, slot: usize, stack: &Stack, simulate: bool) -> i32 {
        let target = &mut self.slots[slot];
        if !target.valid
            || (!target.stack.is_empty() && !target.stack.is_same_item_same_components(stack))
        {
            return stack.count;
        }
        let space = (target.limit.min(stack.max_stack) - target.stack.count).max(0);
        let inserted = space.min(stack.count).max(0);
        if !simulate && inserted > 0 {
            if target.stack.is_empty() {
                target.stack = stack.with_count(inserted);
            } else {
                target.stack.count += inserted;
            }
        }
        stack.count - inserted
    }
}

fn recipe(id: &str, ingredients: &[&[&str]]) -> Candidate {
    Candidate::new(
        id,
        ingredients
            .iter()
            .map(|choices| Ingredient {
                accepted: choices.iter().map(|name| (*name).to_owned()).collect(),
            })
            .collect(),
    )
}

fn result<'a>(
    inventory: &'a TestInventory,
    slot_count: i32,
    recipes: &'a [Candidate],
    items: &[Stack],
    context: Option<&[ContextEntry]>,
) -> Option<Plan> {
    plan(
        &Device {
            inventory,
            slot_start: 0,
            slot_count,
            recipes,
        },
        items,
        context,
        |_| true,
    )
}

#[test]
fn uniform_bulk_servings_backtrack_for_overlapping_ingredient_tags() {
    let inv = TestInventory::empty(2);
    let recipes = [recipe(
        "gold_or_iron_then_iron",
        &[&["iron", "gold"], &["iron"]],
    )];
    let plan = result(
        &inv,
        2,
        &recipes,
        &[Stack::new("iron", 2), Stack::new("gold", 2)],
        None,
    )
    .expect("bulk plan");
    // Selecting iron for the first broad ingredient would strand the second.
    assert_eq!(plan.additions()[0], Stack::new("gold", 2));
    assert_eq!(plan.additions()[1], Stack::new("iron", 2));
}

#[test]
fn one_item_per_slot_uses_package_order_before_bulk_layout() {
    let inv = TestInventory::empty(2);
    let recipes = [recipe("two_ingredients", &[&["iron"], &["gold"]])];
    let plan = result(
        &inv,
        2,
        &recipes,
        &[Stack::new("gold", 1), Stack::new("iron", 1)],
        None,
    )
    .expect("one serving");
    // Unlike the bulk branch, Java keeps the original package order here.
    assert_eq!(plan.additions()[0], Stack::new("gold", 1));
    assert_eq!(plan.additions()[1], Stack::new("iron", 1));
}

#[test]
fn contextual_pattern_order_has_priority_over_plain_package_order() {
    let inv = TestInventory::empty(2);
    let recipes = [recipe("mixed", &[&["iron"], &["gold"]])];
    let context = [
        ContextEntry {
            stack: Stack::new("gold", 1),
            count: 1,
        },
        ContextEntry {
            stack: Stack::new("iron", 1),
            count: 1,
        },
    ];
    let plan = result(
        &inv,
        2,
        &recipes,
        &[Stack::new("iron", 2), Stack::new("gold", 2)],
        Some(&context),
    )
    .expect("contextual pattern");
    assert_eq!(plan.additions()[0], Stack::new("gold", 2));
    assert_eq!(plan.additions()[1], Stack::new("iron", 2));
}

#[test]
fn occupied_slots_preserve_positions_and_require_uniform_stored_servings() {
    let mut inv = TestInventory::empty(3);
    inv.slots[2].stack = Stack::new("iron", 2);
    let recipes = [recipe("one_iron", &[&["iron"]])];
    let plan = result(&inv, 3, &recipes, &[Stack::new("iron", 3)], None).expect("occupied slot");
    assert!(plan.additions()[0].is_empty());
    assert!(plan.additions()[1].is_empty());
    assert_eq!(plan.additions()[2], Stack::new("iron", 3));

    inv.slots[1].stack = Stack::new("gold", 3);
    let two = recipe("iron_gold", &[&["iron"], &["gold"]]);
    assert!(
        result(
            &inv,
            3,
            &[two],
            &[Stack::new("gold", 3), Stack::new("iron", 3)],
            None,
        )
        .is_none()
    );
}

#[test]
fn simulated_and_actual_insertion_validate_all_slots_before_writing() {
    let mut inv = TestInventory::empty(2);
    inv.slots[1].limit = 1;
    let recipes = [recipe("iron_gold", &[&["iron"], &["gold"]])];
    let plan = result(
        &inv,
        2,
        &recipes,
        &[Stack::new("iron", 2), Stack::new("gold", 2)],
        None,
    )
    .expect("planner can build layout even when slot is full");
    assert!(!plan.apply(&mut inv, true));
    assert!(!plan.apply(&mut inv, false));
    assert!(inv.slots.iter().all(|slot| slot.stack.is_empty()));

    inv.slots[1].limit = 64;
    assert!(plan.apply(&mut inv, true));
    assert!(inv.slots.iter().all(|slot| slot.stack.is_empty()));
    assert!(plan.apply(&mut inv, false));
    assert_eq!(inv.slots[0].stack, Stack::new("iron", 2));
    assert_eq!(inv.slots[1].stack, Stack::new("gold", 2));
    assert!(!plan.apply(&mut inv, false)); // snapshot is stale now
}

#[test]
fn rejection_of_first_matching_recipe_does_not_select_second() {
    let inv = TestInventory::empty(1);
    let candidates = [
        recipe("disabled", &[&["iron"]]),
        recipe("allowed", &[&["iron"]]),
    ];
    let device = Device {
        inventory: &inv,
        slot_start: 0,
        slot_count: 1,
        recipes: &candidates,
    };
    assert!(
        plan(&device, &[Stack::new("iron", 1)], None, |r| r.id
            != "disabled")
        .is_none()
    );
}

#[test]
fn component_identity_and_integer_overflow_are_fail_closed() {
    let inv = TestInventory::empty(2);
    let recipes = [recipe("one_iron", &[&["iron"]])];
    let mut a = Stack::new("iron", 2);
    a.components = vec![1];
    let mut b = Stack::new("iron", 2);
    b.components = vec![2];
    // These may not be aggregated: they are different saved item stacks.
    assert!(result(&inv, 1, &recipes, &[a, b], None).is_none());
    assert!(
        result(
            &inv,
            1,
            &recipes,
            &[Stack::new("iron", i32::MAX), Stack::new("iron", 1)],
            None,
        )
        .is_none()
    );
}

#[test]
fn bad_context_count_falls_back_but_out_of_range_device_does_not() {
    let inv = TestInventory::empty(1);
    let recipes = [recipe("one_iron", &[&["iron"]])];
    let context = [ContextEntry {
        stack: Stack::new("iron", 1),
        count: -1,
    }];
    assert!(result(&inv, 1, &recipes, &[Stack::new("iron", 1)], Some(&context),).is_some());
    let device = Device {
        inventory: &inv,
        slot_start: i32::MAX,
        slot_count: 9,
        recipes: &recipes,
    };
    assert!(plan(&device, &[Stack::new("iron", 1)], None, |_| true).is_none());
}

#[test]
fn empty_placeholders_are_excluded_from_recipe_ingredient_count() {
    let inv = TestInventory::empty(1);
    let recipes = [Candidate::new(
        "keg",
        vec![
            Ingredient { accepted: vec![] },
            Ingredient {
                accepted: vec!["iron".to_owned()],
            },
            Ingredient { accepted: vec![] },
        ],
    )];
    assert_eq!(recipes[0].ingredients.len(), 1);
    assert!(result(&inv, 1, &recipes, &[Stack::new("iron", 1)], None).is_some());
}
