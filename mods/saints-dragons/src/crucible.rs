use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    InterfaceUnavailable, Player, Pos, RegistrationError, Registry,
    content::{self, Block, Content, ContentEvent, EntitySnapshot, Placement, State, World},
    interaction::{BlockUseResult, PlayerWillDestroy, UseBlock},
    inventory_menu::{
        self, Access, Action, Control, Managed, ManagedChange, ManagedSlots, MenuKind, Validity,
    },
    item_input::{self, ItemInput, Offer, Take},
};
use serde::{Deserialize, Serialize};

use crate::{CONTENT, STACKS, TAGS};

mod smelting;

pub const ID: &str = "saintsdragons:draconic_crucible";
pub const NAME: &str = "Draconic Crucible";
const GRID_SIZE: usize = 9;
const SLOTS: usize = 27;
const GRID: [usize; GRID_SIZE] = [0, 1, 2, 9, 10, 11, 18, 19, 20];
const FUEL: usize = 4;
const OUTPUT: usize = 6;
const START: usize = 15;
const CLOSED: Access = Access {
    take: false,
    insert: false,
};
const OPEN: Access = Access {
    take: true,
    insert: true,
};
const CAPACITY: [i32; 4] = [0, 5, 11, 17];
const COST: [i32; 4] = [0, 1, 2, 6];
type Stack = HashMap<String, Value>;

static RECIPES: OnceLock<Vec<Recipe>> = OnceLock::new();
static BLOCK_STATES: OnceLock<[u32; 8]> = OnceLock::new();
static AUTOMATION: OnceLock<ItemInput> = OnceLock::new();
static ACTIVE: Mutex<Option<HashMap<usize, HashSet<Pos>>>> = Mutex::new(None);
static DIRTY_SCREEN: Mutex<Option<HashMap<usize, HashSet<Pos>>>> = Mutex::new(None);
static MENUS: OnceLock<Managed> = OnceLock::new();
static KIND: OnceLock<MenuKind> = OnceLock::new();

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = MENUS.set(inventory_menu::require_managed()?);
    let automation = item_input::require()?;
    automation.require_output()?;
    let _ = AUTOMATION.set(automation);
    Ok(())
}

#[derive(Deserialize)]
struct Ingredient {
    item: String,
}

#[derive(Deserialize)]
struct ResultStack {
    item: String,
    count: i32,
}

#[derive(Deserialize)]
struct Recipe {
    pattern: Option<Vec<String>>,
    #[serde(default)]
    key: HashMap<char, Ingredient>,
    ingredient: Option<Ingredient>,
    result: ResultStack,
    required_heat_level: usize,
    processing_time: i32,
}

impl Recipe {
    fn result_stack(&self) -> Stack {
        stack(&self.result.item, self.result.count)
    }

    fn matches_grid(&self, grid: &[Stack; GRID_SIZE]) -> bool {
        let Some(pattern) = &self.pattern else {
            return false;
        };
        let width = pattern[0].len();
        let height = pattern.len();
        for top in 0..=3 - height {
            for left in 0..=3 - width {
                for mirrored in [false, true] {
                    let matches = grid.iter().enumerate().all(|(slot, stack)| {
                        let row = slot / 3;
                        let col = slot % 3;
                        let expected = if (top..top + height).contains(&row)
                            && (left..left + width).contains(&col)
                        {
                            let x = if mirrored {
                                width - 1 - (col - left)
                            } else {
                                col - left
                            };
                            let code = pattern[row - top].as_bytes()[x] as char;
                            self.key
                                .get(&code)
                                .map(|ingredient| ingredient.item.as_str())
                        } else {
                            None
                        };
                        item_id(stack) == expected
                    });
                    if matches {
                        return true;
                    }
                }
            }
        }
        false
    }
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(default)]
struct Crucible {
    input: [Stack; GRID_SIZE],
    fuel: Stack,
    output: Stack,
    charge: i32,
    job: Option<Job>,
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
struct Job {
    result: Stack,
    minimum_charge: i32,
    cost: i32,
    ticks: i32,
    elapsed: i32,
}

impl Crucible {
    /// The released container is unsided: output, fuel, then the nine input slots.
    /// canPlaceItem refuses every automated insertion while processing.
    fn insert_one(&mut self, offered: &Stack, limit: i32, is_fuel: bool) -> bool {
        if self.job.is_some() || count(offered) != 1 || limit < 1 {
            return false;
        }
        for slot in std::iter::once(&mut self.fuel)
            .filter(|_| is_fuel)
            .chain(&mut self.input)
        {
            if slot.is_empty() {
                *slot = offered.clone();
                return true;
            }
            if same_item(slot, offered) && count(slot) < limit {
                slot.insert("count".into(), Value::Int(count(slot) + 1));
                return true;
            }
        }
        false
    }

