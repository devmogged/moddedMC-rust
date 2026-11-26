//! BoilerData's server-side water sampling and output limits.

use std::collections::HashMap;

use moddedmc_api::{
    Pos,
    content::{self, ContentEvent, World},
};

use super::{boiler_gauge::Gauges, runtime::is_tank, tank::TankInventory, tank_runtime};
use crate::{
    CONTENT, TAGS,
    belt_tunnel::read,
    blaze_burner::{self, Heat},
    offset,
};

const SAMPLE_RATE: u8 = 5;
const SAMPLE_COUNT: usize = 10;
const WATER_SUPPLY_PER_LEVEL: u32 = 10;
const TANKS_PER_LEVEL: u32 = 4;
const MAX_LEVEL: u32 = 18;
const PASSIVE_ENGINE_EFFICIENCY: f32 = 0.125;

#[derive(Debug, Default)]
pub(super) struct Boiler {
    pub(super) engines: u32,
    whistles: u32,
    pub(super) active_heat: u32,
    pub(super) passive_heat: bool,
    pub(super) supply: f32,
    gathered: u32,
    samples: [f32; SAMPLE_COUNT],
    next: usize,
    until_sample: u8,
    attachments: Attachments,
    gauges: Gauges,
}

impl Boiler {
    pub(super) fn restore(supply: f32) -> Self {
        if !supply.is_finite() || supply < 0.0 {
            return Self::default();
        }
        Self {
            supply,
            samples: [supply.trunc(); SAMPLE_COUNT],
            ..Self::default()
        }
    }

    pub(super) fn receive_water(&mut self, amount: u32) {
        self.gathered = self.gathered.saturating_add(amount);
    }

    pub(super) fn sample(&mut self) -> bool {
        self.until_sample = self.until_sample.saturating_sub(1);
        if self.until_sample > 0 {
            return false;
        }
        self.until_sample = SAMPLE_RATE;
        let previous = self.supply;
        let sample = std::mem::take(&mut self.gathered) as f32 / f32::from(SAMPLE_RATE);
        self.samples[self.next] = sample;
        self.supply = self.supply.max(sample);
        self.next = (self.next + 1) % SAMPLE_COUNT;
        if self.next == 0 {
            self.supply = self.samples.into_iter().fold(0.0, f32::max);
        }
        self.supply != previous
    }

    pub(super) fn engine_efficiency(&self, volume: u32) -> f32 {
        if self.engines == 0 {
            return 0.0;
        }
        let (size, water) = self.limits(volume);
        if self.is_passive(size, water) {
            return PASSIVE_ENGINE_EFFICIENCY / self.engines as f32;
        }
        let actual_heat = self.active_heat.min(size).min(water);
        actual_heat.min(self.engines) as f32 / self.engines as f32
    }

    fn limits(&self, volume: u32) -> (u32, u32) {
        (
            (volume / TANKS_PER_LEVEL).min(MAX_LEVEL),
            (self.supply.ceil() as u32 / WATER_SUPPLY_PER_LEVEL).min(MAX_LEVEL),
        )
    }

    fn is_passive(&self, size: u32, water: u32) -> bool {
        self.active_heat == 0 && self.passive_heat && size > 0 && water > 0
    }

    fn gauge_target(&self, volume: u32) -> f32 {
        let (size, water) = self.limits(volume);
        if self.is_passive(size, water) {
            PASSIVE_ENGINE_EFFICIENCY
        } else if size == 0 {
            0.0
        } else {
            self.active_heat.min(size).min(water) as f32 / size as f32
        }
    }
}

const STEAM_ENGINE_CAPACITY: f32 = 1024.0;
const MINIMUM_SPEED: i32 = 16;

/// PoweredShaftBlockEntity's generated RPM and added capacity per RPM.
pub(crate) fn shaft_output(efficiency: f32, direction: i32) -> (i32, f32) {
    if efficiency <= 0.0 {
        return (0, 0.0);
    }
    let modifier = if efficiency >= 1.0 {
        4
    } else {
        1 + (efficiency * 4.0).floor().min(2.0) as i32
    };
    let speed = direction.signum() * MINIMUM_SPEED * modifier;
    let capacity = efficiency * STEAM_ENGINE_CAPACITY / modifier as f32;
    (speed, capacity)
}

#[derive(Default, serde::Deserialize, serde::Serialize)]
#[serde(default, rename_all = "PascalCase")]
struct Saved {
    supply: f32,
    active_heat: u32,
    passive_heat: bool,
    engines: u32,
    whistles: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    engine_positions: Vec<fastnbt::IntArray>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    whistle_positions: Vec<fastnbt::IntArray>,
}

impl Boiler {
    fn read(world: World<'_>, owner: Pos) -> Self {
        tank_runtime::read_tag(world, owner)
            .and_then(|tag| Self::from_tag(tag.get("Boiler")?))
            .unwrap_or_default()
    }

