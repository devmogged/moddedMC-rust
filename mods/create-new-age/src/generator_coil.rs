//! GeneratorCoilBlockEntity and RelativeBlockPos from New Age 1.2.0.
//! Verified against the published 1.2.0 NeoForge jar (javap) and 1.21.1 source.
//! Rendering, world tag lookup and kinetic network callbacks are not included.

/// Java `Direction.Axis` as used by `RelativeBlockPos`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Axis {
    X,
    Y,
    Z,
}

/// RelativeBlockPos.up / right: the plane perpendicular to the shaft axis.
/// Match Java's `Axis.choose` values rather than assuming all coils face north.
fn axes(axis: Axis) -> ([i32; 3], [i32; 3]) {
    match axis {
        Axis::X => ([0, 1, 0], [0, 0, 1]),
        Axis::Y => ([1, 0, 0], [0, 0, -1]),
        Axis::Z => ([0, 1, 0], [-1, 0, 0]),
    }
}

/// The twelve positions from the original constructor's magnetPositions list,
/// relative to the centre coil. Order is meaningful for tag resolution.
pub fn magnet_offsets(axis: Axis) -> [[i32; 3]; 12] {
    let (up, right) = axes(axis);
    let coords = [
        (2, 1),
        (2, 0),
        (2, -1),
        (1, -2),
        (0, -2),
        (-1, -2),
        (-2, -1),
        (-2, 0),
        (-2, 1),
        (-1, 2),
        (0, 2),
        (1, 2),
    ];
    coords.map(|(u, r)| {
        [
            up[0] * u + right[0] * r,
            up[1] * u + right[1] * r,
            up[2] * u + right[2] * r,
        ]
    })
}

/// `GeneratorCoilBlockEntity.calculateStressApplied` adds the magnetic
/// strengths of the matching twelve blocks to plain stress. Callers must
/// resolve `CNATags.Block.MAGNET` and the block's magnetic strength using
/// the resource tag API, then pass zero for other positions.
pub fn stress_with_magnets(plain_stress: f32, strengths: [f32; 12]) -> f32 {
    strengths.into_iter().fold(plain_stress, |sum, n| sum + n)
}

/// Java `GeneratorCoilBlockEntity.tick()`: the original multiplies floats
/// for stress difference and speed, then multiplies the double config value,
/// then truncates to an int.
pub fn generate_energy(
    plain_stress: f32,
    last_stress_applied: f32,
    signed_rpm: f32,
    su_to_energy: f64,
) -> i32 {
    let stress_delta = last_stress_applied - plain_stress;
    let su_per_tick = stress_delta * signed_rpm.abs();
    (f64::from(su_per_tick) * su_to_energy) as i32
}

/// `takeGeneratedEnergy` returns the most recently computed amount and
/// clears the buffer so a second consumer cannot take it twice.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeneratorCoil {
    generated_energy: i32,
}

impl GeneratorCoil {
    pub fn tick(
        &mut self,
        plain_stress: f32,
        last_stress_applied: f32,
        speed: f32,
        su_to_energy: f64,
    ) {
        self.generated_energy =
            generate_energy(plain_stress, last_stress_applied, speed, su_to_energy);
    }

    pub fn take_generated_energy(&mut self) -> i32 {
        std::mem::take(&mut self.generated_energy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnet_positions_keep_original_order_for_all_axes() {
        let x = magnet_offsets(Axis::X);
        assert_eq!(x[0], [0, 2, 1]);
        assert_eq!(x[1], [0, 2, 0]);
        assert_eq!(x[3], [0, 1, -2]);
        assert_eq!(x[6], [0, -2, -1]);
        assert_eq!(x[11], [0, 1, 2]);

        let y = magnet_offsets(Axis::Y);
        assert_eq!(y[0], [2, 0, -1]);
        assert_eq!(y[3], [1, 0, 2]);
        assert_eq!(y[6], [-2, 0, 1]);
        assert_eq!(y[11], [1, 0, -2]);

        let z = magnet_offsets(Axis::Z);
        assert_eq!(z[0], [-1, 2, 0]);
        assert_eq!(z[3], [2, 1, 0]);
        assert_eq!(z[6], [1, -2, 0]);
        assert_eq!(z[11], [-2, 1, 0]);
    }

    #[test]
    fn magnet_strength_changes_stress_and_energy_at_default_conversion() {
        let mut strengths = [0.0; 12];
        strengths[0] = 8.0;
        strengths[5] = 4.0;
        assert_eq!(stress_with_magnets(2.0, strengths), 14.0);
        // Default suToEnergy=0.029296875 in the 1.21.1 ServerConfig.
        assert_eq!(generate_energy(2.0, 14.0, 128.0, 0.029296875), 45);
        assert_eq!(generate_energy(2.0, 14.0, -128.0, 0.029296875), 45);
        assert_eq!(generate_energy(2.0, 14.0, 0.0, 0.029296875), 0);
    }

    #[test]
    fn generated_buffer_can_be_taken_only_once_until_next_tick() {
        let mut coil = GeneratorCoil::default();
        coil.tick(2.0, 14.0, 128.0, 0.029296875);
        assert_eq!(coil.take_generated_energy(), 45);
        assert_eq!(coil.take_generated_energy(), 0);
        coil.tick(2.0, 22.0, 128.0, 0.029296875);
        assert_eq!(coil.take_generated_energy(), 75);
    }
}
