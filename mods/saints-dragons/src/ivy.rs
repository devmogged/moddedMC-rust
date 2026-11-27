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
    inventory_menu::{self, Access, Change, MenuKind, Validated, Validity, ValiditySlots},
    items::{Item, ItemDefaults},
    mob_ai::goal_phase,
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, RESOURCES, STACKS, codex, flight, taming};

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
static INVENTORY_KEY: OnceLock<Key> = OnceLock::new();
static INVENTORY_MENUS: OnceLock<Validated> = OnceLock::new();
static INVENTORY_KIND: OnceLock<MenuKind> = OnceLock::new();
static INVENTORY_WRITE: Mutex<()> = Mutex::new(());

// IvyInventoryMenu: four armor slots, one weapon, and 27 storage slots.
// A vanilla four-row screen has four padding slots.
const INVENTORY_SIZE: usize = 36;
const STORAGE_START: usize = 9;

#[derive(Deserialize, Serialize)]
struct Inventory {
    ivy: [u8; 16],
    slots: Vec<Option<Vec<u8>>>,
}

fn inventories(world: World<'_>) -> Option<Vec<Inventory>> {
    match SAVES.get()?.read(&world, *INVENTORY_KEY.get()?) {
        Some(bytes) => serde_json::from_slice(&bytes).ok(),
        None => Some(Vec::new()),
    }
}

fn inventory(world: World<'_>, uuid: [u8; 16]) -> Option<Vec<Option<Vec<u8>>>> {
    Some(
        inventories(world)?
            .into_iter()
            .find(|record| record.ivy == uuid)
            .map_or_else(|| vec![None; INVENTORY_SIZE], |record| record.slots),
    )
}

fn can_access(ivy: Entity<'_>, viewer: Entity<'_>) -> bool {
    MOBS.get().is_some_and(|mobs| mobs.kind(ivy) == Some(KIND))
        && ivy.flags() & ENTITY_ALIVE != 0
        && !downed_state(ivy).active()
        && taming::is_owner(ivy, viewer)
        && ivy.view().zip(viewer.view()).is_some_and(|(a, b)| {
            flight::length_squared(flight::offset(a.position, b.position)) < RADIUS * RADIUS
        })
}

fn open_inventory(world: World<'_>, viewer: Entity<'_>, ivy: Entity<'_>) -> bool {
    let (Some(menus), Some(&kind), Some(player), Some(uuid)) = (
        INVENTORY_MENUS.get(),
        INVENTORY_KIND.get(),
        viewer.as_player(),
        MOBS.get().and_then(|mobs| mobs.uuid(ivy)),
    ) else {
        return false;
    };
    if !can_access(ivy, viewer) {
        return false;
    }
    let mut access = [Access {
        take: true,
        insert: true,
    }; INVENTORY_SIZE];
    for slot in &mut access[5..STORAGE_START] {
        *slot = Access {
            take: false,
            insert: false,
        };
    }
    menus.open(
        &player,
        kind,
        ivy.id() as u64,
        NAME,
        ValiditySlots {
            world,
            access: &access,
            context: &uuid,
        },
        || inventory(world, uuid),
    )
}

fn valid_inventory(view: Validity<'_>) -> bool {
    let (Some(mobs), Some(entities), Ok(id)) =
        (MOBS.get(), ENTITIES.get(), i32::try_from(view.inventory))
    else {
        return false;
    };
    let mut valid = false;
    view.player.with_entity_context(|world, viewer| {
        if world.dimension_id() != view.world.dimension_id() {
            return;
        }
        valid = entities.with_entity(world, id, |ivy| {
            mobs.uuid(ivy).is_some_and(|uuid| uuid == view.context) && can_access(ivy, viewer)
        }) == Some(true);
    });
    valid
}

fn component(stack: &[u8], name: &str) -> Option<Value> {
    let bytes = STACKS.get()?.component(stack, name)?;
    fastnbt::from_bytes::<Compound>(&bytes)
        .ok()?
        .remove("value")
}

fn attribute(stack: &[u8], name: &str, slot: &str, base: f64) -> Option<f64> {
    let value = component(stack, "minecraft:attribute_modifiers")?;
    attribute_from(value, name, slot, base)
}

fn attribute_from(value: Value, name: &str, slot: &str, base: f64) -> Option<f64> {
    let Value::List(modifiers) = value else {
        return None;
    };
    let mut additions = 0.0;
    let mut multiplied_base = 0.0;
    let mut total = 1.0;
    let mut found = false;
    for modifier in modifiers {
        let Value::Compound(m) = modifier else {
            continue;
        };
        if !matches!(m.get("type"), Some(Value::String(s)) if s.trim_start_matches("minecraft:") == name)
        {
            continue;
        }
        if matches!(m.get("slot"), Some(Value::String(s)) if s != slot && s != "any" && !(s == "armor" && slot != "mainhand") && !(s == "hand" && slot == "mainhand"))
        {
            continue;
        }
        let amount = match m.get("amount") {
            Some(Value::Double(n)) => *n,
            Some(Value::Float(n)) => f64::from(*n),
            _ => continue,
        };
        match m.get("operation") {
            Some(Value::String(s)) if s == "add_value" => additions += amount,
            Some(Value::String(s)) if s == "add_multiplied_base" => multiplied_base += amount,
            Some(Value::String(s)) if s == "add_multiplied_total" => total *= 1.0 + amount,
            _ => continue,
        }
        found = true;
    }
    found.then_some((base + additions + base * multiplied_base) * total)
}

