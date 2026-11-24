use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use fastnbt::IntArray;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{
        Block, BlockUpdate, Content, ContentEvent, Placement, ShapeBox, State, UNLOADED,
        UPDATE_ALL, World,
    },
    effects::SOUND_BLOCKS,
    entities::Entity,
    interaction::{BlockUseResult, UseBlock},
    items::{Item, Items},
};
use serde::{Deserialize, Serialize};

use crate::{CONTENT, EFFECTS, ENTITIES, MOB_HEALTH, MOBS, appearance::Appearance, roster, taming};

const GROW_TICKS: i32 = 24_000;
const AGE: &str = "saintsdragons:growth";
const STATES: [&str; 3] = ["hatch=0", "hatch=1", "hatch=2"];

struct Species {
    name: &'static str,
    path: &'static str,
    display: &'static str,
    ticks: u32,
    shape: ShapeBox,
    juvenile: (f32, f64, f64),
    adult: (f64, f64),
    attack: Option<f64>,
}

const SPECIES: &[Species] = &[
    Species {
        name: "ignivorus",
        path: "ignivorus_egg",
        display: "Ignivorus Egg",
        ticks: 36_000,
        shape: ShapeBox {
            min: [3, 0, 3],
            max: [13, 13, 13],
        },
        juvenile: (90.0, 0.0, 0.55),
        adult: (450.0, 4.0),
        attack: Some(50.0),
    },
    Species {
        name: "stegonaut",
        path: "stegonaut_egg",
        display: "Stegonaut Egg",
        ticks: 30_000,
        shape: ShapeBox {
            min: [3, 0, 3],
            max: [13, 10, 13],
        },
        juvenile: (50.0, 5.0, 0.65),
        adult: (100.0, 15.0),
        attack: None,
    },
    Species {
        name: "varasuchus",
        path: "varasuchus_egg",
        display: "Varasuchus Egg",
        ticks: 24_000,
        shape: ShapeBox {
            min: [4, 0, 4],
            max: [12, 10, 12],
        },
        juvenile: (80.0, 0.0, 0.5),
        adult: (200.0, 8.0),
        attack: None,
    },
    Species {
        name: "atroxiia",
        path: "atroxiia_egg",
        display: "Atroxiia Egg",
        ticks: 24_000,
        shape: ShapeBox {
            min: [5, 0, 5],
            max: [11, 8, 11],
        },
        juvenile: (30.0, 0.0, 0.30),
        adult: (200.0, 10.0),
        attack: None,
    },
    Species {
        name: "volitans",
        path: "volitans_egg",
        display: "Volitans Egg",
        ticks: 18_000,
        shape: ShapeBox {
            min: [4, 0, 4],
            max: [12, 10, 12],
        },
        juvenile: (60.0, 0.0, 0.55),
        adult: (160.0, 6.0),
        attack: None,
    },
    Species {
        name: "cindervane",
        path: "cindervane_egg",
        display: "Cindervane Egg",
        ticks: 12_000,
        shape: ShapeBox {
            min: [3, 0, 3],
            max: [13, 10, 13],
        },
        juvenile: (40.0, 0.0, 1.0),
        adult: (80.0, 4.0),
        attack: None,
    },
    Species {
        name: "raevyx",
        path: "raevyx_egg",
        display: "Raevyx Egg",
        ticks: 18_000,
        shape: ShapeBox {
            min: [3, 0, 3],
            max: [13, 10, 13],
        },
        juvenile: (60.0, 0.0, 0.4),
        adult: (180.0, 8.0),
        attack: None,
    },
];

struct EggStates {
    stages: Vec<Vec<u32>>,
    water: u32,
    incubator: u32,
    air: u32,
}

static IDS: OnceLock<EggStates> = OnceLock::new();
static ACTIVE: Mutex<Option<HashMap<usize, HashSet<Pos>>>> = Mutex::new(None);

#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
struct Egg {
    hatch_progress: f64,
    #[serde(rename = "OwnerUUID", skip_serializing_if = "Option::is_none")]
    owner: Option<IntArray>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baby_gender: Option<i8>,
    paused_hatching_particles_shown: bool,
}

#[derive(Deserialize, Serialize)]
struct Growth {
    remaining: i32,
    #[serde(default)]
    piebald_on_maturity: bool,
    #[serde(default)]
    void_kissed_on_maturity: bool,
}

fn properties(species: &Species) -> Vec<String> {
    if species.name == "cindervane" {
        (1..=3)
            .flat_map(|count| STATES.map(|stage| format!("eggs={count},{stage}")))
            .collect()
    } else if species.name == "volitans" {
        [false, true]
            .into_iter()
            .flat_map(|wet| STATES.map(|stage| format!("{stage},waterlogged={wet}")))
            .collect()
    } else {
        STATES.map(str::to_owned).into()
    }
}

