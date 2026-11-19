use std::collections::{HashMap, VecDeque, hash_map::Entry};

use moddedmc_api::Pos;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Piece {
    Shaft(u8),
    Small(u8),
    Large(u8),
    Motor(u8, i32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation {
    pub speed: f32,
    pub source: Option<Pos>,
    pub network: Option<Pos>,
}

pub const FACES: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, -1],
    [0, 0, 1],
    [-1, 0, 0],
    [1, 0, 0],
];

pub const OFFSETS: [[i32; 3]; 18] = [
    FACES[0],
    FACES[1],
    FACES[2],
    FACES[3],
    FACES[4],
    FACES[5],
    [-1, -1, 0],
    [1, -1, 0],
    [-1, 1, 0],
    [1, 1, 0],
    [-1, 0, -1],
    [1, 0, -1],
    [-1, 0, 1],
    [1, 0, 1],
    [0, -1, -1],
    [0, 1, -1],
    [0, -1, 1],
    [0, 1, 1],
];
const MAX_ROTATION_SPEED: f64 = 256.0;

pub const fn offset(pos: Pos, [x, y, z]: [i32; 3]) -> Pos {
    Pos {
        x: pos.x + x,
        y: pos.y + y,
        z: pos.z + z,
    }
}

pub const fn face_axis(face: u8) -> u8 {
    [1, 1, 2, 2, 0, 0][face as usize]
}

impl Piece {
    pub const fn axis(self) -> u8 {
        match self {
            Self::Shaft(axis) | Self::Small(axis) | Self::Large(axis) => axis,
            Self::Motor(face, _) => face_axis(face),
        }
    }

    pub const fn shaft_towards(self, face: u8) -> bool {
        match self {
            Self::Motor(front, _) => front == face,
            _ => self.axis() == face_axis(face),
        }
    }

    fn generated(self) -> f64 {
        match self {
            Self::Motor(face, setting) if face % 2 == 0 => -f64::from(setting),
            Self::Motor(_, setting) => f64::from(setting),
            _ => 0.0,
        }
    }
}

// RotationPropagator.getRotationSpeedModifier for simple relays and creative motors.
pub fn ratio(from: Piece, to: Piece, step: [i32; 3]) -> Option<f64> {
    let from_axis = usize::from(from.axis());
    let to_axis = usize::from(to.axis());
    if let Some(face) = FACES.iter().position(|&face| face == step) {
        let face = face as u8;
        if from.shaft_towards(face) && to.shaft_towards(face ^ 1) {
            return Some(1.0);
        }
        return (matches!(from, Piece::Small(_))
            && matches!(to, Piece::Small(_))
            && from_axis == to_axis
            && step[from_axis] == 0)
            .then_some(-1.0);
    }
    if matches!((from, to), (Piece::Large(_), Piece::Large(_)))
        && from_axis != to_axis
        && step[from_axis] != 0
        && step[to_axis] != 0
    {
        return Some(if (step[from_axis] > 0) ^ (step[to_axis] > 0) {
            -1.0
        } else {
            1.0
        });
    }
    if from_axis == to_axis && step[from_axis] == 0 {
        return match (from, to) {
            (Piece::Large(_), Piece::Small(_)) => Some(-2.0),
            (Piece::Small(_), Piece::Large(_)) => Some(-0.5),
            _ => None,
        };
    }
    None
}

fn connected(pos: Pos, nodes: &HashMap<Pos, Piece>) -> impl Iterator<Item = (Pos, f64)> {
    OFFSETS.into_iter().filter_map(move |step| {
        let target = offset(pos, step);
        Some((target, ratio(nodes[&pos], *nodes.get(&target)?, step)?))
    })
}

