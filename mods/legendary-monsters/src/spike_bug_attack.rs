//! Exact standalone attack timing of 2.2.4 `SpikeBugAttackGoal`.
//!
//! Based on the released jar's class bytecode, not the older public source.
//! The host will supply vanilla `adjustedTickDelay(attackDelay * 2)` and
//! apply emitted actions once the required interfaces are available.
//!
//! TODO(abi): #54 — doHurtTarget/damage events.
//! TODO(abi): #65 — custom mob hitbox (0.7 × 0.5).
//! TODO(abi): #67 — vanilla targeting and target visibility.

/// Original `SpikeBugAttackGoal` uses a squared-distance threshold, not
/// an ordinary linear reach calculation.
pub const ATTACK_DISTANCE_SQUARED: f64 = 3.5 * 2.5;
const ATTACK_DELAY: i32 = 8;
pub const BASE_RESET_TICKS: i32 = ATTACK_DELAY * 2;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AttackActions {
    /// The `SpikebugEntity.setAttacking` call, if issued on this check.
    pub set_attacking: Option<bool>,
    /// Original outside-reach branch sets the client's animation timeout to 5.
    pub animation_timeout: Option<i32>,
    /// Caller should look at the target, swing and call doHurtTarget, in order.
    pub strike: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpikeBugAttackGoal {
    attack_delay: i32,
    ticks_until_next_attack: i32,
    should_count_till_next_attack: bool,
    has_played_attack_sound: bool,
}

impl Default for SpikeBugAttackGoal {
    fn default() -> Self {
        Self {
            attack_delay: ATTACK_DELAY,
            ticks_until_next_attack: ATTACK_DELAY,
            should_count_till_next_attack: false,
            has_played_attack_sound: false,
        }
    }
}

impl SpikeBugAttackGoal {
    /// Mirrors `start`: it resets only `attackDelay` and
    /// `ticksUntilNextAttack`, not the other flags.
    pub fn start(&mut self) {
        self.attack_delay = ATTACK_DELAY;
        self.ticks_until_next_attack = ATTACK_DELAY;
    }

    /// Mirrors the override of `checkAndPerformAttack`, called by the
    /// *superclass* portion of `tick`. `adjusted_reset_ticks` is the result
    /// of vanilla `adjustedTickDelay(16)` (not necessarily 16 on all hosts).
    pub fn check_and_perform_attack(
        &mut self,
        squared_distance: f64,
        adjusted_reset_ticks: i32,
    ) -> AttackActions {
        if squared_distance <= ATTACK_DISTANCE_SQUARED {
            self.should_count_till_next_attack = true;
            let mut actions = AttackActions::default();
            if self.ticks_until_next_attack <= self.attack_delay && !self.has_played_attack_sound {
                actions.set_attacking = Some(true);
                self.has_played_attack_sound = true;
            }
            if self.ticks_until_next_attack <= 0 {
                self.has_played_attack_sound = false;
                self.ticks_until_next_attack = adjusted_reset_ticks;
                actions.strike = true;
            }
            actions
        } else {
            self.ticks_until_next_attack = adjusted_reset_ticks;
            self.should_count_till_next_attack = false;
            self.has_played_attack_sound = false;
            AttackActions {
                set_attacking: Some(false),
                animation_timeout: Some(5),
                strike: false,
            }
        }
    }

    /// Call strictly *after* the superclass tick/check. The released Java
    /// class decrements once only when the target has been in reach.
    pub fn after_super_tick(&mut self) {
        if self.should_count_till_next_attack {
            self.ticks_until_next_attack = (self.ticks_until_next_attack - 1).max(0);
        }
    }

    /// Mirrors `stop`: clears the attack sync flag and played-sound flag;
    /// it deliberately leaves the countdown and count flag unchanged.
    pub fn stop(&mut self) -> AttackActions {
        self.has_played_attack_sound = false;
        AttackActions {
            set_attacking: Some(false),
            ..Default::default()
        }
    }

    pub fn remaining_ticks(&self) -> i32 {
        self.ticks_until_next_attack
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_animation_is_started_at_eight_ticks() {
        let mut goal = SpikeBugAttackGoal::default();
        let first = goal.check_and_perform_attack(1.0, 16);
        assert_eq!(first.set_attacking, Some(true));
        assert!(!first.strike);
        goal.after_super_tick();
        assert_eq!(goal.remaining_ticks(), 7);
        assert_eq!(
            goal.check_and_perform_attack(1.0, 16).set_attacking,
            None,
            "animation must not start twice while the swing is winding up"
        );
    }

    #[test]
    fn strike_occurs_after_eight_completed_countdown_ticks() {
        let mut goal = SpikeBugAttackGoal::default();
        for before in (1..=8).rev() {
            assert_eq!(goal.remaining_ticks(), before);
            assert!(!goal.check_and_perform_attack(0.0, 16).strike);
            goal.after_super_tick();
        }
        assert_eq!(goal.remaining_ticks(), 0);
        let action = goal.check_and_perform_attack(0.0, 16);
        assert!(action.strike);
        assert_eq!(goal.remaining_ticks(), 16);
        goal.after_super_tick();
        assert_eq!(goal.remaining_ticks(), 15);
    }

    #[test]
    fn distance_boundary_includes_eight_point_seventy_five() {
        let mut goal = SpikeBugAttackGoal::default();
        assert_eq!(
            goal.check_and_perform_attack(ATTACK_DISTANCE_SQUARED, 16)
                .set_attacking,
            Some(true)
        );
        assert_eq!(
            goal.check_and_perform_attack(
                f64::from_bits(ATTACK_DISTANCE_SQUARED.to_bits() + 1),
                16
            ),
            AttackActions {
                set_attacking: Some(false),
                animation_timeout: Some(5),
                strike: false
            }
        );
    }

    #[test]
    fn out_of_range_resets_timer_and_pauses_countdown() {
        let mut goal = SpikeBugAttackGoal::default();
        goal.check_and_perform_attack(0.0, 16);
        goal.after_super_tick();
        let action = goal.check_and_perform_attack(9.0, 16);
        assert_eq!(action.set_attacking, Some(false));
        assert_eq!(action.animation_timeout, Some(5));
        goal.after_super_tick();
        assert_eq!(goal.remaining_ticks(), 16);
    }

    #[test]
    fn outside_distance_includes_nan() {
        let mut goal = SpikeBugAttackGoal::default();
        assert_eq!(
            goal.check_and_perform_attack(f64::NAN, 16).set_attacking,
            Some(false)
        );
    }

    #[test]
    fn uses_host_adjusted_delay_not_a_hardcoded_cooldown() {
        let mut goal = SpikeBugAttackGoal::default();
        let outside = goal.check_and_perform_attack(f64::INFINITY, 9);
        assert_eq!(outside.animation_timeout, Some(5));
        assert_eq!(goal.remaining_ticks(), 9);
        for _ in 0..9 {
            goal.check_and_perform_attack(0.0, 9);
            goal.after_super_tick();
        }
        assert!(goal.check_and_perform_attack(0.0, 9).strike);
        assert_eq!(goal.remaining_ticks(), 9);
    }

    #[test]
    fn stop_does_not_reinitialize_cooldown() {
        let mut goal = SpikeBugAttackGoal::default();
        goal.check_and_perform_attack(0.0, 16);
        goal.after_super_tick();
        assert_eq!(goal.remaining_ticks(), 7);
        assert_eq!(goal.stop().set_attacking, Some(false));
        assert_eq!(goal.remaining_ticks(), 7);
        goal.start();
        assert_eq!(goal.remaining_ticks(), 8);
    }
}
