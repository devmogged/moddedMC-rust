//! Container loot rolls without consuming a vanilla block entity's pending table.
use moddedmc_api::abi::{self, PlayerRef, Pos, Sink, Str, WorldRef};
use pumpkin_core::{
    entity::EntityBase,
    world::loot::{LootContextParameters, generate_loot_from_handle, place_items_in_slots},
};
use pumpkin_data::{attributes::Attributes, item_stack::ItemStack};
use pumpkin_nbt::Nbt;
use pumpkin_util::math::vector3::Vector3;

pub static CONTAINER_LOOT: abi::ContainerLootV1 = abi::ContainerLootV1 {
    size: size_of::<abi::ContainerLootV1>(),
    generate,
};

unsafe extern "C" fn generate(
    raw: WorldRef,
    player: PlayerRef,
    key: Str,
    seed: i64,
    pos: Pos,
    size: usize,
    sink: Sink,
) -> bool {
    if !(1..=54).contains(&size)
        || key.len == 0
        || key.len > 256
        || pos.x.unsigned_abs() > 30_000_000
        || pos.z.unsigned_abs() > 30_000_000
    {
        return false;
    }
    // SAFETY: the caller borrows both handles and the loot identifier.
    let (Some(world), Some(player)) = (unsafe { crate::content::world(raw) }, unsafe {
        crate::native::player(player)
    }) else {
        return false;
    };
    if !std::ptr::eq(world, player.world().as_ref()) {
        return false;
    }
    let Some(table) = world.get_loot_table(unsafe { key.str() }) else {
        return false;
    };
    let params = LootContextParameters {
        world: Some(world),
        position: Some(Vector3::new(
            f64::from(pos.x) + 0.5,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + 0.5,
        )),
        this_entity: Some(player as &dyn EntityBase),
        luck: player.living_entity.get_attribute_value(&Attributes::LUCK) as f32,
        ..Default::default()
    };
    let mut slots = vec![ItemStack::EMPTY.clone(); size];
    place_items_in_slots(
        &mut slots,
        generate_loot_from_handle(&table, seed, &params),
        seed,
    );
    let mut encoded = Vec::new();
    for (i, stack) in slots.iter().enumerate() {
        if stack.is_empty() {
            continue;
        }
        let Some(nbt) = stack.to_nbt() else {
            return false;
        };
        let bytes = Nbt::from(nbt).write();
        if bytes.len() > abi::MAX_STACK_BYTES {
            return false;
        }
        encoded.push((i as u64, bytes));
    }
    for (i, bytes) in encoded {
        // SAFETY: the bytes remain live through the synchronous sink.
        unsafe { (sink.put)(sink.ctx, i, Str::new(&bytes)) };
    }
    true
}
