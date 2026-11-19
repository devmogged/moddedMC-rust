//! Entities touching a mod's blocks: read and change their motion, rotation and ground state.

use std::{ffi::c_void, marker::PhantomData, sync::OnceLock};

use crate::{
    InterfaceUnavailable, Pos, RegistrationError, Registry, abi, content::World, guard,
    require_interface_prefix,
};

pub use abi::{
    ENTITY_ALIVE, ENTITY_FLYING, ENTITY_ITEM, ENTITY_LIVING, ENTITY_PLAYER, ENTITY_SNEAKING,
    EntityBody, EntityChange, EntityView, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION, SET_ON_GROUND,
    SET_YAW,
};

/// Entity functions added after the first ABI 1 release. Require the ones a mod uses in
/// `Mod::start`, so an older server skips the mod instead of running it without them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntitiesExtension {
    /// Ids, lookup by id, flags, movement, effects, step height and box queries.
    Movement,
}

/// Most entity ids `Entities::entities_in` returns.
const MAX_QUERY: usize = 256;

#[derive(Clone, Copy)]
pub struct Entities(&'static abi::EntitiesV1);

/// An entity, valid only during the callback that passed it.
#[derive(Clone, Copy)]
pub struct Entity<'a> {
    raw: abi::EntityRef,
    api: &'static abi::EntitiesV1,
    scope: PhantomData<&'a ()>,
}

pub type Inside = fn(World<'_>, Pos, u32, Entity<'_>);

static TABLE: OnceLock<&'static abi::EntitiesV1> = OnceLock::new();
static INSIDE: OnceLock<Inside> = OnceLock::new();

pub fn require() -> Result<Entities, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:entities v1; appended fields are
    // read only after require_extension checks the size.
    let table = unsafe {
        require_interface_prefix::<abi::EntitiesV1>(abi::ENTITIES, 1, abi::ENTITIES_V1_BASE_SIZE)
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
        let EntitiesExtension::Movement = extension;
        if self.0.size >= size_of::<abi::EntitiesV1>() {
            Ok(())
        } else {
            Err(InterfaceUnavailable {
                name: "moddedmc:entities.movement",
                version: 1,
            })
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
            api: &'static abi::EntitiesV1,
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
            (self.0.with_entity)(world.raw(), id, (&raw mut ctx).cast(), visit::<F, T>);
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
            (self.0.entities_in)(
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
        RegistrationError::from_code(unsafe { (self.0.on_entity_inside)(reg.reg, inside) })
    }
}

impl Entity<'_> {
    /// An entity another interface's callback passed in, when this mod required the table.
    pub(crate) fn borrowed(raw: abi::EntityRef) -> Option<Self> {
        (!raw.is_null()).then_some(())?;
        Some(Entity {
            raw,
            api: TABLE.get()?,
            scope: PhantomData,
        })
    }

    pub fn view(self) -> Option<EntityView> {
        let mut view = EntityView::default();
        // SAFETY: the entity is live for this callback and the output is a local.
        unsafe { (self.api.view)(self.raw, &raw mut view) }.then_some(view)
    }

    pub fn type_id(self) -> &'static str {
        // SAFETY: the host returns a static resource name.
        unsafe { (self.api.type_id)(self.raw).str() }
    }

    pub fn change(self, change: &EntityChange) -> bool {
        // SAFETY: the entity is live for this callback and the change is borrowed for the call.
        unsafe { (self.api.change)(self.raw, change) }
    }

    // The rest need `EntitiesExtension::Movement`.

    pub fn id(self) -> i32 {
        // SAFETY: the entity is live for this callback.
        unsafe { (self.api.id)(self.raw) }
    }

    /// `ENTITY_ALIVE`, `ENTITY_SNEAKING` and `ENTITY_FLYING` bits.
    pub fn flags(self) -> u32 {
        // SAFETY: the entity is live for this callback.
        unsafe { (self.api.flags)(self.raw) }
    }

    /// Vanilla `Entity.move(MoverType.SELF, delta)`.
    pub fn move_by(self, delta: [f64; 3]) {
        // SAFETY: the entity is live for this callback.
        unsafe { (self.api.move_by)(self.raw, abi::Vec3(delta)) }
    }

    pub fn set_position(self, position: [f64; 3]) {
        // SAFETY: the entity is live for this callback.
        unsafe { (self.api.set_position)(self.raw, abi::Vec3(position)) }
    }

    pub fn body(self) -> Option<EntityBody> {
        let mut body = EntityBody::default();
        // SAFETY: the entity is live for this callback and the output is a local.
        unsafe { (self.api.body)(self.raw, &raw mut body) }.then_some(body)
    }

    pub fn reset_fall_distance(self) {
        // SAFETY: the entity is live for this callback.
        unsafe { (self.api.reset_fall_distance)(self.raw) }
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
            (self.api.add_effect)(
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
        let value = unsafe { (self.api.step_height)(self.raw) };
        (!value.is_nan()).then_some(value)
    }

    pub fn set_step_height(self, value: f64) {
        // SAFETY: the entity is live for this callback.
        unsafe { (self.api.set_step_height)(self.raw, value) }
    }
}
