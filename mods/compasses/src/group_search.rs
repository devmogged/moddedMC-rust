//! Explorer's Compass group planning independent of the host ABI.
//! A group is a structure set, not a tag or a name prefix.
use super::structure_search;
use moddedmc_api::{Pos, worldgen_lookup::Lookup};
use std::collections::HashMap;

/// Registry order and member order are significant: the original mod resolves
/// a found structure's group to the first registered set containing it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub id: String,
    pub members: Vec<String>,
}

pub fn group_for<'a>(groups: &'a [Group], structure: &str) -> Option<&'a Group> {
    groups
        .iter()
        .find(|g| g.members.iter().any(|id| id == structure))
}

/// Upstream's group button is attached to a selectable individual structure.
/// Only offer first-set groups reached through a selectable member; do not
/// filter the group's full membership, which can contain hidden members.
pub fn offered_groups(groups: &[Group], targets: &[String]) -> Vec<usize> {
    let mut first = HashMap::new();
    for (index, group) in groups.iter().enumerate() {
        for member in &group.members {
            first.entry(member.as_str()).or_insert(index);
        }
    }
    let mut result: Vec<_> = targets
        .iter()
        .filter_map(|member| first.get(member.as_str()).copied())
        .collect();
    result.sort_unstable();
    result.dedup();
    result.sort_by(|&a, &b| groups[a].id.cmp(&groups[b].id));
    result
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub index: u32,
    pub spacing: i32,
    pub members: Vec<String>,
}

/// Reproduce SearchWorkerManager's insertion-ordered placement-to-structures
/// map. Each member's placements come from the active generator, not just the
/// group set: a structure can participate in multiple sets. Empty placement
/// lists mean that member cannot generate in this dimension.
pub fn plan(
    group: &Group,
    mut placements: impl FnMut(&str) -> Option<Vec<(u32, i32)>>,
) -> Option<Vec<Placement>> {
    let mut result: Vec<Placement> = Vec::new();
    for member in &group.members {
        for (index, spacing) in placements(member)? {
            if spacing < 0 {
                return None;
            }
            if let Some(entry) = result.iter_mut().find(|p| p.index == index) {
                if entry.spacing != spacing {
                    return None;
                }
                if !entry.members.contains(member) {
                    entry.members.push(member.clone());
                }
            } else {
                result.push(Placement {
                    index,
                    spacing,
                    members: vec![member.clone()],
                });
            }
        }
    }
    Some(result)
}

/// Returns the actual weighted structure selected for the chunk, never just
/// the requested set or an arbitrary member. A valid but unwanted winner is
/// a negative candidate; it must not cause another member to be generated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub position: Pos,
    pub structure: String,
}

pub trait Resolver {
    fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos>;
    fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>>;
    /// The host must test the actual weighted start for this placement, not
    /// independently try to generate the requested structure.
    fn start(&mut self, placement: u32, member: &str, x: i32, z: i32) -> Lookup<Pos>;
}

/// Reuses the single-target spiral/ring worker. In particular, placements
/// run sequentially and the first successful worker wins, rather than taking
/// the globally closest result across different placements.
pub struct Search {
    inner: structure_search::Search,
    plan: Vec<Placement>,
    probe: Option<Probe>,
}

#[derive(Clone, Copy)]
struct Probe {
    placement: u32,
    x: i32,
    z: i32,
    member: usize,
}

impl Search {
    pub fn new(origin: Pos, previous: Vec<Pos>, plan: Vec<Placement>) -> Self {
        let inner = structure_search::Search::new(
            origin,
            previous,
            plan.iter().map(|p| (p.index, p.spacing)).collect(),
        );
        Self {
            inner,
            plan,
            probe: None,
        }
    }

    pub fn advance(&mut self, resolver: &mut impl Resolver) -> Lookup<Match> {
        let mut adapter = Adapter {
            resolver,
            plan: &self.plan,
            probe: &mut self.probe,
            matched: None,
        };
        match self.inner.advance(&mut adapter) {
            Lookup::Found(_) => adapter.matched.map_or(Lookup::Unavailable, Lookup::Found),
            Lookup::Pending => Lookup::Pending,
            Lookup::Absent => Lookup::Absent,
            Lookup::Unavailable => Lookup::Unavailable,
        }
    }
}

struct Adapter<'a, R> {
    resolver: &'a mut R,
    plan: &'a [Placement],
    probe: &'a mut Option<Probe>,
    matched: Option<Match>,
}