const ARMOR_SLOTS: [&str; 4] = ["head", "chest", "legs", "feet"];

fn accepts(slot: usize, stack: &[u8]) -> bool {
    if slot >= STORAGE_START {
        return true;
    }
    if slot >= 5
        || !STACKS
            .get()
            .and_then(|s| s.info(stack))
            .is_some_and(|s| s.count == 1)
    {
        return false;
    }
    if slot == 4 {
        return attribute(stack, "attack_damage", "mainhand", 1.0).is_some();
    }
    // The effective equippable component includes vanilla and modded armor.
    matches!(component(stack, "minecraft:equippable"), Some(Value::Compound(c))
        if matches!(c.get("slot"), Some(Value::String(s)) if s == ARMOR_SLOTS[slot]))
        && attribute(stack, "armor", ARMOR_SLOTS[slot], 0.0).is_some()
}

fn change_inventory(mut change: Change<'_>) {
    let (Some(mobs), Some(entities), Some(&key), Ok(id)) = (
        MOBS.get(),
        ENTITIES.get(),
        INVENTORY_KEY.get(),
        i32::try_from(change.inventory),
    ) else {
        return;
    };
    if change.slots().len() != INVENTORY_SIZE || change.overflow().len() != 0 {
        return;
    }
    let Some(uuid) = entities
        .with_entity(change.world, id, |ivy| {
            if mobs.kind(ivy) != Some(KIND)
                || ivy.flags() & ENTITY_ALIVE == 0
                || taming::owner(ivy) != Some(change.player_uuid)
            {
                return None;
            }
            mobs.uuid(ivy)
        })
        .flatten()
    else {
        return;
    };
    let slots: Vec<_> = change
        .slots()
        .map(|slot| slot.map(<[u8]>::to_vec))
        .collect();
    if !slots
        .iter()
        .enumerate()
        .all(|(i, stack)| stack.as_deref().is_none_or(|s| accepts(i, s)))
    {
        return;
    }
    let _lock = INVENTORY_WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let Some(mut records) = inventories(change.world) else {
        return;
    };
    records.retain(|record| record.ivy != uuid);
    records.push(Inventory { ivy: uuid, slots });
    // shortcut: transactions can only commit saved_data, not entity NBT. Keep the
    // inventory keyed by Ivy's persistent UUID until the host offers entity commits.
    if let Ok(bytes) = serde_json::to_vec(&records)
        && change.commit(key, &bytes)
        && let Some(state) = FIGHTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&id)
    {
        state.armor_synced = false;
    }
}

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = SAVES.set(saved_data::require()?);
    let _ = INVENTORY_MENUS.set(inventory_menu::require_validity()?);
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
    inventory: bool,
}

static SESSIONS: LazyLock<Mutex<HashMap<[u8; 16], Session>>> = LazyLock::new(Mutex::default);
static NONCE: AtomicU64 = AtomicU64::new(1);

struct TradeResume {
    ivy: [u8; 16],
    dimension: String,
    path: String,
    node: String,
    due: Option<u64>,
}

impl TradeResume {
    fn close(&mut self, merchant: [u8; 16], dimension: Option<&str>, tick: u64) {
        if self.ivy == merchant && dimension == Some(self.dimension.as_str()) && self.due.is_none()
        {
            self.due = Some(tick + 3);
        }
    }
}

static TRADE_RESUMES: LazyLock<Mutex<HashMap<[u8; 16], TradeResume>>> =
    LazyLock::new(Mutex::default);

/// IvyTheDragonMerchant.scheduleDialogueResumeAfterTrade.
pub fn merchant_close(world: World<'_>, viewer: Entity<'_>, ivy: Entity<'_>) {
    let Some((owner, merchant)) = MOBS
        .get()
        .and_then(|mobs| Some((mobs.uuid(viewer)?, mobs.uuid(ivy)?)))
    else {
        return;
    };
    let mut pending = TRADE_RESUMES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(resume) = pending.get_mut(&owner) {
        resume.close(merchant, world.dimension_id(), moddedmc_api::tick_count());
    }
}

/// IvyTheDragonMerchant.tickDialogueTradeResume: wait for the trade close to reach the client.
pub fn trade_resume_tick(world: World<'_>) {
    let (Some(mobs), Some(entities)) = (MOBS.get(), ENTITIES.get()) else {
        return;
    };
    let tick = moddedmc_api::tick_count();
    let ready = {
        let mut pending = TRADE_RESUMES.lock().unwrap_or_else(PoisonError::into_inner);
        let owners: Vec<_> = pending
            .iter()
            .filter(|(_, resume)| {
                world.dimension_id() == Some(resume.dimension.as_str())
                    && resume.due.is_some_and(|due| due <= tick)
            })
            .map(|(owner, _)| *owner)
            .collect();
        owners
            .into_iter()
            .filter_map(|owner| pending.remove(&owner).map(|resume| (owner, resume)))
            .collect::<Vec<_>>()
    };
    for (owner, resume) in ready {
        let (Some(viewer), Some(ivy)) = (
            mobs.by_uuid(world, &owner),
            mobs.by_uuid(world, &resume.ivy),
        ) else {
            continue;
        };
        entities.with_entity(world, viewer, |viewer| {
            entities.with_entity(world, ivy, |ivy| {
                if mobs.kind(ivy) == Some(KIND)
                    && ivy.flags() & ENTITY_ALIVE != 0
                    && !downed_state(ivy).active()
                    && viewer.view().zip(ivy.view()).is_some_and(|(a, b)| {
                        flight::length_squared(flight::offset(a.position, b.position))
                            <= RADIUS * RADIUS
                    })
                {
                    show(
                        world,
                        viewer,
                        ivy,
                        &resume.path,
                        Some(&resume.node),
                        memory(world, viewer),
                    );
                }
            });
        });
    }
}

