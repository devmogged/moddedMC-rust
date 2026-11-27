//! GlobalDragonAllySavedData and CompanionCombatRules: owner-scoped UUID trust.
use std::sync::{Mutex, OnceLock, PoisonError};

use moddedmc_api::{
    InterfaceUnavailable, RegistrationError, Registry,
    content::World,
    entities::{ENTITY_PLAYER, Entity},
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

use crate::{ENTITIES, MOBS, taming};

static SAVES: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();
static WRITE: Mutex<()> = Mutex::new(());
pub const MAX: usize = 10;
type Uuid = [u8; 16];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ally {
    pub uuid: Uuid,
    pub name: String,
}

#[derive(Serialize, Deserialize)]
struct Entry {
    owner: Uuid,
    ally: Ally,
}

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = SAVES.set(saved_data::require()?);
    Ok(())
}

pub fn register(reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saves = SAVES.get().ok_or(RegistrationError::InvalidDefinition)?;
    let _ = KEY.set(saves.register(reg, "global_dragon_allies")?);
    Ok(())
}

fn read(world: World<'_>) -> Option<Vec<Entry>> {
    let (saves, key) = (SAVES.get()?, *KEY.get()?);
    match saves.read(&world, key) {
        Some(bytes) => serde_json::from_slice(&bytes).ok(),
        None => Some(Vec::new()),
    }
}

pub fn list(world: World<'_>, owner: Uuid) -> Vec<Ally> {
    let mut list: Vec<_> = read(world)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.owner == owner)
        .map(|entry| entry.ally)
        .collect();
    list.sort_by_key(|ally| ally.name.to_ascii_lowercase());
    list
}

fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn add(entries: &mut Vec<Entry>, owner: Uuid, ally: Ally) -> bool {
    if owner == ally.uuid || !valid_name(&ally.name) {
        return false;
    }
    let owned = entries.iter().filter(|entry| entry.owner == owner);
    if owned.clone().any(|entry| entry.ally.uuid == ally.uuid) || owned.count() >= MAX {
        return false;
    }
    entries.push(Entry { owner, ally });
    true
}

fn remove(entries: &mut Vec<Entry>, owner: Uuid, candidate: Uuid) -> bool {
    let before = entries.len();
    entries.retain(|entry| entry.owner != owner || entry.ally.uuid != candidate);
    entries.len() != before
}

fn update(world: World<'_>, change: impl FnOnce(&mut Vec<Entry>) -> bool) -> bool {
    let _lock = WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let (Some(mut entries), Some(saves), Some(&key)) = (read(world), SAVES.get(), KEY.get()) else {
        return false;
    };
    change(&mut entries)
        && serde_json::to_vec(&entries).is_ok_and(|bytes| saves.write(&world, key, &bytes))
}

/// Only a currently online, named player can be added; recheck the UUID on click.
pub fn online(world: World<'_>) -> Vec<Ally> {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return Vec::new();
    };
    let mut players: Vec<_> = entities
        .players(world)
        .into_iter()
        .filter_map(|id| {
            entities
                .with_entity(world, id, |entity| {
                    let name = entity.as_player()?.name().to_owned();
                    valid_name(&name).then_some(Ally {
                        uuid: mobs.uuid(entity)?,
                        name,
                    })
                })
                .flatten()
        })
        .collect();
    players.sort_by_key(|ally| ally.name.to_ascii_lowercase());
    players
}

pub fn add_online(world: World<'_>, owner: Uuid, candidate: Uuid) -> bool {
    let Some(ally) = online(world)
        .into_iter()
        .find(|ally| ally.uuid == candidate)
    else {
        return false;
    };
    update(world, |entries| add(entries, owner, ally))
}

pub fn remove_saved(world: World<'_>, owner: Uuid, candidate: Uuid) -> bool {
    update(world, |entries| remove(entries, owner, candidate))
}

fn trusted(entries: &[Entry], owner: Uuid, candidate: Uuid) -> bool {
    owner == candidate
        || entries
            .iter()
            .any(|e| e.owner == owner && e.ally.uuid == candidate)
}

pub fn trusts_uuid(world: World<'_>, owner: Uuid, candidate: Uuid) -> bool {
    owner == candidate || read(world).is_some_and(|entries| trusted(&entries, owner, candidate))
}

/// CompanionCombatRules.isTrusted: the player's UUID, or the companion's owner's UUID.
pub fn trusts(world: World<'_>, owner: Uuid, target: Entity<'_>) -> bool {
    let candidate = if target.view().is_some_and(|v| v.kind & ENTITY_PLAYER != 0) {
        MOBS.get().and_then(|mobs| mobs.uuid(target))
    } else {
        // shortcut: generic vanilla OwnableEntity owner UUID is not exposed.
        taming::owner(target)
    };
    candidate.is_some_and(|candidate| trusts_uuid(world, owner, candidate))
}

/// The owner-trust part of DragonEntity.isAlly; species-specific exclusions stay in their AI.
pub fn is_ally(world: World<'_>, mob: Entity<'_>, target: Entity<'_>) -> bool {
    taming::owner(mob).is_some_and(|owner| trusts(world, owner, target))
        || taming::is_kin(mob, target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ally_limit_direction_and_owner_isolation_survive_serialization() {
        let mut entries = Vec::new();
        let owner = [0; 16];
        let ally = |i| Ally {
            uuid: [i; 16],
            name: format!("Player_{i}"),
        };
        assert!(!add(&mut entries, owner, ally(0)));
        assert!(!add(
            &mut entries,
            owner,
            Ally {
                uuid: [11; 16],
                name: "bad name".into()
            }
        ));
        for i in 1..=10 {
            assert!(add(&mut entries, owner, ally(i)));
        }
        assert!(!add(&mut entries, owner, ally(1)));
        assert!(!add(&mut entries, owner, ally(11)));
        assert!(add(&mut entries, [12; 16], ally(1)));
        let bytes = serde_json::to_vec(&entries).unwrap();
        let mut entries: Vec<Entry> = serde_json::from_slice(&bytes).unwrap();
        assert!(trusted(&entries, owner, [1; 16]));
        assert!(trusted(&entries, owner, owner));
        assert!(!trusted(&entries, [1; 16], owner));
        assert!(!trusted(&entries, [12; 16], [2; 16]));
        assert!(remove(&mut entries, owner, [1; 16]));
        assert!(!trusted(&entries, owner, [1; 16]));
        assert!(trusted(&entries, [12; 16], [1; 16]));
        assert!(add(&mut entries, owner, ally(11)));
    }
}