impl<R: Resolver> structure_search::Resolver for Adapter<'_, R> {
    fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos> {
        self.resolver.candidate(placement, x, z)
    }

    fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>> {
        self.resolver.rings(placement)
    }

    fn start(&mut self, placement: u32, x: i32, z: i32) -> Lookup<Pos> {
        let Some(entry) = self.plan.iter().find(|p| p.index == placement) else {
            return Lookup::Unavailable;
        };
        let mut probe = self
            .probe
            .take()
            .filter(|p| (p.placement, p.x, p.z) == (placement, x, z))
            .unwrap_or(Probe {
                placement,
                x,
                z,
                member: 0,
            });
        let Some(member) = entry.members.get(probe.member) else {
            return Lookup::Absent;
        };
        // Exactly one potentially expensive host lookup per advance.
        match self.resolver.start(placement, member, x, z) {
            Lookup::Found(pos) => {
                self.matched = Some(Match {
                    position: pos,
                    structure: member.clone(),
                });
                Lookup::Found(pos)
            }
            Lookup::Absent => {
                probe.member += 1;
                if probe.member < entry.members.len() {
                    *self.probe = Some(probe);
                    Lookup::Pending
                } else {
                    Lookup::Absent
                }
            }
            Lookup::Pending => {
                *self.probe = Some(probe);
                Lookup::Pending
            }
            Lookup::Unavailable => Lookup::Unavailable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, members: &[&str]) -> Group {
        Group {
            id: id.into(),
            members: members.iter().map(|id| (*id).into()).collect(),
        }
    }

    #[test]
    fn registry_order_and_member_placement_order_are_not_sorted() {
        let groups = [
            group("test:z", &["test:b", "test:a"]),
            group("test:a", &["test:a"]),
        ];
        assert_eq!(group_for(&groups, "test:a"), Some(&groups[0]));
        assert_eq!(
            offered_groups(&groups, &["test:a".into(), "test:b".into()]),
            vec![0]
        );
        assert!(offered_groups(&groups, &["test:none".into()]).is_empty());
        let result = plan(&groups[0], |id| {
            Some(match id {
                "test:b" => vec![(7, 32), (2, 0)],
                "test:a" => vec![(2, 0), (4, 16)],
                _ => unreachable!(),
            })
        })
        .unwrap();
        assert_eq!(
            result,
            vec![
                Placement {
                    index: 7,
                    spacing: 32,
                    members: vec!["test:b".into()]
                },
                Placement {
                    index: 2,
                    spacing: 0,
                    members: vec!["test:b".into(), "test:a".into()]
                },
                Placement {
                    index: 4,
                    spacing: 16,
                    members: vec!["test:a".into()]
                },
            ]
        );
        assert!(plan(&groups[0], |_| None).is_none());
        assert_eq!(plan(&groups[0], |_| Some(vec![])), Some(vec![]));
    }

    struct Fake {
        calls: Vec<(u32, String, i32, i32)>,
        pending: bool,
    }
    impl Resolver for Fake {
        fn candidate(&mut self, _: u32, x: i32, z: i32) -> Option<Pos> {
            Some(Pos { x, y: 0, z })
        }
        fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>> {
            Lookup::Found(if placement == 7 {
                vec![
                    Pos { x: 3, y: 0, z: 0 },
                    Pos { x: 1, y: 0, z: 0 },
                    Pos { x: 2, y: 0, z: 0 },
                ]
            } else {
                vec![Pos::default()]
            })
        }
        fn start(&mut self, placement: u32, member: &str, x: i32, z: i32) -> Lookup<Pos> {
            self.calls.push((placement, member.into(), x, z));
            if self.pending {
                self.pending = false;
                return Lookup::Pending;
            }
            if x == 1 || member != "test:b" {
                return Lookup::Absent;
            }
            Lookup::Found(Pos {
                x: x * 16,
                y: 0,
                z: z * 16,
            })
        }
    }

    #[test]
    fn actual_weighted_member_and_previous_positions_control_group_results() {
        let group = group("test:group", &["test:a", "test:b"]);
        let placements = plan(&group, |_| Some(vec![(7, 0), (8, 0)])).unwrap();
        let mut search = Search::new(Pos::default(), vec![Pos { x: 32, y: 0, z: 0 }], placements);
        let mut fake = Fake {
            calls: vec![],
            pending: true,
        };
        for _ in 0..6 {
            assert_eq!(search.advance(&mut fake), Lookup::Pending);
        }
        assert_eq!(
            search.advance(&mut fake),
            Lookup::Found(Match {
                position: Pos { x: 48, y: 0, z: 0 },
                structure: "test:b".into(),
            })
        );
        // The later worker's closer position never runs; first success wins.
        assert_eq!(
            fake.calls,
            [
                (7, "test:a".into(), 1, 0),
                (7, "test:a".into(), 1, 0),
                (7, "test:b".into(), 1, 0),
                (7, "test:a".into(), 2, 0),
                (7, "test:b".into(), 2, 0),
                (7, "test:a".into(), 3, 0),
                (7, "test:b".into(), 3, 0),
            ]
        );
    }

    #[test]
    fn exhausted_placement_advances_to_next_worker_and_preserves_vertical_offset() {
        let group = group("test:group", &["test:a"]);
        let plan = plan(&group, |_| Some(vec![(7, 0), (8, 0)])).unwrap();
        let mut search = Search::new(Pos::default(), vec![Pos { x: 9, y: 0, z: -5 }], plan);
        struct Fallback(Vec<u32>);
        impl Resolver for Fallback {
            fn candidate(&mut self, _: u32, _: i32, _: i32) -> Option<Pos> {
                unreachable!()
            }
            fn rings(&mut self, _: u32) -> Lookup<Vec<Pos>> {
                Lookup::Found(vec![Pos::default()])
            }
            fn start(&mut self, placement: u32, member: &str, _: i32, _: i32) -> Lookup<Pos> {
                assert_eq!(member, "test:a");
                self.0.push(placement);
                if placement == 7 {
                    // Same complete locate position is ignored.
                    Lookup::Found(Pos { x: 9, y: 0, z: -5 })
                } else {
                    // Same X/Z but a different Y is a different upstream BlockPos.
                    Lookup::Found(Pos { x: 9, y: 7, z: -5 })
                }
            }
        }
        let mut resolver = Fallback(Vec::new());
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(search.advance(&mut resolver), Lookup::Pending);
        assert_eq!(
            search.advance(&mut resolver),
            Lookup::Found(Match {
                position: Pos { x: 9, y: 7, z: -5 },
                structure: "test:a".into(),
            })
        );
        assert_eq!(resolver.0, [7, 8]);
    }
}
