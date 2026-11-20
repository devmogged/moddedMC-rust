//! Fuel timing from SnowmanCoolerBlockEntity and HeatLevel from SnowmanCoolerBlock.
//! Source: iglee42/create-more-recipes, branch 1.21.1, commit 79cc098.
//! No game integration here; fuel items, server NBT and basin hooks belong in the port.

pub const MAX_HEAT_CAPACITY: u32 = 10_000;
pub const INSERTION_THRESHOLD: u32 = 500;
const REGULAR_FUEL_TICKS: u32 = 1_600;
const SPECIAL_FUEL_TICKS: u32 = 3_200;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fuel {
    None,
    Normal,
    Special,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Heat {
    Idle,
    Fading,
    Cooling,
    Freezing,
}

impl Heat {
    /// `BasinRecipeMixin.cmr$checkForCooler`: the cooler immediately below the basin
    /// must be at least this hot (cold), independent of Blaze Burner's heat enum.
    pub const fn allows(self, required: Self) -> bool {
        self as u8 >= required as u8
    }
}

/// The server-owned fields of `SnowmanCoolerBlockEntity`. Client-only animation,
/// particles, hats and goggles do not affect the fuel/heat transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cooler {
    pub fuel: Fuel,
    pub remaining: u32,
    pub creative: bool,
    creative_heat: Heat,
}

impl Default for Cooler {
    fn default() -> Self {
        Self {
            fuel: Fuel::None,
            remaining: 0,
            creative: false,
            creative_heat: Heat::Idle,
        }
    }
}

impl Cooler {
    pub fn heat(self) -> Heat {
        if self.creative {
            return self.creative_heat;
        }
        match self.fuel {
            Fuel::None => Heat::Idle,
            Fuel::Special => Heat::Freezing,
            Fuel::Normal if (f64::from(self.remaining) / f64::from(MAX_HEAT_CAPACITY)) < 0.0125 => {
                Heat::Fading
            }
            Fuel::Normal => Heat::Cooling,
        }
    }

    /// Returns whether a tagged fuel item is accepted. `simulate` never mutates.
    /// The caller resolves the regular/special item tags and consumes one item on success.
    pub fn refuel(&mut self, offered: Fuel, force_overflow: bool, simulate: bool) -> bool {
        if self.creative || offered == Fuel::None || offered < self.fuel {
            return false;
        }

        let mut burn_time = match offered {
            Fuel::Normal => REGULAR_FUEL_TICKS,
            Fuel::Special => SPECIAL_FUEL_TICKS,
            Fuel::None => unreachable!(),
        };

        if offered == self.fuel {
            if self.remaining <= INSERTION_THRESHOLD {
                burn_time += self.remaining;
            } else if force_overflow && offered == Fuel::Normal {
                burn_time = self
                    .remaining
                    .saturating_add(burn_time)
                    .min(MAX_HEAT_CAPACITY);
            } else {
                return false;
            }
        }

        if !simulate {
            self.fuel = offered;
            self.remaining = burn_time;
        }
        true
    }

    /// The Java tick decrements one tick, then converts exhausted special fuel
    /// into 5,000 ticks of normal fuel, or clears exhausted normal fuel.
    pub fn tick(&mut self) {
        if self.creative {
            return;
        }
        if self.remaining > 0 {
            self.remaining -= 1;
        }
        if self.remaining > 0 {
            return;
        }
        match self.fuel {
            Fuel::Special => {
                self.fuel = Fuel::Normal;
                self.remaining = MAX_HEAT_CAPACITY / 2;
            }
            _ => self.fuel = Fuel::None,
        }
    }

