//! Saved dragon appearance, spawn-egg variants and Creative appearance tools.

use moddedmc_api::{
    entities::Entity,
    interaction::{UseMob, UseResult},
};
use serde::{Deserialize, Serialize};

use crate::{MOBS, ai::random, visuals};

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

fn spawn_variant(species: &str, in_end: bool, roll: u64) -> u8 {
    u8::from(match species {
        "cindervane" | "volitans" => roll >= 85,
        "raevyx" => roll >= 90,
        "ignivorus" => roll >= 95,
        "varasuchus" => in_end,
        _ => false,
    })
}

pub fn spawned_from_egg(mob: Entity<'_>, species: &str, in_end: bool) {
    if variants(species).is_none() {
        return;
    }
    let appearance = Appearance {
        female: random(2) == 1,
        variant: spawn_variant(species, in_end, random(100)),
    };
    if appearance.save(mob) {
        visuals::invalidate_appearance(mob.id());
    }
}

/// `DragonEntity.finalizeSpawn` for an adult that joined without an appearance: a natural or
/// chunk generation spawn.
pub fn ensure(mob: Entity<'_>, species: &str, in_end: bool) {
    if mob.data(DATA).is_none() {
        spawned_from_egg(mob, species, in_end);
    }
}

#[derive(Deserialize)]
struct TemplateAppearance<'a> {
    id: &'a str,
    #[serde(rename = "IsFemale", default)]
    female: bool,
    #[serde(rename = "TextureVariantId", default)]
    variant: Option<&'a str>,
    #[serde(rename = "Pos")]
    position: [f64; 3],
}

pub fn spawned_from_template(mob: Entity<'_>, nbt: &[u8]) {
    let Ok(data) = fastnbt::from_bytes::<TemplateAppearance<'_>>(nbt) else {
        return;
    };
    crate::roost::set_home(mob, data.position);
    let Some(variants) = data.id.strip_prefix("saintsdragons:").and_then(variants) else {
        return;
    };
    let variant = data
        .variant
        .and_then(|id| id.strip_prefix("saintsdragons:"))
        .and_then(|name| variants.iter().position(|candidate| *candidate == name))
        .unwrap_or_default() as u8;
    Appearance {
        female: data.female,
        variant,
    }
    .save(mob);
}

impl Appearance {
    pub fn load(mob: Entity<'_>) -> Self {
        mob.data(DATA)
            .and_then(|nbt| fastnbt::from_bytes(&nbt).ok())
            .unwrap_or_default()
    }

    fn save(self, mob: Entity<'_>) -> bool {
        fastnbt::to_bytes(&self).is_ok_and(|nbt| mob.set_data(DATA, &nbt))
    }

    pub fn is_female(mob: Entity<'_>) -> bool {
        Self::load(mob).female
    }

    pub fn set_female(female: bool, mob: Entity<'_>) {
        let appearance = Self {
            female,
            ..Self::default()
        };
        appearance.save(mob);
    }

    pub fn mature_with_variant(mob: Entity<'_>, variant: u8) -> bool {
        let mut appearance = Self::load(mob);
        appearance.variant = variant;
        let saved = appearance.save(mob);
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
    if !appearance.save(mob) {
        return UseResult::Fail;
    }
    visuals::invalidate_appearance(mob.id());
    mobs.message(player, &message, true);
    UseResult::Success
}

#[cfg(test)]
mod tests {
    use super::spawn_variant;

    #[test]
    fn spawn_egg_variants_follow_the_released_weights() {
        for (species, rare_threshold) in [
            ("cindervane", 85),
            ("volitans", 85),
            ("raevyx", 90),
            ("ignivorus", 95),
        ] {
            assert_eq!(spawn_variant(species, false, rare_threshold - 1), 0);
            assert_eq!(spawn_variant(species, false, rare_threshold), 1);
            assert_eq!(spawn_variant(species, false, 99), 1);
        }
        assert_eq!(spawn_variant("varasuchus", false, 99), 0);
        assert_eq!(spawn_variant("varasuchus", true, 0), 1);
        assert_eq!(spawn_variant("stegonaut", true, 99), 0);
    }
}
