//! Resumable single-target Explorer's Compass search.
use moddedmc_api::{Pos, worldgen_lookup::Lookup};

const MAX_RADIUS: i32 = 10_000;
const MAX_SAMPLES: u32 = 100_000;
// Count cheap distance-only entries in batches while making at most one
// potentially expensive structure-start request per advance.
const RING_ENTRIES_PER_ADVANCE: usize = 64;

#[derive(Debug)]
struct Ring {
    radius: i32,
    x: i32,
    z: i32,
}

impl Ring {
    fn new() -> Self {
        Self {
            radius: 0,
            x: 0,
            z: 0,
        }
    }
    fn current(&self) -> (i32, i32) {
        (self.x, self.z)
    }
    fn next(&mut self) {
        if self.z < self.radius {
            if self.x.abs() == self.radius {
                self.z += 1;
            } else {
                self.z = self.radius;
            }
        } else if self.x < self.radius {
            self.x += 1;
            self.z = -self.radius;
        } else {
            self.radius += 1;
            self.x = -self.radius;
            self.z = -self.radius;
        }
    }
}

#[derive(Debug)]
pub struct Search {
    origin: Pos,
    previous: Vec<Pos>,
    placements: Vec<(u32, i32)>,
    index: usize,
    ring: Ring,
    spread_candidate: Option<Pos>,
    concentric: Option<Vec<Pos>>,
    closest: Option<(i64, Pos)>,
    found: Option<Pos>,
    cursor: usize,
    samples: u32,
    radius: i32,
    max_radius: i32,
    max_samples: u32,
}

pub trait Resolver {
    fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos>;
    fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>>;
    fn start(&mut self, placement: u32, x: i32, z: i32) -> Lookup<Pos>;
    /// Called only for a non-ignored start that becomes the worker's best match.
    fn accept(&mut self, _position: Pos) {}
}

impl Search {
    pub fn new(origin: Pos, previous: Vec<Pos>, placements: Vec<(u32, i32)>) -> Self {
        Self::with_limits(origin, previous, placements, MAX_RADIUS, MAX_SAMPLES)
    }

    pub fn with_limits(
        origin: Pos,
        previous: Vec<Pos>,
        placements: Vec<(u32, i32)>,
        max_radius: i32,
        max_samples: u32,
    ) -> Self {
        Self {
            origin,
            previous,
            placements,
            index: 0,
            ring: Ring::new(),
            spread_candidate: None,
            concentric: None,
            closest: None,
            found: None,
            cursor: 0,
            samples: 0,
            radius: 0,
            max_radius,
            max_samples,
        }
    }

    fn next_placement(&mut self) {
        self.index += 1;
        if self.index == self.placements.len() {
            // Retain the final worker's sample/radius totals until discarded.
            return;
        }
        self.ring = Ring::new();
        self.spread_candidate = None;
        self.concentric = None;
        self.closest = None;
        self.cursor = 0;
        self.samples = 0;
        self.radius = 0;
    }

    /// Per-worker progress, as reported by the original Explorer workers.
    /// Concentric ring positions are not ordered by radius in the source,
    /// so the ring worker does not advertise a meaningful search radius.
    pub fn progress(&self) -> (u32, Option<i32>) {
        let radius = self
            .placements
            .get(self.index)
            .or_else(|| self.placements.last())
            .and_then(|&(_, spacing)| (spacing != 0).then_some(self.radius));
        (self.samples, radius)
    }

