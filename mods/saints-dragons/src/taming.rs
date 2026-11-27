//! Adult Cindervane interaction, following, and rider movement.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use fastnbt::Value;
use moddedmc_api::{
    content::World,
    effects::{SOUND_NEUTRAL, SOUND_PLAYERS},
    entities::{Entity, EntityChange, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION, SET_YAW},
    interaction::{UseMob, UseResult},
    mob_ai::goal_phase,
    mobs::{PLAYER_INPUT_JUMP, PLAYER_INPUT_SPRINT},
    tick_count,
};

use crate::{EFFECTS, ENTITIES, MOB_AI, MOBS, STACKS, TAGS, ai::random, appearance};

pub const RIDE_OR_SIT: u64 = 5;
pub const FOLLOW: u64 = 6;
const FOOD_TAG: &str = "saintsdragons:foods/cindervane";
pub const HEARTY_MEAL: &str = "saintsdragons:hearty_dragon_meal";
pub const CREATIVE_MEAL: &str = "saintsdragons:creative_dragon_tame_meal";
pub const PLUCKER: &str = "saintsdragons:scale_plucker";
pub const BRUSH: &str = "saintsdragons:dragon_brush";
pub const GOLDEN_BRUSH: &str = "saintsdragons:golden_dragon_brush";
const REGROWTH_TICKS: u16 = 6000;
const BRUSH_WEAR: i32 = 256;
const BRUSH_COOLDOWN: u64 = 10;
const SADDLE: &str = "minecraft:saddle";
const FEED_COOLDOWN: u64 = 44;

#[derive(Default)]
struct InteractionTimers {
    feed: HashMap<i32, u64>,
    brush: HashMap<i32, u64>,
}

// Serializes player clicks and keeps the short interaction cooldowns off disk.
static TIMERS: Mutex<Option<InteractionTimers>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    Follow,
    Sit,
    Wander,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tamed {
    owner: [u8; 16],
    order: Order,
    saddle: bool,
    regrowth: u16,
    happiness: u8,
    plucked: bool,
}

impl Tamed {
    fn decode(bytes: &[u8]) -> Option<Self> {
        let [1, order, saddle @ (0 | 1), owner @ ..] = bytes else {
            return None;
        };
        let order = match order {
            0 => Order::Follow,
            1 => Order::Sit,
            2 => Order::Wander,
            _ => return None,
        };
        let (owner, regrowth, happiness, plucked) = match owner {
            [owner @ .., low, high, happiness, plucked @ (0 | 1)] if owner.len() == 16 => (
                owner,
                u16::from_le_bytes([*low, *high]),
                (*happiness).min(100),
                *plucked == 1,
            ),
            [owner @ .., low, high] if owner.len() == 16 => {
                (owner, u16::from_le_bytes([*low, *high]), 100, false)
            }
            owner => (owner, 0, 100, false),
        };
        Some(Self {
            owner: owner.try_into().ok()?,
            regrowth,
            happiness,
            plucked,
            order,
            saddle: *saddle == 1,
        })
    }

    fn encode(self) -> [u8; 23] {
        let mut data = [0; 23];
        data[0] = 1;
        data[1] = match self.order {
            Order::Follow => 0,
            Order::Sit => 1,
            Order::Wander => 2,
        };
        data[2] = u8::from(self.saddle);
        data[3..19].copy_from_slice(&self.owner);
        data[19..21].copy_from_slice(&self.regrowth.to_le_bytes());
        data[21] = self.happiness;
        data[22] = u8::from(self.plucked);
        data
    }
}

fn tamed(mob: Entity<'_>) -> Option<Tamed> {
    Tamed::decode(&MOBS.get()?.state(mob)?)
}

pub fn codex_stats(mob: Entity<'_>) -> Option<([u8; 16], u8)> {
    if MOBS.get()?.kind(mob) == Some(crate::ivy::KIND) {
        return None;
    }
    tamed(mob).map(|state| (state.owner, state.happiness))
}

