//! Entities touching a mod's blocks: read and change their motion, rotation and ground state.

use std::{ffi::c_void, marker::PhantomData, sync::OnceLock};

use crate::{
    InterfacePrefix, InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World,
    guard,
};

pub use abi::{
    ENTITY_ALIVE, ENTITY_FLYING, ENTITY_HANGING, ENTITY_ITEM, ENTITY_LIVING, ENTITY_PLAYER,
    ENTITY_PUSH_IGNORED, ENTITY_SNEAKING, EntityBody, EntityChange, EntityView, SET_BODY_YAW,
    SET_HEAD_YAW, SET_MOTION, SET_ON_GROUND, SET_YAW,
};

/// Entity functions added after the first ABI 1 release. Require the ones a mod uses in
/// `Mod::start`, so an older server skips the mod instead of running it without them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntitiesExtension {
    /// Ids, lookup by id, flags, movement, effects, step height and box queries.
    Movement,
    /// Mod data saved with the entity.
    Data,
    /// `Entity::may_build`.
    MayBuild,
    /// Carrier entities, `discard`, riding, `vehicle`, filtered box queries, and the
    /// `ENTITY_NO_PHYSICS` and `ENTITY_PASSENGER` flags.
    Carriers,
    /// Convert a borrowed player entity to a player handle.
    Player,
    /// Hurt entities with a registered damage type.
    Damage,
    /// `Entity::has_scoreboard_tag`.
    ScoreboardTags,
    /// `Entities::players`.
    Players,
    /// Attributed damage, effect removal and computed attributes.
    CombatEffects,
    /// Frozen tick reads and writes.
    Freezing,
    /// Native projectile spawning and ownership.
    Projectiles,
}

/// Most entity ids `Entities::entities_in` returns.
const MAX_QUERY: usize = 256;

#[derive(Clone, Copy)]
pub struct Entities(InterfacePrefix<abi::EntitiesV1>);

/// An entity, valid only during the callback that passed it.
#[derive(Clone, Copy)]
pub struct Entity<'a> {
    raw: abi::EntityRef,
    api: InterfacePrefix<abi::EntitiesV1>,
    scope: PhantomData<&'a ()>,
}

pub type Inside = fn(World<'_>, Pos, u32, Entity<'_>);

static TABLE: OnceLock<InterfacePrefix<abi::EntitiesV1>> = OnceLock::new();
static INSIDE: OnceLock<Inside> = OnceLock::new();

pub fn require() -> Result<Entities, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:entities v1; appended fields are
    // read only after require_extension checks the size.
    let table = unsafe {
        InterfacePrefix::<abi::EntitiesV1>::require(abi::ENTITIES, 1, abi::ENTITIES_V1_BASE_SIZE)
    }?;
    let _ = TABLE.set(table);
    Ok(Entities(table))
}

unsafe extern "C" fn inside(world: abi::WorldRef, pos: Pos, state: u32, entity: abi::EntityRef) {
    guard("entity inside", (), || {
        let (Some(&api), Some(callback), Some(world)) =
            (TABLE.get(), INSIDE.get(), World::borrowed(world))
        else {
            return;
        };
        callback(
            world,
            pos,
            state,
            Entity {
                raw: entity,
                api,
                scope: PhantomData,
            },
        );
    });
}

