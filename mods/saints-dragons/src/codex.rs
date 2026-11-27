//! Persistent owner records for the Draconic Codex and the stock-client reader.
use std::{
    collections::HashMap,
    sync::{
        LazyLock, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use fastnbt::Value;
use moddedmc_api::{
    InterfaceUnavailable, Menu, MenuButton, Player, RegistrationError, Registry,
    content::World,
    entities::Entity,
    interaction::{UseItem, UseMob, UseResult},
    items::{Item, ItemDefaults},
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

use crate::{ENTITIES, MOBS, appearance::Appearance, binders, eggs, roster, taming};

pub const ID: &str = "saintsdragons:draconic_codex";
pub const NAME: &str = "Draconic Codex";
static SAVES: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();
static RECORDS: Mutex<()> = Mutex::new(());
const ACTION: &str = "saintsdragons:codex/";
static NONCE: AtomicU64 = AtomicU64::new(1);
struct Session {
    nonce: u64,
    expires: u64,
    entries: Vec<[u8; 16]>,
}
static SESSIONS: LazyLock<Mutex<HashMap<[u8; 16], Session>>> = LazyLock::new(Mutex::default);

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Record {
    owner: [u8; 16],
    dragon: [u8; 16],
    name: String,
    species: String,
    health: [f32; 2],
    #[serde(default = "full_hunger")]
    hunger: i32,
    happiness: u8,
    coating: String,
    young: bool,
    #[serde(default)]
    female: Option<bool>,
    #[serde(default)]
    biome: Option<String>,
    #[serde(default)]
    brushing: Option<u16>,
    dimension: String,
    position: [i32; 3],
    bound: bool,
}

fn full_hunger() -> i32 {
    crate::hunger::MAX
}

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = SAVES.set(saved_data::require()?);
    Ok(())
}

pub fn register(items: ItemDefaults, reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saves = SAVES.get().ok_or(RegistrationError::InvalidDefinition)?;
    let _ = KEY.set(saves.register(reg, "draconic_codex")?);
    let defaults = fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String("minecraft:book".into())),
        (
            "components",
            Value::Compound(HashMap::from([(
                "minecraft:max_stack_size".into(),
                Value::Int(1),
            )])),
        ),
    ]))
    .map_err(|_| RegistrationError::InvalidDefinition)?;
    items.add_item(
        reg,
        &Item {
            id: ID,
            display_name: NAME,
            carrier: "minecraft:book",
            model: if cfg!(feature = "original-assets") {
                "saintsdragons:item/draconic_codex"
            } else {
                "minecraft:item/book"
            },
        },
        &defaults,
    )
}

fn records(world: World<'_>) -> Option<Vec<Record>> {
    let (saves, key) = (SAVES.get()?, *KEY.get()?);
    match saves.read(&world, key) {
        Some(data) => serde_json::from_slice(&data).ok(),
        None => Some(Vec::new()),
    }
}

fn write(world: World<'_>, entries: &[Record]) {
    if let (Some(saves), Some(&key), Ok(data)) =
        (SAVES.get(), KEY.get(), serde_json::to_vec(entries))
    {
        saves.write(&world, key, &data);
    }
}

pub(crate) fn snapshot(
    world: World<'_>,
    dragon: Entity<'_>,
    bound_name: Option<&str>,
) -> Option<Record> {
    let mobs = MOBS.get()?;
    let kind = mobs.kind(dragon)?.strip_prefix("saintsdragons:")?;
    let (owner, happiness) = taming::codex_stats(dragon)?;
    let (current, max) = mobs.health(dragon)?;
    let view = dragon.view()?;
    let species = roster::names().find(|(id, _)| *id == kind)?.1;
    let name = bound_name
        .map(str::to_owned)
        .or_else(|| binders::saved_name(dragon))
        .unwrap_or_else(|| species.to_owned());
    Some(Record {
        owner,
        dragon: mobs.uuid(dragon)?,
        name,
        species: species.to_owned(),
        health: [current, max],
        hunger: crate::hunger::level(dragon),
        happiness,
        coating: Appearance::load(dragon)
            .texture(kind)
            .unwrap_or_else(|| "default".into()),
        young: eggs::is_juvenile(dragon),
        female: Appearance::gender(dragon),
        biome: moddedmc_api::worldgen_lookup::require()
            .ok()
            .and_then(|lookup| lookup.biomes(world))
            .and_then(|biomes| {
                biomes
                    .into_iter()
                    .find(|id| world.in_biome(crate::flight::cell(view.position), id) == Some(true))
            }),
        brushing: taming::brushing_progress(dragon),
        dimension: world.dimension_id()?.to_owned(),
        position: view.position.map(|v| v.floor() as i32),
        bound: bound_name.is_some(),
    })
}

