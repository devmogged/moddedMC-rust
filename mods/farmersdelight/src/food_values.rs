//! Source-verified 1.21.1 Farmer's Delight FoodValues reference data.
//! Not a 26.3 food registration layer: native default components are needed.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Clone, Debug, Deserialize)]
pub struct Effect {
    pub id: String,
    pub duration_ticks: u32,
    pub amplifier: u32,
    pub chance: f32,
    pub chance_literal: String,
    pub lambda: String,
    pub ambient: Option<bool>,
    pub show_particles: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FoodProfile {
    pub name: String,
    pub source: String,
    pub nutrition: Option<i32>,
    pub saturation_modifier: Option<f32>,
    pub saturation_literal: Option<String>,
    pub fast: Option<bool>,
    pub always_edible: Option<bool>,
    pub using_converts_to: Option<String>,
    #[serde(default)]
    pub effects: Vec<Effect>,
    pub constructor_java: Option<String>,
    pub requires_manual_java_1_21_1_record_semantic_review: Option<bool>,
    pub item_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct FoodManifest {
    pub upstream_commit: String,
    pub upstream_game_version: String,
    pub upstream_mod_version: String,
    pub reference_only: bool,
    pub profiles: Vec<FoodProfile>,
    pub vanilla_soup_overrides: Vec<FoodProfile>,
}

static MANIFEST: OnceLock<FoodManifest> = OnceLock::new();

/// The exact 1.21.1 source profiles, parsed once, never on an item-use hot path.
/// The KELP_ROLL explicit Java constructor is deliberately left unresolved.
pub fn reference() -> &'static FoodManifest {
    MANIFEST.get_or_init(|| {
        serde_json::from_str(include_str!("../food_values.json"))
            .expect("source food_values.json must be well-formed")
    })
}

pub fn profile(name: &str) -> Option<&'static FoodProfile> {
    reference().profiles.iter().find(|value| value.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_population_is_complete() {
        let manifest = reference();
        assert!(manifest.reference_only);
        assert_eq!(manifest.upstream_commit, "dd8b9b0");
        assert_eq!(manifest.upstream_game_version, "1.21.1");
        assert_eq!(manifest.upstream_mod_version, "1.3.4");
        assert_eq!(manifest.profiles.len(), 74);
        assert_eq!(manifest.vanilla_soup_overrides.len(), 3);
        assert_eq!(
            manifest
                .profiles
                .iter()
                .filter(|p| !p.effects.is_empty())
                .count(),
            37
        );
        assert_eq!(
            manifest
                .profiles
                .iter()
                .filter(|p| p.fast == Some(true))
                .count(),
            18
        );
        assert_eq!(
            manifest
                .profiles
                .iter()
                .filter(|p| p.always_edible == Some(true))
                .count(),
            3
        );
        let mut names: Vec<&str> = manifest.profiles.iter().map(|p| p.name.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 74);
    }

    #[test]
    fn raw_crops_match_java() {
        for (name, nutrition, saturation) in [
            ("CABBAGE", 2, 0.4_f32),
            ("TOMATO", 1, 0.3),
            ("ONION", 2, 0.4),
        ] {
            let food = profile(name).unwrap();
            assert_eq!(food.source, "FoodProperties.Builder");
            assert_eq!(food.nutrition, Some(nutrition));
            assert_eq!(food.saturation_modifier, Some(saturation));
            assert_eq!(food.fast, Some(false));
            assert_eq!(food.always_edible, Some(false));
            assert!(food.effects.is_empty());
        }
    }

    #[test]
    fn effect_chance_consumption_flags_and_bowl_remainder() {
        let dough = profile("WHEAT_DOUGH").unwrap();
        assert_eq!(dough.effects.len(), 1);
        let effect = &dough.effects[0];
        assert_eq!(effect.id, "minecraft:hunger");
        assert_eq!(effect.duration_ticks, 600);
        assert_eq!(effect.amplifier, 0);
        assert_eq!(effect.chance, 0.3_f32);
        assert!(effect.chance_literal.ends_with('F'));
        assert_eq!(effect.ambient, None);
        assert_eq!(effect.show_particles, None);

        let rice = profile("COOKED_RICE").unwrap();
        assert_eq!(rice.effects[0].id, "farmersdelight:nourishment");
        assert_eq!(rice.effects[0].duration_ticks, 600);
        assert_eq!(rice.effects[0].chance, 1.0);
        assert_eq!(rice.effects[0].show_particles, Some(false));

        let stew = profile("RABBIT_STEW_BUFF").unwrap();
        assert_eq!(stew.using_converts_to.as_deref(), Some("minecraft:bowl"));
        let cider = profile("APPLE_CIDER").unwrap();
        assert_eq!(cider.nutrition, None);
        assert_eq!(cider.always_edible, Some(true));
        assert_eq!(cider.effects[0].id, "minecraft:absorption");
    }

    #[test]
    fn explicit_constructor_is_not_guessed() {
        let kelp = profile("KELP_ROLL").unwrap();
        assert_eq!(kelp.source, "explicit_constructor");
        assert_eq!(
            kelp.requires_manual_java_1_21_1_record_semantic_review,
            Some(true)
        );
        assert_eq!(kelp.nutrition, None);
        assert!(
            kelp.constructor_java
                .as_ref()
                .is_some_and(|java| { java.starts_with("new FoodProperties(12, 12, false") })
        );
        assert!(
            reference()
                .vanilla_soup_overrides
                .iter()
                .any(|p| { p.item_id.as_deref() == Some("minecraft:rabbit_stew") })
        );
    }
}