impl Entities {
    pub fn require_extension(
        self,
        extension: EntitiesExtension,
    ) -> Result<(), InterfaceUnavailable> {
        let (end, name) = match extension {
            EntitiesExtension::Movement => (
                std::mem::offset_of!(abi::EntitiesV1, has_indirect_passenger),
                "moddedmc:entities.movement",
            ),
            EntitiesExtension::Data => (
                std::mem::offset_of!(abi::EntitiesV1, set_data),
                "moddedmc:entities.data",
            ),
            EntitiesExtension::MayBuild => (
                std::mem::offset_of!(abi::EntitiesV1, may_build),
                "moddedmc:entities.may_build",
            ),
            EntitiesExtension::Player => (
                std::mem::offset_of!(abi::EntitiesV1, with_player_context),
                "moddedmc:entities.player",
            ),
            EntitiesExtension::Damage => (
                std::mem::offset_of!(abi::EntitiesV1, hurt),
                "moddedmc:entities.damage",
            ),
            EntitiesExtension::ScoreboardTags => (
                std::mem::offset_of!(abi::EntitiesV1, has_scoreboard_tag),
                "moddedmc:entities.scoreboard_tags",
            ),
            EntitiesExtension::Players => (
                std::mem::offset_of!(abi::EntitiesV1, players),
                "moddedmc:entities.players",
            ),
            EntitiesExtension::Carriers => (
                std::mem::offset_of!(abi::EntitiesV1, entities_matching),
                "moddedmc:entities.carriers",
            ),
            EntitiesExtension::CombatEffects => (
                std::mem::offset_of!(abi::EntitiesV1, attribute_value),
                "moddedmc:entities.combat_effects",
            ),
            EntitiesExtension::Freezing => (
                std::mem::offset_of!(abi::EntitiesV1, set_ticks_frozen),
                "moddedmc:entities.freezing",
            ),
            EntitiesExtension::Projectiles => (
                std::mem::offset_of!(abi::EntitiesV1, projectile_owner),
                "moddedmc:entities.projectiles",
            ),
        };
        if self.0.size >= end + size_of::<usize>() {
            Ok(())
        } else {
            Err(InterfaceUnavailable { name, version: 1 })
        }
    }

    /// Run `f` with the loaded entity `id`; `None` when it isn't there. Needs
    /// `EntitiesExtension::Movement`.
    pub fn with_entity<F: FnOnce(Entity<'_>) -> T, T>(
        self,
        world: World<'_>,
        id: i32,
        f: F,
    ) -> Option<T> {
        struct Visit<'f, F, T> {
            f: Option<F>,
            api: InterfacePrefix<abi::EntitiesV1>,
            out: &'f mut Option<T>,
        }
        unsafe extern "C" fn visit<F: FnOnce(Entity<'_>) -> T, T>(
            ctx: *mut c_void,
            raw: abi::EntityRef,
        ) {
            // SAFETY: ctx is the Visit below, live and exclusive for this call.
            let visit = unsafe { &mut *ctx.cast::<Visit<'_, F, T>>() };
            if let Some(f) = visit.f.take() {
                *visit.out = Some(f(Entity {
                    raw,
                    api: visit.api,
                    scope: PhantomData,
                }));
            }
        }
        let mut out = None;
        let mut ctx = Visit {
            f: Some(f),
            api: self.0,
            out: &mut out,
        };
        // SAFETY: the extension was required; ctx outlives the call and matches visit's type.
        unsafe {
            ((&raw const (*self.0.ptr).with_entity).read())(
                world.raw(),
                id,
                (&raw mut ctx).cast(),
                visit::<F, T>,
            );
        }
        out
    }

    /// Ids of the entities whose boxes intersect `min..max`, except `except`. Needs
    /// `EntitiesExtension::Movement`.
    pub fn entities_in(
        self,
        world: World<'_>,
        min: [f64; 3],
        max: [f64; 3],
        except: i32,
    ) -> Vec<i32> {
        let mut ids = vec![0; MAX_QUERY];
        // SAFETY: the extension was required and `ids` has the capacity passed.
        let count = unsafe {
            ((&raw const (*self.0.ptr).entities_in).read())(
                world.raw(),
                abi::Vec3(min),
                abi::Vec3(max),
                except,
                ids.as_mut_ptr(),
                ids.len(),
            )
        };
        ids.truncate(count.min(MAX_QUERY));
        ids
    }

    /// Ids of the world's players, spectators and dead ones included. Needs
    /// `EntitiesExtension::Players`.
    pub fn players(self, world: World<'_>) -> Vec<i32> {
        let mut ids = vec![0; 64];
        loop {
            // SAFETY: the extension was required and `ids` has the capacity passed.
            let count = unsafe {
                ((&raw const (*self.0.ptr).players).read())(
                    world.raw(),
                    ids.as_mut_ptr(),
                    ids.len(),
                )
            };
            if count <= ids.len() {
                ids.truncate(count);
                return ids;
            }
            ids.resize(count, 0);
        }
    }