pub fn remember(world: World<'_>, dragon: Entity<'_>, bound_name: Option<&str>) {
    let Some(record) = snapshot(world, dragon, bound_name) else {
        return;
    };
    remember_record(world, record);
}

pub(crate) fn remember_record(world: World<'_>, record: Record) {
    store_record(world, record, true);
}

fn store_record(world: World<'_>, record: Record, insert: bool) {
    let _guard = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(mut records) = records(world) else {
        return;
    };
    if let Some(old) = records
        .iter_mut()
        .find(|old| old.owner == record.owner && old.dragon == record.dragon)
    {
        if *old == record {
            return;
        }
        *old = record;
    } else if insert {
        records.push(record);
    } else {
        return;
    }
    write(world, &records);
}

pub fn remove(world: World<'_>, dragon: Entity<'_>) {
    let Some((owner, _)) = taming::codex_stats(dragon) else {
        return;
    };
    let Some(id) = MOBS.get().and_then(|mobs| mobs.uuid(dragon)) else {
        return;
    };
    remove_record(world, owner, id);
}

/// MessageDraconicCodexRemoveEntry: only the requesting owner's record is removed.
fn remove_record(world: World<'_>, owner: [u8; 16], dragon: [u8; 16]) {
    let _guard = RECORDS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(mut records) = records(world) else {
        return;
    };
    let before = records.len();
    retain_other_records(&mut records, owner, dragon);
    if records.len() != before {
        write(world, &records);
    }
}

fn retain_other_records(records: &mut Vec<Record>, owner: [u8; 16], dragon: [u8; 16]) {
    records.retain(|record| record.owner != owner || record.dragon != dragon);
}

fn session(owner: [u8; 16], entries: Vec<[u8; 16]>) -> u64 {
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let now = moddedmc_api::tick_count();
    let mut sessions = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
    sessions.retain(|_, session| session.expires > now);
    sessions.insert(
        owner,
        Session {
            nonce,
            expires: now + 1200,
            entries,
        },
    );
    nonce
}

fn button(label: String, nonce: u64, operation: &str, index: usize) -> MenuButton {
    MenuButton {
        label,
        action: format!("{ACTION}{nonce}/{operation}/{index}"),
        tooltip: String::new(),
    }
}

fn show(world: World<'_>, viewer: Entity<'_>, selected: Option<Entity<'_>>) -> UseResult {
    let (Some(player), Some(mobs), Some(entities)) =
        (viewer.as_player(), MOBS.get(), ENTITIES.get())
    else {
        return UseResult::Fail;
    };
    let Some(owner) = mobs.uuid(viewer) else {
        return UseResult::Fail;
    };
    let selected_id = selected
        .filter(|mob| taming::codex_stats(*mob).is_some_and(|(id, _)| id == owner))
        .and_then(|mob| {
            remember(world, mob, None);
            mobs.uuid(mob)
        });
    let Some(records) = records(world) else {
        return UseResult::Fail;
    };
    let mut owned: Vec<_> = records
        .into_iter()
        .filter(|record| record.owner == owner)
        .collect();
    for record in &mut owned {
        if record.bound {
            continue;
        }
        if let Some(id) = mobs.by_uuid(world, &record.dragon)
            && let Some(Some(current)) =
                entities.with_entity(world, id, |dragon| snapshot(world, dragon, None))
            && current.owner == owner
        {
            store_record(world, current.clone(), false);
            *record = current;
        }
    }
    owned.sort_by(|a, b| {
        (Some(a.dragon) != selected_id, &a.name).cmp(&(Some(b.dragon) != selected_id, &b.name))
    });
    let mut lines = Vec::new();
    for record in owned.iter().take(24) {
        let [x, y, z] = record.position;
        let [current, max] = record.health;
        let status = if record.bound {
            "In binder"
        } else if record.young {
            "Hatchling"
        } else {
            "Adult"
        };
        lines.push(format!(
            "{} ({}) ? {}\nHealth {:.0}/{:.0}, hunger {}%, happiness {}%, {}\n{} ({x}, {y}, {z})",
            record.name,
            record.species,
            status,
            current,
            max,
            record.hunger,
            record.happiness,
            record.coating.replace('_', " "),
            record.dimension
        ));
    }
    if owned.len() > 24 {
        lines.push(format!(
            "{} more dragons in your records.",
            owned.len() - 24
        ));
    }
    let nonce = session(
        owner,
        owned.iter().take(24).map(|record| record.dragon).collect(),
    );
    let mut buttons: Vec<_> = owned
        .iter()
        .take(24)
        .enumerate()
        .map(|(index, record)| button(record.name.clone(), nonce, "select", index))
        .collect();
    buttons.push(MenuButton {
        label: "Done".to_owned(),
        action: "saintsdragons:codex/done".to_owned(),
        tooltip: String::new(),
    });
    player.show_menu(&Menu {
        title: NAME.to_owned(),
        body: if lines.is_empty() {
            "You have no recorded dragons.".to_owned()
        } else {
            lines.join("\n\n")
        },
        buttons,
    });
    UseResult::Success
}

