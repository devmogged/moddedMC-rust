//! Stock-client displays for the BoilerData gauge and its moving dial.

use moddedmc_api::{Pos, content::World, stacks::ItemDisplay};

use crate::{
    display_model::ModelDisplay,
    kinetics::FACES,
    steam_engine::{compose, rotate_vector, spin},
};

const GAUGE: &str = "create:moddedmc/parts/steam_engine/gauge";
const DIAL: &str = "create:moddedmc/parts/steam_engine/gauge_dial";
const CHASE_FACTOR: f32 = 0.125;
const DIAL_PIVOT_Y: f32 = 2.0 / 16.0;

#[derive(Debug, Default)]
pub(super) struct Gauges {
    displays: [[ModelDisplay; 2]; 4],
    progress: f32,
    visible: u8,
    width: u8,
}

impl Gauges {
    pub(super) fn update(&mut self, world: World<'_>, pos: Pos, time: u64, width: u8, target: f32) {
        let inset = f64::from(width) / 2.0;
        let center = [
            f64::from(pos.x) + inset,
            f64::from(pos.y) + 0.5,
            f64::from(pos.z) + inset,
        ];
        let refresh = width != self.width || time.is_multiple_of(10);
        if refresh {
            self.width = width;
            self.visible = 0;
            for (side, direction) in FACES[2..].iter().enumerate() {
                if !occluded(world, center, inset, *direction) {
                    self.visible |= 1 << side;
                }
            }
        }
        let previous = self.progress;
        self.progress = if (target - previous).abs() <= 0.001 {
            target
        } else {
            previous + (target - previous) * CHASE_FACTOR
        };
        if !refresh && self.progress == previous {
            return;
        }
        let angle = (-145.0 * self.progress + 90.0).to_radians();
        let (sin, cos) = angle.sin_cos();
        for (side, [housing, dial]) in self.displays.iter_mut().enumerate() {
            if self.visible & (1 << side) == 0 {
                housing.clear(world);
                dial.clear(world);
                continue;
            }
            let rotation = spin(1, [90.0f32, -90.0, 180.0, 0.0][side].to_radians());
            let offset = [inset as f32 - 6.0 / 16.0, 0.0, 0.0];
            let shifted = rotate_vector(rotation, offset);
            let position = std::array::from_fn(|axis| center[axis] + f64::from(shifted[axis]));
            let mut shown = ItemDisplay {
                position,
                rotation,
                context: 0,
                scale: 1.0,
                interpolation: 1,
            };
            housing.show(world, GAUGE, &shown);
            let pivot = rotate_vector(
                rotation,
                [0.0, (cos - 1.0) * DIAL_PIVOT_Y, sin * DIAL_PIVOT_Y],
            );
            shown.position = std::array::from_fn(|axis| position[axis] + f64::from(pivot[axis]));
            shown.rotation = compose(rotation, spin(0, angle));
            dial.show(world, DIAL, &shown);
        }
    }

    pub(super) fn clear(&mut self, world: World<'_>) {
        for parts in &mut self.displays {
            for display in parts {
                display.clear(world);
            }
        }
    }
}

fn occluded(world: World<'_>, center: [f64; 3], radius: f64, [dx, _, dz]: [i32; 3]) -> bool {
    let center = [
        center[0] + f64::from(dx) * (radius + 0.25),
        center[1],
        center[2] + f64::from(dz) * (radius + 0.25),
    ];
    let half = [
        0.125 + f64::from(dz.abs()) / 2.0,
        0.375,
        0.125 + f64::from(dx.abs()) / 2.0,
    ];
    let min: [f64; 3] = std::array::from_fn(|axis| center[axis] - half[axis]);
    let max: [f64; 3] = std::array::from_fn(|axis| center[axis] + half[axis]);
    let [x1, y1, z1] = min.map(|coordinate| coordinate.floor() as i32);
    let [x2, y2, z2] = max.map(|coordinate| coordinate.floor() as i32);
    for x in x1..=x2 {
        for y in y1..=y2 {
            for z in z1..=z2 {
                let delta = [x, y, z].map(f64::from);
                let Some(shapes) = world.collision_shape(Pos { x, y, z }) else {
                    return true;
                };
                if shapes.iter().any(|shape| {
                    (0..3).all(|axis| {
                        shape.min[axis] + delta[axis] < max[axis]
                            && shape.max[axis] + delta[axis] > min[axis]
                    })
                }) {
                    return true;
                }
            }
        }
    }
    false
}