    /// Every entity whose box intersects `min..max`, except `except` and those with any
    /// `exclude` flag bit, however many there are. Needs `EntitiesExtension::Carriers`.
    pub fn entities_matching(
        self,
        world: World<'_>,
        min: [f64; 3],
        max: [f64; 3],
        except: i32,
        exclude: u32,
    ) -> Vec<i32> {
        let mut ids = Vec::new();
        let mut page = vec![0; MAX_QUERY];
        loop {
            // SAFETY: the extension was required and `page` has the capacity passed.
            let left = unsafe {
                ((&raw const (*self.0.ptr).entities_matching).read())(
                    world.raw(),
                    abi::Vec3(min),
                    abi::Vec3(max),
                    except,
                    exclude,
                    ids.len(),
                    page.as_mut_ptr(),
                    page.len(),
                )
            };
            let taken = left.min(page.len());
            ids.extend_from_slice(&page[..taken]);
            if left <= page.len() {
                return ids;
            }
        }
    }

    /// Spawn a saved `abi::CARRIER_*` entity with mod data `nbt` under `key`. `None` when it
    /// couldn't be spawned. Needs `EntitiesExtension::Carriers`.
    pub fn spawn_carrier(
        self,
        world: World<'_>,
        kind: u8,
        position: [f64; 3],
        key: &str,
        nbt: &[u8],
    ) -> Option<i32> {
        // SAFETY: the extension was required and the slices are borrowed for the call.
        let id = unsafe {
            ((&raw const (*self.0.ptr).spawn_carrier).read())(
                world.raw(),
                kind,
                abi::Vec3(position),
                abi::Str::new(key.as_bytes()),
                abi::Str::new(nbt),
            )
        };
        (id >= 0).then_some(id)
    }

    /// Vanilla `Block.entityInside` for every content block this mod registers.
    pub fn on_entity_inside(
        self,
        reg: &mut Registry<'_>,
        callback: Inside,
    ) -> Result<(), RegistrationError> {
        if INSIDE.set(callback).is_err() {
            return Err(RegistrationError::DuplicateId);
        }
        // SAFETY: registration is live and the trampoline is a static function.
        RegistrationError::from_code(unsafe {
            ((&raw const (*self.0.ptr).on_entity_inside).read())(reg.reg, inside)
        })
    }
}

