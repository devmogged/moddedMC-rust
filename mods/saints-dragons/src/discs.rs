use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    items::{Item, ItemDefaults},
};

use crate::worldroot::compound;

const DISCS: &[(&str, &str, i32, &str, &str)] = &[
    (
        "bleeding_bolt_music_disc",
        "bleeding_bolt",
        104,
        "rare",
        "Bleeding Bolt - By Saint",
    ),
    (
        "mossback_music_music_disc",
        "mossback_music",
        42,
        "common",
        "Mossback Music",
    ),
];

pub fn names() -> impl Iterator<Item = (&'static str, &'static str)> {
    DISCS
        .iter()
        .map(|&(path, _, _, _, description)| (path, description))
}

pub fn register(
    defaults: ItemDefaults,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    for &(path, sound, length, rarity, description) in DISCS {
        let song = compound([
            (
                "sound_event",
                compound([("sound_id", Value::String(format!("saintsdragons:{sound}")))]),
            ),
            ("description", Value::String(description.to_owned())),
            ("length_in_seconds", Value::Int(length)),
            ("comparator_output", Value::Int(1)),
        ]);
        let components = HashMap::from([
            ("minecraft:jukebox_playable".to_owned(), song),
            (
                "minecraft:rarity".to_owned(),
                Value::String(rarity.to_owned()),
            ),
            ("minecraft:max_stack_size".to_owned(), Value::Int(1)),
        ]);
        let nbt = fastnbt::to_bytes(&compound([
            ("id", Value::String("minecraft:music_disc_13".into())),
            ("components", Value::Compound(components)),
        ]))
        .map_err(|_| RegistrationError::InvalidDefinition)?;
        let model = if cfg!(feature = "original-assets") {
            format!("saintsdragons:item/{path}")
        } else {
            "minecraft:item/music_disc_13".to_owned()
        };
        defaults.add_item(
            registry,
            &Item {
                id: &format!("saintsdragons:{path}"),
                display_name: "Music Disc",
                carrier: "minecraft:music_disc_13",
                model: &model,
            },
            &nbt,
        )?;
    }
    Ok(())
}
