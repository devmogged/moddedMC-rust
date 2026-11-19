use pumpkin_data::packet::clientbound::play as p;
use pumpkin_protocol::{
    java::server::play::{SPlayerAbilities, SPlayerInput},
    ser::{NetworkReadExt as _, ReadingError},
};
use pumpkin_util::math::vector3::Vector3;

const FLYING: i8 = 2;
const MAY_FLY: i8 = 4;

pub struct Controls {
    input: i8,
    abilities: i8,
    food: i32,
    fly_speed: f32,
    pub sprinting: bool,
    pub jump_delay: u8,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            input: 0,
            abilities: 0,
            food: 20,
            fly_speed: 0.05,
            sprinting: false,
            jump_delay: 0,
        }
    }
}

impl Controls {
    pub fn observe(&mut self, kind: i32, mut bytes: &[u8]) -> Result<(), ReadingError> {
        if kind == p::PLAYER_ABILITIES.0 {
            self.abilities = bytes.get_i8()?;
            self.fly_speed = bytes.get_f32_be()?;
        } else if kind == p::SET_HEALTH.0 {
            bytes.get_f32_be()?;
            self.food = bytes.get_var_int()?.0;
        }
        Ok(())
    }

    pub fn keys(&mut self, input: &str) -> Result<SPlayerInput, String> {
        use SPlayerInput as Key;
        let mut bits = 0;
        if input != "none" {
            for key in input.split_whitespace() {
                bits |= match key {
                    "forward" => Key::FORWARD,
                    "back" => Key::BACKWARD,
                    "left" => Key::LEFT,
                    "right" => Key::RIGHT,
                    "jump" => Key::JUMP,
                    "sneak" => Key::SNEAK,
                    "sprint" => Key::SPRINT,
                    _ => return Err(format!("unknown movement key {key}")),
                };
            }
            if bits == 0 {
                return Err("input requires one or more keys or none".into());
            }
        }
        self.input = bits;
        Ok(SPlayerInput { input: bits })
    }

    pub fn fly(&mut self, mode: &str) -> Result<SPlayerAbilities, String> {
        let flags = match mode {
            "on" if self.abilities & MAY_FLY != 0 => FLYING,
            "on" => return Err("the server has not granted the bot flight".into()),
            "off" => 0,
            _ => return Err("fly requires on or off".into()),
        };
        self.abilities = (self.abilities & !FLYING) | flags;
        Ok(SPlayerAbilities::new(flags, None, None))
    }

    pub const fn flying(&self) -> bool {
        self.abilities & FLYING != 0
    }

    pub const fn sneaking(&self) -> bool {
        !self.flying() && self.has(SPlayerInput::SNEAK)
    }

    pub const fn jumping(&self) -> bool {
        self.has(SPlayerInput::JUMP)
    }

    const fn has(&self, key: i8) -> bool {
        self.input & key != 0
    }

    pub const fn update_sprint(&mut self, horizontal_collision: bool) {
        let forward = self.has(SPlayerInput::FORWARD) && !self.has(SPlayerInput::BACKWARD);
        self.sprinting = forward
            && !self.sneaking()
            && (self.food > 6 || self.abilities & MAY_FLY != 0)
            && if self.sprinting {
                !horizontal_collision
            } else {
                self.has(SPlayerInput::SPRINT)
            };
    }

    pub fn acceleration(
        &self,
        yaw: f32,
        friction: f32,
        on_ground: bool,
        crouching: bool,
    ) -> Vector3<f64> {
        use SPlayerInput as Key;
        let axis = |positive, negative| i8::from(self.has(positive)) - i8::from(self.has(negative));
        let strafe = f32::from(axis(Key::LEFT, Key::RIGHT));
        let forward = f32::from(axis(Key::FORWARD, Key::BACKWARD));
        let (strafe, forward) = lateral_input(strafe, forward, crouching);
        let speed = if on_ground {
            let walk = if self.sprinting {
                (f64::from(0.1f32) * (1.0 + f64::from(0.3f32))) as f32
            } else {
                0.1
            };
            if f64::from(friction) > 0.6 {
                walk * (0.21600002 / (friction * friction * friction))
            } else {
                walk
            }
        } else if self.flying() {
            self.fly_speed * if self.sprinting { 2.0 } else { 1.0 }
        } else if self.sprinting {
            0.025999999
        } else {
            0.02
        };
        let (sin, cos) = yaw_sin_cos(yaw);
        let strafe = strafe * f64::from(speed);
        let forward = forward * f64::from(speed);
        Vector3::new(
            strafe * cos - forward * sin,
            if self.flying() {
                f64::from(f32::from(axis(Key::JUMP, Key::SNEAK)) * self.fly_speed * 3.0)
            } else {
                0.0
            },
            forward * cos + strafe * sin,
        )
    }
}

#[expect(
    clippy::imprecise_flops,
    reason = "match the order and float rounding of Minecraft's Vec2 and Vec3"
)]
fn lateral_input(strafe: f32, forward: f32, crouching: bool) -> (f64, f64) {
    let length = (strafe * strafe + forward * forward).sqrt();
    if length == 0.0 {
        return (0.0, 0.0);
    }
    // KeyboardInput normalizes first. LocalPlayer then scales toward the unit square.
    let multiplier = if crouching { 0.3 } else { 1.0 };
    let strafe = strafe / length * 0.98 * multiplier;
    let forward = forward / length * 0.98 * multiplier;
    let magnitude = (strafe * strafe + forward * forward).sqrt();
    let direction = 1.0 / magnitude;
    let to_square = 1.0 / ((strafe * direction).abs().max((forward * direction).abs()));
    let magnitude = (magnitude * to_square).min(1.0);
    let x = f64::from(strafe * direction * magnitude);
    let z = f64::from(forward * direction * magnitude);
    let length = (x * x + z * z).sqrt().max(1.0);
    (x / length, z / length)
}

pub fn yaw_sin_cos(yaw: f32) -> (f64, f64) {
    // Mth uses a 65,536-entry sine table. The cosine index is a quarter turn later.
    let radians = f64::from(yaw * (std::f64::consts::PI / 180.0) as f32);
    let index = radians * 10_430.378_350_470_453;
    let lookup = |index: f64| {
        let radians = ((index as i64) & 0xffff) as f64 * std::f64::consts::TAU / 65_536.0;
        f64::from(radians.sin() as f32)
    };
    (lookup(index), lookup(index + 16_384.0))
}