pub fn order_name(mob: Entity<'_>) -> &'static str {
    match tamed(mob).map(|state| state.order) {
        Some(Order::Follow) => "follow",
        Some(Order::Sit) => "stay",
        _ => "wander",
    }
}

pub fn cycle_ivy_order(ivy: Entity<'_>, viewer: Entity<'_>) {
    let (Some(mut state), Some(mobs)) = (tamed(ivy), MOBS.get()) else {
        return;
    };
    state.order = match state.order {
        Order::Follow => Order::Sit,
        Order::Sit => Order::Wander,
        Order::Wander => Order::Follow,
    };
    if mobs.set_state(ivy, &state.encode()) {
        if let Some(ai) = MOB_AI.get() {
            ai.stop(ivy);
        }
        mobs.message(viewer, &format!("Ivy: {}", order_name(ivy)), true);
    }
}

pub fn inherit_owner(mob: Entity<'_>, owner: [u8; 16]) {
    if let Some(mobs) = MOBS.get() {
        mobs.set_state(
            mob,
            &Tamed {
                owner,
                order: Order::Follow,
                saddle: false,
                regrowth: 0,
                happiness: 100,
                plucked: false,
            }
            .encode(),
        );
        mobs.make_persistent(mob);
    }
}

pub fn set_follow(mob: Entity<'_>) {
    if let (Some(mobs), Some(mut state)) = (MOBS.get(), tamed(mob)) {
        state.order = Order::Follow;
        mobs.set_state(mob, &state.encode());
    }
}

pub fn owner(mob: Entity<'_>) -> Option<[u8; 16]> {
    tamed(mob).map(|state| state.owner)
}

pub fn is_tamed(mob: Entity<'_>) -> bool {
    tamed(mob).is_some()
}

pub fn is_kin(mob: Entity<'_>, other: Entity<'_>) -> bool {
    other.type_id() == crate::CINDERVANE_CARRIER.trim_start_matches("minecraft:")
        && MOBS
            .get()
            .is_some_and(|mobs| mobs.kind(other) == Some(crate::CINDERVANE))
        && tamed(mob).map(|state| state.owner) == tamed(other).map(|state| state.owner)
}

pub fn describe(bytes: &[u8]) -> &'static str {
    match Tamed::decode(bytes) {
        Some(state) if state.saddle => "tamed+saddled",
        Some(_) => "tamed",
        None => "wild",
    }
}

pub fn is_owner(mob: Entity<'_>, entity: Entity<'_>) -> bool {
    tamed(mob).is_some_and(|state| MOBS.get().and_then(|m| m.uuid(entity)) == Some(state.owner))
}

fn held_item(click: &UseMob<'_>) -> Option<String> {
    let held = click
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes::<Value>(&bytes).ok());
    match held {
        Some(Value::Compound(mut nbt)) => match nbt.remove("id") {
            Some(Value::String(id)) => Some(id),
            _ => None,
        },
        _ => None,
    }
}

fn apply_creative_meal(click: &UseMob<'_>) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(ai)) =
        (click.player, click.mob, MOBS.get(), MOB_AI.get())
    else {
        return UseResult::Pass;
    };
    if !click.creative {
        mobs.message(
            player,
            "Creative Dragon Tame Meal requires Creative mode.",
            true,
        );
        return UseResult::Fail;
    }
    let Some(owner) = mobs.uuid(player) else {
        return UseResult::Fail;
    };
    if mobs.health(mob).is_none_or(|(health, _)| health <= 0.0) {
        return UseResult::Fail;
    }
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(state) = tamed(mob) {
        let message = if state.owner == owner {
            "This dragon is already yours."
        } else {
            "This dragon already has an owner."
        };
        mobs.message(player, message, true);
        return UseResult::Consume;
    }
    let state = Tamed {
        owner,
        order: Order::Follow,
        saddle: false,
        regrowth: 0,
        happiness: 100,
        plucked: false,
    };
    if !mobs.set_state(mob, &state.encode()) {
        return UseResult::Fail;
    }
    mobs.make_persistent(mob);
    ai.stop(mob);
    ai.set_target(click.world, mob, None);
    crate::codex::remember(click.world, mob, None);
    mobs.message(player, "Dragon tamed.", true);
    UseResult::Success
}