    fn automation_slot(&mut self, slot: u32) -> Option<&mut Stack> {
        match slot {
            0 => Some(&mut self.output),
            1 => Some(&mut self.fuel),
            2..=10 => self.input.get_mut(slot as usize - 2),
            _ => None,
        }
    }

    fn access(&self) -> [Access; SLOTS] {
        let mut access = [CLOSED; SLOTS];
        for slot in GRID {
            access[slot] = if self.job.is_none() { OPEN } else { CLOSED };
        }
        access[FUEL] = OPEN;
        access[OUTPUT] = Access {
            take: true,
            insert: false,
        };
        access
    }

    fn slots(&self) -> Option<Vec<Option<Vec<u8>>>> {
        let mut slots = vec![None; SLOTS];
        for (slot, item) in GRID
            .into_iter()
            .zip(&self.input)
            .chain([(FUEL, &self.fuel), (OUTPUT, &self.output)])
        {
            if !item.is_empty() {
                slots[slot] = Some(fastnbt::to_bytes(item).ok()?);
            }
        }
        let marker = if self.job.is_some() {
            "minecraft:clock"
        } else {
            "minecraft:lever"
        };
        let mut marker = stack(marker, 1);
        marker.insert(
            "components".into(),
            Value::Compound(HashMap::from([(
                "minecraft:custom_data".into(),
                Value::Compound(HashMap::from([(
                    "moddedmc:screen_state".into(),
                    Value::Compound(self.screen_state()),
                )])),
            )])),
        );
        slots[START] = Some(fastnbt::to_bytes(&marker).ok()?);
        Some(slots)
    }

    /// The released DraconicCrucibleScreen fills each strip from its bottom edge.
    fn screen_state(&self) -> Stack {
        let progress = |height: i32| {
            self.job.as_ref().map_or(0, |job| {
                if job.ticks <= 0 {
                    0
                } else {
                    ((i64::from(height) * i64::from(job.elapsed) + i64::from(job.ticks) - 1)
                        / i64::from(job.ticks))
                    .clamp(0, i64::from(height)) as i32
                }
            })
        };
        let can_start = self.job.is_none()
            && self
                .clone()
                .start(recipes(), max_count, smelting::find, fuel_level);
        HashMap::from([
            (
                "processing".into(),
                Value::Int(i32::from(self.job.is_some())),
            ),
            ("can_start".into(), Value::Int(i32::from(can_start))),
            ("gauge".into(), Value::Int(gauge_height(self.charge))),
            ("arrow".into(), Value::Int(progress(9))),
            ("bar".into(), Value::Int(progress(40))),
        ])
    }

    fn store_fuel(&mut self, fuel_level: impl Fn(&str) -> Option<usize>) {
        if self.charge == 0
            && let Some(level) = item_id(&self.fuel).and_then(fuel_level)
        {
            self.charge = CAPACITY[level];
            decrement(&mut self.fuel);
        }
    }

    fn start(
        &mut self,
        recipes: &[Recipe],
        max_count: impl Fn(&Stack) -> i32,
        vanilla: impl Fn(&str) -> Option<(Stack, i32)>,
        fuel_level: impl Fn(&str) -> Option<usize>,
    ) -> bool {
        if self.job.is_some() {
            return false;
        }
        self.store_fuel(&fuel_level);

        // Released shaped recipes have equal priority and are stored in recipe-id order.
        let shaped = recipes
            .iter()
            .find(|recipe| recipe.matches_grid(&self.input));
        let matched = shaped
            .map(|recipe| {
                (
                    recipe.result_stack(),
                    recipe.required_heat_level,
                    recipe.processing_time,
                    None,
                )
            })
            .or_else(|| {
                self.input.iter().enumerate().find_map(|(slot, stack)| {
                    let id = item_id(stack)?;
                    if let Some(recipe) = recipes.iter().find(|recipe| {
                        recipe
                            .ingredient
                            .as_ref()
                            .is_some_and(|input| input.item == id)
                    }) {
                        return Some((
                            recipe.result_stack(),
                            recipe.required_heat_level,
                            recipe.processing_time,
                            Some(slot),
                        ));
                    }
                    vanilla(id).map(|(result, ticks)| (result, 1, ticks, Some(slot)))
                })
            });
        let Some((result, level, ticks, slot)) = matched else {
            return false;
        };
        let total = count(&result) + count(&self.output);
        if (!self.output.is_empty() && !same_item(&self.output, &result))
            || total > max_count(&result)
        {
            return false;
        }

        let minimum_charge = CAPACITY[level - 1];
        let cost = COST[level];
        if self.charge < minimum_charge + cost {
            let Some(fuel) = item_id(&self.fuel).and_then(fuel_level) else {
                return false;
            };
            if CAPACITY[fuel] < minimum_charge + cost {
                return false;
            }
            self.charge = CAPACITY[fuel];
            decrement(&mut self.fuel);
        }
        if let Some(slot) = slot {
            decrement(&mut self.input[slot]);
        } else {
            for stack in &mut self.input {
                decrement(stack);
            }
        }
        self.job = Some(Job {
            result,
            minimum_charge,
            cost,
            ticks,
            elapsed: 0,
        });
        true
    }

