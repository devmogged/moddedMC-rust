//! Native content block comparator fixture.
use std::sync::OnceLock;

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Pos, RegistrationError, Registry,
    content::{self, Block, Content, State, WorldExtension},
};

const BLOCK_ID: &str = "abi_analog:source";
static CONTENT: OnceLock<Content> = OnceLock::new();

struct Analog;

impl Mod for Analog {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_extension(WorldExtension::AnalogOutput)?;
        content.require_extension(WorldExtension::Dimension)?;
        let _ = CONTENT.set(content);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let api = CONTENT.get().ok_or(RegistrationError::InvalidDefinition)?;
        let states = [State {
            properties: "",
            physical: "minecraft:stone",
            model: "minecraft:block/stone",
            rotating_model: "",
            x: 0,
            y: 0,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape: &[],
        }];
        api.add_block(
            registry,
            &Block {
                id: BLOCK_ID,
                display_name: "Analog Source",
                entity: BLOCK_ID,
                item_model: "minecraft:block/stone",
                states: &states,
            },
            None,
        )?;
        api.add_analog_output(registry, BLOCK_ID)?;
        assert!(api.add_analog_output(registry, BLOCK_ID).is_err());
        assert!(api.add_analog_output(registry, "minecraft:stone").is_err());
        registry.add_command("abi_analog_0", "Reset the first source", 0)?;
        registry.add_command("abi_analog_7", "Set the first source to seven", 7)?;
        registry.add_command("abi_analog_13", "Set the second source to thirteen", 13)?;
        registry.add_command("abi_analog_16", "Reject strength above 15", 16)?;
        registry.add_command("abi_analog_air", "Reject a missing source", 101)?;
        registry.add_command("abi_analog_dimension", "Report current world dimension", 102)?;
        Ok(())
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        let Some(api) = CONTENT.get().copied() else {
            return 0;
        };
        let Some(world) = api.command_world(&command) else {
            return 0;
        };
        if tag == 102 {
            let actual = world.dimension().unwrap_or("<missing>");
            moddedmc_api::info(&format!("abi_analog dimension={actual}"));
            return 1;
        }
        let (x, y, z, strength) = match tag {
            0 => (1025, 100, 1025, 0),
            7 => (1025, 100, 1025, 7),
            13 => (1035, 100, 1025, 13),
            16 => (1025, 100, 1025, 16),
            101 => (1038, 100, 1025, 3),
            _ => return 0,
        };
        let pos = Pos { x, y, z };
        let ok = world.set_analog_output(pos, strength);
        moddedmc_api::info(&format!(
            "abi_analog set {x} {y} {z} strength={strength} ok={ok}"
        ));
        1
    }
}

moddedmc_api::export_mod!(Analog, id = "abi_analog");
