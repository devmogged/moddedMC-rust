//! Create 6.0.10's DrillMovementBehaviour and BlockBreakingMovementBehaviour.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, Content, ShapeBox, State, World},
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTENT, DIRECTIONS, STONE, StateIds, TAGS,
    collider::Frame,
    kinetics::{FACES, Kind, face_axis, positive},
};

pub const ID: &str = "create:mechanical_drill";
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];
const ACTIVE_AREA_OFFSET: f64 = 0.65_f32 as f64;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let properties: Vec<_> = DIRECTIONS
        .into_iter()
        .flat_map(|face| [false, true].map(move |wet| format!("facing={face},waterlogged={wet}")))
        .collect();
    let shapes: [[ShapeBox; 1]; 6] = std::array::from_fn(|face| {
        let axis = usize::from(face_axis(face as u8));
        let mut shape = ShapeBox {
            min: [0; 3],
            max: [16; 3],
        };
        if positive(face as u8) {
            shape.max[axis] = 12;
        } else {
            shape.min[axis] = 4;
        }
        [shape]
    });
    let states: Vec<_> = properties
        .iter()
        .enumerate()
        .map(|(i, properties)| {
            let face = i / 2;
            let (x, y) = ROTATIONS[face];
            State {
                properties,
                physical: STONE,
                model: "create:block/mechanical_drill/block",
                rotating_model: "create:block/mechanical_drill/head",
                x,
                y,
                orientation: crate::relay_orientation(face_axis(face as u8)),
                shape: &shapes[face],
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Mechanical Drill",
            entity: ID,
            item_model: "create:item/mechanical_drill",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (face, direction) in DIRECTIONS.into_iter().enumerate() {
        for wet in [false, true] {
            states.add(
                content,
                ID,
                &format!("facing={direction},waterlogged={wet}"),
                Some(Kind::Drill(face as u8)),
            );
        }
    }
}

fn block_pos([x, y, z]: [i32; 3]) -> Pos {
    Pos { x, y, z }
}

pub fn breakable(world: World<'_>, position: Pos) -> Option<f32> {
    let mut states = [content::UNLOADED_STATE];
    world.read(&[position], &mut states);
    let name = CONTENT.get()?.describe_state(states[0])?;
    let block = name.split('[').next()?;
    let hardness = world.destroy_speed(position)?;
    let tags = TAGS.get()?;
    if hardness < 0.0
        || matches!(
            block,
            "minecraft:air"
                | "minecraft:cave_air"
                | "minecraft:void_air"
                | "minecraft:water"
                | "minecraft:lava"
        )
        || tags.contains("block", "create:non_breakable", block)
        || tags.contains("block", "create:tracks", block)
        || world.collision_shape(position)?.is_empty()
    {
        return None;
    }
    Some(hardness)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BreakProgress {
    progress: i32,
    ticks_until_next_progress: i32,
}

impl BreakProgress {
    /// The number of intervening ticks is truncated before the next progress increment.
    fn tick(&mut self, hardness: f32, speed: f32) -> bool {
        if self.ticks_until_next_progress > 0 {
            self.ticks_until_next_progress -= 1;
            return false;
        }
        self.progress = self.progress.clamp(0, 9);
        self.progress += ((speed / hardness) as i32).clamp(1, 10 - self.progress);
        if self.progress == 10 {
            *self = Self::default();
            return true;
        }
        self.ticks_until_next_progress = (hardness / speed) as i32;
        false
    }
}

pub enum Tick {
    Travel,
    Stalled,
    Broke(Pos, Vec<Vec<u8>>),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct DrillMovementBehaviour {
    local: [i32; 3],
    facing: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_pos: Option<[i32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    breaking_pos: Option<[i32; 3]>,
    speed: f32,
    breaker: BreakProgress,
}

impl DrillMovementBehaviour {
    pub fn new(local: [i32; 3], facing: u8) -> Self {
        Self {
            local,
            facing,
            last_pos: None,
            breaking_pos: None,
            speed: 0.0,
            breaker: BreakProgress::default(),
        }
    }

    pub fn tick(&mut self, world: World<'_>, previous: Frame, next: Frame) -> Tick {
        let Some(step) = FACES.get(usize::from(self.facing)) else {
            return Tick::Travel;
        };
        let step = step.map(f64::from);
        let point =
            std::array::from_fn(|i| f64::from(self.local[i]) + 0.5 + ACTIVE_AREA_OFFSET * step[i]);
        let (before, after) = (previous.to_world(point), next.to_world(point));
        let motion: [f64; 3] = std::array::from_fn(|i| after[i] - before[i]);
        let local_motion = next.to_local(before);
        let advance: f64 = (0..3).map(|i| (point[i] - local_motion[i]) * step[i]).sum();
        if advance < -1.0e-9 {
            self.breaking_pos = None;
            self.last_pos = None;
            self.breaker = BreakProgress::default();
            return Tick::Travel;
        }
        let speed = motion
            .into_iter()
            .map(|component| component * component)
            .sum::<f64>()
            .sqrt();
        if speed == 0.0 {
            return Tick::Travel;
        }

        let grid = after.map(|n| n.floor() as i32);
        if self.breaking_pos.is_none()
            && self.last_pos.replace(grid) != Some(grid)
            && breakable(world, block_pos(grid)).is_some()
        {
            self.breaking_pos = Some(grid);
            self.speed = block_breaking_speed(speed);
        }
        let Some(target) = self.breaking_pos.map(block_pos) else {
            return Tick::Travel;
        };
        let Some(hardness) = breakable(world, target) else {
            self.breaking_pos = None;
            self.breaker = BreakProgress::default();
            return Tick::Travel;
        };
        if !self.breaker.tick(hardness, self.speed) {
            return Tick::Stalled;
        }
        self.breaking_pos = None;
        if let Some(drops) = world.block_drops(target, None)
            && world.destroy(target, false)
        {
            Tick::Broke(target, drops)
        } else {
            Tick::Travel
        }
    }
}

fn block_breaking_speed(speed: f64) -> f32 {
    let animation_speed = if speed < 1.0 / 512.0 {
        0
    } else {
        (speed * 1000.0 + 100.0) as i32 / 100 * 100
    };
    (animation_speed as f32 / 500.0).clamp(1.0 / 128.0, 16.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_counts_intervening_ticks_and_resets_on_break() {
        let mut progress = BreakProgress::default();
        let speed = block_breaking_speed(0.25);
        assert_eq!(speed, 0.6);
        for _ in 0..36 {
            assert!(!progress.tick(2.0, speed));
        }
        assert!(progress.tick(2.0, speed));
        assert_eq!(progress.progress, 0);
    }
}
