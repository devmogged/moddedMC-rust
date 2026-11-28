//! RaevyxSummonStormAbility and RaevyxStormSavedData, 0.9.88.
use std::sync::OnceLock;

use moddedmc_api::{
    InterfaceUnavailable, RegistrationError, Registry,
    content::{WeatherState, World},
    saved_data::{self, Key, SavedData},
};
use serde::{Deserialize, Serialize};

static SAVES: OnceLock<SavedData> = OnceLock::new();
static KEY: OnceLock<Key> = OnceLock::new();

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Provenance {
    summoned_storm: bool,
    natural_thunder_ticks: i32,
}

impl Provenance {
    fn before_summoning(&mut self, weather: WeatherState) {
        if !self.summoned_storm {
            self.natural_thunder_ticks = if weather.thundering {
                weather.thunder_time.max(0)
            } else {
                0
            };
        }
        self.summoned_storm = true;
    }

    fn tick(&mut self, weather: WeatherState) {
        if !self.summoned_storm {
            return;
        }
        if !weather.thundering {
            *self = Self::default();
        } else if weather.cycle_enabled && self.natural_thunder_ticks > 0 {
            self.natural_thunder_ticks -= 1;
        }
    }

    fn allows(self, weather: WeatherState) -> bool {
        weather.raining
            && weather.thundering
            && (!self.summoned_storm || self.natural_thunder_ticks > 0)
    }
}

pub fn require() -> Result<(), InterfaceUnavailable> {
    let _ = SAVES.set(saved_data::require()?);
    Ok(())
}

pub fn register(reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let saves = SAVES.get().ok_or(RegistrationError::InvalidDefinition)?;
    let _ = KEY.set(saves.register(reg, "raevyx_storm")?);
    Ok(())
}

fn load(world: World<'_>) -> Provenance {
    SAVES
        .get()
        .zip(KEY.get())
        .and_then(|(saves, key)| saves.read(&world, *key))
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save(world: World<'_>, state: Provenance) {
    if let (Some(saves), Some(key), Ok(bytes)) =
        (SAVES.get(), KEY.get(), serde_json::to_vec(&state))
    {
        saves.write(&world, *key, &bytes);
    }
}

fn storm(weather: WeatherState, duration: i32) -> WeatherState {
    WeatherState {
        clear_time: 0,
        rain_time: if weather.raining {
            weather.rain_time.max(duration)
        } else {
            duration
        },
        thunder_time: if weather.thundering {
            weather.thunder_time.max(duration)
        } else {
            duration
        },
        raining: true,
        thundering: true,
        ..weather
    }
}

pub fn summon(world: World<'_>, duration: i32) {
    let Some(weather) = world.weather_state() else {
        return;
    };
    let mut provenance = load(world);
    // The Java saved data is overworld-global. Only the overworld has natural storms.
    if world.dimension_id() == Some("minecraft:overworld") {
        provenance.before_summoning(weather);
    }
    if world.set_weather_state(storm(weather, duration)) == Some(true)
        && world.dimension_id() == Some("minecraft:overworld")
    {
        save(world, provenance);
    }
}

pub fn tick(world: World<'_>) {
    if world.dimension_id() != Some("minecraft:overworld") {
        return;
    }
    let Some(weather) = world.weather_state() else {
        return;
    };
    let before = load(world);
    let mut state = before;
    state.tick(weather);
    if state != before {
        save(world, state);
    }
}

pub fn allows_wild_spawns(world: World<'_>) -> bool {
    world
        .weather_state()
        .is_some_and(|weather| load(world).allows(weather))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_independent_active_timers_and_ignores_inactive_timers() {
        let previous = WeatherState {
            clear_time: 500,
            rain_time: 3000,
            thunder_time: 800,
            raining: true,
            thundering: true,
            cycle_enabled: true,
        };
        let next = storm(previous, 1200);
        assert_eq!(
            (next.clear_time, next.rain_time, next.thunder_time),
            (0, 3000, 1200)
        );
        let next = storm(
            WeatherState {
                raining: false,
                rain_time: 5000,
                thunder_time: 3000,
                ..previous
            },
            1200,
        );
        assert_eq!((next.rain_time, next.thunder_time), (1200, 3000));
    }

    #[test]
    fn natural_thunder_expires_without_refreshing_on_recast() {
        let weather = WeatherState {
            raining: true,
            thundering: true,
            thunder_time: 2,
            cycle_enabled: true,
            ..WeatherState::default()
        };
        let mut state = Provenance::default();
        state.before_summoning(weather);
        let summoned = storm(weather, 1200);
        state.tick(summoned);
        state.before_summoning(summoned);
        assert_eq!(state.natural_thunder_ticks, 1);
        state.tick(WeatherState {
            cycle_enabled: false,
            ..summoned
        });
        assert!(state.allows(summoned));
        state.tick(summoned);
        assert!(!state.allows(summoned));
        state.tick(WeatherState::default());
        assert!(state.allows(weather));
        assert!(!state.allows(WeatherState {
            raining: false,
            ..weather
        }));
    }
}
