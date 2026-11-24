//! Creative controls for the adult dragons' saved appearance.

use moddedmc_api::{
    entities::Entity,
    interaction::{UseMob, UseResult},
};
use serde::{Deserialize, Serialize};

use crate::{MOBS, visuals};

pub const GENDER_WAND: &str = "saintsdragons:creative_dragon_gender_wand";
pub const VARIANT_BRUSH: &str = "saintsdragons:creative_dragon_variant_brush";
const DATA: &str = "saintsdragons:appearance";

#[derive(Clone, Copy, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Appearance {
    female: bool,
    variant: u8,
}

fn variants(species: &str) -> Option<&'static [&'static str]> {
    Some(match species {
        "cindervane" => &["default", "albino", "piebald"],
        "ignivorus" => &["default", "crimson"],
        "raevyx" => &["default", "night_gold"],
        "varasuchus" => &["default", "void_kissed"],
        "volitans" => &["default", "bloodshot"],
        "atroxiia" | "nulljaw" | "stegonaut" => &["default"],
        _ => return None,
    })
}

impl Appearance {
    pub fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|nbt| fastnbt::from_bytes(&nbt).ok())
            .unwrap_or_default()
    }

    pub fn set_female(female: bool, mob: Entity<'_>) {
        let appearance = Self {
            female,
            ..Self::default()
        };
        if let Ok(nbt) = fastnbt::to_bytes(&appearance) {
            mob.set_data(DATA, &nbt);
        }
    }

    pub fn mature_with_variant(mob: Entity<'_>, variant: u8) -> bool {
        let mut appearance = Self::load(mob);
        appearance.variant = variant;
        let saved = fastnbt::to_bytes(&appearance).is_ok_and(|nbt| mob.set_data(DATA, &nbt));
        if saved {
            visuals::invalidate_appearance(mob.id());
        }
        saved
    }

    pub fn texture(self, species: &str) -> Option<String> {
        let variant = variants(species)?.get(usize::from(self.variant)).copied()?;
        match (variant, self.female) {
            ("default", false) => None,
            (name, false) => Some(name.to_owned()),
            (name, true) => Some(format!("{name}_female")),
        }
    }
}

pub fn use_tool(click: &UseMob<'_>, item: &str) -> UseResult {
    let (Some(mobs), Some(player), Some(mob)) = (MOBS.get(), click.player, click.mob) else {
        return UseResult::Pass;
    };
    let Some(kind) = mobs.kind(mob) else {
        return UseResult::Pass;
    };
    let Some(variants) = kind.strip_prefix("saintsdragons:").and_then(variants) else {
        return UseResult::Pass;
    };
    if !click.creative {
        mobs.message(player, "This tool requires Creative mode.", true);
        return UseResult::Fail;
    }

    let mut appearance = Appearance::load(mob);
    let message = if item == GENDER_WAND {
        appearance.female = !appearance.female;
        if appearance.female {
            "Dragon gender set to female."
        } else {
            "Dragon gender set to male."
        }
        .to_owned()
    } else {
        let next = variants
            .get(usize::from(appearance.variant))
            .map_or(0, |_| {
                (usize::from(appearance.variant) + 1) % variants.len()
            });
        appearance.variant = next as u8;
        format!(
            "Dragon variant set to {}.",
            variants[next].replace('_', " ")
        )
    };
    let Ok(nbt) = fastnbt::to_bytes(&appearance) else {
        return UseResult::Fail;
    };
    if !mob.set_data(DATA, &nbt) {
        return UseResult::Fail;
    }
    visuals::invalidate_appearance(mob.id());
    mobs.message(player, &message, true);
    UseResult::Success
}
