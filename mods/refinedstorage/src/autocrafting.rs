//! Source v2.0.9's Pattern Grid and Autocrafter use actual recipe identities,
//! not synthetic outputs. This initial native subset stores nine installed
//! crafting patterns and a bounded queue per pattern in one block-entity CAS.
//! Executing a job consumes inputs and inserts the real recipe result through
//! one digital storage CAS. It never impersonates arbitrary external machines.

use crate::{craft::Pattern, stack::Stack};
use fastnbt::{ByteArray, Value};
use moddedmc_api::{Pos, content::World, crafting::RecipeMatch};
use std::collections::HashMap;

type Compound = HashMap<String, Value>;
const KEY: &str = "refinedstorage_autocrafting";
const MAX_JOBS: usize = 9;
const MAX_PENDING: u32 = 1024;

#[derive(Clone, Debug)]
pub struct RecipePattern {
    pub grid: Pattern,
    pub result: Stack,
    pub id: Option<String>,
    pub serializer: String,
}

impl RecipePattern {
    pub fn from_live(grid: Pattern, recipe: &RecipeMatch) -> Option<Self> {
        if grid.slots.iter().all(Option::is_none) {
            return None;
        }
        let mut grid = grid;
        if !grid.remember_recipe(recipe) {
            return None;
        }
        let result = Stack::parse(&recipe.result)?;
        if recipe.serializer.is_empty()
            || recipe.serializer.len() > 256
            || recipe
                .id
                .as_ref()
                .is_some_and(|id| id.is_empty() || id.len() > 256)
        {
            return None;
        }
        Some(Self {
            grid,
            result,
            id: recipe.id.clone(),
            serializer: recipe.serializer.clone(),
        })
    }

    pub fn matches(&self, live: &RecipeMatch) -> bool {
        let Some(result) = Stack::parse(&live.result) else {
            return false;
        };
        self.result.same_item(&result)
            && self.result.count == result.count
            && self.id == live.id
            && self.serializer == live.serializer
    }

    fn fields(&self) -> Option<Compound> {
        let mut grid = Compound::new();
        self.grid.encode(&mut grid)?;
        let mut result = self.result.key.clone();
        result.insert(
            "count".into(),
            Value::Int(i32::try_from(self.result.count).ok()?),
        );
        let mut fields = Compound::from([
            ("version".into(), Value::Int(1)),
            ("grid".into(), Value::Compound(grid)),
            ("result".into(), Value::Compound(result)),
            ("serializer".into(), Value::String(self.serializer.clone())),
        ]);
        if let Some(id) = &self.id {
            fields.insert("id".into(), Value::String(id.clone()));
        }
        Some(fields)
    }

    fn decode(fields: &Compound) -> Option<Self> {
        if fields.get("version") != Some(&Value::Int(1)) {
            return None;
        }
        let (
            Some(Value::Compound(grid)),
            Some(Value::Compound(result)),
            Some(Value::String(serializer)),
        ) = (
            fields.get("grid"),
            fields.get("result"),
            fields.get("serializer"),
        )
        else {
            return None;
        };
        let id = match fields.get("id") {
            None => None,
            Some(Value::String(id)) if !id.is_empty() && id.len() <= 256 => Some(id.clone()),
            _ => return None,
        };
        let recipe = RecipeMatch {
            result: fastnbt::to_bytes(result).ok()?,
            id,
            serializer: serializer.clone(),
        };
        Self::from_live(Pattern::decode(grid)?, &recipe)
    }

    /// This is stored in the Pattern item's mod data, not an arbitrary
    /// unrestricted NBT payload. Every load revalidates bounds and structure.
    pub fn item_data(&self) -> Option<Vec<u8>> {
        let data = fastnbt::to_bytes(&self.fields()?).ok()?;
        (data.len() <= moddedmc_api::interaction::MAX_STACK_DATA).then_some(data)
    }
    pub fn from_item_data(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > moddedmc_api::interaction::MAX_STACK_DATA {
            return None;
        }
        let fields: Compound = fastnbt::from_bytes(bytes).ok()?;
        Self::decode(&fields)
    }

