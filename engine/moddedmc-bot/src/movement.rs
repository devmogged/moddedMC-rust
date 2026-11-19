use std::{sync::Arc, time::Duration};

use pumpkin_data::{BlockStateId, packet::clientbound::play as p};
use pumpkin_protocol::{
    PositionFlag, RawPacket,
    codec::lp_vector_3d::LpVector3d,
    java::{
        client::play::CPlayerPosition,
        server::play::{
            FLAG_HORIZONTAL_COLLISION, FLAG_ON_GROUND, SClientTickEnd, SConfirmTeleport,
            SPlayerPosition,
        },
    },
    ser::{NetworkReadExt as _, NetworkReadSliceExt as _, ReadingError},
};
use pumpkin_util::math::{
    boundingbox::{BoundingBox, EntityDimensions},
    vector3::Vector3,
};
use tokio::sync::Mutex;

use crate::{Writer, bot::send_framed_packet, collision::collide, terrain::World};

const GRAVITY: f64 = 0.08;
const AIR_DRAG: f32 = 0.98;
const PLAYER_SIZE: EntityDimensions = EntityDimensions::new(0.6, 1.8, 1.62);
const BLOCK_BELOW: f64 = 0.500001f32 as f64;
pub const MAX_MANUAL_TICKS: u16 = 1200;

#[derive(Default, Eq, PartialEq)]
pub enum Mode {
    #[default]
    Off,
    Realtime,
    Manual,
}

#[derive(Default)]
pub struct Player {
    pub position: Vector3<f64>,
    pub mode: Mode,
    velocity: Vector3<f64>,
    id: i32,
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    horizontal_collision: bool,
    legacy_heartbeat: Vector3<f64>,
    world: World,
    terrain_error: Option<String>,
}

impl Player {
    pub fn observe(&mut self, packet: &RawPacket) -> Result<(), String> {
        if let Err(error) = self.world.observe(packet.id, &packet.payload) {
            self.terrain_error = Some(error.clone());
            if self.mode != Mode::Off {
                return Err(error);
            }
        }
        self.observe_motion(packet.id, &packet.payload)
            .map_err(|error| error.to_string())
    }

    fn observe_motion(&mut self, id: i32, mut bytes: &[u8]) -> Result<(), ReadingError> {
        if id == p::LOGIN.0 {
            self.id = bytes.get_i32_be()?;
        } else if id == p::SET_ENTITY_MOTION.0 && bytes.get_var_int()?.0 == self.id {
            self.velocity = LpVector3d::read(&mut bytes)?.0;
        } else if id == p::EXPLODE.0 {
            // The remaining explosion fields only control audiovisual effects.
            bytes.read_slice_borrowed(32)?;
            if bytes.get_bool()? {
                self.velocity += Vector3::new(
                    bytes.get_f64_be()?,
                    bytes.get_f64_be()?,
                    bytes.get_f64_be()?,
                );
            }
        } else if id == p::RESPAWN.0 {
            self.terrain_error = None;
            self.velocity = Vector3::default();
            self.on_ground = false;
        }
        Ok(())
    }

    pub fn teleport(&mut self, update: &CPlayerPosition) -> SConfirmTeleport {
        use PositionFlag as F;
        let relative = |flag| update.relatives.contains(&flag);
        let before = self.position;
        let previous_yaw = self.yaw;
        let previous_pitch = self.pitch;
        self.position = update.position
            + Vector3::new(
                if relative(F::X) { before.x } else { 0.0 },
                if relative(F::Y) { before.y } else { 0.0 },
                if relative(F::Z) { before.z } else { 0.0 },
            );
        self.yaw = update.yaw + if relative(F::YRot) { previous_yaw } else { 0.0 };
        self.pitch = (update.pitch
            + if relative(F::XRot) {
                previous_pitch
            } else {
                0.0
            })
        .clamp(-90.0, 90.0);
        let mut old = self.velocity;
        if relative(F::RotateDelta) {
            let (sin, cos) = (previous_pitch - self.pitch).to_radians().sin_cos();
            let (sin, cos) = (f64::from(sin), f64::from(cos));
            old = Vector3::new(old.x, old.y * cos + old.z * sin, old.z * cos - old.y * sin);
            let (sin, cos) = (previous_yaw - self.yaw).to_radians().sin_cos();
            let (sin, cos) = (f64::from(sin), f64::from(cos));
            old = Vector3::new(old.x * cos + old.z * sin, old.y, old.z * cos - old.x * sin);
        }
        self.velocity = update.delta
            + Vector3::new(
                if relative(F::DeltaX) { old.x } else { 0.0 },
                if relative(F::DeltaY) { old.y } else { 0.0 },
                if relative(F::DeltaZ) { old.z } else { 0.0 },
            );
        self.legacy_heartbeat = self.position;
        // Legacy scenarios need a changed position to make Pumpkin stream the new chunk.
        self.legacy_heartbeat.y -= 0.1;
        SConfirmTeleport {
            teleport_id: update.teleport_id,
            position: self.position,
            yaw: self.yaw,
            pitch: self.pitch,
        }
    }

