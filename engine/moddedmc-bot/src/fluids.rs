use pumpkin_data::{
    BlockDirection, BlockId, BlockStateId, block_properties::WaterLikeProperties,
    tag::Block as BlockTag,
};
use pumpkin_util::math::{boundingbox::BoundingBox, vector3::Vector3};

use crate::terrain::World;

pub const JUMP_THRESHOLD: f64 = 0.4;
pub const SWIM_SPEED: f32 = 0.02;
pub const SWIM_LIFT: f64 = 0.04f32 as f64;

const SOURCE_HEIGHT: f32 = 8.0 / 9.0;
const UP: Vector3<i32> = Vector3::new(0, 1, 0);
const SIDES: [(Vector3<i32>, BlockDirection); 4] = [
    (Vector3::new(0, 0, -1), BlockDirection::North),
    (Vector3::new(1, 0, 0), BlockDirection::East),
    (Vector3::new(0, 0, 1), BlockDirection::South),
    (Vector3::new(-1, 0, 0), BlockDirection::West),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Water,
    Lava,
}

#[derive(Clone, Copy)]
struct Fluid {
    kind: Kind,
    own_height: f32,
    source: bool,
    falling: bool,
}

fn fluid(state: BlockStateId) -> Option<Fluid> {
    let kind = match state.to_block_id() {
        BlockId::WATER => Kind::Water,
        BlockId::LAVA => Kind::Lava,
        BlockId::KELP
        | BlockId::KELP_PLANT
        | BlockId::SEAGRASS
        | BlockId::TALL_SEAGRASS
        | BlockId::BUBBLE_COLUMN => return Some(source_water()),
        _ => return state.is_waterlogged().then(source_water),
    };
    let level = WaterLikeProperties::from_state_id(state).level;
    let amount = if level == 0 || level >= 8 {
        8
    } else {
        8 - level
    };
    Some(Fluid {
        kind,
        own_height: f32::from(amount) / 9.0,
        source: level == 0,
        falling: level >= 8,
    })
}

const fn source_water() -> Fluid {
    Fluid {
        kind: Kind::Water,
        own_height: SOURCE_HEIGHT,
        source: true,
        falling: false,
    }
}

fn at(world: &World, pos: Vector3<i32>) -> Option<Fluid> {
    world.block(pos).and_then(fluid)
}

pub fn water_at(world: &World, pos: Vector3<i32>) -> bool {
    at(world, pos).is_some_and(|state| state.kind == Kind::Water)
}

impl Fluid {
    fn height(self, world: &World, pos: Vector3<i32>) -> f64 {
        if at(world, pos + UP).is_some_and(|above| above.kind == self.kind) {
            1.0
        } else {
            f64::from(self.own_height)
        }
    }

    fn flow(self, world: &World, pos: Vector3<i32>) -> Vector3<f64> {
        let mut flow = Vector3::default();
        for (offset, _) in SIDES {
            let neighbor = pos + offset;
            let Some(block) = world.block(neighbor) else {
                continue;
            };
            let distance = match fluid(block) {
                Some(other) if other.kind == self.kind => self.own_height - other.own_height,
                None if !block
                    .to_block_id()
                    .has_tag(BlockTag::MINECRAFT_BLOCKS_FLUID_FLOW) =>
                {
                    let Some(below) =
                        at(world, neighbor - UP).filter(|below| below.kind == self.kind)
                    else {
                        continue;
                    };
                    self.own_height - (below.own_height - SOURCE_HEIGHT)
                }
                _ => continue,
            };
            flow.x += f64::from(offset.x as f32 * distance);
            flow.z += f64::from(offset.z as f32 * distance);
        }
        if self.falling
            && SIDES.into_iter().any(|(offset, face)| {
                let solid_face = |p| {
                    world.block(p).is_some_and(|block| {
                        block.to_block_id() != BlockId::ICE
                            && !fluid(block).is_some_and(|other| other.kind == self.kind)
                            && block.is_side_solid(face)
                    })
                };
                solid_face(pos + offset) || solid_face(pos + offset + UP)
            })
        {
            flow = normalize(flow) - Vector3::new(0.0, 6.0, 0.0);
        }
        normalize(flow)
    }
}

fn normalize(value: Vector3<f64>) -> Vector3<f64> {
    let length = value.length();
    if length < 1.0e-5 {
        Vector3::default()
    } else {
        value / length
    }
}

#[derive(Default)]
struct Current {
    height: f64,
    sum: Vector3<f64>,
    cells: u32,
}

