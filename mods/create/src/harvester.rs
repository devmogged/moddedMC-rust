//! Create 6.0.10's HarvesterBlock and HarvesterMovementBehaviour.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, Placement, ShapeBox, State, World},
};

use crate::{
    CONTENT, DIRECTIONS, STONE, StateIds, TAGS,
    belt_inventory::{count, with_count},
    collider::Frame,
    contraption::PaletteEntry,
    kinetics::{FACES, face_axis, offset, positive},
};

pub const ID: &str = "create:mechanical_harvester";
const HORIZONTAL: [u8; 4] = [2, 3, 4, 5];
const ROTATION: [i16; 4] = [0, 180, 270, 90];
const ACTIVE_AREA_OFFSET: f64 = 0.45;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    register_attached(
        content,
        registry,
        ID,
        "Mechanical Harvester",
        "create:block/mechanical_harvester/block",
        "create:item/mechanical_harvester",
    )
}

pub(crate) fn register_attached(
    content: Content,
    registry: &mut Registry<'_>,
    id: &str,
    name: &str,
    model: &str,
    item_model: &str,
) -> Result<(), RegistrationError> {
    let properties: Vec<_> = HORIZONTAL
        .into_iter()
        .flat_map(|face| {
            [false, true].map(move |wet| {
                format!("facing={},waterlogged={wet}", DIRECTIONS[usize::from(face)])
            })
        })
        .collect();
    let shapes: [[ShapeBox; 1]; 4] = HORIZONTAL.map(|face| {
        let axis = usize::from(face_axis(face));
        let mut shape = ShapeBox {
            min: [0, 2, 0],
            max: [16, 14, 16],
        };
        if positive(face) {
            shape.max[axis] = 3;
        } else {
            shape.min[axis] = 13;
        }
        [shape]
    });
    let states: Vec<_> = properties
        .iter()
        .enumerate()
        .map(|(i, properties)| State {
            properties,
            physical: STONE,
            model,
            rotating_model: "",
            x: 0,
            y: ROTATION[i / 2],
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape: &shapes[i / 2],
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id,
            display_name: name,
            entity: id,
            item_model,
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, states: &mut StateIds, id: &'static str) {
    for face in HORIZONTAL {
        for wet in [false, true] {
            states.add(
                content,
                id,
                &format!("facing={},waterlogged={wet}", DIRECTIONS[usize::from(face)]),
                None,
            );
        }
    }
}

pub fn facing(entry: &PaletteEntry) -> Option<u8> {
    if entry.name != ID {
        return None;
    }
    attached_facing(entry)
}

pub(crate) fn attached_facing(entry: &PaletteEntry) -> Option<u8> {
    if !matches!(entry.name.as_str(), ID | crate::plough::ID) {
        return None;
    }
    let name = entry.properties.get("facing")?;
    HORIZONTAL
        .into_iter()
        .find(|&face| name == DIRECTIONS[usize::from(face)])
}

fn state_at(world: World<'_>, pos: Pos) -> Option<(u32, PaletteEntry)> {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[pos], &mut state);
    let selector = CONTENT.get()?.describe_state(state[0])?;
    Some((state[0], PaletteEntry::parse(&selector)))
}

fn supports_actor(world: World<'_>, pos: Pos, face: u8) -> Option<bool> {
    let (_, entry) = state_at(world, pos)?;
    if TAGS
        .get()?
        .contains("block", "minecraft:leaves", &entry.name)
    {
        return Some(false);
    }
    let axis = usize::from(crate::kinetics::face_axis(face));
    let [u, v] = [(1, 2), (0, 2), (0, 1)][axis].into();
    let shapes = world.collision_shape(pos)?;
    let touching: Vec<_> = shapes
        .iter()
        .filter(|bounds| {
            if crate::kinetics::positive(face) {
                bounds.max[axis] >= 1.0
            } else {
                bounds.min[axis] <= 0.0
            }
        })
        .collect();
    if touching.iter().any(|bounds| {
        bounds.min[u] <= 0.0 && bounds.min[v] <= 0.0 && bounds.max[u] >= 1.0 && bounds.max[v] >= 1.0
    }) {
        return Some(true);
    }
    let cuts = |axis| {
        let mut cuts = vec![0.0, 1.0];
        for bounds in &touching {
            cuts.extend([bounds.min[axis], bounds.max[axis]].map(|edge: f64| edge.clamp(0.0, 1.0)));
        }
        cuts.sort_unstable_by(f64::total_cmp);
        cuts.dedup();
        cuts
    };
    let (u_cuts, v_cuts) = (cuts(u), cuts(v));
    Some(u_cuts.windows(2).all(|a| {
        v_cuts.windows(2).all(|b| {
            touching.iter().any(|bounds| {
                bounds.min[u] <= a[0]
                    && bounds.max[u] >= a[1]
                    && bounds.min[v] <= b[0]
                    && bounds.max[v] >= b[1]
            })
        })
    }))
}

pub fn placed_state(id: &str, args: &Placement<'_>, horizontal_look: u8) -> Option<u32> {
    let support = offset(args.position, FACES[usize::from(args.face)]);
    let face = if args.face < 2 {
        horizontal_look ^ 1
    } else {
        state_at(args.world, support)
            .and_then(|(_, entry)| attached_facing(&entry))
            .unwrap_or(args.face ^ 1)
    };
    let support = offset(args.position, FACES[usize::from(face ^ 1)]);
    if !supports_actor(args.world, support, face)? {
        return None;
    }
    let wet = args.world.fluid_id(args.position)? == "minecraft:water";
    CONTENT.get()?.state(&format!(
        "{id}[facing={},waterlogged={wet}]",
        DIRECTIONS[usize::from(face)]
    ))
}

fn is_crop_block(name: &str) -> bool {
    matches!(
        name,
        "minecraft:wheat"
            | "minecraft:carrots"
            | "minecraft:potatoes"
            | "minecraft:beetroots"
            | "minecraft:torchflower_crop"
    )
}

fn is_growing_plant(name: &str) -> bool {
    matches!(
        name,
        "minecraft:kelp"
            | "minecraft:kelp_plant"
            | "minecraft:cave_vines"
            | "minecraft:cave_vines_plant"
            | "minecraft:weeping_vines"
            | "minecraft:weeping_vines_plant"
            | "minecraft:twisting_vines"
            | "minecraft:twisting_vines_plant"
    )
}

#[derive(Clone, Copy)]
enum Harvest {
    Replant(i32),
    Cut,
    Shear,
}

fn harvest(world: World<'_>, pos: Pos, entry: &PaletteEntry) -> Option<Harvest> {
    let tags = TAGS.get()?;
    let name = entry.name.as_str();
    if tags.contains("block", "create:non_harvestable", name) {
        return None;
    }
    let empty_shape = world.collision_shape(pos)?.is_empty();
    if (is_crop_block(name) || empty_shape || name == "minecraft:cocoa")
        && let Some(age) = entry
            .properties
            .get("age")
            .and_then(|age| age.parse::<i32>().ok())
    {
        let reset_age = i32::from(name == "minecraft:sweet_berry_bush");
        if age > reset_age {
            let mut next_age = entry.clone();
            next_age
                .properties
                .insert("age".into(), age.checked_add(1)?.to_string());
            // A parsed selector can resolve to the default age when the property is out of range.
            let has_next_age = next_age
                .state()
                .and_then(|state| CONTENT.get()?.describe_state(state))
                .is_some_and(|state| PaletteEntry::parse(&state) == next_age);
            if !has_next_age {
                return Some(
                    if is_growing_plant(name)
                        || tags.contains("block", "create:sugar_cane_variants", name)
                    {
                        Harvest::Cut
                    } else {
                        Harvest::Replant(reset_age)
                    },
                );
            }
        }
        if name != "minecraft:sugar_cane" && !is_growing_plant(name) {
            return None;
        }
    }
    if tags.contains("block", "minecraft:leaves", name) {
        return Some(Harvest::Shear);
    }
    let vegetation = matches!(
        name,
        "minecraft:short_grass"
            | "minecraft:fern"
            | "minecraft:dead_bush"
            | "minecraft:red_mushroom"
            | "minecraft:brown_mushroom"
            | "minecraft:nether_sprouts"
            | "minecraft:crimson_roots"
            | "minecraft:warped_roots"
    );
    (name == "minecraft:sugar_cane"
        || empty_shape
            && (is_growing_plant(name)
                || vegetation
                || tags.contains("block", "minecraft:flowers", name)))
    .then_some(Harvest::Cut)
}

pub fn can_harvest(world: World<'_>, pos: Pos) -> bool {
    state_at(world, pos).is_some_and(|(_, entry)| harvest(world, pos, &entry).is_some())
}

pub fn event(world: World<'_>, event: &content::ContentEvent) {
    if event.reason == content::NEIGHBOR_CHANGED
        && let Some((state, entry)) = state_at(world, event.position)
        && event.state == state
        && let Some(face) = attached_facing(&entry)
        && supports_actor(
            world,
            offset(event.position, FACES[usize::from(face ^ 1)]),
            face,
        ) == Some(false)
    {
        world.destroy(event.position, true);
    }
}

pub fn newly_visited(local: [i32; 3], face: u8, previous: Frame, next: Frame) -> Option<Pos> {
    let step = FACES[usize::from(face)].map(f64::from);
    let point = std::array::from_fn(|i| f64::from(local[i]) + 0.5 + ACTIVE_AREA_OFFSET * step[i]);
    let before = previous.to_world(point);
    let relative_before = next.to_local(before);
    if (0..3)
        .map(|i| (point[i] - relative_before[i]) * step[i])
        .sum::<f64>()
        < -1.0e-9
    {
        return None;
    }
    let before = before.map(|n| n.floor() as i32);
    let [x, y, z] = next.to_world(point).map(|n| n.floor() as i32);
    (before != [x, y, z]).then_some(Pos { x, y, z })
}

pub fn visit_new_position(world: World<'_>, pos: Pos) -> Option<Vec<Vec<u8>>> {
    use fastnbt::Value;
    let (state, mut entry) = state_at(world, pos)?;
    let harvest = harvest(world, pos, &entry)?;
    let content = CONTENT.get()?;
    let air = content.state("minecraft:air")?;
    let cut = if let Harvest::Replant(age) = harvest {
        entry.properties.insert("age".into(), age.to_string());
        let planted = entry.state()?;
        if world.can_survive(pos, planted)? {
            planted
        } else {
            air
        }
    } else {
        content.state(&world.fluid_id(pos)?).unwrap_or(air)
    };
    let tool = matches!(harvest, Harvest::Shear)
        .then(|| {
            fastnbt::to_bytes(&std::collections::HashMap::from([
                ("id", Value::String("minecraft:shears".into())),
                ("count", Value::Int(1)),
            ]))
            .ok()
        })
        .flatten();
    let mut drops = world.block_drops(pos, tool.as_deref())?;
    if matches!(harvest, Harvest::Replant(_))
        && let Some(seed) = content.block_item(state)
        && let Some((index, item)) = drops.iter().enumerate().find_map(|(index, bytes)| {
            let item = fastnbt::from_bytes::<Value>(bytes).ok()?;
            let Value::Compound(nbt) = &item else {
                return None;
            };
            (nbt.get("id")
                .is_some_and(|id| matches!(id, Value::String(id) if id == &seed)))
            .then_some((index, item))
        })
    {
        if count(&item) == 1 {
            drops.remove(index);
        } else {
            drops[index] = fastnbt::to_bytes(&with_count(&item, count(&item) - 1)).ok()?;
        }
    }
    if !world.destroy(pos, false) {
        return None;
    }
    if cut != air {
        world.write(&[BlockUpdate {
            position: pos,
            state: cut,
            flags: content::UPDATE_ALL,
        }]);
    }
    Some(drops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blade_visits_cells_only_when_entering_them_in_its_working_direction() {
        let origin = Pos { x: 0, y: 0, z: 0 };
        let still = Frame::translated(origin, [0.0; 3]);
        let near = Frame::translated(origin, [0.025, 0.0, 0.0]);
        let entered = Frame::translated(origin, [0.125, 0.0, 0.0]);
        assert_eq!(newly_visited([0; 3], 5, still, near), None);
        assert_eq!(
            newly_visited([0; 3], 5, near, entered),
            Some(Pos { x: 1, y: 0, z: 0 })
        );
        assert_eq!(newly_visited([0; 3], 5, entered, near), None);
    }
}