pub fn solve(nodes: &HashMap<Pos, Piece>) -> HashMap<Pos, Rotation> {
    let mut result = HashMap::with_capacity(nodes.len());
    let mut ratios = HashMap::with_capacity(nodes.len());
    let mut roots: Vec<_> = nodes.keys().copied().collect();
    roots.sort_unstable_by_key(|pos| (pos.y, pos.z, pos.x));
    for root in roots {
        if result.contains_key(&root) {
            continue;
        }
        ratios.clear();
        ratios.insert(root, 1.0);
        let mut queue = VecDeque::from([root]);
        let mut generator: Option<(Pos, f64)> = None;
        let mut conflicting = false;
        while let Some(pos) = queue.pop_front() {
            let factor = ratios[&pos];
            let generated = nodes[&pos].generated();
            if generated != 0.0 {
                let normalized = generated / factor;
                if let Some((_, existing)) = generator {
                    conflicting |= normalized.is_sign_positive() != existing.is_sign_positive();
                }
                if generator.is_none_or(|(_, existing)| existing.abs() < normalized.abs()) {
                    generator = Some((pos, normalized));
                }
            }
            for (next, multiplier) in connected(pos, nodes) {
                let predicted = factor * multiplier;
                match ratios.entry(next) {
                    Entry::Vacant(entry) => {
                        entry.insert(predicted);
                        queue.push_back(next);
                    }
                    Entry::Occupied(entry) => conflicting |= *entry.get() != predicted,
                }
            }
        }
        if let Some((source, normalized)) = generator
            && !conflicting
            && ratios
                .values()
                .all(|ratio| (ratio * normalized).abs() <= MAX_ROTATION_SPEED)
        {
            result.insert(
                source,
                Rotation {
                    speed: nodes[&source].generated() as f32,
                    source: None,
                    network: Some(source),
                },
            );
            queue.push_back(source);
            while let Some(pos) = queue.pop_front() {
                for (next, _) in connected(pos, nodes) {
                    if let Entry::Vacant(entry) = result.entry(next) {
                        entry.insert(Rotation {
                            speed: (ratios[&next] * normalized) as f32,
                            source: Some(pos),
                            network: Some(source),
                        });
                        queue.push_back(next);
                    }
                }
            }
        } else {
            // shortcut: incompatible and overspeed networks stop until destructive parity is ported.
            result.extend(ratios.keys().map(|&pos| {
                (
                    pos,
                    Rotation {
                        speed: 0.0,
                        source: None,
                        network: None,
                    },
                )
            }));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{Piece, Rotation, solve};
    use moddedmc_api::Pos;
    use std::collections::HashMap;

    const fn pos(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }

    #[test]
    fn shaft_and_parallel_gears_propagate_create_ratios_then_stop_when_cut() {
        let motor = pos(0, 0, 0);
        let shaft = pos(0, 1, 0);
        let large = pos(0, 2, 0);
        let small = pos(1, 2, 1);
        let small2 = pos(2, 2, 1);
        let mut nodes = HashMap::from([
            (motor, Piece::Motor(1, 16)),
            (shaft, Piece::Shaft(1)),
            (large, Piece::Large(1)),
            (small, Piece::Small(1)),
            (small2, Piece::Small(1)),
        ]);
        let powered = solve(&nodes);
        assert_eq!(powered[&shaft].speed, 16.0);
        assert_eq!(powered[&large].speed, 16.0);
        assert_eq!(
            powered[&small],
            Rotation {
                speed: -32.0,
                source: Some(large),
                network: Some(motor)
            }
        );
        assert_eq!(powered[&small2].speed, 32.0);
        nodes.remove(&shaft);
        assert_eq!(
            solve(&nodes)[&small2],
            Rotation {
                speed: 0.0,
                source: None,
                network: None
            }
        );
    }

    #[test]
    fn motor_drives_only_its_front_and_small_drives_large_at_half_speed() {
        let motor = pos(0, 1, 0);
        let front = pos(0, 0, 0);
        let back = pos(0, 2, 0);
        let large = pos(1, 0, 1);
        let nodes = HashMap::from([
            (motor, Piece::Motor(0, 16)),
            (front, Piece::Small(1)),
            (back, Piece::Shaft(1)),
            (large, Piece::Large(1)),
        ]);
        let solved = solve(&nodes);
        assert_eq!(solved[&front].speed, -16.0);
        assert_eq!(solved[&back].speed, 0.0);
        assert_eq!(solved[&large].speed, 8.0);
    }
}
