//! Source-faithful server-side timing and belt callback decisions for
//! `autosmithing/SmithingBehaviour.java` and `SmithingCallbacks.java`.
//! Create More Recipes 1.2.1-fix1, branch 1.21.1 at 79cc098.
//!
//! In-world execution awaits the Create belt, smithing recipe and kinetics
//! services. This module does not claim a working smithing press.

use serde::{Deserialize, Serialize};

pub const CYCLE: i32 = 240;
pub const ENTITY_SCAN: i32 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SmithingPress {
    pub running: bool,
    pub finished: bool,
    pub ticks: i32,
    pub previous_ticks: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TickEffect {
    /// Original Java emits its press activation sound only at exactly half
    /// the cycle and when speed is nonzero.
    pub activate_sound: bool,
    /// The process completes on a later tick when `ticks > CYCLE`.
    pub completed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeltDecision {
    Pass,
    Hold,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldDecision {
    pub action: BeltDecision,
    /// True means the source's `tryProcessOnBelt` succeeded and the caller
    /// must apply the result, consume template/addition and replace one belt item.
    pub processed: bool,
}

/// Only the vanilla Java keys from `SmithingBehaviour.write/read`; the
/// template and addition inventories are serialized separately by the owning
/// SmithingPressBlockEntity.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SmithingSave {
    #[serde(rename = "Running", default)]
    pub running: bool,
    #[serde(rename = "Finished", default)]
    pub finished: bool,
    #[serde(rename = "Ticks", default)]
    pub ticks: i32,
}

/// Counts from the SmithingPressBlockEntity's separate one-slot template and
/// addition inventories. Vanilla's smithing recipe manager resolves and
/// assembles the result *before* these slots are consumed. The belt itself is
/// decremented by SmithingCallbacks, not by tryProcessOnBelt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SmithingInputs {
    pub templates: u32,
    pub additions: u32,
}

impl SmithingInputs {
    /// `tryProcessOnBelt`: `recipe_output_present` is the result of the
    /// actual Vanilla SmithingRecipe. Simulation does not consume inputs.
    /// A missing recipe or empty output must consume nothing.
    pub fn process(&mut self, recipe_output_present: bool, simulate: bool) -> bool {
        if !recipe_output_present {
            return false;
        }
        if !simulate {
            self.templates = self.templates.saturating_sub(1);
            self.additions = self.additions.saturating_sub(1);
        }
        true
    }
}

impl SmithingPress {
    pub fn save(self) -> SmithingSave {
        SmithingSave {
            running: self.running,
            finished: self.finished,
            ticks: self.ticks,
        }
    }

    pub fn load(saved: SmithingSave) -> Self {
        Self {
            running: saved.running,
            finished: saved.finished,
            ticks: saved.ticks,
            previous_ticks: saved.ticks,
        }
    }

    /// `SmithingBehaviour.start()` does NOT clear `finished`. Preserve
    /// that source behavior; it may be cleared by other Create callbacks.
    pub fn start(&mut self) {
        self.running = true;
        self.previous_ticks = 0;
        self.ticks = 0;
    }

    /// Source Java: `(int)Mth.lerp(clamp(abs(speed)/512f,0,1),1,60)`.
    /// The float operation is intentional; Java casts the result to an int.
    pub fn running_tick_speed(rpm: f32) -> i32 {
        if rpm == 0.0 {
            return 0;
        }
        let t = (rpm.abs() / 512.0_f32).clamp(0.0, 1.0);
        (1.0_f32 + t * 59.0_f32) as i32
    }

    /// Java `SmithingCallbacks.onItemReceived`: always HOLD an incoming
    /// belt item while a press is already running, even if it doesn't match.
    pub fn on_item_received(&mut self, rpm: f32, can_smith: bool) -> BeltDecision {
        if rpm == 0.0 {
            return BeltDecision::Pass;
        }
        if self.running {
            return BeltDecision::Hold;
        }
        if !can_smith {
            return BeltDecision::Pass;
        }
        self.start();
        BeltDecision::Hold
    }

    /// Java `SmithingCallbacks.whenItemHeld` processes at exactly half the
    /// cycle. If the smithing recipe no longer matches, it releases the item.
    pub fn while_item_held(&self, rpm: f32, can_smith: bool) -> HeldDecision {
        if rpm == 0.0 || !self.running {
            return HeldDecision {
                action: BeltDecision::Pass,
                processed: false,
            };
        }
        if self.ticks != CYCLE / 2 {
            return HeldDecision {
                action: BeltDecision::Hold,
                processed: false,
            };
        }
        HeldDecision {
            action: if can_smith {
                BeltDecision::Hold
            } else {
                BeltDecision::Pass
            },
            processed: can_smith,
        }
    }

    /// One server-side Java `SmithingBehaviour.tick`. Speed zero pauses the
    /// counter, but the press remains running. Half-cycle clamps to 120 even
    /// when the speed step would overshoot it. Completion is checked *before*
    /// incrementing so 240 is not yet a completed cycle.
    pub fn tick(&mut self, rpm: f32) -> TickEffect {
        if !self.running {
            return TickEffect::default();
        }
        let mut effect = TickEffect {
            activate_sound: self.ticks == CYCLE / 2 && rpm != 0.0,
            completed: false,
        };
        if self.ticks > CYCLE {
            self.running = false;
            self.finished = true;
            effect.completed = true;
            return effect;
        }
        self.previous_ticks = self.ticks;
        self.ticks += Self::running_tick_speed(rpm);
        if self.previous_ticks < CYCLE / 2 && self.ticks >= CYCLE / 2 {
            self.ticks = CYCLE / 2;
        }
        effect
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastnbt::Value;

    #[test]
    fn running_speed_is_rpm_scaled_and_truncated_like_java() {
        assert_eq!(SmithingPress::running_tick_speed(0.0), 0);
        assert_eq!(SmithingPress::running_tick_speed(1.0), 1);
        assert_eq!(SmithingPress::running_tick_speed(16.0), 2);
        assert_eq!(SmithingPress::running_tick_speed(-16.0), 2);
        assert_eq!(SmithingPress::running_tick_speed(256.0), 30);
        assert_eq!(SmithingPress::running_tick_speed(512.0), 60);
        assert_eq!(SmithingPress::running_tick_speed(-1024.0), 60);
    }

    #[test]
    fn belt_item_is_held_only_when_source_would_start_or_continue() {
        let mut press = SmithingPress::default();
        assert_eq!(press.on_item_received(0.0, true), BeltDecision::Pass);
        assert_eq!(press.on_item_received(16.0, false), BeltDecision::Pass);
        assert_eq!(press.on_item_received(16.0, true), BeltDecision::Hold);
        assert_eq!(press.ticks, 0);
        assert!(press.running);
        assert_eq!(press.on_item_received(16.0, false), BeltDecision::Hold);
        assert_eq!(press.while_item_held(16.0, true).action, BeltDecision::Hold);
        assert!(!press.while_item_held(16.0, true).processed);
    }

    #[test]
    fn press_half_cycle_is_clamped_and_completion_occurs_after_240() {
        let mut press = SmithingPress::default();
        press.start();
        assert_eq!(press.tick(512.0), TickEffect::default());
        assert_eq!(press.ticks, 60);
        assert_eq!(press.tick(512.0), TickEffect::default());
        assert_eq!(press.ticks, 120);
        assert!(press.while_item_held(512.0, true).processed);
        assert_eq!(
            press.while_item_held(512.0, false).action,
            BeltDecision::Pass
        );
        assert!(press.tick(512.0).activate_sound);
        assert_eq!(press.ticks, 180);
        assert!(!press.tick(512.0).completed);
        assert_eq!(press.ticks, 240);
        assert!(!press.tick(512.0).completed);
        assert_eq!(press.ticks, 300);
        assert!(press.tick(512.0).completed);
        assert!(press.finished);
        assert!(!press.running);
        assert_eq!(press.ticks, 300);
        assert_eq!(press.tick(512.0), TickEffect::default());
    }

    #[test]
    fn zero_speed_pauses_a_running_press_and_negative_rpm_restarts_time() {
        let mut press = SmithingPress::default();
        press.start();
        assert!(!press.tick(-256.0).completed);
        assert_eq!(press.ticks, 30);
        for _ in 0..10 {
            assert_eq!(press.tick(0.0), TickEffect::default());
        }
        assert!(press.running);
        assert_eq!(press.ticks, 30);
        press.tick(-256.0);
        assert_eq!(press.ticks, 60);
        assert_eq!(press.while_item_held(0.0, true).action, BeltDecision::Pass);
    }

    #[test]
    fn save_restore_uses_java_running_finished_ticks_fields_only() {
        let mut press = SmithingPress::default();
        press.start();
        press.tick(256.0);
        let bytes = fastnbt::to_bytes(&press.save()).expect("nbt");
        let Value::Compound(compound) = fastnbt::from_bytes::<Value>(&bytes).expect("nbt") else {
            panic!("expected NBT compound");
        };
        assert_eq!(compound.len(), 3);
        assert_eq!(compound.get("Running"), Some(&Value::Byte(1)));
        assert_eq!(compound.get("Finished"), Some(&Value::Byte(0)));
        assert_eq!(compound.get("Ticks"), Some(&Value::Int(30)));
        let saved: SmithingSave = fastnbt::from_bytes(&bytes).expect("NBT");
        let loaded = SmithingPress::load(saved);
        assert_eq!(loaded.previous_ticks, loaded.ticks);
        assert_eq!(loaded.ticks, 30);
        assert!(loaded.running);
        assert!(!loaded.finished);
    }

    #[test]
    fn smithing_input_simulation_and_failure_conserve_items() {
        let mut inputs = SmithingInputs {
            templates: 2,
            additions: 3,
        };
        assert!(!inputs.process(false, false));
        assert!(inputs.process(true, true));
        assert_eq!(inputs.templates, 2);
        assert_eq!(inputs.additions, 3);
        assert!(inputs.process(true, false));
        assert_eq!(inputs.templates, 1);
        assert_eq!(inputs.additions, 2);
    }

    #[test]
    fn start_preserves_finished_flag_matching_source() {
        let mut press = SmithingPress {
            finished: true,
            ..Default::default()
        };
        press.start();
        assert!(press.finished);
        assert!(press.running);
        assert_eq!(press.ticks, 0);
    }
}
