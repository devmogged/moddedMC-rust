//! Raevyx's transient DragonCombatLearning (0.9.88), scoped to its beam.
use crate::{MOBS, flight::offset};
use moddedmc_api::entities::Entity;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, PoisonError},
};

type Uuid = [u8; 16];
type Vec3 = [f64; 3];
const MEMORY_TICKS: u64 = 1200;
const SAMPLE_TICKS: u64 = 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Response {
    #[default]
    None,
    Close,
    Retreat,
    Left,
    Right,
}
const RESPONSES: [Response; 5] = [
    Response::None,
    Response::Close,
    Response::Retreat,
    Response::Left,
    Response::Right,
];

#[derive(Clone, Copy, Default)]
pub(super) struct Expectation {
    response: Response,
    confidence: f64,
    weight: f64,
}

impl Expectation {
    pub(super) fn spacing(self) -> f64 {
        if self.response == Response::Close {
            8.0 * self.confidence
        } else {
            0.0
        }
    }
    pub(super) fn weight(self) -> f64 {
        if self.weight == 0.0 { 1.0 } else { self.weight }
    }
}

#[derive(Default)]
struct Habits {
    responses: [f64; 5],
    observations: u32,
    outcomes: u32,
    hits: f64,
    misses: f64,
    predictions: f64,
    correct: f64,
    decayed_at: u64,
}

impl Habits {
    fn decay(&mut self, now: u64) {
        let factor = 0.5f64.powf(now.saturating_sub(self.decayed_at) as f64 / 600.0);
        self.responses.iter_mut().for_each(|n| *n *= factor);
        self.hits *= factor;
        self.misses *= factor;
        self.predictions *= factor;
        self.correct *= factor;
        self.decayed_at = now;
    }