fn place_volitans(placement: Placement<'_>) -> Option<u32> {
    if placement.world.fluid_id(placement.position).as_deref() == Some("minecraft:water") {
        CONTENT
            .get()?
            .state("saintsdragons:volitans_egg[hatch=0,waterlogged=true]")
    } else {
        None
    }
}

pub fn register(
    content: Content,
    items: Items,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let original = cfg!(feature = "original-assets");
    for species in SPECIES {
        let id = format!("saintsdragons:{}", species.path);
        let properties = properties(species);
        let clutch = species.name == "cindervane";
        let models: Vec<_> = (0..properties.len())
            .map(|index| {
                let stage = index % 3;
                let stem = if clutch {
                    format!("{}_{}", species.path, index / 3 + 1)
                } else {
                    species.path.to_owned()
                };
                format!("saintsdragons:block/{}/{stem}_stage{stage}", species.name)
            })
            .collect();
        let shapes: Vec<_> = (0..properties.len())
            .map(|index| {
                let mut shape = species.shape;
                if clutch {
                    let extra = (index / 3) as i16;
                    for axis in [0, 2] {
                        shape.min[axis] -= extra;
                        shape.max[axis] += extra;
                    }
                }
                [shape]
            })
            .collect();
        let states: Vec<_> = properties
            .iter()
            .enumerate()
            .map(|(index, properties)| State {
                properties,
                physical: if species.name == "volitans" && index >= 3 {
                    "minecraft:iron_bars[waterlogged=true]"
                } else {
                    "minecraft:iron_bars"
                },
                model: if original {
                    &models[index]
                } else {
                    "minecraft:block/dragon_egg"
                },
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0.0, 0.0, 0.0, 1.0],
                shape: &shapes[index],
            })
            .collect();
        let model = format!("saintsdragons:item/{}", species.path);
        content.add_block(
            registry,
            &Block {
                id: &id,
                display_name: species.display,
                entity: &id,
                states: &states,
                item_model: if original {
                    &model
                } else {
                    "minecraft:block/dragon_egg"
                },
            },
            (species.name == "volitans").then_some(place_volitans),
        )?;
        for (index, state) in properties.iter().enumerate() {
            let selector = format!("{id}[{state}]");
            content.set_comparator_output(registry, &selector, (index % STATES.len()) as u8)?;
        }
        items.add_item(
            registry,
            &Item {
                id: &format!("saintsdragons:baby_{}_display", species.name),
                display_name: species.display.trim_end_matches(" Egg"),
                carrier: "minecraft:paper",
                model: "minecraft:item/dragon_head",
            },
        )?;
    }
    Ok(())
}

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    SPECIES
        .iter()
        .map(|species| (species.path, species.display))
}

fn ids() -> Option<&'static EggStates> {
    if let Some(ids) = IDS.get() {
        return Some(ids);
    }
    let content = CONTENT.get()?;
    let stages = SPECIES
        .iter()
        .map(|species| {
            let id = species.path;
            properties(species)
                .iter()
                .map(|state| content.state(&format!("saintsdragons:{id}[{state}]")))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<_>>()?;
    let _ = IDS.set(EggStates {
        stages,
        incubator: content.state("saintsdragons:ignivorus_incubator_block")?,
        air: content.state("minecraft:air")?,
        water: content.state("minecraft:water")?,
    });
    IDS.get()
}

fn center(pos: Pos, y_offset: f64) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + y_offset,
        f64::from(pos.z) + 0.5,
    ]
}

fn sound(world: World<'_>, pos: Pos, name: &str) {
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(world, center(pos, 0.5), name, SOUND_BLOCKS, 0.7, 1.0);
    }
}

fn particles(world: World<'_>, pos: Pos, starting: bool) {
    if let Some(effects) = EFFECTS.get() {
        if starting {
            effects.particles(
                world,
                "minecraft:happy_villager",
                center(pos, 0.85),
                [0.28, 0.18, 0.28],
                0.02,
                8,
            );
        } else {
            effects.particles(
                world,
                "minecraft:smoke",
                center(pos, 0.7),
                [0.26, 0.18, 0.26],
                0.02,
                10,
            );
        }
    }
}

fn attributes(mob: Entity<'_>, species: &Species, juvenile: bool) -> bool {
    let Some(health) = MOB_HEALTH.get() else {
        return false;
    };
    let (max, armor, scale) = if juvenile {
        (
            f64::from(species.juvenile.0),
            species.juvenile.1,
            species.juvenile.2,
        )
    } else {
        (species.adult.0, species.adult.1, 1.0)
    };
    health.set_attribute(mob, "minecraft:max_health", max)
        && health.set_attribute(mob, "minecraft:armor", armor)
        && species.attack.is_none_or(|attack| {
            health.set_attribute(
                mob,
                "minecraft:attack_damage",
                if juvenile { 0.0 } else { attack },
            )
        })
        && health.set_attribute(mob, "minecraft:scale", scale)
}

