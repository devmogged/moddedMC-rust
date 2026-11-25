//! Ivy's released dialogue tree, recruited ownership, and plushie recall.
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use fastnbt::{IntArray, Value};
use moddedmc_api::{
    InterfaceUnavailable, Menu, MenuButton, Player, RegistrationError, Registry,
    content::World,
    effects::SOUND_NEUTRAL,
    entities::{ENTITY_ALIVE, Entity, EntityChange, SET_MOTION},
    interaction::{UseItem, UseMob, UseResult},
    items::{Item, ItemDefaults},
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOBS, RESOURCES, STACKS, codex, flight, taming};

pub const KIND: &str = "saintsdragons:ivy_oleander";
pub const SPECIES: &str = "ivy_oleander";
pub const EGG: &str = "ivy_the_merchant";
pub const NAME: &str = "Ivy the Merchant";
pub const PLUSHIE: &str = "saintsdragons:ivy_octopus_plushie";
pub const PLUSHIE_NAME: &str = "Ivy's Octopus Plushie";

const ACTION: &str = "saintsdragons:ivy/";
const RADIUS: f64 = 8.0;

type Compound = HashMap<String, Value>;

#[derive(Clone, Default, Deserialize, PartialEq, Serialize)]
#[serde(default)]
struct Memory {
    flags: HashSet<String>,
    impression: String,
    known: bool,
    recruited: Option<[u8; 16]>,
}

#[derive(Deserialize, Serialize)]
struct PlayerMemory {
    player: [u8; 16],
    memory: Memory,
}

static SAVES: OnceLock<SavedData> = OnceLock::new();
static MEMORY_KEY: OnceLock<Key> = OnceLock::new();
static MEMORY_WRITE: Mutex<()> = Mutex::new(());

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = SAVES.set(saved_data::require()?);
    Ok(())
}

#[derive(Deserialize)]
struct Dialogue {
    start_at: String,
    states: HashMap<String, Node>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Node {
    text: Option<NodeText>,
    texts: Vec<Text>,
    choices: Vec<Choice>,
    #[serde(rename = "type")]
    kind: String,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum NodeText {
    One(Text),
    Many(Vec<Text>),
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Text {
    translate: String,
    text: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Choice {
    text: Text,
    next: String,
    requires_flag: String,
    requires_all_flags: Vec<String>,
    hidden_if_flag: String,
    hidden_if_all_flags: Vec<String>,
    set_flag: String,
    impression: String,
}

struct Action {
    next: String,
    flag: String,
    impression: String,
}

struct Session {
    ivy: [u8; 16],
    path: String,
    dimension: String,
    nonce: u64,
    expires: u64,
    actions: Vec<Action>,
    /// An `open_trade` node: a choice opens Ivy's trades.
    trade: bool,
}

static SESSIONS: LazyLock<Mutex<HashMap<[u8; 16], Session>>> = LazyLock::new(Mutex::default);
static NONCE: AtomicU64 = AtomicU64::new(1);

pub fn register(items: ItemDefaults, reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saves = SAVES.get().ok_or(RegistrationError::InvalidDefinition)?;
    let _ = MEMORY_KEY.set(saves.register(reg, "ivy_memory")?);
    let components = Value::Compound(HashMap::from([
        ("minecraft:max_stack_size".into(), Value::Int(1)),
        ("minecraft:rarity".into(), Value::String("rare".into())),
    ]));
    let defaults = fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String("minecraft:paper".into())),
        ("components", components),
    ]))
    .map_err(|_| RegistrationError::InvalidDefinition)?;
    items.add_item(
        reg,
        &Item {
            id: PLUSHIE,
            display_name: PLUSHIE_NAME,
            carrier: "minecraft:paper",
            model: if cfg!(feature = "original-assets") {
                "saintsdragons:item/ivy_octopus_plushie"
            } else {
                "minecraft:item/nautilus_shell"
            },
        },
        &defaults,
    )
}

fn memories(world: World<'_>) -> Option<Vec<PlayerMemory>> {
    let (saves, key) = (SAVES.get()?, *MEMORY_KEY.get()?);
    match saves.read(&world, key) {
        Some(bytes) => serde_json::from_slice(&bytes).ok(),
        None => Some(Vec::new()),
    }
}

