//! Short-lived, nonpersistent item display effects. The companion owns rendering.
use crate::visuals::DISPLAYS;
use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    content::World,
    items::{Item, Items},
    stacks::ItemDisplay,
    tick_count,
};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

pub const MAGMA: &str = "saintsdragons:ignivorus_magma_pillar_display";
pub const AMETHYST: &str = "saintsdragons:amethyst_pillar_display";
const ARC: &str = "saintsdragons:lightning_arc_display";

pub fn register(items: Items, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for id in [MAGMA, AMETHYST, ARC] {
        items.add_item(
            registry,
            &Item {
                id,
                model: "minecraft:item/paper",
                display_name: "Visual Effect",
                carrier: "minecraft:paper",
            },
        )?;
    }
    Ok(())
}

fn stack(id: &str, data: HashMap<String, Value>) -> Vec<u8> {
    fastnbt::to_bytes(&HashMap::from([
        ("id", Value::String(id.to_owned())),
        ("count", Value::Int(1)),
        (
            "components",
            Value::Compound(HashMap::from([(
                "minecraft:custom_data".to_owned(),
                Value::Compound(data),
            )])),
        ),
    ]))
    .expect("effect stack has fixed NBT fields")
}

fn display(position: [f64; 3], yaw: f32) -> ItemDisplay {
    let half = -yaw.to_radians() * 0.5;
    ItemDisplay {
        position,
        rotation: [0.0, half.sin(), 0.0, half.cos()],
        scale: 1.0,
        context: 0,
        interpolation: 0,
    }
}

fn pillar_stack(id: &str, clip: &str) -> Vec<u8> {
    stack(
        id,
        HashMap::from([(
            "moddedmc:rig_clip".to_owned(),
            Value::String(clip.to_owned()),
        )]),
    )
}

pub fn pillar(
    world: World<'_>,
    id: &str,
    clip: &str,
    position: [f64; 3],
    yaw: f32,
    scale: f32,
) -> Option<i32> {
    DISPLAYS.get()?.spawn_item(
        world,
        &pillar_stack(id, clip),
        &ItemDisplay {
            scale,
            ..display(position, yaw)
        },
    )
}

pub fn pillar_clip(world: World<'_>, display: i32, id: &str, clip: &str) {
    if let Some(displays) = DISPLAYS.get() {
        displays.set_item(world, display, &pillar_stack(id, clip));
    }
}

pub fn remove(world: World<'_>, id: i32) {
    if let Some(displays) = DISPLAYS.get() {
        displays.remove(world, id);
    }
}

// RaevyxGroundRendTrailRenderer.renderLayer: scale, width, jitter, RGBA.
const RED: [[f32; 7]; 3] = [
    [1.0, 0.17, 0.020, 0.36, 0.01, 0.025, 0.52],
    [0.72, 0.11, 0.013, 1.0, 0.035, 0.085, 0.72],
    [0.38, 0.065, 0.007, 1.0, 0.78, 0.82, 0.92],
];
const BLUE: [[f32; 7]; 3] = [
    [1.0, 0.18, 0.012, 0.45, 0.45, 0.50, 0.26],
    [0.74, 0.12, 0.008, 0.66, 0.77, 0.98, 0.31],
    [0.46, 0.07, 0.004, 0.96, 0.98, 1.0, 0.36],
];

fn arc_stack(delta: [f64; 3], blood: bool, lifetime: i32, seed: i32, scale: f32) -> Vec<u8> {
    let layers = if blood { RED } else { BLUE }
        .into_iter()
        .map(|layer| Value::List(layer.into_iter().map(Value::Float).collect()))
        .collect();
    stack(
        ARC,
        HashMap::from([(
            "moddedmc:arc".to_owned(),
            Value::Compound(HashMap::from([
                ("x".to_owned(), Value::Float(delta[0] as f32)),
                ("y".to_owned(), Value::Float(delta[1] as f32)),
                ("z".to_owned(), Value::Float(delta[2] as f32)),
                ("lifetime".to_owned(), Value::Int(lifetime)),
                ("seed".to_owned(), Value::Int(seed)),
                ("scale".to_owned(), Value::Float(scale)),
                ("grow".to_owned(), Value::Byte(i8::from(blood))),
                ("layers".to_owned(), Value::List(layers)),
            ])),
        )]),
    )
}

type Active = HashMap<&'static str, Vec<(i32, u64)>>;
static ARCS: LazyLock<Mutex<Active>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// LightningVisualEntity: a seeded segment centered between its endpoints.
pub fn arc(world: World<'_>, from: [f64; 3], to: [f64; 3], blood: bool, lifetime: i32, scale: f32) {
    let delta = std::array::from_fn(|i| to[i] - from[i]);
    if delta.iter().map(|v| v * v).sum::<f64>() < 1.0e-6 {
        return;
    }
    let (Some(displays), Some(dimension)) = (DISPLAYS.get(), world.dimension_id()) else {
        return;
    };
    let midpoint = std::array::from_fn(|i| (from[i] + to[i]) * 0.5);
    let seed = crate::ai::random(u64::from(u32::MAX)) as i32;
    if let Some(id) = displays.spawn_item(
        world,
        &arc_stack(delta, blood, lifetime, seed, scale),
        &display(midpoint, 0.0),
    ) {
        ARCS.lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(dimension)
            .or_default()
            .push((id, tick_count() + lifetime as u64));
    }
}

pub fn tick(world: World<'_>, now: u64) {
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    let mut arcs = ARCS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(active) = arcs.get_mut(dimension) {
        active.retain(|&(id, expires)| {
            if now >= expires {
                remove(world, id);
                false
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arc_carries_release_style_and_endpoints() {
        let nbt: HashMap<String, Value> =
            fastnbt::from_bytes(&arc_stack([2.0, -3.0, 4.0], true, 5, 123, 1.0)).unwrap();
        assert_eq!(nbt["id"], Value::String(ARC.to_owned()));
        let Value::Compound(components) = &nbt["components"] else {
            panic!()
        };
        let Value::Compound(data) = &components["minecraft:custom_data"] else {
            panic!()
        };
        let Value::Compound(arc) = &data["moddedmc:arc"] else {
            panic!()
        };
        assert_eq!(arc["y"], Value::Float(-3.0));
        assert_eq!(arc["lifetime"], Value::Int(5));
        assert_eq!(arc["grow"], Value::Byte(1));
        assert_eq!(
            arc["layers"],
            Value::List(
                RED.into_iter()
                    .map(|layer| Value::List(layer.into_iter().map(Value::Float).collect()))
                    .collect()
            )
        );
    }
}