    pub fn set_mode(&mut self, mode: &str) -> Result<(), String> {
        self.mode = match mode {
            "off" => Mode::Off,
            "on" if self.terrain_error.is_none() => Mode::Realtime,
            "manual" if self.terrain_error.is_none() => Mode::Manual,
            "on" | "manual" => {
                return Err(format!(
                    "cannot simulate movement: {}",
                    self.terrain_error.as_deref().unwrap_or_default()
                ));
            }
            _ => return Err("physics requires on, off or manual".into()),
        };
        Ok(())
    }

    pub fn check(&self, args: &str) -> Result<(&'static str, Vector3<f64>), String> {
        let args = args.split_whitespace().collect::<Vec<_>>();
        let [kind, x, y, z, "within", tolerance] = args.as_slice() else {
            return Err("expect requires pos or vel followed by x y z within tolerance".into());
        };
        let (kind, actual) = match *kind {
            "pos" => ("pos", self.position),
            "vel" => ("vel", self.velocity),
            _ => return Err("expect requires pos or vel".into()),
        };
        let [x, y, z, tolerance] =
            [x, y, z, tolerance].map(|value| value.parse::<f64>().unwrap_or(f64::NAN));
        if ![x, y, z, tolerance].into_iter().all(f64::is_finite) || tolerance < 0.0 {
            return Err("expect requires finite coordinates and a nonnegative tolerance".into());
        }
        let target = Vector3::new(x, y, z);
        let distance = (actual - target).length_squared().sqrt();
        if distance <= tolerance {
            Ok((kind, actual))
        } else {
            Err(format!(
                "expected bot {kind} {target:?} within {tolerance}, got {actual:?} (distance {distance})"
            ))
        }
    }

    fn below(&self) -> BlockStateId {
        let below = (self.position - Vector3::new(0.0, BLOCK_BELOW, 0.0)).floor_to_i32();
        self.world.block(below).unwrap_or(BlockStateId::AIR)
    }

    // LivingEntity.aiStep -> travelInAir -> Entity.move. Shape- and input-dependent
    // travel such as swimming and climbing has its own vanilla paths.
    fn travel(&mut self) -> Result<Option<SPlayerPosition>, String> {
        let mut speed = self.velocity;
        if ![self.position.x, self.position.y, self.position.z]
            .into_iter()
            .all(|value| value.abs() < 32_000_000.0)
            || ![speed.x, speed.y, speed.z]
                .into_iter()
                .all(|value| value.abs() <= 32.0)
        {
            return Err("position or impulse exceeds the bot's collision search bounds".into());
        }
        if self.world.block(self.position.floor_to_i32()).is_none() {
            return Ok(None);
        }
        if speed.horizontal_length_squared() < 9.0e-6 {
            speed.x = 0.0;
            speed.z = 0.0;
        }
        if speed.y.abs() < 0.003 {
            speed.y = 0.0;
        }
        let friction = if self.on_ground {
            self.below().to_block().slipperiness
        } else {
            1.0
        };
        let bounds = BoundingBox::new_from_pos(
            self.position.x,
            self.position.y,
            self.position.z,
            &PLAYER_SIZE,
        );
        let moved = collide(&self.world, bounds, speed, self.on_ground);
        let distance = moved.length_squared();
        if distance > 1.0e-7 || speed.length_squared() - distance < 1.0e-7 {
            self.position += moved;
        }
        let x_collision = (speed.x - moved.x).abs() >= 1.0e-5;
        let z_collision = (speed.z - moved.z).abs() >= 1.0e-5;
        self.horizontal_collision = x_collision || z_collision;
        self.on_ground = speed.y < 0.0 && speed.y != moved.y;
        let speed_here = self
            .world
            .block(self.position.floor_to_i32())
            .unwrap_or(BlockStateId::AIR)
            .to_block()
            .velocity_multiplier;
        let block_speed = if speed_here == 1.0 {
            self.below().to_block().velocity_multiplier
        } else {
            speed_here
        };
        let drag = f64::from(friction * 0.91f32);
        self.velocity = Vector3::new(
            if x_collision {
                0.0
            } else {
                speed.x * f64::from(block_speed) * drag
            },
            (if speed.y == moved.y { speed.y } else { 0.0 } - GRAVITY) * f64::from(AIR_DRAG),
            if z_collision {
                0.0
            } else {
                speed.z * f64::from(block_speed) * drag
            },
        );
        Ok(Some(SPlayerPosition {
            position: self.position,
            collision: (u8::from(self.on_ground) * FLAG_ON_GROUND)
                | (u8::from(self.horizontal_collision) * FLAG_HORIZONTAL_COLLISION),
        }))
    }
}

