use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, Content, State},
};

const BLOCKS: &[(&str, &str, &str)] = &[
    (
        "dragonheart_alloy_block",
        "Dragonheart Alloy Block",
        "netherite_block",
    ),
    ("dragonheart_block", "Dragonheart Block", "netherite_block"),
    (
        "deepslate_worldroot_ore",
        "Deepslate Worldroot Ore",
        "deepslate_diamond_ore",
    ),
    ("dragonheart_ore", "Dragonheart Ore", "end_stone"),
    ("worldroot_block", "Worldroot Block", "iron_block"),
    (
        "raw_worldroot_block",
        "Raw Worldroot Block",
        "raw_iron_block",
    ),
    (
        "draconian_pellucida",
        "Draconian Pellucida",
        "purple_stained_glass",
    ),
    (
        "ignivorus_incubator_block",
        "Ignivorus Incubator",
        "raw_iron_block",
    ),
];

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    BLOCKS.iter().map(|&(name, display, _)| (name, display))
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for &(name, display_name, carrier) in BLOCKS {
        let physical = format!("minecraft:{carrier}");
        let model = if cfg!(feature = "original-assets") {
            format!("saintsdragons:block/{name}")
        } else {
            format!("minecraft:block/{carrier}")
        };
        content.add_block(
            registry,
            &Block {
                id: &format!("saintsdragons:{name}"),
                display_name,
                entity: "",
                item_model: &model,
                states: &[State {
                    properties: "",
                    physical: &physical,
                    model: &model,
                    rotating_model: "",
                    x: 0,
                    y: 0,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }],
            },
            None,
        )?;
    }
    Ok(())
}