/// Grooming loot tables in 0.9.88 each contain one guaranteed species scale.
fn scale(kind: &str) -> Option<String> {
    let species = kind.strip_prefix("saintsdragons:")?;
    matches!(
        species,
        "atroxiia"
            | "cindervane"
            | "ignivorus"
            | "raevyx"
            | "stegonaut"
            | "varasuchus"
            | "volitans"
    )
    .then(|| format!("{kind}_scale"))
}

fn brush(click: &mut UseMob<'_>, golden: bool) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(stacks)) =
        (click.player, click.mob, MOBS.get(), STACKS.get())
    else {
        return UseResult::Pass;
    };
    let Some(id) = mobs.kind(mob).and_then(scale) else {
        return UseResult::Pass;
    };
    let mut guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let timers = &mut guard.get_or_insert_default().brush;
    let Some(old) = tamed(mob) else {
        return UseResult::Pass;
    };
    let now = tick_count();
    timers.retain(|_, until| *until > now);
    if timers.contains_key(&mob.id()) {
        return UseResult::Consume;
    }
    if old.regrowth > 0 && old.happiness == 100 {
        mobs.message(player, "This dragon's scales are still growing.", true);
        return UseResult::Consume;
    }
    let Some(view) = mob.view() else {
        return UseResult::Fail;
    };
    let count = if old.regrowth == 0 {
        (if golden { 6 } else { 1 }) + random(5) as i32
    } else {
        0
    };
    let Ok(reward) = fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String(id)),
        ("count", Value::Int(count)),
    ])) else {
        return UseResult::Fail;
    };
    let Some(original) = click.stack.read() else {
        return UseResult::Fail;
    };
    let mut next = old;
    next.happiness = (old.happiness + if golden { 20 } else { 10 }).min(100);
    if count > 0 {
        next.regrowth = REGROWTH_TICKS;
        next.plucked = false;
    }
    if !mobs.set_state(mob, &next.encode()) {
        return UseResult::Fail;
    }
    if !click.creative && !wear_tool(click, &original, BRUSH_WEAR) {
        mobs.set_state(mob, &old.encode());
        return UseResult::Fail;
    }
    if count > 0
        && stacks
            .spawn(click.world, view.position, [0.0, 0.2, 0.0], &reward)
            .is_none()
    {
        if !click.creative {
            click.stack.write(&original);
        }
        mobs.set_state(mob, &old.encode());
        return UseResult::Fail;
    }
    timers.insert(mob.id(), now.saturating_add(BRUSH_COOLDOWN));
    if let Some(effects) = EFFECTS.get() {
        if next.happiness > old.happiness {
            let pitch = 1.25 + (random(1024) as f32 / 1024.0) * 0.15;
            effects.play_sound(
                click.world,
                view.position,
                "minecraft:entity.experience_orb.pickup",
                SOUND_PLAYERS,
                0.45,
                pitch,
            );
        }
        let sound = if cfg!(feature = "original-assets") {
            "saintsdragons:brushing"
        } else {
            "minecraft:item.brush.brushing.generic"
        };
        effects.play_sound(click.world, view.position, sound, SOUND_PLAYERS, 1.0, 1.0);
    }
    if count > 0 {
        mobs.message(player, &format!("Brushed off {count} scales."), true);
    } else {
        mobs.message(player, "Brushing raised the dragon's happiness.", true);
    }
    UseResult::Success
}