    fn advance(
        &mut self,
        recipes: &[Recipe],
        max_count: impl Fn(&Stack) -> i32,
        vanilla: impl Fn(&str) -> Option<(Stack, i32)>,
        fuel_level: impl Fn(&str) -> Option<usize>,
    ) {
        self.store_fuel(&fuel_level);
        let Some(job) = &mut self.job else {
            return;
        };
        let next = job.elapsed + 1;
        let spent = job.cost * next / job.ticks - job.cost * job.elapsed / job.ticks;
        if self.charge <= job.minimum_charge || self.charge - spent < job.minimum_charge {
            return;
        }

        self.charge -= spent;
        job.elapsed = next;
        if next == job.ticks {
            let Some(job) = self.job.take() else {
                return;
            };
            let total = count(&job.result) + count(&self.output);
            self.output = job.result;
            self.output.insert("count".into(), Value::Int(total));
            self.start(recipes, max_count, vanilla, fuel_level);
        }
    }
}

fn stack(id: &str, count: i32) -> Stack {
    HashMap::from([
        ("id".into(), Value::String(id.into())),
        ("count".into(), Value::Int(count)),
    ])
}

fn gauge_height(charge: i32) -> i32 {
    let heights = [0, 21, 42, 56];
    let charge = charge.clamp(0, CAPACITY[3]);
    let tier = (1..=3).find(|&tier| charge <= CAPACITY[tier]).unwrap_or(3);
    let range = CAPACITY[tier] - CAPACITY[tier - 1];
    let progress = charge - CAPACITY[tier - 1];
    heights[tier - 1] + ((heights[tier] - heights[tier - 1]) * progress + range - 1) / range
}

fn item_id(stack: &Stack) -> Option<&str> {
    match stack.get("id") {
        Some(Value::String(id)) => Some(id),
        _ => None,
    }
}

fn count(stack: &Stack) -> i32 {
    match stack.get("count") {
        Some(Value::Int(count)) => *count,
        _ => 0,
    }
}

fn decrement(stack: &mut Stack) {
    let count = count(stack);
    if count <= 1 {
        stack.clear();
    } else {
        stack.insert("count".into(), Value::Int(count - 1));
    }
}

fn same_item(left: &Stack, right: &Stack) -> bool {
    fn components(stack: &Stack) -> Option<&Value> {
        stack
            .get("components")
            .filter(|value| !matches!(value, Value::Compound(fields) if fields.is_empty()))
    }
    item_id(left) == item_id(right) && components(left) == components(right)
}

fn max_count(stack: &Stack) -> i32 {
    fastnbt::to_bytes(stack)
        .ok()
        .and_then(|bytes| STACKS.get()?.info(&bytes))
        .map_or(0, |info| info.max_count as i32)
}

fn fuel_level(id: &str) -> Option<usize> {
    let tags = TAGS.get()?;
    [
        (3, "saintsdragons:draconic_crucible/fuels/level_3"),
        (2, "saintsdragons:draconic_crucible/fuels/level_2"),
        (1, "saintsdragons:draconic_crucible/fuels/level_1"),
    ]
    .into_iter()
    .find_map(|(level, tag)| tags.contains("item", tag, id).then_some(level))
}

const FACINGS: [&str; 4] = ["north", "east", "south", "west"];

fn states() -> Option<&'static [u32; 8]> {
    if let Some(states) = BLOCK_STATES.get() {
        return Some(states);
    }
    let content = CONTENT.get()?;
    let mut states = [0; 8];
    for (index, facing) in FACINGS.iter().enumerate() {
        for lit in [false, true] {
            states[index + usize::from(lit) * 4] =
                content.state(&format!("{ID}[facing={facing},lit={lit}]"))?;
        }
    }
    Some(BLOCK_STATES.get_or_init(|| states))
}