#[expect(
    clippy::print_stdout,
    reason = "reports tick failure to the scenario runner"
)]
pub async fn realtime(player: Arc<Mutex<Player>>, writer: Writer) {
    let mut clock = tokio::time::interval(Duration::from_millis(50));
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    clock.tick().await;
    let mut heartbeat = 0;
    loop {
        clock.tick().await;
        heartbeat = (heartbeat + 1) % 20;
        if let Err(error) = tick(&player, &writer, Mode::Realtime, false).await {
            println!("[bot] error: {error}");
            std::process::exit(2);
        }
        if heartbeat == 0 {
            let _ = tick(&player, &writer, Mode::Off, true).await;
        }
    }
}

pub async fn tick(
    player: &Mutex<Player>,
    writer: &Writer,
    mode: Mode,
    heartbeat: bool,
) -> Result<(), String> {
    let mut writer = writer.lock().await;
    let report = {
        let mut player = player.lock().await;
        if player.mode != mode {
            return Ok(());
        }
        if mode == Mode::Off {
            if !heartbeat {
                return Ok(());
            }
            Some(SPlayerPosition {
                position: player.legacy_heartbeat,
                collision: 0,
            })
        } else {
            player.travel()?
        }
    };
    if let Some(report) = report {
        send_framed_packet(&mut *writer, &report).await?;
    } else if mode == Mode::Manual {
        return Err("cannot tick physics before the bot receives its current chunk".into());
    }
    send_framed_packet(&mut *writer, &SClientTickEnd).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use pumpkin_data::Block;
    use pumpkin_protocol::ser::NetworkWriteExt as _;

    #[test]
    fn owns_motion_and_adds_explosion_knockback() -> Result<(), Box<dyn std::error::Error>> {
        let mut player = Player {
            id: 2,
            ..Player::default()
        };
        let mut motion = vec![2];
        LpVector3d(Vector3::new(0.0, 0.0, 0.5)).write(&mut motion)?;
        player.observe_motion(p::SET_ENTITY_MOTION.0, &motion)?;
        player.check("vel 0 0 0.5 within 0.0001")?;
        player.observe_motion(p::SET_ENTITY_MOTION.0, &[3, 0])?;
        player.check("vel 0 0 0.5 within 0.0001")?;
        let mut explosion = vec![0; 32];
        explosion.write_bool(true)?;
        for component in [0.25, -0.125, -0.125] {
            explosion.write_f64_be(component)?;
        }
        player.observe_motion(p::EXPLODE.0, &explosion)?;
        player.check("vel 0.25 -0.125 0.375 within 0.0001")?;
        Ok(())
    }

    #[test]
    fn lands_and_steps_onto_a_slab() -> Result<(), String> {
        let mut player = Player {
            position: Vector3::new(0.5, 2.0, 0.5),
            world: World::test_column(&[
                (Vector3::new(0, 0, 0), Block::STONE.default_state.id),
                (Vector3::new(1, 0, 0), Block::STONE.default_state.id),
                (Vector3::new(1, 1, 0), Block::STONE_SLAB.default_state.id),
            ]),
            ..Player::default()
        };
        for _ in 0..10 {
            player.travel()?;
        }
        assert!(player.on_ground);
        player.check("pos 0.5 1 0.5 within 0.000001")?;
        player.velocity.x = 0.75;
        player.travel()?;
        player.check("pos 1.25 1.5 0.5 within 0.000001")?;
        player.check("vel 0.40950003 -0.0784000015 0 within 0.000001")?;
        Ok(())
    }
}