impl Current {
    fn apply(&self, velocity: &mut Vector3<f64>, scale: f64) {
        if self.cells == 0 || self.sum.length_squared() < f64::from(1.0e-5f32) {
            return;
        }
        let mut impulse = self.sum * (1.0 / f64::from(self.cells)) * scale;
        let minimum = 0.0045000000000000005;
        if velocity.x.abs() < 0.003 && velocity.z.abs() < 0.003 && impulse.length() < minimum {
            impulse = normalize(impulse) * minimum;
        }
        *velocity += impulse;
    }
}

#[derive(Default)]
pub struct Contact {
    water: Current,
    lava: Current,
    pub eyes_in_water: bool,
}

impl Contact {
    pub fn sample(world: &World, body: BoundingBox, eye: Vector3<f64>) -> Self {
        let mut contact = Self::default();
        let scan = body.expand_all(-0.001);
        let lo = scan.min.floor_to_i32();
        let hi = Vector3::new(
            scan.max.x.ceil() as i32 - 1,
            scan.max.y.ceil() as i32 - 1,
            scan.max.z.ceil() as i32 - 1,
        );
        for (x, z) in [
            (lo.x - 1, lo.z - 1),
            (lo.x - 1, hi.z + 1),
            (hi.x + 1, lo.z - 1),
            (hi.x + 1, hi.z + 1),
        ] {
            if world.block(Vector3::new(x, lo.y, z)).is_none() {
                return contact;
            }
        }
        let eye_block = eye.floor_to_i32();
        for x in lo.x..=hi.x {
            for y in lo.y..=hi.y {
                for z in lo.z..=hi.z {
                    let pos = Vector3::new(x, y, z);
                    let Some(state) = at(world, pos) else {
                        continue;
                    };
                    let height = state.height(world, pos);
                    let top = f64::from(y) + height;
                    if top < scan.min.y {
                        continue;
                    }
                    let current = match state.kind {
                        Kind::Water => {
                            if x == eye_block.x && z == eye_block.z && eye.y >= f64::from(y) {
                                let capped = state.source
                                    && world.block(pos + UP).is_some_and(|above| {
                                        above.is_side_solid(BlockDirection::Down)
                                    });
                                contact.eyes_in_water |=
                                    eye.y <= if capped { f64::from(y) + 1.0 } else { top };
                            }
                            &mut contact.water
                        }
                        Kind::Lava => &mut contact.lava,
                    };
                    current.height = current.height.max(top - body.min.y);
                    let flow = state.flow(world, pos);
                    current.sum += if current.height < JUMP_THRESHOLD {
                        flow * current.height
                    } else {
                        flow
                    };
                    current.cells += 1;
                }
            }
        }
        contact
    }

    pub fn apply_current(&self, velocity: &mut Vector3<f64>, fast_lava: bool) {
        self.water.apply(velocity, 0.014);
        self.lava.apply(
            velocity,
            if fast_lava {
                0.007
            } else {
                0.0023333333333333335
            },
        );
    }

    pub fn kind(&self) -> Option<Kind> {
        if self.water.height > 0.0 {
            Some(Kind::Water)
        } else if self.lava.height > 0.0 {
            Some(Kind::Lava)
        } else {
            None
        }
    }

    pub const fn depth(&self, kind: Kind) -> f64 {
        match kind {
            Kind::Water => self.water.height,
            Kind::Lava => self.lava.height,
        }
    }
}

pub fn adjust(
    kind: Kind,
    depth: f64,
    mut velocity: Vector3<f64>,
    gravity: f64,
    falling: bool,
    sprint: bool,
) -> Vector3<f64> {
    let shallow = depth <= JUMP_THRESHOLD;
    let (horizontal, vertical) = match kind {
        Kind::Water => (
            if sprint { 0.9f32 as f64 } else { 0.8f32 as f64 },
            0.8f32 as f64,
        ),
        Kind::Lava => (0.5, if shallow { 0.8f32 as f64 } else { 0.5 }),
    };
    velocity.x *= horizontal;
    velocity.z *= horizontal;
    velocity.y *= vertical;
    if (kind == Kind::Water || shallow) && gravity != 0.0 && !sprint {
        let sink = gravity / 16.0;
        velocity.y = if falling
            && (velocity.y - 0.005).abs() >= 0.003
            && (velocity.y - sink).abs() < 0.003
        {
            -0.003
        } else {
            velocity.y - sink
        };
    }
    if kind == Kind::Lava {
        velocity.y -= gravity / 4.0;
    }
    velocity
}
