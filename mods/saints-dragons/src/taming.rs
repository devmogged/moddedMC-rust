//! Dragon taming, mounting, following, and rider movement.

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
const CHEST: &str = "minecraft:chest";
const FEED_COOLDOWN: u64 = 44;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MountProfile {
    saddle: bool,
    seats: usize,
    flying: bool,
}

/// The released DragonSaddleCarrier implementations and player passenger limits.
fn mount_profile(kind: &str) -> Option<MountProfile> {
    let (saddle, seats, flying) = match kind {
        "saintsdragons:cindervane" => (true, 2, true),
        "saintsdragons:atroxiia" | "saintsdragons:stegonaut" => (true, 1, false),
        "saintsdragons:varasuchus" => (false, 1, false),
        "saintsdragons:ignivorus"
        | "saintsdragons:raevyx"
        | "saintsdragons:volitans"
        | "saintsdragons:nulljaw" => (false, 1, true),
        _ => return None,
    };
    Some(MountProfile {
        saddle,
        seats,
        flying,
    })
}

impl MountProfile {
    fn owner_can_mount(self, state: Tamed, juvenile: bool) -> bool {
        !juvenile && (!self.saddle || state.saddle) && state.happiness > 30
    }

    fn guest_seat(self, passengers: usize, owner_in_first_seat: bool) -> Result<(), &'static str> {
        if passengers >= self.seats {
            return Err("Sorry, but the seats are full.");
        }
        if self.seats == 1 || !owner_in_first_seat {
            return Err("The owner needs to mount it first before you can do it.");
        }
        Ok(())
    }
}

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
    chest: bool,
    regrowth: u16,
    happiness: u8,
    plucked: bool,
}