/// Stock-client text equivalent of the released physiology and ecology panels.
fn detail(world: World<'_>, viewer: Entity<'_>, owner: [u8; 16], dragon: [u8; 16], ecology: bool) {
    let Some(player) = viewer.as_player() else {
        return;
    };
    let Some(mut record) = records(world).and_then(|records| {
        records
            .into_iter()
            .find(|record| record.owner == owner && record.dragon == dragon)
    }) else {
        return;
    };
    if !record.bound
        && let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get())
        && let Some(id) = mobs.by_uuid(world, &dragon)
        && let Some(Some(current)) =
            entities.with_entity(world, id, |mob| snapshot(world, mob, None))
        && current.owner == owner
    {
        store_record(world, current.clone(), false);
        record = current;
    }
    let nonce = session(owner, vec![dragon]);
    let [x, y, z] = record.position;
    let body = if ecology {
        ecology_text(&record.species).to_owned()
    } else {
        let gender = match record.female {
            Some(true) => "Female",
            Some(false) => "Male",
            None => "Unknown",
        };
        let brushing = record
            .brushing
            .map_or_else(String::new, |progress| format!("\nBrushing: {progress}%"));
        format!(
            "{} ({})\nHealth {:.0}/{:.0}\nHunger {}/100\nHappiness {}/100\nGender: {gender}\nVariant: {}\n{} ({x}, {y}, {z})\nBiome: {}{brushing}",
            record.name,
            record.species,
            record.health[0],
            record.health[1],
            record.hunger,
            record.happiness,
            record.coating.replace('_', " "),
            record.dimension,
            record.biome.as_deref().unwrap_or("Unknown")
        )
    };
    player.show_menu(&Menu {
        title: format!("{NAME}: {}", if ecology { "Ecology" } else { "Physiology" }),
        body,
        buttons: vec![
            button(
                if ecology { "Physiology" } else { "Ecology" }.into(),
                nonce,
                if ecology { "select" } else { "ecology" },
                0,
            ),
            button("Remove entry".into(), nonce, "confirm", 0),
            button("Back".into(), nonce, "back", 0),
        ],
    });
}

fn ecology_text(species: &str) -> &'static str {
    match species {
        "Atroxiia" => include_str!("codex_ecology/atroxiia.txt"),
        "Cindervane" => include_str!("codex_ecology/cindervane.txt"),
        "Ignivorus" => include_str!("codex_ecology/ignivorus.txt"),
        "Nulljaw" => include_str!("codex_ecology/nulljaw.txt"),
        "Raevyx" => include_str!("codex_ecology/raevyx.txt"),
        "Stegonaut" => include_str!("codex_ecology/stegonaut.txt"),
        "Varasuchus" => include_str!("codex_ecology/varasuchus.txt"),
        "Volitans" => include_str!("codex_ecology/volitans.txt"),
        _ => "",
    }
}