fn memory(world: World<'_>, player: Entity<'_>) -> Memory {
    MOBS.get()
        .and_then(|mobs| mobs.uuid(player))
        .and_then(|id| {
            memories(world)?
                .into_iter()
                .find(|entry| entry.player == id)
        })
        .map_or_else(Memory::default, |entry| entry.memory)
}

fn save_memory(world: World<'_>, player: Entity<'_>, memory: &Memory) {
    let _guard = MEMORY_WRITE.lock().unwrap_or_else(PoisonError::into_inner);
    let (Some(saves), Some(&key), Some(id), Some(mut entries)) = (
        SAVES.get(),
        MEMORY_KEY.get(),
        MOBS.get().and_then(|mobs| mobs.uuid(player)),
        memories(world),
    ) else {
        return;
    };
    if let Some(entry) = entries.iter_mut().find(|entry| entry.player == id) {
        if entry.memory == *memory {
            return;
        }
        entry.memory = memory.clone();
    } else {
        entries.push(PlayerMemory {
            player: id,
            memory: memory.clone(),
        });
    }
    if let Ok(bytes) = serde_json::to_vec(&entries) {
        saves.write(&world, key, &bytes);
    }
}

fn language() -> &'static HashMap<String, String> {
    static ENGLISH: OnceLock<HashMap<String, String>> = OnceLock::new();
    ENGLISH.get_or_init(|| {
        RESOURCES
            .get()
            .and_then(|res| res.read("saintsdragons:dialogues/ivy_en.json"))
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    })
}

fn dialogue(path: &str) -> Option<Dialogue> {
    let bytes = RESOURCES
        .get()?
        .read(&format!("saintsdragons:dialogues/ivy/{path}.json"))?;
    serde_json::from_slice(&bytes).ok()
}

fn text(content: &Text, lang: &HashMap<String, String>, name: &str) -> String {
    lang.get(&content.translate)
        .map_or(content.text.as_str(), String::as_str)
        .replace("{name}", name)
        .replace("%1$s", name)
        .replace("%s", name)
}

fn speak(player: Entity<'_>, key: &str, fallback: &str) {
    let lang = language();
    if let Some(mobs) = MOBS.get() {
        mobs.message(player, lang.get(key).map_or(fallback, String::as_str), true);
    }
}

fn flags(
    world: World<'_>,
    viewer: Entity<'_>,
    ivy: Entity<'_>,
    memory: &Memory,
) -> HashSet<String> {
    let mut flags = memory.flags.clone();
    if let Some(id) = world.dimension_id() {
        flags.insert(format!("dimension:{id}"));
    }
    if taming::is_owner(ivy, viewer) {
        flags.insert("ivy_recruited".to_owned());
        flags.insert(format!("ivy_command_{}", taming::order_name(ivy)));
    }
    if let Some(owner) = MOBS.get().and_then(|mobs| mobs.uuid(viewer)) {
        flags.extend(codex::dialogue_flags(world, owner));
    }
    flags
}

fn eligible(choice: &Choice, flags: &HashSet<String>) -> bool {
    (choice.requires_flag.is_empty() || flags.contains(&choice.requires_flag))
        && choice
            .requires_all_flags
            .iter()
            .all(|flag| flags.contains(flag))
        && !flags.contains(&choice.hidden_if_flag)
        && (choice.hidden_if_all_flags.is_empty()
            || !choice
                .hidden_if_all_flags
                .iter()
                .all(|flag| flags.contains(flag)))
}

fn uuid_value(id: [u8; 16]) -> Value {
    Value::IntArray(IntArray::new(
        id.as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(i32::from_be_bytes)
            .collect(),
    ))
}

fn parse_uuid(value: &Value) -> Option<[u8; 16]> {
    let Value::IntArray(words) = value else {
        return None;
    };
    if words.len() != 4 {
        return None;
    }
    let bytes: Vec<_> = words.iter().flat_map(|word| word.to_be_bytes()).collect();
    bytes.try_into().ok()
}

