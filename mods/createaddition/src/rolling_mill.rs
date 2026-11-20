//! Pure Rolling Mill processing state from Create Crafts & Additions
//! `neoforge-1.21.1-1.7.2`: RollingMillBlockEntity.java.
//! Kinetic RPM, recipe lookups and belt/item IO require host integration.

use serde::{Deserialize, Serialize};

pub const ROLLING_DURATION: i32 = 120;
pub const MISSING_RECIPE_RETRY: i32 = 100;
pub const OUTPUT_SLOTS: usize = 9;
pub const OUTPUT_SLOT_LIMIT: u32 = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollingRecipe {
    pub input_tag: &'static str,
    pub output: &'static str,
    pub output_count: u32,
}

pub const COPPER_ROD: RollingRecipe = RollingRecipe {
    input_tag: "c:ingots/copper",
    output: "createaddition:copper_rod",
    output_count: 2,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputSlot {
    pub item: Option<&'static str>,
    pub count: u32,
}

impl OutputSlot {
    pub fn is_full(self) -> bool {
        self.count == OUTPUT_SLOT_LIMIT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollingMill {
    pub timer: i32,
    pub input_count: u32,
    pub output: [OutputSlot; OUTPUT_SLOTS],
    /// `lastRecipe` is a Java in-memory cache. The original only saves
    /// `Timer` and both inventory NBT compounds, never the cached recipe.
    pub cached_recipe: Option<usize>,
}

impl Default for RollingMill {
    fn default() -> Self {
        Self {
            timer: 0,
            input_count: 0,
            output: [OutputSlot::default(); OUTPUT_SLOTS],
            cached_recipe: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickResult {
    pub timer_started: bool,
    pub no_recipe: bool,
    pub processed: bool,
    pub inserted: u32,
    /// `ItemHandlerHelper.insertItemStacked` returns remainder which the
    /// source code ignores. Keep this visible for future oracle testing.
    pub remainder: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollingTimerSave {
    #[serde(rename = "Timer", default)]
    pub timer: i32,
}

impl RollingMill {
    /// Java `Mth.clamp((int)Math.abs(getSpeed()/16f),1,512)`.
    pub fn processing_speed(rpm: f32) -> i32 {
        ((rpm.abs() / 16.0) as i32).clamp(1, 512)
    }

    pub fn save(self) -> RollingTimerSave {
        RollingTimerSave { timer: self.timer }
    }

    pub fn restore_timer(&mut self, saved: RollingTimerSave) {
        self.timer = saved.timer;
        self.cached_recipe = None;
    }

    /// Model the original nine-slot `ItemHandlerHelper.insertItemStacked`.
    /// Matching existing stacks are filled first, then free slots.
    fn insert_output(&mut self, item: &'static str, mut remaining: u32) -> u32 {
        let original = remaining;
        for slot in &mut self.output {
            if slot.item == Some(item) && slot.count < OUTPUT_SLOT_LIMIT {
                let accepted = (OUTPUT_SLOT_LIMIT - slot.count).min(remaining);
                slot.count += accepted;
                remaining -= accepted;
            }
        }
        for slot in &mut self.output {
            if slot.item.is_none() && remaining > 0 {
                let accepted = remaining.min(OUTPUT_SLOT_LIMIT);
                slot.item = Some(item);
                slot.count = accepted;
                remaining -= accepted;
            }
        }
        original - remaining
    }

    /// One server tick. `matched_recipe` is the recipe chosen by the host's
    /// current `createaddition:rolling` recipe manager for the current input.
    /// A None value faithfully represents a mismatch; the cache is only valid
    /// when it still matches the input. No source recipe is guessed.
    pub fn tick(
        &mut self,
        rpm: f32,
        matched_recipe: Option<usize>,
        recipes: &[RollingRecipe],
        processing_duration: i32,
    ) -> TickResult {
        let mut result = TickResult::default();
        if rpm == 0.0 || self.output.iter().any(|slot| slot.is_full()) {
            return result;
        }

        if self.timer > 0 {
            self.timer -= Self::processing_speed(rpm);
            if self.timer <= 0
                && self.input_count > 0
                && let Some(recipe) = matched_recipe.and_then(|index| recipes.get(index))
            {
                // process() checks the recipe again at completion.
                self.input_count -= 1;
                let accepted = self.insert_output(recipe.output, recipe.output_count);
                result.processed = true;
                result.inserted = accepted;
                result.remainder = recipe.output_count - accepted;
                self.cached_recipe = matched_recipe;
            }
            return result;
        }

        if self.input_count == 0 {
            return result;
        }

        if matched_recipe
            .and_then(|index| recipes.get(index))
            .is_some()
        {
            self.cached_recipe = matched_recipe;
            self.timer = processing_duration;
        } else {
            self.cached_recipe = None;
            self.timer = MISSING_RECIPE_RETRY;
            result.no_recipe = true;
        }
        result.timer_started = true;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;

    #[test]
    fn published_copper_recipe_matches_ported_output() {
        let fixture = include_str!("../../../tests/fixtures/ws4-createaddition/copper_ingot.json");
        let data: serde_json::Value = serde_json::from_str(fixture).expect("JSON");
        assert_eq!(data["type"], "createaddition:rolling");
        assert_eq!(data["ingredients"][0]["tag"], COPPER_ROD.input_tag);
        assert_eq!(data["results"][0]["id"], COPPER_ROD.output);
        assert_eq!(data["results"][0]["count"], COPPER_ROD.output_count);
    }

    #[test]
    fn rpm_processing_speed_matches_published_java_bytecode() {
        assert_eq!(RollingMill::processing_speed(0.0), 1);
        assert_eq!(RollingMill::processing_speed(1.0), 1);
        assert_eq!(RollingMill::processing_speed(16.0), 1);
        assert_eq!(RollingMill::processing_speed(-32.0), 2);
        assert_eq!(RollingMill::processing_speed(256.0), 16);
        assert_eq!(RollingMill::processing_speed(8_192.0), 512);
        assert_eq!(RollingMill::processing_speed(16_384.0), 512);
    }

    #[test]
    fn cold_start_and_completion_consume_exactly_one_input() {
        let mut mill = RollingMill {
            input_count: 3,
            ..Default::default()
        };
        let recipes = [COPPER_ROD];
        assert!(
            mill.tick(256.0, Some(0), &recipes, ROLLING_DURATION)
                .timer_started
        );
        assert_eq!(mill.timer, 120);
        for _ in 0..7 {
            assert!(
                !mill
                    .tick(256.0, Some(0), &recipes, ROLLING_DURATION)
                    .processed
            );
        }
        assert_eq!(mill.timer, 8);
        let outcome = mill.tick(256.0, Some(0), &recipes, ROLLING_DURATION);
        assert!(outcome.processed);
        assert_eq!(outcome.inserted, 2);
        assert_eq!(mill.input_count, 2);
        assert_eq!(
            mill.output[0],
            OutputSlot {
                item: Some("createaddition:copper_rod"),
                count: 2
            }
        );
        assert!(
            mill.tick(256.0, Some(0), &recipes, ROLLING_DURATION)
                .timer_started
        );
    }

    #[test]
    fn no_recipe_waits_one_hundred_ticks_before_retry() {
        let mut mill = RollingMill {
            input_count: 1,
            ..Default::default()
        };
        let result = mill.tick(16.0, None, &[COPPER_ROD], ROLLING_DURATION);
        assert!(result.no_recipe && result.timer_started);
        assert_eq!(mill.timer, MISSING_RECIPE_RETRY);
        for _ in 0..100 {
            assert!(
                !mill
                    .tick(16.0, None, &[COPPER_ROD], ROLLING_DURATION)
                    .processed
            );
        }
        assert_eq!(mill.input_count, 1);
        assert!(
            mill.tick(16.0, Some(0), &[COPPER_ROD], ROLLING_DURATION)
                .timer_started
        );
        assert_eq!(mill.timer, ROLLING_DURATION);
    }

    #[test]
    fn zero_rpm_and_a_single_full_output_slot_halt_everything() {
        let mut mill = RollingMill {
            input_count: 2,
            timer: 15,
            ..Default::default()
        };
        assert_eq!(
            mill.tick(0.0, Some(0), &[COPPER_ROD], ROLLING_DURATION),
            TickResult::default()
        );
        assert_eq!(mill.timer, 15);
        mill.output[7] = OutputSlot {
            item: Some("minecraft:stone"),
            count: OUTPUT_SLOT_LIMIT,
        };
        assert_eq!(
            mill.tick(256.0, Some(0), &[COPPER_ROD], ROLLING_DURATION),
            TickResult::default()
        );
        assert_eq!(mill.timer, 15);
        mill.output[7].count = 63;
        mill.tick(256.0, Some(0), &[COPPER_ROD], ROLLING_DURATION);
        assert!(mill.timer <= 0);
    }

    #[test]
    fn output_stacks_then_uses_empty_slot_when_first_nearly_full() {
        let mut mill = RollingMill {
            input_count: 1,
            timer: 1,
            ..Default::default()
        };
        mill.output[0] = OutputSlot {
            item: Some(COPPER_ROD.output),
            count: 63,
        };
        let effect = mill.tick(16.0, Some(0), &[COPPER_ROD], ROLLING_DURATION);
        assert!(effect.processed);
        assert_eq!(effect.inserted, 2);
        assert_eq!(effect.remainder, 0);
        assert_eq!(mill.output[0].count, 64);
        assert_eq!(mill.output[1].count, 1);
    }

    #[test]
    fn revalidation_prevents_input_consumption_after_recipe_mismatch() {
        let mut mill = RollingMill {
            input_count: 2,
            timer: 1,
            ..Default::default()
        };
        assert!(
            !mill
                .tick(16.0, None, &[COPPER_ROD], ROLLING_DURATION)
                .processed
        );
        assert_eq!(mill.input_count, 2);
    }

    #[test]
    fn timer_nbt_round_trip_does_not_persist_recipe_cache() {
        let mut mill = RollingMill {
            input_count: 3,
            timer: 72,
            cached_recipe: Some(0),
            ..Default::default()
        };
        let bytes = fastnbt::to_bytes(&mill.save()).expect("NBT");
        let Value::Compound(fields) = fastnbt::from_bytes::<Value>(&bytes).expect("NBT") else {
            panic!("compound")
        };
        assert_eq!(fields.len(), 1);
        assert_eq!(fields.get("Timer"), Some(&Value::Int(72)));
        let save: RollingTimerSave = fastnbt::from_bytes(&bytes).expect("NBT");
        mill.restore_timer(save);
        assert_eq!(mill.timer, 72);
        assert_eq!(mill.cached_recipe, None);
    }
}
