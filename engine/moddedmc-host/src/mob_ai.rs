//! `moddedmc:mob_ai`: goals for mod mob types, attached to every mob of the type when it joins a
//! world (spawned or loaded), and the navigation, look and target calls they use.

use std::{
    cell::Cell,
    collections::BTreeMap,
    ptr,
    sync::{Arc, Mutex, PoisonError},
};

use moddedmc_api::abi::{self, EntityRef, Reg, Str, Vec3, WorldRef, code, goal_control};
use pumpkin_core::{
    entity::{
        EntityBase,
        ai::{
            goal::{Controls, Goal},
            pathfinder::NavigatorGoal,
        },
        mob::Mob,
    },
    plugin::{
        BoxFuture, Cancellable, EventHandler,
        api::events::{
            entity::{
                entity_damage::EntityDamageEvent, entity_damage_final::EntityDamageFinalEvent,
                entity_death::EntityDeathEvent, entity_regain_health::EntityRegainHealthEvent,
            },
            player::player_interact_entity_event::PlayerInteractEntityEvent,
        },
    },
    server::Server,
    world::World,
};
use pumpkin_data::{damage::DamageType, data_component_impl::IDSetContent};
use pumpkin_util::math::vector3::Vector3;

/// Resolve dynamic damage types through their runtime registry, whose keys
/// are distinct from death-message IDs. Vanilla IDs retain their static names.
pub fn damage_type_name(server: &Server, damage_type: &DamageType) -> String {
    if <DamageType as IDSetContent>::from_id(u16::from(damage_type.id)).is_none()
        && let Some((key, _)) = server
            .datapack_manager
            .get_damage_type_registry_map()
            .into_iter()
            .find(|(_, entry)| entry.damage_type == *damage_type)
    {
        return key;
    }
    IDSetContent::to_string(damage_type)
}

use crate::{
    content::{entity, world},
    mobs, native,
};

pub static MOB_AI: abi::MobAiV1 = abi::MobAiV1 {
    size: size_of::<abi::MobAiV1>(),
    add_goal,
    replace_goals,
    walk_to,
    stop,
    navigating,
    look_at,
    set_target,
    target,
    on_hurt,
    last_hurt_by,
    attack,
    damage_source,
    on_hurt_final,
    on_heal,
};

thread_local! {
    static DAMAGE_SOURCE: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

/// Scope the damage source to a synchronous hurt callback, restoring any enclosing hit.
pub fn with_damage_source<T>(event: &EntityDamageEvent, callback: impl FnOnce() -> T) -> T {
    struct Restore(Option<(i32, i32)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            DAMAGE_SOURCE.set(self.0);
        }
    }
    let _restore = Restore(DAMAGE_SOURCE.replace(Some((
        event.causing_entity_id.unwrap_or(-1),
        event.direct_entity_id.unwrap_or(-1),
    ))));
    callback()
}

unsafe extern "C" fn damage_source(causing: *mut i32, direct: *mut i32) -> bool {
    let source = DAMAGE_SOURCE.get();
    let (causing_id, direct_id) = source.unwrap_or((-1, -1));
    // SAFETY: non-null output pointers are writable for the call.
    if let Some(out) = unsafe { causing.as_mut() } {
        *out = causing_id;
    }
    // SAFETY: a non-null direct output pointer is writable for the call.
    if let Some(out) = unsafe { direct.as_mut() } {
        *out = direct_id;
    }
    source.is_some()
}

#[cfg(test)]
mod damage_source_tests {
    use super::*;

    fn read_source() -> (bool, i32, i32) {
        let (mut causing, mut direct) = (0, 0);
        // SAFETY: both output pointers are live.
        let active = unsafe { damage_source(&raw mut causing, &raw mut direct) };
        (active, causing, direct)
    }

    #[test]
    fn nested_hurt_callbacks_restore_outer_source() {
        let projectile =
            EntityDamageEvent::new(1, DamageType::ARROW, 2.0).with_source(Some(3), Some(4));
        let fall = EntityDamageEvent::new(1, DamageType::FALL, 1.0);
        assert_eq!(read_source(), (false, -1, -1));
        with_damage_source(&projectile, || {
            assert_eq!(read_source(), (true, 3, 4));
            with_damage_source(&fall, || {
                assert_eq!(read_source(), (true, -1, -1));
            });
            assert_eq!(read_source(), (true, 3, 4));
        });
        assert_eq!(read_source(), (false, -1, -1));
    }
}

