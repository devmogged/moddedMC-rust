//! Create 6.0.10's SuperGlueEntity and SuperGlueSelectionHelper.

use std::collections::{HashMap, HashSet, VecDeque};

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    abi::CARRIER_MARKER,
    content::World,
    interaction::{UseOnBlock, UseResult},
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    ENTITIES, ITEMS,
    contraption::{BlockMovement, BlockMovementChecks, StructureTransform},
    kinetics::{FACES, offset},
    states,
};

pub const ID: &str = "create:super_glue";
const MAX_SELECTION_DISTANCE: i64 = 25;
const DURABILITY: i32 = 99;

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: ID,
                display_name: "Super Glue",
                carrier: "minecraft:shears",
                model: "create:item/super_glue",
            },
        )
}

fn coords(pos: Pos) -> [i32; 3] {
    [pos.x, pos.y, pos.z]
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct GlueBox {
    from: [f64; 3],
    to: [f64; 3],
}

impl GlueBox {
    fn span(a: Pos, b: Pos) -> Self {
        let (a, b) = (coords(a), coords(b));
        Self {
            from: std::array::from_fn(|i| f64::from(a[i].min(b[i]))),
            to: std::array::from_fn(|i| f64::from(a[i].max(b[i])) + 1.0),
        }
    }

    fn contains(self, pos: Pos) -> bool {
        coords(pos).into_iter().enumerate().all(|(i, n)| {
            let center = f64::from(n) + 0.5;
            self.from[i] <= center && center < self.to[i]
        })
    }

    fn include(&mut self, pos: Pos) {
        for (i, n) in coords(pos).map(f64::from).into_iter().enumerate() {
            self.from[i] = self.from[i].min(n);
            self.to[i] = self.to[i].max(n + 1.0);
        }
    }

    pub(crate) fn local(self, anchor: Pos) -> Self {
        let anchor = coords(anchor).map(f64::from);
        Self {
            from: std::array::from_fn(|i| self.from[i] - anchor[i]),
            to: std::array::from_fn(|i| self.to[i] - anchor[i]),
        }
    }

    fn spawn(self, world: World<'_>) -> Option<i32> {
        let position = [
            f64::midpoint(self.from[0], self.to[0]),
            self.from[1],
            f64::midpoint(self.from[2], self.to[2]),
        ];
        let data = fastnbt::to_bytes(&self).ok()?;
        ENTITIES
            .get()?
            .spawn_carrier(world, CARRIER_MARKER, position, ID, &data)
    }

    fn transformed(self, transform: StructureTransform) -> Self {
        let a = transform.apply(self.from.map(|n| n as i32));
        let b = transform.apply(self.to.map(|n| (n - 1.0) as i32));
        Self::span(a, b)
    }

    pub fn place(self, world: World<'_>, transform: StructureTransform) -> Option<i32> {
        self.transformed(transform).spawn(world)
    }
}

#[derive(Default)]
pub struct GlueScan {
    sections: HashSet<[i32; 3]>,
    markers: HashMap<i32, GlueBox>,
    used: HashSet<i32>,
}

impl GlueScan {
    pub fn is_glued(&mut self, world: Option<World<'_>>, pos: Pos, adjacent: Pos) -> bool {
        let (Some(world), Some(entities)) = (world, ENTITIES.get()) else {
            return false;
        };
        let section = coords(pos).map(|n| n.div_euclid(16));
        if self.sections.insert(section) {
            // Markers have point bounds. Include every origin of a selection touching this section.
            let min = section.map(|n| f64::from(n * 16 - MAX_SELECTION_DISTANCE as i32));
            let max = section.map(|n| f64::from(n * 16 + 16 + MAX_SELECTION_DISTANCE as i32));
            for id in entities.entities_matching(world, min, max, -1, 0) {
                if self.markers.contains_key(&id) {
                    continue;
                }
                if let Some(box_) = entities
                    .with_entity(world, id, |entity| {
                        if entity.type_id() != "marker" {
                            return None;
                        }
                        fastnbt::from_bytes::<GlueBox>(&entity.data(ID)?).ok()
                    })
                    .flatten()
                {
                    self.markers.insert(id, box_);
                }
            }
        }
        if let Some((&id, _)) = self
            .markers
            .iter()
            .filter(|(_, glue)| glue.contains(pos) && glue.contains(adjacent))
            .min_by_key(|&(id, _)| (!self.used.contains(id), *id))
        {
            self.used.insert(id);
            true
        } else {
            false
        }
    }

    pub fn capture(
        self,
        anchor: Pos,
        positions: impl Iterator<Item = Pos> + Clone,
    ) -> (Vec<GlueBox>, Vec<i32>) {
        let mut used: Vec<_> = self.used.into_iter().collect();
        used.sort_unstable();
        let boxes = used
            .iter()
            .filter_map(|id| {
                let original = self.markers[id];
                let mut inside = positions.clone().filter(|pos| original.contains(*pos));
                let first = inside.next()?;
                let mut cropped = GlueBox::span(first, inside.next()?);
                for pos in inside {
                    cropped.include(pos);
                }
                Some(cropped.local(anchor))
            })
            .collect();
        (boxes, used)
    }
}

pub fn discard(world: World<'_>, id: i32) {
    if let Some(entities) = ENTITIES.get() {
        entities.with_entity(world, id, |entity| entity.discard());
    }
}

fn search_glue_group(world: World<'_>, checks: &BlockMovementChecks, start: Pos, end: Pos) -> bool {
    let area = GlueBox::span(start, end);
    let mut visited = HashSet::from([start]);
    let mut frontier = VecDeque::from([start]);
    while let Some(current) = frontier.pop_front() {
        if current == end {
            return true;
        }
        let Some(BlockMovement::Movable(block)) = checks.classify(world, current) else {
            continue;
        };
        for (face, step) in FACES.into_iter().enumerate() {
            let adjacent = offset(current, step);
            if !area.contains(adjacent) || visited.contains(&adjacent) {
                continue;
            }
            if let Some(BlockMovement::Movable(neighbor)) = checks.classify(world, adjacent)
                && !block.is_not_supportive(face as u8)
                && !neighbor.is_not_supportive(face as u8 ^ 1)
            {
                visited.insert(adjacent);
                frontier.push_back(adjacent);
            }
        }
    }
    false
}

#[derive(Deserialize, Serialize)]
struct Selection {
    world: usize,
    from: [i32; 3],
}

pub fn use_on_block(context: &mut UseOnBlock<'_>) -> UseResult {
    if !context.player.is_some_and(|player| player.may_build()) {
        return UseResult::Fail;
    }
    if context.sneaking {
        return if context.stack.set_data(&[]) {
            UseResult::Success
        } else {
            UseResult::Fail
        };
    }
    let (world, end) = (context.world, context.position);
    let Some(states) = states() else {
        return UseResult::Fail;
    };
    let checks = &states.contraption_blocks;
    let Some(selection) = context
        .stack
        .data()
        .as_deref()
        .and_then(|bytes| fastnbt::from_bytes::<Selection>(bytes).ok())
        .filter(|selection| selection.world == world.id())
    else {
        if !matches!(checks.classify(world, end), Some(BlockMovement::Movable(_))) {
            return UseResult::Fail;
        }
        return if fastnbt::to_bytes(&Selection {
            world: world.id(),
            from: coords(end),
        })
        .is_ok_and(|data| context.stack.set_data(&data))
        {
            UseResult::Success
        } else {
            UseResult::Fail
        };
    };
    let [x, y, z] = selection.from;
    let start = Pos { x, y, z };
    if start == end {
        context.stack.set_data(&[]);
        return UseResult::Success;
    }
    let max = MAX_SELECTION_DISTANCE;
    let deltas = coords(end)
        .into_iter()
        .zip(selection.from)
        .map(|(a, b)| i64::from(a) - i64::from(b));
    if deltas.clone().any(|d| d.abs() >= max)
        || deltas.map(|d| d * d).sum::<i64>() >= max * max
        || !search_glue_group(world, checks, start, end)
    {
        return UseResult::Fail;
    }
    let Some(id) = GlueBox::span(start, end).spawn(world) else {
        return UseResult::Fail;
    };
    if !(context.creative
        || crate::stack_after_damage(&context.stack, DURABILITY)
            .is_some_and(|nbt| context.stack.write(&nbt)))
    {
        discard(world, id);
        return UseResult::Fail;
    }
    if context.stack.count() > 0 {
        context.stack.set_data(&[]);
    }
    UseResult::Success
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glue_crops_to_moved_blocks_then_rotates_about_the_anchor() {
        let anchor = Pos { x: 6, y: 10, z: 4 };
        let arm = offset(anchor, [1, 0, 0]);
        let scan = GlueScan {
            markers: HashMap::from([(42, GlueBox::span(anchor, offset(anchor, [3, 0, 2])))]),
            used: HashSet::from([42]),
            ..GlueScan::default()
        };
        let (glue, removed) = scan.capture(anchor, [anchor, arm].into_iter());
        assert_eq!(removed, [42]);
        let moved = glue[0].transformed(StructureTransform {
            anchor,
            axis: 1,
            angle: 90.0,
        });
        assert!(moved.contains(anchor));
        assert!(moved.contains(offset(anchor, [0, 0, -1])));
        assert!(!moved.contains(offset(anchor, [0, 0, -2])));
        assert!(!moved.contains(arm));
    }
}