fn pluck(click: &mut UseMob<'_>) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(vitals)) =
        (click.player, click.mob, MOBS.get(), crate::MOB_HEALTH.get())
    else {
        return UseResult::Pass;
    };
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let old = tamed(mob);
    let health = mobs.health(mob).map_or(0.0, |(health, _)| health);
    let Some(id) = mobs.kind(mob).and_then(scale) else {
        return UseResult::Pass;
    };
    let reason = match old {
        None => "Tame the dragon before plucking its scales.",
        Some(state) if mobs.uuid(player) != Some(state.owner) => "Only the owner can pluck scales.",
        Some(state) if state.regrowth == 0 => "Brush this dragon before plucking.",
        Some(state) if state.plucked => "This dragon has already been plucked this cycle.",
        Some(_) if health <= 5.0 => "The dragon needs more health before plucking.",
        Some(_) => "",
    };
    if !reason.is_empty() {
        mobs.message(player, reason, true);
        return UseResult::Consume;
    }
    let Some(old) = old else {
        return UseResult::Fail;
    };
    let (Ok(reward), Some(original)) = (
        fastnbt::to_bytes(&HashMap::from([
            ("id", Value::String(id)),
            ("count", Value::Int(3)),
        ])),
        click.stack.read(),
    ) else {
        return UseResult::Fail;
    };
    let mut next = old;
    next.regrowth = REGROWTH_TICKS;
    next.plucked = true;
    next.happiness = old.happiness.saturating_sub(40);
    if !mobs.set_state(mob, &next.encode()) {
        return UseResult::Fail;
    }
    let wore_tool = !click.creative && wear_tool(click, &original, 64);
    if (!click.creative && !wore_tool) || !vitals.set(mob, health - 5.0) {
        if wore_tool {
            click.stack.write(&original);
        }
        mobs.set_state(mob, &old.encode());
        return UseResult::Fail;
    }
    if !click.give(&reward) {
        vitals.set(mob, health);
        if wore_tool {
            click.stack.write(&original);
        }
        mobs.set_state(mob, &old.encode());
        return UseResult::Fail;
    }
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let sound = if cfg!(feature = "original-assets") {
            "saintsdragons:plucking"
        } else {
            "minecraft:item.shears.snip"
        };
        effects.play_sound(click.world, view.position, sound, SOUND_NEUTRAL, 1.0, 1.0);
    }
    mobs.message(player, "Plucked three scales.", true);
    UseResult::Success
}

fn wear_tool(click: &mut UseMob<'_>, original: &[u8], limit: i32) -> bool {
    let Ok(Value::Compound(mut stack)) = fastnbt::from_bytes(original) else {
        return false;
    };
    let Value::Compound(components) = stack
        .entry("components".to_owned())
        .or_insert_with(|| Value::Compound(HashMap::new()))
    else {
        return false;
    };
    let wear = match components.get("minecraft:damage") {
        Some(Value::Int(wear)) => wear.saturating_add(1),
        _ => 1,
    };
    if wear >= limit {
        return click.stack.write(&[]);
    }
    components.insert("minecraft:max_damage".to_owned(), Value::Int(limit));
    components.insert("minecraft:damage".to_owned(), Value::Int(wear));
    fastnbt::to_bytes(&stack).is_ok_and(|bytes| click.stack.write(&bytes))
}

// shortcut: advance loaded regrowth in one-second steps to avoid a state write every tick.
pub fn regrow(mob: Entity<'_>) {
    if !(tick_count().wrapping_add(mob.id() as u64)).is_multiple_of(20) {
        return;
    }
    if !tamed(mob).is_some_and(|state| state.regrowth > 0) {
        return;
    }
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    if let (Some(mobs), Some(mut state)) = (MOBS.get(), tamed(mob)) {
        state.regrowth = state.regrowth.saturating_sub(20);
        if state.regrowth == 0 {
            state.plucked = false;
        }
        mobs.set_state(mob, &state.encode());
    }
}