    pub fn advance(&mut self, resolver: &mut impl Resolver) -> Lookup<Pos> {
        if let Some(pos) = self.found {
            return Lookup::Found(pos);
        }
        let Some(&(placement, spacing)) = self.placements.get(self.index) else {
            return Lookup::Absent;
        };
        if self.max_radius <= 0
            || self.samples >= self.max_samples
            || (spacing != 0 && self.radius >= self.max_radius)
        {
            self.next_placement();
            return Lookup::Pending;
        }
        if spacing == 0 {
            if self.concentric.is_none() {
                match resolver.rings(placement) {
                    Lookup::Found(chunks) => {
                        self.concentric = Some(chunks);
                    }
                    Lookup::Pending => return Lookup::Pending,
                    Lookup::Absent => {
                        self.next_placement();
                        return Lookup::Pending;
                    }
                    Lookup::Unavailable => return Lookup::Unavailable,
                }
            }
            let chunks = self.concentric.as_ref().unwrap();
            if self.cursor >= chunks.len() {
                self.next_placement();
                return Lookup::Pending;
            }
            // Java visits the original ring order and counts every entry,
            // even when a farther/equidistant chunk needs no start lookup.
            let mut queried = false;
            for _ in 0..RING_ENTRIES_PER_ADVANCE {
                let candidate = chunks[self.cursor];
                let distance = distance_sq(self.origin, candidate);
                if self.closest.is_none_or(|(best, _)| distance < best) {
                    if queried {
                        return Lookup::Pending;
                    }
                    queried = true;
                    match resolver.start(placement, candidate.x, candidate.z) {
                        Lookup::Pending => return Lookup::Pending,
                        Lookup::Unavailable => return Lookup::Unavailable,
                        Lookup::Found(pos) if !self.previous.contains(&pos) => {
                            self.closest = Some((distance, pos));
                            resolver.accept(pos);
                        }
                        _ => {}
                    }
                }
                self.samples += 1;
                self.cursor += 1;
                if self.samples >= self.max_samples || self.cursor == chunks.len() {
                    if let Some((_, pos)) = self.closest {
                        self.found = Some(pos);
                        return Lookup::Found(pos);
                    }
                    return Lookup::Pending;
                }
            }
            return Lookup::Pending;
        }
        let candidate = if let Some(candidate) = self.spread_candidate {
            candidate
        } else {
            let (x, z) = self.ring.current();
            let Some(x) = x
                .checked_mul(spacing)
                .and_then(|x| x.checked_add(self.origin.x >> 4))
            else {
                return Lookup::Unavailable;
            };
            let Some(z) = z
                .checked_mul(spacing)
                .and_then(|z| z.checked_add(self.origin.z >> 4))
            else {
                return Lookup::Unavailable;
            };
            let Some(candidate) = resolver.candidate(placement, x, z) else {
                return Lookup::Unavailable;
            };
            self.spread_candidate = Some(candidate);
            candidate
        };
        let found = match resolver.start(placement, candidate.x, candidate.z) {
            Lookup::Pending => return Lookup::Pending,
            Lookup::Unavailable => return Lookup::Unavailable,
            Lookup::Found(pos) if !self.previous.contains(&pos) => Some(pos),
            _ => None,
        };
        self.samples += 1;
        self.spread_candidate = None;
        // StructureUtils.getHorizontalDistanceToLocation uses Mth.sqrt(float)
        // followed by truncation to an integer.
        self.radius = (distance_sq(self.origin, candidate) as f32).sqrt() as i32;
        self.ring.next();
        if let Some(pos) = found {
            self.found = Some(pos);
            resolver.accept(pos);
            return Lookup::Found(pos);
        }
        Lookup::Pending
    }
}