fn gift(world: World<'_>, viewer: Entity<'_>, ivy: Entity<'_>) -> bool {
    let (Some(mobs), Some(stacks), Some(view)) = (MOBS.get(), STACKS.get(), viewer.view()) else {
        return false;
    };
    let (Some(owner), Some(ivy)) = (mobs.uuid(viewer), mobs.uuid(ivy)) else {
        return false;
    };
    let stack = HashMap::from([
        ("id", Value::String(PLUSHIE.into())),
        ("count", Value::Int(1)),
        (
            "components",
            Value::Compound(HashMap::from([(
                "minecraft:custom_data".into(),
                Value::Compound(HashMap::from([
                    ("BoundIvyUUID".into(), uuid_value(ivy)),
                    ("BoundOwnerUUID".into(), uuid_value(owner)),
                ])),
            )])),
        ),
    ]);
    fastnbt::to_bytes(&stack)
        .ok()
        .and_then(|data| stacks.spawn(world, view.position, [0.0, 0.08, 0.0], &data))
        .is_some()
}

fn effects(
    world: World<'_>,
    viewer: Entity<'_>,
    ivy: Entity<'_>,
    kind: &str,
    flags: &HashSet<String>,
    memory: &mut Memory,
) {
    match kind {
        "use_player_name" | "name_input" => memory.known = true,
        "recruit_ivy" if flags.contains("has_tamed_cindervane") && !taming::is_tamed(ivy) => {
            if let Some((owner, id)) = MOBS
                .get()
                .and_then(|mobs| Some((mobs.uuid(viewer)?, mobs.uuid(ivy)?)))
            {
                taming::inherit_owner(ivy, owner);
                memory.recruited = Some(id);
            }
        }
        "give_ivy_plushie"
            if taming::is_owner(ivy, viewer)
                && !memory.flags.contains("ivy_plushie_received")
                && gift(world, viewer, ivy) =>
        {
            memory.flags.insert("ivy_plushie_received".into());
        }
        _ => {}
    }
}

fn show(
    world: World<'_>,
    viewer: Entity<'_>,
    ivy: Entity<'_>,
    path: &str,
    state: Option<&str>,
    mut memory: Memory,
) -> Option<()> {
    let (player, mobs) = (viewer.as_player()?, MOBS.get()?);
    let owner = mobs.uuid(viewer)?;
    let dragon = mobs.uuid(ivy)?;
    let tree = dialogue(path)?;
    let node = tree.states.get(state.unwrap_or(&tree.start_at))?;
    let flags = flags(world, viewer, ivy, &memory);
    effects(world, viewer, ivy, &node.kind, &flags, &mut memory);
    save_memory(world, viewer, &memory);
    let lang = language();
    let name = player.name();
    let nonce = NONCE.fetch_add(1, Ordering::Relaxed);
    let mut actions = Vec::new();
    let mut buttons = Vec::new();
    for choice in node
        .choices
        .iter()
        .filter(|choice| eligible(choice, &flags))
    {
        buttons.push(MenuButton {
            label: text(&choice.text, lang, name),
            action: format!("{ACTION}{nonce}/{}", actions.len()),
            tooltip: String::new(),
        });
        actions.push(Action {
            next: choice.next.clone(),
            flag: choice.set_flag.clone(),
            impression: choice.impression.clone(),
        });
    }
    if buttons.is_empty() {
        buttons.push(MenuButton {
            label: "Done".into(),
            action: format!("{ACTION}done"),
            tooltip: String::new(),
        });
    }
    let body = match &node.text {
        Some(NodeText::One(line)) => text(line, lang, name),
        Some(NodeText::Many(lines)) => lines
            .iter()
            .map(|line| text(line, lang, name))
            .collect::<Vec<_>>()
            .join("\n"),
        None if !node.texts.is_empty() => text(
            &node.texts[crate::ai::random(node.texts.len() as u64) as usize],
            lang,
            name,
        ),
        None => String::new(),
    };
    let tick = moddedmc_api::tick_count();
    let mut sessions = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
    sessions.retain(|_, session| session.expires > tick);
    sessions.insert(
        owner,
        Session {
            ivy: dragon,
            path: path.into(),
            nonce,
            actions,
            dimension: world.dimension_id()?.to_owned(),
            expires: tick + 1200,
            trade: node.kind == "open_trade",
        },
    );
    drop(sessions);
    player.show_menu(&Menu {
        title: NAME.into(),
        body,
        buttons,
    });
    Some(())
}

