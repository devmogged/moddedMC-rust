//! The Draconian Nucleus encounter and its controller.
use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex, OnceLock, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{
        Block, BlockUpdate, Content, ContentEvent, ShapeBox, State, UNLOADED, UNLOADED_STATE,
        UPDATE_ALL, World,
    },
    effects::SOUND_BLOCKS,
    entities::{ENTITY_PLAYER, ENTITY_SNEAKING, Entity},
    interaction::{UseItem, UseMob, UseOnBlock, UseResult},
    items::{Item, ItemDefaults},
    mobs::{self, MobLoot},
    tick_count,
};
use serde::{Deserialize, Serialize};

use crate::{CONTENT, EFFECTS, ENTITIES, MOBS, flight, roster, swarm};

pub const BLOCK: &str = "saintsdragons:draconian_nucleus";
pub const BLOCK_NAME: &str = "Draconian Nucleus";
pub const CONTROLLER: &str = "saintsdragons:draconian_controller";
pub const CONTROLLER_NAME: &str = "Draconian Controller";
const ENCOUNTER: &str = "saintsdragons:nucleus_encounter";
const WAVE_SIZES: [u8; 3] = [6, 9, 12];
const SPAWN_OFFSETS: [[i32; 3]; 8] = [
    [0, 3, 0],
    [2, 3, 0],
    [-2, 3, 0],
    [0, 3, 2],
    [0, 3, -2],
    [2, 4, 2],
    [-2, 4, -2],
    [0, 5, 0],
];
const SHAPE: &[ShapeBox] = &[ShapeBox {
    min: [2; 3],
    max: [14; 3],
}];

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct Encounter {
    owner: Option<[i8; 16]>,
    wave: u8,
    spawned: u8,
    members: Vec<[i8; 16]>,
    delay: u16,
    cooldown: u16,
    complete: bool,
}

#[derive(Deserialize, Serialize)]
struct Link {
    position: [i32; 3],
}

// Player actions and world ticks both update encounter NBT.
static MUTATIONS: Mutex<()> = Mutex::new(());
static LAST_USED: LazyLock<Mutex<HashMap<[i8; 16], u64>>> = LazyLock::new(Mutex::default);
static LOOT: OnceLock<MobLoot> = OnceLock::new();

pub fn require() -> Result<(), moddedmc_api::InterfaceUnavailable> {
    let _ = LOOT.set(mobs::require_loot()?);
    Ok(())
}

static STATE_IDS: OnceLock<(u32, u32)> = OnceLock::new();
static NUCLEI: LazyLock<Mutex<HashMap<usize, HashSet<Pos>>>> = LazyLock::new(Mutex::default);

pub fn register(
    content: Content,
    items: ItemDefaults,
    reg: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let original = cfg!(feature = "original-assets");
    let model = if original {
        "saintsdragons:block/draconian_nucleus"
    } else {
        "minecraft:block/obsidian"
    };
    let state = |properties, physical| State {
        properties,
        physical,
        model,
        rotating_model: "",
        x: 0,
        y: 0,
        orientation: [0.0, 0.0, 0.0, 1.0],
        shape: SHAPE,
    };
    content.add_block(
        reg,
        &Block {
            id: BLOCK,
            display_name: BLOCK_NAME,
            entity: BLOCK,
            item_model: if original {
                "saintsdragons:item/draconian_nucleus"
            } else {
                "minecraft:item/obsidian"
            },
            states: &[
                state("harvestable=false", "minecraft:bedrock"),
                state("harvestable=true", "minecraft:obsidian"),
            ],
        },
        None,
    )?;
    let components = HashMap::from([("minecraft:max_stack_size", Value::Int(1))]);
    let defaults = fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String("minecraft:paper".into())),
        (
            "components",
            Value::Compound(
                components
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v))
                    .collect(),
            ),
        ),
    ]))
    .map_err(|_| RegistrationError::InvalidDefinition)?;
    items.add_item(
        reg,
        &Item {
            id: CONTROLLER,
            display_name: CONTROLLER_NAME,
            carrier: "minecraft:paper",
            model: if original {
                "saintsdragons:item/draconian_controller"
            } else {
                "minecraft:item/ender_pearl"
            },
        },
        &defaults,
    )
}