#[derive(Clone, Copy)]
pub struct StagedGoal {
    key: u64,
    priority: u8,
    controls: u32,
    callback: abi::GoalCallback,
}

/// The AI a mod stages for one mob type.
#[derive(Default)]
pub struct TypeAi {
    replace: bool,
    goals: Vec<StagedGoal>,
    hurt: Option<abi::HurtCallback>,
    hurt_final: Option<abi::HurtFinalCallback>,
    heal: Option<abi::HealCallback>,
    use_mob: Option<abi::UseMobFn>,
    death: Option<abi::MobDeathFn>,
    merchant_close: Option<abi::MerchantCloseFn>,
}

static TYPES: Mutex<BTreeMap<String, TypeAi>> = Mutex::new(BTreeMap::new());

fn types() -> std::sync::MutexGuard<'static, BTreeMap<String, TypeAi>> {
    TYPES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The staged AI for `kind`, when it names a mod mob type registered or staged by now.
///
/// # Safety
/// `reg` is a live registration and `kind` is borrowed for the call.
unsafe fn staged<'a>(reg: Reg, kind: Str) -> Option<&'a mut TypeAi> {
    // SAFETY: the caller passes a live registration and a borrowed kind.
    let (staging, kind) = unsafe { (native::staging(reg), kind.str()) };
    if !mobs::known(&staging.mobs, kind) {
        return None;
    }
    Some(staging.mob_ai.entry(kind.to_owned()).or_default())
}

unsafe extern "C" fn add_goal(reg: Reg, def: *const abi::GoalDefinition) -> i32 {
    // SAFETY: the definition is borrowed for the call.
    let Some(def) = (unsafe { def.as_ref() }) else {
        return code::INVALID_DEFINITION;
    };
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, def.kind) }) else {
        return code::INVALID_DEFINITION;
    };
    ai.goals.push(StagedGoal {
        key: def.key,
        priority: u8::try_from(def.priority).unwrap_or(u8::MAX),
        controls: def.controls,
        callback: def.callback,
    });
    code::OK
}

unsafe extern "C" fn replace_goals(reg: Reg, kind: Str) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    ai.replace = true;
    code::OK
}

unsafe extern "C" fn on_hurt(reg: Reg, kind: Str, callback: abi::HurtCallback) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    if ai.hurt.is_some() {
        return code::DUPLICATE_ID;
    }
    ai.hurt = Some(callback);
    code::OK
}

unsafe extern "C" fn on_hurt_final(reg: Reg, kind: Str, callback: abi::HurtFinalCallback) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    if ai.hurt_final.is_some() {
        return code::DUPLICATE_ID;
    }
    ai.hurt_final = Some(callback);
    code::OK
}

unsafe extern "C" fn on_heal(reg: Reg, kind: Str, callback: abi::HealCallback) -> i32 {
    // SAFETY: registration and kind are borrowed for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    if ai.heal.is_some() {
        return code::DUPLICATE_ID;
    }
    ai.heal = Some(callback);
    code::OK
}

pub unsafe extern "C" fn on_use_mob(reg: Reg, kind: Str, callback: abi::UseMobFn) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    if ai.use_mob.replace(callback).is_some() {
        return code::DUPLICATE_ID;
    }
    code::OK
}

pub unsafe extern "C" fn on_death(reg: Reg, kind: Str, callback: abi::MobDeathFn) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    if ai.death.replace(callback).is_some() {
        return code::DUPLICATE_ID;
    }
    code::OK
}

pub unsafe extern "C" fn on_merchant_close(
    reg: Reg,
    kind: Str,
    callback: abi::MerchantCloseFn,
) -> i32 {
    // SAFETY: registration is live for the call.
    let Some(ai) = (unsafe { staged(reg, kind) }) else {
        return code::INVALID_DEFINITION;
    };
    if ai.merchant_close.is_some() {
        return code::DUPLICATE_ID;
    }
    ai.merchant_close = Some(callback);
    code::OK
}

pub fn merchant_close_callback(kind: &str) -> Option<abi::MerchantCloseFn> {
    types().get(kind).and_then(|ai| ai.merchant_close)
}

/// The loaded entity `id` in any world, with its world.
fn find(server: &Server, id: i32) -> Option<(Arc<World>, Arc<dyn EntityBase>)> {
    server.worlds.load().iter().find_map(|world| {
        world
            .get_entity_by_id(id)
            .map(|entity| (world.clone(), entity))
    })
}