fn is_crucible(state: u32) -> bool {
    states().is_some_and(|states| states.contains(&state))
}

/// DraconicCrucibleBlock.getStateForPlacement faces opposite the player's yaw.
fn facing_index(yaw: f32) -> usize {
    (yaw / 90.0 + 0.5).floor() as i32 as usize & 3
}

fn place(args: Placement<'_>) -> Option<u32> {
    Some(states()?[facing_index(args.yaw)])
}

fn recipes() -> &'static [Recipe] {
    RECIPES.get().map_or(&[], Vec::as_slice)
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let menus = MENUS.get().ok_or(RegistrationError::InvalidDefinition)?;
    let kind = menus.register(registry, changed, action, valid, accepts, route)?;
    let _ = KIND.set(kind);
    let recipes = serde_json::from_str(include_str!("crucible_recipes.json"))
        .map_err(|_| RegistrationError::InvalidDefinition)?;
    let _ = RECIPES.set(recipes);
    let model = if cfg!(feature = "original-assets") {
        "saintsdragons:block/draconic_crucible"
    } else {
        "minecraft:block/blast_furnace"
    };
    let properties = std::array::from_fn::<_, 8, _>(|index| {
        format!("facing={},lit={}", FACINGS[index % 4], index >= 4)
    });
    let states = std::array::from_fn::<_, 8, _>(|index| State {
        properties: &properties[index],
        physical: if index >= 4 {
            "minecraft:blast_furnace[lit=true]"
        } else {
            "minecraft:blast_furnace[lit=false]"
        },
        model,
        rotating_model: "",
        x: 0,
        y: [0, 90, 180, 270][index % 4],
        orientation: [0.0, 0.0, 0.0, 1.0],
        shape: &[],
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: NAME,
            entity: ID,
            item_model: if cfg!(feature = "original-assets") {
                "saintsdragons:item/draconic_crucible"
            } else {
                "minecraft:item/blast_furnace"
            },
            states: &states,
        },
        Some(place),
    )?;
    let automation = AUTOMATION
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?;
    automation.register(registry, &[ID], receive)?;
    automation.register_output(registry, ID, 11, extract)
}

fn snapshot(world: World<'_>, pos: Pos) -> Option<(EntitySnapshot, Crucible)> {
    let snapshot = world.entity_snapshot(pos)?;
    if !is_crucible(snapshot.state) {
        return None;
    }
    let machine = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    Some((snapshot, machine))
}

fn replacement(snapshot: &EntitySnapshot, machine: &Crucible) -> Option<Vec<u8>> {
    let mut fields: Stack = fastnbt::from_bytes(&fastnbt::to_bytes(machine).ok()?).ok()?;
    let original: Stack = fastnbt::from_bytes(&snapshot.nbt).ok()?;
    for key in ["id", "x", "y", "z"] {
        fields.insert(key.into(), original.get(key)?.clone());
    }
    fastnbt::to_bytes(&fields).ok()
}

fn mutate(world: World<'_>, pos: Pos, snapshot: &EntitySnapshot, machine: &Crucible) -> bool {
    let (Some(menus), Some(kind), Some(slots), Some(bytes)) = (
        MENUS.get(),
        KIND.get(),
        machine.slots(),
        replacement(snapshot, machine),
    ) else {
        return false;
    };
    menus.mutate(
        world,
        *kind,
        snapshot.incarnation,
        &machine.access(),
        &slots,
        || world.compare_entity(pos, snapshot, &bytes),
    )
}

fn receive(offer: Offer<'_>) -> u32 {
    let Some(offered) = decode_stack(Some(offer.stack)) else {
        return 0;
    };
    let Some((before, mut machine)) = snapshot(offer.world, offer.position) else {
        return 0;
    };
    if before.state != offer.state
        || !machine.insert_one(
            &offered,
            max_count(&offered),
            item_id(&offered).and_then(fuel_level).is_some(),
        )
    {
        return 0;
    }
    if offer.simulate {
        return 1;
    }
    let committed = mutate(offer.world, offer.position, &before, &machine);
    if committed {
        mark_screen_dirty(offer.world, offer.position);
    }
    u32::from(committed)
}

