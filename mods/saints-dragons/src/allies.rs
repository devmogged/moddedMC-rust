//! GlobalDragonAllySavedData and CompanionCombatRules: owner-scoped UUID trust.
use std::sync::{Mutex, OnceLock, PoisonError};

use moddedmc_api::{
    Command, InterfaceUnavailable, RegistrationError, Registry,
    commands::{Argument, Commands, Node},
    content::World,
    entities::{ENTITY_PLAYER, Entity},
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

use crate::{COMMANDS, ENTITIES, MOBS, taming};

static SAVES: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();
static WRITE: Mutex<()> = Mutex::new(());
pub const MAX: usize = 10;
type Uuid = [u8; 16];
const ADD: u64 = 4;
const REMOVE: u64 = 5;
const LIST: u64 = 6;
const CLEAR: u64 = 7;

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
    let commands = COMMANDS.get().ok_or(RegistrationError::InvalidDefinition)?;
    let mut nodes = Vec::new();
    for (name, tag, username) in [
        ("add", ADD, true),
        ("remove", REMOVE, true),
        ("list", LIST, false),
        ("clear", CLEAR, false),
    ] {
        let parent = nodes.len() as u32;
        nodes.push(Node {
            parent: None,
            name,
            argument: Argument::Literal,
            tag: None,
        });
        nodes.push(Node {
            parent: Some(parent),
            name: "drake",
            argument: Argument::Entity,
            tag: (!username).then_some(tag),
        });
        if username {
            nodes.push(Node {
                parent: Some(parent + 1),
                name: "username",
                argument: Argument::String,
                tag: Some(tag),
            });
        }
    }
    commands.add(reg, "dragonally", "Manage player allies", 2, &nodes)
}

const SUCCESS: &str = "Successfully managed player ally";
const INVALID: &str = "Invalid username provided";
const NOT_ALLY: &str = "Player is not an ally";

fn player(commands: Commands, command: &Command<'_>, key: &str) -> Option<Ally> {
    let mut result = None;
    commands.online_player(command, key, |_, entity, _| {
        if let (Some(player), Some(uuid)) =
            (entity.as_player(), MOBS.get().and_then(|m| m.uuid(entity)))
        {
            result = Some(Ally {
                uuid,
                name: player.name().to_owned(),
            });
        }
    });
    result
}

fn add_named(
    world: World<'_>,
    commands: Commands,
    command: &Command<'_>,
    owner: &Ally,
    username: &str,
) -> &'static str {
    let username = username.trim();
    if !valid_name(username) {
        return INVALID;
    }
    if username.eq_ignore_ascii_case(&owner.name) {
        return "You are already the owner of this dragon!";
    }
    let mut result = INVALID;
    let changed = update(world, |entries| {
        let owned = entries.iter().filter(|entry| entry.owner == owner.uuid);
        if owned
            .clone()
            .any(|entry| entry.ally.name.eq_ignore_ascii_case(username))
        {
            result = "Player is already an ally";
        } else if owned.count() >= MAX {
            result = "Maximum player ally limit reached";
        } else if let Some(ally) = player(commands, command, username) {
            if !ally.name.eq_ignore_ascii_case(username) {
                result = "Username-UUID validation failed";
            } else if add(entries, owner.uuid, ally) {
                result = SUCCESS;
                return true;
            } else {
                result = "Player is already an ally";
            }
        } else {
            result = "Player not found on server";
        }
        false
    });
    if result == SUCCESS && !changed {
        INVALID
    } else {
        result
    }
}

fn remove_named(world: World<'_>, owner: Uuid, username: &str) -> &'static str {
    let username = username.trim();
    if username.is_empty() {
        return INVALID;
    }
    if update(world, |entries| {
        let candidate = entries
            .iter()
            .find(|entry| entry.owner == owner && entry.ally.name.eq_ignore_ascii_case(username))
            .map(|entry| entry.ally.uuid);
        candidate.is_some_and(|uuid| remove(entries, owner, uuid))
    }) {
        SUCCESS
    } else {
        NOT_ALLY
    }
}

/// DragonAllyCommand: operator level two; arguments and output match the Java command.
pub fn command(tag: u64, command: &Command<'_>) -> Option<i32> {
    if !matches!(tag, ADD | REMOVE | LIST | CLEAR) {
        return None;
    }
    let (Some(&commands), Some(mobs)) = (COMMANDS.get(), MOBS.get()) else {
        return Some(0);
    };
    let username = if matches!(tag, ADD | REMOVE) {
        let Some(username) = commands.string(command, "username") else {
            return Some(0);
        };
        username
    } else {
        String::new()
    };
    let mut result = 0;
    commands.select(command, "drake", Argument::Entity, |world, entity, name| {
        let species = mobs
            .kind(entity)
            .and_then(|kind| kind.strip_prefix("saintsdragons:"))
            .filter(|kind| {
                matches!(
                    *kind,
                    "cindervane"
                        | "raevyx"
                        | "stegonaut"
                        | "varasuchus"
                        | "ignivorus"
                        | "volitans"
                        | "nulljaw"
                        | "atroxiia"
                )
            });
        let Some(species) = species else {
            commands.failure(command, &format!("Entity {name} is not a dragon"));
            if tag == LIST {
                result += 1;
            }
            return;
        };
        // The carrier's vanilla name is replaced by the mod species name.
        let name = if matches!(
            name,
            "Ravager" | "Spider" | "entity.minecraft.ravager" | "entity.minecraft.spider"
        ) {
            crate::roster::display(species)
        } else {
            name
        };
        let owner = taming::owner(entity).and_then(|uuid| {
            let mut key = String::with_capacity(32);
            use std::fmt::Write as _;
            for byte in uuid {
                let _ = write!(key, "{byte:02x}");
            }
            player(commands, command, &key)
        });
        match tag {
            ADD | REMOVE => {
                let message = owner.as_ref().map_or(INVALID, |owner| {
                    if tag == ADD {
                        add_named(world, commands, command, owner, &username)
                    } else {
                        remove_named(world, owner.uuid, &username)
                    }
                });
                commands.feedback(command, &format!("Dragon {name}: {username} - {message}"));
                if message == SUCCESS {
                    result += 1;
                }
            }
            LIST => {
                let allies = owner.map_or_else(Vec::new, |owner| list(world, owner.uuid));
                commands.feedback(
                    command,
                    &format!("Player allies for {name} ({}/{}):", allies.len(), MAX),
                );
                if allies.is_empty() {
                    commands.feedback(command, "No player allies set");
                } else {
                    for ally in allies {
                        commands.feedback(command, &format!("  - {}", ally.name));
                    }
                }
                result += 1;
            }
            CLEAR => {
                if let Some(owner) = owner {
                    update(world, |entries| {
                        entries.retain(|entry| entry.owner != owner.uuid);
                        true
                    });
                }
                commands.feedback(command, &format!("Cleared all player allies for {name}"));
                result += 1;
            }
            _ => {}
        }
    });
    Some(result)
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
        taming::companion_owner(target)
    };
    candidate.is_some_and(|candidate| trusts_uuid(world, owner, candidate))
}

/// The owner-trust part of DragonEntity.isAlly; species-specific exclusions stay in their AI.
pub fn is_ally(world: World<'_>, mob: Entity<'_>, target: Entity<'_>) -> bool {
    taming::owner(mob).is_some_and(|owner| trusts(world, owner, target))
        || taming::is_kin(mob, target)
        || taming::companion_ally(mob, target)
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