pub fn interact(click: &mut UseMob<'_>) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(ai), Some(entities), Some(tags)) = (
        click.player,
        click.mob,
        MOBS.get(),
        MOB_AI.get(),
        ENTITIES.get(),
        TAGS.get(),
    ) else {
        return UseResult::Pass;
    };
    let Some(uuid) = mobs.uuid(player) else {
        return UseResult::Pass;
    };
    if mobs.health(mob).is_none_or(|(health, _)| health <= 0.0) {
        return UseResult::Pass;
    }
    if mobs.kind(mob) == Some(crate::ivy::KIND) {
        return crate::ivy::use_on_mob(click);
    }
    crate::advancements::award(player, "encounter_dragon", "encounter_dragon");
    let item = held_item(click).unwrap_or_default();
    if item == CREATIVE_MEAL {
        return apply_creative_meal(click);
    }
    let item = item.as_str();
    if item == crate::codex::ID {
        return crate::codex::use_on_mob(click);
    }
    if item == crate::nucleus::CONTROLLER {
        return crate::nucleus::use_on_mob(click);
    }
    if let Some(result) = crate::binders::capture(click, item) {
        return result;
    }
    if matches!(item, appearance::GENDER_WAND | appearance::VARIANT_BRUSH) {
        return appearance::use_tool(click, item);
    }
    if item == PLUCKER {
        return pluck(click);
    }
    if matches!(item, BRUSH | GOLDEN_BRUSH) {
        return brush(click, item == GOLDEN_BRUSH);
    }
    if let Some(result) = crate::breed::feed(click, item) {
        return result;
    }
    if let Some(result) = crate::wild::interact(click, item) {
        return result;
    }
    if let Some(result) = crate::stun::feed(click, item) {
        return result;
    }
    if mobs.kind(mob) != Some(crate::CINDERVANE) {
        return UseResult::Pass;
    }
    let mut lock = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let fed = &mut lock.get_or_insert_default().feed;
    let state = tamed(mob);

    if tags.contains("item", FOOD_TAG, item) {
        let now = tick_count();
        fed.retain(|_, until| *until > now);
        if fed.contains_key(&mob.id()) {
            mobs.message(player, "Cindervane is still eating.", true);
            return UseResult::Consume;
        }
        fed.insert(mob.id(), now + FEED_COOLDOWN);
        if !click.creative {
            click.stack.shrink(1);
        }
        crate::audio::eat(click.world, mob);
        let hearty = item == HEARTY_MEAL;
        if hearty {
            mob.add_effect("minecraft:regeneration", 200, 1, false, true);
        }
        if state.is_some() {
            mobs.heal(mob, if hearty { 15.0 } else { 5.0 });
            feed_particles(click.world, mob, true);
        } else {
            let chance = match item {
                HEARTY_MEAL => 500_000,
                "minecraft:chicken" => 333_333,
                _ => 250_000,
            };
            let success = random(1_000_000) < chance && tame(click.world, mob, uuid);
            feed_particles(click.world, mob, success);
        }
        return UseResult::Success;
    }

    let Some(mut state) = state.filter(|_| click.main_hand) else {
        return UseResult::Pass;
    };
    let owner = uuid == state.owner;
    if owner && item == SADDLE && !state.saddle {
        state.saddle = true;
        if !mobs.set_state(mob, &state.encode()) {
            return UseResult::Fail;
        }
        if !click.creative {
            click.stack.shrink(1);
        }
        if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
            effects.play_sound(
                click.world,
                view.position,
                "minecraft:entity.horse.saddle",
                SOUND_NEUTRAL,
                1.0,
                1.0,
            );
        }
        return UseResult::Success;
    }
    if click.sneaking {
        if !owner {
            return UseResult::Pass;
        }
        let (order, label) = match state.order {
            Order::Follow => (Order::Sit, "sitting"),
            Order::Sit => (Order::Wander, "wandering"),
            Order::Wander => (Order::Follow, "following"),
        };
        state.order = order;
        if !mobs.set_state(mob, &state.encode()) {
            return UseResult::Fail;
        }
        ai.stop(mob);
        mobs.message(player, &format!("Cindervane is {label}."), true);
        return UseResult::Success;
    }
    if !state.saddle {
        mobs.message(player, "Cindervane needs a saddle.", true);
        return UseResult::Consume;
    }

    let passengers = mobs.passengers(mob);
    let can_mount = if owner {
        passengers.is_empty()
    } else {
        let [first] = passengers.as_slice() else {
            return UseResult::Fail;
        };
        entities.with_entity(click.world, *first, |rider| mobs.uuid(rider))
            == Some(Some(state.owner))
    };
    if !can_mount || !player.start_riding(mob.id(), !owner) {
        return UseResult::Fail;
    }
    if owner {
        if state.order == Order::Sit {
            state.order = Order::Follow;
            mobs.set_state(mob, &state.encode());
        }
        ai.stop(mob);
        ai.set_target(click.world, mob, None);
    }
    UseResult::Success
}