fn hatch(
    world: World<'_>,
    pos: Pos,
    species: &Species,
    egg: &Egg,
    replacement: u32,
    count: usize,
) -> bool {
    let Some(entities) = ENTITIES.get() else {
        return false;
    };
    let mut spawned = Vec::with_capacity(count);
    let kind = format!("saintsdragons:{}", species.name);
    let mut initialized = 0;
    for _ in 0..count {
        let mut at = center(pos, 0.0);
        if count > 1 {
            for axis in [0, 2] {
                at[axis] += (crate::ai::random(1001) as f64 / 1000.0 - 0.5) * 0.5;
            }
        }
        let Some(id) = roster::spawn(world, &kind, at) else {
            break;
        };
        spawned.push(id);
        if entities.with_entity(world, id, |mob| init_hatchling(world, mob, species, egg))
            != Some(true)
        {
            break;
        }
        initialized += 1;
    }
    if initialized == count
        && world.write(&[BlockUpdate {
            position: pos,
            state: replacement,
            flags: UPDATE_ALL,
        }]) == 1
    {
        sound(world, pos, "minecraft:entity.turtle.egg_hatch");
        return true;
    }
    for id in spawned {
        entities.with_entity(world, id, |mob| mob.discard());
    }
    false
}

fn init_hatchling(world: World<'_>, mob: Entity<'_>, species: &Species, egg: &Egg) -> bool {
    let (Some(health), Some(mobs)) = (MOB_HEALTH.get(), MOBS.get()) else {
        return false;
    };
    let female = egg.baby_gender.map_or_else(
        || mobs.uuid(mob).is_some_and(|id| id[15] & 1 != 0),
        |gender| gender == 1,
    );
    let piebald_on_maturity =
        species.name == "cindervane" && cindervane_piebald(female, crate::ai::random(100));
    let Ok(age) = fastnbt::to_bytes(&Growth {
        remaining: GROW_TICKS,
        piebald_on_maturity,
        void_kissed_on_maturity: species.name == "varasuchus"
            && world.dimension_id() == Some("minecraft:the_end"),
    }) else {
        return false;
    };
    if !attributes(mob, species, true)
        || !health.set(mob, species.juvenile.0)
        || !mob.set_data(AGE, &age)
    {
        return false;
    }
    Appearance::set_female(female, mob);
    if let Some(owner) = &egg.owner
        && owner.len() == 4
    {
        let mut uuid = [0; 16];
        for (dst, value) in uuid.as_chunks_mut::<4>().0.iter_mut().zip(owner.iter()) {
            dst.copy_from_slice(&value.to_be_bytes());
        }
        taming::inherit_owner(mob, uuid);
        crate::codex::remember(world, mob, None);
    }
    true
}

pub fn stack_clutch(click: &mut UseBlock<'_>) -> BlockUseResult {
    if !click.with_item || click.sneaking {
        return BlockUseResult::Pass;
    }
    let Some(stages) = ids().and_then(|ids| {
        SPECIES
            .iter()
            .position(|species| species.name == "cindervane")
            .map(|i| &ids.stages[i])
    }) else {
        return BlockUseResult::Pass;
    };
    let Some(index) = stages[..6].iter().position(|&state| state == click.state) else {
        return BlockUseResult::Pass;
    };
    let Some(stack) = click
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<HashMap<String, fastnbt::Value>>(&bytes).ok())
    else {
        return BlockUseResult::Pass;
    };
    if !matches!(stack.get("id"), Some(fastnbt::Value::String(id)) if id == "saintsdragons:cindervane_egg")
    {
        return BlockUseResult::Pass;
    }
    if click.world.write(&[BlockUpdate {
        position: click.position,
        state: stages[index + 3],
        flags: UPDATE_ALL,
    }]) != 1
    {
        return BlockUseResult::Fail;
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    sound(click.world, click.position, "minecraft:block.metal.place");
    BlockUseResult::Success
}

pub fn is_juvenile(mob: Entity<'_>) -> bool {
    mob.data(AGE).is_some()
}

pub fn grow(mob: Entity<'_>, name: &str) -> bool {
    let Some(species) = SPECIES.iter().find(|species| species.name == name) else {
        return false;
    };
    let Some(mut growth) = mob
        .data(AGE)
        .and_then(|data| fastnbt::from_bytes::<Growth>(&data).ok())
    else {
        return false;
    };
    if growth.remaining <= 0 {
        return false;
    }
    growth.remaining -= 1;
    if growth.remaining == 0
        && (!attributes(mob, species, false)
            || (growth.piebald_on_maturity && !Appearance::mature_with_variant(mob, 2))
            || (growth.void_kissed_on_maturity && !Appearance::mature_with_variant(mob, 1)))
    {
        return true;
    }
    if growth.remaining == 0 {
        mob.set_data(AGE, &[]);
    } else if let Ok(bytes) = fastnbt::to_bytes(&growth) {
        mob.set_data(AGE, &bytes);
    }
    growth.remaining > 0
}