impl Entity<'_> {
    /// An entity another interface's callback passed in, when this mod required the table.
    pub(crate) fn borrowed(raw: abi::EntityRef) -> Option<Self> {
        (!raw.is_null()).then_some(())?;
        Some(Entity {
            raw,
            api: *TABLE.get()?,
            scope: PhantomData,
        })
    }

    pub fn view(self) -> Option<EntityView> {
        let mut view = EntityView::default();
        // SAFETY: the entity is live for this callback and the output is a local.
        unsafe { ((&raw const (*self.api.ptr).view).read())(self.raw, &raw mut view) }
            .then_some(view)
    }

    /// Checks a vanilla scoreboard tag. Returns false on hosts missing this extension.
    pub fn has_scoreboard_tag(self, tag: &str) -> bool {
        // SAFETY: the table size covers the field and the tag and entity are live for the call.
        self.api.size
            >= std::mem::offset_of!(abi::EntitiesV1, has_scoreboard_tag) + size_of::<usize>()
            && unsafe {
                ((&raw const (*self.api.ptr).has_scoreboard_tag).read())(self.raw, crate::s(tag))
            }
    }

    pub fn type_id(self) -> &'static str {
        // SAFETY: the host returns a static resource name.
        unsafe { ((&raw const (*self.api.ptr).type_id).read())(self.raw).str() }
    }

    pub fn change(self, change: &EntityChange) -> bool {
        // SAFETY: the entity is live for this callback and the change is borrowed for the call.
        unsafe { ((&raw const (*self.api.ptr).change).read())(self.raw, change) }
    }

    // The rest need `EntitiesExtension::Movement`.

    pub fn id(self) -> i32 {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).id).read())(self.raw) }
    }

    /// The number of ticks this entity has existed, including item entities.
    pub fn age(self) -> Option<i32> {
        let api = self.api.upto(std::mem::offset_of!(abi::EntitiesV1, age))?;
        // SAFETY: the checked prefix includes the function and the entity is borrowed.
        let age = unsafe { ((&raw const (*api).age).read())(self.raw) };
        (age >= 0).then_some(age)
    }

    /// `ENTITY_ALIVE`, `ENTITY_SNEAKING`, `ENTITY_FLYING`, `ENTITY_HANGING` and
    /// `ENTITY_PUSH_IGNORED` bits.
    pub fn flags(self) -> u32 {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).flags).read())(self.raw) }
    }

    /// Vanilla `Entity.move(MoverType.SELF, delta)`.
    pub fn move_by(self, delta: [f64; 3]) {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).move_by).read())(self.raw, abi::Vec3(delta)) }
    }

    pub fn set_position(self, position: [f64; 3]) {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).set_position).read())(self.raw, abi::Vec3(position)) }
    }

    pub fn body(self) -> Option<EntityBody> {
        let mut body = EntityBody::default();
        // SAFETY: the entity is live for this callback and the output is a local.
        unsafe { ((&raw const (*self.api.ptr).body).read())(self.raw, &raw mut body) }
            .then_some(body)
    }

    pub fn reset_fall_distance(self) {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).reset_fall_distance).read())(self.raw) }
    }

    /// Vanilla `LivingEntity.addEffect`.
    pub fn add_effect(
        self,
        effect: &str,
        duration: i32,
        amplifier: u8,
        ambient: bool,
        visible: bool,
    ) -> bool {
        // SAFETY: the entity is live and the id is borrowed for the call.
        unsafe {
            ((&raw const (*self.api.ptr).add_effect).read())(
                self.raw,
                abi::Str::new(effect.as_bytes()),
                duration,
                amplifier,
                ambient,
                visible,
            )
        }
    }

    /// The `minecraft:step_height` base value, `None` when the entity isn't living.
    pub fn step_height(self) -> Option<f64> {
        // SAFETY: the entity is live for this callback.
        let value = unsafe { ((&raw const (*self.api.ptr).step_height).read())(self.raw) };
        (!value.is_nan()).then_some(value)
    }

    /// Vanilla `Entity.hasIndirectPassenger`.
    pub fn has_indirect_passenger(self, other: i32) -> bool {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).has_indirect_passenger).read())(self.raw, other) }
    }

    /// The compound saved on this entity under `key`, as Java named-root NBT. Needs
    /// `EntitiesExtension::Data`.
    pub fn data(self, key: &str) -> Option<Vec<u8>> {
        let mut data = Vec::new();
        let sink = abi::Sink {
            ctx: (&raw mut data).cast(),
            put: crate::content::bytes,
        };
        // SAFETY: the entity is live for the callback and the sink collects synchronously.
        unsafe {
            ((&raw const (*self.api.ptr).data).read())(
                self.raw,
                abi::Str::new(key.as_bytes()),
                sink,
            )
        }
        .then_some(data)
    }

    /// Save a compound under `key`, as Java named-root NBT; empty removes it. Needs
    /// `EntitiesExtension::Data`.
    pub fn set_data(self, key: &str, nbt: &[u8]) -> bool {
        // SAFETY: the entity is live for the callback and both slices are borrowed for the call.
        unsafe {
            ((&raw const (*self.api.ptr).set_data).read())(
                self.raw,
                abi::Str::new(key.as_bytes()),
                abi::Str::new(nbt),
            )
        }
    }

    /// Vanilla `Player.mayBuild()`: false for other entities and for Adventure and Spectator
    /// players. `EntitiesExtension::MayBuild`.
    pub fn may_build(self) -> bool {
        // SAFETY: the entity is live for the callback.
        unsafe { ((&raw const (*self.api.ptr).may_build).read())(self.raw) }
    }

    pub fn set_step_height(self, value: f64) {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).set_step_height).read())(self.raw, value) }
    }

    // The rest need `EntitiesExtension::Carriers`.

    /// Vanilla `Entity.discard()`.
    pub fn discard(self) -> bool {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).discard).read())(self.raw) }
    }

    /// Vanilla `Entity.startRiding(vehicle, force)`.
    pub fn start_riding(self, vehicle: i32, force: bool) -> bool {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).start_riding).read())(self.raw, vehicle, force) }
    }

    /// Vanilla `Entity.stopRiding()`; false when it wasn't riding.
    pub fn stop_riding(self) -> bool {
        // SAFETY: the entity is live for this callback.
        unsafe { ((&raw const (*self.api.ptr).stop_riding).read())(self.raw) }
    }

    /// The id of the entity this one rides.
    pub fn vehicle(self) -> Option<i32> {
        // SAFETY: the entity is live for this callback.
        let id = unsafe { ((&raw const (*self.api.ptr).vehicle).read())(self.raw) };
        (id != 0).then_some(id)
    }
}