impl Tamed {
    fn decode(bytes: &[u8]) -> Option<Self> {
        let (bytes, chest) = match bytes {
            [base @ .., chest @ (0 | 1)] if base.len() == 23 => (base, *chest == 1),
            _ => (bytes, false),
        };
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
            chest: chest && *saddle == 1,
        })
    }

    fn encode(self) -> [u8; 24] {
        let mut data = [0; 24];
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
        data[23] = u8::from(self.chest);
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

pub fn happiness(mob: Entity<'_>) -> Option<u8> {
    tamed(mob).map(|state| state.happiness)
}

/// DragonGroomingComponent.getBrushingProgressPercent.
pub fn brushing_progress(mob: Entity<'_>) -> Option<u16> {
    if MOBS.get()?.kind(mob) == Some("saintsdragons:nulljaw") {
        return None;
    }
    let remaining = u32::from(tamed(mob)?.regrowth);
    let total = u32::from(REGROWTH_TICKS);
    Some((total.saturating_sub(remaining) * 100 / total) as u16)
}

/// Change a tamed dragon's happiness within 0..=100 and return the new value.
pub fn add_happiness(mob: Entity<'_>, delta: i32) -> Option<u8> {
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut state = tamed(mob)?;
    let next = (i32::from(state.happiness) + delta).clamp(0, 100) as u8;
    if next != state.happiness {
        state.happiness = next;
        MOBS.get()?.set_state(mob, &state.encode());
    }
    Some(next)
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
                chest: false,
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

pub fn held_item(click: &UseMob<'_>) -> Option<String> {
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
        chest: false,
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

/// DragonGroomingComponent.tryBrush.
fn brush(click: &mut UseMob<'_>, golden: bool) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(stacks)) =
        (click.player, click.mob, MOBS.get(), STACKS.get())
    else {
        return UseResult::Pass;
    };
    let id = mobs.kind(mob).and_then(scale);
    let mut guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let timers = &mut guard.get_or_insert_default().brush;
    let Some(old) = tamed(mob) else {
        return UseResult::Consume;
    };
    let now = tick_count();
    timers.retain(|_, until| *until > now);
    if timers.contains_key(&mob.id()) {
        return UseResult::Consume;
    }
    if (old.regrowth > 0 || id.is_none()) && old.happiness == 100 {
        mobs.message(player, "This dragon's scales are still growing.", true);
        return UseResult::Consume;
    }
    let Some(view) = mob.view() else {
        return UseResult::Fail;
    };
    let count = if old.regrowth == 0 && id.is_some() {
        (if golden { 6 } else { 1 }) + random(5) as i32
    } else {
        0
    };
    let Ok(reward) = fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String(id.unwrap_or_default())),
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

/// DragonGroomingComponent.tryPluck.
fn pluck(click: &mut UseMob<'_>) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(vitals)) =
        (click.player, click.mob, MOBS.get(), crate::MOB_HEALTH.get())
    else {
        return UseResult::Pass;
    };
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let old = tamed(mob);
    let health = mobs.health(mob).map_or(0.0, |(health, _)| health);
    let kind = mobs.kind(mob).unwrap_or_default();
    let id = scale(kind);
    let name = crate::binders::saved_name(mob).unwrap_or_else(|| {
        crate::roster::display(kind.strip_prefix("saintsdragons:").unwrap_or(kind)).to_owned()
    });
    let reason = match old {
        None => format!("{name} must be tamed before a scale can be plucked."),
        Some(state) if mobs.uuid(player) != Some(state.owner) => {
            format!("{name} will not let you near its scales.")
        }
        Some(_) if crate::eggs::is_juvenile(mob) => {
            format!("{name} is too young to have a scale plucked.")
        }
        Some(_) if id.is_none() => format!("{name} has no scale that can be plucked."),
        Some(state) if state.regrowth == 0 => {
            format!("{name}'s scales are ready to be brushed normally.")
        }
        Some(state) if state.plucked => format!(
            "A scale has already been forcibly plucked from {name} during this shedding cycle."
        ),
        Some(_) if health <= 5.0 => format!("{name} is too weak to endure having a scale plucked."),
        Some(_) => String::new(),
    };
    if !reason.is_empty() {
        mobs.message(player, &reason, true);
        return UseResult::Consume;
    }
    let (Some(old), Some(id)) = (old, id) else {
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
    if !click.give(&reward)
        && !mob.view().is_some_and(|view| {
            STACKS.get().is_some_and(|stacks| {
                stacks
                    .spawn(click.world, view.position, [0.0, 0.2, 0.0], &reward)
                    .is_some()
            })
        })
    {
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
    crate::roost::wake(mob, false, 100);
    crate::visuals::hurt_reaction(mob, kind);
    crate::audio::hurt(click.world, mob, kind);
    crate::codex::remember(click.world, mob, None);
    mobs.message(
        player,
        &format!("You forcibly pluck a scale from {name}."),
        true,
    );
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
    if click.mob.and_then(|mob| MOBS.get()?.kind(mob)) == Some(crate::mossback::KIND) {
        return crate::mossback::interact(click);
    }
    let (Some(player), Some(mob), Some(mobs), Some(tags)) =
        (click.player, click.mob, MOBS.get(), TAGS.get())
    else {
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
    if let Some(result) = crate::wild::sleep_refusal(click) {
        return result;
    }
    // These Java handlers route crouch-feeding through breeding even for tame babies.
    if matches!(
        mobs.kind(mob),
        Some("saintsdragons:volitans" | "saintsdragons:nulljaw")
    ) && let Some(result) = crate::breed::feed(click, item)
    {
        return result;
    }
    if let Some(result) = crate::baby::interact(click, item) {
        return result;
    }
    if let Some(result) = attach_equipment(click, item) {
        return result;
    }
    if let Some(result) = crate::breed::feed(click, item) {
        return result;
    }
    if let Some(result) = crate::wild::feed(click, item) {
        return result;
    }
    if let Some(result) = crate::wild::interact(click, item) {
        return result;
    }
    if let Some(result) = crate::stun::feed(click, item) {
        return result;
    }
    if mobs.kind(mob) != Some(crate::CINDERVANE) {
        return mount_interact(click, item);
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
        // `add_happiness` takes the timer lock.
        drop(lock);
        if !click.creative {
            click.stack.shrink(1);
        }
        crate::audio::eat(click.world, mob, item);
        let hearty = item == HEARTY_MEAL;
        if hearty {
            mob.add_effect("minecraft:regeneration", 200, 1, false, true);
        }
        let hungry = crate::hunger::feed(mob, hearty);
        if state.is_some() {
            let heal = if hearty { 15.0 } else { 5.0 };
            let full = mobs
                .health(mob)
                .is_some_and(|(health, max)| health + heal >= max);
            mobs.heal(mob, heal);
            feed_particles(click.world, mob, true);
            let message = match (full, hungry) {
                (true, true) => "Feeding Cindervane...",
                (true, false) => "Cindervane is fully healed!",
                _ => "Cindervane is being healed...",
            };
            mobs.message(player, message, true);
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

    drop(lock);
    mount_interact(click, item)
}

/// DragonSaddleInteractionHelper and each species' mounting handler.
fn mount_interact(click: &mut UseMob<'_>, item: &str) -> UseResult {
    let (Some(player), Some(mob), Some(mobs), Some(ai), Some(entities)) = (
        click.player,
        click.mob,
        MOBS.get(),
        MOB_AI.get(),
        ENTITIES.get(),
    ) else {
        return UseResult::Pass;
    };
    let Some(kind) = mobs.kind(mob) else {
        return UseResult::Pass;
    };
    let Some(profile) = mount_profile(kind) else {
        return UseResult::Pass;
    };
    if !click.main_hand || crate::eggs::is_juvenile(mob) {
        return UseResult::Pass;
    }
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(mut state) = tamed(mob) else {
        return UseResult::Pass;
    };
    let owner = mobs.uuid(player) == Some(state.owner);
    if !owner && profile.seats == 1 {
        return UseResult::Pass;
    }
    if click.sneaking {
        if !owner
            || !matches!(
                kind,
                crate::CINDERVANE | crate::wild::VARASUCHUS | "saintsdragons:stegonaut"
            )
        {
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
        if kind == crate::CINDERVANE {
            mobs.message(player, &format!("Cindervane is {label}."), true);
        }
        return UseResult::Success;
    }
    if owner && profile.saddle && !state.saddle {
        let name = crate::binders::saved_name(mob).unwrap_or_else(|| {
            crate::roster::display(kind.trim_start_matches("saintsdragons:")).to_owned()
        });
        mobs.message(
            player,
            &format!("{name} won't let you ride without a saddle."),
            true,
        );
        return UseResult::Consume;
    }

    if player.vehicle() == Some(mob.id()) {
        return UseResult::Consume;
    }
    // Species food handlers run first; an unhandled food is not a mounting click.
    if TAGS.get().is_some_and(|tags| {
        tags.contains(
            "item",
            &format!(
                "saintsdragons:foods/{}",
                kind.trim_start_matches("saintsdragons:")
            ),
            item,
        )
    }) {
        return UseResult::Pass;
    }
    let mut passengers = mobs.passengers(mob);
    let owner_in_first_seat = passengers.first().is_some_and(|&first| {
        entities.with_entity(click.world, first, |rider| mobs.uuid(rider))
            == Some(Some(state.owner))
    });
    if owner {
        if !profile.owner_can_mount(state, false) {
            if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
                let [x, y, z] = view.position;
                effects.particles(
                    click.world,
                    "minecraft:angry_villager",
                    [x, y + 2.0, z],
                    [0.3, 0.2, 0.3],
                    0.0,
                    6,
                );
            }
            return UseResult::Consume;
        }
        if !passengers.is_empty() {
            if profile.seats == 1 || owner_in_first_seat {
                return UseResult::Consume;
            }
            // Cindervane's owner can reclaim the controlling seat.
            entities.with_entity(click.world, passengers[0], |rider| rider.stop_riding());
            passengers = mobs.passengers(mob);
            if !passengers.is_empty() {
                return UseResult::Fail;
            }
        }
    } else {
        if let Err(message) = profile.guest_seat(passengers.len(), owner_in_first_seat) {
            mobs.message(player, message, true);
            return UseResult::Fail;
        }
        if !state.saddle {
            return UseResult::Fail;
        }
    }
    // shortcut: the ABI has no per-passenger seat transform or safe dismount hook;
    // the host's carrier attachment and normal dismount handle positioning.
    if !player.start_riding(mob.id(), !owner) {
        return UseResult::Fail;
    }
    if owner {
        if state.order == Order::Sit {
            state.order = Order::Follow;
            mobs.set_state(mob, &state.encode());
        }
        crate::roost::wake(
            mob,
            true,
            if kind == crate::wild::VARASUCHUS {
                200
            } else {
                100
            },
        );
        ai.stop(mob);
        ai.set_target(click.world, mob, None);
    }
    UseResult::Success
}

/// DragonSaddleInteractionHelper.handle: attach equipment on an owned adult.
/// Cargo and slot removal require the rider inventory action and entity-backed menu.
fn attach_equipment(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    if !click.main_hand || !matches!(item, SADDLE | CHEST) {
        return None;
    }
    let (mob, player, mobs) = (click.mob?, click.player?, MOBS.get()?);
    if !matches!(
        mobs.kind(mob)?,
        crate::CINDERVANE | "saintsdragons:atroxiia" | "saintsdragons:stegonaut"
    ) || crate::eggs::is_juvenile(mob)
    {
        return None;
    }
    let _guard = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut state = tamed(mob)?;
    if state.owner != mobs.uuid(player)? {
        return None;
    }
    let sound = match item {
        SADDLE if !state.saddle => {
            state.saddle = true;
            "minecraft:entity.horse.saddle"
        }
        CHEST if !state.chest => {
            if !state.saddle {
                let name = crate::binders::saved_name(mob).unwrap_or_else(|| {
                    crate::roster::names()
                        .find(|(id, _)| {
                            mobs.kind(mob)
                                .and_then(|kind| kind.strip_prefix("saintsdragons:"))
                                == Some(*id)
                        })
                        .map_or("Dragon", |(_, name)| name)
                        .to_owned()
                });
                mobs.message(
                    player,
                    &format!("{name} needs a saddle before you can attach a chest."),
                    true,
                );
                return Some(UseResult::Success);
            }
            state.chest = true;
            "minecraft:entity.donkey.chest"
        }
        _ => return None,
    };
    if !mobs.set_state(mob, &state.encode()) {
        return Some(UseResult::Fail);
    }
    if !click.creative {
        click.stack.shrink(1);
    }
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        effects.play_sound(click.world, view.position, sound, SOUND_NEUTRAL, 1.0, 1.0);
    }
    Some(UseResult::Success)
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
        chest: false,
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
    crate::particles::tame(world, mob, happy);
}

pub fn death(world: World<'_>, mob: Entity<'_>) {
    crate::combat_nulljaw::death(world, mob);
    if MOBS.get().and_then(|mobs| mobs.kind(mob)) == Some(crate::mossback::KIND)
        && crate::mossback::baby(mob)
        && let Ok(loot) = moddedmc_api::mobs::require_loot()
    {
        loot.suppress(mob);
    }
    crate::audio::death(world, mob);
    crate::nucleus::defeated(world, mob);
    crate::codex::remove(world, mob);
    crate::flight::forget(mob.id());
    crate::swarm::forget(mob.id());
    crate::stun::forget(mob.id());
    crate::breed::forget(mob.id());
    crate::wild::forget(mob.id());
    crate::cindervane_wild::forget(mob.id());
    crate::roost::forget(mob.id());
    crate::hunger::forget(mob.id());
    crate::hunt::forget(mob.id());
    crate::baby::forget(mob.id());
    crate::combat_volitans::forget(mob.id());
    crate::ivy::forget(mob.id());
    crate::combat_varasuchus::forget(mob.id());
    crate::visuals::death(world, mob);
    let chest = {
        let mut lock = TIMERS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(timers) = lock.as_mut() {
            timers.feed.remove(&mob.id());
            timers.brush.remove(&mob.id());
        }
        let (Some(mobs), Some(mut state)) = (MOBS.get(), tamed(mob).filter(|state| state.saddle))
        else {
            return;
        };
        let chest = state.chest;
        state.saddle = false;
        state.chest = false;
        if !mobs.set_state(mob, &state.encode()) {
            return;
        }
        chest
    };
    let (Some(stacks), Some(view)) = (STACKS.get(), mob.view()) else {
        return;
    };
    for item in [Some(SADDLE), chest.then_some(CHEST)].into_iter().flatten() {
        if let Ok(stack) = fastnbt::to_bytes(&HashMap::from([
            ("id", Value::String(item.to_owned())),
            ("count", Value::Int(1)),
        ])) {
            stacks.spawn(world, view.position, [0.0; 3], &stack);
        }
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
                        if mobs.kind(mob) == Some(crate::wild::VARASUCHUS) {
                            crate::combat_varasuchus::ride(world, mob, rider);
                        } else if mobs.kind(mob) == Some("saintsdragons:stegonaut") {
                            crate::combat_stegonaut::ride(mob, rider);
                        } else if mobs.kind(mob) == Some("saintsdragons:atroxiia") {
                            crate::combat_atroxiia::ride(world, mob, rider);
                        } else {
                            ride(world, mob, rider);
                        }
                    } else if mobs.kind(mob) != Some(crate::CINDERVANE) {
                        rider.stop_riding();
                    }
                });
            }
            false
        }
        _ => false,
    }
}

fn ride(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>) {
    let (Some(mobs), Some(view), Some(look), Some(body)) =
        (MOBS.get(), mob.view(), rider.view(), rider.body())
    else {
        return;
    };
    if crate::combat::flexing(mob) {
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [0.0; 3],
            ..EntityChange::default()
        });
        return;
    }
    let keys = mobs.player_input(rider).unwrap_or(0);
    if mobs.kind(mob) == Some("saintsdragons:volitans") {
        crate::combat_volitans::ride(world, mob, rider, view, look, body.input, keys);
        return;
    }
    let (ascend, mut descend, accelerate, locked) =
        if mobs.kind(mob) == Some("saintsdragons:raevyx") {
            crate::combat_raevyx::rider_movement(mob)
        } else {
            (false, false, false, false)
        };
    if locked {
        return;
    }
    let mut sprint = accelerate || keys & PLAYER_INPUT_SPRINT != 0;
    let mut jump = ascend || keys & PLAYER_INPUT_JUMP != 0;
    let mut ground_speed = None;
    if mobs.kind(mob) == Some("saintsdragons:ignivorus") {
        let (locked, ascend, down, accelerate, speed) =
            crate::combat_ignivorus::rider_movement(mob, sprint);
        if locked {
            rider.reset_fall_distance();
            mob.reset_fall_distance();
            return;
        }
        sprint |= accelerate;
        jump |= ascend;
        descend = down;
        ground_speed = Some(speed);
    }
    let yaw = f64::from(look.yaw).to_radians();
    let strafe = body.input[0] * 0.4;
    let forward = body.input[2];
    // shortcut: stock clients use jump and look pitch for vertical control. The Java release
    // also sends custom descend, dive and flight mode packets.
    let flying = mobs
        .kind(mob)
        .and_then(mount_profile)
        .is_some_and(|profile| profile.flying);
    let air = flying && (jump || !view.on_ground);
    let speed = match (air, sprint) {
        (true, true) => 0.60 * 4.0,
        (true, false) => 0.60 * 3.0,
        (false, true) => ground_speed.unwrap_or(0.45),
        (false, false) => ground_speed.unwrap_or(0.18),
    };
    let desired_x = (strafe * yaw.cos() - forward * yaw.sin()) * speed;
    let desired_z = (strafe * yaw.sin() + forward * yaw.cos()) * speed;
    let vertical = if descend && air {
        if mobs.kind(mob) == Some("saintsdragons:ignivorus") {
            -0.45
        } else {
            -0.85
        }
    } else if flying && jump {
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
    // `DragonFollowOwnerBehaviour` excludes sleep-locked dragons.
    // shortcut: owner bed state is unavailable in the ABI; only roost sleep exists today.
    if mobs.kind(mob) == Some(crate::wild::VARASUCHUS)
        && crate::roost::sleep_clip(mob.id()).is_some()
    {
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
    fn released_mount_profiles_and_owner_requirements() {
        for (species, saddle, seats, flying) in [
            ("cindervane", true, 2, true),
            ("atroxiia", true, 1, false),
            ("stegonaut", true, 1, false),
            ("varasuchus", false, 1, false),
            ("ignivorus", false, 1, true),
            ("raevyx", false, 1, true),
            ("volitans", false, 1, true),
            ("nulljaw", false, 1, true),
        ] {
            let profile = mount_profile(&format!("saintsdragons:{species}")).unwrap();
            assert_eq!(
                profile,
                MountProfile {
                    saddle,
                    seats,
                    flying
                }
            );
            let mut state = Tamed {
                owner: [0; 16],
                order: Order::Sit,
                saddle: false,
                chest: false,
                regrowth: 0,
                happiness: 100,
                plucked: false,
            };
            assert_eq!(profile.owner_can_mount(state, false), !saddle);
            state.saddle = true;
            assert!(profile.owner_can_mount(state, false));
            assert!(!profile.owner_can_mount(state, true));
            state.happiness = 30;
            assert!(!profile.owner_can_mount(state, false));
            state.happiness = 31;
            assert!(profile.owner_can_mount(state, false));
        }
        assert_eq!(mount_profile("saintsdragons:ivy"), None);
        assert_eq!(mount_profile("saintsdragons:mossback"), None);
        assert_eq!(mount_profile("saintsdragons:moop"), None);
        let cindervane = mount_profile(crate::CINDERVANE).unwrap();
        assert!(cindervane.guest_seat(0, false).is_err());
        assert!(cindervane.guest_seat(1, false).is_err());
        assert!(cindervane.guest_seat(1, true).is_ok());
        assert!(cindervane.guest_seat(2, true).is_err());
        let stegonaut = mount_profile("saintsdragons:stegonaut").unwrap();
        assert!(stegonaut.guest_seat(0, false).is_err());
        assert!(stegonaut.guest_seat(1, true).is_err());
    }

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
                chest: true,
                regrowth: REGROWTH_TICKS,
                happiness: 60,
                plucked: true,
            };
            let data = state.encode();
            assert_eq!(Tamed::decode(&data), Some(state));
            assert_eq!(
                Tamed::decode(&data[..23]),
                Some(Tamed {
                    chest: false,
                    ..state
                })
            );
            assert_eq!(Tamed::decode(&data[..18]), None);
        }
    }
}
