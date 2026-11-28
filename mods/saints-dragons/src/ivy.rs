//! Ivy's released dialogue tree, recruited ownership, and plushie recall.
use std::{
    cell::Cell,
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
    entities::{ENTITY_ALIVE, Entity, EntityBody, EntityChange, SET_MOTION},
    interaction::{UseItem, UseMob, UseResult},
    inventory_menu::{self, Access, Change, MenuKind, Validated, Validity, ValiditySlots},
    items::{Item, ItemDefaults},
    mob_ai::goal_phase,
    mobs::ProjectileStep,
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
    name: String,
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
    #[serde(default)]
    revision: u32,
}

fn inventories(world: World<'_>) -> Option<Vec<Inventory>> {
    match SAVES.get()?.read(&world, *INVENTORY_KEY.get()?) {
        Some(bytes) => serde_json::from_slice(&bytes).ok(),
        None => Some(Vec::new()),
    }
}

fn inventory_record(world: World<'_>, uuid: [u8; 16]) -> Option<Inventory> {
    Some(
        inventories(world)?
            .into_iter()
            .find(|record| record.ivy == uuid)
            .unwrap_or_else(|| Inventory {
                ivy: uuid,
                slots: vec![None; INVENTORY_SIZE],
                revision: 0,
            }),
    )
}

fn inventory(world: World<'_>, uuid: [u8; 16]) -> Option<Vec<Option<Vec<u8>>>> {
    Some(inventory_record(world, uuid)?.slots)
}

/// StegonautBuffAbility.hasBoundStegonautBinder checks Ivy's actual saved inventory.
pub fn has_bound_binder(world: World<'_>, ivy: Entity<'_>) -> bool {
    MOBS.get()
        .and_then(|mobs| inventory(world, mobs.uuid(ivy)?))
        .is_some_and(|slots| {
            slots
                .iter()
                .flatten()
                .any(|stack| crate::combat_stegonaut::bound_binder(stack))
        })
}

// A new revision invalidates the old native view when Ivy consumes a stack.
fn inventory_id(id: i32, revision: u32) -> u64 {
    (u64::from(revision) << 32) | u64::from(id as u32)
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
    let Some(record) = inventory_record(world, uuid) else {
        return false;
    };
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
        inventory_id(ivy.id(), record.revision),
        if cfg!(feature = "original-assets") {
            "moddedmc:screen/saintsdragons:ivy_inventory"
        } else {
            NAME
        },
        ValiditySlots {
            world,
            access: &access,
            context: &uuid,
        },
        || Some(record.slots),
    )
}