impl crate::Player<'_> {
    /// Borrow the current world and player entity while handling a player callback.
    /// Requires `EntitiesExtension::Player`.
    pub fn with_entity_context<F, T>(&self, visit: F) -> Option<T>
    where
        F: FnOnce(World<'_>, Entity<'_>) -> T,
    {
        unsafe extern "C" fn call<F, T>(
            context: *mut c_void,
            world: abi::WorldRef,
            entity: abi::EntityRef,
        ) where
            F: FnOnce(World<'_>, Entity<'_>) -> T,
        {
            // SAFETY: the pointer names the tuple below and the host borrows it exclusively.
            let (visit, output) = unsafe { &mut *context.cast::<(Option<F>, Option<T>)>() };
            if let (Some(visit), Some(world), Some(entity)) = (
                visit.take(),
                World::borrowed(world),
                Entity::borrowed(entity),
            ) {
                *output =
                    crate::guard("player entity context", None, || Some(visit(world, entity)));
            }
        }
        let api = *TABLE.get()?;
        Entities(api)
            .require_extension(EntitiesExtension::Player)
            .ok()?;
        let mut context = (Some(visit), None);
        // SAFETY: the extension was checked and the tuple is borrowed until the synchronous visit returns.
        unsafe {
            ((&raw const (*api.ptr).with_player_context).read())(
                self.raw,
                (&raw mut context).cast(),
                call::<F, T>,
            );
        }
        context.1
    }
}

impl<'a> Entity<'a> {
    /// Damage from a loaded attacker. None means this host lacks the call.
    pub fn hurt_by(self, attacker: i32, damage_type: &str, amount: f32) -> Option<bool> {
        let api = self
            .api
            .upto(std::mem::offset_of!(abi::EntitiesV1, hurt_by))?;
        // SAFETY: the field is size checked and inputs are borrowed.
        Some(unsafe {
            ((&raw const (*api).hurt_by).read())(self.raw, attacker, crate::s(damage_type), amount)
        })
    }

    pub fn remove_effect(self, effect: &str) -> Option<bool> {
        let api = self
            .api
            .upto(std::mem::offset_of!(abi::EntitiesV1, remove_effect))?;
        // SAFETY: the field is size checked and inputs are borrowed.
        Some(unsafe { ((&raw const (*api).remove_effect).read())(self.raw, crate::s(effect)) })
    }

    pub fn attribute_value(self, attribute: &str) -> Option<f64> {
        let api = self
            .api
            .upto(std::mem::offset_of!(abi::EntitiesV1, attribute_value))?;
        // SAFETY: the field is size checked and inputs are borrowed.
        let value =
            unsafe { ((&raw const (*api).attribute_value).read())(self.raw, crate::s(attribute)) };
        value.is_finite().then_some(value)
    }

