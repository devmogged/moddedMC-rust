//! Create's DirectedDirectionalBlock targeting, rotation and filter plate.

use moddedmc_api::{
    Pos,
    content::{Placement, World},
};

use crate::{
    CONTAINERS,
    belt_inventory::{self, TransportedItemStack},
    belt_tunnel::{read, shift},
    filtering::{self, Filter},
    kinetics::{FACES, face_axis},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mount {
    pub(crate) facing: u8,
    pub(crate) target: u8,
}

impl Mount {
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        [2, 3, 4, 5]
            .into_iter()
            .flat_map(|facing| [facing, 0, 1].map(|target| Self { facing, target }))
    }

    pub(crate) fn preferred(args: &Placement<'_>, supports: impl Fn(u32) -> bool) -> Self {
        let (sy, cy) = args.yaw.to_radians().sin_cos();
        let (sp, cp) = args.pitch.to_radians().sin_cos();
        let x = -sy * cp;
        let y = -sp;
        let z = cy * cp;
        let mut nearest = [
            (if x > 0.0 { 5 } else { 4 }, x.abs()),
            (if z > 0.0 { 3 } else { 2 }, z.abs()),
            (if y > 0.0 { 1 } else { 0 }, y.abs()),
        ];
        nearest.sort_by(|a, b| b.1.total_cmp(&a.1));
        let nearest = nearest.map(|(face, _)| face);
        let target = nearest
            .into_iter()
            .chain(nearest.into_iter().rev().map(|face| face ^ 1))
            .find(|&face| {
                let pos = shift(args.position, face);
                supports(read(args.world, pos))
                    || CONTAINERS.get().is_some_and(|containers| {
                        containers
                            .at(args.world, pos, moddedmc_api::containers::NO_FACE)
                            .is_some()
                    })
            })
            .unwrap_or_else(|| nearest[0] ^ u8::from(!args.sneak));
        let facing = if target >= 2 {
            target
        } else {
            crate::nearest_looking_direction(args.yaw, 0.0)
        };
        Self { facing, target }
    }

    pub(crate) fn name(self) -> &'static str {
        match self.target {
            0 => "floor",
            1 => "ceiling",
            _ => "wall",
        }
    }

    pub(crate) fn yaw(self) -> i16 {
        (crate::belt::horizontal_angle(self.facing) + 180) % 360
    }

    pub(crate) fn rotated(mut self, face: u8) -> Self {
        if face < 2 {
            self.facing = clockwise(self.facing, 1);
            if self.target >= 2 {
                self.target = self.facing;
            }
        } else {
            self.target =
                clockwise(self.target, face_axis(face)) ^ u8::from(face.is_multiple_of(2));
            if self.target >= 2 {
                self.facing = self.target;
            }
        }
        self
    }

    pub(crate) fn coordinates(self, face: u8, hit: [f32; 3]) -> (f32, f32) {
        let [x, y, z] = hit;
        match face {
            2 => (0.5 - x, y - 0.5),
            3 => (x - 0.5, y - 0.5),
            4 => (z - 0.5, y - 0.5),
            5 => (0.5 - z, y - 0.5),
            _ => match self.facing {
                3 => (0.5 - x, z - 0.5),
                4 => (z - 0.5, x - 0.5),
                5 => (0.5 - z, 0.5 - x),
                _ => (x - 0.5, 0.5 - z),
            },
        }
    }

    pub(crate) fn filter_face(self, face: u8, bottom_slot: bool) -> bool {
        if face == self.target {
            return false;
        }
        if face == self.target ^ 1 {
            return true;
        }
        if self.target >= 2 {
            return face == 1 || (face == 0 && bottom_slot);
        }
        if !bottom_slot {
            return face == self.facing ^ u8::from(self.target == 1);
        }
        face >= 2 && face_axis(face) != face_axis(self.facing)
    }
}

/// Vanilla `Direction.getClockWise(axis)`, with axes numbered x, y, z.
pub(crate) fn clockwise(direction: u8, axis: u8) -> u8 {
    const CLOCKWISE: [[u8; 6]; 3] = [[3, 2, 0, 1, 4, 5], [0, 1, 5, 4, 2, 3], [4, 5, 2, 3, 1, 0]];
    CLOCKWISE[usize::from(axis)][usize::from(direction)]
}

/// Blockstate rotations are clockwise when looking along the positive axis.
pub(crate) fn orientation(pitch: i16, yaw: i16) -> [f32; 4] {
    let (sx, cx) = (-f32::from(pitch).to_radians() * 0.5).sin_cos();
    let (sy, cy) = (-f32::from(yaw).to_radians() * 0.5).sin_cos();
    [cy * sx, sy * cx, -sy * sx, cy * cx]
}

pub(crate) fn render_filter(
    world: World<'_>,
    pos: Pos,
    mount: Mount,
    filter: &Filter,
    display: &mut Option<TransportedItemStack>,
) {
    let face = mount.target ^ 1;
    let [dx, dy, dz] = FACES[usize::from(face)].map(f64::from);
    let [x, y, z] = [pos.x, pos.y, pos.z].map(|value| f64::from(value) + 0.5);
    let yaw = if face >= 2 {
        filtering::preview_angle(face)
    } else {
        i32::from(mount.yaw())
    };
    let mut shown = belt_inventory::item_display(
        [x + dx * 0.49, y + dy * 0.49, z + dz * 0.49],
        yaw,
        face >= 2,
    );
    if face == 0 {
        shown.rotation = orientation(180, mount.yaw());
    }
    shown.scale = 0.24;
    filtering::render(world, filter.stack.as_ref(), display, shown);
}
