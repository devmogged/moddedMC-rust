//! Stock item displays for the joints and carried stack of a Mechanical Arm.

use moddedmc_api::{Pos, content::World, stacks::ItemDisplay};

use crate::{
    DISPLAYS, STACKS,
    arm::{Arm, PointType, Selection},
    belt_inventory::{TransportedItemStack, remove_display, render_stack},
    display_model::ModelDisplay,
};

const LOWER_LENGTH: f64 = 14.0 / 16.0;
const UPPER_LENGTH: f64 = 15.0 / 16.0;
const ARM_ORIGIN: f64 = 6.0 / 16.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Angles {
    base: f64,
    lower: f64,
    upper: f64,
    head: f64,
}

impl Angles {
    fn rest(base: f64) -> Self {
        Self {
            base,
            lower: 135.0,
            upper: 45.0,
            head: 0.0,
        }
    }

    /// ArmAngleTarget's two-segment inverse kinematics.
    fn towards(point: Selection, ceiling: bool) -> Self {
        let [x, y, z] = point.pos.map(f64::from);
        let height = match point.kind {
            PointType::Belt | PointType::Depot => 14.0 / 16.0,
            PointType::Chute => 1.0,
            _ => 0.5,
        };
        let claw = [
            x,
            y + height - 0.5 - if ceiling { -ARM_ORIGIN } else { ARM_ORIGIN },
            z,
        ];
        let dy = (claw[1] + 0.5) * if ceiling { -1.0 } else { 1.0 };
        let horizontal = x.hypot(z);
        let yaw = x.atan2(z).to_degrees() + 180.0;
        let base = if ceiling { 180.0 - yaw } else { yaw };
        let a = LOWER_LENGTH;
        let b = UPPER_LENGTH;
        let length = dy.hypot(horizontal).clamp(1.0 / 8.0, a + b);
        let lower = ((a * a - b * b + length * length) / (2.0 * a * length))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
            + dy.atan2(horizontal).to_degrees();
        let upper = ((a * a + b * b - length * length) / (2.0 * b * a))
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees();
        let hand = Pose::origin([0.0; 3], ceiling)
            .rotate(1, base)
            .rotate(0, lower)
            .translate([0.0, 0.0, -a])
            .rotate(0, upper - 180.0)
            .translate([0.0, 0.0, -b])
            .position;
        let [dx, dy, dz] = subtract(claw, hand);
        let pitch = (dy * if ceiling { -1.0 } else { 1.0 })
            .atan2(dx.hypot(dz))
            .to_degrees();
        let head = pitch - lower - upper - 135.0;
        Self {
            base,
            lower,
            upper,
            head,
        }
    }

    fn lerp(self, next: Self, fraction: f64) -> Self {
        let angle = |a: f64, b: f64| a + ((b - a + 180.0).rem_euclid(360.0) - 180.0) * fraction;
        Self {
            base: angle(self.base, next.base),
            lower: self.lower + (next.lower - self.lower) * fraction,
            upper: self.upper + (next.upper - self.upper) * fraction,
            head: angle(self.head, next.head),
        }
    }
}

fn add([x, y, z]: [f64; 3], [a, b, c]: [f64; 3]) -> [f64; 3] {
    [x + a, y + b, z + c]
}

fn subtract([x, y, z]: [f64; 3], [a, b, c]: [f64; 3]) -> [f64; 3] {
    [x - a, y - b, z - c]
}

fn cross([x, y, z]: [f64; 3], [a, b, c]: [f64; 3]) -> [f64; 3] {
    [y * c - z * b, z * a - x * c, x * b - y * a]
}

#[derive(Clone, Copy)]
struct Pose {
    position: [f64; 3],
    rotation: [f64; 4],
}

impl Pose {
    fn origin(position: [f64; 3], ceiling: bool) -> Self {
        Self {
            position,
            rotation: if ceiling {
                [1.0, 0.0, 0.0, 0.0]
            } else {
                [0.0, 0.0, 0.0, 1.0]
            },
        }
    }

    fn translate(mut self, offset: [f64; 3]) -> Self {
        let [x, y, z, w] = self.rotation;
        let axis = [x, y, z];
        let doubled = cross(axis, offset).map(|value| value * 2.0);
        self.position = add(
            self.position,
            add(
                offset,
                add(doubled.map(|value| value * w), cross(axis, doubled)),
            ),
        );
        self
    }

