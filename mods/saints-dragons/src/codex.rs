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

use crate::{ENTITIES, MOBS, allies, appearance::Appearance, binders, eggs, roster, taming};

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
    entries: SessionEntries,
}
enum SessionEntries {
    Dragons(Vec<[u8; 16]>),
    Allies(Vec<AllyAction>),
}
#[derive(Clone)]
enum AllyAction {
    Add(allies::Ally),
    Remove(allies::Ally),
    Page(usize),
    Back,
}
static SESSIONS: LazyLock<Mutex<HashMap<[u8; 16], Session>>> = LazyLock::new(Mutex::default);

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Record {
    owner: [u8; 16],
    dragon: [u8; 16],
    name: String,
    species: String,
    health: [f32; 2],
    #[serde(default)]
    armor: Option<f64>,
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
        armor: dragon.attribute_value("minecraft:armor"),
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
    store_session(owner, SessionEntries::Dragons(entries))
}

fn store_session(owner: [u8; 16], entries: SessionEntries) -> u64 {
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
    owned.sort_by_key(|record| record.name.to_lowercase());
    owned.truncate(128);
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
        owned
            .iter()
            .map(|record| record.dragon)
            .chain([owner])
            .collect(),
    );
    let mut buttons: Vec<_> = owned
        .iter()
        .take(24)
        .enumerate()
        .map(|(index, record)| button(record.name.clone(), nonce, "select", index))
        .collect();
    buttons.push(button("Ally".into(), nonce, "ally", owned.len()));
    buttons.push(pack_screen(world, owner, &owned, selected_id, nonce));
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

