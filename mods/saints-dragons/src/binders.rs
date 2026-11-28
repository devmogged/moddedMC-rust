//! The eight 0.9.88 dragon binders share the same ownership and storage rules.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use fastnbt::{IntArray, Value};
use moddedmc_api::{
    InterfaceUnavailable, RegistrationError, Registry,
    content::World,
    entities::Entity,
    interaction::{Stack, UseItem, UseMob, UseOnBlock, UseResult},
    items::{Item, ItemDefaults},
    mobs::{self, MobStorage},
};

use crate::{ENTITIES, MOBS, flight, taming};

type Compound = HashMap<String, Value>;

struct Binder {
    item: &'static str,
    dragon: &'static str,
    name: &'static str,
    field: &'static str,
    flavor: &'static str,
    flying: bool,
}

const BINDERS: &[Binder] = &[
    Binder {
        item: "raevyx_binder",
        dragon: "saintsdragons:raevyx",
        name: "Raevyx",
        field: "RaevyxData",
        flavor: "Holds the storm at bay. For now.",
        flying: true,
    },
    Binder {
        item: "cindervane_binder",
        dragon: "saintsdragons:cindervane",
        name: "Cindervane",
        field: "CindervaneData",
        flavor: "The wind hums within, restless yet graceful.",
        flying: true,
    },
    Binder {
        item: "stegonaut_binder",
        dragon: "saintsdragons:stegonaut",
        name: "Stegonaut",
        field: "StegonautData",
        flavor: "A steadfast core. You feel protected.",
        flying: false,
    },
    Binder {
        item: "varasuchus_binder",
        dragon: "saintsdragons:varasuchus",
        name: "Varasuchus",
        field: "VarasuchusData",
        flavor: "Strength radiates quietly within.",
        flying: false,
    },
    Binder {
        item: "ignivorus_binder",
        dragon: "saintsdragons:ignivorus",
        name: "Ignivorus",
        field: "IgnivorusData",
        flavor: "Seals the fury of the living inferno until called.",
        flying: true,
    },
    Binder {
        item: "volitans_binder",
        dragon: "saintsdragons:volitans",
        name: "Volitans",
        field: "VolitansData",
        flavor: "You hear the sound of the ocean waves.",
        flying: true,
    },
    Binder {
        item: "nulljaw_binder",
        dragon: "saintsdragons:nulljaw",
        name: "Nulljaw",
        field: "NulljawData",
        flavor: "A quiet pull toward the void.",
        flying: false,
    },
    Binder {
        item: "atroxiia_binder",
        dragon: "saintsdragons:atroxiia",
        name: "Atroxiia",
        field: "AtroxiiaData",
        flavor: "Winter waits within, silent and unyielding.",
        flying: false,
    },
];

static STORAGE: OnceLock<MobStorage> = OnceLock::new();
// A pair of clicks from different players can otherwise target the same captured UUID.
static TRANSFER: Mutex<()> = Mutex::new(());
// shortcut: suppress the stock client's follow-up UseItem packet until it predicts binder use.
const CLICK_COOLDOWN: u32 = 2;

impl Binder {
    fn display(&self) -> String {
        format!("{} Binder", self.name)
    }

    fn lore(&self, name: Option<&str>) -> Value {
        let hint = if self.flying {
            "Right-click on ground or into open air to release the bound dragon"
        } else {
            "Right-click on ground to release the bound creature"
        };
        Value::List(vec![
            Value::String(self.flavor.into()),
            Value::String(name.map_or_else(|| "Empty".into(), |name| format!("Contains: {name}"))),
            Value::String(
                name.map_or("Right-click on a tamed creature to bind it", |_| hint)
                    .into(),
            ),
        ])
    }
}

pub fn start() -> Result<(), InterfaceUnavailable> {
    let _ = STORAGE.set(mobs::require_storage()?);
    moddedmc_api::entities::require()?
        .require_extension(moddedmc_api::entities::EntitiesExtension::ItemDestroyed)?;
    Ok(())
}

