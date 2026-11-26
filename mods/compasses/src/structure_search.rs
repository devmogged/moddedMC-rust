//! Resumable single-target Explorer's Compass search.
use moddedmc_api::{Pos, worldgen_lookup::Lookup};

const MAX_RADIUS: i32 = 10_000;
const MAX_SAMPLES: u32 = 100_000;

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
    concentric: Option<Vec<Pos>>,
    cursor: usize,
    samples: u32,
    radius: i32,
}

pub trait Resolver {
    fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos>;
    fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>>;
    fn start(&mut self, placement: u32, x: i32, z: i32) -> Lookup<Pos>;
}

impl Search {
    pub fn new(origin: Pos, previous: Vec<Pos>, placements: Vec<(u32, i32)>) -> Self {
        Self {
            origin,
            previous,
            placements,
            index: 0,
            ring: Ring::new(),
            concentric: None,
            cursor: 0,
            samples: 0,
            radius: 0,
        }
    }

    fn next_placement(&mut self) {
        self.index += 1;
        self.ring = Ring::new();
        self.concentric = None;
        self.cursor = 0;
        self.samples = 0;
        self.radius = 0;
    }

    pub fn advance(&mut self, resolver: &mut impl Resolver) -> Lookup<Pos> {
        let Some(&(placement, spacing)) = self.placements.get(self.index) else {
            return Lookup::Absent;
        };
        if self.samples >= MAX_SAMPLES || (spacing != 0 && self.radius >= MAX_RADIUS) {
            self.next_placement();
            return Lookup::Pending;
        }
        let candidate = if spacing == 0 {
            if self.concentric.is_none() {
                match resolver.rings(placement) {
                    Lookup::Found(mut chunks) => {
                        // Java chooses nearest chunk center; stable sort preserves ties.
                        chunks.sort_by_key(|p| distance_sq(self.origin, *p));
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
            let Some(candidate) = self
                .concentric
                .as_ref()
                .and_then(|p| p.get(self.cursor))
                .copied()
            else {
                self.next_placement();
                return Lookup::Pending;
            };
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
            candidate
        };
        let result = resolver.start(placement, candidate.x, candidate.z);
        match result {
            Lookup::Pending => return Lookup::Pending,
            Lookup::Unavailable => return Lookup::Unavailable,
            Lookup::Found(pos) if !self.previous.contains(&pos) => {
                return Lookup::Found(pos);
            }
            _ => {}
        }
        self.samples += 1;
        // StructureUtils.getHorizontalDistanceToLocation uses Mth.sqrt(float)
        // followed by truncation to an integer.
        self.radius = (distance_sq(self.origin, candidate) as f32).sqrt() as i32;
        if spacing == 0 {
            self.cursor += 1;
        } else {
            self.ring.next();
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
        assert_eq!(
            search.advance(&mut fake),
            Lookup::Found(Pos { x: 48, y: 0, z: 0 })
        );
        assert_eq!(fake.calls, [(1, 0), (1, 0), (3, 0)]);
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
        assert_eq!(resolver.0, 1);
    }
}
