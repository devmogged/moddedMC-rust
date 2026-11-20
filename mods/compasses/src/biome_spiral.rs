//! Nature's Compass `BiomeSearchWorker` sampling order, without host-world reads.
//! Source: upstream 26.3-3.3.1 `worker/BiomeSearchWorker.java`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Up,
    North,
    East,
    South,
    West,
}

/// Stateful square spiral. The caller must finish each probe before asking for the next.
#[derive(Debug)]
pub struct BiomeSpiral {
    origin: (i32, i32),
    position: (i32, i32),
    step: i32,
    max_radius: i32,
    max_samples: u32,
    samples: u32,
    length: i32,
    next_length: i32,
    direction: Direction,
    pending: bool,
}

impl BiomeSpiral {
    /// `step` and `max_radius` already include the dimension's biome-size multiplier.
    pub fn new(origin: (i32, i32), step: i32, max_radius: i32, max_samples: u32) -> Option<Self> {
        if step <= 0 || max_radius <= 0 {
            return None;
        }
        Some(Self {
            origin,
            position: origin,
            step,
            max_radius,
            max_samples,
            samples: 0,
            length: 0,
            next_length: step,
            direction: Direction::Up,
            pending: false,
        })
    }

    pub const fn samples(&self) -> u32 {
        self.samples
    }

    pub fn radius(&self) -> i32 {
        let dx = f64::from(self.position.0) - f64::from(self.origin.0);
        let dz = f64::from(self.position.1) - f64::from(self.origin.1);
        // Java converts `distSqr` to float before `Mth.sqrt` and truncates to int.
        ((dx * dx + dz * dz) as f32).sqrt() as i32
    }

    pub fn has_work(&self) -> bool {
        self.samples <= self.max_samples && self.radius() <= self.max_radius
    }

    /// Returns a block X/Z coordinate. Caller samples quart X/Z and the configured Y values.
    pub fn next_probe(&mut self) -> Option<(i32, i32)> {
        if self.pending || !self.has_work() {
            return None;
        }
        match self.direction {
            Direction::Up => {}
            Direction::North => self.position.1 = self.position.1.saturating_sub(self.step),
            Direction::East => self.position.0 = self.position.0.saturating_add(self.step),
            Direction::South => self.position.1 = self.position.1.saturating_add(self.step),
            Direction::West => self.position.0 = self.position.0.saturating_sub(self.step),
        }
        self.pending = true;
        Some(self.position)
    }

    /// Equivalent to `finishSample` when no biome is found or a connected candidate is skipped.
    pub fn finish_probe(&mut self) {
        if !self.pending {
            return;
        }
        self.pending = false;
        self.samples += 1;
        self.length += self.step;
        if self.length >= self.next_length {
            match self.direction {
                Direction::Up => self.direction = Direction::North,
                Direction::North => {
                    self.next_length += self.step;
                    self.direction = Direction::East;
                }
                Direction::East => {
                    self.next_length += self.step;
                    self.direction = Direction::South;
                }
                Direction::South => {
                    self.next_length += self.step;
                    self.direction = Direction::West;
                }
                Direction::West => {
                    self.next_length += self.step;
                    self.direction = Direction::North;
                }
            }
            self.length = 0;
        }
    }

    /// Negative block coordinates must round down on conversion to quart coordinates.
    pub const fn quart(block: i32) -> i32 {
        block >> 2
    }

    // TODO(abi): #72: resolve a noise biome at quart XYZ, using the worldgen biome source.
}

#[cfg(test)]
mod tests {
    use super::BiomeSpiral;

    #[test]
    fn java_spiral_visits_origin_then_legs_of_one_two_three() {
        let mut spiral = BiomeSpiral::new((100, -100), 4, 200, 100).unwrap();
        let expected = [
            (100, -100),
            (100, -104),
            (104, -104),
            (108, -104),
            (108, -100),
            (108, -96),
            (108, -92),
            (104, -92),
            (100, -92),
            (96, -92),
            (92, -92),
            (92, -96),
            (92, -100),
            (92, -104),
            (92, -108),
            (92, -112),
        ];
        for position in expected {
            assert_eq!(spiral.next_probe(), Some(position));
            assert_eq!(spiral.next_probe(), None, "probe must be completed first");
            spiral.finish_probe();
        }
        assert_eq!(spiral.samples(), 16);
    }

    #[test]
    fn search_limits_are_inclusive_like_java_has_work() {
        let mut spiral = BiomeSpiral::new((0, 0), 4, 4, 1).unwrap();
        assert_eq!(spiral.next_probe(), Some((0, 0)));
        spiral.finish_probe();
        assert_eq!(spiral.next_probe(), Some((0, -4)));
        spiral.finish_probe();
        assert_eq!(spiral.next_probe(), None);
        assert_eq!(spiral.samples(), 2);
        assert!(BiomeSpiral::new((0, 0), 0, 4, 1).is_none());
    }

    #[test]
    fn quart_conversion_floors_negative_blocks() {
        assert_eq!(BiomeSpiral::quart(-1), -1);
        assert_eq!(BiomeSpiral::quart(-5), -2);
        assert_eq!(BiomeSpiral::quart(3), 0);
    }
}