fn extract(take: Take<'_>) -> Option<Vec<u8>> {
    let (before, mut machine) = snapshot(take.world, take.position)?;
    if before.state != take.state {
        return None;
    }
    let slot = machine.automation_slot(take.slot)?;
    if count(slot) <= 0 {
        return None;
    }
    let mut one = slot.clone();
    one.insert("count".into(), Value::Int(1));
    let bytes = fastnbt::to_bytes(&one).ok()?;
    if let Some(expected) = take.expected {
        let expected = decode_stack(Some(expected))?;
        if count(&expected) != 1 || !same_item(&expected, &one) {
            return None;
        }
        decrement(slot);
        if !mutate(take.world, take.position, &before, &machine) {
            return None;
        }
        mark_screen_dirty(take.world, take.position);
    }
    Some(bytes)
}

fn remember(world: World<'_>, pos: Pos) {
    ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .entry(world.id())
        .or_default()
        .insert(pos);
}

pub fn tick(world: World<'_>, events: &[ContentEvent]) {
    if states().is_none() {
        return;
    }
    let positions = {
        let mut worlds = ACTIVE.lock().unwrap_or_else(PoisonError::into_inner);
        if events.is_empty() && worlds.is_none() {
            return;
        }
        let positions = worlds
            .get_or_insert_with(HashMap::new)
            .entry(world.id())
            .or_default();
        for event in events {
            if is_crucible(event.previous)
                && (!is_crucible(event.state) || event.reason == content::UNLOADED)
            {
                positions.remove(&event.position);
            } else if is_crucible(event.state) {
                positions.insert(event.position);
            }
        }
        positions.clone()
    };
    let dirty = DIRTY_SCREEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_mut()
        .and_then(|worlds| worlds.remove(&world.id()))
        .unwrap_or_default();
    for pos in positions {
        let Some((before, mut machine)) = snapshot(world, pos) else {
            continue;
        };
        if !dirty.contains(&pos)
            && machine.job.is_none()
            && (machine.charge != 0 || machine.fuel.is_empty())
        {
            sync_lit(world, pos, before.state, false);
            continue;
        }
        machine.advance(recipes(), max_count, smelting::find, fuel_level);
        if mutate(world, pos, &before, &machine) {
            sync_lit(world, pos, before.state, machine.job.is_some());
        } else if dirty.contains(&pos) {
            mark_screen_dirty(world, pos);
        }
    }
}

/// Switch only the facing-preserving property; Pumpkin keeps the block entity
/// and open menu incarnation when the block type does not change.
fn sync_lit(world: World<'_>, pos: Pos, previous: u32, lit: bool) -> bool {
    let Some(states) = states() else { return false };
    let Some(index) = states.iter().position(|&state| state == previous) else {
        return false;
    };
    let next = states[index % 4 + usize::from(lit) * 4];
    if next == previous || !world.compare_state(pos, previous, next) {
        return false;
    }
    if let Some(effects) = crate::EFFECTS.get() {
        let sound = if cfg!(feature = "original-assets") {
            if lit {
                "saintsdragons:draconic_crucible_close"
            } else {
                "saintsdragons:draconic_crucible_open"
            }
        } else if lit {
            "minecraft:block.iron_trapdoor.close"
        } else {
            "minecraft:block.iron_trapdoor.open"
        };
        effects.play_sound(
            world,
            [
                f64::from(pos.x) + 0.5,
                f64::from(pos.y) + 0.5,
                f64::from(pos.z) + 0.5,
            ],
            sound,
            moddedmc_api::effects::SOUND_BLOCKS,
            0.5,
            0.9 + crate::ai::random(1000) as f32 * 0.0001,
        );
    }
    true
}

fn mark_screen_dirty(world: World<'_>, pos: Pos) {
    DIRTY_SCREEN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .entry(world.id())
        .or_default()
        .insert(pos);
}

fn position_context(pos: Pos) -> [u8; 12] {
    let mut bytes = [0; 12];
    for (index, coordinate) in [pos.x, pos.y, pos.z].into_iter().enumerate() {
        bytes[index * 4..index * 4 + 4].copy_from_slice(&coordinate.to_le_bytes());
    }
    bytes
}