fn distance_sq(origin: Pos, chunk: Pos) -> i64 {
    let dx = i64::from(chunk.x) * 16 + 8 - i64::from(origin.x);
    let dz = i64::from(chunk.z) * 16 + 8 - i64::from(origin.z);
    dx.saturating_mul(dx).saturating_add(dz.saturating_mul(dz))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ring_order_skips_interior() {
        let mut ring = Ring::new();
        let mut actual = Vec::new();
        for _ in 0..10 {
            actual.push(ring.current());
            ring.next();
        }
        assert_eq!(
            actual,
            [
                (0, 0),
                (-1, -1),
                (-1, 0),
                (-1, 1),
                (0, -1),
                (0, 1),
                (1, -1),
                (1, 0),
                (1, 1),
                (-2, -2)
            ]
        );
    }
    struct Fake {
        calls: Vec<(i32, i32)>,
        pending: bool,
    }
    impl Resolver for Fake {
        fn candidate(&mut self, _: u32, x: i32, z: i32) -> Option<Pos> {
            Some(Pos { x, y: 0, z })
        }
        fn rings(&mut self, _: u32) -> Lookup<Vec<Pos>> {
            Lookup::Found(vec![
                Pos { x: 5, y: 0, z: 0 },
                Pos { x: 1, y: 0, z: 0 },
                Pos { x: 3, y: 0, z: 0 },
            ])
        }
        fn start(&mut self, _: u32, x: i32, z: i32) -> Lookup<Pos> {
            self.calls.push((x, z));
            if self.pending {
                self.pending = false;
                return Lookup::Pending;
            }
            Lookup::Found(Pos {
                x: x * 16,
                y: 0,
                z: z * 16,
            })
        }
    }
    #[test]
    fn pending_retries_and_find_next_skips_previous_ring_start() {
        let mut search = Search::new(
            Pos::default(),
            vec![Pos { x: 16, y: 0, z: 0 }],
            vec![(0, 0)],
        );
        let mut fake = Fake {
            calls: vec![],
            pending: true,
        };
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(
            search.advance(&mut fake),
            Lookup::Found(Pos { x: 48, y: 0, z: 0 })
        );
        assert_eq!(fake.calls, [(5, 0), (5, 0), (1, 0), (3, 0)]);
        assert_eq!(search.progress(), (3, None));
    }

    #[test]
    fn ring_sample_limit_applies_to_generation_order() {
        let mut search = Search::with_limits(Pos::default(), vec![], vec![(0, 0)], 10_000, 1);
        let mut fake = Fake {
            calls: vec![],
            pending: false,
        };
        // Original ring order is 5, 1, 3; a one-sample limit cannot see 1.
        assert_eq!(
            search.advance(&mut fake),
            Lookup::Found(Pos { x: 80, y: 0, z: 0 })
        );
        assert_eq!(fake.calls, [(5, 0)]);
    }

    #[test]
    fn ring_ignores_positive_radius_but_zero_radius_or_samples_disable_worker() {
        let mut fake = Fake {
            calls: vec![],
            pending: false,
        };
        let mut search = Search::with_limits(Pos::default(), vec![], vec![(0, 0)], 1, 3);
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(
            search.advance(&mut fake),
            Lookup::Found(Pos { x: 16, y: 0, z: 0 })
        );
        assert_eq!(fake.calls, [(5, 0), (1, 0)]);
        assert_eq!(search.progress(), (3, None));
        for (radius, samples) in [(0, 3), (10_000, 0)] {
            fake.calls.clear();
            let mut search =
                Search::with_limits(Pos::default(), vec![], vec![(0, 0)], radius, samples);
            assert_eq!(search.advance(&mut fake), Lookup::Pending);
            assert_eq!(search.advance(&mut fake), Lookup::Absent);
            assert!(fake.calls.is_empty());
        }
    }

    #[test]
    fn progress_distinguishes_ring_and_spread_workers() {
        let mut search = Search::with_limits(
            Pos::default(),
            vec![Pos { x: 80, y: 0, z: 0 }],
            vec![(0, 0), (1, 32)],
            10000,
            1,
        );
        let mut fake = Fake {
            calls: vec![],
            pending: true,
        };
        assert_eq!(search.progress(), (0, None));
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(search.progress(), (0, None));
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(search.progress(), (1, None));
        assert_eq!(search.advance(&mut fake), Lookup::Pending);
        assert_eq!(search.progress(), (0, Some(0)));
        assert_eq!(search.advance(&mut fake), Lookup::Found(Pos::default()));
        assert_eq!(search.progress(), (1, Some(11)));
    }

    #[test]
    fn equidistant_ring_candidates_keep_first_generation_order_and_count_skips() {
        let mut search = Search::with_limits(
            Pos {
                x: 16,
                y: 100,
                z: 8,
            },
            vec![],
            vec![(0, 0)],
            1,
            3,
        );
        struct Ties(Vec<i32>);
        impl Resolver for Ties {
            fn candidate(&mut self, _: u32, _: i32, _: i32) -> Option<Pos> {
                unreachable!()
            }
            fn rings(&mut self, _: u32) -> Lookup<Vec<Pos>> {
                Lookup::Found(vec![
                    Pos { x: 1, y: 0, z: 0 },
                    Pos::default(),
                    Pos { x: 2, y: 0, z: 0 },
                ])
            }
            fn start(&mut self, _: u32, x: i32, _: i32) -> Lookup<Pos> {
                self.0.push(x);
                Lookup::Found(Pos {
                    x: x * 16,
                    y: 0,
                    z: 0,
                })
            }
        }
        let mut resolver = Ties(vec![]);
        assert_eq!(
            search.advance(&mut resolver),
            Lookup::Found(Pos { x: 16, y: 0, z: 0 })
        );
        assert_eq!(resolver.0, [1]);
        assert_eq!(search.progress(), (3, None));
    }

    #[test]
    fn distance_only_ring_entries_are_batched_without_extra_world_lookups() {
        struct Farther {
            lookups: u32,
        }
        impl Resolver for Farther {
            fn candidate(&mut self, _: u32, _: i32, _: i32) -> Option<Pos> {
                unreachable!()
            }
            fn rings(&mut self, _: u32) -> Lookup<Vec<Pos>> {
                Lookup::Found((0..200).map(|x| Pos { x, y: 0, z: 0 }).collect())
            }
            fn start(&mut self, _: u32, x: i32, z: i32) -> Lookup<Pos> {
                self.lookups += 1;
                assert_eq!((x, z), (0, 0));
                Lookup::Found(Pos::default())
            }
        }
        let mut search = Search::new(Pos::default(), vec![], vec![(0, 0)]);
        let mut resolver = Farther { lookups: 0 };
        for samples in [64, 128, 192] {
            assert_eq!(search.advance(&mut resolver), Lookup::Pending);
            assert_eq!(search.progress(), (samples, None));
        }
        assert_eq!(search.advance(&mut resolver), Lookup::Found(Pos::default()));
        assert_eq!(search.progress(), (200, None));
        assert_eq!(resolver.lookups, 1);
        assert_eq!(search.advance(&mut resolver), Lookup::Found(Pos::default()));
        assert_eq!(search.progress(), (200, None));
        assert_eq!(resolver.lookups, 1);
    }

    #[test]
    fn random_spread_retries_the_same_negative_section_and_counts_completed_starts() {
        #[derive(Default)]
        struct Spread {
            sections: Vec<(i32, i32)>,
            starts: Vec<(i32, i32)>,
            retry: bool,
        }
        impl Resolver for Spread {
            fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos> {
                assert_eq!(placement, 7);
                self.sections.push((x, z));
                Some(Pos {
                    x: x + 1,
                    y: 0,
                    z: z + 2,
                })
            }
            fn rings(&mut self, _: u32) -> Lookup<Vec<Pos>> {
                unreachable!()
            }
            fn start(&mut self, _: u32, x: i32, z: i32) -> Lookup<Pos> {
                self.starts.push((x, z));
                if (x, z) == (-33, -31) {
                    if !self.retry {
                        self.retry = true;
                        return Lookup::Pending;
                    }
                    return Lookup::Absent;
                }
                Lookup::Found(Pos {
                    x: x * 16,
                    y: 5,
                    z: z * 16,
                })
            }
        }
        let mut search = Search::with_limits(
            Pos {
                x: -17,
                y: 120,
                z: -1,
            },
            vec![Pos {
                x: -16,
                y: 5,
                z: 16,
            }],
            vec![(7, 32)],
            10_000,
            3,
        );
        let mut resolver = Spread::default();
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(search.progress(), (1, Some(26)));
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(search.progress(), (1, Some(26)));
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(search.progress().0, 2);
        assert_eq!(
            search.advance(&mut resolver),
            Lookup::Found(Pos {
                x: -528,
                y: 5,
                z: 16
            })
        );
        assert_eq!(search.progress(), (3, Some(503)));
        assert_eq!(
            search.advance(&mut resolver),
            Lookup::Found(Pos {
                x: -528,
                y: 5,
                z: 16
            })
        );
        assert_eq!(search.progress(), (3, Some(503)));
        assert_eq!(resolver.sections, [(-2, -1), (-34, -33), (-34, -1)]);
        assert_eq!(resolver.starts, [(-1, 1), (-33, -31), (-33, -31), (-33, 1)]);
    }

    #[test]
    fn radius_limit_uses_java_float_rounding() {
        struct Boundary(u32);
        impl Resolver for Boundary {
            fn candidate(&mut self, _: u32, _: i32, _: i32) -> Option<Pos> {
                Some(Pos::default())
            }
            fn rings(&mut self, _: u32) -> Lookup<Vec<Pos>> {
                unreachable!()
            }
            fn start(&mut self, _: u32, _: i32, _: i32) -> Lookup<Pos> {
                self.0 += 1;
                Lookup::Absent
            }
        }
        // The squared distance is 99,999,997, which rounds to 100,000,000
        // when converted to Java float before sqrt, but not to double.
        let origin = Pos {
            x: -1338,
            y: 0,
            z: -9901,
        };
        assert_eq!(distance_sq(origin, Pos::default()), 99_999_997);
        let mut search = Search::new(origin, vec![], vec![(0, 32)]);
        let mut resolver = Boundary(0);
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(search.radius, MAX_RADIUS);
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(search.advance(&mut resolver), Lookup::Absent);
        assert_eq!(search.progress(), (1, Some(MAX_RADIUS)));
        assert_eq!(search.advance(&mut resolver), Lookup::Absent);
        assert_eq!(resolver.0, 1);
    }
}
