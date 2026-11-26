//! Create 6.0.10's Mechanical Saw and TreeCutter.

use std::collections::{HashSet, VecDeque};

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{Block, Content, Placement, State, World},
    stacks::ItemDisplay,
};

use crate::{
    CONTENT, DIRECTIONS, STONE, StateIds, TAGS,
    belt_tunnel::{read, shift},
    directed_detector::orientation,
    display_model::ModelDisplay,
    drill,
    kinetics::{Kind, directional_shaft_axis, face_axis, offset, positive},
};

pub(crate) const ID: &str = "create:mechanical_saw";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SawBlock {
    pub(crate) facing: u8,
    along_first: bool,
    flipped: bool,
}

impl SawBlock {
    fn all() -> impl Iterator<Item = Self> {
        (0..6).flat_map(|facing| {
            [false, true].into_iter().flat_map(move |along_first| {
                [false, true].map(|flipped| Self {
                    facing,
                    along_first,
                    flipped,
                })
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "axis_along_first={},facing={},flipped={}",
            self.along_first,
            DIRECTIONS[usize::from(self.facing)],
            self.flipped
        )
    }

    pub(crate) const fn horizontal(self) -> bool {
        self.facing >= 2
    }

    pub(crate) const fn axis(self) -> u8 {
        if self.horizontal() {
            face_axis(self.facing)
        } else {
            directional_shaft_axis(self.facing, self.along_first)
        }
    }

    pub(crate) const fn output_direction(self, speed: f32) -> u8 {
        (if self.along_first { 2 } else { 5 }) ^ ((speed < 0.0) as u8)
    }

    pub(crate) fn filter_center(self) -> [f64; 2] {
        let offset = if self.flipped { -3.0 } else { 3.0 } / 16.0;
        if self.along_first {
            [0.5 + offset, 0.5]
        } else {
            [0.5, 0.5 + offset]
        }
    }

    fn casing_rotation(self) -> (i16, i16) {
        if self.horizontal() {
            (0, (360 - crate::belt::horizontal_angle(self.facing)) % 360)
        } else {
            let x = if self.facing == 0 { 180 } else { 0 };
            let y =
                (if self.along_first { 270 } else { 0 } + if self.flipped { 180 } else { 0 }) % 360;
            (x, y)
        }
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows: Vec<_> = SawBlock::all()
        .map(|block| {
            let shape = [crate::piston::axial_box(block.facing, 0, 12, 0, 16)];
            (block, block.properties(), shape)
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(block, properties, shape)| {
            let (x, y) = block.casing_rotation();
            State {
                properties,
                physical: STONE,
                model: if block.horizontal() {
                    "create:block/mechanical_saw/horizontal"
                } else {
                    "create:block/mechanical_saw/vertical"
                },
                rotating_model: if block.horizontal() {
                    "create:block/shaft_half"
                } else {
                    "create:block/shaft"
                },
                x,
                y,
                orientation: if block.horizontal() {
                    orientation(0, (y + 180) % 360)
                } else {
                    crate::relay_orientation(block.axis())
                },
                shape,
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Mechanical Saw",
            item_model: "create:item/mechanical_saw",
            entity: ID,
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for block in SawBlock::all() {
        states.add(content, ID, &block.properties(), Some(Kind::Saw(block)));
    }
}

pub(crate) fn placed(
    args: &Placement<'_>,
    look: u8,
    states: &StateIds,
    neighbors: &[u32; 7],
) -> Option<u32> {
    let (facing, along_first) = crate::deployer::placement_axis(args, look, states, neighbors);
    let block = SawBlock {
        facing,
        along_first,
        flipped: facing < 2 && positive(crate::nearest_looking_direction(args.yaw, 0.0)),
    };
    states.lookup(&format!("{ID}[{}]", block.properties()))
}

pub(crate) fn show_blade(
    world: World<'_>,
    pos: Pos,
    block: SawBlock,
    speed: f32,
    display: &mut ModelDisplay,
) {
    let model = match (block.horizontal(), speed.total_cmp(&0.0)) {
        (true, std::cmp::Ordering::Greater) => "create:moddedmc/parts/saw/horizontal_active",
        (true, std::cmp::Ordering::Less) if speed != 0.0 => {
            "create:moddedmc/parts/saw/horizontal_reversed"
        }
        (true, _) => "create:moddedmc/parts/saw/horizontal_inactive",
        (false, std::cmp::Ordering::Greater) => "create:moddedmc/parts/saw/vertical_active",
        (false, std::cmp::Ordering::Less) if speed != 0.0 => {
            "create:moddedmc/parts/saw/vertical_reversed"
        }
        (false, _) => "create:moddedmc/parts/saw/vertical_inactive",
    };
    let (x, y) = if block.horizontal() {
        block.casing_rotation()
    } else {
        let pitch = if block.facing == 1 { 90 } else { -90 };
        let yaw = if block.along_first { 270 } else { 0 };
        (pitch, yaw)
    };
    let [x_pos, y_pos, z_pos] = [pos.x, pos.y, pos.z].map(|c| f64::from(c) + 0.5);
    display.show(
        world,
        model,
        &ItemDisplay {
            position: [x_pos, y_pos, z_pos],
            rotation: orientation(x, y),
            scale: 1.0,
            context: 0,
            interpolation: 0,
        },
    );
}

fn in_tag(id: &str, tag: &str) -> bool {
    TAGS.get()
        .is_some_and(|tags| tags.contains("block", tag, id))
}

fn is_root(id: &str) -> bool {
    in_tag(id, "create:roots")
}

fn is_log(id: &str) -> bool {
    id == "minecraft:mushroom_stem"
        || in_tag(id, "minecraft:logs")
        || in_tag(id, "tconstruct:slimy_logs")
}

fn is_vertical_plant(id: &str) -> bool {
    matches!(
        id,
        "minecraft:bamboo"
            | "minecraft:cactus"
            | "minecraft:sugar_cane"
            | "minecraft:kelp"
            | "minecraft:kelp_plant"
    )
}

fn is_chorus(id: &str) -> bool {
    matches!(id, "minecraft:chorus_plant" | "minecraft:chorus_flower")
}

pub(crate) fn is_sawable(id: &str) -> bool {
    !in_tag(id, "minecraft:saplings")
        && (is_log(id)
            || is_root(id)
            || in_tag(id, "minecraft:leaves")
            || is_vertical_plant(id)
            || matches!(
                id,
                "minecraft:pumpkin" | "minecraft:melon" | "minecraft:chorus_plant"
            ))
}

fn describe(world: World<'_>, pos: Pos) -> String {
    CONTENT
        .get()
        .and_then(|content| content.describe_state(read(world, pos)))
        .unwrap_or_default()
}

fn name(selector: &str) -> &str {
    selector.split_once('[').map_or(selector, |(name, _)| name)
}

fn leaf_distance(selector: &str) -> Option<i32> {
    let (_, properties) = selector.strip_suffix(']')?.split_once('[')?;
    properties
        .split(',')
        .find_map(|entry| entry.strip_prefix("distance=")?.parse().ok())
}

fn giant_leaf_distance(id: &str) -> i32 {
    if id == "minecraft:red_mushroom_block" {
        2
    } else if id == "minecraft:brown_mushroom_block"
        || matches!(
            id,
            "minecraft:weeping_vines" | "minecraft:weeping_vines_plant"
        )
        || in_tag(id, "minecraft:wart_blocks")
    {
        3
    } else {
        -1
    }
}

fn neighbours(pos: Pos, min_y: i32, max_y: i32) -> impl Iterator<Item = Pos> {
    (-1..=1).flat_map(move |z| {
        (min_y..=max_y).flat_map(move |y| (-1..=1).map(move |x| offset(pos, [x, y, z])))
    })
}

fn supported_above_cut(world: World<'_>, cut: Pos) -> bool {
    let mut queue = VecDeque::from([cut, shift(cut, 1)]);
    let mut visited = HashSet::new();
    while let Some(pos) = queue.pop_front() {
        if !visited.insert(pos) {
            continue;
        }
        let state = describe(world, pos);
        if !is_log(name(&state)) && !is_root(name(&state)) {
            continue;
        }
        let lower_layer = pos.y == cut.y;
        if !lower_layer {
            let below = shift(pos, 0);
            if below != cut {
                let below = describe(world, below);
                if is_log(name(&below)) || is_root(name(&below)) {
                    return true;
                }
            }
        }
        queue.extend(
            (if lower_layer { 1 } else { 2 }..6)
                .map(|side| shift(pos, side))
                .filter(|pos| !visited.contains(pos)),
        );
    }
    false
}

#[derive(Default)]
struct Tree {
    logs: Vec<Pos>,
    leaves: Vec<Pos>,
    attachments: Vec<Pos>,
}

fn regular_tree(world: World<'_>, cut: Pos) -> Tree {
    let mut tree = Tree::default();
    if supported_above_cut(world, cut) {
        return tree;
    }
    let mut seen = HashSet::from([cut]);
    let mut queue: VecDeque<_> = neighbours(cut, 0, 1).filter(|&pos| pos != cut).collect();
    seen.extend(queue.iter().copied());
    let mut roots = Vec::new();
    while let Some(pos) = queue.pop_front() {
        let state = describe(world, pos);
        if is_root(name(&state)) {
            roots.push(pos);
        } else if !is_log(name(&state)) {
            continue;
        }
        tree.logs.push(pos);
        queue.extend(neighbours(pos, 0, 1).filter(|&next| seen.insert(next)));
    }
    seen.clear();
    seen.extend(tree.logs.iter().copied());
    queue.extend(roots);
    while let Some(pos) = queue.pop_front() {
        for next in neighbours(pos, -1, 0).filter(|&next| seen.insert(next)) {
            if is_root(name(&describe(world, next))) {
                tree.logs.push(next);
                queue.push_back(next);
            }
        }
    }
    seen.clear();
    seen.extend(tree.logs.iter().copied());
    queue.extend(tree.logs.iter().copied());
    while let Some(pos) = queue.pop_front() {
        let prev_distance = leaf_distance(&describe(world, pos)).unwrap_or(0);
        for next in neighbours(pos, -1, 1) {
            if seen.contains(&next) {
                continue;
            }
            let state = describe(world, next);
            let id = name(&state);
            if in_tag(id, "create:tree_attachments") {
                tree.attachments.push(next);
                seen.insert(next);
                continue;
            }
            let distance = (next.x - cut.x).abs().max((next.z - cut.z).abs());
            if distance <= giant_leaf_distance(id)
                || leaf_distance(&state).is_some_and(|distance| distance > prev_distance)
            {
                tree.leaves.push(next);
                seen.insert(next);
                queue.push_back(next);
            }
        }
    }
    tree
}

fn tree_at(world: World<'_>, cut: Pos, broken: &str) -> Tree {
    let mut tree = Tree::default();
    if is_vertical_plant(broken) {
        let mut pos = shift(cut, 1);
        while is_vertical_plant(name(&describe(world, pos))) {
            tree.logs.push(pos);
            pos = shift(pos, 1);
        }
        tree.logs.reverse();
    } else if is_chorus(broken) {
        let mut queue = VecDeque::from([shift(cut, 1)]);
        let mut seen = HashSet::from([cut]);
        while let Some(pos) = queue.pop_front() {
            if !seen.insert(pos) || !is_chorus(name(&describe(world, pos))) {
                continue;
            }
            tree.logs.push(pos);
            queue.extend((0..6).map(|side| shift(pos, side)));
        }
        tree.logs.reverse();
    } else {
        return regular_tree(world, cut);
    }
    tree
}

pub(crate) fn cut_tree(world: World<'_>, cut: Pos, facing: u8, broken: &str) {
    let tree = tree_at(world, cut, broken);
    let forward = crate::kinetics::FACES[usize::from(facing)].map(f64::from);
    for pos in tree
        .attachments
        .into_iter()
        .chain(tree.logs)
        .chain(tree.leaves)
    {
        let dx = f64::from(pos.x - cut.x);
        let dy = f64::from(pos.y - cut.y);
        let dz = f64::from(pos.z - cut.z);
        let distance = dx.hypot(dy).hypot(dz);
        drill::break_and_drop(world, pos, forward.map(|step| step * distance / 20.0));
    }
}
