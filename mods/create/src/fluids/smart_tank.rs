//! Single-segment SmartFluidTankBehaviour storage.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{Pos, content::World};

use super::tank::TankInventory;

const TANKS: &str = "Tanks";

pub fn read(world: World<'_>, pos: Pos) -> Option<HashMap<String, Value>> {
    fastnbt::from_bytes(&world.entity(pos)?).ok()
}

pub fn load(data: &HashMap<String, Value>, capacity: u32) -> Option<TankInventory> {
    let mut tank = TankInventory::new(capacity, 1).ok()?;
    if let Some(tanks) = data.get(TANKS) {
        let Value::List(tanks) = tanks else {
            return None;
        };
        let Some(Value::Compound(first)) = tanks.first() else {
            return None;
        };
        tank.read_content_nbt(first.get("TankContent")?).ok()?;
    }
    Some(tank)
}

pub fn store(data: &mut HashMap<String, Value>, tank: &TankInventory) -> bool {
    let segments = data
        .entry(TANKS.to_owned())
        .or_insert_with(|| Value::List(vec![Value::Compound(HashMap::new())]));
    let Value::List(segments) = segments else {
        return false;
    };
    let Some(Value::Compound(segment)) = segments.first_mut() else {
        return false;
    };
    segment.insert("TankContent".to_owned(), tank.content_nbt());
    true
}

pub fn write(world: World<'_>, pos: Pos, data: &HashMap<String, Value>) -> bool {
    fastnbt::to_bytes(data).is_ok_and(|bytes| world.set_entity(pos, &bytes))
}

pub fn save(world: World<'_>, pos: Pos, tank: &TankInventory) -> bool {
    read(world, pos).is_some_and(|mut data| store(&mut data, tank) && write(world, pos, &data))
}