/// Hands a right-click on a mod mob to its type's callback; a result other than pass cancels
/// the carrier's own interaction.
pub struct UseMobHandler;

impl EventHandler<PlayerInteractEntityEvent> for UseMobHandler {
    fn handle_blocking<'a>(
        &'a self,
        _server: &'a Arc<Server>,
        event: &'a mut PlayerInteractEntityEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            use pumpkin_protocol::java::server::play::ActionType;
            // Java 26.3 sends a single InteractAt for a right-click.
            if event.cancelled() || !matches!(event.action, ActionType::InteractAt) {
                return;
            }
            let Some(callback) = mobs::registered_kind(event.target.as_ref())
                .and_then(|kind| types().get(kind).and_then(|ai| ai.use_mob))
            else {
                return;
            };
            let player = event.player.clone();
            let world = player.world();
            let hand = event.hand;
            let inventory = player.inventory();
            let before = inventory.get_stack_in_hand(hand);
            let mut stack = before.clone();
            let context = abi::UseMob {
                sneaking: event.sneaking,
                creative: player.is_creative(),
                main_hand: hand == pumpkin_util::Hand::Right,
            };
            let result = crate::interaction::use_mob(
                callback,
                &world,
                &player,
                (&mut stack, hand),
                event.target.as_ref(),
                &context,
            );
            if !stack.are_equal(&before) {
                inventory.set_stack_in_hand(hand, stack);
            }
            if result != abi::USE_PASS {
                event.set_cancelled(true);
            }
        })
    }
}

/// Tells a mod mob type's callback that one of its mobs died, before its loot drops.
pub struct DeathHandler;

impl EventHandler<EntityDeathEvent> for DeathHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityDeathEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if types().values().all(|ai| ai.death.is_none()) {
                return;
            }
            let Some((world, entity)) = find(server, event.entity_id) else {
                return;
            };
            let Some(callback) = mobs::registered_kind(entity.as_ref())
                .and_then(|kind| types().get(kind).and_then(|ai| ai.death))
            else {
                return;
            };
            let base: &dyn EntityBase = entity.as_ref();
            // SAFETY: the world and mob outlive this synchronous callback. The
            // event is dispatched while the dying entity is still indexed.
            unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    ptr::from_ref(&base).cast(),
                );
            }
        })
    }
}

/// Commit a mod's staged AI after its registration succeeded.
pub fn register(staged: BTreeMap<String, TypeAi>) {
    let mut types = types();
    for (kind, ai) in staged {
        let entry = types.entry(kind).or_default();
        entry.replace |= ai.replace;
        entry.goals.extend(ai.goals);
        entry.hurt = entry.hurt.or(ai.hurt);
        entry.hurt_final = entry.hurt_final.or(ai.hurt_final);
        entry.heal = entry.heal.or(ai.heal);
        entry.use_mob = entry.use_mob.or(ai.use_mob);
        entry.death = entry.death.or(ai.death);
        entry.merchant_close = entry.merchant_close.or(ai.merchant_close);
    }
}

pub struct HealHandler;
impl EventHandler<EntityRegainHealthEvent> for HealHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityRegainHealthEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() || types().values().all(|ai| ai.heal.is_none()) {
                return;
            }
            let Some((world, entity)) = find(server, event.entity_id) else {
                return;
            };
            let Some(callback) = mobs::registered_kind(entity.as_ref())
                .and_then(|kind| types().get(kind).and_then(|ai| ai.heal))
            else {
                return;
            };
            let base: &dyn EntityBase = entity.as_ref();
            // SAFETY: world and entity remain live for this synchronous callback.
            if !unsafe {
                callback(
                    Arc::as_ptr(&world).cast(),
                    (&raw const base).cast(),
                    event.amount,
                )
            } {
                event.set_cancelled(true);
            }
        })
    }
}

/// Asks a mod mob type's hurt callback about each hit as vanilla `hurt` starts.
pub struct HurtHandler;

