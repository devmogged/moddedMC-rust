//! Server-side corpse access and lifecycle decisions.
//!
//! Pure rules, kept independent of the host until a death/drop and corpse-entity
//! interface is available. Values are server ticks; they do not advance while
//! the corpse's chunk is unloaded.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessPolicy {
    pub only_owner: bool,
    pub skeleton_public: bool,
}

impl AccessPolicy {
    /// A server administrator can always open a corpse; the skeleton exception
    /// only applies when owner-only access was configured.
    pub fn permits(self, is_owner: bool, is_admin: bool, is_skeleton: bool) -> bool {
        !self.only_owner || is_owner || is_admin || (self.skeleton_public && is_skeleton)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifetimePolicy {
    /// Start showing skeleton appearance at this age (0 means immediately).
    pub skeleton_after: u32,
    /// Remove an emptied corpse after this many ticks spent empty.
    pub empty_despawn_after: u32,
    /// Force removal of a full corpse, if positive. None means never.
    pub force_despawn_after: Option<u32>,
}

impl Default for LifetimePolicy {
    fn default() -> Self {
        Self {
            skeleton_after: 20 * 60 * 60,
            empty_despawn_after: 20 * 30,
            force_despawn_after: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Lifetime {
    pub age_ticks: u32,
    /// Original empty timer persists once started, even if items are reinserted.
    pub first_empty_tick: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TickResult {
    pub is_skeleton: bool,
    /// The caller must scatter remaining items before discarding an entity.
    pub remove: bool,
}

impl Lifetime {
    pub fn tick(&mut self, is_empty: bool, policy: LifetimePolicy) -> TickResult {
        self.age_ticks = self.age_ticks.saturating_add(1);
        let is_skeleton = self.age_ticks >= policy.skeleton_after;
        // A zero threshold is not a request to delete corpses immediately.
        let force = policy
            .force_despawn_after
            .is_some_and(|limit| limit > 0 && self.age_ticks > limit);
        if force {
            return TickResult {
                is_skeleton,
                remove: true,
            };
        }
        if is_empty && self.first_empty_tick.is_none() {
            self.first_empty_tick = Some(self.age_ticks);
        }
        let timed_out = is_empty
            && self.first_empty_tick.is_some_and(|start| {
                self.age_ticks.saturating_sub(start) >= policy.empty_despawn_after
            });
        TickResult {
            is_skeleton,
            remove: timed_out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_by_default() {
        assert!(
            AccessPolicy {
                only_owner: false,
                skeleton_public: false
            }
            .permits(false, false, false)
        );
    }

    #[test]
    fn owner_and_admin_bypass_access_restriction() {
        let p = AccessPolicy {
            only_owner: true,
            skeleton_public: false,
        };
        assert!(p.permits(true, false, false));
        assert!(p.permits(false, true, false));
        assert!(!p.permits(false, false, false));
        assert!(!p.permits(false, false, true));
    }

    #[test]
    fn skeleton_access_requires_both_options() {
        let p = AccessPolicy {
            only_owner: true,
            skeleton_public: true,
        };
        assert!(!p.permits(false, false, false));
        assert!(p.permits(false, false, true));
    }

    #[test]
    fn skeleton_transition_is_on_threshold_tick() {
        let mut state = Lifetime::default();
        let policy = LifetimePolicy {
            skeleton_after: 2,
            ..Default::default()
        };
        assert!(!state.tick(false, policy).is_skeleton);
        assert!(state.tick(false, policy).is_skeleton);
        assert_eq!(state.age_ticks, 2);
    }

    #[test]
    fn empty_age_is_recorded_on_first_empty_tick() {
        let mut state = Lifetime::default();
        let policy = LifetimePolicy {
            empty_despawn_after: 2,
            ..Default::default()
        };
        assert!(!state.tick(true, policy).remove);
        assert_eq!(state.first_empty_tick, Some(1));
        assert!(!state.tick(true, policy).remove);
        assert!(state.tick(true, policy).remove);
    }

    #[test]
    fn full_corpse_does_not_time_out_without_force_setting() {
        let mut state = Lifetime::default();
        let policy = LifetimePolicy {
            empty_despawn_after: 1,
            ..Default::default()
        };
        for _ in 0..32 {
            assert!(!state.tick(false, policy).remove);
        }
        assert_eq!(state.first_empty_tick, None);
    }

    #[test]
    fn forced_despawn_uses_strictly_greater_than_threshold() {
        let mut state = Lifetime::default();
        let policy = LifetimePolicy {
            force_despawn_after: Some(2),
            ..Default::default()
        };
        assert!(!state.tick(false, policy).remove);
        assert!(!state.tick(false, policy).remove);
        assert!(state.tick(false, policy).remove);
    }

    #[test]
    fn zero_force_threshold_is_disabled() {
        let mut state = Lifetime::default();
        let policy = LifetimePolicy {
            force_despawn_after: Some(0),
            ..Default::default()
        };
        for _ in 0..32 {
            assert!(!state.tick(false, policy).remove);
        }
    }

    #[test]
    fn refilling_does_not_reset_first_empty_tick() {
        let mut state = Lifetime::default();
        let policy = LifetimePolicy {
            empty_despawn_after: 2,
            ..Default::default()
        };
        state.tick(true, policy);
        state.tick(false, policy);
        assert_eq!(state.first_empty_tick, Some(1));
        assert!(state.tick(true, policy).remove);
    }

    #[test]
    fn persisted_age_and_first_empty_age_continue_after_reload() {
        let policy = LifetimePolicy {
            empty_despawn_after: 3,
            ..Default::default()
        };
        let mut state = Lifetime {
            age_ticks: 100,
            first_empty_tick: Some(99),
        };
        assert!(!state.tick(true, policy).remove);
        assert!(state.tick(true, policy).remove);
    }
}