    fn from_tag(tag: &fastnbt::Value) -> Option<Self> {
        let saved: Saved = fastnbt::from_value(tag).ok()?;
        Some(Self {
            active_heat: saved.active_heat,
            passive_heat: saved.passive_heat,
            engines: saved.engines,
            whistles: saved.whistles,
            attachments: Attachments {
                engines: unpack_positions(&saved.engine_positions),
                whistles: unpack_positions(&saved.whistle_positions),
            },
            ..Self::restore(saved.supply)
        })
    }

    fn stats(&self) -> (u32, u32, u32, bool, f32) {
        (
            self.engines,
            self.whistles,
            self.active_heat,
            self.passive_heat,
            self.supply,
        )
    }

    fn write(&self, world: World<'_>, owner: Pos) -> bool {
        let Some(mut tag) = tank_runtime::read_tag(world, owner) else {
            return false;
        };
        let saved = Saved {
            supply: self.supply,
            active_heat: self.active_heat,
            passive_heat: self.passive_heat,
            engines: self.engines,
            whistles: self.whistles,
            engine_positions: pack_positions(&self.attachments.engines),
            whistle_positions: pack_positions(&self.attachments.whistles),
        };
        let Ok(saved) = fastnbt::to_value(saved) else {
            return false;
        };
        tag.insert("Boiler".to_owned(), saved);
        super::smart_tank::write(world, owner, &tag)
    }
}

fn pack_positions(positions: &[Pos]) -> Vec<fastnbt::IntArray> {
    positions
        .iter()
        .map(|pos| fastnbt::IntArray::new(vec![pos.x, pos.y, pos.z]))
        .collect()
}