impl EventHandler<EntityDamageEvent> for HurtHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityDamageEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if event.cancelled() || types().values().all(|ai| ai.hurt.is_none()) {
                return;
            }
            let Some((world, entity)) = find(server, event.entity_id) else {
                return;
            };
            let Some(callback) = mobs::registered_kind(entity.as_ref())
                .and_then(|kind| types().get(kind).and_then(|ai| ai.hurt))
            else {
                return;
            };
            // Native hurt callbacks expect the stable Minecraft registry key.
            // The display string is not a protocol identifier and can differ
            // from the original resource location (or be locale-dependent).
            // `registry_key` is the death message id (`inFire`); this is `minecraft:in_fire`.
            let name = damage_type_name(server, &event.damage_type);
            let base: &dyn EntityBase = entity.as_ref();
            // SAFETY: the world, the mob and the name outlive this synchronous callback.
            let take = with_damage_source(event, || unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    ptr::from_ref(&base).cast(),
                    Str::new(name.as_bytes()),
                    event.damage,
                )
            });
            if !take {
                event.set_cancelled(true);
            }
        })
    }
}

/// Dispatch damage after mitigation to the registered mod mob.
pub struct HurtFinalHandler;

impl EventHandler<EntityDamageFinalEvent> for HurtFinalHandler {
    fn handle_blocking<'a>(
        &'a self,
        server: &'a Arc<Server>,
        event: &'a mut EntityDamageFinalEvent,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if types().values().all(|ai| ai.hurt_final.is_none()) {
                return;
            }
            let Some((world, entity)) = find(server, event.hit.entity_id) else {
                return;
            };
            let Some(callback) = mobs::registered_kind(entity.as_ref())
                .and_then(|kind| types().get(kind).and_then(|ai| ai.hurt_final))
            else {
                return;
            };
            let name = IDSetContent::to_string(&event.hit.damage_type);
            let base: &dyn EntityBase = entity.as_ref();
            // SAFETY: the world, mob and name outlive the synchronous callback.
            let amount = with_damage_source(&event.hit, || unsafe {
                callback(
                    ptr::from_ref(world.as_ref()).cast(),
                    ptr::from_ref(&base).cast(),
                    Str::new(name.as_bytes()),
                    event.hit.damage,
                    event.lethal,
                    event.absorption,
                )
            });
            if amount.is_finite() {
                event.hit.damage = amount.max(0.0);
            }
        })
    }
}

/// Give a mod mob its type's goals as it joins a world.
pub fn attach(entity: &Arc<dyn EntityBase>) {
    let Some(mob) = entity.get_mob() else { return };
    let Some(kind) = mobs::registered_kind(entity.as_ref()) else {
        return;
    };
    let me = mob.get_mob_entity();
    mobs::default_loot(me, kind);
    let (replace, goals) = match types().get(kind) {
        Some(ai) => (ai.replace, ai.goals.clone()),
        None => return,
    };
    if replace {
        me.goals_selector.clear(mob);
        me.target_selector.clear(mob);
    }
    for goal in goals {
        let selector = if goal.controls & goal_control::TARGET == 0 {
            &me.goals_selector
        } else {
            &me.target_selector
        };
        selector.add_goal(goal.priority, Box::new(ModGoal(goal)));
    }
}

struct ModGoal(StagedGoal);

impl ModGoal {
    fn ask(&self, phase: u32, mob: &dyn Mob) -> bool {
        let world = mob.get_entity().world.load_full();
        let base: &dyn EntityBase = mob;
        // SAFETY: the world and the mob outlive this synchronous callback.
        unsafe {
            (self.0.callback)(
                phase,
                self.0.key,
                ptr::from_ref(world.as_ref()).cast(),
                ptr::from_ref(&base).cast(),
            )
        }
    }
}

impl Goal for ModGoal {
    fn can_start(&mut self, mob: &dyn Mob) -> bool {
        self.ask(abi::goal_phase::CAN_START, mob)
    }

    fn should_continue(&mut self, mob: &dyn Mob) -> bool {
        self.ask(abi::goal_phase::CONTINUE, mob)
    }

    fn start(&mut self, mob: &dyn Mob) {
        self.ask(abi::goal_phase::START, mob);
    }

    fn stop(&mut self, mob: &dyn Mob) {
        self.ask(abi::goal_phase::STOP, mob);
    }

    fn tick(&mut self, mob: &dyn Mob) {
        self.ask(abi::goal_phase::TICK, mob);
    }

    fn should_run_every_tick(&self) -> bool {
        true
    }

