//! Source: released Create Crafts & Additions 1.7.2 NeoForge 1.21.1 jar.
//! Compiled Java methods checked with javap: ElectricMotorBlockEntity
//! `getEnergyConsumptionRate` and `tick`; AlternatorBlockEntity
//! `getEnergyProductionRate` and `calculateStressApplied`.
//! No claims about host service behavior or networking until integrated.

/// Java uses this divisor for FE-to-rotation and rotation-to-FE conversion.
const MAX_RPM: f64 = 256.0;

/// `ElectricMotorBlockEntity.getEnergyConsumptionRate` with live config values.
/// The Java `(int)` truncates the double after applying its minimum FE/t.
pub fn motor_consumption(rpm: f32, fe_at_max_rpm: i32, minimum_consumption: i32) -> i32 {
    if rpm.abs() <= 0.0 {
        return 0;
    }
    ((f64::from(fe_at_max_rpm) * f64::from(rpm.abs()) / MAX_RPM)
        .max(f64::from(minimum_consumption))) as i32
}

/// `AlternatorBlockEntity.getEnergyProductionRate` with live config.
/// The Java int input is absolute-valued first, then converted to double.
pub fn alternator_production(rpm: i32, fe_at_max_rpm: i32, efficiency: f64) -> i32 {
    let magnitude = rpm.wrapping_abs();
    (f64::from(fe_at_max_rpm) * f64::from(magnitude.wrapping_abs()) / MAX_RPM * efficiency) as i32
}

/// `AlternatorBlockEntity.calculateStressApplied` and
/// `ElectricMotorBlockEntity.calculateAddedStressCapacity`.
pub fn stress_at_max_rpm(max_stress: i32) -> f32 {
    max_stress as f32 / 256.0
}

/// Energy and activation state on a single electric motor. The connected
/// Create kinetic network must update when the returned activity flag changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motor {
    pub energy: i32,
    pub active: bool,
}

impl Motor {
    /// Server-side `ElectricMotorBlockEntity.tick`: inactive motors start
    /// only above twice the current FE/t consumption, while active motors
    /// consume FE before checking redstone or insufficient energy.
    /// Returns true when the motor's generated rotation needs recalculation.
    pub fn tick(&mut self, consumption: i32, powered: bool) -> bool {
        let old = self.active;
        if !self.active {
            if self.energy > consumption.saturating_mul(2) && !powered {
                self.active = true;
            }
        } else {
            let consumed = self.energy.min(consumption).max(0);
            self.energy -= consumed;
            if consumed < consumption || powered {
                self.active = false;
            }
        }
        self.active != old
    }
}

/// The alternator stores its energy before trying transfers to adjacent
/// energy handlers. Sided transfer ownership is outside this pure function.
pub fn alternator_generate(energy: i32, capacity: i32, production: i32) -> i32 {
    energy.saturating_add(production).min(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motor_fe_rate_matches_java_truncation_and_minimum() {
        // CommonConfig default values at the original 1.21.1 release:
        // 480 FE/t at 256 RPM; minimum 8 FE/t.
        assert_eq!(motor_consumption(0.0, 480, 8), 0);
        assert_eq!(motor_consumption(1.0, 480, 8), 8);
        assert_eq!(motor_consumption(-1.0, 480, 8), 8);
        assert_eq!(motor_consumption(16.0, 480, 8), 30);
        assert_eq!(motor_consumption(-64.0, 480, 8), 120);
        assert_eq!(motor_consumption(256.0, 480, 8), 480);
        assert_eq!(motor_consumption(7.0, 480, 0), 13);
    }

    #[test]
    fn alternator_rate_preserves_integer_truncation_and_direction_independence() {
        assert_eq!(alternator_production(0, 480, 0.75), 0);
        assert_eq!(alternator_production(16, 480, 0.75), 22);
        assert_eq!(alternator_production(-16, 480, 0.75), 22);
        assert_eq!(alternator_production(256, 480, 0.75), 360);
        assert_eq!(alternator_production(1, 480, 0.75), 1);
        assert_eq!(stress_at_max_rpm(16_384), 64.0);
    }

    #[test]
    fn motor_starts_above_two_ticks_energy_and_consumes_only_after_start() {
        let mut motor = Motor {
            energy: 60,
            active: false,
        };
        // 16 RPM: 30 FE/t. Exactly 60 is not enough to start.
        assert!(!motor.tick(30, false));
        assert_eq!(motor.energy, 60);
        motor.energy = 61;
        assert!(motor.tick(30, false));
        assert!(motor.active);
        assert_eq!(motor.energy, 61);
        assert!(!motor.tick(30, false));
        assert_eq!(motor.energy, 31);
        assert!(!motor.tick(30, false));
        assert_eq!(motor.energy, 1);
        assert!(motor.tick(30, false));
        assert_eq!(motor.energy, 0);
        assert!(!motor.active);
    }

    #[test]
    fn redstone_stops_a_running_motor_after_its_last_energy_tick() {
        let mut motor = Motor {
            energy: 120,
            active: false,
        };
        assert!(!motor.tick(30, true));
        assert_eq!(motor.energy, 120);
        assert!(motor.tick(30, false));
        assert!(motor.tick(30, true));
        assert_eq!(motor.energy, 90);
        assert!(!motor.active);
        assert_eq!(alternator_generate(4_900, 5_000, 360), 5_000);
        assert_eq!(alternator_generate(100, 5_000, 0), 100);
    }
}
