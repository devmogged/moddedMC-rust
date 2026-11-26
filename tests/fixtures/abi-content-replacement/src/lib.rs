//! Exercise exact stale-incarnation and changed-NBT guards through the ABI,
//! independently of the Lootr lifecycle scheduler.
use fastnbt::Value;
use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    content::{self, Content, EntitySnapshot, WorldExtension},
};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

static CONTENT: OnceLock<Content> = OnceLock::new();
static CAPTURED: Mutex<Option<EntitySnapshot>> = Mutex::new(None);
const POS: Pos = Pos {
    x: 1025,
    y: 100,
    z: 1025,
};

fn equal_nbt(a: &[u8], b: &[u8]) -> bool {
    match (
        fastnbt::from_bytes::<HashMap<String, Value>>(a),
        fastnbt::from_bytes::<HashMap<String, Value>>(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

struct Fixture;

impl Mod for Fixture {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_extension(WorldExtension::ContentEntityTransaction)?;
        content.require_extension(WorldExtension::ContentEntityReplacement)?;
        let _ = CONTENT.set(content);
        Ok(())
    }

    fn register(&self, reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
        reg.add_command(
            "abi_replacement_capture",
            "Capture exact custom entity snapshot",
            1,
        )?;
        reg.add_command("abi_replacement_stale", "Attempt old snapshot swap", 2)?;
        reg.add_command("abi_replacement_changed", "Attempt old metadata swap", 3)?;
        reg.add_command("abi_replacement_fresh", "Attempt fresh snapshot swap", 4)
    }

    fn command(&self, tag: u64, cmd: Command<'_>) -> i32 {
        let Some(content) = CONTENT.get() else {
            return 0;
        };
        let Some(world) = content.command_world(&cmd) else {
            return 0;
        };
        let Some(mut current) = world.entity_snapshot(POS) else {
            return 0;
        };
        let Ok(mut captured) = CAPTURED.lock() else {
            return 0;
        };
        if tag == 1 {
            *captured = Some(current);
            moddedmc_api::info("abi replacement captured");
            return 1;
        }
        let Some(old) = captured.as_ref() else {
            return 0;
        };
        let Some(air) = content.state("minecraft:air") else {
            return 0;
        };
        if tag == 3 {
            // /data merge reconstructs the entity, changing incarnation.
            // Use the guarded metadata-only API to isolate the NBT check.
            let Ok(mut fields) = fastnbt::from_bytes::<HashMap<String, Value>>(&current.nbt) else {
                return 0;
            };
            fields.insert("AbiTestChanged".into(), Value::Int(8));
            let Ok(replacement) = fastnbt::to_bytes(&fields) else {
                return 0;
            };
            if !world.compare_entity(POS, &current, &replacement) {
                return 0;
            }
            let Some(updated) = world.entity_snapshot(POS) else {
                return 0;
            };
            current = updated;
        }
        if tag == 4 {
            let committed = world.compare_replace_entity(POS, &current, air, None);
            let removed = world.entity_snapshot(POS).is_none();
            moddedmc_api::info(&format!(
                "abi replacement fresh committed={committed} removed={removed}"
            ));
            return i32::from(committed && removed);
        }
        let same_state = old.state == current.state;
        let same_incarnation = old.incarnation == current.incarnation;
        let same_nbt = equal_nbt(&old.nbt, &current.nbt);
        let committed = world.compare_replace_entity(POS, old, air, None);
        let preserved = world.entity_snapshot(POS).is_some_and(|after| {
            after.state == current.state
                && after.incarnation == current.incarnation
                && equal_nbt(&after.nbt, &current.nbt)
        });
        moddedmc_api::info(&format!(
            "abi replacement guard same_state={same_state} same_incarnation={same_incarnation} same_nbt={same_nbt} committed={committed} preserved={preserved}"
        ));
        i32::from(
            same_state
                && !committed
                && preserved
                && match tag {
                    2 => !same_incarnation && same_nbt,
                    3 => same_incarnation && !same_nbt,
                    _ => false,
                },
        )
    }
}

moddedmc_api::export_mod!(Fixture, id = "abi_content_replacement");