fn states() -> Option<(u32, u32)> {
    if let Some(ids) = STATE_IDS.get() {
        return Some(*ids);
    }
    let content = CONTENT.get()?;
    let ids = (
        content.state("saintsdragons:draconian_nucleus[harvestable=false]")?,
        content.state("saintsdragons:draconian_nucleus[harvestable=true]")?,
    );
    Some(*STATE_IDS.get_or_init(|| ids))
}

fn track(world: World<'_>, pos: Pos) {
    NUCLEI
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(world.id())
        .or_default()
        .insert(pos);
}

fn set_harvestable(world: World<'_>, pos: Pos, value: bool) {
    if let Some((locked, open)) = states() {
        let state = if value { open } else { locked };
        let mut current = [0];
        if world.read(&[pos], &mut current)
            && current[0] != state
            && [locked, open].contains(&current[0])
        {
            world.write(&[BlockUpdate {
                position: pos,
                state,
                flags: UPDATE_ALL,
            }]);
        }
    }
}

fn load(world: World<'_>, pos: Pos) -> Encounter {
    world
        .entity(pos)
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
        .unwrap_or_default()
}

fn save(world: World<'_>, pos: Pos, state: &Encounter) -> bool {
    fastnbt::to_bytes(state).is_ok_and(|bytes| world.set_entity(pos, &bytes))
}

pub fn placed(world: World<'_>, player: Entity<'_>, pos: Pos, state: u32) {
    let Some((locked, open)) = states() else {
        return;
    };
    if ![locked, open].contains(&state) {
        return;
    }
    let _mutation = MUTATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(owner) = MOBS.get().and_then(|mobs| mobs.uuid(player)) else {
        return;
    };
    set_harvestable(world, pos, true);
    save(
        world,
        pos,
        &Encounter {
            owner: Some(owner.map(|byte| byte as i8)),
            ..Encounter::default()
        },
    );
    track(world, pos);
}

fn center(pos: Pos) -> [f64; 3] {
    [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    ]
}

fn sound(world: World<'_>, pos: Pos, event: &str) {
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(world, center(pos), event, SOUND_BLOCKS, 1.0, 1.0);
    }
}

fn begin(world: World<'_>, pos: Pos, data: &mut Encounter) {
    data.wave = 1;
    data.spawned = 0;
    data.delay = 219;
    sound(
        world,
        pos,
        if cfg!(feature = "original-assets") {
            "saintsdragons:draconian_nucleus_summon"
        } else {
            "minecraft:block.respawn_anchor.charge"
        },
    );
    set_harvestable(world, pos, false);
}

fn survival_near(world: World<'_>, pos: Pos) -> bool {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return false;
    };
    let at = center(pos);
    entities
        .entities_in(world, at.map(|v| v - 20.0), at.map(|v| v + 20.0), -1)
        .into_iter()
        .any(|id| {
            entities.with_entity(world, id, |e| {
                e.view().is_some_and(|v| v.kind & ENTITY_PLAYER != 0)
                    && mobs.game_mode(e) == Some(0)
                    && e.view().is_some_and(|v| {
                        flight::length_squared(flight::offset(at, v.position)) <= 400.0
                    })
            }) == Some(true)
        })
}

pub fn tick(world: World<'_>, time: u64, events: &[ContentEvent]) {
    let Some((locked, open)) = states() else {
        return;
    };
    let positions = {
        let mut index = NUCLEI.lock().unwrap_or_else(PoisonError::into_inner);
        if events.is_empty() && !index.contains_key(&world.id()) {
            return;
        }
        let positions = index.entry(world.id()).or_default();
        for event in events {
            if event.reason != UNLOADED && [locked, open].contains(&event.state) {
                positions.insert(event.position);
            } else if [locked, open].contains(&event.previous) {
                positions.remove(&event.position);
            }
        }
        positions.iter().copied().collect::<Vec<_>>()
    };
    for pos in positions {
        let _mutation = MUTATIONS.lock().unwrap_or_else(PoisonError::into_inner);
        let mut data = load(world, pos);
        if data.complete {
            continue;
        }
        if data.wave == 0 {
            if data.cooldown > 0 {
                data.cooldown -= 1;
            } else if data.owner.is_none() && time.is_multiple_of(20) && survival_near(world, pos) {
                begin(world, pos, &mut data);
            } else {
                continue;
            }
        } else if data.delay > 0 {
            data.delay -= 1;
        } else if let Some(&wave_size) = WAVE_SIZES.get(usize::from(data.wave - 1)) {
            if data.spawned < wave_size {
                if let Some(uuid) = spawn(world, pos, data.spawned, data.owner.is_some()) {
                    if data.wave == 1 && data.spawned == 0 {
                        award_near(world, pos, "encounter_draconian_swarm");
                    }
                    if data.wave > 1 && data.spawned == 0 && cfg!(feature = "original-assets") {
                        sound(
                            world,
                            pos,
                            "saintsdragons:draconian_nucleus_summon_phase_2_and_3",
                        );
                    }
                    data.spawned += 1;
                    data.members.push(uuid);
                }
                data.delay = 9;
            } else if data.members.is_empty() {
                if data.wave == 3 {
                    data.complete = true;
                    set_harvestable(world, pos, true);
                    award_near(world, pos, "vanquish_draconian_swarm");
                } else {
                    data.wave += 1;
                    data.spawned = 0;
                    data.delay = 39;
                }
            } else {
                continue;
            }
        } else {
            continue;
        }
        save(world, pos, &data);
    }
}