pub fn register(defaults: ItemDefaults, reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    ENTITIES
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?
        .on_item_destroyed(reg, destroyed)?;
    for binder in BINDERS {
        let carrier = "minecraft:paper";
        let stack = fastnbt::to_bytes(&Compound::from([
            ("id".into(), Value::String(carrier.into())),
            (
                "components".into(),
                Value::Compound(Compound::from([
                    ("minecraft:max_stack_size".into(), Value::Int(1)),
                    ("minecraft:lore".into(), binder.lore(None)),
                ])),
            ),
        ]))
        .map_err(|_| RegistrationError::InvalidDefinition)?;
        let model = format!("saintsdragons:item/{}", binder.item);
        defaults.add_item(
            reg,
            &Item {
                id: &format!("saintsdragons:{}", binder.item),
                display_name: &binder.display(),
                carrier,
                model: if cfg!(feature = "original-assets") {
                    &model
                } else {
                    "minecraft:item/echo_shard"
                },
            },
            &stack,
        )?;
    }
    Ok(())
}

pub fn names() -> impl Iterator<Item = (&'static str, String)> {
    BINDERS.iter().map(|binder| (binder.item, binder.display()))
}

fn find(item: &str) -> Option<&'static Binder> {
    let path = item.strip_prefix("saintsdragons:")?;
    BINDERS.iter().find(|binder| binder.item == path)
}

fn nested<'a>(map: &'a mut Compound, key: &str) -> Option<&'a mut Compound> {
    match map
        .entry(key.to_owned())
        .or_insert_with(|| Value::Compound(Compound::new()))
    {
        Value::Compound(map) => Some(map),
        _ => None,
    }
}

fn uuid(value: [u8; 16]) -> Value {
    Value::IntArray(IntArray::new(
        value
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(i32::from_be_bytes)
            .collect(),
    ))
}

fn read_uuid(value: &Value) -> Option<[u8; 16]> {
    let Value::IntArray(words) = value else {
        return None;
    };
    let words: &[i32; 4] = (&words[..]).try_into().ok()?;
    let mut bytes = [0; 16];
    for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        chunk.copy_from_slice(&word.to_be_bytes());
    }
    Some(bytes)
}

fn destroyed_identity(bytes: &[u8]) -> Option<([u8; 16], [u8; 16])> {
    let stack: Compound = fastnbt::from_bytes(bytes).ok()?;
    let Value::String(item) = stack.get("id")? else {
        return None;
    };
    let binder = find(item)?;
    let Value::Compound(components) = stack.get("components")? else {
        return None;
    };
    let Value::Compound(custom) = components.get("minecraft:custom_data")? else {
        return None;
    };
    if custom.get("IsBound") != Some(&Value::Byte(1)) {
        return None;
    }
    let dragon = custom.get("BoundDragonUUID")?;
    let Value::Compound(saved) = custom.get(binder.field)? else {
        return None;
    };
    if saved.get("UUID") != Some(dragon) {
        return None;
    }
    Some((
        read_uuid(custom.get("BoundOwnerUUID")?)?,
        read_uuid(dragon)?,
    ))
}

/// BinderComponentUtil.handleDestroyedBoundBinder.
fn destroyed(world: World<'_>, bytes: &[u8], _cause: &str) {
    if let Some((owner, dragon)) = destroyed_identity(bytes) {
        crate::codex::remove_record(world, owner, dragon);
    }
}

fn say(player: Entity<'_>, text: &str) {
    if let Some(mobs) = MOBS.get() {
        mobs.message(player, text, true);
    }
}

pub fn capture(click: &mut UseMob<'_>, item: &str) -> Option<UseResult> {
    let binder = find(item)?;
    let (Some(player), Some(mob), Some(mobs), Some(storage)) =
        (click.player, click.mob, MOBS.get(), STORAGE.get())
    else {
        return Some(UseResult::Fail);
    };
    if mobs.kind(mob) != Some(binder.dragon) {
        return None;
    }
    click.cooldown(item, CLICK_COOLDOWN);
    let _transfer = TRANSFER.lock().unwrap_or_else(PoisonError::into_inner);
    if !taming::is_owner(mob, player) {
        say(player, "You don't own this dragon");
        return Some(UseResult::Fail);
    }
    if !mobs.passengers(mob).is_empty() || mob.vehicle().is_some() {
        say(player, "This creature cannot be bound right now");
        return Some(UseResult::Fail);
    }
    Some(taming::with_attachment_lock(|| {
        if !mobs.passengers(mob).is_empty() || mob.vehicle().is_some() {
            return UseResult::Fail;
        }
        capture_stack(click.world, &mut click.stack, binder, player, mob, *storage)
            .unwrap_or(UseResult::Fail)
    }))
}