    /// `applyCreativeFuel` cycles idle -> cooling -> freezing -> idle;
    /// it skips fading and disables fuel countdown as in the original.
    pub fn apply_creative_fuel(&mut self) {
        let next = match self.heat() {
            Heat::Idle | Heat::Fading => Heat::Cooling,
            Heat::Cooling => Heat::Freezing,
            Heat::Freezing => Heat::Idle,
        };
        self.fuel = Fuel::None;
        self.remaining = 0;
        self.creative = true;
        self.creative_heat = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regular_fuel_reaches_fading_then_goes_idle() {
        let mut cooler = Cooler::default();
        assert!(cooler.refuel(Fuel::Normal, false, false));
        assert_eq!(cooler.heat(), Heat::Cooling);
        for _ in 0..(REGULAR_FUEL_TICKS - 125) {
            cooler.tick();
        }
        assert_eq!(cooler.remaining, 125);
        assert_eq!(cooler.heat(), Heat::Cooling);
        cooler.tick();
        assert_eq!(cooler.heat(), Heat::Fading);
        for _ in 0..124 {
            cooler.tick();
        }
        assert_eq!(cooler.remaining, 0);
        assert_eq!(cooler.fuel, Fuel::None);
        assert_eq!(cooler.heat(), Heat::Idle);
    }

    #[test]
    fn special_fuel_falls_back_to_5000_regular_ticks() {
        let mut cooler = Cooler::default();
        assert!(cooler.refuel(Fuel::Special, false, false));
        assert_eq!(cooler.heat(), Heat::Freezing);
        for _ in 0..SPECIAL_FUEL_TICKS - 1 {
            cooler.tick();
        }
        assert_eq!(cooler.fuel, Fuel::Special);
        cooler.tick();
        assert_eq!(cooler.fuel, Fuel::Normal);
        assert_eq!(cooler.remaining, MAX_HEAT_CAPACITY / 2);
        assert_eq!(cooler.heat(), Heat::Cooling);
    }

    #[test]
    fn refuel_respects_tier_threshold_and_simulation() {
        let mut cooler = Cooler::default();
        assert!(cooler.refuel(Fuel::Normal, false, false));
        let original = cooler;
        assert!(!cooler.refuel(Fuel::Normal, false, false));
        assert!(cooler.refuel(Fuel::Normal, true, true));
        assert_eq!(cooler, original);
        assert!(cooler.refuel(Fuel::Normal, true, false));
        assert_eq!(cooler.remaining, 3_200);
        assert!(cooler.refuel(Fuel::Special, false, false));
        assert_eq!(cooler.remaining, SPECIAL_FUEL_TICKS);
        assert!(!cooler.refuel(Fuel::Normal, true, false));
        assert!(!cooler.refuel(Fuel::Special, true, false));
        for _ in 0..(SPECIAL_FUEL_TICKS - INSERTION_THRESHOLD) {
            cooler.tick();
        }
        assert_eq!(cooler.remaining, INSERTION_THRESHOLD);
        assert!(cooler.refuel(Fuel::Special, false, false));
        assert_eq!(cooler.remaining, SPECIAL_FUEL_TICKS + INSERTION_THRESHOLD);
    }

    #[test]
    fn requirements_follow_basin_heat_gate() {
        assert!(!Heat::Idle.allows(Heat::Cooling));
        assert!(!Heat::Fading.allows(Heat::Cooling));
        assert!(Heat::Cooling.allows(Heat::Cooling));
        assert!(!Heat::Cooling.allows(Heat::Freezing));
        assert!(Heat::Freezing.allows(Heat::Cooling));
        assert!(Heat::Freezing.allows(Heat::Freezing));
    }

    #[test]
    fn creative_fuel_skips_fading_and_never_counts_down() {
        let mut cooler = Cooler::default();
        cooler.apply_creative_fuel();
        assert_eq!(cooler.heat(), Heat::Cooling);
        for _ in 0..10_000 {
            cooler.tick();
        }
        assert_eq!(cooler.heat(), Heat::Cooling);
        cooler.apply_creative_fuel();
        assert_eq!(cooler.heat(), Heat::Freezing);
        cooler.apply_creative_fuel();
        assert_eq!(cooler.heat(), Heat::Idle);
    }
}