pub fn use_on_mob(click: &UseMob<'_>) -> UseResult {
    let (Some(viewer), Some(ivy)) = (click.player, click.mob) else {
        return UseResult::Pass;
    };
    if click.sneaking && taming::is_owner(ivy, viewer) {
        if click.main_hand {
            taming::cycle_ivy_order(ivy, viewer);
        }
        return UseResult::Consume;
    }
    let memory = memory(click.world, viewer);
    let path = if !memory.known && !taming::is_tamed(ivy) {
        "first_meeting"
    } else if taming::is_owner(ivy, viewer) {
        "recruited_greeting"
    } else if taming::is_tamed(ivy) {
        "recruited_visitor_greeting"
    } else {
        match memory.impression.as_str() {
            "rude" => "known_greeting_rude",
            "trespasser" => "known_greeting_trespasser",
            "wares" => "known_greeting_wares",
            _ => "known_greeting",
        }
    };
    if show(click.world, viewer, ivy, path, None, memory).is_some() {
        UseResult::Consume
    } else {
        UseResult::Fail
    }
}

pub fn click(player: &Player<'_>, action: &str) -> bool {
    let Some(action) = action.strip_prefix(ACTION) else {
        return false;
    };
    let Some((nonce, index)) = action.split_once('/') else {
        return true;
    };
    let (Ok(nonce), Ok(index)) = (nonce.parse::<u64>(), index.parse::<usize>()) else {
        return true;
    };
    player.with_entity_context(|world, viewer| {
        let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
            return;
        };
        let Some(owner) = mobs.uuid(viewer) else {
            return;
        };
        let selected = {
            let mut sessions = SESSIONS.lock().unwrap_or_else(PoisonError::into_inner);
            if sessions.get(&owner).is_none_or(|session| {
                session.nonce != nonce
                    || session.expires <= moddedmc_api::tick_count()
                    || index >= session.actions.len()
                    || world.dimension_id() != Some(session.dimension.as_str())
            }) {
                return;
            }
            sessions.remove(&owner)
        };
        let Some(mut session) = selected else { return };
        let action = session.actions.swap_remove(index);
        let Some(id) = mobs.by_uuid(world, &session.ivy) else {
            return;
        };
        entities.with_entity(world, id, |ivy| {
            if mobs.kind(ivy) != Some(KIND)
                || ivy.flags() & ENTITY_ALIVE == 0
                || !viewer.view().zip(ivy.view()).is_some_and(|(a, b)| {
                    flight::length_squared(flight::offset(a.position, b.position))
                        <= RADIUS * RADIUS
                })
            {
                return;
            }
            let mut memory = memory(world, viewer);
            if !action.flag.is_empty() && action.flag != "ivy_plushie_received" {
                memory.flags.insert(action.flag);
            }
            if !action.impression.is_empty() {
                memory.impression = action.impression;
            }
            if ["work_dragons", "work_gardening", "work_expert"]
                .into_iter()
                .all(|flag| memory.flags.contains(flag))
            {
                memory.flags.insert("known_work_done".into());
            }
            if session.trade {
                // shortcut: Java resumes the dialogue at `next` when the trade screen closes.
                save_memory(world, viewer, &memory);
                open_trades(viewer, ivy);
                return;
            }
            let (path, node) = action
                .next
                .strip_prefix("saintsdragons:ivy/")
                .and_then(|reference| reference.split_once('#'))
                .unwrap_or((&session.path, &action.next));
            show(world, viewer, ivy, path, Some(node), memory);
        });
    });
    true
}

const TRADES_ROLLED: &str = "saintsdragons:ivy_trades_rolled";

/// `IvyTheDragonMerchant.getOffers`: rolled the first time, then kept by her villager carrier.
fn open_trades(viewer: Entity<'_>, ivy: Entity<'_>) {
    let Some(mobs) = MOBS.get() else {
        return;
    };
    let offers = if ivy.data(TRADES_ROLLED).is_some() {
        Vec::new()
    } else {
        crate::ivy_trades::roll().unwrap_or_default()
    };
    if mobs.open_trades(ivy, viewer, &offers)
        && !offers.is_empty()
        && let Ok(flag) = fastnbt::to_bytes(&Value::Compound(Compound::new()))
    {
        ivy.set_data(TRADES_ROLLED, &flag);
    }
}

fn custom_data(stack: &mut Compound) -> Option<&mut Compound> {
    let Value::Compound(components) = stack
        .entry("components".into())
        .or_insert_with(|| Value::Compound(Compound::new()))
    else {
        return None;
    };
    let Value::Compound(custom) = components
        .entry("minecraft:custom_data".into())
        .or_insert_with(|| Value::Compound(Compound::new()))
    else {
        return None;
    };
    Some(custom)
}