    pub fn ticks_frozen(self) -> Option<i32> {
        let api = self
            .api
            .upto(std::mem::offset_of!(abi::EntitiesV1, ticks_frozen))?;
        // SAFETY: the field is size checked and the handle is borrowed.
        let ticks = unsafe { ((&raw const (*api).ticks_frozen).read())(self.raw) };
        (ticks >= 0).then_some(ticks)
    }

    pub fn set_ticks_frozen(self, ticks: i32) -> Option<bool> {
        let api = self
            .api
            .upto(std::mem::offset_of!(abi::EntitiesV1, set_ticks_frozen))?;
        // SAFETY: the field is size checked and the handle is borrowed.
        Some(unsafe { ((&raw const (*api).set_ticks_frozen).read())(self.raw, ticks) })
    }

    pub fn projectile_owner(self) -> Option<i32> {
        let api = self
            .api
            .upto(std::mem::offset_of!(abi::EntitiesV1, projectile_owner))?;
        // SAFETY: the field is size checked and the handle is borrowed.
        let id = unsafe { ((&raw const (*api).projectile_owner).read())(self.raw) };
        (id >= 0).then_some(id)
    }

    /// Apply damage without an attacker. Requires [EntitiesExtension::Damage].
    pub fn hurt(self, damage_type: &str, amount: f32) -> bool {
        if Entities(self.api)
            .require_extension(EntitiesExtension::Damage)
            .is_err()
        {
            return false;
        }
        // SAFETY: the table contains the appended field and both inputs live for the call.
        unsafe {
            ((&raw const (*self.api.ptr).hurt).read())(
                self.raw,
                abi::Str::new(damage_type.as_bytes()),
                amount,
            )
        }
    }

    /// The player represented by this borrowed entity, if there is one.
    /// Requires `EntitiesExtension::Player`.
    pub fn as_player(self) -> Option<crate::Player<'a>> {
        if Entities(self.api)
            .require_extension(EntitiesExtension::Player)
            .is_err()
        {
            return None;
        }
        // SAFETY: the table contains the field and the handle is borrowed for 'a.
        let raw = unsafe { ((&raw const (*self.api.ptr).as_player).read())(self.raw) };
        (!raw.0.is_null()).then_some(crate::Player {
            raw,
            _call: PhantomData,
        })
    }

    pub(crate) const fn raw(self) -> abi::EntityRef {
        self.raw
    }
}

impl Entities {
    /// Spawn a vanilla projectile with a loaded owner; shulker bullets require a target.
    pub fn spawn_projectile(
        self,
        world: World<'_>,
        kind: &str,
        owner: i32,
        target: Option<i32>,
        position: [f64; 3],
        velocity: [f64; 3],
    ) -> Option<i32> {
        let api = self
            .0
            .upto(std::mem::offset_of!(abi::EntitiesV1, spawn_projectile))?;
        // SAFETY: the field is size checked and all inputs are borrowed.
        let id = unsafe {
            ((&raw const (*api).spawn_projectile).read())(
                world.raw(),
                crate::s(kind),
                owner,
                target.unwrap_or(-1),
                abi::Vec3(position),
                abi::Vec3(velocity),
            )
        };
        (id >= 0).then_some(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_calls_are_unavailable_on_old_hosts() {
        let api = InterfacePrefix {
            ptr: std::ptr::null(),
            size: std::mem::offset_of!(abi::EntitiesV1, hurt_by),
        };
        let entity = Entity {
            raw: std::ptr::null(),
            api,
            scope: PhantomData,
        };
        assert_eq!(entity.hurt_by(1, "minecraft:mob_attack", 1.0), None);
        assert_eq!(entity.remove_effect("minecraft:poison"), None);
        assert_eq!(entity.attribute_value("minecraft:armor"), None);
        assert_eq!(entity.ticks_frozen(), None);
        assert_eq!(entity.set_ticks_frozen(20), None);
        assert_eq!(entity.projectile_owner(), None);
        assert!(
            Entities(api)
                .require_extension(EntitiesExtension::Projectiles)
                .is_err()
        );
    }
}
