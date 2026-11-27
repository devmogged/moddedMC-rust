//! Resumable Nature's Compass 26.3-3.3.1 BiomeSearchWorker.
use super::biome_spiral::BiomeSpiral;

pub type Position = (i32, i32, i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchResult {
    Pending,
    Found {
        position: Position,
        samples: u32,
        radius: i32,
    },
    NotFound {
        samples: u32,
        radius: i32,
    },
    Unavailable,
}

#[derive(Debug)]
struct Connection {
    candidate: Position,
    previous_index: usize,
    index: i32,
    count: i32,
    misses: i32,
}

#[derive(Debug)]
pub struct Search {
    spiral: BiomeSpiral,
    step: i32,
    heights: Vec<i32>,
    previous: Vec<Position>,
    probe: Option<(i32, i32)>,
    height_index: usize,
    connection: Option<Connection>,
    result: SearchResult,
}

impl Search {
    /// Height bounds match the level's getMinY and getMaxY; the lower bound
    /// used by Java is min_y + 1, and the upper bound is inclusive.
    pub fn new(
        origin: Position,
        min_y: i32,
        max_y: i32,
        step: i32,
        max_radius: i32,
        max_samples: u32,
        previous: Vec<Position>,
    ) -> Option<Self> {
        let lower = min_y.checked_add(1)?;
        if lower > max_y {
            return None;
        }
        // Mth.outFromOrigin first clamps the starting Y into its inclusive bounds.
        let y = origin.1.clamp(lower, max_y);
        let mut heights = vec![y];
        let mut offset = 64_i64;
        loop {
            let up = i64::from(y) + offset;
            let down = i64::from(y) - offset;
            if up > i64::from(max_y) && down < i64::from(lower) {
                break;
            }
            if up <= i64::from(max_y) {
                heights.push(up as i32);
            }
            if down >= i64::from(lower) {
                heights.push(down as i32);
            }
            offset += 64;
        }
        Some(Self {
            spiral: BiomeSpiral::new((origin.0, origin.2), step, max_radius, max_samples)?,
            step,
            heights,
            previous,
            probe: None,
            height_index: 0,
            connection: None,
            result: SearchResult::Pending,
        })
    }

    pub fn previous(&self) -> &[Position] {
        &self.previous
    }
    pub fn samples(&self) -> u32 {
        self.spiral.samples()
    }
    pub fn radius(&self) -> i32 {
        self.spiral.radius()
    }

    fn accept(&mut self, position: Position) {
        self.previous.push(position);
        self.result = SearchResult::Found {
            position,
            samples: self.samples(),
            radius: self.radius(),
        };
    }

    fn reject_probe(&mut self) {
        self.connection = None;
        self.probe = None;
        self.height_index = 0;
        self.spiral.finish_probe();
    }

    /// At most budget resolver calls per invocation. None means unavailable
    /// (not a negative biome match); it leaves the current lookup retryable.
    pub fn advance(
        &mut self,
        mut budget: usize,
        mut resolver: impl FnMut(i32, i32, i32) -> Option<bool>,
    ) -> SearchResult {
        if self.result != SearchResult::Pending {
            return self.result;
        }
        while budget > 0 {
            if let Some(mut c) = self.connection.take() {
                // The upstream worker's hasWork also gates connection checks.
                if !self.spiral.has_work() {
                    self.result = SearchResult::NotFound {
                        samples: self.samples(),
                        radius: self.radius(),
                    };
                    return self.result;
                }
                if c.previous_index >= self.previous.len() {
                    self.accept(c.candidate);
                    return self.result;
                }
                let prev = self.previous[c.previous_index];
                let dx = c.candidate.0.wrapping_sub(prev.0);
                let dz = c.candidate.2.wrapping_sub(prev.2);
                if c.index == 0 {
                    // Java multiplies ints before widening to double.
                    let distance =
                        (f64::from(dx.wrapping_mul(dx)) + f64::from(dz.wrapping_mul(dz))).sqrt();
                    if distance <= f64::from(self.step.wrapping_mul(2)) {
                        self.reject_probe();
                        continue;
                    }
                    c.count = (distance / f64::from(self.step)) as i32;
                    c.index = 1;
                    c.misses = 0;
                }
                if c.index >= c.count {
                    self.reject_probe();
                    continue;
                }
                let t = f64::from(c.index) / f64::from(c.count);
                let x = (f64::from(prev.0) + f64::from(dx) * t) as i32;
                let z = (f64::from(prev.2) + f64::from(dz) * t) as i32;
                let matched = if let Some(&y) = self.heights.get(self.height_index) {
                    budget -= 1;
                    match resolver(
                        BiomeSpiral::quart(x),
                        BiomeSpiral::quart(y),
                        BiomeSpiral::quart(z),
                    ) {
                        None => {
                            self.connection = Some(c);
                            return SearchResult::Unavailable;
                        }
                        Some(true) => true,
                        Some(false) => {
                            self.height_index += 1;
                            if self.height_index < self.heights.len() {
                                self.connection = Some(c);
                                continue;
                            }
                            false
                        }
                    }
                } else {
                    false
                };
                self.height_index = 0;
                if matched {
                    c.misses = 0;
                } else {
                    c.misses += 1;
                }
                if c.misses >= (c.count / 4).clamp(1, 10) {
                    c.previous_index += 1;
                    c.index = 0;
                    c.misses = 0;
                } else {
                    c.index += 1;
                }
                self.connection = Some(c);
                continue;
            }

            if self.probe.is_none() {
                self.probe = self.spiral.next_probe();
                if self.probe.is_none() {
                    self.result = SearchResult::NotFound {
                        samples: self.samples(),
                        radius: self.radius(),
                    };
                    return self.result;
                }
            }
            let (x, z) = self.probe.unwrap();
            let Some(&y) = self.heights.get(self.height_index) else {
                self.reject_probe();
                // Charge empty columns too to keep the loop bounded.
                budget -= 1;
                continue;
            };
            budget -= 1;
            match resolver(
                BiomeSpiral::quart(x),
                BiomeSpiral::quart(y),
                BiomeSpiral::quart(z),
            ) {
                None => return SearchResult::Unavailable,
                Some(false) => {
                    self.height_index += 1;
                    if self.height_index == self.heights.len() {
                        self.reject_probe();
                    }
                }
                Some(true) => {
                    let candidate = (x, y, z);
                    if self.previous.is_empty() {
                        self.accept(candidate);
                        return self.result;
                    }
                    self.height_index = 0;
                    self.connection = Some(Connection {
                        candidate,
                        previous_index: 0,
                        index: 0,
                        count: 0,
                        misses: 0,
                    });
                }
            }
        }
        SearchResult::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_order_and_budget() {
        let mut search = Search::new((-1, 65, -5), -64, 320, 4, 100, 10, vec![]).unwrap();
        let mut seen = vec![];
        for _ in 0..5 {
            assert_eq!(
                search.advance(1, |x, y, z| {
                    seen.push((x, y, z));
                    Some(false)
                }),
                SearchResult::Pending
            );
        }
        assert_eq!(
            seen,
            vec![
                (-1, 16, -2),
                (-1, 32, -2),
                (-1, 0, -2),
                (-1, 48, -2),
                (-1, -16, -2)
            ]
        );
        assert_eq!(search.samples(), 0);
    }

    #[test]
    fn origin_outside_height_bounds_is_clamped_before_sampling() {
        for (origin_y, expected_y, expected_quart) in [(500, 319, 79), (-100, -63, -16)] {
            let mut search = Search::new((-1, origin_y, -5), -64, 319, 4, 100, 10, vec![]).unwrap();
            assert_eq!(
                search.advance(1, |x, y, z| {
                    assert_eq!((x, y, z), (-1, expected_quart, -2));
                    Some(true)
                }),
                SearchResult::Found {
                    position: (-1, expected_y, -5),
                    samples: 0,
                    radius: 0,
                }
            );
        }
    }

    #[test]
    fn inclusive_samples_and_radius() {
        let mut search = Search::new((0, 0, 0), -1, 0, 4, 4, 1, vec![]).unwrap();
        assert_eq!(
            search.advance(10, |_, _, _| Some(false)),
            SearchResult::NotFound {
                samples: 2,
                radius: 4
            }
        );
        let mut search = Search::new((0, 0, 0), -1, 0, 4, 1, 10, vec![]).unwrap();
        assert_eq!(
            search.advance(10, |_, _, z| Some(z == -1)),
            SearchResult::Found {
                position: (0, 0, -4),
                samples: 1,
                radius: 4
            }
        );
    }

    #[test]
    fn find_next_skips_nearby_and_accepts_separated_candidate() {
        let mut search = Search::new((0, 0, 0), -1, 0, 4, 100, 100, vec![(0, 0, 0)]).unwrap();
        let result = loop {
            let r = search.advance(1, |x, _, z| Some((x == 0 && z == 0) || (x == 2 && z == -1)));
            if r != SearchResult::Pending {
                break r;
            }
        };
        assert_eq!(
            result,
            SearchResult::Found {
                position: (8, 0, -4),
                samples: 3,
                radius: 8
            }
        );
        assert_eq!(search.previous().len(), 2);
    }

    #[test]
    fn unavailable_retries_same_lookup() {
        let mut search = Search::new((0, 0, 0), -1, 0, 4, 100, 100, vec![]).unwrap();
        assert_eq!(search.advance(1, |_, _, _| None), SearchResult::Unavailable);
        assert_eq!(
            search.advance(1, |x, y, z| {
                assert_eq!((x, y, z), (0, 0, 0));
                Some(true)
            }),
            SearchResult::Found {
                position: (0, 0, 0),
                samples: 0,
                radius: 0
            }
        );
    }

    #[test]
    fn connection_gap_resumes_at_interrupted_height_and_checks_all_previous_positions() {
        let mut search = Search::new(
            (0, 0, 0),
            -1,
            64,
            4,
            100,
            100,
            vec![(-16, 0, 0), (0, 0, -16)],
        )
        .unwrap();
        // Initial candidate, then the first interpolation column's two heights.
        assert_eq!(
            search.advance(1, |x, y, z| {
                assert_eq!((x, y, z), (0, 0, 0));
                Some(true)
            }),
            SearchResult::Pending
        );
        assert_eq!(
            search.advance(1, |x, y, z| {
                assert_eq!((x, y, z), (-3, 0, 0));
                Some(false)
            }),
            SearchResult::Pending
        );
        assert_eq!(
            search.advance(1, |x, y, z| {
                assert_eq!((x, y, z), (-3, 16, 0));
                None
            }),
            SearchResult::Unavailable
        );
        assert_eq!(
            search.advance(1, |x, y, z| {
                assert_eq!((x, y, z), (-3, 16, 0));
                Some(false)
            }),
            SearchResult::Pending
        );
        let mut seen = vec![];
        assert_eq!(
            search.advance(3, |x, y, z| {
                seen.push((x, y, z));
                Some(false)
            }),
            SearchResult::Found {
                position: (0, 0, 0),
                samples: 0,
                radius: 0
            }
        );
        assert_eq!(seen, [(0, 0, -3), (0, 16, -3)]);
        assert_eq!(search.previous().len(), 3);
    }

    #[test]
    fn matching_upper_height_prevents_false_connection_gap() {
        let mut search = Search::new((0, 0, 0), -1, 64, 4, 100, 0, vec![(-16, 0, 0)]).unwrap();
        let mut seen = vec![];
        let result = search.advance(32, |x, y, z| {
            seen.push((x, y, z));
            Some(x == 0 || y == 16)
        });
        assert_eq!(
            result,
            SearchResult::NotFound {
                samples: 1,
                radius: 0
            }
        );
        assert_eq!(
            seen,
            [
                (0, 0, 0),
                (-3, 0, 0),
                (-3, 16, 0),
                (-2, 0, 0),
                (-2, 16, 0),
                (-1, 0, 0),
                (-1, 16, 0)
            ]
        );
        assert_eq!(search.previous(), &[(-16, 0, 0)]);
    }

    #[test]
    fn gap_requires_consecutive_misses_and_caps_threshold_at_ten() {
        // Eighty intervals: the upstream 25% gap threshold is capped at ten.
        // Nine misses followed by a matching column must reset the streak.
        let mut search = Search::new((0, 0, 0), -1, 0, 4, 100, 0, vec![(-320, 0, 0)]).unwrap();
        let mut seen = vec![];
        let result = search.advance(100, |x, _, _| {
            seen.push(x);
            Some(x == 0 || x == -70)
        });
        assert_eq!(
            result,
            SearchResult::Found {
                position: (0, 0, 0),
                samples: 0,
                radius: 0
            }
        );
        let expected: Vec<_> = std::iter::once(0).chain(-79..=-60).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn previous_history_does_not_allow_connection_checks_beyond_radius_limit() {
        let mut search = Search::new((0, 0, 0), -1, 0, 4, 1, 10, vec![(-16, 0, 0)]).unwrap();
        let mut seen = vec![];
        let result = search.advance(10, |x, y, z| {
            seen.push((x, y, z));
            Some(z == -1)
        });
        // The original scheduler removes a worker that crosses the radius
        // boundary before its pending connection check can run.
        assert_eq!(
            result,
            SearchResult::NotFound {
                samples: 1,
                radius: 4
            }
        );
        assert_eq!(seen, [(0, 0, 0), (0, 0, -1)]);
        assert_eq!(search.previous(), &[(-16, 0, 0)]);
    }
}
