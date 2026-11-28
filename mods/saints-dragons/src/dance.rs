//! DancingEntity: a live jukebox in the 16-block cube, sampled every ten ticks.
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, PoisonError},
};

use moddedmc_api::{
    Pos,
    content::World,
    entities::{ENTITY_ALIVE, Entity, EntityChange, SET_MOTION},
    tick_count,
};
use serde::Deserialize;

use crate::{CONTENT, MOB_AI, MOBS, eggs, flight, ivy, roost};

const RANGE: i32 = 16;
const INTERVAL: u64 = 10;
static ACTIVE: LazyLock<Mutex<HashMap<i32, u64>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Deserialize)]
struct Playback {
    ticks_since_song_started: Option<i64>,
}

fn playing(bytes: &[u8]) -> bool {
    // The engine's JukeboxBlockEntity.write_nbt includes this field only while playing.
    fastnbt::from_bytes::<Playback>(bytes)
        .is_ok_and(|nbt| nbt.ticks_since_song_started.is_some_and(|ticks| ticks >= 0))
}

fn nearby(world: World<'_>, center: Pos) -> bool {
    let Some(state) = CONTENT
        .get()
        .and_then(|content| content.state("minecraft:jukebox[has_record=true]"))
    else {
        return false;
    };
    // Three X slices per ABI batch (3*33*33 < 4096); only read NBT for records.
    // A stopped record is not a playing jukebox.
    let mut positions = Vec::with_capacity(3267);
    let mut states = vec![0; 3267];
    for start in (-RANGE..=RANGE).step_by(3) {
        positions.clear();
        for x in start..=(start + 2).min(RANGE) {
            for y in -RANGE..=RANGE {
                for z in -RANGE..=RANGE {
                    positions.push(Pos {
                        x: center.x + x,
                        y: center.y + y,
                        z: center.z + z,
                    });
                }
            }
        }
        if world.read(&positions, &mut states[..positions.len()])
            && positions.iter().zip(&states).any(|(&pos, &id)| {
                id == state && world.block_entity(pos).is_some_and(|nbt| playing(&nbt))
            })
        {
            return true;
        }
    }
    false
}

pub fn forget(id: i32) {
    ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
}

pub fn sweep(now: u64) {
    ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .retain(|_, seen| now.saturating_sub(*seen) <= 40);
}

pub fn tick(world: World<'_>, mob: Entity<'_>, name: &str) -> bool {
    let (Some(mobs), Some(ai), Some(view)) = (MOBS.get(), MOB_AI.get(), mob.view()) else {
        return false;
    };
    let eligible = mob.flags() & ENTITY_ALIVE != 0
        && ai.target(mob).is_none()
        && if name == ivy::SPECIES {
            ivy::can_dance(world, mob)
        } else {
            eggs::is_juvenile(mob)
                && mobs.passengers(mob).is_empty()
                && roost::sleep_clip(mob.id()).is_none()
        };
    if !eligible {
        forget(mob.id());
        return false;
    }
    let now = tick_count();
    let dancing = if (now.wrapping_add(mob.id() as u64)).is_multiple_of(INTERVAL) {
        nearby(world, flight::cell(view.position))
    } else {
        ACTIVE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(&mob.id())
    };
    if dancing {
        ACTIVE
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(mob.id(), now);
        ai.stop(mob);
        if view.motion[0] != 0.0 || view.motion[2] != 0.0 {
            mob.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0, view.motion[1], 0.0],
                ..EntityChange::default()
            });
        }
    } else {
        forget(mob.id());
    }
    dancing
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopped_record_is_not_playing() {
        let stopped = fastnbt::to_bytes(&HashMap::from([("id", "minecraft:jukebox")])).unwrap();
        let playing_nbt =
            fastnbt::to_bytes(&HashMap::from([("ticks_since_song_started", 0_i64)])).unwrap();
        assert!(!playing(&stopped));
        assert!(playing(&playing_nbt));
    }
}
