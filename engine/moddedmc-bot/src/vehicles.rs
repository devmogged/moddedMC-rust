use pumpkin_data::{Block, attributes::Attributes, entity::EntityType};
use pumpkin_protocol::java::{
    client::play::CMoveVehicle,
    server::play::{SMoveVehicle, SPaddleBoat, SPlayerInput as Key},
};
use pumpkin_util::math::{boundingbox::BoundingBox, vector3::Vector3};

use crate::{
    collision::collide_body,
    controls::{Controls, yaw_sin_cos},
    entities::{Entities, Tracked},
    fluids,
    terrain::World,
};

pub struct Ride {
    id: i32,
    position: Vector3<f64>,
    velocity: Vector3<f64>,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    delta_rotation: f32,
}

impl Ride {
    pub const fn new(id: i32, mount: &Tracked) -> Self {
        Self {
            id,
            position: mount.position,
            velocity: mount.velocity,
            yaw: mount.yaw,
            pitch: mount.pitch,
            on_ground: mount.on_ground,
            delta_rotation: 0.0,
        }
    }

    pub const fn correct(&mut self, packet: &CMoveVehicle) {
        self.position = Vector3::new(packet.x, packet.y, packet.z);
        self.yaw = packet.yaw;
        self.pitch = packet.pitch;
    }

    pub fn tick(
        &mut self,
        world: &World,
        entities: &Entities,
        keys: &Controls,
        rotation: (f32, f32),
    ) -> Option<(SMoveVehicle, Option<SPaddleBoat>)> {
        let mount = entities.get(self.id)?;
        world.block(self.position.floor_to_i32())?;
        let bounds = BoundingBox::new_from_pos(
            self.position.x,
            self.position.y,
            self.position.z,
            &mount.dimensions(),
        );
        let (paddles, step, drag) = if mount.kind.boat.is_some() {
            (Some(self.control_boat(world, bounds, keys)), 0.0, None)
        } else {
            let drag = self.travel_ridden(world, mount, keys, rotation);
            (
                None,
                mount.attribute(&Attributes::STEP_HEIGHT, 0.6) as f32,
                Some(drag),
            )
        };
        let moved = collide_body(
            world,
            entities,
            bounds,
            self.velocity,
            self.on_ground,
            step,
            Some(self.id),
        );
        self.position += moved;
        self.on_ground = self.velocity.y < 0.0 && self.velocity.y != moved.y;
        if (self.velocity.x - moved.x).abs() >= f64::from(1e-5f32) {
            self.velocity.x = 0.0;
        }
        if self.velocity.y != moved.y {
            self.velocity.y = 0.0;
        }
        if (self.velocity.z - moved.z).abs() >= f64::from(1e-5f32) {
            self.velocity.z = 0.0;
        }
        if let Some(drag) = drag {
            self.velocity.x *= drag;
            self.velocity.z *= drag;
            let gravity = mount.attribute(&Attributes::GRAVITY, 0.08);
            self.velocity.y = (self.velocity.y - gravity) * f64::from(0.98f32);
        }
        Some((
            SMoveVehicle {
                x: self.position.x,
                y: self.position.y,
                z: self.position.z,
                yaw: self.yaw,
                pitch: self.pitch,
                on_ground: self.on_ground,
            },
            paddles,
        ))
    }

    fn travel_ridden(
        &mut self,
        world: &World,
        mount: &Tracked,
        keys: &Controls,
        (yaw, pitch): (f32, f32),
    ) -> f64 {
        self.yaw = yaw;
        self.pitch = pitch * 0.5;
        let friction = world
            .block((self.position - Vector3::new(0.0, f64::from(0.500001f32), 0.0)).floor_to_i32())
            .map_or(Block::AIR.slipperiness, |state| {
                state.to_block().slipperiness
            });
        let mut speed = mount.attribute(&Attributes::MOVEMENT_SPEED, 0.25);
        let input = if mount.kind.id == EntityType::PIG.id {
            speed *= 0.225;
            Vector3::new(0.0, 0.0, 1.0)
        } else {
            let mut input = keys.relative_input(0.0, false, 1.0);
            input.x *= 0.5;
            if input.z < 0.0 {
                input.z *= 0.25;
            }
            input
        };
        let speed = speed as f32;
        let acceleration = if !self.on_ground {
            speed * 0.1
        } else if f64::from(friction) > 0.6 {
            speed * (0.21600002 / (friction * friction * friction))
        } else {
            speed
        };
        let input = input * f64::from(acceleration);
        let (sin, cos) = yaw_sin_cos(yaw);
        let trim = |value: f64| if value.abs() < 0.003 { 0.0 } else { value };
        self.velocity = Vector3::new(
            trim(self.velocity.x) + input.x * cos - input.z * sin,
            trim(self.velocity.y),
            trim(self.velocity.z) + input.z * cos + input.x * sin,
        );
        f64::from(if self.on_ground {
            friction * 0.91
        } else {
            0.91
        })
    }

