//! Kinetic networks, solved from the blocks around the motors.
//!
//! Rules (after Create's kinetics):
//! - Every part turns about its axis and passes its speed to the parts in line with it on both
//!   sides, if they share the axis. A powered clutch passes nothing.
//! - Small cogs mesh with small cogs next to them in the plane of rotation (speed reversed),
//!   and with large cogs diagonally next to them in that plane (speed reversed and halved;
//!   large to small doubles it).
//! - A network whose parts would need two different speeds is jammed, and one whose loads need
//!   more stress than its motors give is overstressed. Both stop: every part gets speed 0.

use std::collections::HashMap;

use moddedmc_api::Pos;

/// Motor speed in RPM, about its axis.
pub const MOTOR_RPM: i32 = 16;
/// Stress a motor can carry.
pub const MOTOR_CAPACITY: i32 = 256;
/// Stress per RPM a load needs.
pub const LOAD_IMPACT: i32 = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Motor,
    Shaft,
    Clutch,
    Cog,
    LargeCog,
    Load,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Part {
    pub kind: Kind,
    /// 0 = x, 1 = y, 2 = z.
    pub axis: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Jammed,
    Overstressed,
}

/// One connected network: every part's speed (signed RPM, 0 when stopped).
#[derive(Debug)]
pub struct Network {
    pub status: Status,
    pub speeds: Vec<(Pos, Part, i32)>,
}

const fn offset(p: Pos, d: [i32; 3]) -> Pos {
    Pos {
        x: p.x + d[0],
        y: p.y + d[1],
        z: p.z + d[2],
    }
}

fn unit(axis: usize, sign: i32) -> [i32; 3] {
    let mut d = [0; 3];
    d[axis] = sign;
    d
}

/// Neighbors that may mesh with a part, with the speed ratio (numerator, denominator).
fn links(pos: Pos, part: Part) -> Vec<(Pos, [i32; 2], Option<Kind>)> {
    let mut out = Vec::new();
    for sign in [-1, 1] {
        out.push((offset(pos, unit(part.axis, sign)), [1, 1], None));
    }
    let plane: Vec<usize> = (0..3).filter(|&a| a != part.axis).collect();
    let (a, b) = (plane[0], plane[1]);
    match part.kind {
        Kind::Cog => {
            for sign in [-1, 1] {
                for axis in [a, b] {
                    out.push((offset(pos, unit(axis, sign)), [-1, 1], Some(Kind::Cog)));
                }
            }
            for (sa, sb) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                let mut d = unit(a, sa);
                d[b] = sb;
                out.push((offset(pos, d), [-1, 2], Some(Kind::LargeCog)));
            }
        }
        Kind::LargeCog => {
            for (sa, sb) in [(-1, -1), (-1, 1), (1, -1), (1, 1)] {
                let mut d = unit(a, sa);
                d[b] = sb;
                out.push((offset(pos, d), [-2, 1], Some(Kind::Cog)));
            }
        }
        _ => {}
    }
    out
}

