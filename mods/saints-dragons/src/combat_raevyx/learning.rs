//! Transient DragonCombatLearning (0.9.88), shared by ranged combat controllers.
use crate::{MOBS, flight::offset};
use moddedmc_api::entities::Entity;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
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
pub(crate) struct Expectation {
    response: Response,
    confidence: f64,
    weight: f64,
}

impl Expectation {
    pub(crate) fn spacing(self) -> f64 {
        if self.response == Response::Close {
            8.0 * self.confidence
        } else {
            0.0
        }
    }
    pub(crate) fn weight(self) -> f64 {
        if self.weight == 0.0 { 1.0 } else { self.weight }
    }
    pub(crate) fn air_spacing(self) -> f64 {
        28.0 + self.spacing()
            - if self.response == Response::Retreat {
                4.0 * self.confidence
            } else {
                0.0
            }
    }
    pub(crate) fn air_side(self) -> f64 {
        match self.response {
            Response::Left => 4.0 * self.confidence,
            Response::Right => -4.0 * self.confidence,
            _ => 0.0,
        }
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
pub(crate) struct Observation {
    pub uuid: Uuid,
    pub anchor: Option<i32>,
    pub center: Vec3,
    pub health: f32,
    pub hurt: Option<(i32, i64)>,
}

impl Observation {
    pub(crate) fn from_entity(entity: Entity<'_>) -> Option<Self> {
        let mobs = MOBS.get()?;
        let body = entity.body()?;
        Some(Self {
            uuid: mobs.uuid(entity)?,
            anchor: entity.vehicle(),
            center: std::array::from_fn(|i| (body.min[i] + body.max[i]) * 0.5),
            health: mobs.health(entity)?.0,
            hurt: mobs.last_hurt_by_mob(entity),
        })
    }
}

struct Memory {
    uuid: Uuid,
    seen: u64,
    habits: [Habits; 4],
}

#[derive(Clone, Copy)]
struct Sample {
    observation: Observation,
    tick: u64,
}

struct Trial {
    token: u64,
    projectile: bool,
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
pub(crate) enum Outcome {
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
    motion_confidence: f64,
    trial: Option<Trial>,
    pending: Vec<(Trial, Outcome, u64, Option<i32>)>,
    seen: u64,
}

impl Learner {
    fn reset_motion(&mut self) {
        self.sample = None;
        self.velocity = [0.0; 3];
        self.samples = 0;
        self.motion_confidence = 0.0;
    }

    fn observe(&mut self, now: u64, observation: Option<Observation>) {
        self.seen = now;
        self.memory
            .retain(|m| now.saturating_sub(m.seen) <= MEMORY_TICKS);
        for (trial, _, _, _) in &mut self.pending {
            if observation.is_none_or(|obs| {
                obs.uuid != trial.target || obs.anchor != trial.start.observation.anchor
            }) {
                trial.obscured = true;
            }
        }
        self.expire(now);
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
                    let error =
                        crate::flight::length_squared(offset(measured, self.velocity)).sqrt();
                    let speed = crate::flight::length_squared(measured).sqrt();
                    let agreement = 1.0 - (error / (0.15 + speed)).clamp(0.0, 1.0);
                    self.motion_confidence = if self.samples < 2 {
                        0.0
                    } else {
                        self.motion_confidence * 0.6 + agreement * 0.4
                    };
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
        self.expectation_kind(now, aerial, false)
    }

    fn expectation_kind(&mut self, now: u64, aerial: bool, projectile: bool) -> Expectation {
        let Some(sample) = self.sample else {
            return Expectation::default();
        };
        self.memory
            .iter_mut()
            .find(|m| m.uuid == sample.observation.uuid)
            .map_or_else(Expectation::default, |m| {
                m.habits[usize::from(aerial) + 2 * usize::from(projectile)].expectation(now)
            })
    }

    fn predict(&self, now: u64, horizon: f64, max_lead: f64, max_horizon: f64) -> Option<Vec3> {
        let sample = self
            .sample
            .filter(|s| now.saturating_sub(s.tick) <= SAMPLE_TICKS)?;
        let confidence = self.motion_confidence * (f64::from(self.samples) / 4.0).min(1.0);
        let ticks = now.saturating_sub(sample.tick) as f64 + horizon.clamp(0.0, max_horizon);
        let mut lead = self.velocity.map(|v| v * ticks * confidence);
        let len = crate::flight::length_squared(lead).sqrt();
        let limit = max_lead.clamp(0.0, 12.0);
        if len > limit {
            lead = lead.map(|v| v * limit / len);
        }
        Some(crate::flight::add(sample.observation.center, lead))
    }

    fn begin(&mut self, now: u64, position: Vec3, aerial: bool, projectile: bool) -> u64 {
        self.trial = None;
        let Some(start) = self
            .sample
            .filter(|s| now.saturating_sub(s.tick) <= SAMPLE_TICKS)
        else {
            return 0;
        };
        if self.samples < 3 {
            return 0;
        }
        let d = offset(start.observation.center, position);
        let len = d[0].hypot(d[2]);
        if len < 1e-3 {
            return 0;
        }
        let expected = self.expectation_kind(now, aerial, projectile);
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let token = NEXT.fetch_add(1, Ordering::Relaxed);
        self.trial = Some(Trial {
            token,
            projectile,
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
        token
    }

    fn finish(&mut self, now: u64, outcome: Outcome) {
        let Some(t) = self.trial.take() else { return };
        self.finish_trial(now, t, outcome);
    }

    fn defer(&mut self, now: u64, outcome: Outcome, lifetime: u64, projectile: Option<i32>) {
        if let Some(t) = self.trial.take() {
            if self.pending.len() >= 8 {
                let (old, _, _, _) = self.pending.remove(0);
                self.finish_trial(now, old, Outcome::Cancelled);
            }
            self.pending
                .push((t, outcome, now.saturating_add(lifetime), projectile));
        }
    }

    fn expire(&mut self, now: u64) {
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].2 <= now {
                let (trial, outcome, _, _) = self.pending.swap_remove(i);
                self.finish_trial(now, trial, outcome);
            } else {
                i += 1;
            }
        }
    }

    fn finish_trial(&mut self, now: u64, t: Trial, outcome: Outcome) {
        let Some(mem) = self.memory.iter_mut().find(|m| m.uuid == t.target) else {
            return;
        };
        let habits = &mut mem.habits[usize::from(t.aerial) + 2 * usize::from(t.projectile)];
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

pub(crate) fn observe(mob: Entity<'_>, obs: Option<Observation>, aerial: bool) -> Expectation {
    with(mob, |l, now| {
        l.observe(now, obs);
        l.expectation(now, aerial)
    })
}
pub(crate) fn begin(mob: Entity<'_>, pos: Vec3, aerial: bool) -> u64 {
    with(mob, |l, now| l.begin(now, pos, aerial, false))
}
pub(crate) fn begin_projectile(mob: Entity<'_>, pos: Vec3, aerial: bool) -> u64 {
    with(mob, |l, now| l.begin(now, pos, aerial, true))
}
pub(crate) fn projectile_expectation(mob: Entity<'_>, aerial: bool) -> Expectation {
    with(mob, |l, now| l.expectation_kind(now, aerial, true))
}
pub(crate) fn defer(mob: Entity<'_>, outcome: Outcome, lifetime: u64) {
    with(mob, |l, now| l.defer(now, outcome, lifetime, None));
}
pub(crate) fn defer_projectile(mob: Entity<'_>, id: i32, lifetime: u64) {
    with(mob, |l, now| {
        l.defer(now, Outcome::Cancelled, lifetime, Some(id))
    });
}
pub(crate) fn projectile_token(mob: Entity<'_>, id: i32) -> u64 {
    with(mob, |l, _| {
        l.pending
            .iter()
            .find(|(_, _, _, projectile)| *projectile == Some(id))
            .map_or(0, |(t, _, _, _)| t.token)
    })
}
pub(crate) fn expire(mob: Entity<'_>) {
    with(mob, |l, now| l.expire(now));
}
pub(crate) fn token(mob: Entity<'_>) -> u64 {
    with(mob, |l, _| l.trial.as_ref().map_or(0, |t| t.token))
}
pub(crate) fn contact_token(mob: Entity<'_>, token: u64, target: Uuid, hit: bool) {
    if token == 0 {
        return;
    }
    with(mob, |l, _| {
        let trial = l
            .trial
            .iter_mut()
            .chain(l.pending.iter_mut().map(|(t, _, _, _)| t))
            .find(|t| t.token == token && t.target == target);
        if let Some(t) = trial {
            t.contact = true;
            t.hit |= hit;
        }
    });
}
pub(crate) fn settle(mob: Entity<'_>, token: u64, outcome: Outcome) {
    if token == 0 {
        return;
    }
    with(mob, |l, now| {
        if let Some(index) = l.pending.iter().position(|(t, _, _, _)| t.token == token) {
            let (trial, _, _, _) = l.pending.swap_remove(index);
            l.finish_trial(now, trial, outcome);
        }
    });
}
pub(crate) fn release(mob: Entity<'_>) {
    with(mob, |l, _| {
        if let Some(t) = &mut l.trial {
            t.released = true;
        }
    });
}
pub(crate) fn predict(mob: Entity<'_>, horizon: f64, lead: f64) -> Option<Vec3> {
    let max_horizon = if MOBS.get().and_then(|m| m.kind(mob)) == Some("saintsdragons:ignivorus") {
        20.0
    } else {
        8.0
    };
    with(mob, |l, now| l.predict(now, horizon, lead, max_horizon))
}
pub(crate) fn contact(mob: Entity<'_>, target: Uuid, hit: bool) {
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
pub(crate) fn finish(mob: Entity<'_>, outcome: Outcome) {
    with(mob, |l, now| l.finish(now, outcome));
}
pub(crate) fn clear(mob: Entity<'_>) {
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
    fn prediction_is_bounded_and_disappears_when_sight_is_lost() {
        let mut l = Learner::default();
        for tick in (0..=20).step_by(2) {
            l.observe(
                tick,
                Some(Observation {
                    uuid: [3; 16],
                    anchor: None,
                    center: [20.0 + tick as f64 * 0.2, 0.0, 0.0],
                    health: 100.0,
                    hurt: None,
                }),
            );
        }
        let predicted = l.predict(20, 8.0, 1.0, 8.0).unwrap();
        assert!(predicted[0] > 24.0 && predicted[0] <= 25.0);
        l.observe(21, None);
        assert!(l.predict(21, 8.0, 1.0, 8.0).is_none());
    }

    #[test]
    fn deferred_projectile_result_is_separate_from_current_breath() {
        let mut l = Learner::default();
        let obs = Observation {
            uuid: [4; 16],
            anchor: None,
            center: [20.0, 0.0, 0.0],
            health: 100.0,
            hurt: None,
        };
        for tick in [0, 2, 4] {
            l.observe(tick, Some(obs));
        }
        let projectile = l.begin(4, [0.0; 3], false, true);
        l.trial.as_mut().unwrap().released = true;
        l.defer(4, Outcome::Cancelled, 30, Some(12));
        let breath = l.begin(4, [0.0; 3], false, false);
        assert_ne!(projectile, breath);
        assert_eq!(l.pending[0].3, Some(12));
        l.pending[0].0.hit = true;
        l.expire(34);
        assert_eq!(l.memory[0].habits[2].outcomes, 1);
        assert_eq!(l.memory[0].habits[0].outcomes, 0);
        assert_eq!(l.trial.as_ref().unwrap().token, breath);
    }

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
            l.begin(start + 4, [0.0; 3], false, false);
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
        l.begin(4, [0.0; 3], false, false);
        l.trial.as_mut().unwrap().contact = true;
        l.finish(10, Outcome::Blocked);
        assert_eq!(l.memory[0].habits[0].outcomes, 0);
        l.begin(4, [0.0; 3], false, false);
        l.finish(10, Outcome::Cancelled);
        assert_eq!(l.memory[0].habits[0].outcomes, 0);
    }
}
