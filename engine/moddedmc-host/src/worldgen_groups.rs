//! Ordered structure set membership for group-based locator mods.
use moddedmc_api::abi::{self, Sink, Str, WorldRef};
use pumpkin_data::{structure_registry::full_id, structures::StructureSet};

pub static GROUPS: abi::WorldgenStructureGroupsV1 = abi::WorldgenStructureGroupsV1 {
    size: size_of::<abi::WorldgenStructureGroupsV1>(),
    groups,
};

unsafe extern "C" fn groups(raw: WorldRef, out: Sink) -> bool {
    // SAFETY: the caller borrows the world.
    if unsafe { crate::content::world(raw) }.is_none() {
        return false;
    }
    let names = StructureSet::names();
    let sets = StructureSet::all();
    if names.len() != sets.len() {
        return false;
    }
    // Include disabled sets: group membership is registry-wide, not specific
    // to a dimension. Placement queries separately filter active sets.
    for (index, (name, set)) in names.iter().zip(sets).enumerate() {
        let mut record = full_id(name);
        for entry in set.structures {
            record.push('\0');
            record.push_str(&full_id(entry.structure.to_name()));
        }
        // SAFETY: record stays alive until the synchronous sink returns.
        unsafe { (out.put)(out.ctx, index as u64, Str::new(record.as_bytes())) };
    }
    true
}
