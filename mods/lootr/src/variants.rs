//! Source-matched block identities and state mapping. Runtime IDs are resolved
//! once after the registry freezes; no string work on menu validity checks.
use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, BlockMaterial, Content, Placement, ShapeBox, State},
};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Variant {
    pub id: &'static str,
    pub vanilla: &'static str,
    pub physical: &'static str,
    pub title: &'static str,
    pub entity: &'static str,
}

pub(crate) const VARIANTS: [Variant; 7] = [
    Variant {
        id: "lootr:chest",
        vanilla: "minecraft:chest",
        physical: "minecraft:chest",
        title: "Loot Chest",
        entity: "lootr:chest",
    },
    Variant {
        id: "lootr:trapped_chest",
        vanilla: "minecraft:trapped_chest",
        physical: "minecraft:trapped_chest",
        title: "Loot Chest",
        entity: "lootr:trapped_chest",
    },
    Variant {
        id: "lootr:barrel",
        vanilla: "minecraft:barrel",
        physical: "minecraft:barrel",
        title: "Loot Barrel",
        entity: "lootr:barrel",
    },
    Variant {
        id: "lootr:copper_chest",
        vanilla: "minecraft:copper_chest",
        physical: "minecraft:waxed_copper_chest",
        title: "Copper Loot Chest",
        entity: "lootr:chest",
    },
    Variant {
        id: "lootr:exposed_copper_chest",
        vanilla: "minecraft:exposed_copper_chest",
        physical: "minecraft:waxed_exposed_copper_chest",
        title: "Exposed Copper Loot Chest",
        entity: "lootr:chest",
    },
    Variant {
        id: "lootr:weathered_copper_chest",
        vanilla: "minecraft:weathered_copper_chest",
        physical: "minecraft:waxed_weathered_copper_chest",
        title: "Weathered Copper Loot Chest",
        entity: "lootr:chest",
    },
    Variant {
        id: "lootr:oxidized_copper_chest",
        vanilla: "minecraft:oxidized_copper_chest",
        physical: "minecraft:waxed_oxidized_copper_chest",
        title: "Oxidized Copper Loot Chest",
        entity: "lootr:chest",
    },
];
const FACES: [&str; 6] = ["north", "east", "south", "west", "up", "down"];
const SHAPE: [ShapeBox; 1] = [ShapeBox {
    min: [1, 0, 1],
    max: [15, 14, 15],
}];

impl Variant {
    pub fn barrel(self) -> bool {
        self.id == "lootr:barrel"
    }

    fn properties(self, face: &str, flag: bool) -> String {
        if self.barrel() {
            format!("facing={face},open={flag}")
        } else {
            format!("facing={face},type=single,waterlogged={flag}")
        }
    }

    fn faces(self) -> &'static [&'static str] {
        if self.barrel() { &FACES } else { &FACES[..4] }
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for variant in VARIANTS {
        let properties: Vec<_> = variant
            .faces()
            .iter()
            .flat_map(|face| [false, true].map(|flag| variant.properties(face, flag)))
            .collect();
        let physical: Vec<_> = properties
            .iter()
            .map(|p| format!("{}[{p}]", variant.physical))
            .collect();
        let model = if variant.barrel() {
            "minecraft:block/barrel"
        } else {
            "minecraft:block/chest"
        };
        let item_model = format!(
            "minecraft:item/{}",
            variant.physical.trim_start_matches("minecraft:")
        );
        let states: Vec<_> = properties
            .iter()
            .zip(&physical)
            .map(|(properties, physical)| State {
                properties,
                physical,
                model,
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0., 0., 0., 1.],
                shape: if variant.barrel() { &[] } else { &SHAPE },
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id: variant.id,
                display_name: variant.title,
                entity: variant.entity,
                item_model: &item_model,
                states: &states,
            },
            Some(placed),
        )?;
        content.set_block_material(
            registry,
            variant.id,
            &BlockMaterial {
                template: variant.physical,
                hardness: Some(2.5),
                map_color: None,
            },
        )?;
        // Java's default powerComparators=true yields strength one irrespective
        // of inventory contents. Trapped direct/weak power is a separate hook.
        for properties in &properties {
            content.set_comparator_output(registry, &format!("{}[{properties}]", variant.id), 1)?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Mapping {
    variant: Variant,
    custom: u32,
    original: u32,
    physical: u32,
}
static STATES: OnceLock<Vec<Mapping>> = OnceLock::new();

fn mapping(content: Content) -> Option<&'static [Mapping]> {
    if let Some(states) = STATES.get() {
        return Some(states);
    }
    let mut states = Vec::new();
    for variant in VARIANTS {
        for face in variant.faces() {
            for flag in [false, true] {
                let properties = variant.properties(face, flag);
                states.push(Mapping {
                    variant,
                    custom: content.state(&format!("{}[{properties}]", variant.id))?,
                    original: content.state(&format!("{}[{properties}]", variant.vanilla))?,
                    physical: content.state(&format!("{}[{properties}]", variant.physical))?,
                });
            }
        }
    }
    let _ = STATES.set(states);
    STATES.get().map(Vec::as_slice)
}

pub(crate) fn kind(content: Content, state: u32) -> Option<Variant> {
    mapping(content)?
        .iter()
        .find(|m| m.custom == state)
        .map(|m| m.variant)
}

pub(crate) fn converted(content: Content, source: u32) -> Option<u32> {
    mapping(content)?
        .iter()
        .find(|m| m.original == source || m.physical == source)
        .map(|m| m.custom)
}

fn placed(args: Placement<'_>) -> Option<u32> {
    let content = super::API.get()?.content;
    let variant = kind(content, args.state)?;
    let facing = super::placement_facing(args.yaw)?;
    let face = if variant.barrel() {
        if !args.pitch.is_finite() {
            return None;
        }
        let (sin, cos) = args.pitch.to_radians().sin_cos();
        let (yaw_sin, yaw_cos) = args.yaw.to_radians().sin_cos();
        // Opposite the nearest looking direction, including vertical.
        if sin.abs() > (cos * yaw_sin).abs().max((cos * yaw_cos).abs()) {
            if sin > 0. { "up" } else { "down" }
        } else {
            FACES[facing]
        }
    } else {
        FACES[facing]
    };
    let flag =
        !variant.barrel() && args.world.fluid_id(args.position)?.as_str() == "minecraft:water";
    content.state(&format!(
        "{}[{}]",
        variant.id,
        variant.properties(face, flag)
    ))
}