pub fn register(items: ItemDefaults, reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saves = SAVES.get().ok_or(RegistrationError::InvalidDefinition)?;
    let _ = MEMORY_KEY.set(saves.register(reg, "ivy_memory")?);
    let _ = INVENTORY_KEY.set(saves.register(reg, "ivy_inventory")?);
    let menus = INVENTORY_MENUS
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?;
    let _ = INVENTORY_KIND.set(menus.register(reg, change_inventory, valid_inventory)?);
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
    TRADE_RESUMES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&owner);
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
            inventory: node.kind == "open_inventory",
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
    let mut downed = downed_state(ivy);
    if downed.active() {
        if downed.downed
            && taming::is_owner(ivy, viewer)
            && viewer.view().zip(ivy.view()).is_some_and(|(a, b)| {
                flight::length_squared(flight::offset(a.position, b.position)) <= 9.0
            })
        {
            downed.interact();
            if let Some(mobs) = MOBS.get() {
                mobs.message(
                    viewer,
                    &format!("Reviving Ivy... {}/60", downed.downed_revive_progress),
                    true,
                );
            }
            if downed.downed_revive_progress >= 60 {
                revive(ivy, &mut downed);
            }
            save_downed(ivy, &downed);
        }
        return UseResult::Consume;
    }
    if click.sneaking && taming::is_owner(ivy, viewer) {
        if click.main_hand {
            taming::cycle_ivy_order(ivy, viewer);
        }
        return UseResult::Consume;
    }
    crate::advancements::award(viewer, "meet_ivy", "meet_ivy");
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
                || downed_state(ivy).active()
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
            let (path, node) = action
                .next
                .strip_prefix("saintsdragons:ivy/")
                .and_then(|reference| reference.split_once('#'))
                .unwrap_or((&session.path, &action.next));
            if session.trade {
                save_memory(world, viewer, &memory);
                if open_trades(viewer, ivy) {
                    TRADE_RESUMES
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .insert(
                            owner,
                            TradeResume {
                                ivy: session.ivy,
                                dimension: session.dimension.clone(),
                                path: path.into(),
                                node: node.into(),
                                due: None,
                            },
                        );
                }
                return;
            }
            if session.inventory {
                save_memory(world, viewer, &memory);
                open_inventory(world, viewer, ivy);
                return;
            }
            show(world, viewer, ivy, path, Some(node), memory);
        });
    });
    true
}

const TRADES_ROLLED: &str = "saintsdragons:ivy_trades_rolled";

pub const COMBAT: u64 = 18;

const DOWNED: &str = "saintsdragons:ivy_downed";

/// IvyTheDragonMerchant's saved downed state and revival counters.
#[derive(Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
struct DownedState {
    downed: bool,
    downed_bleed_ticks: i32,
    downed_finish_hits: i32,
    downed_revive_progress: i32,
    grace: i32,
    arise: i32,
}

impl DownedState {
    fn active(&self) -> bool {
        self.downed || self.arise > 0
    }

    fn interact(&mut self) {
        self.grace = 8;
        self.downed_revive_progress = (self.downed_revive_progress + 5).min(60);
    }

    fn finish_hit(&mut self) {
        self.downed_finish_hits += 1;
        self.downed_bleed_ticks = (self.downed_bleed_ticks - 100).max(0);
    }

    fn tick(&mut self) -> bool {
        if self.downed {
            if self.grace > 0 {
                self.grace -= 1;
            } else if self.downed_revive_progress > 0 {
                self.downed_revive_progress -= 1;
            }
            if self.downed_bleed_ticks <= 0 || self.downed_finish_hits >= 3 {
                return true;
            }
            self.downed_bleed_ticks -= 1;
        } else if self.arise > 0 {
            self.arise -= 1;
        }
        false
    }
}

fn downed_state(ivy: Entity<'_>) -> DownedState {
    ivy.data(DOWNED)
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok())
        .unwrap_or_default()
}

fn save_downed(ivy: Entity<'_>, state: &DownedState) {
    if let Ok(bytes) = fastnbt::to_bytes(state) {
        ivy.set_data(DOWNED, &bytes);
    }
}

fn revive(ivy: Entity<'_>, state: &mut DownedState) {
    if let (Some(vitals), Some((_, max))) = (
        crate::MOB_HEALTH.get(),
        MOBS.get().and_then(|mobs| mobs.health(ivy)),
    ) && vitals.set(ivy, (max * 0.35).max(6.0).min(max))
    {
        *state = DownedState {
            arise: 30,
            ..DownedState::default()
        };
    }
}