    pub fn item_stack(&self) -> Option<Vec<u8>> {
        let data = self.item_data()?;
        let raw = Compound::from([
            ("id".into(), Value::String("refinedstorage:pattern".into())),
            ("count".into(), Value::Int(1)),
            (
                "components".into(),
                Value::Compound(Compound::from([(
                    "minecraft:custom_data".into(),
                    Value::Compound(Compound::from([(
                        "moddedmc:data".into(),
                        Value::ByteArray(ByteArray::new(
                            data.iter().copied().map(|value| value as i8).collect(),
                        )),
                    )])),
                )])),
            ),
        ]);
        fastnbt::to_bytes(&raw).ok()
    }

    pub fn same_recipe(&self, other: &Self) -> bool {
        self.id == other.id
            && self.serializer == other.serializer
            && self.result.same_item(&other.result)
            && self.result.count == other.result.count
            && self
                .grid
                .slots
                .iter()
                .zip(other.grid.slots.iter())
                .all(|(left, right)| match (left, right) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.same_item(b) && a.count == b.count,
                    _ => false,
                })
    }
}

#[derive(Clone, Debug)]
pub struct Job {
    pub pattern: RecipePattern,
    pub pending: u32,
    /// Saved before touching storage. An uncertain crash never replays a job
    /// automatically and risks creating the same output twice.
    pub inflight: bool,
}
#[derive(Clone, Debug, Default)]
pub struct Autocrafter {
    pub jobs: Vec<Job>,
    pub needs_signal: bool,
}
impl Autocrafter {
    pub fn enabled(&self, powered: bool) -> bool {
        !self.needs_signal || powered
    }
    pub fn install(&mut self, recipe: RecipePattern) -> bool {
        if self.jobs.len() >= MAX_JOBS
            || self.jobs.iter().any(|job| job.pattern.same_recipe(&recipe))
        {
            return false;
        }
        self.jobs.push(Job {
            pattern: recipe,
            pending: 0,
            inflight: false,
        });
        true
    }
    pub fn queue(&mut self, index: usize) -> bool {
        let Some(job) = self.jobs.get_mut(index) else {
            return false;
        };
        if job.pending >= MAX_PENDING {
            return false;
        }
        job.pending += 1;
        true
    }
    pub fn dequeue(&mut self, index: usize) -> bool {
        let Some(job) = self.jobs.get_mut(index) else {
            return false;
        };
        if job.pending == 0 || job.inflight {
            return false;
        }
        job.pending -= 1;
        true
    }
    pub fn begin(&mut self, index: usize, expected: &RecipePattern) -> bool {
        let Some(job) = self.jobs.get_mut(index) else {
            return false;
        };
        if job.pending == 0 || job.inflight || !job.pattern.same_recipe(expected) {
            return false;
        }
        job.inflight = true;
        true
    }
    pub fn finish(&mut self, index: usize, expected: &RecipePattern) -> bool {
        let Some(job) = self.jobs.get_mut(index) else {
            return false;
        };
        if job.pending == 0 || !job.inflight || !job.pattern.same_recipe(expected) {
            return false;
        }
        job.pending -= 1;
        job.inflight = false;
        true
    }
    /// Only for cases where no storage change was committed.
    pub fn abandon_without_mutation(&mut self, index: usize, expected: &RecipePattern) -> bool {
        let Some(job) = self.jobs.get_mut(index) else {
            return false;
        };
        if !job.inflight || !job.pattern.same_recipe(expected) {
            return false;
        }
        job.inflight = false;
        true
    }
    /// Explicit user cancellation forfeits uncertain queued work, preserving
    /// any output already produced without repeating its consumption.
    pub fn cancel_uncertain(&mut self, index: usize) -> bool {
        let Some(job) = self.jobs.get_mut(index) else {
            return false;
        };
        if !job.inflight {
            return false;
        }
        job.pending = 0;
        job.inflight = false;
        true
    }
    pub fn eject(&mut self, index: usize) -> Option<RecipePattern> {
        if self.jobs.get(index)?.pending > 0 {
            return None;
        }
        Some(self.jobs.remove(index).pattern)
    }
    pub fn decode(root: &Compound) -> Option<Self> {
        let Some(saved) = root.get(KEY) else {
            return Some(Self::default());
        };
        let Value::Compound(saved) = saved else {
            return None;
        };
        let (Some(Value::Int(1)), Some(Value::List(entries))) =
            (saved.get("version"), saved.get("jobs"))
        else {
            return None;
        };
        if entries.len() > MAX_JOBS {
            return None;
        }
        let mut jobs = Vec::with_capacity(entries.len());
        for entry in entries {
            let Value::Compound(fields) = entry else {
                return None;
            };
            let (Some(Value::Int(pending)), Some(Value::Compound(pattern))) =
                (fields.get("pending"), fields.get("pattern"))
            else {
                return None;
            };
            let pending = u32::try_from(*pending).ok()?;
            if pending > MAX_PENDING {
                return None;
            }
            let pattern = RecipePattern::decode(pattern)?;
            if jobs
                .iter()
                .any(|job: &Job| job.pattern.same_recipe(&pattern))
            {
                return None;
            }
            let inflight = match fields.get("inflight") {
                None => false,
                Some(Value::Byte(flag)) => *flag != 0,
                _ => return None,
            };
            if inflight && pending == 0 {
                return None;
            }
            jobs.push(Job {
                pending,
                pattern,
                inflight,
            });
        }
        let needs_signal = match saved.get("needsSignal") {
            None => false,
            Some(Value::Byte(enabled)) => *enabled != 0,
            _ => return None,
        };
        Some(Self { jobs, needs_signal })
    }
    pub fn encode(&self, root: &mut Compound) -> Option<()> {
        if self.jobs.len() > MAX_JOBS {
            return None;
        }
        let mut entries = Vec::with_capacity(self.jobs.len());
        for job in &self.jobs {
            if job.pending > MAX_PENDING {
                return None;
            }
            entries.push(Value::Compound(Compound::from([
                (
                    "pending".into(),
                    Value::Int(i32::try_from(job.pending).ok()?),
                ),
                ("inflight".into(), Value::Byte(i8::from(job.inflight))),
                ("pattern".into(), Value::Compound(job.pattern.fields()?)),
            ])));
        }
        root.insert(
            KEY.into(),
            Value::Compound(Compound::from([
                ("version".into(), Value::Int(1)),
                (
                    "needsSignal".into(),
                    Value::Byte(i8::from(self.needs_signal)),
                ),
                ("jobs".into(), Value::List(entries)),
            ])),
        );
        Some(())
    }
}