pub fn use_item(click: &mut UseItem<'_>) -> UseResult {
    let (Some(viewer), Some(mobs), Some(entities)) = (click.player, MOBS.get(), ENTITIES.get())
    else {
        return UseResult::Fail;
    };
    let Some(owner) = mobs.uuid(viewer) else {
        return UseResult::Fail;
    };
    let Some(mut stack) = click
        .stack
        .read()
        .and_then(|nbt| fastnbt::from_bytes::<Compound>(&nbt).ok())
    else {
        return UseResult::Fail;
    };
    let Some(custom) = custom_data(&mut stack) else {
        return UseResult::Fail;
    };
    if custom
        .get("BoundOwnerUUID")
        .and_then(parse_uuid)
        .is_some_and(|id| id != owner)
    {
        speak(
            viewer,
            "message.saintsdragons.ivy_plushie.not_owner",
            "This plushie belongs to someone else's Ivy.",
        );
        return UseResult::Fail;
    }
    let ivy = custom
        .get("BoundIvyUUID")
        .and_then(parse_uuid)
        .or_else(|| memory(click.world, viewer).recruited);
    let Some((id, uuid)) = ivy.and_then(|uuid| Some((mobs.by_uuid(click.world, &uuid)?, uuid)))
    else {
        speak(
            viewer,
            "message.saintsdragons.ivy_plushie.not_loaded",
            "Ivy is too far away for the plushie's magic to reach.",
        );
        return UseResult::Fail;
    };
    let result = entities.with_entity(click.world, id, |ivy| {
        if mobs.kind(ivy) != Some(KIND)
            || ivy.flags() & ENTITY_ALIVE == 0
            || !taming::is_owner(ivy, viewer)
        {
            return false;
        }
        let (Some(from), Some(to), Some(body)) = (ivy.view(), viewer.view(), ivy.body()) else {
            return false;
        };
        let Some(at) = [
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [0.0; 3],
        ]
        .into_iter()
        .flat_map(|[x, _, z]| [0.0, 0.5, 1.0, 2.0].map(move |y| [x, y, z]))
        .map(|offset| flight::add(to.position, offset))
        .find(|at| flight::clear_body(click.world, body, flight::offset(*at, from.position))) else {
            return false;
        };
        let Some(custom) = custom_data(&mut stack) else {
            return false;
        };
        custom.insert("BoundOwnerUUID".into(), uuid_value(owner));
        custom.insert("BoundIvyUUID".into(), uuid_value(uuid));
        if !fastnbt::to_bytes(&stack).is_ok_and(|nbt| click.stack.write(&nbt)) {
            return false;
        }
        ivy.stop_riding();
        ivy.set_position(at);
        ivy.reset_fall_distance();
        ivy.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0; 3],
            ..EntityChange::default()
        });
        if let Some(effects) = EFFECTS.get() {
            for pos in [from.position, at] {
                effects.play_sound(
                    click.world,
                    pos,
                    "minecraft:entity.enderman.teleport",
                    SOUND_NEUTRAL,
                    0.8,
                    1.1,
                );
                effects.particles(
                    click.world,
                    "minecraft:portal",
                    flight::add(pos, [0.0, 1.0, 0.0]),
                    [0.45, 0.75, 0.45],
                    0.08,
                    32,
                );
            }
        }
        true
    }) == Some(true);
    if result {
        click.cooldown(40);
        UseResult::Success
    } else {
        speak(
            viewer,
            "message.saintsdragons.ivy_plushie.no_safe_position",
            "The plushie's magic fizzles out.",
        );
        UseResult::Fail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_released_ivy_dialogue_decodes() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources/data/saintsdragons/dialogues/ivy");
        let mut count = 0;
        for file in path.read_dir().unwrap().map(Result::unwrap) {
            let bytes = std::fs::read(file.path()).unwrap();
            let tree: Dialogue = serde_json::from_slice(&bytes)
                .unwrap_or_else(|error| panic!("{}: {error}", file.path().display()));
            assert!(tree.states.contains_key(&tree.start_at));
            count += 1;
        }
        assert_eq!(count, 25);
    }
}