/// Solves the networks reachable from `motors`. `read` returns the parts at positions (`None`
/// for anything else, including unloaded blocks); `powered` says if a clutch is powered.
pub fn solve(
    motors: &[Pos],
    mut read: impl FnMut(&[Pos]) -> Vec<Option<Part>>,
    mut powered: impl FnMut(Pos) -> bool,
) -> Vec<Network> {
    let mut seen: HashMap<Pos, usize> = HashMap::new();
    let mut networks = Vec::new();
    let found = read(motors);
    for (&start, part) in motors.iter().zip(found) {
        let Some(part) = part.filter(|p| p.kind == Kind::Motor) else {
            continue;
        };
        if seen.contains_key(&start) {
            continue;
        }
        let index = networks.len();
        let mut speeds: HashMap<Pos, (Part, i32)> = HashMap::new();
        let mut jammed = false;
        speeds.insert(start, (part, MOTOR_RPM));
        seen.insert(start, index);
        let mut frontier = vec![start];
        while !frontier.is_empty() {
            let mut candidates = Vec::new();
            for &pos in &frontier {
                let (part, speed) = speeds[&pos];
                if part.kind == Kind::Clutch && powered(pos) {
                    continue;
                }
                for (next, [num, den], want) in links(pos, part) {
                    candidates.push((pos, next, speed * num / den, part.axis, want));
                }
            }
            let parts = read(&candidates.iter().map(|c| c.1).collect::<Vec<_>>());
            let mut next_frontier = Vec::new();
            for ((from, pos, speed, axis, want), found) in candidates.into_iter().zip(parts) {
                let Some(found) = found else { continue };
                if found.axis != axis || want.is_some_and(|k| k != found.kind) {
                    continue;
                }
                let speed = if found.kind == Kind::Motor {
                    MOTOR_RPM
                } else {
                    speed
                };
                // A motor drives; reaching one at another speed jams the network.
                let incoming = speeds[&from].1;
                let expected = if found.kind == Kind::Motor {
                    incoming
                } else {
                    speed
                };
                match speeds.get(&pos) {
                    Some(&(_, s)) if s != expected && found.kind != Kind::Motor => jammed = true,
                    Some(_) => {}
                    None => {
                        if found.kind == Kind::Motor && incoming != MOTOR_RPM {
                            jammed = true;
                        }
                        speeds.insert(pos, (found, speed));
                        seen.insert(pos, index);
                        next_frontier.push(pos);
                    }
                }
            }
            frontier = next_frontier;
        }
        let motors = speeds
            .values()
            .filter(|(p, _)| p.kind == Kind::Motor)
            .count();
        let stress: i32 = speeds
            .values()
            .filter(|(p, _)| p.kind == Kind::Load)
            .map(|(_, s)| LOAD_IMPACT * s.abs())
            .sum();
        let status = if jammed {
            Status::Jammed
        } else if stress > MOTOR_CAPACITY * i32::try_from(motors).unwrap_or(i32::MAX) {
            Status::Overstressed
        } else {
            Status::Running
        };
        let mut speeds: Vec<(Pos, Part, i32)> = speeds
            .into_iter()
            .map(|(p, (part, s))| (p, part, if status == Status::Running { s } else { 0 }))
            .collect();
        speeds.sort_by_key(|(p, _, _)| (p.x, p.y, p.z));
        networks.push(Network { status, speeds });
    }
    networks
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn p(x: i32, y: i32, z: i32) -> Pos {
        Pos { x, y, z }
    }

    fn world(parts: &[(Pos, Kind, usize)]) -> impl FnMut(&[Pos]) -> Vec<Option<Part>> + '_ {
        |ps: &[Pos]| {
            ps.iter()
                .map(|q| {
                    parts
                        .iter()
                        .find(|(pos, _, _)| pos == q)
                        .map(|&(_, kind, axis)| Part { kind, axis })
                })
                .collect()
        }
    }

    fn speed(n: &Network, at: Pos) -> Option<i32> {
        n.speeds.iter().find(|(q, _, _)| *q == at).map(|s| s.2)
    }

    /// The scenario rig: motor, shafts, two small cogs, a large cog, clutch, load.
    fn rig() -> Vec<(Pos, Kind, usize)> {
        let mut parts = vec![(p(0, 0, 0), Kind::Motor, 0)];
        parts.extend((1..10).map(|x| (p(x, 0, 0), Kind::Shaft, 0)));
        parts.extend([
            (p(10, 0, 0), Kind::Cog, 0),
            (p(10, 0, 1), Kind::Cog, 0),
            (p(10, 1, 2), Kind::LargeCog, 0),
            (p(11, 1, 2), Kind::Shaft, 0),
            (p(12, 1, 2), Kind::Clutch, 0),
            (p(13, 1, 2), Kind::Shaft, 0),
            (p(14, 1, 2), Kind::Load, 0),
        ]);
        parts
    }

    #[test]
    fn gears_reverse_and_scale() {
        let parts = rig();
        let n = solve(&[p(0, 0, 0)], world(&parts), |_| false);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].status, Status::Running);
        assert_eq!(speed(&n[0], p(9, 0, 0)), Some(16));
        assert_eq!(speed(&n[0], p(10, 0, 1)), Some(-16));
        assert_eq!(speed(&n[0], p(10, 1, 2)), Some(8));
        assert_eq!(speed(&n[0], p(14, 1, 2)), Some(8));
    }

    #[test]
    fn clutch_overstress_and_split() {
        let mut parts = rig();
        let n = solve(&[p(0, 0, 0)], world(&parts), |q| q == p(12, 1, 2));
        assert_eq!(
            speed(&n[0], p(14, 1, 2)),
            None,
            "a powered clutch cuts the load off"
        );

        parts.push((p(9, 1, 2), Kind::Load, 0));
        let n = solve(&[p(0, 0, 0)], world(&parts), |_| false);
        assert_eq!(n[0].status, Status::Overstressed);
        assert_eq!(speed(&n[0], p(14, 1, 2)), Some(0));

        parts.retain(|(q, _, _)| *q != p(5, 0, 0));
        let n = solve(&[p(0, 0, 0)], world(&parts), |_| false);
        assert_eq!(speed(&n[0], p(4, 0, 0)), Some(16));
        assert_eq!(
            speed(&n[0], p(6, 0, 0)),
            None,
            "a broken shaft splits the network"
        );
    }

    #[test]
    fn opposed_motors_jam() {
        let parts = [
            (p(0, 0, 0), Kind::Motor, 0),
            (p(1, 0, 0), Kind::Cog, 0),
            (p(1, 0, 1), Kind::Cog, 0),
            (p(2, 0, 1), Kind::Motor, 0),
        ];
        let n = solve(&[p(0, 0, 0), p(2, 0, 1)], world(&parts), |_| false);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].status, Status::Jammed);
    }
}