pub fn load(world: World<'_>, site: Pos) -> Option<Autocrafter> {
    let snapshot = world.entity_snapshot(site)?;
    let root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Autocrafter::decode(&root)
}
pub fn update<T>(
    world: World<'_>,
    site: Pos,
    mutate: impl FnOnce(&mut Autocrafter) -> Option<T>,
) -> Option<T> {
    let snapshot = world.entity_snapshot(site)?;
    let mut root: Compound = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    let mut autocrafter = Autocrafter::decode(&root)?;
    let out = mutate(&mut autocrafter)?;
    autocrafter.encode(&mut root)?;
    let bytes = fastnbt::to_bytes(&root).ok()?;
    world.compare_entity(site, &snapshot, &bytes).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(id: &str) -> Stack {
        Stack::parse(
            &fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(1)),
            ]))
            .unwrap(),
        )
        .unwrap()
    }
    fn recipe() -> RecipePattern {
        let mut grid = Pattern::default();
        for _ in 0..9 {
            assert!(grid.add(&item("minecraft:diamond")));
        }
        RecipePattern::from_live(
            grid,
            &RecipeMatch {
                id: Some("minecraft:diamond_block".into()),
                serializer: "minecraft:crafting_shaped".into(),
                result: item("minecraft:diamond_block").raw,
            },
        )
        .unwrap()
    }
    #[test]
    fn pattern_item_preserves_exact_3x3_recipe_and_identity() {
        let pattern = recipe();
        let item = pattern.item_data().unwrap();
        let restored = RecipePattern::from_item_data(&item).unwrap();
        assert!(restored.same_recipe(&pattern));
        let item_stack = pattern.item_stack().unwrap();
        let item = Stack::parse(&item_stack).unwrap();
        assert_eq!(item.id, "refinedstorage:pattern");
        let Value::Compound(components) = item.key.get("components").unwrap() else {
            unreachable!()
        };
        let Value::Compound(data) = components.get("minecraft:custom_data").unwrap() else {
            unreachable!()
        };
        let Value::ByteArray(bytes) = data.get("moddedmc:data").unwrap() else {
            unreachable!()
        };
        assert!(
            RecipePattern::from_item_data(&bytes.iter().map(|&b| b as u8).collect::<Vec<_>>())
                .unwrap()
                .same_recipe(&pattern)
        );
        let changed = RecipeMatch {
            result: Stack::parse(&pattern.result.raw)
                .unwrap()
                .with_count(2)
                .unwrap(),
            id: pattern.id.clone(),
            serializer: pattern.serializer.clone(),
        };
        assert!(!pattern.matches(&changed));
        assert!(RecipePattern::from_item_data(b"corrupt").is_none());
    }
    #[test]
    fn queued_jobs_survive_restart_but_duplicates_and_busy_eject_fail() {
        let mut state = Autocrafter::default();
        let pattern = recipe();
        assert!(state.install(pattern.clone()));
        assert!(!state.install(pattern.clone()));
        assert!(state.queue(0));
        assert!(state.queue(0));
        assert!(state.eject(0).is_none());
        let mut nbt = Compound::from([("foreign".into(), Value::Int(77))]);
        state.encode(&mut nbt).unwrap();
        let roundtrip: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&nbt).unwrap()).unwrap();
        let mut restored = Autocrafter::decode(&roundtrip).unwrap();
        assert_eq!(roundtrip.get("foreign"), Some(&Value::Int(77)));
        assert_eq!(restored.jobs[0].pending, 2);
        assert!(restored.dequeue(0));
        assert!(restored.queue(0));
        assert!(restored.begin(0, &pattern));
        assert!(!restored.begin(0, &pattern));
        assert!(restored.finish(0, &pattern));
        assert!(restored.begin(0, &pattern));
        assert!(restored.finish(0, &pattern));
        assert!(!restored.finish(0, &pattern));
        assert!(restored.eject(0).unwrap().same_recipe(&pattern));
    }
    #[test]
    fn invalid_queue_or_recipe_data_are_rejected() {
        let mut state = Autocrafter::default();
        assert!(state.install(recipe()));
        let mut nbt = Compound::new();
        state.encode(&mut nbt).unwrap();
        let Some(Value::Compound(saved)) = nbt.get_mut(KEY) else {
            unreachable!()
        };
        let Some(Value::List(jobs)) = saved.get_mut("jobs") else {
            unreachable!()
        };
        let Some(Value::Compound(job)) = jobs.first_mut() else {
            unreachable!()
        };
        job.insert("pending".into(), Value::Int(-1));
        assert!(Autocrafter::decode(&nbt).is_none());
    }

    #[test]
    fn uncertain_crash_job_survives_restart_and_cannot_automatically_replay() {
        let pattern = recipe();
        let mut state = Autocrafter::default();
        assert!(state.install(pattern.clone()));
        assert!(state.queue(0));
        assert!(state.begin(0, &pattern));
        let mut nbt = Compound::new();
        state.encode(&mut nbt).unwrap();
        let roundtrip: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&nbt).unwrap()).unwrap();
        let mut restored = Autocrafter::decode(&roundtrip).unwrap();
        assert!(restored.jobs[0].inflight);
        assert_eq!(restored.jobs[0].pending, 1);
        assert!(!restored.begin(0, &pattern));
        assert!(restored.cancel_uncertain(0));
        assert!(!restored.jobs[0].inflight);
        assert_eq!(restored.jobs[0].pending, 0);
    }
    #[test]
    fn autocrafter_redstone_enablement_survives_restart_and_is_opt_in() {
        let mut state = Autocrafter::default();
        assert!(state.enabled(false));
        state.needs_signal = true;
        assert!(!state.enabled(false));
        assert!(state.enabled(true));
        let mut root = Compound::new();
        state.encode(&mut root).unwrap();
        let roundtrip: Compound = fastnbt::from_bytes(&fastnbt::to_bytes(&root).unwrap()).unwrap();
        let restored = Autocrafter::decode(&roundtrip).unwrap();
        assert!(restored.needs_signal);
        assert!(!restored.enabled(false));
        let mut old = roundtrip;
        if let Some(Value::Compound(saved)) = old.get_mut(KEY) {
            saved.remove("needsSignal");
        }
        assert!(Autocrafter::decode(&old).unwrap().enabled(false));
    }
}