pub fn saved_name(mob: Entity<'_>) -> Option<String> {
    let bytes = STORAGE.get()?.save(mob)?;
    let nbt: Compound = fastnbt::from_bytes(&bytes).ok()?;
    match nbt.get("CustomName")? {
        Value::String(name) => Some(name.clone()),
        Value::Compound(display) => match display.get("text") {
            Some(Value::String(name)) => Some(name.clone()),
            _ => None,
        },
        _ => None,
    }
}

fn capture_stack(
    world: World<'_>,
    stack: &mut Stack<'_>,
    binder: &Binder,
    player: Entity<'_>,
    mob: Entity<'_>,
    storage: MobStorage,
) -> Option<UseResult> {
    if stack.count() != 1 {
        return None;
    }
    let owner = MOBS.get()?.uuid(player)?;
    let original = stack.read()?;
    let mut item: Compound = fastnbt::from_bytes(&original).ok()?;
    let components = nested(&mut item, "components")?;
    let custom = nested(components, "minecraft:custom_data")?;
    if matches!(custom.get("IsBound"), Some(Value::Byte(1))) || custom.contains_key(binder.field) {
        say(player, "This binder already contains a creature");
        return Some(UseResult::Fail);
    }
    let Some(bytes) = storage.save(mob) else {
        say(player, "This creature cannot be bound right now");
        return Some(UseResult::Fail);
    };
    let saved: Compound = fastnbt::from_bytes(&bytes).ok()?;
    let name = match saved.get("CustomName") {
        Some(Value::String(name)) => name.clone(),
        Some(Value::Compound(display)) => match display.get("text") {
            Some(Value::String(name)) => name.clone(),
            _ => binder.name.to_owned(),
        },
        _ => binder.name.to_owned(),
    };
    custom.insert("BoundDragonUUID".into(), saved.get("UUID")?.clone());
    custom.insert("BoundDragonName".into(), Value::String(name.clone()));
    custom.insert("BoundOwnerUUID".into(), uuid(owner));
    custom.insert("IsBound".into(), Value::Byte(1));
    custom.insert(binder.field.into(), Value::Compound(saved));
    components.insert(
        "minecraft:enchantment_glint_override".into(),
        Value::Byte(1),
    );
    components.insert("minecraft:lore".into(), binder.lore(Some(&name)));
    // Snapshot while the entity is still live; commit the record only after discard succeeds.
    let record = crate::codex::snapshot(world, mob, Some(&name));
    if !stack.write(&fastnbt::to_bytes(&item).ok()?) {
        return None;
    }
    if !mob.discard() {
        stack.write(&original);
        return None;
    }
    if let Some(record) = record {
        crate::codex::remember_record(world, record);
    }
    flight::forget(mob.id());
    say(player, &format!("Bound {name} in the binder!"));
    crate::advancements::award(player, "pocket_dragon", "bind_dragon");
    Some(UseResult::Success)
}

pub fn use_on_block(click: &mut UseOnBlock<'_>) -> Option<UseResult> {
    let binder = find(click.item)?;
    let player = click.player?;
    let at = [
        f64::from(click.position.x) + 0.5,
        f64::from(click.position.y) + 1.0,
        f64::from(click.position.z) + 0.5,
    ];
    let result = release(&mut click.stack, binder, player, click.world, at, false);
    if result != UseResult::Pass {
        click.cooldown(CLICK_COOLDOWN);
    }
    Some(result)
}

pub fn use_in_air(click: &mut UseItem<'_>) -> UseResult {
    let (Some(binder), Some(player)) = (
        find(click.item).filter(|binder| binder.flying),
        click.player,
    ) else {
        return UseResult::Pass;
    };
    let Some(view) = player.view() else {
        return UseResult::Fail;
    };
    let (yaw_sin, yaw_cos) = f64::from(click.yaw).to_radians().sin_cos();
    let (pitch_sin, pitch_cos) = f64::from(click.pitch).to_radians().sin_cos();
    let at = flight::add(
        view.position,
        [
            -yaw_sin * pitch_cos * 5.0,
            -pitch_sin * 5.0,
            yaw_cos * pitch_cos * 5.0,
        ],
    );
    release(&mut click.stack, binder, player, click.world, at, true)
}