fn spawn(world: World<'_>, origin: Pos, number: u8, controller_summoned: bool) -> Option<[i8; 16]> {
    let (mobs, entities) = (MOBS.get()?, ENTITIES.get()?);
    let (member, _) = swarm::MEMBERS[usize::from(number) % swarm::MEMBERS.len()];
    for i in 0..SPAWN_OFFSETS.len() {
        let [x, y, z] = SPAWN_OFFSETS[(usize::from(number) + i) % SPAWN_OFFSETS.len()];
        let pos = Pos {
            x: origin.x + x,
            y: origin.y + y,
            z: origin.z + z,
        };
        let free = |pos| {
            world
                .collision_shape(pos)
                .is_some_and(|shape| shape.is_empty())
        };
        if !free(pos)
            || !free(Pos {
                y: pos.y + 1,
                ..pos
            })
        {
            continue;
        }
        let at = [
            f64::from(pos.x) + 0.5,
            f64::from(pos.y),
            f64::from(pos.z) + 0.5,
        ];
        let id = roster::spawn(world, &format!("saintsdragons:{member}"), at)?;
        return entities.with_entity(world, id, |mob| {
            let uuid = mobs.uuid(mob)?.map(|byte| byte as i8);
            let link = fastnbt::to_bytes(&Link {
                position: [origin.x, origin.y, origin.z],
            })
            .ok()?;
            if !mob.set_data(ENCOUNTER, &link)
                || (controller_summoned && !LOOT.get().is_some_and(|loot| loot.suppress(mob)))
            {
                mob.discard();
                return None;
            }
            mobs.make_persistent(mob);
            Some(uuid)
        })?;
    }
    None
}

pub fn combat_center(mob: Entity<'_>) -> Option<[f64; 3]> {
    let link: Link = fastnbt::from_bytes(&mob.data(ENCOUNTER)?).ok()?;
    Some(link.position.map(|v| f64::from(v) + 0.5))
}

pub fn clean_up(world: World<'_>, mob: Entity<'_>) -> bool {
    if !(tick_count() + mob.id() as u64).is_multiple_of(20) {
        return false;
    }
    let Some(link) = mob
        .data(ENCOUNTER)
        .and_then(|bytes| fastnbt::from_bytes::<Link>(&bytes).ok())
    else {
        return false;
    };
    let Some(uuid) = MOBS.get().and_then(|mobs| mobs.uuid(mob)) else {
        return false;
    };
    let [x, y, z] = link.position;
    let pos = Pos { x, y, z };
    let _mutation = MUTATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut state = [UNLOADED_STATE];
    if !world.read(&[pos], &mut state) || state[0] == UNLOADED_STATE {
        return false;
    }
    let active = states().is_some_and(|(a, b)| [a, b].contains(&state[0]))
        && load(world, pos)
            .members
            .contains(&uuid.map(|byte| byte as i8));
    !active && mob.discard()
}

pub fn defeated(world: World<'_>, mob: Entity<'_>) {
    let Some(link) = mob
        .data(ENCOUNTER)
        .and_then(|bytes| fastnbt::from_bytes::<Link>(&bytes).ok())
    else {
        return;
    };
    let Some(uuid) = MOBS.get().and_then(|mobs| mobs.uuid(mob)) else {
        return;
    };
    let [x, y, z] = link.position;
    let pos = Pos { x, y, z };
    let _mutation = MUTATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut data = load(world, pos);
    let before = data.members.len();
    data.members.retain(|id| id != &uuid.map(|byte| byte as i8));
    if before != data.members.len() {
        save(world, pos, &data);
    }
}