    fn controls(&self) -> Controls {
        [
            (goal_control::MOVE, Controls::MOVE),
            (goal_control::LOOK, Controls::LOOK),
            (goal_control::JUMP, Controls::JUMP),
            (goal_control::TARGET, Controls::TARGET),
        ]
        .into_iter()
        .filter(|(bit, _)| self.0.controls & bit != 0)
        .fold(Controls::empty(), |all, (_, control)| all | control)
    }
}

/// # Safety
/// `raw` is an entity a callback passed, still live.
unsafe fn mob<'a>(raw: EntityRef) -> Option<&'a dyn Mob> {
    // SAFETY: the caller passes a live entity.
    unsafe { entity(raw) }?.get_mob()
}

fn finite(Vec3([x, y, z]): Vec3) -> Option<Vector3<f64>> {
    (x.is_finite() && y.is_finite() && z.is_finite()).then(|| Vector3::new(x, y, z))
}

unsafe extern "C" fn walk_to(raw: EntityRef, target: Vec3, speed: f64) -> bool {
    // SAFETY: the mob is live for the callback.
    let (Some(mob), Some(target)) = (unsafe { mob(raw) }, finite(target)) else {
        return false;
    };
    if !speed.is_finite() || speed <= 0.0 {
        return false;
    }
    let position = mob.get_entity().pos.load();
    mob.get_mob_entity()
        .navigator
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .set_progress(NavigatorGoal::new(position, target, speed));
    true
}

unsafe extern "C" fn stop(raw: EntityRef) {
    // SAFETY: the mob is live for the callback.
    if let Some(mob) = unsafe { mob(raw) } {
        mob.get_mob_entity()
            .navigator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop();
    }
}

unsafe extern "C" fn navigating(raw: EntityRef) -> bool {
    // SAFETY: the mob is live for the callback.
    unsafe { mob(raw) }.is_some_and(|mob| {
        !mob.get_mob_entity()
            .navigator
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_idle()
    })
}

unsafe extern "C" fn look_at(raw: EntityRef, position: Vec3) -> bool {
    // SAFETY: the mob is live for the callback.
    let (Some(mob), Some(at)) = (unsafe { mob(raw) }, finite(position)) else {
        return false;
    };
    mob.get_mob_entity()
        .look_control
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .look_at(mob, at.x, at.y, at.z);
    true
}

unsafe extern "C" fn set_target(raw_world: WorldRef, raw: EntityRef, target: i32) -> bool {
    // SAFETY: the world and the mob are live for the callback.
    let (Some(world), Some(mob)) = (unsafe { world(raw_world) }, unsafe { mob(raw) }) else {
        return false;
    };
    let target = if target < 0 {
        None
    } else {
        match world.get_entity_by_id(target) {
            Some(found) if found.get_entity().entity_id != mob.get_entity().entity_id => {
                Some(found)
            }
            _ => return false,
        }
    };
    mob.apply_mob_target(target);
    true
}

unsafe extern "C" fn target(raw: EntityRef) -> i32 {
    // SAFETY: the mob is live for the callback.
    unsafe { mob(raw) }
        .and_then(|mob| mob.get_mob_entity().get_target())
        .map_or(-1, |t| t.get_entity().entity_id)
}

unsafe extern "C" fn last_hurt_by(raw: EntityRef, timestamp: *mut i32) -> i32 {
    // SAFETY: the mob is live for the callback.
    let Some(living) = (unsafe { entity(raw) }).and_then(EntityBase::get_living_entity) else {
        return -1;
    };
    let Some(id) = living.get_last_hurt_by_mob_id() else {
        return -1;
    };
    // SAFETY: a non-null timestamp is writable for the call.
    if let Some(out) = unsafe { timestamp.as_mut() } {
        *out = living.get_last_hurt_by_mob_timestamp();
    }
    id
}

unsafe extern "C" fn attack(raw_world: WorldRef, raw: EntityRef, target: i32, amount: f32) -> bool {
    // SAFETY: the world and the mob are live for the callback.
    let (Some(world), Some(attacker)) = (unsafe { world(raw_world) }, unsafe { entity(raw) })
    else {
        return false;
    };
    if !amount.is_finite() || target == attacker.get_entity().entity_id {
        return false;
    }
    let Some(target) = world.get_entity_by_id(target) else {
        return false;
    };
    target.damage_with_context(
        target.as_ref(),
        amount,
        DamageType::MOB_ATTACK,
        None,
        Some(attacker),
        Some(attacker),
    )
}