fn release(
    stack: &mut Stack<'_>,
    binder: &Binder,
    player: Entity<'_>,
    world: World<'_>,
    at: [f64; 3],
    airborne: bool,
) -> UseResult {
    let _transfer = TRANSFER.lock().unwrap_or_else(PoisonError::into_inner);
    let Release {
        original,
        cleared,
        saved,
        name,
    } = match release_stacks(stack, binder, player) {
        Ok(Some(release)) => release,
        Ok(None) => return UseResult::Pass,
        Err(message) => {
            say(player, message);
            return UseResult::Fail;
        }
    };
    let (Some(storage), Some(entities)) = (STORAGE.get(), ENTITIES.get()) else {
        return UseResult::Fail;
    };
    if !stack.write(&cleared) {
        return UseResult::Fail;
    }
    let Some(id) = storage.restore(world, binder.dragon, &saved, at) else {
        stack.write(&original);
        say(
            player,
            &format!("Could not release {name} here. Try again nearby."),
        );
        return UseResult::Fail;
    };
    entities.with_entity(world, id, |dragon| {
        crate::codex::remember(world, dragon, None);
        if airborne {
            flight::released(dragon);
        }
    });
    say(player, &format!("Released {name} from the binder!"));
    UseResult::Success
}

struct Release {
    original: Vec<u8>,
    cleared: Vec<u8>,
    saved: Vec<u8>,
    name: String,
}

fn release_stacks(
    stack: &Stack<'_>,
    binder: &Binder,
    player: Entity<'_>,
) -> Result<Option<Release>, &'static str> {
    const CORRUPT: &str = "The binder data is corrupted";
    let Some(original) = stack.read() else {
        return Ok(None);
    };
    let mut item: Compound = fastnbt::from_bytes(&original).map_err(|_| CORRUPT)?;
    let components = nested(&mut item, "components").ok_or(CORRUPT)?;
    let custom = nested(components, "minecraft:custom_data").ok_or(CORRUPT)?;
    if custom.get("IsBound") != Some(&Value::Byte(1)) {
        return Ok(None);
    }
    if stack.count() != 1 {
        return Err(CORRUPT);
    }
    let owner = MOBS
        .get()
        .and_then(|mobs| mobs.uuid(player))
        .ok_or(CORRUPT)?;
    if custom.get("BoundOwnerUUID") != Some(&uuid(owner)) {
        return Err("You can only release dragons you own");
    }
    let Some(Value::Compound(mut saved)) = custom.remove(binder.field) else {
        return Err(CORRUPT);
    };
    if custom.get("BoundDragonUUID").is_none() || custom.get("BoundDragonUUID") != saved.get("UUID")
    {
        return Err(CORRUPT);
    }
    let name = match custom.remove("BoundDragonName") {
        Some(Value::String(name)) => name,
        _ => binder.name.to_owned(),
    };
    saved.remove("NoGravity");
    for key in ["BoundDragonUUID", "BoundOwnerUUID", "IsBound"] {
        custom.remove(key);
    }
    if custom.is_empty() {
        components.remove("minecraft:custom_data");
    }
    components.remove("minecraft:enchantment_glint_override");
    components.insert("minecraft:lore".into(), binder.lore(None));
    Ok(Some(Release {
        original,
        cleared: fastnbt::to_bytes(&item).map_err(|_| CORRUPT)?,
        saved: fastnbt::to_bytes(&saved).map_err(|_| CORRUPT)?,
        name,
    }))
}

#[cfg(test)]
mod destruction_tests {
    use super::*;

    #[test]
    fn only_real_filled_binder_with_matching_uuid_is_destroyed() {
        let owner = [1; 16];
        let dragon = [2; 16];
        let mut stack = Compound::from([
            (
                "id".into(),
                Value::String("saintsdragons:cindervane_binder".into()),
            ),
            ("count".into(), Value::Int(1)),
            (
                "components".into(),
                Value::Compound(Compound::from([(
                    "minecraft:custom_data".into(),
                    Value::Compound(Compound::from([
                        ("IsBound".into(), Value::Byte(1)),
                        ("BoundOwnerUUID".into(), uuid(owner)),
                        ("BoundDragonUUID".into(), uuid(dragon)),
                        (
                            "CindervaneData".into(),
                            Value::Compound(Compound::from([("UUID".into(), uuid(dragon))])),
                        ),
                    ])),
                )])),
            ),
        ]);
        assert_eq!(
            destroyed_identity(&fastnbt::to_bytes(&stack).unwrap()),
            Some((owner, dragon))
        );
        stack.insert("id".into(), Value::String("minecraft:paper".into()));
        assert_eq!(
            destroyed_identity(&fastnbt::to_bytes(&stack).unwrap()),
            None
        );
    }
}
