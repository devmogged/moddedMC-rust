//! Short-lived, nonpersistent item display effects. The companion owns rendering.
use crate::visuals::{DISPLAYS, display_tick};
use fastnbt::Value;
use moddedmc_api::{
    RegistrationError, Registry,
    content::World,
    entities::Entity,
    items::{Item, Items},
    stacks::ItemDisplay,
};
use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
};

pub const MAGMA: &str = "saintsdragons:ignivorus_magma_pillar_display";
pub const AMETHYST: &str = "saintsdragons:amethyst_pillar_display";
const ARC: &str = "saintsdragons:lightning_arc_display";
pub const FIREBALL: &str = "saintsdragons:cindervane_fireball_display";
pub const POISON_ORB: &str = "saintsdragons:volitans_poison_orb_display";
pub const SPINE: &str = "saintsdragons:volitans_spine_display";

pub fn register(items: Items, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for id in [MAGMA, AMETHYST, ARC, FIREBALL, POISON_ORB, SPINE] {
        items.add_item(
            registry,
            &Item {
                id,
                model: if id == ARC {
                    "saintsdragons:item/vfx_textures"
                } else {
                    "minecraft:item/paper"
                },
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

/// Keep a release model attached to a simulated projectile. A short lease
/// removes the display even if its owner unloads or the projectile is discarded.
pub fn projectile(
    world: World<'_>,
    handle: &mut Option<i32>,
    model: &str,
    position: [f64; 3],
    velocity: [f64; 3],
    scale: f32,
) {
    let (Some(displays), Some(dimension)) = (DISPLAYS.get(), world.dimension_id()) else {
        return;
    };
    let yaw = (-velocity[0]).atan2(velocity[2]) as f32;
    let pitch = (-velocity[1]).atan2(velocity[0].hypot(velocity[2])) as f32;
    let (sy, cy) = (-yaw * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    let frame = ItemDisplay {
        position,
        rotation: [cy * sp, sy * cp, -sy * sp, cy * cp],
        scale,
        context: 0,
        interpolation: 1,
    };
    let mut leases = ARCS.lock().unwrap_or_else(|e| e.into_inner());
    let active = leases.entry(dimension).or_default();
    if let Some(id) = *handle
        && let Some(entry) = active.iter_mut().find(|entry| entry.0 == id)
        && displays.update(world, id, &frame)
    {
        entry.1 = display_tick(world) + 2;
        return;
    }
    *handle = displays.spawn_item(world, &stack(model, HashMap::new()), &frame);
    if let Some(id) = *handle {
        active.push((id, display_tick(world) + 2));
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

#[derive(Clone, Copy)]
struct Ribbon {
    texture: &'static str,
    frames: i32,
    width: f32,
    alpha: f32,
    repeats: i32,
    color: [f32; 3],
}

fn ribbon_stack(delta: [f64; 3], lifetime: i32, flat: bool, styles: &[Ribbon]) -> Vec<u8> {
    let layers = styles
        .iter()
        .map(|style| {
            Value::Compound(HashMap::from([
                (
                    "texture".to_owned(),
                    Value::String(format!("saintsdragons:textures/particle/{}", style.texture)),
                ),
                ("frames".to_owned(), Value::Int(style.frames)),
                ("width".to_owned(), Value::Float(style.width)),
                ("alpha".to_owned(), Value::Float(style.alpha)),
                ("repeats".to_owned(), Value::Int(style.repeats)),
                ("r".to_owned(), Value::Float(style.color[0])),
                ("g".to_owned(), Value::Float(style.color[1])),
                ("b".to_owned(), Value::Float(style.color[2])),
            ]))
        })
        .collect();
    stack(
        ARC,
        HashMap::from([(
            "moddedmc:ribbon".to_owned(),
            Value::Compound(HashMap::from([
                ("x".to_owned(), Value::Float(delta[0] as f32)),
                ("y".to_owned(), Value::Float(delta[1] as f32)),
                ("z".to_owned(), Value::Float(delta[2] as f32)),
                ("lifetime".to_owned(), Value::Int(lifetime)),
                ("flat".to_owned(), Value::Byte(i8::from(flat))),
                ("layers".to_owned(), Value::List(layers)),
            ])),
        )]),
    )
}

fn ribbon(
    world: World<'_>,
    from: [f64; 3],
    to: [f64; 3],
    lifetime: i32,
    flat: bool,
    styles: &[Ribbon],
) {
    let delta = std::array::from_fn(|i| to[i] - from[i]);
    if delta.iter().map(|v| v * v).sum::<f64>() < 1.0e-6 {
        return;
    }
    let (Some(displays), Some(dimension)) = (DISPLAYS.get(), world.dimension_id()) else {
        return;
    };
    let midpoint = std::array::from_fn(|i| (from[i] + to[i]) * 0.5);
    if let Some(id) = displays.spawn_item(
        world,
        &ribbon_stack(delta, lifetime, flat, styles),
        &display(midpoint, 0.0),
    ) {
        ARCS.lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(dimension)
            .or_default()
            .push((id, display_tick(world) + lifetime as u64));
    }
}

/// RaevyxBeamLightningRenderer's four principal release textures and widths.
pub fn beam(world: World<'_>, mob: Entity<'_>, from: [f64; 3], to: [f64; 3]) {
    let color = if crate::appearance::Appearance::load(mob)
        .texture("raevyx")
        .is_some_and(|name| name.starts_with("night_gold"))
    {
        [1.0, 0.75, 0.15]
    } else {
        [1.0, 0.0, 0.0]
    };
    let styles = [
        Ribbon {
            texture: "raevyx/beam/lightning_beam_aura",
            frames: 1,
            width: 0.92,
            alpha: 0.72,
            repeats: 2,
            color,
        },
        Ribbon {
            texture: "raevyx/beam/lightning_beam",
            frames: 1,
            width: 0.75,
            alpha: 1.0,
            repeats: 2,
            color,
        },
        Ribbon {
            texture: "raevyx/beam/lightning_beam_swirl",
            frames: 1,
            width: 0.88,
            alpha: 0.85,
            repeats: 2,
            color,
        },
        Ribbon {
            texture: "raevyx/beam/lightning_beam_second_swirl",
            frames: 1,
            width: 0.62,
            alpha: 0.60,
            repeats: 2,
            color: [0.0; 3],
        },
    ];
    ribbon(world, from, to, 3, false, &styles);
}

pub enum Element {
    Fire,
    Poison,
    Water,
}

/// Sample the existing swept breath path with the released animated textures.
pub fn breath(world: World<'_>, from: [f64; 3], to: [f64; 3], element: Element, width: f32) {
    let (texture, frames) = match element {
        Element::Fire => ("shared/fire/fire/fire", 17),
        Element::Poison => ("volitans/poison/volitans_poison/volitans_poison", 5),
        Element::Water => ("volitans/water/volitans_water/volitans_water", 5),
    };
    let length = (0..3)
        .map(|i| (to[i] - from[i]).powi(2))
        .sum::<f64>()
        .sqrt();
    let style = Ribbon {
        texture,
        frames,
        width,
        alpha: 0.85,
        repeats: (length / 2.0).ceil().clamp(1.0, 48.0) as i32,
        color: [1.0; 3],
    };
    ribbon(world, from, to, 5, false, &[style]);
}

/// Released animated impact and dash sprites, sampled at the combat event.
pub fn burst(
    world: World<'_>,
    at: [f64; 3],
    radius: f32,
    texture: &'static str,
    frames: i32,
    flat: bool,
) {
    let r = f64::from(radius);
    let (from, to) = if flat {
        ([at[0] - r, at[1], at[2]], [at[0] + r, at[1], at[2]])
    } else {
        ([at[0], at[1] - r, at[2]], [at[0], at[1] + r, at[2]])
    };
    ribbon(
        world,
        from,
        to,
        frames,
        flat,
        &[Ribbon {
            texture,
            frames,
            width: radius,
            alpha: 1.0,
            repeats: 1,
            color: [1.0; 3],
        }],
    );
}

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
            .push((id, display_tick(world) + lifetime as u64));
    }
}

pub fn tick(world: World<'_>) {
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    let now = display_tick(world);
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
    fn ribbon_carries_texture_frames_and_tint() {
        let style = Ribbon {
            texture: "shared/fire/fire/fire",
            frames: 17,
            width: 0.75,
            alpha: 0.8,
            repeats: 2,
            color: [1.0, 0.75, 0.15],
        };
        let nbt: HashMap<String, Value> =
            fastnbt::from_bytes(&ribbon_stack([2.0, 0.0, 3.0], 5, false, &[style])).unwrap();
        let Value::Compound(components) = &nbt["components"] else {
            panic!()
        };
        let Value::Compound(data) = &components["minecraft:custom_data"] else {
            panic!()
        };
        let Value::Compound(ribbon) = &data["moddedmc:ribbon"] else {
            panic!()
        };
        assert_eq!(ribbon["lifetime"], Value::Int(5));
        assert_eq!(ribbon["z"], Value::Float(3.0));
        let Value::List(layers) = &ribbon["layers"] else {
            panic!()
        };
        let Value::Compound(layer) = &layers[0] else {
            panic!()
        };
        assert_eq!(
            layer["texture"],
            Value::String("saintsdragons:textures/particle/shared/fire/fire/fire".into())
        );
        assert_eq!(layer["frames"], Value::Int(17));
        assert_eq!(layer["g"], Value::Float(0.75));
    }

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