/// IvyTheDragonMerchant.hurtDowned. The existing callback has only the type, not the attacker.
pub fn hurt(ivy: Entity<'_>, damage_type: &str, amount: f32) -> bool {
    let mut state = downed_state(ivy);
    if !state.downed
        || matches!(
            damage_type,
            "minecraft:out_of_world" | "minecraft:generic_kill"
        )
    {
        // shortcut: actuallyHurt needs a post-mitigation, pre-death callback to
        // start Downed at exactly 1 HP (and allow inventory totems first).
        return true;
    }
    if amount > 0.0
        && !matches!(
            damage_type,
            "minecraft:mob_attack"
                | "minecraft:mob_attack_no_aggro"
                | "minecraft:arrow"
                | "minecraft:trident"
                | "minecraft:mob_projectile"
        )
    {
        // shortcut: projectile owner/direct attacker is unavailable; don't count
        // ambiguous projectiles as finishing hits until the host exposes the source.
        state.finish_hit();
        save_downed(ivy, &state);
    }
    false
}

/// IvyTheDragonMerchant.tickDownedState, including the stock-client animation and revive timer.
pub fn downed_tick(world: World<'_>, ivy: Entity<'_>) {
    let mut state = downed_state(ivy);
    if !state.active() {
        return;
    }
    if let Some(ai) = MOB_AI.get() {
        ai.stop(ivy);
        ai.set_target(world, ivy, None);
    }
    if let Some(view) = ivy.view() {
        ivy.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0, view.motion[1].min(0.0), 0.0],
            ..EntityChange::default()
        });
    }
    ivy.reset_fall_distance();
    if state.downed {
        if let Some(fight) = FIGHTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&ivy.id())
        {
            fight.move_ = None;
            fight.stance = false;
            fight.target = None;
        }
        if MOBS
            .get()
            .and_then(|mobs| mobs.health(ivy))
            .is_some_and(|(health, _)| health > 1.0)
        {
            revive(ivy, &mut state);
            save_downed(ivy, &state);
            return;
        }
        if moddedmc_api::tick_count().is_multiple_of(10)
            && let (Some(ai), Some(entities), Some(body)) =
                (MOB_AI.get(), ENTITIES.get(), ivy.body())
        {
            for id in entities.entities_in(
                world,
                body.min.map(|v| v - 32.0),
                body.max.map(|v| v + 32.0),
                ivy.id(),
            ) {
                entities.with_entity(world, id, |other| {
                    if ai.target(other) == Some(ivy.id()) {
                        ai.set_target(world, other, None);
                        ai.stop(other);
                    }
                });
            }
        }
        // shortcut: clearing last-hurt/brain memories and observing heal events
        // directly needs host calls; clear active targets and observe health changes.
    }
    let arising = state.arise == 1;
    if state.tick() {
        save_downed(ivy, &DownedState::default());
        ivy.hurt("minecraft:generic_kill", f32::MAX);
    } else {
        save_downed(ivy, &state);
        if arising
            && combat_allowed(world, ivy)
            && let (Some(ai), Some(mobs), Some(entities), Some(owner)) =
                (MOB_AI.get(), MOBS.get(), ENTITIES.get(), taming::owner(ivy))
            && let Some(id) = mobs.by_uuid(world, &owner)
            && let Some((attacker, _)) = entities
                .with_entity(world, id, |owner| ai.last_hurt_by(owner))
                .flatten()
            && target_position(world, ivy, attacker).is_some()
        {
            ai.set_target(world, ivy, Some(attacker));
        }
    }
}