/// A successful taming feed: owned, sitting, persistent and calm.
pub fn tame(world: World<'_>, mob: Entity<'_>, owner: [u8; 16]) -> bool {
    let (Some(mobs), Some(ai)) = (MOBS.get(), MOB_AI.get()) else {
        return false;
    };
    let state = Tamed {
        owner,
        order: Order::Sit,
        saddle: false,
        regrowth: 0,
        happiness: 100,
        plucked: false,
    };
    if !mobs.set_state(mob, &state.encode()) {
        return false;
    }
    mobs.make_persistent(mob);
    ai.stop(mob);
    ai.set_target(world, mob, None);
    crate::codex::remember(world, mob, None);
    if let Some(species) = mobs
        .kind(mob)
        .and_then(|kind| kind.strip_prefix("saintsdragons:"))
    {
        let advancement = format!("tame_{species}");
        crate::advancements::award_uuid(world, owner, &advancement, &advancement);
    }
    true
}

pub fn feed_particles(world: World<'_>, mob: Entity<'_>, happy: bool) {
    let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) else {
        return;
    };
    // The carrier may not react to the taming status packet.
    let [x, y, z] = view.position;
    let particle = if happy {
        "minecraft:heart"
    } else {
        "minecraft:smoke"
    };
    effects.particles(world, particle, [x, y + 1.5, z], [0.6, 0.5, 0.6], 0.0, 7);
}

pub fn death(world: World<'_>, mob: Entity<'_>) {
    crate::audio::death(world, mob);
    crate::nucleus::defeated(world, mob);
    crate::codex::remove(world, mob);
    crate::flight::forget(mob.id());
    crate::swarm::forget(mob.id());
    crate::stun::forget(mob.id());
    crate::breed::forget(mob.id());
    crate::wild::forget(mob.id());
    crate::visuals::death(world, mob);
    {
        let mut lock = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(timers) = lock.as_mut() {
            timers.feed.remove(&mob.id());
            timers.brush.remove(&mob.id());
        }
        let (Some(mobs), Some(mut state)) = (MOBS.get(), tamed(mob).filter(|state| state.saddle))
        else {
            return;
        };
        state.saddle = false;
        if !mobs.set_state(mob, &state.encode()) {
            return;
        }
    }
    let (Some(stacks), Some(view)) = (STACKS.get(), mob.view()) else {
        return;
    };
    if let Ok(stack) = fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String(SADDLE.to_owned())),
        ("count", Value::Int(1)),
    ])) {
        stacks.spawn(world, view.position, [0.0; 3], &stack);
    }
}

pub fn ride_or_sit(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(mobs), Some(ai), Some(entities), Some(state)) =
        (MOBS.get(), MOB_AI.get(), ENTITIES.get(), tamed(mob))
    else {
        return false;
    };
    let passengers = mobs.passengers(mob);
    let active = (state.order == Order::Sit
        && !released_in_flight(mob)
        && !crate::breed::wants_mate(world, mob))
        || passengers
            .first()
            .is_some_and(|&id| !crate::combat::holding(mob, id));
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => active,
        goal_phase::START => {
            ai.stop(mob);
            ai.set_target(world, mob, None);
            false
        }
        goal_phase::TICK => {
            if let Some(&first) = passengers.first() {
                entities.with_entity(world, first, |rider| {
                    if mobs.uuid(rider) == Some(state.owner) {
                        ride(mob, rider);
                    } else {
                        rider.stop_riding();
                    }
                });
            }
            false
        }
        _ => false,
    }
}