fn pack_screen(
    world: World<'_>,
    owner: [u8; 16],
    owned: &[Record],
    selected: Option<[u8; 16]>,
    nonce: u64,
) -> MenuButton {
    let action = |operation: &str, index| format!("{ACTION}{nonce}/{operation}/{index}");
    let owner_index = owned.len();
    let mut actions = HashMap::from([
        (action("refresh", owner_index), false),
        (action("add_name", owner_index), true),
        (action("remove_name", owner_index), true),
    ]);
    let id = |uuid: [u8; 16]| {
        uuid.iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let entries: Vec<_> = owned.iter().enumerate().map(|(index, record)| {
        let remove = action("remove", index);
        actions.insert(remove.clone(), false);
        let [x, y, z] = record.position;
        let gender = match record.female { Some(true) => "Female", Some(false) => "Male", None => "Unknown" };
        serde_json::json!({
            "id": id(record.dragon), "name": record.name, "species": record.species,
            "health": format!("{:.0}/{:.0}", record.health[0], record.health[1]),
            "armor": record.armor.map_or_else(|| "?".into(), |armor| format!("{armor:.0}")),
            "gender": gender, "hunger": format!("{}/100", record.hunger),
            "happiness": format!("{}/100", record.happiness),
            "variant": record.coating.replace('_', " "),
            "position": format!("Current Position: X: {x}, Y: {y}, Z: {z}"),
            "biome": format!("Current Biome: {}", record.biome.as_deref().unwrap_or("Unknown")),
            "brushing": record.brushing.map_or_else(String::new, |value| format!("{value}%")),
            "brushing_status": record.brushing.map_or("none", |value| if value >= 100 { "ready" } else { "waiting" }),
            "ecology": ecology_text(&record.species),
            "ecology_sections": ecology_sections(&record.species),
            "portrait": portrait(record),
            "remove": remove
        })
    }).collect();
    let allies = allies::list(world, owner);
    let state = serde_json::json!({
        "entries": entries, "selected": selected.map(id).unwrap_or_default(),
        "allies": allies.iter().map(|ally| &ally.name).collect::<Vec<_>>(),
        "ally_count": format!("Player Allies: {}/{}", allies.len(), allies::MAX),
        "refresh": action("refresh", owner_index),
        "add_name": action("add_name", owner_index),
        "remove_name": action("remove_name", owner_index),
    });
    MenuButton {
        label: String::new(),
        action: "moddedmc:pack_screen".into(),
        tooltip: serde_json::json!({"id": "saintsdragons:draconic_codex", "state": state, "actions": actions}).to_string(),
    }
}

/// CodexDragonRenderer.PORTRAIT_DEFINITIONS, including its juvenile offsets.
fn portrait_frame(species: &str, young: bool) -> Option<(i32, i32)> {
    let (scale, adult_y, baby_y) = match species {
        "Ignivorus" => (8, 0, -20),
        "Raevyx" => (13, -5, -20),
        "Atroxiia" => (13, 0, -30),
        "Volitans" => (13, -5, -5),
        "Varasuchus" => (15, -10, -10),
        "Cindervane" => (12, -15, 0),
        "Stegonaut" => (23, -15, -10),
        "Nulljaw" => (30, 0, -30),
        _ => return None,
    };
    Some(if young {
        (scale + 30, adult_y + baby_y)
    } else {
        (scale, adult_y)
    })
}

fn portrait(record: &Record) -> Option<serde_json::Value> {
    let (scale, offset_y) = portrait_frame(&record.species, record.young)?;
    let species = record.species.to_lowercase();
    let age = if record.young { "baby_" } else { "" };
    let texture = if record.young {
        if record
            .female
            .unwrap_or_else(|| record.coating.ends_with("_female"))
        {
            "default_female"
        } else {
            "default"
        }
    } else {
        &record.coating
    };
    Some(serde_json::json!({
        "rig": format!("saintsdragons:{age}{species}_display"),
        "texture": texture,
        "scale": scale, "offsetX": 0, "offsetY": offset_y
    }))
}

fn ecology_sections(species: &str) -> Vec<serde_json::Value> {
    let drops: &[&str] = match species {
        "Ignivorus" => &[
            "ignivorus_scale",
            "ignivorus_tooth",
            "ignivorus_heart",
            "ignivorus_egg",
            "ignivorus_wing_hide",
        ],
        "Atroxiia" => &["atroxiia_scale", "atroxiia_egg"],
        "Raevyx" => &[
            "raevyx_scale",
            "raevyx_egg",
            "raevyx_wing_hide",
            "raevyx_wingtalon",
        ],
        "Varasuchus" => &["varasuchus_scale", "varasuchus_egg"],
        "Cindervane" => &["cindervane_scale", "cindervane_egg"],
        "Stegonaut" => &["stegonaut_scale", "stegonaut_egg"],
        "Volitans" => &[
            "volitans_scale",
            "volitans_spine",
            "volitans_egg",
            "minecraft:salmon",
            "minecraft:cod",
            "minecraft:tropical_fish",
            "minecraft:pufferfish",
        ],
        _ => return Vec::new(),
    };
    let names = |ids: &[&str]| {
        ids.iter()
            .map(|id| id.rsplit(':').next().unwrap_or(id).replace('_', " "))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let foods = crate::TAGS
        .get()
        .and_then(|tags| {
            tags.values(
                "item",
                &format!("saintsdragons:foods/{}", species.to_lowercase()),
            )
        })
        .unwrap_or_default();
    let drop_ids = drops
        .iter()
        .map(|id| {
            if id.contains(':') {
                (*id).to_owned()
            } else {
                format!("saintsdragons:{id}")
            }
        })
        .collect::<Vec<_>>();
    vec![
        serde_json::json!({"label": "Favorite Food", "body": names(&foods.iter().map(String::as_str).collect::<Vec<_>>()), "items": foods}),
        serde_json::json!({"label": "Drops", "body": names(drops), "items": drop_ids}),
    ]
}

/// The released ally tab manages the owner's global list, including before taming a dragon.
/// shortcut: the dialog API has no text input; offer the online players instead.
fn show_allies(world: World<'_>, viewer: Entity<'_>, owner: [u8; 16], page: usize, feedback: &str) {
    let Some(player) = viewer.as_player() else {
        return;
    };
    let owned = allies::list(world, owner);
    let mut body = format!("Player Allies: {}/{}", owned.len(), allies::MAX);
    if !feedback.is_empty() {
        body.push_str(&format!("\n{feedback}"));
    }
    if owned.is_empty() {
        body.push_str("\nNo player allies added yet.");
    } else {
        for ally in &owned {
            body.push_str(&format!("\n  - {}", ally.name));
        }
    }
    let mut entries: Vec<_> = owned.iter().cloned().map(AllyAction::Remove).collect();
    const PAGE_SIZE: usize = 12;
    if owned.len() < allies::MAX {
        let online: Vec<_> = allies::online(world)
            .into_iter()
            .filter(|ally| ally.uuid != owner && !owned.iter().any(|a| a.uuid == ally.uuid))
            .collect();
        let page = page.min(online.len().saturating_sub(1) / PAGE_SIZE);
        entries.extend(
            online
                .iter()
                .skip(page * PAGE_SIZE)
                .take(PAGE_SIZE)
                .cloned()
                .map(AllyAction::Add),
        );
        if page > 0 {
            entries.push(AllyAction::Page(page - 1));
        }
        if (page + 1) * PAGE_SIZE < online.len() {
            entries.push(AllyAction::Page(page + 1));
        }
    }
    entries.push(AllyAction::Back);
    let labels: Vec<_> = entries
        .iter()
        .map(|action| match action {
            AllyAction::Add(ally) => format!("Add Player: {}", ally.name),
            AllyAction::Remove(ally) => format!("Remove Player: {}", ally.name),
            AllyAction::Page(next) => format!("Online players: page {}", next + 1),
            AllyAction::Back => "Back".into(),
        })
        .collect();
    let nonce = store_session(owner, SessionEntries::Allies(entries));
    player.show_menu(&Menu {
        title: format!("{NAME}: Ally"),
        body,
        buttons: labels
            .into_iter()
            .enumerate()
            .map(|(i, label)| button(label, nonce, "ally_action", i))
            .collect(),
    });
}

fn apply_ally_action(world: World<'_>, viewer: Entity<'_>, owner: [u8; 16], action: AllyAction) {
    let feedback = match action {
        AllyAction::Add(ally) => {
            if allies::add_online(world, owner, ally.uuid) {
                format!("Successfully updated player ally {}", ally.name)
            } else if !allies::online(world).iter().any(|a| a.uuid == ally.uuid) {
                format!(
                    "Who is {}? Are you sure you're playing with that person?",
                    ally.name
                )
            } else {
                "Could not update player ally.".into()
            }
        }
        AllyAction::Remove(ally) => {
            if allies::remove_saved(world, owner, ally.uuid) {
                format!("Successfully updated player ally {}", ally.name)
            } else {
                format!("{} is not a player ally", ally.name)
            }
        }
        AllyAction::Page(page) => {
            show_allies(world, viewer, owner, page, "");
            return;
        }
        AllyAction::Back => {
            show(world, viewer, None);
            return;
        }
    };
    show_allies(world, viewer, owner, 0, &feedback);
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
    let name = parts.next();
    if parts.next().is_some() || (matches!(operation, "add_name" | "remove_name") != name.is_some())
    {
        return true;
    }
    player.with_entity_context(|world, viewer| {
        let Some(owner) = MOBS.get().and_then(|mobs| mobs.uuid(viewer)) else {
            return;
        };
        let entries = {
            let mut sessions = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
            let Some(session) = sessions.get(&owner) else {
                return;
            };
            if session.nonce != nonce || session.expires <= moddedmc_api::tick_count() {
                return;
            }
            sessions
                .remove(&owner)
                .expect("session was just checked")
                .entries
        };
        let dragon = match entries {
            SessionEntries::Dragons(entries) => {
                let Some(dragon) = entries.get(index).copied() else {
                    return;
                };
                dragon
            }
            SessionEntries::Allies(entries) => {
                if operation == "ally_action"
                    && let Some(action) = entries.get(index).cloned()
                {
                    apply_ally_action(world, viewer, owner, action);
                }
                return;
            }
        };
        match operation {
            "refresh" if dragon == owner => {
                show(world, viewer, None);
            }
            "add_name" | "remove_name" if dragon == owner => {
                let name = name.unwrap_or_default();
                let candidates = if operation == "add_name" {
                    allies::online(world)
                } else {
                    allies::list(world, owner)
                };
                if let Some(ally) = candidates
                    .into_iter()
                    .find(|ally| ally.name.eq_ignore_ascii_case(name))
                {
                    let changed = if operation == "add_name" {
                        allies::add_online(world, owner, ally.uuid)
                    } else {
                        allies::remove_saved(world, owner, ally.uuid)
                    };
                    if changed {
                        player.send_message(&format!(
                            "Successfully updated player ally {}",
                            ally.name
                        ));
                    }
                }
                show(world, viewer, None);
            }
            "ally" if dragon == owner => show_allies(world, viewer, owner, 0, ""),
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
    #[test]
    fn portrait_frames_match_released_adult_and_baby_values() {
        for (species, adult, baby) in [
            ("Ignivorus", (8, 0), (38, -20)),
            ("Raevyx", (13, -5), (43, -25)),
            ("Atroxiia", (13, 0), (43, -30)),
            ("Volitans", (13, -5), (43, -10)),
            ("Varasuchus", (15, -10), (45, -20)),
            ("Cindervane", (12, -15), (42, -15)),
            ("Stegonaut", (23, -15), (53, -25)),
            ("Nulljaw", (30, 0), (60, -30)),
        ] {
            assert_eq!(super::portrait_frame(species, false), Some(adult));
            assert_eq!(super::portrait_frame(species, true), Some(baby));
        }
        assert_eq!(super::portrait_frame("Moop", false), None);
    }
    use super::*;

    #[test]
    fn volitans_ecology_preserves_vanilla_and_custom_item_ids() {
        let sections = ecology_sections("Volitans");
        assert_eq!(
            sections[1]["items"],
            serde_json::json!([
                "saintsdragons:volitans_scale",
                "saintsdragons:volitans_spine",
                "saintsdragons:volitans_egg",
                "minecraft:salmon",
                "minecraft:cod",
                "minecraft:tropical_fish",
                "minecraft:pufferfish"
            ])
        );
        assert!(ecology_sections("Nulljaw").is_empty());
    }

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
        record.female = Some(true);
        record.coating = "albino_female".into();
        let preview = portrait(&record).unwrap();
        assert_eq!(preview["rig"], "saintsdragons:baby_nulljaw_display");
        assert_eq!(preview["texture"], "default_female");
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
            armor: None,
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