pub fn click(player: &Player<'_>, action: &str) -> bool {
    let Some(action) = action.strip_prefix(ACTION) else {
        return false;
    };
    let mut parts = action.split('/');
    let (Some(nonce), Some(operation), Some(index)) = (parts.next(), parts.next(), parts.next())
    else {
        return true;
    };
    let (Ok(nonce), Ok(index)) = (nonce.parse::<u64>(), index.parse::<usize>()) else {
        return true;
    };
    if parts.next().is_some() {
        return true;
    }
    player.with_entity_context(|world, viewer| {
        let Some(owner) = MOBS.get().and_then(|mobs| mobs.uuid(viewer)) else {
            return;
        };
        let dragon = {
            let mut sessions = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(session) = sessions.get(&owner) else {
                return;
            };
            if session.nonce != nonce || session.expires <= moddedmc_api::tick_count() {
                return;
            }
            let Some(dragon) = session.entries.get(index).copied() else {
                return;
            };
            sessions.remove(&owner);
            dragon
        };
        match operation {
            "select" | "ecology" => detail(world, viewer, owner, dragon, operation == "ecology"),
            "confirm" => {
                let nonce = session(owner, vec![dragon]);
                player.show_menu(&Menu {
                    title: NAME.into(),
                    body: "Remove this dragon from your Codex?".into(),
                    buttons: vec![
                        button("Remove".into(), nonce, "remove", 0),
                        button("Cancel".into(), nonce, "select", 0),
                    ],
                });
            }
            "remove" => {
                remove_record(world, owner, dragon);
                show(world, viewer, None);
            }
            "back" => {
                show(world, viewer, None);
            }
            _ => {}
        }
    });
    true
}

pub fn dialogue_flags(world: World<'_>, owner: [u8; 16]) -> Vec<String> {
    let mut flags = Vec::new();
    for record in records(world)
        .unwrap_or_default()
        .into_iter()
        .filter(|record| record.owner == owner)
    {
        let kind = record.species.to_ascii_lowercase();
        flags.push(format!("owns_dragon:{kind}"));
        let age = if record.young { "baby" } else { "adult" };
        flags.push(format!("owns_{age}_dragon:{kind}"));
        if record.bound {
            flags.push(format!("has_dragon_binder:{kind}"));
        }
        if kind == "cindervane" {
            flags.push("has_tamed_cindervane".into());
        }
        if matches!(
            kind.as_str(),
            "raevyx" | "varasuchus" | "ignivorus" | "volitans"
        ) {
            flags.push("has_advanced_dragon".into());
        }
        if matches!(kind.as_str(), "stegonaut" | "nulljaw") {
            flags.push("has_basic_extra_dragon".into());
        }
    }
    flags
}

pub fn use_item(click: &mut UseItem<'_>) -> UseResult {
    click
        .player
        .map_or(UseResult::Fail, |player| show(click.world, player, None))
}

pub fn use_on_mob(click: &UseMob<'_>) -> UseResult {
    let result = click.player.map_or(UseResult::Fail, |player| {
        show(click.world, player, click.mob)
    });
    if result == UseResult::Success {
        click.cooldown(ID, 2);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_codex_records_default_to_full_hunger_and_new_records_keep_hunger() {
        let legacy = serde_json::json!({
            "owner": ([0; 16]),
            "dragon": ([1; 16]),
            "name": "Nulljaw",
            "species": "Nulljaw",
            "health": [70.0, 70.0],
            "happiness": 80,
            "coating": "default",
            "young": true,
            "dimension": "minecraft:the_end",
            "position": [0, 64, 0],
            "bound": true
        });
        let mut record: Record = serde_json::from_value(legacy).unwrap();
        assert_eq!(record.hunger, crate::hunger::MAX);
        record.hunger = 37;
        let bytes = serde_json::to_vec(&record).unwrap();
        let restored: Record = serde_json::from_slice(&bytes).unwrap();
        assert!(record == restored);
    }

    #[test]
    fn removing_a_record_preserves_other_owners_and_dragons() {
        let record = Record {
            owner: [0; 16],
            dragon: [1; 16],
            name: "Ember".into(),
            species: "Cindervane".into(),
            health: [80.0, 80.0],
            hunger: 100,
            happiness: 100,
            coating: "default".into(),
            young: false,
            female: Some(true),
            biome: None,
            brushing: None,
            dimension: "minecraft:overworld".into(),
            position: [0; 3],
            bound: true,
        };
        let mut entries = vec![
            record.clone(),
            Record {
                owner: [2; 16],
                ..record.clone()
            },
            Record {
                dragon: [3; 16],
                ..record
            },
        ];
        retain_other_records(&mut entries, [0; 16], [1; 16]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].owner, [2; 16]);
        assert_eq!(entries[1].dragon, [3; 16]);
        assert!(ecology_text("Cindervane").contains("carriers"));
    }
}
