//! Lootr 1.25.42.124 content port.
//! Original source: LootrMinecraft/Lootr, commit 2a9c830, TrophyBlock.java.

pub mod inventory_store;

use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, Mod, RegistrationError, Registry,
    content::{self, Block, Content, Placement, ShapeBox, State},
};

static CONTENT: OnceLock<Content> = OnceLock::new();

const TROPHY_ID: &str = "lootr:trophy";

// The C ABI stores shape coordinates in integral sixteenths. Upstream uses half-sixteenths.
const NORTH_SOUTH: [ShapeBox; 1] = [ShapeBox {
    min: [4, 0, 2],
    max: [12, 15, 14],
}];
const EAST_WEST: [ShapeBox; 1] = [ShapeBox {
    min: [2, 0, 4],
    max: [14, 15, 12],
}];

fn facing_from_yaw(yaw: f32) -> &'static str {
    // Minecraft's Direction.fromYRot(yaw), then the opposite facing for TrophyBlock placement.
    let quadrant = ((f64::from(yaw) / 90.0 + 0.5).floor() as i64).rem_euclid(4);
    match quadrant {
        0 => "north",
        1 => "east",
        2 => "south",
        _ => "west",
    }
}

fn placed_state(args: Placement<'_>) -> Option<u32> {
    CONTENT.get()?.state(&format!(
        "{TROPHY_ID}[facing={}]",
        facing_from_yaw(args.yaw)
    ))
}

struct Lootr;

impl Mod for Lootr {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let _ = CONTENT.set(content::require()?);
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        CONTENT
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?
            .add_block(
                registry,
                &Block {
                    id: TROPHY_ID,
                    display_name: "Lootr Trophy",
                    entity: "",
                    item_model: "lootr:item/trophy",
                    states: &[
                        State {
                            properties: "facing=north",
                            physical: "minecraft:stone",
                            model: "lootr:block/trophy",
                            rotating_model: "",
                            x: 0,
                            y: 0,
                            orientation: [0.0, 0.0, 0.0, 1.0],
                            shape: &NORTH_SOUTH,
                        },
                        State {
                            properties: "facing=east",
                            physical: "minecraft:stone",
                            model: "lootr:block/trophy",
                            rotating_model: "",
                            x: 0,
                            y: 0,
                            orientation: [
                                0.0,
                                -std::f32::consts::FRAC_1_SQRT_2,
                                0.0,
                                std::f32::consts::FRAC_1_SQRT_2,
                            ],
                            shape: &EAST_WEST,
                        },
                        State {
                            properties: "facing=south",
                            physical: "minecraft:stone",
                            model: "lootr:block/trophy",
                            rotating_model: "",
                            x: 0,
                            y: 0,
                            orientation: [0.0, 1.0, 0.0, 0.0],
                            shape: &NORTH_SOUTH,
                        },
                        State {
                            properties: "facing=west",
                            physical: "minecraft:stone",
                            model: "lootr:block/trophy",
                            rotating_model: "",
                            x: 0,
                            y: 0,
                            orientation: [
                                0.0,
                                std::f32::consts::FRAC_1_SQRT_2,
                                0.0,
                                std::f32::consts::FRAC_1_SQRT_2,
                            ],
                            shape: &EAST_WEST,
                        },
                    ],
                },
                Some(placed_state),
            )
    }
}

moddedmc_api::export_mod!(Lootr);

#[cfg(test)]
mod tests {
    use super::facing_from_yaw;

    #[test]
    fn trophy_is_placed_facing_against_the_player_direction() {
        assert_eq!(facing_from_yaw(0.0), "north");
        assert_eq!(facing_from_yaw(90.0), "east");
        assert_eq!(facing_from_yaw(180.0), "south");
        assert_eq!(facing_from_yaw(-90.0), "west");
        assert_eq!(facing_from_yaw(270.0), "west");
    }

    #[test]
    fn trophy_yaw_quantization_wraps_at_cardinal_boundaries() {
        assert_eq!(facing_from_yaw(44.9), "north");
        assert_eq!(facing_from_yaw(45.0), "east");
        assert_eq!(facing_from_yaw(-44.9), "north");
        assert_eq!(facing_from_yaw(-45.0), "north");
        assert_eq!(facing_from_yaw(405.0), "east");
    }
}
