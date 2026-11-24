//! Items with no block: ingots, alloys, tools and the like.

use std::sync::OnceLock;

use crate::{
    InterfaceUnavailable, RegistrationError, Registry, abi, entities::Entity, guard,
    require_interface, require_interface_prefix, s,
};

#[derive(Clone, Copy)]
pub struct Items(&'static abi::ItemsV1);

pub fn require() -> Result<Items, InterfaceUnavailable> {
    // SAFETY: the base fields are unchanged and later fields are checked before use.
    unsafe { require_interface_prefix(abi::ITEMS, 1, abi::ITEMS_V1_BASE_SIZE) }.map(Items)
}

#[derive(Clone, Copy)]
pub struct ItemDefaults(&'static abi::ItemDefaultsV1);

pub fn require_defaults() -> Result<ItemDefaults, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:item_defaults v1.
    unsafe { require_interface(abi::ITEM_DEFAULTS, 1) }.map(ItemDefaults)
}

impl ItemDefaults {
    pub fn add_item(
        self,
        reg: &mut Registry<'_>,
        item: &Item<'_>,
        defaults: &[u8],
    ) -> Result<(), RegistrationError> {
        let def = item.definition();
        // SAFETY: registration, definition and NBT are borrowed for this call.
        RegistrationError::from_code(unsafe {
            (self.0.add_item)(reg.reg, &def, abi::Str::new(defaults))
        })
    }
}

pub struct Item<'a> {
    pub id: &'a str,
    pub display_name: &'a str,
    /// The vanilla item it behaves like (stack size, food, tool), such as `minecraft:iron_ingot`.
    pub carrier: &'a str,
    pub model: &'a str,
}

impl Item<'_> {
    fn definition(&self) -> abi::ContentItemDefinition {
        abi::ContentItemDefinition {
            id: s(self.id),
            display_name: s(self.display_name),
            carrier: s(self.carrier),
            model: s(self.model),
        }
    }
}

impl Items {
    /// Give a registered item a durability bar and a single-item stack limit.
    pub fn item_durability(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        max_damage: u16,
    ) -> Result<(), RegistrationError> {
        if self.0.size < size_of::<abi::ItemsV1>() {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: the table size was checked and the registrar and id are borrowed.
        RegistrationError::from_code(unsafe {
            (self.0.item_durability)(reg.reg, s(item), max_damage)
        })
    }

    pub fn add_item(
        self,
        reg: &mut Registry<'_>,
        item: &Item<'_>,
    ) -> Result<(), RegistrationError> {
        let def = item.definition();
        // SAFETY: registration and the borrowed definition are live for the call.
        RegistrationError::from_code(unsafe { (self.0.add_item)(reg.reg, &def) })
    }
}

/// Additional behaviour for content items used in bows and crossbows.
#[derive(Clone, Copy)]
pub struct ArrowAmmo(&'static abi::ArrowAmmoV1);

pub type ArrowHurt = fn(&str, Entity<'_>);
static AFTER_ARROW_HURT: OnceLock<ArrowHurt> = OnceLock::new();

pub fn require_arrow_ammo() -> Result<ArrowAmmo, InterfaceUnavailable> {
    // SAFETY: this is the published table type for moddedmc:arrow_ammo v1.
    unsafe { require_interface(abi::ARROW_AMMO, 1) }.map(ArrowAmmo)
}

unsafe extern "C" fn after_arrow_hurt(item: abi::Str, raw: abi::EntityRef) {
    guard("arrow after hurt", (), || {
        if let (Some(callback), Some(target)) = (AFTER_ARROW_HURT.get(), Entity::borrowed(raw)) {
            // SAFETY: the host borrows the item id and target for this call.
            callback(unsafe { item.str() }, target);
        }
    });
}

impl ArrowAmmo {
    /// Bind an already-staged item with the `minecraft:arrow` carrier. The velocity
    /// multiplier must be finite and positive, and water inertia must be within 0 to 1.
    /// Mods that use the callback must require `moddedmc:entities` in `Mod::start`.
    pub fn bind(
        self,
        reg: &mut Registry<'_>,
        item: &str,
        velocity_multiplier: f32,
        water_inertia: f64,
        after_hurt: ArrowHurt,
    ) -> Result<(), RegistrationError> {
        let first = AFTER_ARROW_HURT.get_or_init(|| after_hurt);
        if !std::ptr::fn_addr_eq(*first, after_hurt) {
            return Err(RegistrationError::InvalidDefinition);
        }
        // SAFETY: the registration and item id are borrowed and the trampoline is static.
        RegistrationError::from_code(unsafe {
            (self.0.bind)(
                reg.reg,
                s(item),
                velocity_multiplier,
                water_inertia,
                after_arrow_hurt,
            )
        })
    }
}