fn unpack_positions(positions: &[fastnbt::IntArray]) -> Vec<Pos> {
    positions
        .iter()
        .filter_map(|packed| {
            let &[x, y, z] = packed.as_ref() else {
                return None;
            };
            Some(Pos { x, y, z })
        })
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) enum Attachment {
    Engine,
    Whistle,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Attachments {
    pub(crate) engines: Vec<Pos>,
    pub(crate) whistles: Vec<Pos>,
}

impl Attachments {
    fn kinds(&self) -> [(Attachment, &[Pos]); 2] {
        [
            (Attachment::Engine, &self.engines),
            (Attachment::Whistle, &self.whistles),
        ]
    }

    fn list(&mut self, kind: Attachment) -> &mut Vec<Pos> {
        match kind {
            Attachment::Engine => &mut self.engines,
            Attachment::Whistle => &mut self.whistles,
        }
    }

    fn is_empty(&self) -> bool {
        self.engines.is_empty() && self.whistles.is_empty()
    }
}

pub(crate) type Controllers = HashMap<Pos, (TankInventory, Attachments)>;

#[derive(Default)]
pub(crate) struct Boilers {
    loaded: HashMap<Pos, Boiler>,
}

impl Boilers {
    pub(crate) fn active(&self, owner: Pos) -> bool {
        self.loaded
            .get(&owner)
            .is_some_and(|boiler| boiler.engines > 0 || boiler.whistles > 0)
    }

    pub(crate) fn heated(&self, owner: Pos) -> bool {
        self.loaded
            .get(&owner)
            .is_some_and(|boiler| boiler.active_heat > 0 || boiler.passive_heat)
    }

    pub(crate) fn receive(&mut self, owner: Pos, amount: u32) -> bool {
        let Some(boiler) = self
            .loaded
            .get_mut(&owner)
            .filter(|boiler| boiler.engines > 0 || boiler.whistles > 0)
        else {
            return false;
        };
        boiler.receive_water(amount);
        true
    }

    pub(crate) fn loaded_tanks(&mut self, world: World<'_>, events: &[ContentEvent]) {
        for event in events {
            if event.reason == content::LOADED
                && is_tank(event.state)
                && !self.loaded.contains_key(&event.position)
                && let Some(tag) = tank_runtime::read_tag(world, event.position)
                && !tag.contains_key("Controller")
                && let Some(boiler) = tag.get("Boiler").and_then(Boiler::from_tag)
                && (boiler.engines > 0 || boiler.whistles > 0)
                && !boiler.attachments.is_empty()
            {
                self.loaded.insert(event.position, boiler);
            }
        }
    }

    pub(crate) fn tick(
        &mut self,
        world: World<'_>,
        controllers: &mut Controllers,
        can_still_attach: impl Fn(Pos, Pos, Attachment) -> bool,
        time: u64,
    ) -> HashMap<Pos, f32> {
        self.loaded.retain(|&owner, boiler| {
            if controllers.contains_key(&owner) {
                return true;
            }
            if let Some(tank) = tank_runtime::load(world, owner) {
                if boiler.attachments.kinds().iter().any(|&(kind, positions)| {
                    positions
                        .iter()
                        .any(|&pos| can_still_attach(owner, pos, kind))
                }) {
                    controllers.insert(owner, (tank, Attachments::default()));
                    return true;
                }
                boiler.engines = 0;
                boiler.whistles = 0;
                boiler.attachments = Attachments::default();
                boiler.write(world, owner);
            }
            boiler.gauges.clear(world);
            false
        });
        let mut efficiency = HashMap::with_capacity(controllers.len());
        for (&owner, (tank, attached)) in controllers {
            let boiler = self
                .loaded
                .entry(owner)
                .or_insert_with(|| Boiler::read(world, owner));
            let before = boiler.stats();
            for (kind, previous) in boiler.attachments.kinds() {
                let current = attached.list(kind);
                for &pos in previous {
                    if !current.contains(&pos) && can_still_attach(owner, pos, kind) {
                        current.push(pos);
                    }
                }
                current.sort_unstable_by_key(|pos| (pos.x, pos.y, pos.z));
                current.dedup();
            }
            let moved = *attached != boiler.attachments;
            if moved {
                boiler.attachments.clone_from(attached);
            }
            boiler.engines = attached.engines.len() as u32;
            boiler.whistles = attached.whistles.len() as u32;
            if tank.window() {
                tank.set_window(false);
                if !tank_runtime::update_groups(world, &[(owner, tank.clone())]) {
                    tank.set_window(true);
                }
            }
            let (active, passive) = heat_below(world, owner, tank.width());
            boiler.active_heat = active;
            boiler.passive_heat = passive && active == 0;
            boiler.sample();
            if moved || before != boiler.stats() {
                boiler.write(world, owner);
            }
            let volume = tank.capacity() / tank_runtime::CAPACITY_PER_BLOCK;
            let target = boiler.gauge_target(volume);
            boiler
                .gauges
                .update(world, owner, time, tank.width(), target);
            efficiency.insert(owner, boiler.engine_efficiency(volume));
        }
        efficiency
    }
}

fn heat_below(world: World<'_>, owner: Pos, width: u8) -> (u32, bool) {
    let (Some(content), Some(tags)) = (CONTENT.get(), TAGS.get()) else {
        return (0, false);
    };
    let mut active = 0;
    let mut passive = false;
    for x in 0..i32::from(width) {
        for z in 0..i32::from(width) {
            let pos = offset(owner, [x, -1, z]);
            let state = read(world, pos);
            if blaze_burner::is_burner(state) {
                match blaze_burner::heat_at(world, pos) {
                    Heat::None => {}
                    Heat::Smouldering => passive = true,
                    Heat::Fading | Heat::Kindled => active += 1,
                    Heat::Seething => active += 2,
                }
                continue;
            }
            if let Some(selector) = content.describe_state(state) {
                let name = selector
                    .split_once('[')
                    .map_or(selector.as_str(), |(id, _)| id);
                passive |= tags.contains("block", "create:passive_boiler_heaters", name)
                    && !selector.contains("lit=false");
            }
        }
    }
    (active, passive)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boiler_output_is_limited_by_water_size_and_heat() {
        let mut boiler = Boiler {
            engines: 2,
            passive_heat: true,
            ..Boiler::restore(29.1)
        };
        assert_eq!(boiler.engine_efficiency(3), 0.0);
        assert_eq!(boiler.engine_efficiency(12), 0.0625);
        boiler.active_heat = 3;
        assert_eq!(boiler.engine_efficiency(4), 0.5);
        assert_eq!(boiler.engine_efficiency(12), 1.0);
        boiler.supply = 19.0;
        assert_eq!(boiler.engine_efficiency(12), 0.5);
    }

    #[test]
    fn powered_shaft_speed_changes_without_inflating_total_stress_capacity() {
        assert_eq!(shaft_output(0.0, 1), (0, 0.0));
        assert_eq!(shaft_output(0.125, 1), (16, 128.0));
        assert_eq!(shaft_output(0.25, -1), (-32, 128.0));
        let (rpm, capacity) = shaft_output(0.75, 1);
        assert_eq!((rpm, rpm as f32 * capacity), (48, 12_288.0));
        assert_eq!(shaft_output(1.0, 1), (64, 256.0));
    }

    #[test]
    fn water_peaks_age_out_after_the_live_sample_window() {
        let mut boiler = Boiler::default();
        boiler.receive_water(100);
        assert!(boiler.sample());
        assert_eq!(boiler.supply, 20.0);
        for _ in 0..50 {
            assert!(!boiler.sample());
        }
        for _ in 0..44 {
            assert!(!boiler.sample());
        }
        assert!(boiler.sample());
        assert_eq!(boiler.supply, 0.0);
    }
}