    fn control_boat(&mut self, world: &World, bounds: BoundingBox, keys: &Controls) -> SPaddleBoat {
        // AbstractBoat.floatBoat and controlBoat use float friction and steering.
        let (drag, gravity, buoyancy) = boat_float(world, bounds);
        let drag64 = f64::from(drag);
        self.velocity.x *= drag64;
        self.velocity.y -= gravity;
        self.velocity.z *= drag64;
        if buoyancy > 0.0 {
            self.velocity.y = (self.velocity.y + buoyancy * (0.04 / 0.65)) * 0.75;
        }
        self.delta_rotation *= drag;
        let left = keys.has(Key::LEFT);
        let right = keys.has(Key::RIGHT);
        let forward = keys.has(Key::FORWARD);
        let back = keys.has(Key::BACKWARD);
        self.delta_rotation += f32::from(u8::from(right)) - f32::from(u8::from(left));
        self.yaw += self.delta_rotation;
        let mut acceleration: f32 = if left != right && !forward && !back {
            0.005
        } else {
            0.0
        };
        if forward {
            acceleration += 0.04;
        }
        if back {
            acceleration -= 0.005;
        }
        let (sin, _) = yaw_sin_cos(-self.yaw);
        let (_, cos) = yaw_sin_cos(self.yaw);
        self.velocity.x += f64::from(sin as f32 * acceleration);
        self.velocity.z += f64::from(cos as f32 * acceleration);
        SPaddleBoat {
            left_paddle: (right && !left) || forward,
            right_paddle: (left && !right) || forward,
        }
    }
}

fn boat_float(world: &World, bounds: BoundingBox) -> (f32, f64, f64) {
    let min = bounds.min.floor_to_i32();
    let max_x = bounds.max.x.ceil() as i32;
    let max_z = bounds.max.z.ceil() as i32;
    let top = bounds.max.y + 0.001;
    let mut submerged = false;
    let mut water_level = bounds.min.y;
    for x in min.x..max_x {
        for z in min.z..max_z {
            for y in (bounds.max.y.floor() as i32)..(top.ceil() as i32) {
                if let Some((height, source)) = fluids::water_surface(world, Vector3::new(x, y, z))
                    && height > top
                {
                    if !source {
                        return (0.9, 0.0007, 0.0);
                    }
                    submerged = true;
                }
            }
            for y in min.y..((bounds.min.y + 0.001).ceil() as i32) {
                if let Some((height, _)) = fluids::water_surface(world, Vector3::new(x, y, z)) {
                    water_level = water_level.max(height);
                }
            }
        }
    }
    if submerged {
        (0.45, 0.04, f64::from(0.01f32))
    } else if water_level > bounds.min.y {
        (
            0.9,
            0.04,
            (water_level - bounds.min.y) / (bounds.max.y - bounds.min.y),
        )
    } else {
        let contact = BoundingBox::new(
            bounds.min - Vector3::new(0.0, 0.001, 0.0),
            Vector3::new(bounds.max.x, bounds.min.y, bounds.max.z),
        );
        let mut previous = None;
        let mut sum = 0.0;
        let mut count = 0;
        world.visit_shapes(contact, |pos, _| {
            if previous != Some(pos) {
                previous = Some(pos);
                if let Some(state) = world.block(pos.0)
                    && state.to_block_id() != Block::LILY_PAD.id
                {
                    sum += state.to_block().slipperiness;
                    count += 1;
                }
            }
        });
        (if count == 0 { 0.9 } else { sum / count as f32 }, 0.04, 0.0)
    }
}