fn decode_pos(bytes: &[u8]) -> Option<Pos> {
    if bytes.len() != 12 {
        return None;
    }
    Some(Pos {
        x: i32::from_le_bytes(bytes[0..4].try_into().ok()?),
        y: i32::from_le_bytes(bytes[4..8].try_into().ok()?),
        z: i32::from_le_bytes(bytes[8..12].try_into().ok()?),
    })
}

fn reachable(player: &Player<'_>, pos: Pos) -> bool {
    let player = player.position();
    let dx = player.x - f64::from(pos.x) - 0.5;
    let dy = player.y - f64::from(pos.y) - 0.5;
    let dz = player.z - f64::from(pos.z) - 0.5;
    dx * dx + dy * dy + dz * dz <= 64.0
}

fn valid(view: Validity<'_>) -> bool {
    decode_pos(view.context).is_some_and(|pos| {
        reachable(&view.player, pos)
            && snapshot(view.world, pos)
                .is_some_and(|(snapshot, _)| snapshot.incarnation == view.inventory)
    })
}

fn decode_stack(bytes: Option<&[u8]>) -> Option<Stack> {
    bytes.map_or_else(
        || Some(Stack::new()),
        |bytes| fastnbt::from_bytes(bytes).ok(),
    )
}

fn accepts(_: &[u8], slot: u8, bytes: &[u8]) -> bool {
    let slot = usize::from(slot);
    if slot == FUEL {
        decode_stack(Some(bytes))
            .as_ref()
            .and_then(item_id)
            .and_then(fuel_level)
            .is_some()
    } else {
        GRID.contains(&slot)
    }
}

fn route(_: &[u8], bytes: &[u8]) -> u64 {
    if accepts(&[], FUEL as u8, bytes) {
        1 << FUEL
    } else {
        GRID.into_iter().fold(
            moddedmc_api::abi::MENU_ROUTE_PLAYER_FALLBACK,
            |mask, slot| mask | (1 << slot),
        )
    }
}

fn changed(mut event: ManagedChange<'_>) {
    let Some(pos) = decode_pos(event.context) else {
        return;
    };
    let world = event.change.world;
    let Some((before, mut machine)) = snapshot(world, pos) else {
        return;
    };
    if before.incarnation != event.change.inventory {
        return;
    }
    let Some(proposed) = event
        .change
        .slots()
        .map(decode_stack)
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    if proposed.len() != SLOTS {
        return;
    }
    let input = GRID.map(|slot| proposed[slot].clone());
    let fuel = &proposed[FUEL];
    let output = &proposed[OUTPUT];
    if (machine.job.is_some()
        && input
            .iter()
            .zip(&machine.input)
            .any(|(left, right)| !same_item(left, right) || count(left) != count(right)))
        || (!fuel.is_empty() && item_id(fuel).and_then(fuel_level).is_none())
        || (!output.is_empty()
            && (!same_item(output, &machine.output) || count(output) > count(&machine.output)))
    {
        return;
    }
    machine.input = input;
    machine.fuel = fuel.clone();
    machine.output = output.clone();
    let Some(bytes) = replacement(&before, &machine) else {
        return;
    };
    if event.commit_external(|| world.compare_entity(pos, &before, &bytes)) {
        mark_screen_dirty(world, pos);
    }
}

fn action(event: Action<'_>) {
    let positions = ACTIVE
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .and_then(|worlds| worlds.get(&event.world.id()))
        .cloned();
    let Some(positions) = positions else { return };
    for pos in positions {
        if !reachable(&event.player, pos) {
            continue;
        }
        let Some((before, mut machine)) = snapshot(event.world, pos) else {
            continue;
        };
        if before.incarnation != event.inventory {
            continue;
        }
        if machine.job.is_some() {
            return;
        }
        let charge = machine.charge;
        let started = machine.start(recipes(), max_count, smelting::find, fuel_level);
        if (started || machine.charge != charge) && mutate(event.world, pos, &before, &machine) {
            sync_lit(event.world, pos, before.state, machine.job.is_some());
        }
        return;
    }
}