fn nearby_lightning(world: World<'_>, pos: Pos) -> bool {
    let Some(entities) = ENTITIES.get() else {
        return false;
    };
    let corner = [f64::from(pos.x), f64::from(pos.y), f64::from(pos.z)];
    entities
        .entities_matching(
            world,
            corner.map(|v| v - 3.0),
            corner.map(|v| v + 4.0),
            0,
            0,
        )
        .into_iter()
        .any(|id| {
            entities.with_entity(world, id, |entity| entity.type_id() == "lightning_bolt")
                == Some(true)
        })
}

fn advance(world: World<'_>, pos: Pos, ids: &EggStates) -> bool {
    let below = Pos {
        y: pos.y - 1,
        ..pos
    };
    let mut states = [0; 2];
    if !world.read(&[pos, below], &mut states) {
        return false;
    }
    let Some(index) = ids
        .stages
        .iter()
        .position(|stages| stages.contains(&states[0]))
    else {
        return false;
    };
    let species = &SPECIES[index];
    let stages = &ids.stages[index];
    let group = stages
        .iter()
        .position(|&stage| stage == states[0])
        .unwrap_or_default()
        / 3;
    let waterlogged = species.name == "volitans" && group == 1;
    let count = if species.name == "cindervane" {
        group + 1
    } else {
        1
    };
    let Some(mut egg) = world
        .entity(pos)
        .and_then(|bytes| fastnbt::from_bytes::<Egg>(&bytes).ok())
    else {
        return true;
    };
    let paused = match species.name {
        "ignivorus" => states[1] != ids.incubator,
        "volitans" => !waterlogged,
        _ => false,
    };
    if paused {
        if !egg.paused_hatching_particles_shown {
            egg.paused_hatching_particles_shown = true;
            particles(world, pos, false);
            save(world, pos, &egg);
        }
        return true;
    }
    egg.paused_hatching_particles_shown = false;
    if egg.hatch_progress <= 0.0 {
        particles(world, pos, true);
    }
    let raevyx = species.name == "raevyx";
    let ticks = if raevyx && world.is_thundering() {
        9_600
    } else {
        species.ticks
    };
    egg.hatch_progress = (egg.hatch_progress + 1.0 / f64::from(ticks)).min(1.0);
    if egg.hatch_progress >= 1.0 || (raevyx && nearby_lightning(world, pos)) {
        return !hatch(
            world,
            pos,
            species,
            &egg,
            if waterlogged { ids.water } else { ids.air },
            count,
        );
    }
    let stage = ((egg.hatch_progress * 3.0) as usize).min(2) + group * 3;
    if stages[stage] != states[0]
        && world.write(&[BlockUpdate {
            position: pos,
            state: stages[stage],
            flags: UPDATE_ALL,
        }]) == 1
    {
        sound(world, pos, "minecraft:entity.turtle.egg_crack");
    }
    save(world, pos, &egg);
    true
}

fn save(world: World<'_>, pos: Pos, egg: &Egg) {
    if let Ok(bytes) = fastnbt::to_bytes(egg) {
        world.set_entity(pos, &bytes);
    }
}

pub fn tick(world: World<'_>, events: &[ContentEvent]) {
    let Some(ids) = ids() else {
        return;
    };
    let mut active = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
    if events.is_empty() && active.is_none() {
        return;
    }
    let positions = active
        .get_or_insert_with(HashMap::new)
        .entry(world.id())
        .or_default();
    let is_egg = |state| ids.stages.iter().any(|stages| stages.contains(&state));
    for event in events {
        if is_egg(event.state) && event.reason != UNLOADED {
            positions.insert(event.position);
        } else if is_egg(event.previous) || event.reason == UNLOADED {
            positions.remove(&event.position);
        }
    }
    positions.retain(|&pos| advance(world, pos, ids));
}

fn cindervane_piebald(female: bool, roll: u64) -> bool {
    roll < if female { 8 } else { 15 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn released_piebald_rates_and_legacy_growth() {
        assert_eq!((0..100).filter(|&r| cindervane_piebald(true, r)).count(), 8);
        assert_eq!(
            (0..100).filter(|&r| cindervane_piebald(false, r)).count(),
            15
        );
        let old = fastnbt::to_bytes(&HashMap::from([("remaining", 240)])).unwrap();
        let growth: Growth = fastnbt::from_bytes(&old).unwrap();
        assert_eq!(growth.remaining, 240);
        assert!(!growth.piebald_on_maturity);
    }
}
