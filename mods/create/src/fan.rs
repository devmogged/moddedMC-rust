//! Create 6.0.10's EncasedFanBlock, AirCurrent and FanProcessing.

use std::{
    collections::{HashMap, HashSet},
    sync::Mutex,
};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, CollisionBox, Content, State, UNLOADED_STATE, World},
    entities::{
        ENTITY_ALIVE, ENTITY_FLYING, ENTITY_ITEM, ENTITY_SNEAKING, Entity, EntityChange, SET_MOTION,
    },
    items::Item,
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTENT, DIRECTIONS, ENTITIES, ITEMS, RESOURCES, STACKS, STONE, StateIds, TAGS,
    belt_inventory::{self, TransportedItemStack, count},
    kinetics::{FACES, Kind, Kinetics, face_axis, offset, positive},
    processing_recipes::{Process, RecipeBook},
};

pub const ID: &str = "create:encased_fan";
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];
const FAN_ROTATION_ARGMAX: f32 = 256.0;
const FAN_DISTANCE: usize = 20;
const FAN_PROCESSING_TIME: i32 = 150;
const FAN_BLOCK_CHECK_RATE: u8 = 30;
const ENTITY_SEARCH_COOLDOWN: u8 = 5;

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    ITEMS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .add_item(
            registry,
            &Item {
                id: "create:propeller",
                display_name: "Propeller",
                carrier: "minecraft:paper",
                model: "create:item/propeller",
            },
        )?;
    let properties = DIRECTIONS.map(|face| format!("facing={face}"));
    let states = std::array::from_fn::<_, 6, _>(|face| {
        let (x, y) = ROTATIONS[face];
        State {
            properties: &properties[face],
            physical: STONE,
            model: "create:block/encased_fan/block",
            rotating_model: "",
            x,
            y,
            orientation: crate::relay_orientation(face_axis(face as u8)),
            shape: &[],
        }
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Encased Fan",
            entity: ID,
            item_model: "create:item/encased_fan",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (face, name) in DIRECTIONS.iter().enumerate() {
        states.add(
            content,
            ID,
            &format!("facing={name}"),
            Some(Kind::Fan(face as u8)),
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FanProcessingType {
    #[serde(rename = "create:blasting", alias = "BLASTING")]
    Blasting,
    #[serde(rename = "create:smoking", alias = "SMOKING")]
    Smoking,
    #[serde(rename = "create:haunting", alias = "HAUNTING")]
    Haunting,
    #[serde(rename = "create:splashing", alias = "SPLASHING")]
    Splashing,
}

use FanProcessingType::{Blasting, Haunting, Smoking, Splashing};

impl FanProcessingType {
    const ALL: [Self; 4] = [Blasting, Smoking, Haunting, Splashing];

    const fn catalyst_tag(self) -> &'static str {
        match self {
            Blasting => "create:fan_processing_catalysts/blasting",
            Smoking => "create:fan_processing_catalysts/smoking",
            Haunting => "create:fan_processing_catalysts/haunting",
            Splashing => "create:fan_processing_catalysts/splashing",
        }
    }

    fn process(self, recipes: &RecipeBook, item: &Value) -> Option<Vec<Value>> {
        let recipe = match self {
            Splashing => recipes.find(Process::Splashing, item)?,
            Haunting => recipes.find(Process::Haunting, item)?,
            Smoking => recipes.find(Process::Smoking, item)?,
            Blasting => {
                let heat = recipes
                    .find(Process::Smelting, item)
                    .or_else(|| recipes.find(Process::Blasting, item));
                if let Some(recipe) = heat {
                    let smoke = recipes.find(Process::Smoking, item);
                    if smoke.is_none_or(|other| other.first_output() != recipe.first_output()) {
                        return Some(recipe.roll_results(count(item)));
                    }
                } else if is_fire_resistant(item) {
                    return None;
                }
                return Some(Vec::new());
            }
        };
        Some(recipe.roll_results(count(item)))
    }

    fn can_process(self, recipes: &RecipeBook, item: &Value) -> bool {
        match self {
            Splashing => recipes.find(Process::Splashing, item).is_some(),
            Haunting => recipes.find(Process::Haunting, item).is_some(),
            Smoking => recipes.find(Process::Smoking, item).is_some(),
            Blasting => {
                recipes.find(Process::Smelting, item).is_some()
                    || recipes.find(Process::Blasting, item).is_some()
                    || !is_fire_resistant(item)
            }
        }
    }
}

fn is_fire_resistant(item: &Value) -> bool {
    #[derive(Deserialize)]
    struct Resistance {
        value: DamageTypes,
    }
    #[derive(Deserialize)]
    struct DamageTypes {
        types: Value,
    }
    let Some(types) = fastnbt::to_bytes(item)
        .ok()
        .and_then(|bytes| {
            STACKS
                .get()?
                .component(&bytes, "minecraft:damage_resistant")
        })
        .and_then(|bytes| fastnbt::from_bytes::<Resistance>(&bytes).ok())
        .map(|resistance| resistance.value.types)
    else {
        return false;
    };
    let Some(tags) = TAGS.get() else {
        return false;
    };
    let is_fire = |id: &str| tags.contains("damage_type", "minecraft:is_fire", id);
    match types {
        Value::String(tag) if tag == "#minecraft:is_fire" => true,
        Value::String(tag) if tag.starts_with('#') => tags
            .values("damage_type", &tag)
            .is_some_and(|ids| ids.iter().any(|id| is_fire(id))),
        Value::String(id) => is_fire(&id),
        Value::List(ids) => ids
            .iter()
            .any(|id| matches!(id, Value::String(id) if is_fire(id))),
        _ => false,
    }
}

#[derive(Default)]
struct Catalysts {
    generation: u64,
    blocks: HashMap<String, FanProcessingType>,
    fluids: HashMap<String, FanProcessingType>,
    transparent: HashSet<String>,
}

static CATALYSTS: Mutex<Option<Catalysts>> = Mutex::new(None);

impl Catalysts {
    fn load(generation: u64) -> Option<Self> {
        let tags = *TAGS.get()?;
        let mut catalysts = Self {
            generation,
            ..Self::default()
        };
        for kind in FanProcessingType::ALL {
            for id in tags
                .values("block", kind.catalyst_tag())
                .unwrap_or_default()
            {
                catalysts.blocks.insert(id, kind);
            }
            for id in tags
                .values("fluid", kind.catalyst_tag())
                .unwrap_or_default()
            {
                catalysts.fluids.insert(id, kind);
            }
        }
        catalysts.transparent.extend(
            tags.values("block", "create:fan_transparent")
                .unwrap_or_default(),
        );
        Some(catalysts)
    }

    fn kind_at(&self, world: World<'_>, pos: Pos, selector: &str) -> Option<FanProcessingType> {
        let (id, properties) = selector
            .strip_suffix(']')
            .and_then(|selector| selector.split_once('['))
            .unwrap_or((selector, ""));
        let mut block = self.blocks.get(id).copied();
        if matches!(id, "minecraft:campfire" | "minecraft:soul_campfire")
            && properties.split(',').any(|prop| prop == "lit=false")
        {
            block = None;
        }
        let fluid = world
            .fluid_id(pos)
            .and_then(|id| self.fluids.get(&id).copied());
        // The original registry checks higher priorities first.
        block
            .into_iter()
            .chain(fluid)
            .max_by_key(|kind| *kind as u8)
    }
}

const DEPTH_TEST_COORDINATES: [[f64; 2]; 5] = [
    [0.25, 0.25],
    [0.25, 0.75],
    [0.5, 0.5],
    [0.75, 0.25],
    [0.75, 0.75],
];

fn find_max_depth(shape: &[CollisionBox], face: u8) -> Option<f64> {
    let axis = usize::from(face_axis(face));
    let u = (axis + 1) % 3;
    let v = (axis + 2) % 3;
    let mut max_depth: f64 = 0.0;
    for [a, b] in DEPTH_TEST_COORDINATES {
        let depth = shape
            .iter()
            .filter(|bnd| bnd.min[u] <= a && a < bnd.max[u] && bnd.min[v] <= b && b < bnd.max[v])
            .map(|bnd| {
                if positive(face) {
                    bnd.min[axis]
                } else {
                    1.0 - bnd.max[axis]
                }
            })
            .reduce(f64::min)?;
        max_depth = max_depth.max(depth);
    }
    Some(max_depth)
}

#[derive(Clone, Debug)]
pub struct AirCurrent {
    origin: Pos,
    face: u8,
    pushing: bool,
    distance: f32,
    types: Vec<Option<FanProcessingType>>,
    speed: f32,
    generation: u64,
    block_cooldown: u8,
    entity_cooldown: u8,
    caught_entities: Vec<i32>,
}

impl AirCurrent {
    fn rebuild(world: World<'_>, origin: Pos, face: u8, speed: f32, catalysts: &Catalysts) -> Self {
        let range =
            3.0 + (FAN_DISTANCE as f32 - 3.0) * (speed.abs() / FAN_ROTATION_ARGMAX).min(1.0);
        let len = range.ceil() as usize;
        let direction = FACES[usize::from(face)];
        let positions: [Pos; FAN_DISTANCE] =
            std::array::from_fn(|i| offset(origin, direction.map(|axis| axis * (i as i32 + 1))));
        let mut states = [UNLOADED_STATE; FAN_DISTANCE];
        world.read(&positions[..len], &mut states[..len]);
        let content = CONTENT.get();
        let selectors = states[..len]
            .iter()
            .map(|&state| {
                content
                    .and_then(|content| content.describe_state(state))
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>();
        let mut distance = range;
        for i in 0..len {
            let id = selectors[i].split('[').next().unwrap_or_default();
            if catalysts.transparent.contains(id) {
                continue;
            }
            let Some(shape) = world.collision_shape(positions[i]) else {
                distance = i as f32;
                break;
            };
            if matches!(shape.as_slice(), [one] if one.min == [0.0; 3] && one.max == [1.0; 3]) {
                distance = i as f32;
                break;
            }
            if let Some(depth) = find_max_depth(&shape, face) {
                distance = range.min(i as f32 + depth as f32 + 1.0 / 32.0);
                break;
            }
        }
        let pushing = speed.is_sign_positive() == positive(face);
        let mut types: Vec<_> = selectors[..distance.ceil() as usize]
            .iter()
            .zip(positions)
            .map(|(selector, pos)| catalysts.kind_at(world, pos, selector))
            .collect();
        if !pushing {
            types.reverse();
        }
        let mut active = None;
        for kind in &mut types {
            active = kind.or(active);
            *kind = active;
        }
        if !pushing {
            types.reverse();
        }
        Self {
            origin,
            face,
            pushing,
            distance,
            types,
            speed,
            generation: catalysts.generation,
            block_cooldown: FAN_BLOCK_CHECK_RATE,
            entity_cooldown: 0,
            caught_entities: Vec::new(),
        }
    }

    fn kind_at(&self, distance: f64) -> Option<FanProcessingType> {
        if !(0.0..=f64::from(self.distance)).contains(&distance) {
            return None;
        }
        let index = if self.pushing {
            (distance.ceil() as usize).saturating_sub(1)
        } else {
            (distance.floor() as usize).min(self.types.len().saturating_sub(1))
        };
        self.types.get(index).copied().flatten()
    }

    fn tick_entities(&mut self, world: World<'_>, recipes: &RecipeBook) {
        let Some(&entities) = ENTITIES.get() else {
            return;
        };
        if self.distance < 0.25 {
            return;
        }
        let axis = usize::from(face_axis(self.face));
        let direction = FACES[usize::from(self.face)].map(f64::from);
        let center =
            [self.origin.x, self.origin.y, self.origin.z].map(|coord| f64::from(coord) + 0.5);
        let near = center[axis] + direction[axis] * 0.5;
        let far = near + direction[axis] * f64::from(self.distance);
        let mut min = center.map(|c| c - 0.5);
        let mut max = center.map(|c| c + 0.5);
        min[axis] = near.min(far);
        max[axis] = near.max(far);
        if self.entity_cooldown == 0 {
            self.caught_entities = entities.entities_in(world, min, max, 0);
            self.entity_cooldown = ENTITY_SEARCH_COOLDOWN;
        } else {
            self.entity_cooldown -= 1;
        }
        let mut caught = std::mem::take(&mut self.caught_entities);
        caught.retain(|&id| {
            entities
                .with_entity(world, id, |entity| {
                    let flags = entity.flags();
                    if flags & ENTITY_ALIVE == 0 || flags & ENTITY_FLYING != 0 {
                        return false;
                    }
                    if entity.body().is_none_or(|body| {
                        (0..3).any(|i| body.max[i] <= min[i] || body.min[i] >= max[i])
                    }) {
                        return false;
                    }
                    let Some(view) = entity.view() else {
                        return false;
                    };
                    let from_center =
                        std::array::from_fn::<_, 3, _>(|i| view.position[i] - center[i]);
                    let radius = from_center.iter().map(|x| x * x).sum::<f64>().sqrt();
                    if radius == 0.0 {
                        return true;
                    }
                    let sneak = if flags & ENTITY_SNEAKING == 0 {
                        512.0
                    } else {
                        4096.0
                    };
                    let acceleration = (f64::from(self.speed.abs() / sneak)
                        / (radius / f64::from(self.distance)))
                        as f32;
                    let motion = std::array::from_fn(|i| {
                        let target = if self.pushing {
                            direction[i]
                        } else {
                            -direction[i]
                        } * f64::from(acceleration);
                        view.motion[i] + (target - view.motion[i]).clamp(-5.0, 5.0) * 0.125
                    });
                    entity.change(&EntityChange {
                        mask: SET_MOTION,
                        motion,
                        ..EntityChange::default()
                    });
                    entity.reset_fall_distance();
                    if view.kind & ENTITY_ITEM != 0
                        && let Some(kind) =
                            self.kind_at((view.position[axis] - near) * direction[axis])
                    {
                        process_entity(world, entity, kind, recipes, view.position, motion);
                    }
                    true
                })
                .unwrap_or(false)
        });
        self.caught_entities = caught;
    }
}

const fn processing_time(count: i32) -> i32 {
    FAN_PROCESSING_TIME * ((count - 1) / 16 + 1) + 1
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct EntityFanProcessing {
    #[serde(default)]
    create_data: EntityCreateData,
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct EntityCreateData {
    processing: Option<EntityProcessingTimer>,
    #[serde(flatten)]
    other: HashMap<String, Value>,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
struct EntityProcessingTimer {
    #[serde(rename = "Type")]
    kind: FanProcessingType,
    time: i32,
}

fn process_entity(
    world: World<'_>,
    entity: Entity<'_>,
    kind: FanProcessingType,
    recipes: &RecipeBook,
    pos: [f64; 3],
    motion: [f64; 3],
) {
    let Some(stacks) = STACKS.get() else { return };
    let Some(item) = stacks
        .entity_stack(entity)
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok())
    else {
        return;
    };
    let mut data = entity
        .data("create")
        .and_then(|bytes| fastnbt::from_bytes::<EntityFanProcessing>(&bytes).ok())
        .unwrap_or_default();
    let old = data
        .create_data
        .processing
        .as_ref()
        .filter(|timer| timer.kind == kind);
    let time = match old {
        Some(timer) if timer.time >= 0 => timer.time,
        Some(_) => return,
        None if kind.can_process(recipes, &item) => processing_time(count(&item)),
        None => return,
    } - 1;
    data.create_data.processing = Some(EntityProcessingTimer { kind, time });
    if let Ok(bytes) = fastnbt::to_bytes(&data) {
        entity.set_data("create", &bytes);
    }
    if time == 0
        && let Some(products) = kind.process(recipes, &item)
    {
        let mut products = products.into_iter();
        let Some(first) = products.next() else {
            stacks.set_entity_stack(entity, &[]);
            return;
        };
        let Ok(bytes) = fastnbt::to_bytes(&first) else {
            return;
        };
        if !stacks.set_entity_stack(entity, &bytes) {
            return;
        }
        for product in products {
            if let Ok(bytes) = fastnbt::to_bytes(&product) {
                stacks.spawn(world, pos, motion, &bytes);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TransportedFanProcessing {
    #[serde(rename = "FanProcessingType")]
    kind: FanProcessingType,
    #[serde(rename = "FanProcessingTime")]
    time: i32,
}

/// A result of None may still change the serialized timer.
pub fn process_transported(
    kind: FanProcessingType,
    recipes: &RecipeBook,
    item: &mut TransportedItemStack,
) -> Option<Vec<Value>> {
    let Some(timer) = item
        .fan_processing
        .as_mut()
        .filter(|timer| timer.kind == kind)
    else {
        item.fan_processing = Some(TransportedFanProcessing {
            kind,
            time: if kind.can_process(recipes, &item.item) {
                processing_time(count(&item.item))
            } else {
                -1
            },
        });
        return None;
    };
    if timer.time == -1 {
        return None;
    }
    timer.time -= 1;
    (timer.time == -1)
        .then(|| kind.process(recipes, &item.item))
        .flatten()
}

pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    let fans = kinetics.positions(|kind| matches!(kind, Kind::Fan(_)));
    if fans.is_empty() {
        return;
    }
    let (Some(recipes), Some(content), Some(resources)) = (
        crate::processing_recipes::current(),
        CONTENT.get(),
        RESOURCES.get(),
    ) else {
        return;
    };
    let mut catalysts = CATALYSTS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if catalysts
        .as_ref()
        .is_none_or(|cache| cache.generation != resources.generation())
    {
        *catalysts = Catalysts::load(resources.generation());
    }
    let Some(catalysts) = catalysts.as_ref() else {
        return;
    };
    for origin in fans {
        let Some(be) = kinetics.get_mut(origin) else {
            continue;
        };
        let Kind::Fan(face) = be.kind else { continue };
        let speed = be.get_speed();
        if speed == 0.0 {
            be.fan = None;
            continue;
        }
        let mut flow = be
            .fan
            .take()
            .filter(|flow| {
                flow.speed == speed
                    && flow.face == face
                    && flow.generation == catalysts.generation
                    && flow.block_cooldown > 0
            })
            .unwrap_or_else(|| {
                Box::new(AirCurrent::rebuild(world, origin, face, speed, catalysts))
            });
        flow.block_cooldown -= 1;
        flow.tick_entities(world, &recipes);
        for i in 1..=flow.types.len() {
            let here = offset(origin, FACES[usize::from(face)].map(|step| step * i as i32));
            let segment = flow.kind_at(i as f64 - 1.0);
            for below in 0..=i32::from(face_axis(face) != 1) {
                let pos = offset(here, [0, -below, 0]);
                let depot = kinetics.depots.contains_key(&pos);
                let belt = matches!(kinetics.kind(pos), Some(Kind::Belt(_)));
                if !depot && !belt {
                    continue;
                }
                let mut state = [UNLOADED_STATE];
                world.read(&[pos], &mut state);
                let Some(kind) = content
                    .describe_state(state[0])
                    .and_then(|selector| catalysts.kind_at(world, pos, &selector))
                    .or(segment)
                else {
                    continue;
                };
                if let Some(depot) = kinetics.depots.get_mut(&pos) {
                    depot.fan_process(world, pos, kind, &recipes);
                }
                belt_inventory::fan_process(world, kinetics, pos, kind, &recipes);
            }
        }
        if let Some(be) = kinetics.get_mut(origin) {
            be.fan = Some(flow);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flow_depth_is_the_last_of_five_first_intersections() {
        let rear = CollisionBox {
            min: [0.5, 0.5, 0.0],
            max: [0.75, 1.0, 1.0],
        };
        let front = CollisionBox {
            min: [0.25, 0.0, 0.0],
            max: [0.875, 0.5, 1.0],
        };
        assert_eq!(find_max_depth(&[front], 5), None);
        assert_eq!(find_max_depth(&[front, rear], 5), Some(0.5));
        assert_eq!(find_max_depth(&[front, rear], 4), Some(0.25));
        assert_eq!(processing_time(16), 151);
        assert_eq!(processing_time(17), 301);
    }

    #[test]
    fn catalyst_boundary_belongs_to_the_segment_upstream() {
        let types = vec![Some(Smoking), Some(Haunting), Some(Haunting)];
        let mut flow = AirCurrent {
            origin: Pos { x: 0, y: 0, z: 0 },
            face: 5,
            pushing: true,
            distance: 2.5,
            types,
            speed: 16.0,
            generation: 1,
            block_cooldown: 0,
            entity_cooldown: 0,
            caught_entities: Vec::new(),
        };
        assert_eq!(flow.kind_at(1.0), Some(Smoking));
        assert_eq!(flow.kind_at(1.001), Some(Haunting));
        flow.pushing = false;
        assert_eq!(flow.kind_at(0.999), Some(Smoking));
        assert_eq!(flow.kind_at(1.0), Some(Haunting));
        assert_eq!(flow.kind_at(2.501), None);
    }

    #[test]
    fn transported_timer_is_saved_in_create_fields() {
        let item = Value::Compound(HashMap::from([
            ("id".into(), Value::String("minecraft:sand".into())),
            ("count".into(), Value::Int(3)),
        ]));
        let mut transported = TransportedItemStack::new(item);
        let unprocessed: HashMap<String, Value> =
            fastnbt::from_bytes(&fastnbt::to_bytes(&transported).expect("transported stack"))
                .expect("item tag");
        assert!(!unprocessed.contains_key("FanProcessingType"));
        assert!(!unprocessed.contains_key("FanProcessingTime"));
        let timer = TransportedFanProcessing {
            kind: Haunting,
            time: 42,
        };
        transported.fan_processing = Some(timer);
        let encoded = fastnbt::to_bytes(&transported).expect("transported stack");
        let loaded: TransportedItemStack = fastnbt::from_bytes(&encoded).expect("saved stack");
        assert_eq!(loaded.fan_processing, Some(timer));
        let saved: HashMap<String, Value> = fastnbt::from_bytes(&encoded).expect("saved tags");
        assert_eq!(
            saved["FanProcessingType"],
            Value::String("create:haunting".into())
        );
        assert_eq!(saved["FanProcessingTime"], Value::Int(42));
    }
}