    fn expectation(&mut self, now: u64) -> Expectation {
        self.decay(now);
        let total: f64 = self.responses.iter().sum();
        let (mut best, mut runner, mut response) = (0.0f64, 0.0f64, Response::None);
        for (index, &count) in self.responses.iter().enumerate() {
            if count > best {
                runner = best;
                best = count;
                response = RESPONSES[index];
            } else {
                runner = runner.max(count);
            }
        }
        let confidence = if self.observations >= 3 && total >= 1.5 && best >= total * 0.6 {
            ((best - runner) / (total + 2.0)).min(0.8)
                * (0.5 + 0.5 * (self.correct + 1.0) / (self.predictions + 2.0))
        } else {
            0.0
        };
        let weight = if self.outcomes < 3 {
            1.0
        } else {
            (0.8 + 0.4 * (self.hits + 1.0) / (self.hits + self.misses + 2.0)).clamp(0.8, 1.2)
        };
        Expectation {
            response,
            confidence,
            weight,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Observation {
    pub uuid: Uuid,
    pub anchor: Option<i32>,
    pub center: Vec3,
    pub health: f32,
    pub hurt: Option<(i32, i64)>,
}

struct Memory {
    uuid: Uuid,
    seen: u64,
    habits: [Habits; 2],
}

#[derive(Clone, Copy)]
struct Sample {
    observation: Observation,
    tick: u64,
}

struct Trial {
    target: Uuid,
    aerial: bool,
    start: Sample,
    last: Sample,
    velocity: Vec3,
    forward: Vec3,
    expected: Expectation,
    obscured: bool,
    confounded: bool,
    released: bool,
    contact: bool,
    hit: bool,
}

impl Trial {
    fn response(&self) -> Option<Response> {
        let elapsed = self.last.tick.saturating_sub(self.start.tick);
        if self.obscured || self.confounded || elapsed < 6 {
            return None;
        }
        let movement = offset(self.last.observation.center, self.start.observation.center);
        let change = offset(movement, self.velocity.map(|v| v * elapsed as f64));
        let side = [-self.forward[2], 0.0, self.forward[0]];
        let dot = |a: Vec3, b: Vec3| a.into_iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
        let forward = dot(movement, self.forward);
        let forward_change = dot(change, self.forward);
        let lateral = dot(movement, side);
        let lateral_change = dot(change, side);
        Some(if forward < -2.0 && forward_change < -1.0 {
            Response::Close
        } else if forward > 2.0 && forward_change > 1.0 {
            Response::Retreat
        } else if lateral.abs() > 1.25
            && lateral_change.abs() > 1.25
            && lateral * lateral_change > 0.0
        {
            if lateral > 0.0 {
                Response::Left
            } else {
                Response::Right
            }
        } else {
            Response::None
        })
    }
}

#[derive(Clone, Copy)]
pub(super) enum Outcome {
    Completed,
    ResponseOnly,
    Blocked,
    Cancelled,
}

#[derive(Default)]
struct Learner {
    memory: VecDeque<Memory>,
    sample: Option<Sample>,
    velocity: Vec3,
    samples: u32,
    trial: Option<Trial>,
    seen: u64,
}

impl Learner {
    fn reset_motion(&mut self) {
        self.sample = None;
        self.velocity = [0.0; 3];
        self.samples = 0;
    }

    fn observe(&mut self, now: u64, observation: Option<Observation>) {
        self.seen = now;
        self.memory
            .retain(|m| now.saturating_sub(m.seen) <= MEMORY_TICKS);
        let Some(obs) = observation.filter(|o| o.center.iter().all(|v| v.is_finite())) else {
            if let Some(t) = &mut self.trial {
                t.obscured = true;
            }
            self.reset_motion();
            return;
        };
        if let Some(index) = self.memory.iter().position(|m| m.uuid == obs.uuid) {
            let mut mem = self.memory.remove(index).unwrap();
            mem.seen = now;
            self.memory.push_back(mem);
        } else {
            self.memory.push_back(Memory {
                uuid: obs.uuid,
                seen: now,
                habits: Default::default(),
            });
            if self.memory.len() > 4 {
                self.memory.pop_front();
            }
        }
        if let Some(sample) = self.sample {
            if sample.observation.uuid != obs.uuid || sample.observation.anchor != obs.anchor {
                self.finish(now, Outcome::Cancelled);
                self.reset_motion();
            } else {
                // shortcut: hurtTime is not exposed; reject changed health or hurt timestamp
                // during windup until a direct hurt-time read is available.
                if let Some(t) = &mut self.trial
                    && !t.released
                    && (obs.health < sample.observation.health
                        || obs.hurt != sample.observation.hurt)
                {
                    t.confounded = true;
                }
                let elapsed = now.saturating_sub(sample.tick);
                if elapsed < SAMPLE_TICKS {
                    return;
                }
                let measured =
                    offset(obs.center, sample.observation.center).map(|v| v / elapsed as f64);
                if elapsed > SAMPLE_TICKS * 2 {
                    if let Some(t) = &mut self.trial {
                        t.obscured = true;
                    }
                    self.reset_motion();
                } else if crate::flight::length_squared(measured) > 36.0 {
                    self.finish(now, Outcome::Cancelled);
                    self.reset_motion();
                } else {
                    self.velocity = if self.samples < 2 {
                        measured
                    } else {
                        std::array::from_fn(|i| self.velocity[i] * 0.35 + measured[i] * 0.65)
                    };
                }
            }
        }
        let sample = Sample {
            observation: obs,
            tick: now,
        };
        self.sample = Some(sample);
        self.samples = (self.samples + 1).min(100);
        if let Some(t) = &mut self.trial
            && !t.released
            && !t.obscured
            && t.target == obs.uuid
        {
            t.last = sample;
        }
    }

    fn expectation(&mut self, now: u64, aerial: bool) -> Expectation {
        let Some(sample) = self.sample else {
            return Expectation::default();
        };
        self.memory
            .iter_mut()
            .find(|m| m.uuid == sample.observation.uuid)
            .map_or_else(Expectation::default, |m| {
                m.habits[usize::from(aerial)].expectation(now)
            })
    }

    fn begin(&mut self, now: u64, position: Vec3, aerial: bool) {
        self.trial = None;
        let Some(start) = self
            .sample
            .filter(|s| now.saturating_sub(s.tick) <= SAMPLE_TICKS)
        else {
            return;
        };
        if self.samples < 3 {
            return;
        }
        let d = offset(start.observation.center, position);
        let len = d[0].hypot(d[2]);
        if len < 1e-3 {
            return;
        }
        let expected = self.expectation(now, aerial);
        self.trial = Some(Trial {
            target: start.observation.uuid,
            aerial,
            start,
            last: start,
            velocity: self.velocity,
            forward: [d[0] / len, 0.0, d[2] / len],
            expected,
            obscured: false,
            confounded: false,
            released: false,
            contact: false,
            hit: false,
        });
    }

    fn finish(&mut self, now: u64, outcome: Outcome) {
        let Some(t) = self.trial.take() else { return };
        let Some(mem) = self.memory.iter_mut().find(|m| m.uuid == t.target) else {
            return;
        };
        let habits = &mut mem.habits[usize::from(t.aerial)];
        habits.decay(now);
        if !matches!(outcome, Outcome::Cancelled)
            && let Some(response) = t.response()
        {
            habits.responses[response as usize] += 1.0;
            habits.observations += 1;
            if t.expected.confidence > 0.0 {
                habits.predictions += 1.0;
                if response == t.expected.response {
                    habits.correct += 1.0;
                }
            }
        }
        if t.hit
            || (matches!(outcome, Outcome::Completed) && t.released && !t.obscured && !t.contact)
        {
            habits.outcomes += 1;
            if t.hit {
                habits.hits += 1.0;
            } else {
                habits.misses += 1.0;
            }
        }
    }
}

#[derive(Default)]
struct Registry {
    dragons: HashMap<Uuid, Learner>,
    swept: u64,
}
static LEARNERS: Mutex<Option<Registry>> = Mutex::new(None);

fn with<T: Default>(mob: Entity<'_>, f: impl FnOnce(&mut Learner, u64) -> T) -> T {
    let Some(uuid) = MOBS.get().and_then(|m| m.uuid(mob)) else {
        return T::default();
    };
    let now = moddedmc_api::tick_count();
    let mut all = LEARNERS.lock().unwrap_or_else(PoisonError::into_inner);
    let reg = all.get_or_insert_default();
    if now.saturating_sub(reg.swept) >= MEMORY_TICKS {
        reg.dragons
            .retain(|_, learner| now.saturating_sub(learner.seen) <= MEMORY_TICKS);
        reg.swept = now;
    }
    f(reg.dragons.entry(uuid).or_default(), now)
}

pub(super) fn observe(mob: Entity<'_>, obs: Option<Observation>, aerial: bool) -> Expectation {
    with(mob, |l, now| {
        l.observe(now, obs);
        l.expectation(now, aerial)
    })
}
pub(super) fn begin(mob: Entity<'_>, pos: Vec3, aerial: bool) {
    with(mob, |l, now| l.begin(now, pos, aerial));
}
pub(super) fn contact(mob: Entity<'_>, target: Uuid, hit: bool) {
    with(mob, |l, _| {
        if let Some(t) = &mut l.trial
            && t.target == target
        {
            t.released = true;
            t.contact = true;
            t.hit |= hit;
        }
    });
}
pub(super) fn finish(mob: Entity<'_>, outcome: Outcome) {
    with(mob, |l, now| l.finish(now, outcome));
}
pub(super) fn clear(mob: Entity<'_>) {
    if let Some(uuid) = MOBS.get().and_then(|m| m.uuid(mob))
        && let Some(reg) = LEARNERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    {
        reg.dragons.remove(&uuid);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_close_response_increases_spacing_and_hits_change_weight() {
        let mut l = Learner::default();
        let obs = |x| Observation {
            uuid: [1; 16],
            anchor: None,
            center: [x, 0.0, 0.0],
            health: 100.0,
            hurt: None,
        };
        for start in [0, 40, 80] {
            for dt in [0, 2, 4] {
                l.observe(start + dt, Some(obs(20.0)));
            }
            l.begin(start + 4, [0.0; 3], false);
            for dt in [6, 8, 10, 12] {
                l.observe(start + dt, Some(obs(20.0 - (dt - 4) as f64 * 0.5)));
            }
            l.trial.as_mut().unwrap().hit = true;
            l.finish(start + 12, Outcome::Completed);
        }
        let expected = l.expectation(92, false);
        assert!(expected.spacing() > 2.0);
        assert!(expected.weight() > 1.0);
        assert_eq!(l.expectation(92, true).weight(), 1.0);
        l.observe(1293, None);
        assert!(l.memory.is_empty());
    }

    #[test]
    fn blocked_or_cancelled_contact_does_not_count_as_miss() {
        let mut l = Learner::default();
        let obs = Observation {
            uuid: [2; 16],
            anchor: None,
            center: [10.0, 0.0, 0.0],
            health: 100.0,
            hurt: None,
        };
        for tick in [0, 2, 4] {
            l.observe(tick, Some(obs));
        }
        l.begin(4, [0.0; 3], false);
        l.trial.as_mut().unwrap().contact = true;
        l.finish(10, Outcome::Blocked);
        assert_eq!(l.memory[0].habits[0].outcomes, 0);
        l.begin(4, [0.0; 3], false);
        l.finish(10, Outcome::Cancelled);
        assert_eq!(l.memory[0].habits[0].outcomes, 0);
    }
}