pub fn downed_clip(ivy: Entity<'_>) -> Option<&'static str> {
    let state = downed_state(ivy);
    if state.downed {
        Some("about_to_die")
    } else if state.arise > 0 {
        Some("arise")
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct Hit {
    tick: u64,
    damage: f32,
    range: f64,
    knockback: f64,
    lift: f64,
    sword: bool,
    reset: bool,
}

#[derive(Clone, Copy)]
struct Move {
    clip: &'static str,
    duration: u64,
    hits: &'static [Hit],
    cooldown: u64,
}

const fn hit(
    tick: u64,
    damage: f32,
    range: f64,
    knockback: f64,
    lift: f64,
    sword: bool,
    reset: bool,
) -> Hit {
    Hit {
        tick,
        damage,
        range,
        knockback,
        lift,
        sword,
        reset,
    }
}

// IvyCombatBrain.AttackType: the released melee animation/impact schedule.
const BOXING: [Move; 5] = [
    Move {
        clip: "orthodox_left_jab",
        duration: 10,
        hits: &[hit(4, 2.5, 2.35, 0.015, 0.0, false, false)],
        cooldown: 5,
    },
    Move {
        clip: "orthodox_right_hook",
        duration: 13,
        hits: &[hit(5, 6.0, 2.45, 0.28, 0.08, false, false)],
        cooldown: 15,
    },
    Move {
        clip: "orthodox_left_jab_right_cross",
        duration: 20,
        hits: &[
            hit(5, 2.0, 2.55, 0.02, 0.0, false, true),
            hit(13, 4.0, 2.95, 0.16, 0.04, false, true),
        ],
        cooldown: 5,
    },
    Move {
        clip: "orthodox_jab_jab_hook",
        duration: 25,
        hits: &[
            hit(5, 1.8, 2.45, 0.0, 0.0, false, true),
            hit(11, 1.8, 2.45, 0.0, 0.0, false, true),
            hit(19, 5.5, 3.05, 0.24, 0.06, false, true),
        ],
        cooldown: 5,
    },
    Move {
        clip: "orthodox_right_hook_uppercut",
        duration: 25,
        hits: &[
            hit(8, 4.0, 2.55, 0.015, 0.0, false, true),
            hit(16, 6.0, 3.05, 0.12, 0.50, false, true),
        ],
        cooldown: 15,
    },
];

const SWORD: [Move; 6] = [
    Move {
        clip: "sword_quick_stab",
        duration: 8,
        hits: &[hit(4, 2.5, 2.35, 0.015, 0.0, true, false)],
        cooldown: 20,
    },
    Move {
        clip: "sword_swing",
        duration: 12,
        hits: &[hit(6, 6.0, 2.45, 0.28, 0.08, true, false)],
        cooldown: 20,
    },
    Move {
        clip: "orthodox_sword_left_jab_right_swing",
        duration: 20,
        hits: &[
            hit(5, 2.0, 2.55, 0.02, 0.0, false, true),
            hit(13, 4.0, 3.50, 0.16, 0.04, true, true),
        ],
        cooldown: 20,
    },
    Move {
        clip: "orthodox_sword_jab_jab_swing",
        duration: 25,
        hits: &[
            hit(5, 1.8, 2.45, 0.0, 0.0, false, true),
            hit(11, 1.8, 2.45, 0.0, 0.0, false, true),
            hit(19, 5.5, 3.60, 0.24, 0.06, true, true),
        ],
        cooldown: 20,
    },
    Move {
        clip: "sword_swing_slash",
        duration: 19,
        hits: &[
            hit(6, 4.0, 2.55, 0.015, 0.0, true, true),
            hit(16, 6.0, 3.60, 0.12, 0.50, true, true),
        ],
        cooldown: 20,
    },
    Move {
        clip: "sword_slash_stab_stab",
        duration: 27,
        hits: &[
            hit(6, 4.5, 3.1, 0.10, 0.0, true, true),
            hit(14, 3.5, 2.85, 0.02, 0.0, true, true),
            hit(22, 3.5, 2.85, 0.12, 0.0, true, true),
        ],
        cooldown: 30,
    },
];

// IvyCombatBrain.DASH_FORWARD_RIGHT_CROSS and its sword variant.
const DASH: [Move; 2] = [
    Move {
        clip: "orthodox_dash_forward_right_cross",
        duration: 20,
        hits: &[hit(8, 5.0, 3.2, 0.28, 0.04, false, true)],
        cooldown: 30,
    },
    Move {
        clip: "sword_dash_forward_spin_slash",
        duration: 19,
        hits: &[hit(13, 5.0, 3.2, 0.28, 0.04, true, true)],
        cooldown: 30,
    },
];

#[derive(Default)]
struct DashRead {
    previous: Option<[f64; 3]>,
    moving: u32,
}

impl DashRead {
    /// IvyCombatBrain.updateTargetStillness and readDashCross.
    fn retreating(&mut self, from: [f64; 3], at: [f64; 3]) -> bool {
        let Some(previous) = self.previous else {
            self.previous = Some(at);
            return false;
        };
        let delta = flight::offset(at, previous);
        let speed = delta[0].hypot(delta[2]);
        if speed <= 0.05 {
            self.moving = 0;
            return false;
        }
        self.previous = Some(at);
        self.moving = self.moving.saturating_add(1);
        let away = flight::offset(at, from);
        let old = flight::offset(previous, from);
        let distance_sq = flight::length_squared(away);
        let dot = (away[0] * delta[0] + away[2] * delta[2]) / (distance_sq.sqrt() * speed);
        self.moving >= 10
            && distance_sq > 2.45_f64.powi(2)
            && distance_sq <= 4.65_f64.powi(2)
            && dot >= 0.45
            && distance_sq > old[0].powi(2) + old[2].powi(2) + 0.015
    }
}

fn forward_impulse(
    from: [f64; 3],
    at: [f64; 3],
    motion: [f64; 3],
    strength: f64,
    lift: f64,
) -> [f64; 3] {
    let toward = flight::offset(at, from);
    let length = flight::length_squared(toward).sqrt();
    if length < 0.0001 {
        return motion;
    }
    [
        toward[0] / length * strength,
        motion[1] + lift,
        toward[2] / length * strength,
    ]
}

#[derive(Default)]
struct Fight {
    armor_synced: bool,
    self_hurt: Option<(i32, i32)>,
    owner_hurt: Option<(i32, i32)>,
    owner_synced: bool,
    target: Option<i32>,
    start: u64,
    ready: u64,
    hook_ready: u64,
    combo_ready: u64,
    dash_ready: u64,
    commit_ready: u64,
    dash_read: DashRead,
    move_: Option<Move>,
    sword_damage: Option<f32>,
    stance: bool,
}

static FIGHTS: LazyLock<Mutex<HashMap<i32, Fight>>> = LazyLock::new(Mutex::default);

pub fn forget(id: i32) {
    FIGHTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&id);
}

fn combat_allowed(world: World<'_>, ivy: Entity<'_>) -> bool {
    taming::is_tamed(ivy)
        && !downed_state(ivy).active()
        && taming::order_name(ivy) != "stay"
        && ivy.vehicle().is_none()
        && ivy.view().is_some_and(|view| {
            let mut at = view.position;
            at[1] += 1.0;
            !matches!(
                world.fluid_id(flight::cell(at)).as_deref(),
                Some("minecraft:water" | "minecraft:flowing_water")
            )
        })
}

fn enemy(ivy: Entity<'_>, target: Entity<'_>) -> bool {
    crate::abilities::enemy(ivy, target)
        && !matches!(
            target.type_id().trim_start_matches("minecraft:"),
            "iron_golem" | "snow_golem" | "villager"
        )
        && !(taming::owner(target).is_some() && taming::owner(ivy) == taming::owner(target))
}

fn target_position(world: World<'_>, ivy: Entity<'_>, id: i32) -> Option<[f64; 3]> {
    let from = ivy.view()?.position;
    ENTITIES.get()?.with_entity(world, id, |target| {
        let at = target.view()?.position;
        (enemy(ivy, target) && flight::length_squared(flight::offset(at, from)) <= 32.0 * 32.0)
            .then_some(at)
    })?
}

/// IvyCompanionController.OwnerDefenseGoal: react to a new attacker of Ivy or her owner.
pub fn target_tick(world: World<'_>, ivy: Entity<'_>) {
    let (Some(ai), Some(mobs), Some(entities), Some(owner)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), taming::owner(ivy))
    else {
        return;
    };
    let own_hurt = ai.last_hurt_by(ivy);
    let owner_hurt = mobs.by_uuid(world, &owner).and_then(|id| {
        entities
            .with_entity(world, id, |owner| ai.last_hurt_by(owner))
            .flatten()
    });
    let (own, defense, sync_armor) = {
        let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        let state = states.entry(ivy.id()).or_default();
        let own = own_hurt.filter(|hit| state.self_hurt != Some(*hit));
        state.self_hurt = own_hurt;
        let defense = owner_hurt.filter(|hit| state.owner_synced && state.owner_hurt != Some(*hit));
        state.owner_hurt = owner_hurt;
        state.owner_synced = true;
        let sync = !state.armor_synced;
        state.armor_synced = true;
        (own, defense, sync)
    };
    if sync_armor {
        update_armor(world, ivy);
    }
    if !combat_allowed(world, ivy) {
        if ai.target(ivy).is_some() {
            ai.set_target(world, ivy, None);
        }
        return;
    }
    if ai
        .target(ivy)
        .is_some_and(|id| target_position(world, ivy, id).is_none())
    {
        ai.set_target(world, ivy, None);
    }
    // shortcut: the API exposes last_hurt_by, but not the owner's last_hurt_mob
    // timestamp; attacking a target first cannot trigger owner defense yet.
    for (id, _) in own
        .into_iter()
        .chain(defense.filter(|_| taming::order_name(ivy) == "follow"))
    {
        if target_position(world, ivy, id).is_some() {
            ai.set_target(world, ivy, Some(id));
            break;
        }
    }
}

fn choose_move(
    sword: bool,
    distance: f64,
    still: bool,
    combo: bool,
    hook: bool,
    rng: impl Fn(u64) -> u64,
) -> usize {
    if distance < 2.15 && combo {
        return if sword {
            [2, 3, 5][rng(3) as usize]
        } else {
            2 + rng(2) as usize
        };
    }
    if combo && hook && distance <= 2.45 && rng(100) < 40 {
        return 4;
    }
    let mut weights = if sword {
        [
            if still { 25 } else { 45 },
            if still { 25 } else { 30 },
            if still { 35 } else { 22 },
            if still { 25 } else { 16 },
            0,
            if still { 28 } else { 18 },
        ]
    } else {
        [
            if still { 25 } else { 50 },
            if still { 30 } else { 35 },
            if still { 45 } else { 25 },
            if still { 30 } else { 18 },
            0,
            0,
        ]
    };
    if !hook || distance > 2.15 {
        weights[1] = 0;
    }
    if !combo || distance < 1.65 {
        weights[2..].fill(0);
    }
    let mut roll = rng(weights.iter().sum());
    for i in [5, 3, 2, 1, 0] {
        if roll < weights[i] {
            return i;
        }
        roll -= weights[i];
    }
    0
}

fn equipped_weapon(world: World<'_>, ivy: Entity<'_>) -> Option<f32> {
    let uuid = MOBS.get()?.uuid(ivy)?;
    let slots = inventory(world, uuid)?;
    let stack = slots.get(4)?.as_deref()?;
    attribute(stack, "attack_damage", "mainhand", 1.0).map(|damage| damage.max(0.0) as f32)
}

fn update_armor(world: World<'_>, ivy: Entity<'_>) {
    let (Some(health), Some(uuid)) = (
        crate::MOB_HEALTH.get(),
        MOBS.get().and_then(|m| m.uuid(ivy)),
    ) else {
        return;
    };
    let Some(slots) = inventory(world, uuid) else {
        return;
    };
    for name in ["armor", "armor_toughness"] {
        let value: f64 = ARMOR_SLOTS
            .iter()
            .enumerate()
            .filter_map(|(i, slot)| attribute(slots.get(i)?.as_deref()?, name, slot, 0.0))
            .sum();
        health.set_attribute(ivy, &format!("minecraft:{name}"), value.max(0.0));
    }
}

fn impact(
    world: World<'_>,
    ivy: Entity<'_>,
    id: i32,
    hit: Hit,
    sword_damage: Option<f32>,
    dash: bool,
) {
    let (Some(ai), Some(mobs), Some(entities), Some(body), Some(from)) = (
        MOB_AI.get(),
        MOBS.get(),
        ENTITIES.get(),
        ivy.body(),
        ivy.view(),
    ) else {
        return;
    };
    entities.with_entity(world, id, |target| {
        let (Some(other), Some(view)) = (target.body(), target.view()) else {
            return;
        };
        let range = [hit.range, 1.15, hit.range];
        if !enemy(ivy, target)
            || !(0..3).all(|i| {
                body.min[i] - range[i] <= other.max[i] && body.max[i] + range[i] >= other.min[i]
            })
        {
            return;
        }
        if dash {
            ivy.change(&EntityChange {
                mask: SET_MOTION,
                motion: forward_impulse(from.position, view.position, from.motion, 0.2, 0.02),
                ..EntityChange::default()
            });
        }
        if hit.reset {
            mobs.reset_hurt_cooldown(target);
        }
        let damage = if hit.sword {
            sword_damage.unwrap_or(4.0)
        } else {
            hit.damage
        };
        ai.attack(world, ivy, id, damage);
        let delta = flight::offset(view.position, from.position);
        if delta[0].hypot(delta[2]) > 0.01 {
            target.change(&EntityChange {
                mask: SET_MOTION,
                motion: flight::add(
                    view.motion,
                    [delta[0] * hit.knockback, hit.lift, delta[2] * hit.knockback],
                ),
                ..EntityChange::default()
            });
        }
    });
}

/// IvyCombatBrain's melee stance, timed attacks and cooldowns.
pub fn combat_goal(phase: u32, world: World<'_>, ivy: Entity<'_>) -> bool {
    let Some(ai) = MOB_AI.get() else { return false };
    if downed_state(ivy).active() && phase != goal_phase::STOP {
        if phase == goal_phase::TICK {
            ai.stop(ivy);
        }
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let target = ai.target(ivy).filter(|_| combat_allowed(world, ivy));
    let located = target.and_then(|id| Some((id, target_position(world, ivy, id)?)));
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => return located.is_some(),
        goal_phase::STOP => {
            ai.stop(ivy);
            let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(state) = states.get_mut(&ivy.id()) {
                state.move_ = None;
                state.stance = false;
                state.target = None;
                state.dash_read = DashRead::default();
            }
            return false;
        }
        goal_phase::TICK => {}
        _ => return false,
    }
    let (Some((id, at)), Some(view)) = (located, ivy.view()) else {
        return false;
    };
    let now = moddedmc_api::tick_count();
    ai.look_at(ivy, at);
    let distance = flight::length_squared(flight::offset(at, view.position)).sqrt();
    let mut state = {
        let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        states.remove(&ivy.id()).unwrap_or_default()
    };
    if state.target != Some(id) || !state.stance {
        state.target = Some(id);
        state.sword_damage = equipped_weapon(world, ivy);
        state.stance = true;
        state.move_ = None;
        state.dash_read = DashRead::default();
        state.ready = now + if state.sword_damage.is_some() { 17 } else { 10 };
    }
    if let Some(action) = state.move_ {
        ai.stop(ivy);
        let elapsed = now.saturating_sub(state.start);
        let sword_dash = action.clip == DASH[1].clip;
        if (action.clip == DASH[0].clip && elapsed == 3) || (sword_dash && elapsed == 4) {
            ivy.change(&EntityChange {
                mask: SET_MOTION,
                motion: forward_impulse(
                    view.position,
                    at,
                    view.motion,
                    if sword_dash { 0.85 } else { 1.15 },
                    if sword_dash { 0.03 } else { 0.02 },
                ),
                ..EntityChange::default()
            });
        }
        for &hit in action.hits.iter().filter(|hit| hit.tick == elapsed) {
            impact(
                world,
                ivy,
                id,
                hit,
                state.sword_damage,
                sword_dash || action.clip == DASH[0].clip,
            );
        }
        if elapsed >= action.duration {
            state.move_ = None;
        }
    } else if distance > 2.45 {
        let retreat = state.dash_read.retreating(view.position, at);
        if state.dash_read.moving >= 10 && now >= state.commit_ready {
            state.commit_ready = now + 18;
            if retreat
                && now >= state.ready
                && now >= state.combo_ready
                && now >= state.dash_ready
                && crate::ai::random(100) < 65
            {
                ai.stop(ivy);
                state.sword_damage = equipped_weapon(world, ivy);
                state.move_ = Some(DASH[usize::from(state.sword_damage.is_some())]);
                state.start = now;
                state.ready = now + 30;
                state.combo_ready = now + 30;
                state.dash_ready = now + 45;
            } else {
                ai.walk_to(ivy, at, 1.0);
            }
        } else {
            ai.walk_to(ivy, at, 1.0);
        }
    } else {
        state.dash_read.retreating(view.position, at);
        ai.stop(ivy);
        if now >= state.ready {
            state.sword_damage = equipped_weapon(world, ivy);
            let sword = state.sword_damage.is_some();
            let still = ENTITIES
                .get()
                .and_then(|entities| {
                    entities.with_entity(world, id, |target| {
                        target
                            .view()
                            .is_some_and(|v| v.motion[0].powi(2) + v.motion[2].powi(2) <= 0.0025)
                    })
                })
                .unwrap_or(false);
            let index = choose_move(
                sword,
                distance,
                still,
                now >= state.combo_ready,
                now >= state.hook_ready,
                crate::ai::random,
            );
            let action = if sword { SWORD[index] } else { BOXING[index] };
            state.start = now;
            state.ready = now + action.duration.max(action.cooldown);
            if index == 1 || index == 4 {
                state.hook_ready = now + 15;
            }
            if index >= 2 {
                state.combo_ready = now + 30;
            }
            state.move_ = Some(action);
        }
    }
    FIGHTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(ivy.id(), state);
    // shortcut: no reactive attacker in the pre-hurt hook, equipment post-hit
    // callbacks, or host projectile launch for Ivy's custom venom arrows yet.
    false
}

pub fn combat_clip(id: i32, moving: bool) -> Option<&'static str> {
    let states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = states.get(&id)?;
    if !state.stance {
        return None;
    }
    if let Some(action) = state.move_ {
        return Some(action.clip);
    }
    Some(match (state.sword_damage.is_some(), moving) {
        (true, true) => "sword_walk",
        (true, false) => "sword_idle",
        (false, true) => "orthodox_walk",
        (false, false) => "orthodox_idle",
    })
}

