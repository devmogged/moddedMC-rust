//! One controller per connected network, with only loaded cable and device nodes.
//! Matches Refined Storage's network graph ownership and never merges competing
//! controller domains. The loaded-world graph is rebuilt on demand.

use moddedmc_api::Pos;
use std::collections::{HashMap, HashSet, VecDeque};

const LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Controller,
    Cable,
    Drive,
    Storage,
    Grid,
    CraftingGrid,
    PatternGrid,
    Autocrafter,
    Crafter,
    CrafterManager,
    External,
    Importer,
    Exporter,
    Wireless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Node {
    pub role: Role,
    pub face: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    MissingController,
    MultipleControllers,
    TooLarge,
    Unloaded,
}

#[derive(Clone, Debug)]
pub struct Network {
    pub controller: Pos,
    pub nodes: HashMap<Pos, Node>,
    pub drives: Vec<Pos>,
    pub storage_blocks: Vec<Pos>,
    pub external: Vec<(Pos, u8)>,
    pub importers: Vec<(Pos, u8)>,
    pub exporters: Vec<(Pos, u8)>,
    pub transmitters: Vec<Pos>,
    pub autocrafters: Vec<Pos>,
    pub crafters: Vec<(Pos, u8)>,
    pub crafter_managers: Vec<Pos>,
}

pub fn neighbor(pos: Pos, face: u8) -> Option<Pos> {
    let (x, y, z) = match face {
        0 => (pos.x, pos.y.checked_sub(1)?, pos.z),
        1 => (pos.x, pos.y.checked_add(1)?, pos.z),
        2 => (pos.x, pos.y, pos.z.checked_sub(1)?),
        3 => (pos.x, pos.y, pos.z.checked_add(1)?),
        4 => (pos.x.checked_sub(1)?, pos.y, pos.z),
        5 => (pos.x.checked_add(1)?, pos.y, pos.z),
        _ => return None,
    };
    Some(Pos { x, y, z })
}

/// Probe returns Err for an unloaded world region. An absent or ordinary
/// vanilla block is Ok(None). Boundary failures do not widen the graph.
pub fn discover(
    mut probe: impl FnMut(Pos) -> Result<Option<Node>, ()>,
    origin: Pos,
) -> Result<Network, Error> {
    let initial = probe(origin).map_err(|_| Error::Unloaded)?;
    if initial.is_none() {
        return Err(Error::MissingController);
    }
    let mut nodes = HashMap::new();
    let mut queued = HashSet::from([origin]);
    let mut queue = VecDeque::from([origin]);
    let mut controller = None;
    while let Some(pos) = queue.pop_front() {
        let Some(node) = probe(pos).map_err(|_| Error::Unloaded)? else {
            continue;
        };
        if nodes.insert(pos, node).is_some() {
            continue;
        }
        if nodes.len() > LIMIT {
            return Err(Error::TooLarge);
        }
        if node.role == Role::Controller {
            if controller.replace(pos).is_some() {
                return Err(Error::MultipleControllers);
            }
        }
        for face in 0..6 {
            let Some(adjacent) = neighbor(pos, face) else {
                continue;
            };
            if queued.insert(adjacent) {
                queue.push_back(adjacent);
            }
        }
    }
    let controller = controller.ok_or(Error::MissingController)?;
    let mut positions: Vec<_> = nodes.keys().copied().collect();
    positions.sort_by_key(|p| (p.x, p.y, p.z));
    let mut drives = Vec::new();
    let mut storage_blocks = Vec::new();
    let mut external = Vec::new();
    let mut importers = Vec::new();
    let mut exporters = Vec::new();
    let mut transmitters = Vec::new();
    let mut autocrafters = Vec::new();
    let mut crafters = Vec::new();
    let mut crafter_managers = Vec::new();
    for pos in positions {
        let node = nodes[&pos];
        match node.role {
            Role::Drive => drives.push(pos),
            Role::Storage => storage_blocks.push(pos),
            Role::External => external.push((pos, node.face)),
            Role::Importer => importers.push((pos, node.face)),
            Role::Exporter => exporters.push((pos, node.face)),
            Role::Wireless => transmitters.push(pos),
            Role::Autocrafter => autocrafters.push(pos),
            Role::Crafter => crafters.push((pos, node.face)),
            Role::CrafterManager => crafter_managers.push(pos),
            _ => {}
        }
    }
    Ok(Network {
        controller,
        nodes,
        drives,
        storage_blocks,
        external,
        importers,
        exporters,
        transmitters,
        autocrafters,
        crafters,
        crafter_managers,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x: i32, z: i32) -> Pos {
        Pos { x, y: 80, z }
    }
    fn n(role: Role) -> Node {
        Node { role, face: 5 }
    }
    #[test]
    fn only_connected_devices_with_one_controller_join_network() {
        let blocks = HashMap::from([
            (p(0, 0), n(Role::Controller)),
            (p(1, 0), n(Role::Cable)),
            (p(2, 0), n(Role::Drive)),
            (p(1, 1), n(Role::Grid)),
            (p(4, 0), n(Role::Drive)),
        ]);
        let found = discover(|pos| Ok(blocks.get(&pos).copied()), p(1, 1)).unwrap();
        assert_eq!(found.controller, p(0, 0));
        assert_eq!(found.drives, vec![p(2, 0)]);
        assert_eq!(found.nodes.len(), 4);
        assert_eq!(
            discover(|pos| Ok(blocks.get(&pos).copied()), p(4, 0)).unwrap_err(),
            Error::MissingController
        );
    }
    #[test]
    fn two_controllers_and_unloaded_world_fail_closed() {
        let blocks = HashMap::from([
            (p(0, 0), n(Role::Controller)),
            (p(1, 0), n(Role::Cable)),
            (p(2, 0), n(Role::Controller)),
        ]);
        assert_eq!(
            discover(|pos| Ok(blocks.get(&pos).copied()), p(1, 0)).unwrap_err(),
            Error::MultipleControllers
        );
        assert_eq!(
            discover(
                |pos| if pos == p(2, 0) {
                    Err(())
                } else {
                    Ok(blocks.get(&pos).copied())
                },
                p(0, 0)
            )
            .unwrap_err(),
            Error::Unloaded
        );
    }
    #[test]
    fn physical_device_orientation_is_stable() {
        assert_eq!(neighbor(p(1, 2), 0).unwrap().y, 79);
        assert_eq!(neighbor(p(1, 2), 5), Some(p(2, 2)));
        assert!(neighbor(p(1, 2), 7).is_none());
    }

    #[test]
    fn v1124_crafters_and_manager_are_members_of_the_same_controller_network() {
        let blocks = HashMap::from([
            (p(0, 0), n(Role::Controller)),
            (p(1, 0), n(Role::Cable)),
            (p(2, 0), n(Role::Crafter)),
            (p(1, 1), n(Role::CrafterManager)),
        ]);
        let network = discover(|pos| Ok(blocks.get(&pos).copied()), p(2, 0)).unwrap();
        assert_eq!(network.crafters, vec![(p(2, 0), 5)]);
        assert_eq!(network.crafter_managers, vec![p(1, 1)]);
    }
}