fn ride(mob: Entity<'_>, rider: Entity<'_>) {
    let (Some(mobs), Some(view), Some(look), Some(body)) =
        (MOBS.get(), mob.view(), rider.view(), rider.body())
    else {
        return;
    };
    let keys = mobs.player_input(rider).unwrap_or(0);
    let sprint = keys & PLAYER_INPUT_SPRINT != 0;
    let jump = keys & PLAYER_INPUT_JUMP != 0;
    let yaw = f64::from(look.yaw).to_radians();
    let strafe = body.input[0] * 0.4;
    let forward = body.input[2];
    // shortcut: stock clients use jump and look pitch for vertical control. The Java release
    // also sends custom descend, dive and flight mode packets.
    let air = jump || !view.on_ground;
    let speed = match (air, sprint) {
        (true, true) => 0.60 * 4.0,
        (true, false) => 0.60 * 3.0,
        (false, true) => 0.45,
        (false, false) => 0.18,
    };
    let desired_x = (strafe * yaw.cos() - forward * yaw.sin()) * speed;
    let desired_z = (strafe * yaw.sin() + forward * yaw.cos()) * speed;
    let vertical = if jump {
        0.45
    } else if air {
        let pitch = f64::from(look.pitch).to_radians();
        (-pitch.sin() * forward.abs() * 0.85).clamp(-0.85, 0.45) - 0.08
    } else {
        view.motion[1].min(0.0)
    };
    let horizontal = |current: f64, desired: f64| {
        if air {
            current + (desired - current) * 0.35
        } else {
            desired
        }
    };
    mob.change(&EntityChange {
        mask: SET_YAW | SET_BODY_YAW | SET_HEAD_YAW | SET_MOTION,
        yaw: look.yaw,
        body_yaw: look.yaw,
        head_yaw: look.yaw,
        motion: [
            horizontal(view.motion[0], desired_x),
            vertical,
            horizontal(view.motion[2], desired_z),
        ],
        ..EntityChange::default()
    });
    mob.reset_fall_distance();
    rider.reset_fall_distance();
}

fn released_in_flight(mob: Entity<'_>) -> bool {
    mob.view().is_some_and(|view| !view.on_ground) && crate::flight::released_active(mob.id())
}

pub fn follow(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(mobs), Some(ai), Some(entities), Some(state)) =
        (MOBS.get(), MOB_AI.get(), ENTITIES.get(), tamed(mob))
    else {
        return false;
    };
    if phase == goal_phase::STOP {
        ai.stop(mob);
        return false;
    }
    if released_in_flight(mob) {
        return false;
    }
    let Some(at) = mob.view().map(|view| view.position) else {
        return false;
    };
    let Some(owner) = mobs
        .by_uuid(world, &state.owner)
        .filter(|_| state.order == Order::Follow)
        .and_then(|id| {
            entities
                .with_entity(world, id, |e| e.view().map(|v| v.position))
                .flatten()
        })
    else {
        return false;
    };
    let distance = at
        .iter()
        .zip(owner)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>();
    match phase {
        goal_phase::CAN_START => distance > 6.0_f64.powi(2),
        goal_phase::CONTINUE => distance > 3.0_f64.powi(2),
        goal_phase::START | goal_phase::TICK => {
            if phase == goal_phase::START || tick_count().is_multiple_of(10) {
                ai.walk_to(mob, owner, 1.0);
            }
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tame_state_round_trips_and_reads_old_wild_state() {
        assert_eq!(Tamed::decode(b"wild"), None);
        assert_eq!(Tamed::decode(&[]), None);
        let mut original = [0; 19];
        original[0] = 1;
        assert_eq!(
            Tamed::decode(&original).map(|state| state.regrowth),
            Some(0)
        );
        for order in [Order::Follow, Order::Sit, Order::Wander] {
            let state = Tamed {
                owner: [0x5a; 16],
                order,
                saddle: true,
                regrowth: REGROWTH_TICKS,
                happiness: 60,
                plucked: true,
            };
            let data = state.encode();
            assert_eq!(Tamed::decode(&data), Some(state));
            assert_eq!(Tamed::decode(&data[..18]), None);
        }
    }
}