pub fn use_block(context: &mut UseBlock<'_>) -> BlockUseResult {
    if !is_crucible(context.state) || !context.main_hand {
        return BlockUseResult::Pass;
    }
    let (Some(player), Some(menus), Some(kind), Some((before, machine))) = (
        context.player.and_then(|entity| entity.as_player()),
        MENUS.get(),
        KIND.get(),
        snapshot(context.world, context.position),
    ) else {
        return BlockUseResult::Fail;
    };
    remember(context.world, context.position);
    let pos = context.position;
    let world = context.world;
    let opened = menus.open(
        &player,
        *kind,
        before.incarnation,
        "moddedmc:screen/saintsdragons:draconic_crucible",
        ManagedSlots {
            world,
            access: &machine.access(),
            controls: &[Control {
                slot: START as u8,
                tag: 1,
            }],
            context: &position_context(pos),
        },
        || {
            let (latest, current) = snapshot(world, pos)?;
            (latest.incarnation == before.incarnation && current == machine)
                .then(|| current.slots())
                .flatten()
        },
    );
    if opened {
        BlockUseResult::Success
    } else {
        BlockUseResult::Fail
    }
}

pub fn break_block(context: &PlayerWillDestroy<'_>) {
    if !is_crucible(context.state) {
        return;
    }
    let Some(stacks) = STACKS.get() else { return };
    let pos = context.position;
    let at = [
        f64::from(pos.x) + 0.5,
        f64::from(pos.y) + 0.5,
        f64::from(pos.z) + 0.5,
    ];
    for _ in 0..4 {
        let Some((before, machine)) = snapshot(context.world, pos) else {
            return;
        };
        if !mutate(context.world, pos, &before, &Crucible::default()) {
            continue;
        }
        for stack in machine
            .input
            .into_iter()
            .chain([machine.fuel, machine.output])
        {
            if !stack.is_empty()
                && let Ok(bytes) = fastnbt::to_bytes(&stack)
            {
                stacks.spawn(context.world, at, [0.0; 3], &bytes);
            }
        }
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn released_recipes() -> Vec<Recipe> {
        serde_json::from_str(include_str!("crucible_recipes.json")).unwrap()
    }

    #[test]
    fn automation_follows_released_slot_order_and_processing_lock() {
        let mut machine = Crucible::default();
        let fuel = stack("minecraft:blaze_powder", 1);
        let input = stack("minecraft:raw_iron", 1);
        assert!(machine.insert_one(&fuel, 64, true));
        assert!(machine.insert_one(&input, 1, false));
        assert!(machine.insert_one(&input, 1, false));
        assert_eq!(machine.fuel, fuel);
        assert_eq!(machine.input[0], input);
        assert_eq!(machine.input[1], input);
        machine.output = stack("minecraft:iron_ingot", 2);
        assert_eq!(count(machine.automation_slot(0).unwrap()), 2);
        assert_eq!(
            item_id(machine.automation_slot(1).unwrap()),
            Some("minecraft:blaze_powder")
        );
        assert_eq!(
            item_id(machine.automation_slot(2).unwrap()),
            Some("minecraft:raw_iron")
        );
        assert!(machine.automation_slot(11).is_none());
        machine.job = Some(Job {
            result: stack("minecraft:iron_ingot", 1),
            minimum_charge: 1,
            cost: 1,
            ticks: 200,
            elapsed: 0,
        });
        assert!(!machine.insert_one(&fuel, 64, true));
        assert!(!machine.insert_one(&input, 64, false));
    }

    #[test]
    fn placement_faces_the_player() {
        for (yaw, expected) in [
            (0.0, "north"),
            (90.0, "east"),
            (180.0, "south"),
            (-90.0, "west"),
            (360.0, "north"),
        ] {
            assert_eq!(FACINGS[facing_index(yaw)], expected);
        }
    }

    #[test]
    fn menu_projects_consumption_and_lock_in_the_same_state() {
        let mut machine = Crucible {
            charge: 5,
            ..Default::default()
        };
        machine.input[0] = stack("minecraft:raw_iron", 2);
        assert!(machine.access()[GRID[0]].insert);
        assert!(machine.start(
            &[],
            |_| 64,
            |_| Some((stack("minecraft:iron_ingot", 1), 2)),
            |_| None
        ));
        let access = machine.access();
        for slot in GRID {
            assert!(!access[slot].take && !access[slot].insert);
        }
        assert!(access[FUEL].insert && access[FUEL].take);
        assert!(access[OUTPUT].take && !access[OUTPUT].insert);
        assert!(!access[START].take && !access[START].insert);
        let slots = machine.slots().unwrap();
        assert_eq!(count(&decode_stack(slots[GRID[0]].as_deref()).unwrap()), 1);
        assert_eq!(
            item_id(&decode_stack(slots[START].as_deref()).unwrap()),
            Some("minecraft:clock")
        );
        let state = machine.screen_state();
        assert_eq!(state["processing"], Value::Int(1));
        assert_eq!(state["can_start"], Value::Int(0));
        assert_eq!(state["gauge"], Value::Int(21));
        assert_eq!(state["arrow"], Value::Int(0));
        machine.job.as_mut().unwrap().elapsed = 1;
        let state = machine.screen_state();
        assert_eq!(state["arrow"], Value::Int(5));
        assert_eq!(state["bar"], Value::Int(20));
        let pos = Pos {
            x: -320,
            y: -64,
            z: 500,
        };
        assert!(decode_pos(&position_context(pos)) == Some(pos));
        let bare = stack("minecraft:iron_ingot", 1);
        let mut host_encoded = bare.clone();
        host_encoded.insert("components".into(), Value::Compound(HashMap::new()));
        assert!(same_item(&bare, &host_encoded));
    }

    #[test]
    fn screen_gauge_matches_released_tier_segments() {
        assert_eq!(gauge_height(-1), 0);
        assert_eq!(gauge_height(1), 5);
        assert_eq!(gauge_height(5), 21);
        assert_eq!(gauge_height(6), 25);
        assert_eq!(gauge_height(11), 42);
        assert_eq!(gauge_height(12), 45);
        assert_eq!(gauge_height(17), 56);
        assert_eq!(gauge_height(99), 56);
    }

    #[test]
    fn released_shaped_recipes_match_mirrored_and_translated() {
        let recipes = released_recipes();
        assert_eq!(recipes.len(), 14);
        for recipe in &recipes {
            let Some(pattern) = &recipe.pattern else {
                continue;
            };
            let top = 3 - pattern.len();
            let left = 3 - pattern[0].len();
            let mut grid: [Stack; GRID_SIZE] = Default::default();
            for (row, line) in pattern.iter().enumerate() {
                for (col, code) in line.chars().rev().enumerate() {
                    if let Some(ingredient) = recipe.key.get(&code) {
                        grid[left + col + (top + row) * 3] = stack(&ingredient.item, 1);
                    }
                }
            }
            assert!(recipe.matches_grid(&grid), "{}", recipe.result.item);
            let empty = grid.iter_mut().find(|stack| stack.is_empty());
            if let Some(empty) = empty {
                *empty = stack("minecraft:dirt", 1);
                assert!(!recipe.matches_grid(&grid), "{}", recipe.result.item);
            }
        }
    }

    #[test]
    fn crucible_uses_the_released_charge_threshold_and_reserves_the_result() {
        let recipes = released_recipes();
        let mut machine = Crucible {
            charge: 11,
            ..Default::default()
        };
        for (slot, item) in [
            (1, "saintsdragons:dragonheart_chunk"),
            (2, "saintsdragons:dragonheart_chunk"),
            (4, "saintsdragons:dragonheart_chunk"),
            (5, "saintsdragons:dragonheart_chunk"),
            (7, "saintsdragons:worldroot_ingot"),
            (8, "saintsdragons:worldroot_ingot"),
        ] {
            machine.input[slot] = stack(item, 2);
        }
        assert!(!machine.start(
            &recipes,
            |_| 64,
            |_| None,
            |id| (id == "minecraft:dragon_breath").then_some(3)
        ));
        assert_eq!(count(&machine.input[1]), 2);
        machine.fuel = stack("minecraft:dragon_breath", 1);
        assert!(machine.start(
            &recipes,
            |_| 64,
            |_| None,
            |id| (id == "minecraft:dragon_breath").then_some(3)
        ));
        assert_eq!(machine.charge, 17);
        assert!(machine.fuel.is_empty());
        assert_eq!(count(&machine.input[1]), 1);
        for _ in 0..1799 {
            machine.advance(
                &recipes,
                |_| 64,
                |_| None,
                |id| (id == "minecraft:dragon_breath").then_some(3),
            );
        }
        assert_eq!(machine.charge, 12);
        assert!(machine.output.is_empty());
        let bytes = fastnbt::to_bytes(&machine).unwrap();
        let mut machine: Crucible = fastnbt::from_bytes(&bytes).unwrap();
        machine.advance(
            &recipes,
            |_| 64,
            |_| None,
            |id| (id == "minecraft:dragon_breath").then_some(3),
        );
        assert_eq!(machine.charge, 11);
        assert_eq!(
            item_id(&machine.output),
            Some("saintsdragons:dragonheart_alloy")
        );
        assert_eq!(count(&machine.output), 2);
        assert!(machine.job.is_none());
        assert_eq!(count(&machine.input[1]), 1);
    }
}