fn valid_inventory(view: Validity<'_>) -> bool {
    let (Some(mobs), Some(entities), Ok(id)) = (
        MOBS.get(),
        ENTITIES.get(),
        i32::try_from(view.inventory as u32),
    ) else {
        return false;
    };
    let mut valid = false;
    view.player.with_entity_context(|world, viewer| {
        if world.dimension_id() != view.world.dimension_id() {
            return;
        }
        valid = entities.with_entity(world, id, |ivy| {
            mobs.uuid(ivy).is_some_and(|uuid| {
                uuid == view.context
                    && inventory_record(world, uuid)
                        .is_some_and(|record| view.inventory == inventory_id(id, record.revision))
            }) && can_access(ivy, viewer)
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
        i32::try_from(change.inventory as u32),
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
    let revision = records
        .iter()
        .find(|record| record.ivy == uuid)
        .map_or(0, |record| record.revision);
    if change.inventory != inventory_id(id, revision) {
        return;
    }
    records.retain(|record| record.ivy != uuid);
    records.push(Inventory {
        ivy: uuid,
        slots,
        revision,
    });
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
    moddedmc_api::effects::require()?.require_active_effects()?;
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
    with: Vec<Text>,
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
    name_input: bool,
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
    let template = lang
        .get(&content.translate)
        .map_or(content.text.as_str(), String::as_str)
        .replace("{name}", name);
    let args: Vec<_> = content
        .with
        .iter()
        .map(|arg| text(arg, lang, name))
        .collect();
    let mut next = 0;
    let mut out = String::new();
    let mut remaining = template.as_str();
    while let Some((prefix, suffix)) = remaining.split_once('%') {
        out.push_str(prefix);
        if let Some(rest) = suffix.strip_prefix('%') {
            out.push('%');
            remaining = rest;
        } else if let Some(rest) = suffix.strip_prefix('s') {
            out.push_str(args.get(next).map_or(name, String::as_str));
            next += 1;
            remaining = rest;
        } else if let Some((index, rest)) = suffix.split_once("$s")
            && let Ok(index) = index.parse::<usize>()
            && index > 0
        {
            out.push_str(args.get(index - 1).map_or(name, String::as_str));
            remaining = rest;
        } else {
            out.push('%');
            remaining = suffix;
        }
    }
    out.push_str(remaining);
    out
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
        "use_player_name" => {
            if let Some(player) = viewer.as_player() {
                memory.name = player.name().to_owned();
                memory.known = true;
            }
        }
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

/// DialogueSessionRegistry.sanitizeName: normalize spaces, discard controls,
/// and retain at most fifty non-whitespace characters.
fn chosen_name(raw: &str, username: &str) -> String {
    let mut result = String::new();
    let mut count = 0;
    let mut space = false;
    for c in raw.chars().filter(|c| !c.is_control()) {
        if c.is_whitespace() {
            space = !result.is_empty();
        } else {
            if space {
                result.push(' ');
                space = false;
            }
            result.push(c);
            count += 1;
            if count == 50 {
                break;
            }
        }
    }
    if result.is_empty() {
        username.to_owned()
    } else {
        result
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
    let name = if memory.name.is_empty() {
        player.name()
    } else {
        &memory.name
    };
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
            action: format!("{ACTION}{nonce}/0"),
            tooltip: String::new(),
        });
        actions.push(Action {
            next: String::new(),
            flag: String::new(),
            impression: String::new(),
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
    let name_input = node.kind == "name_input";
    if cfg!(feature = "original-assets") {
        let mut state = serde_json::json!({
            "speaker": "Ivy Oleander", "body": body, "name_input": name_input,
        });
        let mut allowed = HashMap::new();
        let mut choices = Vec::new();
        for (index, button) in buttons.iter().enumerate() {
            allowed.insert(button.action.clone(), false);
            state[format!("choice_{index}")] = serde_json::json!(button.action);
            let mut row = serde_json::json!({
                "label": button.label, "action": button.action,
                "close": actions[index].next.is_empty() || node.kind == "open_trade" || node.kind == "open_inventory",
            });
            if name_input && index == 0 {
                let action = format!("{}/name", button.action);
                allowed.insert(action.clone(), true);
                state["name_action"] = serde_json::json!(action);
                row["typed_action"] = serde_json::json!(action);
            }
            choices.push(row);
        }
        state["choices"] = serde_json::json!(choices);
        buttons.push(MenuButton {
            label: String::new(),
            action: "moddedmc:pack_screen".into(),
            tooltip: serde_json::json!({
                "id": "saintsdragons:ivy_dialogue", "state": state, "actions": allowed,
            })
            .to_string(),
        });
    }
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
            name_input,
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
    if click.stack.read().is_some_and(|stack| named_tag(&stack)) {
        // The original refuses a named tag before every other interaction.
        chatter_speak(click.world, ivy, "rename_refusal", None, 90, 120);
        return UseResult::Consume;
    }
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
    if underwater(click.world, ivy) {
        chatter_speak(click.world, ivy, "underwater", Some(viewer), 90, 80);
        return UseResult::Consume;
    }
    if click.sneaking && taming::is_owner(ivy, viewer) {
        if click.main_hand {
            taming::cycle_ivy_order(ivy, viewer);
            let pool = match taming::order_name(ivy) {
                "follow" => "command_follow",
                "stay" => "command_stay",
                _ => "command_wander",
            };
            chatter_speak(click.world, ivy, pool, Some(viewer), 90, 80);
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
    let mut parts = action.splitn(3, '/');
    let (Some(nonce), Some(index)) = (parts.next(), parts.next()) else {
        return true;
    };
    let typed = match parts.next() {
        None => None,
        Some(suffix) => match suffix.strip_prefix("name/") {
            Some(value)
                if value.len() <= 64
                    && value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.- ".contains(&b)) =>
            {
                Some(value)
            }
            _ => return true,
        },
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
                    || typed.is_some() && (!session.name_input || index != 0)
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
            if session.name_input {
                memory.name = chosen_name(typed.unwrap_or_default(), player.name());
                memory.known = true;
            }
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
            if action.next.is_empty() {
                save_memory(world, viewer, &memory);
                return;
            }
            let (path, node) = action
                .next
                .strip_prefix("saintsdragons:ivy/")
                .and_then(|reference| reference.split_once('#'))
                .unwrap_or((&session.path, &action.next));
            if session.trade {
                save_memory(world, viewer, &memory);
                if open_trades(world, viewer, ivy) {
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

pub const COMBAT: u64 = 170;

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
    totem_pending: bool,
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

/// IvyTheDragonMerchant.hurtDowned.
pub fn hurt(world: World<'_>, ivy: Entity<'_>, damage_type: &str, amount: f32) -> bool {
    let mut state = downed_state(ivy);
    if !state.active()
        && let (Some(ai), Some(entities)) = (MOB_AI.get(), ENTITIES.get())
        && let Some((causing, _)) = ai.damage_source()
    {
        entities.with_entity(world, causing, |attacker| {
            if !enemy(world, ivy, attacker)
                && attacker
                    .view()
                    .is_some_and(|v| v.kind & moddedmc_api::entities::ENTITY_LIVING != 0)
            {
                if ai.target(ivy) == Some(causing) {
                    ai.set_target(world, ivy, None);
                }
                if ai
                    .target(ivy)
                    .is_none_or(|id| target_position(world, ivy, id).is_none())
                {
                    let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
                    if let Some(fight) = states.get_mut(&ivy.id()) {
                        fight.move_ = None;
                        fight.retreat = None;
                        fight.stance = false;
                        fight.target = None;
                    }
                }
            }
        });
    }
    if !state.downed
        || matches!(
            damage_type,
            "minecraft:out_of_world" | "minecraft:generic_kill"
        )
    {
        return true;
    }
    let mob_attacker =
        MOB_AI
            .get()
            .and_then(|ai| ai.damage_source())
            .is_some_and(|(causing, direct)| {
                [causing, direct]
                    .into_iter()
                    .filter(|id| *id >= 0)
                    .any(|id| {
                        ENTITIES
                            .get()
                            .and_then(|entities| {
                                entities.with_entity(world, id, |entity| {
                                    entity.view().is_some_and(|view| {
                                        view.kind & moddedmc_api::entities::ENTITY_LIVING != 0
                                            && view.kind & moddedmc_api::entities::ENTITY_PLAYER
                                                == 0
                                    })
                                })
                            })
                            .unwrap_or(false)
                    })
            });
    if amount > 0.0 && !mob_attacker {
        state.finish_hit();
        save_downed(ivy, &state);
    }
    false
}

/// IvyTheDragonMerchant.consumeOneInventoryItem: consume the first matching storage stack.
fn consume_matching(
    slots: &mut [Option<Vec<u8>>],
    matches: impl Fn(&Compound, &[u8]) -> bool,
) -> bool {
    for slot in slots.iter_mut().skip(STORAGE_START) {
        let Some(bytes) = slot.as_deref() else {
            continue;
        };
        let Ok(mut stack) = fastnbt::from_bytes::<Compound>(bytes) else {
            continue;
        };
        if !matches(&stack, bytes) {
            continue;
        }
        match stack.get("count") {
            Some(Value::Int(1)) => *slot = None,
            Some(Value::Int(count)) if *count > 1 => {
                stack.insert("count".into(), Value::Int(count - 1));
                let Ok(bytes) = fastnbt::to_bytes(&stack) else {
                    return false;
                };
                *slot = Some(bytes);
            }
            _ => continue,
        }
        return true;
    }
    false
}

fn consume_totem(slots: &mut [Option<Vec<u8>>]) -> bool {
    consume_matching(
        slots,
        |stack, _| matches!(stack.get("id"), Some(Value::String(id)) if id == "minecraft:totem_of_undying"),
    )
}

fn consume_inventory(
    world: World<'_>,
    ivy: Entity<'_>,
    consume: impl FnOnce(&mut [Option<Vec<u8>>]) -> bool,
) -> Option<()> {
    let uuid = MOBS.get()?.uuid(ivy)?;
    let _lock = INVENTORY_WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut records = inventories(world)?;
    let record = records.iter_mut().find(|record| record.ivy == uuid)?;
    if !consume(&mut record.slots) {
        return None;
    }
    record.revision = record.revision.checked_add(1)?;
    let bytes = serde_json::to_vec(&records).ok()?;
    SAVES
        .get()?
        .write(&world, *INVENTORY_KEY.get()?, &bytes)
        .then_some(())
}

fn use_totem(world: World<'_>, ivy: Entity<'_>) -> Option<()> {
    consume_inventory(world, ivy, consume_totem)
}

fn named_tag(bytes: &[u8]) -> bool {
    fastnbt::from_bytes::<Compound>(bytes).is_ok_and(|stack| {
        matches!(stack.get("id"), Some(Value::String(id)) if id == "minecraft:name_tag")
            && matches!(stack.get("components"), Some(Value::Compound(components)) if components.contains_key("minecraft:custom_name"))
    })
}

/// IvyTheDragonMerchant.isCookedRecoveryFood: cooked edible items only.
fn cooked_food(stack: &Compound, bytes: &[u8]) -> bool {
    let Some(Value::String(id)) = stack.get("id") else {
        return false;
    };
    let Some((_, path)) = id.split_once(':') else {
        return false;
    };
    path.contains("cooked") && component(bytes, "minecraft:food").is_some()
}

fn has_recovery_food(world: World<'_>, ivy: Entity<'_>) -> bool {
    MOBS.get()
        .and_then(|mobs| inventory(world, mobs.uuid(ivy)?))
        .is_some_and(|slots| {
            slots.iter().skip(STORAGE_START).flatten().any(|bytes| {
                fastnbt::from_bytes::<Compound>(bytes).is_ok_and(|stack| cooked_food(&stack, bytes))
            })
        })
}

fn has_storage_item(world: World<'_>, ivy: Entity<'_>, id: &str) -> bool {
    MOBS.get()
        .and_then(|mobs| inventory(world, mobs.uuid(ivy)?))
        .is_some_and(|slots| {
            slots.iter().skip(STORAGE_START).flatten().any(|bytes| {
                fastnbt::from_bytes::<Compound>(bytes).is_ok_and(
                    |stack| matches!(stack.get("id"), Some(Value::String(item)) if item == id),
                )
            })
        })
}

fn harmful_effects(ivy: Entity<'_>) -> Vec<String> {
    EFFECTS
        .get()
        .and_then(|effects| effects.active_effects(ivy))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(category, id)| (category == 1).then_some(id))
        .collect()
}

fn bucket_stack() -> Option<Vec<u8>> {
    fastnbt::to_bytes(&Compound::from([
        ("id".into(), Value::String("minecraft:bucket".into())),
        ("count".into(), Value::Int(1)),
    ]))
    .ok()
}

/// Replace the consumed milk with its empty bucket. When an externally stacked
/// milk item leaves no storage space, drop the remainder after the save commits.
fn consume_milk(slots: &mut [Option<Vec<u8>>], bucket: &[u8]) -> Option<bool> {
    let index = slots
        .iter()
        .enumerate()
        .skip(STORAGE_START)
        .find_map(|(i, slot)| {
            let stack = fastnbt::from_bytes::<Compound>(slot.as_deref()?).ok()?;
            matches!(stack.get("id"), Some(Value::String(id)) if id == "minecraft:milk_bucket")
                .then_some(i)
        })?;
    let mut stack = fastnbt::from_bytes::<Compound>(slots[index].as_deref()?).ok()?;
    match stack.get("count") {
        Some(Value::Int(1)) => {
            slots[index] = Some(bucket.to_vec());
            return Some(false);
        }
        Some(Value::Int(count)) if *count > 1 => {
            stack.insert("count".into(), Value::Int(count - 1));
            slots[index] = Some(fastnbt::to_bytes(&stack).ok()?);
        }
        _ => return None,
    }
    for slot in slots.iter_mut().skip(STORAGE_START) {
        if slot.is_none() {
            *slot = Some(bucket.to_vec());
            return Some(false);
        }
        let Some(bytes) = slot.as_deref() else {
            continue;
        };
        let Ok(mut stack) = fastnbt::from_bytes::<Compound>(bytes) else {
            continue;
        };
        if matches!(stack.get("id"), Some(Value::String(id)) if id == "minecraft:bucket")
            && !matches!(stack.get("components"), Some(Value::Compound(c)) if !c.is_empty())
            && let Some(Value::Int(count)) = stack.get("count")
            && (1..16).contains(count)
        {
            stack.insert("count".into(), Value::Int(count + 1));
            *slot = Some(fastnbt::to_bytes(&stack).ok()?);
            return Some(false);
        }
    }
    Some(true)
}

fn drink_milk(world: World<'_>, ivy: Entity<'_>) {
    let Some(bucket) = bucket_stack() else { return };
    let mut drop = false;
    if consume_inventory(world, ivy, |slots| {
        let Some(remainder) = consume_milk(slots, &bucket) else {
            return false;
        };
        drop = remainder;
        true
    })
    .is_none()
    {
        return;
    }
    if drop && let (Some(stacks), Some(view)) = (STACKS.get(), ivy.view()) {
        stacks.spawn(world, view.position, [0.0; 3], &bucket);
    }
    for effect in harmful_effects(ivy) {
        ivy.remove_effect(&effect);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryKind {
    Food,
    Milk,
}

fn recovery_kind(world: World<'_>, ivy: Entity<'_>, threshold: f32) -> Option<RecoveryKind> {
    if has_storage_item(world, ivy, "minecraft:milk_bucket") && !harmful_effects(ivy).is_empty() {
        return Some(RecoveryKind::Milk);
    }
    (MOBS
        .get()?
        .health(ivy)
        .is_some_and(|(health, _)| health > 0.0 && health < threshold)
        && has_recovery_food(world, ivy))
    .then_some(RecoveryKind::Food)
}

fn consume_recovery(world: World<'_>, ivy: Entity<'_>, kind: RecoveryKind) {
    match kind {
        RecoveryKind::Milk => drink_milk(world, ivy),
        RecoveryKind::Food => {
            if consume_inventory(world, ivy, |slots| consume_matching(slots, cooked_food)).is_some()
                && let Some(mobs) = MOBS.get()
            {
                mobs.heal(ivy, 8.0);
            }
        }
    }
}

/// IvyTheDragonMerchant.tickPassiveUse: 20 ticks, consume on tick 14, 100 tick cooldown.
fn passive_recovery(world: World<'_>, ivy: Entity<'_>, blocked: bool) {
    let now = moddedmc_api::tick_count();
    let Some(uuid) = MOBS.get().and_then(|mobs| mobs.uuid(ivy)) else {
        return;
    };
    let (start, ready, kind) = {
        let states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(state) = states.get(&ivy.id()) else {
            return;
        };
        (
            state.recovery_start,
            state.recovery_ready,
            state.recovery_kind,
        )
    };
    if start.is_none() && (blocked || now < ready) {
        return;
    }
    let busy = blocked
        || downed_state(ivy).active()
        || SESSIONS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|session| session.ivy == uuid && session.expires > now)
        || TRADE_RESUMES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|session| session.ivy == uuid);
    if let Some(start) = start {
        let age = now.saturating_sub(start);
        if busy || age >= 20 {
            if let Some(state) = FIGHTS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_mut(&ivy.id())
            {
                state.recovery_start = None;
                state.recovery_kind = None;
            }
        } else if age == 14
            && let Some(kind) = kind
        {
            consume_recovery(world, ivy, kind);
        }
    } else if !busy
        && let Some(kind) = recovery_kind(world, ivy, 20.0)
        && let Some(state) = FIGHTS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&ivy.id())
    {
        state.recovery_start = Some(now);
        state.recovery_kind = Some(kind);
        state.recovery_ready = now + 100;
    }
}

/// IvyTheDragonMerchant.tryUseInventoryTotem: vanilla effects after the final hit commits.
fn totem_effects(world: World<'_>, ivy: Entity<'_>) {
    ivy.remove_all_effects();
    ivy.add_effect("minecraft:regeneration", 900, 1, false, true);
    ivy.add_effect("minecraft:absorption", 100, 1, false, true);
    ivy.add_effect("minecraft:fire_resistance", 800, 0, false, true);
    ivy.broadcast_event(35);
    if let (Some(effects), Some(view)) = (EFFECTS.get(), ivy.view()) {
        effects.play_sound(
            world,
            view.position,
            "minecraft:item.totem.use",
            SOUND_NEUTRAL,
            1.0,
            1.0,
        );
    }
}

/// IvyTheDragonMerchant.actuallyHurt: consume an inventory totem before entering downed.
pub fn hurt_final(
    world: World<'_>,
    ivy: Entity<'_>,
    damage_type: &str,
    amount: f32,
    lethal: bool,
    absorption: f32,
) -> f32 {
    if amount > 0.0
        && !downed_state(ivy).active()
        && let Some((causing, _)) = MOB_AI.get().and_then(|ai| ai.damage_source())
        && let Some(entities) = ENTITIES.get()
    {
        entities.with_entity(world, causing, |attacker| {
            if taming::is_owner(ivy, attacker) {
                chatter_speak(world, ivy, "owner_hurt", Some(attacker), 90, 120);
            }
        });
    }
    if !lethal
        || matches!(
            damage_type,
            "minecraft:out_of_world" | "minecraft:generic_kill"
        )
        || crate::TAGS.get().is_some_and(|tags| {
            tags.contains(
                "damage_type",
                "minecraft:bypasses_invulnerability",
                damage_type,
            )
        })
    {
        return amount;
    }
    let Some((health, _)) = MOBS.get().and_then(|mobs| mobs.health(ivy)) else {
        return amount;
    };
    if use_totem(world, ivy).is_some() {
        let mut state = downed_state(ivy);
        state.totem_pending = true;
        save_downed(ivy, &state);
        if health < 1.0
            && let Some(vitals) = crate::MOB_HEALTH.get()
        {
            vitals.set(ivy, 1.0);
        }
        return (health - 1.0).max(0.0) + absorption;
    }
    if !taming::is_tamed(ivy) {
        return amount;
    }
    save_downed(
        ivy,
        &DownedState {
            downed: true,
            downed_bleed_ticks: 1800,
            ..DownedState::default()
        },
    );
    chatter_speak(world, ivy, "downed", None, 1800, 80);
    if let Some(ai) = MOB_AI.get() {
        ai.stop(ivy);
        ai.set_target(world, ivy, None);
    }
    if health < 1.0
        && let Some(vitals) = crate::MOB_HEALTH.get()
    {
        vitals.set(ivy, 1.0);
    }
    (health - 1.0).max(0.0) + absorption
}

/// IvyTheDragonMerchant.tickDownedState, including the stock-client animation and revive timer.
pub fn downed_tick(world: World<'_>, ivy: Entity<'_>) {
    let mut state = downed_state(ivy);
    if state.totem_pending {
        // Apply absorption after the host has deducted the lethal hit's old absorption.
        state.totem_pending = false;
        save_downed(ivy, &state);
        totem_effects(world, ivy);
    }
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
            && let Some(attacker) = entities
                .with_entity(world, id, |owner| {
                    [mobs.last_hurt_by_mob(owner), mobs.last_hurt_mob(owner)]
                        .into_iter()
                        .flatten()
                        .map(|(id, _)| id)
                        .find(|&id| target_position(world, ivy, id).is_some())
                })
                .flatten()
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

/// IvyTheDragonMerchant.canDance: refuse while downed, riding, wet, in dialogue or combat.
pub fn can_dance(world: World<'_>, ivy: Entity<'_>) -> bool {
    let Some(uuid) = MOBS.get().and_then(|mobs| mobs.uuid(ivy)) else {
        return false;
    };
    !downed_state(ivy).active()
        && ivy.vehicle().is_none()
        && combat_clip(ivy.id(), false).is_none()
        && ivy
            .view()
            .is_some_and(|view| !crate::eggs::water_at(world, flight::cell(view.position)))
        && !SESSIONS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|session| session.ivy == uuid && session.expires > moddedmc_api::tick_count())
        && !TRADE_RESUMES
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|trade| trade.ivy == uuid)
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
    projectile_retreat: bool,
    skirmishing: bool,
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
        self.previous = Some(at);
        if speed <= 0.05 {
            self.moving = 0;
            self.projectile_retreat = false;
            self.skirmishing = false;
            return false;
        }
        self.moving = self.moving.saturating_add(1);
        let away = flight::offset(at, from);
        let old = flight::offset(previous, from);
        let distance_sq = flight::length_squared(away);
        let dot = (away[0] * delta[0] + away[2] * delta[2]) / (distance_sq.sqrt() * speed);
        let previous_distance_sq = flight::length_squared(old);
        self.projectile_retreat = dot >= 0.45 && distance_sq > previous_distance_sq + 0.015;
        self.skirmishing = self.projectile_retreat
            || (self.moving >= 10
                && distance_sq.sqrt() >= 3.8
                && distance_sq.sqrt() >= previous_distance_sq.sqrt() - 0.2);
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
    chatter: Option<Chatter>,
    chatter_cooldown: Option<u64>,
    chatter_target: Option<i32>,
    armor_synced: bool,
    self_hurt: Option<(i32, i64)>,
    owner_hurt: Option<(i32, i64)>,
    owner_attack: Option<(i32, i64)>,
    recovery_start: Option<u64>,
    recovery_kind: Option<RecoveryKind>,
    recovery_ready: u64,
    clutch_ready: u64,
    clutch_clip_until: u64,
    clutch_water: Option<(moddedmc_api::Pos, u64)>,
    retreat: Option<Retreat>,
    retreat_ready: u64,
    projectile_ready: u64,
    projectile_punish_until: u64,
    projectiles: Vec<VenomShot>,
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

const THROW: [Move; 2] = [
    Move {
        clip: "orthodox_throw_projectiles",
        duration: 26,
        hits: &[],
        cooldown: 100,
    },
    Move {
        clip: "sword_throw_projectiles",
        duration: 26,
        hits: &[],
        cooldown: 100,
    },
];

struct VenomShot {
    id: i32,
    velocity: [f64; 3],
    age: u32,
}

fn visible_target(world: World<'_>, ivy: Entity<'_>, target: Entity<'_>) -> bool {
    let eye = |entity: Entity<'_>| {
        let mut p = entity.view()?.position;
        let body = entity.body()?;
        p[1] = body.min[1] + (body.max[1] - body.min[1]) * 0.85;
        Some(p)
    };
    eye(ivy).zip(eye(target)).is_some_and(|(from, to)| {
        world
            .raycast(from, to, Some(ivy.id()))
            .is_some_and(|(block, _)| block.is_none())
    })
}

fn can_throw(
    world: World<'_>,
    ivy: Entity<'_>,
    state: &Fight,
    id: i32,
    distance: f64,
    now: u64,
) -> bool {
    now >= state.ready
        && now >= state.projectile_ready
        && (5.0..=16.0).contains(&distance)
        && has_storage_item(world, ivy, crate::venom_arrow::ID)
        && ENTITIES.get().and_then(|entities| {
            entities.with_entity(world, id, |target| {
                let kind = target.type_id().trim_start_matches("minecraft:");
                visible_target(world, ivy, target)
                    && (matches!(kind, "pillager" | "evoker" | "witch" | "vex")
                        || if target.as_player().is_some() {
                            state.dash_read.projectile_retreat
                        } else {
                            now < state.projectile_punish_until || state.dash_read.skirmishing
                        })
            })
        }) == Some(true)
}

/// IvyCombatBrain.throwVenomArrowAtTarget: two arrows, predict four ticks of
/// horizontal movement, use the arrow's released velocity multiplier.
fn throw_venom(world: World<'_>, ivy: Entity<'_>, id: i32, state: &mut Fight) {
    let (Some(entities), Some(mobs), Some(view), Some(body)) =
        (ENTITIES.get(), MOBS.get(), ivy.view(), ivy.body())
    else {
        return;
    };
    let Some((origin, velocity)) = entities
        .with_entity(world, id, |target| {
            if !enemy(world, ivy, target) || !visible_target(world, ivy, target) {
                return None;
            }
            let (target_view, target_body) = (target.view()?, target.body()?);
            let yaw = f64::from(view.yaw).to_radians();
            let pitch = f64::from(view.pitch).to_radians();
            let look = [
                -yaw.sin() * pitch.cos(),
                -pitch.sin(),
                yaw.cos() * pitch.cos(),
            ];
            let origin = flight::add(
                flight::add(
                    view.position,
                    [0.0, (body.max[1] - body.min[1]) * 0.62, 0.0],
                ),
                look.map(|x| x * 0.55),
            );
            let aim = flight::add(
                target_view.position,
                [
                    target_view.motion[0] * 4.0,
                    (target_body.max[1] - target_body.min[1]) * 0.55,
                    target_view.motion[2] * 4.0,
                ],
            );
            let delta = flight::offset(aim, origin);
            let length = flight::length_squared(delta).sqrt();
            let direction = if length < 0.01 {
                look
            } else {
                delta.map(|x| x / length)
            };
            // Vanilla's triangular uncertainty at inaccuracy 2.0.
            let velocity = direction.map(|x| {
                let jitter =
                    (crate::ai::random(10001) as f64 - crate::ai::random(10001) as f64) / 10000.0;
                (x + jitter * 0.0172275 * 2.0) * 1.55 * 1.35
            });
            Some((origin, velocity))
        })
        .flatten()
    else {
        return;
    };
    let Some(projectile) =
        mobs.spawn_controlled_projectile(world, "minecraft:arrow", ivy.id(), origin, velocity)
    else {
        return;
    };
    if consume_inventory(world, ivy, |slots| consume_matching(slots, |stack, _|
        matches!(stack.get("id"), Some(Value::String(id)) if id == crate::venom_arrow::ID)
    )).is_none() {
        entities.with_entity(world, projectile, |arrow| arrow.discard());
        return;
    }
    state.projectiles.push(VenomShot {
        id: projectile,
        velocity,
        age: 0,
    });
}

// Same entry-plane test and 0.3 expansion used by the host trace. The v1
// projectile result returns only position/kind, so remember the nearest
// candidate during its synchronous filter call.
fn projectile_entry(from: [f64; 3], delta: [f64; 3], body: EntityBody) -> Option<f64> {
    let min = body.min.map(|x| x - 0.3);
    let max = body.max.map(|x| x + 0.3);
    let mut scale = 1.0;
    let mut hit = false;
    for axis in 0..3 {
        if delta[axis].abs() <= 1.0e-7 {
            continue;
        }
        let plane = if delta[axis] > 0.0 {
            min[axis]
        } else {
            max[axis]
        };
        let s = (plane - from[axis]) / delta[axis];
        if s <= 0.0 || s >= scale {
            continue;
        }
        if (0..3).filter(|&i| i != axis).all(|i| {
            let p = from[i] + s * delta[i];
            min[i] - 1.0e-7 < p && p < max[i] + 1.0e-7
        }) {
            scale = s;
            hit = true;
        }
    }
    hit.then_some(scale)
}

fn tick_venom(world: World<'_>, ivy: Entity<'_>) {
    let (Some(entities), Some(mobs)) = (ENTITIES.get(), MOBS.get()) else {
        return;
    };
    let mut shots = {
        let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        std::mem::take(&mut states.entry(ivy.id()).or_default().projectiles)
    };
    shots.retain_mut(|shot| {
        entities
            .with_entity(world, shot.id, |arrow| {
                if arrow.projectile_owner() != Some(ivy.id())
                    || arrow.type_id().trim_start_matches("minecraft:") != "arrow"
                {
                    return false;
                }
                let Some(view) = arrow.view() else {
                    return false;
                };
                let nearest = Cell::new(None::<(f64, i32)>);
                let hit = mobs.step_controlled_projectile(arrow, shot.velocity, |candidate| {
                    if candidate.id() == ivy.id() {
                        return false;
                    }
                    if let Some(s) = candidate
                        .body()
                        .and_then(|b| projectile_entry(view.position, shot.velocity, b))
                        && nearest.get().is_none_or(|(old, _)| s < old)
                    {
                        nearest.set(Some((s, candidate.id())));
                    }
                    true
                });
                match hit {
                    Some(ProjectileStep::Entity(_)) => {
                        if let Some((_, target)) = nearest.get() {
                            entities.with_entity(world, target, |target| {
                                // A friendly collision discards the arrow without damage or poison.
                                if enemy(world, ivy, target) {
                                    let damage =
                                        (flight::length_squared(shot.velocity).sqrt() * 2.0).ceil()
                                            as f32;
                                    if mobs.hurt_from(target, arrow, ivy, "minecraft:arrow", damage)
                                        == Some(true)
                                    {
                                        mobs.reset_hurt_cooldown(target);
                                        target.add_effect("minecraft:poison", 600, 1, false, true);
                                        target.add_effect(
                                            "minecraft:blindness",
                                            100,
                                            0,
                                            false,
                                            true,
                                        );
                                    }
                                }
                            });
                        }
                    }
                    Some(ProjectileStep::Miss(position)) if shot.age < 200 => {
                        shot.age += 1;
                        let water = matches!(
                            world.fluid_id(flight::cell(position)).as_deref(),
                            Some("minecraft:water" | "minecraft:flowing_water")
                        );
                        if !water {
                            shot.velocity = shot.velocity.map(|x| x * 0.99);
                        }
                        shot.velocity[1] -= 0.05;
                        return true;
                    }
                    _ => {}
                }
                arrow.discard();
                false
            })
            .unwrap_or(false)
    });
    FIGHTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .entry(ivy.id())
        .or_default()
        .projectiles
        .extend(shots);
}

struct Chatter {
    key: String,
    name: String,
    until: u64,
}

fn chatter_name(world: World<'_>, ivy: Entity<'_>, viewer: Option<Entity<'_>>) -> String {
    fn name(world: World<'_>, viewer: Entity<'_>) -> String {
        let name = memory(world, viewer).name;
        if name.is_empty() {
            viewer
                .as_player()
                .map_or_else(String::new, |player| player.name().to_owned())
        } else {
            name
        }
    }
    if let Some(viewer) = viewer {
        return name(world, viewer);
    }
    taming::owner(ivy)
        .and_then(|owner| MOBS.get()?.by_uuid(world, &owner))
        .and_then(|id| {
            ENTITIES
                .get()?
                .with_entity(world, id, |owner| name(world, owner))
        })
        .unwrap_or_default()
}

fn chatter_speak(
    world: World<'_>,
    ivy: Entity<'_>,
    pool: &str,
    viewer: Option<Entity<'_>>,
    duration: u32,
    cooldown: u64,
) {
    let Some(bytes) = RESOURCES
        .get()
        .and_then(|res| res.read(&format!("saintsdragons:ivy_chatter/{pool}.txt")))
    else {
        return;
    };
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return;
    };
    let lines: Vec<_> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if lines.is_empty() {
        return;
    }
    let key = lines[crate::ai::random(lines.len() as u64) as usize].to_owned();
    let name = chatter_name(world, ivy, viewer);
    if let Some(effects) = EFFECTS.get() {
        effects.entity_chatter(
            world,
            ivy.id(),
            &key,
            &name,
            "saintsdragons:ivy_voice_blip",
            duration,
        );
    }
    let now = moddedmc_api::tick_count();
    let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = states.entry(ivy.id()).or_default();
    state.chatter = Some(Chatter {
        key,
        name,
        until: now + u64::from(duration),
    });
    state.chatter_cooldown = Some(state.chatter_cooldown.unwrap_or(0).max(cooldown));
}

fn underwater(world: World<'_>, ivy: Entity<'_>) -> bool {
    ivy.view().is_some_and(|view| {
        let mut at = view.position;
        at[1] += 1.0;
        matches!(
            world.fluid_id(flight::cell(at)).as_deref(),
            Some("minecraft:water" | "minecraft:flowing_water")
        )
    })
}

fn chatter_tick(world: World<'_>, ivy: Entity<'_>) {
    let now = moddedmc_api::tick_count();
    let target = MOB_AI.get().and_then(|ai| ai.target(ivy));
    let downed = downed_state(ivy);
    let owner_near = taming::owner(ivy)
        .and_then(|owner| MOBS.get()?.by_uuid(world, &owner))
        .and_then(|id| {
            ENTITIES.get()?.with_entity(world, id, |owner| {
                owner.view().zip(ivy.view()).is_some_and(|(a, b)| {
                    flight::length_squared(flight::offset(a.position, b.position)) <= 100.0
                })
            })
        })
        .unwrap_or(false);
    let busy = MOBS
        .get()
        .and_then(|mobs| mobs.uuid(ivy))
        .is_some_and(|uuid| {
            SESSIONS
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .values()
                .any(|session| session.ivy == uuid && session.expires > now)
                || TRADE_RESUMES
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .values()
                    .any(|session| session.ivy == uuid)
        });
    let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = states.entry(ivy.id()).or_default();
    let changed = state.chatter_target != target;
    state.chatter_target = target;
    if let Some(chatter) = &state.chatter {
        if now >= chatter.until || downed.arise > 0 {
            state.chatter = None;
            if let Some(effects) = EFFECTS.get() {
                effects.entity_chatter(world, ivy.id(), "", "", "", 0);
            }
        } else if now.is_multiple_of(10) {
            if let Some(effects) = EFFECTS.get() {
                effects.entity_chatter(
                    world,
                    ivy.id(),
                    &chatter.key,
                    &chatter.name,
                    "saintsdragons:ivy_voice_blip",
                    (chatter.until - now) as u32,
                );
            }
        }
    }
    let mut pool = None;
    if !busy && !downed.active() && target.is_some() && changed {
        pool = Some(("combat", 0));
    } else if state.chatter.is_none() {
        let cooldown = state
            .chatter_cooldown
            .get_or_insert_with(|| 600 + crate::ai::random(801));
        if !owner_near
            || busy
            || target.is_some()
            || downed.active()
            || state.stance
            || state.recovery_start.is_some()
        {
            *cooldown = (*cooldown).max(80);
        } else if *cooldown > 0 {
            *cooldown -= 1;
        } else {
            pool = Some(("idle", 600 + crate::ai::random(801)));
        }
    }
    drop(states);
    if let Some((pool, cooldown)) = pool {
        chatter_speak(world, ivy, pool, None, 90, cooldown);
    }
}

fn block_name(world: World<'_>, pos: moddedmc_api::Pos) -> Option<String> {
    let mut states = [moddedmc_api::content::UNLOADED_STATE];
    if !world.read(&[pos], &mut states) {
        return None;
    }
    crate::CONTENT.get()?.describe_state(states[0])
}

/// IvyTheDragonMerchant.findWaterClutchPlacement: stop at the first solid shape
/// and only place a source in empty air directly above it.
fn clutch_placement(world: World<'_>, feet: moddedmc_api::Pos) -> Option<moddedmc_api::Pos> {
    for distance in 1..=5 {
        let support = moddedmc_api::Pos {
            y: feet.y - distance,
            ..feet
        };
        if world.collision_shape(support)?.is_empty() {
            continue;
        }
        let placement = moddedmc_api::Pos {
            y: support.y + 1,
            ..support
        };
        return (matches!(
            block_name(world, placement)?.as_str(),
            "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air"
        ) && world.fluid_id(placement).as_deref() == Some("minecraft:empty"))
        .then_some(placement);
    }
    None
}

/// The ambient goal executes in server_ai_step before the living entity travels.
fn water_clutch(world: World<'_>, ivy: Entity<'_>) {
    let now = moddedmc_api::tick_count();
    let (ready, clear) = {
        let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        let state = states.entry(ivy.id()).or_default();
        let clear = state.clutch_water.filter(|(_, until)| now >= *until);
        if clear.is_some() {
            state.clutch_water = None;
        }
        (state.clutch_ready, clear)
    };
    let Some(content) = crate::CONTENT.get() else {
        return;
    };
    if let Some((pos, _)) = clear
        && block_name(world, pos)
            .is_some_and(|name| name.split('[').next() == Some("minecraft:water"))
        && let Some(air) = content.state("minecraft:air")
    {
        world.write(&[moddedmc_api::content::BlockUpdate {
            position: pos,
            state: air,
            flags: 11,
        }]);
    }
    let Some(view) = ivy.view() else { return };
    if now < ready
        || view.on_ground
        || view.motion[1] > -0.55
        || ivy.flags() & ENTITY_ALIVE == 0
        || !taming::is_tamed(ivy)
        || !EFFECTS
            .get()
            .and_then(|e| e.fall_distance(ivy))
            .is_some_and(|d| d >= 5.0)
        || !has_storage_item(world, ivy, "minecraft:water_bucket")
    {
        return;
    }
    let mut feet = view.position;
    if let Some(body) = ivy.body() {
        feet[1] = body.min[1];
    }
    let feet = flight::cell(feet);
    if world.fluid_id(feet).as_deref() != Some("minecraft:empty") {
        return;
    }
    let (Some(pos), Some(water)) = (
        clutch_placement(world, feet),
        content.state("minecraft:water[level=0]"),
    ) else {
        return;
    };
    if world.write(&[moddedmc_api::content::BlockUpdate {
        position: pos,
        state: water,
        flags: 11,
    }]) != 1
    {
        return;
    }
    ivy.reset_fall_distance();
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(
            world,
            [
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ],
            "minecraft:item.bucket.empty",
            SOUND_NEUTRAL,
            0.85,
            0.95 + crate::ai::random(100) as f32 / 1000.0,
        );
    }
    let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = states.entry(ivy.id()).or_default();
    state.clutch_ready = now + 80;
    state.clutch_clip_until = now + 14;
    state.clutch_water = Some((pos, now + 22));
    state.recovery_start = None;
    state.recovery_kind = None;
}

/// IvyCombatBrain.tickRetreatRecovery: wait for safe space and landing before
/// committing to the 47-tick recovery animation.
struct Retreat {
    kind: RecoveryKind,
    ticks: u64,
    backstep_ready: u64,
    waiting_for_landing: bool,
    start: Option<u64>,
}

fn retreat_has_space(distance: f64, ticks: u64) -> bool {
    distance >= 5.25 || (ticks >= 28 && distance >= 4.0)
}

fn retreat_recovery_tick(
    world: World<'_>,
    ivy: Entity<'_>,
    state: &mut Fight,
    now: u64,
    at: [f64; 3],
    view: moddedmc_api::entities::EntityView,
    distance: f64,
) -> bool {
    let Some(ai) = MOB_AI.get() else {
        return false;
    };
    let urgent_food = MOBS
        .get()
        .and_then(|mobs| mobs.health(ivy))
        .is_some_and(|(health, _)| health > 0.0 && health < 12.0)
        && has_recovery_food(world, ivy);
    if state.retreat.is_none()
        && (urgent_food || (now >= state.retreat_ready && state.move_.is_none()))
        && let Some(kind) = recovery_kind(world, ivy, 12.0)
    {
        // Urgent food overrides an ongoing attack and the retreat cooldown.
        state.move_ = None;
        state.retreat = Some(Retreat {
            kind,
            ticks: 0,
            backstep_ready: 0,
            waiting_for_landing: false,
            start: None,
        });
        state.retreat_ready = now + 120;
    }
    let Some(retreat) = state.retreat.as_mut() else {
        return false;
    };
    ai.stop(ivy);
    if let Some(start) = retreat.start {
        let age = now.saturating_sub(start);
        if age == 42 {
            consume_recovery(world, ivy, retreat.kind);
        }
        if age >= 47 {
            state.retreat = None;
        }
        return true;
    }
    // Each dodge occupies nine ticks; do not start eating in mid-dodge.
    if now < retreat.backstep_ready.saturating_sub(1) {
        return true;
    }
    if retreat.waiting_for_landing || retreat_has_space(distance, retreat.ticks) {
        retreat.waiting_for_landing = !view.on_ground;
        if view.on_ground {
            retreat.start = Some(now);
            ivy.change(&EntityChange {
                mask: SET_MOTION,
                motion: [0.0, view.motion[1], 0.0],
                ..EntityChange::default()
            });
        }
        return true;
    }
    retreat.ticks += 1;
    if now >= retreat.backstep_ready {
        let away = flight::offset(view.position, at);
        let end = flight::add(view.position, away);
        ivy.change(&EntityChange {
            mask: SET_MOTION,
            motion: forward_impulse(view.position, end, view.motion, 1.55, 0.08),
            ..EntityChange::default()
        });
        retreat.backstep_ready = now + 10;
    }
    true
}

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

fn enemy(world: World<'_>, ivy: Entity<'_>, target: Entity<'_>) -> bool {
    crate::abilities::enemy(world, ivy, target)
        && !matches!(
            target.type_id().trim_start_matches("minecraft:"),
            "iron_golem" | "villager" | "wandering_trader"
        )
        && !(taming::owner(target).is_some() && taming::owner(ivy) == taming::owner(target))
}

fn target_position(world: World<'_>, ivy: Entity<'_>, id: i32) -> Option<[f64; 3]> {
    let from = ivy.view()?.position;
    ENTITIES.get()?.with_entity(world, id, |target| {
        let at = target.view()?.position;
        (enemy(world, ivy, target)
            && flight::length_squared(flight::offset(at, from)) <= 32.0 * 32.0)
            .then_some(at)
    })?
}

/// IvyCompanionController.OwnerDefenseGoal: react to a new attacker of Ivy or her owner.
pub fn target_tick(world: World<'_>, ivy: Entity<'_>) {
    tick_venom(world, ivy);
    water_clutch(world, ivy);
    chatter_tick(world, ivy);
    let (Some(ai), Some(mobs), Some(entities), Some(owner)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), taming::owner(ivy))
    else {
        return;
    };
    let own_hurt = mobs.last_hurt_by_mob(ivy);
    let (owner_hurt, owner_attack) = mobs
        .by_uuid(world, &owner)
        .and_then(|id| {
            entities.with_entity(world, id, |owner| {
                if owner.flags() & moddedmc_api::entities::ENTITY_ALIVE != 0 {
                    (mobs.last_hurt_by_mob(owner), mobs.last_hurt_mob(owner))
                } else {
                    (None, None)
                }
            })
        })
        .unwrap_or_default();
    let (own, defense, offense, sync_armor) = {
        let mut states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
        let state = states.entry(ivy.id()).or_default();
        let own = own_hurt.filter(|hit| state.self_hurt != Some(*hit));
        state.self_hurt = own_hurt;
        let defense = owner_hurt.filter(|hit| state.owner_synced && state.owner_hurt != Some(*hit));
        state.owner_hurt = owner_hurt;
        let offense =
            owner_attack.filter(|hit| state.owner_synced && state.owner_attack != Some(*hit));
        state.owner_attack = owner_attack;
        state.owner_synced = true;
        let sync = !state.armor_synced;
        state.armor_synced = true;
        (own, defense, offense, sync)
    };
    if sync_armor {
        update_armor(world, ivy);
    }
    if !combat_allowed(world, ivy) {
        if ai.target(ivy).is_some() {
            ai.set_target(world, ivy, None);
        }
        passive_recovery(world, ivy, false);
        return;
    }
    if ai
        .target(ivy)
        .is_some_and(|id| target_position(world, ivy, id).is_none())
    {
        ai.set_target(world, ivy, None);
    }
    let follow = taming::order_name(ivy) == "follow";
    for id in own
        .map(|hit| hit.0)
        .into_iter()
        .chain(defense.filter(|_| follow).map(|hit| hit.0))
        .chain(offense.filter(|_| follow).map(|hit| hit.0))
    {
        if target_position(world, ivy, id).is_some() {
            ai.set_target(world, ivy, Some(id));
            if own.is_some_and(|hit| hit.0 == id)
                && entities.with_entity(world, id, |attacker| attacker.as_player().is_none())
                    == Some(true)
            {
                FIGHTS
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .entry(ivy.id())
                    .or_default()
                    .projectile_punish_until = moddedmc_api::tick_count() + 35;
            }
            break;
        }
    }
    passive_recovery(world, ivy, ai.target(ivy).is_some());
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
        if !enemy(world, ivy, target)
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
                state.retreat = None;
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
        state.retreat = None;
        state.dash_read = DashRead::default();
        state.ready = now + if state.sword_damage.is_some() { 17 } else { 10 };
    }
    if retreat_recovery_tick(world, ivy, &mut state, now, at, view, distance) {
        // Recovery takes priority over attacks.
    } else if let Some(action) = state.move_ {
        ai.stop(ivy);
        let elapsed = now.saturating_sub(state.start);
        let sword_dash = action.clip == DASH[1].clip;
        if THROW.iter().any(|throw| throw.clip == action.clip) {
            if matches!(elapsed, 6 | 14) {
                throw_venom(world, ivy, id, &mut state);
            }
            if elapsed == 19 {
                ivy.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: forward_impulse(view.position, at, view.motion, 0.82, 0.03),
                    ..EntityChange::default()
                });
                state.move_ = Some(DASH[usize::from(state.sword_damage.is_some())]);
                state.start = now;
                state.ready = now + 30;
            }
        }
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
        if can_throw(world, ivy, &state, id, distance, now) {
            ai.stop(ivy);
            state.sword_damage = equipped_weapon(world, ivy);
            state.move_ = Some(THROW[usize::from(state.sword_damage.is_some())]);
            state.start = now;
            state.projectile_ready = now + 100;
            state.combo_ready = now + 30;
        } else if state.dash_read.moving >= 10 && now >= state.commit_ready {
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
    // shortcut: equipment post-hit callbacks belong to the cargo task;
    // controlled arrows currently use a vanilla carrier and discard on block impact.
    false
}

pub fn combat_clip(id: i32, moving: bool) -> Option<&'static str> {
    let states = FIGHTS.lock().unwrap_or_else(PoisonError::into_inner);
    let state = states.get(&id)?;
    if moddedmc_api::tick_count() < state.clutch_clip_until {
        return Some("main_hand_interact");
    }
    if state.recovery_start.is_some() {
        return Some(if state.recovery_kind == Some(RecoveryKind::Milk) {
            "main_hand_drink"
        } else {
            "main_hand_eat"
        });
    }
    if !state.stance {
        return None;
    }
    if let Some(retreat) = &state.retreat {
        return Some(
            match (
                state.sword_damage.is_some(),
                retreat.start.is_some(),
                retreat.kind,
            ) {
                (true, true, RecoveryKind::Food) => "sword_retreat_to_eat",
                (false, true, RecoveryKind::Food) => "orthodox_retreat_to_eat",
                (true, true, RecoveryKind::Milk) => "sword_retreat_to_drink",
                (false, true, RecoveryKind::Milk) => "orthodox_retreat_to_drink",
                (true, false, _) => "sword_dodge_backwards",
                (false, false, _) => "dodge_backwards",
            },
        );
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

/// `IvyTheDragonMerchant.tickRestocking`: refresh at the next trade interaction after a
/// resource reload or the restock deadline (the host can replace offers only when opening).
fn open_trades(world: World<'_>, viewer: Entity<'_>, ivy: Entity<'_>) -> bool {
    let Some(mobs) = MOBS.get() else {
        return false;
    };
    let now = world.game_time().unwrap_or_default();
    let Some(revision) = crate::ivy_trades::revision() else {
        return false;
    };
    let generation = revision as i64;
    let previous = ivy
        .data(TRADES_ROLLED)
        .and_then(|bytes| fastnbt::from_bytes::<Compound>(&bytes).ok());
    let current = previous.is_some_and(|state| {
        state.get("generation") == Some(&Value::Long(generation))
            && matches!(state.get("next_restock"), Some(Value::Long(next)) if *next > now)
    });
    let offers = if current {
        Vec::new()
    } else {
        crate::ivy_trades::roll().unwrap_or_default()
    };
    let opened = mobs.open_trades(ivy, viewer, &offers);
    if opened
        && !offers.is_empty()
        && let Ok(flag) = fastnbt::to_bytes(&Value::Compound(
            [
                ("generation".into(), Value::Long(generation)),
                (
                    "next_restock".into(),
                    Value::Long(now.saturating_add(crate::config::restock_interval())),
                ),
            ]
            .into(),
        ))
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
    fn refuses_only_custom_named_tags() {
        let stack = |id: &str, named: bool| {
            let mut components = Compound::new();
            if named {
                components.insert(
                    "minecraft:custom_name".into(),
                    Value::String("Not Ivy".into()),
                );
            }
            fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(1)),
                ("components".into(), Value::Compound(components)),
            ]))
            .unwrap()
        };
        assert!(named_tag(&stack("minecraft:name_tag", true)));
        assert!(!named_tag(&stack("minecraft:name_tag", false)));
        assert!(!named_tag(&stack("minecraft:paper", true)));
        assert!(!named_tag(&[]));
    }

    #[test]
    fn totem_consumes_one_in_storage_order_and_preserves_components() {
        let stack = |id: &str, count: i32| {
            fastnbt::to_bytes(&Compound::from([
                ("id".into(), Value::String(id.into())),
                ("count".into(), Value::Int(count)),
                (
                    "components".into(),
                    Value::Compound(Compound::from([(
                        "minecraft:custom_data".into(),
                        Value::Compound(Compound::new()),
                    )])),
                ),
            ]))
            .unwrap()
        };
        let mut slots = vec![None; INVENTORY_SIZE];
        slots[4] = Some(stack("minecraft:totem_of_undying", 1));
        slots[9] = Some(stack("minecraft:bread", 3));
        slots[10] = Some(stack("minecraft:totem_of_undying", 2));
        slots[11] = Some(stack("minecraft:totem_of_undying", 1));
        assert!(consume_totem(&mut slots));
        assert_eq!(
            fastnbt::from_bytes::<Compound>(slots[10].as_deref().unwrap()).unwrap(),
            fastnbt::from_bytes::<Compound>(&stack("minecraft:totem_of_undying", 1)).unwrap()
        );
        assert!(consume_totem(&mut slots));
        assert!(slots[10].is_none());
        assert!(consume_totem(&mut slots));
        assert!(slots[11].is_none());
        assert!(!consume_totem(&mut slots));
        assert!(slots[4].is_some());
        assert!(slots[9].is_some());
    }

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
    fn milk_recovery_keeps_container_remainder() {
        let mut slots = vec![None; INVENTORY_SIZE];
        slots[4] = Some(stack("minecraft:milk_bucket", 1));
        slots[9] = Some(stack("minecraft:milk_bucket", 2));
        let bucket = bucket_stack().unwrap();
        let single_milk =
            fastnbt::from_bytes::<Compound>(&stack("minecraft:milk_bucket", 1)).unwrap();
        assert_eq!(consume_milk(&mut slots, &bucket), Some(false));
        assert_eq!(
            fastnbt::from_bytes::<Compound>(slots[9].as_deref().unwrap()).unwrap(),
            single_milk
        );
        assert_eq!(slots[10], Some(bucket.clone()));
        assert_eq!(consume_milk(&mut slots, &bucket), Some(false));
        assert_eq!(slots[9], Some(bucket.clone()));
        assert_eq!(consume_milk(&mut slots, &bucket), None);
        assert_eq!(
            fastnbt::from_bytes::<Compound>(slots[4].as_deref().unwrap()).unwrap(),
            single_milk
        );
        slots[9] = Some(stack("minecraft:milk_bucket", 2));
        for slot in &mut slots[10..] {
            *slot = Some(stack("minecraft:stone", 64));
        }
        assert_eq!(consume_milk(&mut slots, &bucket), Some(true));
        assert_eq!(
            fastnbt::from_bytes::<Compound>(slots[9].as_deref().unwrap()).unwrap(),
            single_milk
        );
    }

    #[test]
    fn controlled_arrow_identifies_first_expanded_body() {
        let body = EntityBody {
            min: [2.0, 0.0, -0.1],
            max: [2.6, 2.0, 0.1],
            ..EntityBody::default()
        };
        let entry = projectile_entry([0.0, 1.0, 0.0], [4.0, 0.0, 0.0], body).unwrap();
        assert!((entry - 0.425).abs() < 1.0e-9);
        assert_eq!(
            projectile_entry([0.0, 1.0, 1.0], [4.0, 0.0, 0.0], body),
            None
        );
        assert_eq!(
            projectile_entry([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], body),
            None
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

    #[test]
    fn dialogue_names_normalize_and_survive_memory_roundtrip() {
        assert_eq!(chosen_name("  River   Stone  ", "Steve"), "River Stone");
        assert_eq!(chosen_name("  ", "Steve"), "Steve");
        assert_eq!(chosen_name(&"x".repeat(80), "Steve").len(), 50);
        let memory = Memory {
            name: "River Stone".into(),
            known: true,
            ..Memory::default()
        };
        let restored: Memory =
            serde_json::from_slice(&serde_json::to_vec(&memory).unwrap()).unwrap();
        assert_eq!(restored.name, memory.name);
        assert!(restored.known);
    }

    #[test]
    fn dialogue_translation_resolves_explicit_arguments() {
        let lang = HashMap::from([
            ("line".into(), "Oh, %1$s. I mean %2$s!".into()),
            ("aside".into(), "ahem".into()),
        ]);
        let content: Text = serde_json::from_str(
            r#"{"translate":"line","with":[{"translate":"aside"},{"text":"{name}"}]}"#,
        )
        .unwrap();
        assert_eq!(
            text(&content, &lang, "River Stone"),
            "Oh, ahem. I mean River Stone!"
        );
    }
}