/// `awardNearbyAdvancement`: living players in the nucleus block's box inflated by 64.
fn award_near(world: World<'_>, pos: Pos, advancement: &str) {
    crate::advancements::award_near(world, center(pos), 64.5, true, advancement, advancement);
}

fn change_encounters(world: World<'_>, player: Entity<'_>, sneaking: bool) -> bool {
    let (Some(view), Some(mobs), Some(entities)) = (player.view(), MOBS.get(), ENTITIES.get())
    else {
        return false;
    };
    let Some(owner) = mobs.uuid(player).map(|id| id.map(|byte| byte as i8)) else {
        return false;
    };
    let nearby = NUCLEI
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&world.id())
        .map(|positions| {
            positions
                .iter()
                .copied()
                .filter(|pos| {
                    flight::length_squared(flight::offset(center(*pos), view.position))
                        <= 64.0 * 64.0
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut changed = false;
    for pos in &nearby {
        let mut data = load(world, *pos);
        if data.owner != Some(owner) || data.wave == 0 || data.complete {
            continue;
        }
        for member in data.members.drain(..) {
            if let Some(id) = mobs.by_uuid(world, &member.map(|byte| byte as u8)) {
                entities.with_entity(world, id, |mob| mob.discard());
            }
        }
        data = Encounter {
            owner: Some(owner),
            cooldown: 1200,
            ..Encounter::default()
        };
        set_harvestable(world, *pos, true);
        changed |= save(world, *pos, &data);
    }
    if changed || sneaking {
        return changed;
    }
    for pos in nearby {
        let mut data = load(world, pos);
        if data.owner != Some(owner) || data.wave > 0 || data.complete || data.cooldown > 0 {
            continue;
        }
        begin(world, pos, &mut data);
        changed |= save(world, pos, &data);
        crate::advancements::award(player, "summon_draconian_swarm", "summon_draconian_swarm");
    }
    changed
}

fn activate(world: World<'_>, player: Entity<'_>, sneaking: bool) -> bool {
    let Some(owner) = MOBS
        .get()
        .and_then(|mobs| mobs.uuid(player))
        .map(|id| id.map(|b| b as i8))
    else {
        return false;
    };
    let _mutation = MUTATIONS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut last = LAST_USED.lock().unwrap_or_else(PoisonError::into_inner);
    let now = tick_count();
    if last.get(&owner).is_some_and(|until| *until > now)
        || !change_encounters(world, player, sneaking)
    {
        return false;
    }
    last.retain(|_, until| *until > now);
    last.insert(owner, now + 40);
    true
}

pub fn use_on_mob(click: &UseMob<'_>) -> UseResult {
    if click
        .player
        .is_some_and(|player| activate(click.world, player, click.sneaking))
    {
        click.cooldown(CONTROLLER, 40);
        UseResult::Success
    } else {
        UseResult::Fail
    }
}

pub fn use_item(click: &mut UseItem<'_>) -> UseResult {
    if click
        .player
        .is_some_and(|p| activate(click.world, p, p.flags() & ENTITY_SNEAKING != 0))
    {
        click.cooldown(40);
        UseResult::Success
    } else {
        UseResult::Fail
    }
}

pub fn use_on_block(click: &UseOnBlock<'_>) -> Option<UseResult> {
    if click.item != CONTROLLER {
        return None;
    }
    if click
        .player
        .is_some_and(|p| activate(click.world, p, click.sneaking))
    {
        click.cooldown(40);
        Some(UseResult::Success)
    } else {
        Some(UseResult::Fail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encounter_nbt_round_trips_owner_and_active_members() {
        let owner = [-101; 16];
        let state = Encounter {
            owner: Some(owner),
            wave: 1,
            spawned: 1,
            members: vec![[-30; 16]],
            delay: 220,
            ..Encounter::default()
        };
        let bytes = fastnbt::to_bytes(&state).unwrap();
        let restored: Encounter = fastnbt::from_bytes(&bytes).unwrap();
        assert_eq!(restored.owner, Some(owner));
        assert_eq!(restored.members, state.members);
        assert_eq!(restored.delay, 220);
    }
}
