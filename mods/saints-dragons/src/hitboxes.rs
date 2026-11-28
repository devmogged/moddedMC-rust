//! Native dimensions from 0.9.88 ModEntities, independent of the visual carrier.
use moddedmc_api::{RegistrationError, Registry};

const PELLUCIDA: &str = "saintsdragons:draconian_pellucida";

// The builder's default eye height is 85% of the registered height.
const SIZES: &[(&str, f32, f32, bool, bool)] = &[
    ("raevyx", 4.0, 2.75, true, false),
    ("stegonaut", 2.75, 2.0, true, false),
    ("cindervane", 2.5, 2.5, true, false),
    ("varasuchus", 4.5, 3.5, true, false),
    ("ignivorus", 8.0, 6.0, true, false),
    ("volitans", 5.0, 4.0, true, false),
    ("nulljaw", 1.2, 1.15, true, false),
    ("atroxiia", 3.2, 4.75, true, false),
    ("latcher", 1.75, 1.75, false, true),
    ("winged", 1.5, 1.5, false, true),
    ("whettled", 1.5, 1.5, false, true),
    ("moop", 0.7, 0.35, false, false),
    ("mossback", 0.75, 0.55, false, false),
    ("ivy_oleander", 0.6, 1.95, false, false),
];

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let entities = crate::ENTITIES
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?;
    for &(name, width, height, immovable, swarm) in SIZES {
        entities.mob_geometry(
            registry,
            &format!("saintsdragons:{name}"),
            [width, height, height * 0.85],
            immovable,
            if swarm { PELLUCIDA } else { "" },
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_swarm_passes_pellucida_and_only_dragons_ignore_push() {
        assert_eq!(SIZES.iter().filter(|entry| entry.3).count(), 8);
        let swarm: Vec<_> = SIZES
            .iter()
            .filter(|entry| entry.4)
            .map(|entry| entry.0)
            .collect();
        assert_eq!(swarm, ["latcher", "winged", "whettled"]);
        assert!(SIZES.iter().all(|entry| entry.1 > 0.0 && entry.2 > 0.0));
    }
}