/// `IvyTheDragonMerchant.getOffers`: rolled the first time, then kept by her villager carrier.
fn open_trades(viewer: Entity<'_>, ivy: Entity<'_>) -> bool {
    let Some(mobs) = MOBS.get() else {
        return false;
    };
    let offers = if ivy.data(TRADES_ROLLED).is_some() {
        Vec::new()
    } else {
        crate::ivy_trades::roll().unwrap_or_default()
    };
    let opened = mobs.open_trades(ivy, viewer, &offers);
    if opened
        && !offers.is_empty()
        && let Ok(flag) = fastnbt::to_bytes(&Value::Compound(Compound::new()))
    {
        ivy.set_data(TRADES_ROLLED, &flag);
    }
    opened
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
    fn trade_resume_only_schedules_matching_close_once() {
        let mut resume = TradeResume {
            ivy: [1; 16],
            dimension: "overworld".into(),
            path: "first_meeting".into(),
            node: "anchor_intro".into(),
            due: None,
        };
        resume.close([2; 16], Some("overworld"), 10);
        resume.close([1; 16], Some("nether"), 10);
        assert_eq!(resume.due, None);
        resume.close([1; 16], Some("overworld"), 11);
        assert_eq!(resume.due, Some(14));
        resume.close([1; 16], Some("overworld"), 12);
        assert_eq!(resume.due, Some(14));
    }

    #[test]
    fn downed_revive_grace_and_bleed_out() {
        let mut state = DownedState {
            downed: true,
            downed_bleed_ticks: 1800,
            ..DownedState::default()
        };
        for _ in 0..12 {
            state.interact();
        }
        assert_eq!(state.downed_revive_progress, 60);
        for _ in 0..8 {
            assert!(!state.tick());
        }
        assert_eq!(state.downed_revive_progress, 60);
        assert!(!state.tick());
        assert_eq!(state.downed_revive_progress, 59);
        for _ in 0..3 {
            state.finish_hit();
        }
        assert!(state.tick());
        assert_eq!(state.downed_bleed_ticks, 1491);
        let saved = fastnbt::to_bytes(&state).unwrap();
        let restored: DownedState = fastnbt::from_bytes(&saved).unwrap();
        assert!(restored.downed);
        assert_eq!(restored.downed_finish_hits, 3);
    }

    #[test]
    fn dash_requires_sustained_retreat_in_range() {
        let mut read = DashRead::default();
        let from = [0.0; 3];
        assert!(!read.retreating(from, [2.5, 0.0, 0.0]));
        for i in 1..10 {
            assert!(!read.retreating(from, [2.5 + f64::from(i) * 0.1, 0.0, 0.0]));
        }
        assert!(read.retreating(from, [3.5, 0.0, 0.0]));
        assert!(!read.retreating(from, [3.4, 0.0, 0.0]));
        assert!(!read.retreating(from, [5.0, 0.0, 0.0]));
        assert_eq!(
            forward_impulse(from, [3.0, 0.0, 0.0], [0.0; 3], 1.15, 0.02),
            [1.15, 0.02, 0.0]
        );
        assert_eq!(
            forward_impulse(from, [3.0, 0.0, 0.0], [0.0; 3], 0.85, 0.03),
            [0.85, 0.03, 0.0]
        );
    }

    #[test]
    fn equipment_modifiers_obey_slot_and_operation() {
        let modifier = |slot: &str, operation: &str, amount| {
            Value::Compound(Compound::from([
                (
                    "type".into(),
                    Value::String("minecraft:attack_damage".into()),
                ),
                ("slot".into(), Value::String(slot.into())),
                ("operation".into(), Value::String(operation.into())),
                ("amount".into(), Value::Double(amount)),
            ]))
        };
        let value = Value::List(vec![
            modifier("mainhand", "add_value", 5.0),
            modifier("offhand", "add_value", 100.0),
            modifier("hand", "add_multiplied_base", 1.0),
            modifier("any", "add_multiplied_total", 0.5),
        ]);
        assert_eq!(
            attribute_from(value.clone(), "attack_damage", "mainhand", 1.0),
            Some(10.5)
        );
        assert_eq!(attribute_from(value, "armor", "head", 0.0), None);
    }

    #[test]
    fn attack_selection_respects_combo_and_hook_cooldowns() {
        for sword in [false, true] {
            assert_eq!(choose_move(sword, 2.0, true, false, false, |_| 0), 0);
            assert_eq!(choose_move(sword, 2.0, true, false, true, |n| n - 1), 0);
            assert_eq!(choose_move(sword, 2.3, true, true, true, |_| 0), 4);
        }
        assert_eq!(choose_move(false, 2.0, true, true, false, |n| n - 1), 3);
        assert_eq!(choose_move(true, 2.0, true, true, false, |n| n - 1), 5);
    }

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