    fn rotate(mut self, axis: usize, degrees: f64) -> Self {
        let (sine, cosine) = (degrees.to_radians() / 2.0).sin_cos();
        let mut right = [0.0; 3];
        right[axis] = sine;
        let [x, y, z, w] = self.rotation;
        let left = [x, y, z];
        let [a, b, c] = add(
            add(
                left.map(|value| value * cosine),
                right.map(|value| value * w),
            ),
            cross(left, right),
        );
        let dot: f64 = left.into_iter().zip(right).map(|(a, b)| a * b).sum();
        self.rotation = [a, b, c, w * cosine - dot];
        self
    }

    fn display(self, scale: f32, context: u8) -> ItemDisplay {
        ItemDisplay {
            position: self.position,
            rotation: self.rotation.map(|part| part as f32),
            scale,
            context,
            interpolation: 2,
        }
    }
}

#[derive(Default)]
pub(crate) struct ArmDisplay {
    parts: [ModelDisplay; 6],
    carried: Option<TransportedItemStack>,
    target: Option<Selection>,
    from: Option<Angles>,
    shown: Option<Angles>,
}

impl ArmDisplay {
    pub(crate) fn clear(&mut self, world: World<'_>) {
        for part in &mut self.parts {
            part.clear(world);
        }
        if let Some(item) = self.carried.take() {
            remove_display(world, &item);
        }
    }

    pub(crate) fn render(&mut self, world: World<'_>, pos: Pos, ceiling: bool, arm: &Arm) {
        let target = arm
            .target_point_index
            .and_then(|i| arm.interaction_points.get(i))
            .copied();
        if target != self.target {
            self.target = target;
            self.from = self.shown;
        }
        let rest = Angles::rest(self.from.map_or(0.0, |angles| angles.base));
        let requested = target.map_or(rest, |target| Angles::towards(target, ceiling));
        let progress = f64::from(arm.movement_progress);
        let from = self.from.unwrap_or(rest);
        let mut angles = if progress < 0.5 {
            from.lerp(rest, progress * 2.0)
        } else {
            rest.lerp(requested, progress * 2.0 - 1.0)
        };
        angles.base = from.lerp(requested, progress).base;
        let moved = self.shown != Some(angles);
        self.shown = Some(angles);

        let held_changed = self.carried.as_ref().map(|shown| &shown.item) != arm.held_item.as_ref();
        if held_changed {
            if let Some(old) = self.carried.take() {
                remove_display(world, &old);
            }
            self.carried = arm.held_item.clone().map(TransportedItemStack::new);
        }
        let has_item = self.carried.is_some();
        let block = self.carried.as_mut().is_some_and(|item| {
            STACKS
                .get()
                .is_some_and(|&stacks| item.places_block(stacks))
        });
        let grip = if block {
            3.0 / 16.0
        } else if has_item {
            5.0 / 64.0
        } else {
            1.0 / 16.0
        };
        let center = [pos.x, pos.y, pos.z].map(|coord| f64::from(coord) + 0.5);
        let base = Pose::origin(center, ceiling)
            .translate([0.0, 4.0 / 16.0, 0.0])
            .rotate(1, angles.base);
        let lower = base
            .translate([0.0, 2.0 / 16.0, 0.0])
            .rotate(0, angles.lower);
        let upper = lower
            .translate([0.0, 0.0, -LOWER_LENGTH])
            .rotate(0, angles.upper - 180.0);
        let claw = upper
            .translate([0.0, 0.0, -UPPER_LENGTH])
            .rotate(0, angles.head - 45.0);
        if moved || held_changed {
            let models = [
                ("create:moddedmc/parts/arm/base", base),
                ("create:moddedmc/parts/arm/lower_body", lower),
                ("create:moddedmc/parts/arm/upper_body", upper),
                (
                    "create:moddedmc/parts/arm/claw_base",
                    if ceiling { claw.rotate(2, 180.0) } else { claw },
                ),
                (
                    "create:moddedmc/parts/arm/upper_claw_grip",
                    claw.translate([0.0, grip, -6.0 / 16.0]),
                ),
                (
                    "create:moddedmc/parts/arm/lower_claw_grip",
                    claw.translate([0.0, -grip, -6.0 / 16.0]),
                ),
            ];
            for (part, (model, pose)) in self.parts.iter_mut().zip(models) {
                part.show(world, model, &pose.display(1.0, 0));
            }
        }
        if let (Some(item), Some(&displays)) = (&mut self.carried, DISPLAYS.get()) {
            let height = if block { 9.0 / 16.0 } else { 10.0 / 16.0 };
            let item_pose = claw.rotate(0, 90.0).translate([0.0, -height, 0.0]);
            render_stack(
                world,
                displays,
                item,
                item_pose.display(if block { 0.5 } else { 0.625 }, 8),
            );
        }
    }
}
