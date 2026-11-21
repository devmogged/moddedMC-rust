use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{
        self, Block, BlockUpdate, Content, Placement, ShapeBox, State, UNLOADED_STATE, World,
    },
    interaction::RedstoneQuery,
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTENT, DIRECTIONS, STONE, StateIds,
    contraption::Blocks,
    kinetics::{FACES, Kind, face_axis, offset},
};

pub const ID: &str = "create:elevator_pulley";
pub const CONTACT: &str = "create:redstone_contact";
const ELEVATOR_CONTACT: &str = "create:elevator_contact";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Contact {
    facing: u8,
    elevator: bool,
    powered: bool,
    powering: bool,
    calling: bool,
}

impl Contact {
    fn properties(self) -> String {
        let facing = DIRECTIONS[usize::from(self.facing)];
        if self.elevator {
            format!(
                "facing={facing},powered={},powering={},calling={}",
                self.powered, self.powering, self.calling
            )
        } else {
            format!("facing={facing},powered={}", self.powered)
        }
    }

    fn block(self) -> &'static str {
        if self.elevator {
            ELEVATOR_CONTACT
        } else {
            CONTACT
        }
    }

    fn state(self) -> Option<u32> {
        CONTENT
            .get()?
            .state(&format!("{}[{}]", self.block(), self.properties()))
    }

    fn source(self) -> bool {
        if self.elevator {
            self.powering
        } else {
            self.powered
        }
    }
}

fn contacts(elevator: bool) -> Vec<Contact> {
    let faces = if elevator {
        [3, 2, 4, 5, 0, 1]
    } else {
        [1, 0, 2, 3, 4, 5]
    };
    let mut states = Vec::new();
    for facing in faces {
        for powered in [false, true] {
            for &powering in if elevator {
                &[false, true][..]
            } else {
                &[false]
            } {
                for &calling in if elevator {
                    &[false, true][..]
                } else {
                    &[false]
                } {
                    states.push(Contact {
                        facing,
                        elevator,
                        powered,
                        powering,
                        calling,
                    });
                }
            }
        }
    }
    states
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let model = "create:block/elevator_pulley/block";
    // AllShapes.ELEVATOR_PULLEY, oriented along the shaft axis.
    let x = [
        ShapeBox {
            min: [0, 0, 0],
            max: [2, 16, 16],
        },
        ShapeBox {
            min: [14, 0, 0],
            max: [16, 16, 16],
        },
        ShapeBox {
            min: [2, 0, 2],
            max: [14, 14, 14],
        },
    ];
    let z = [
        ShapeBox {
            min: [0, 0, 0],
            max: [16, 16, 2],
        },
        ShapeBox {
            min: [0, 0, 14],
            max: [16, 16, 16],
        },
        x[2],
    ];
    let facings = [
        ("north", 0, &x),
        ("east", 90, &z),
        ("south", 180, &x),
        ("west", 270, &z),
    ];
    let selectors = facings.map(|(facing, _, _)| format!("facing={facing}"));
    let states = std::array::from_fn::<_, 4, _>(|i| State {
        properties: &selectors[i],
        physical: STONE,
        model,
        rotating_model: "",
        x: 0,
        y: facings[i].1,
        orientation: crate::relay_orientation(if i % 2 == 0 { 0 } else { 2 }),
        shape: facings[i].2,
    });
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Elevator Pulley",
            entity: ID,
            item_model: "create:item/elevator_pulley",
            states: &states,
        },
        Some(crate::placed_state),
    )?;

    for (elevator, id, name, item) in [
        (
            false,
            CONTACT,
            "Redstone Contact",
            "create:block/redstone_contact/block",
        ),
        (true, ELEVATOR_CONTACT, "Elevator Contact", ""),
    ] {
        let contacts = contacts(elevator);
        let selectors: Vec<_> = contacts
            .iter()
            .map(|contact| contact.properties())
            .collect();
        let states: Vec<_> = contacts
            .iter()
            .zip(&selectors)
            .map(|(contact, selector)| {
                let (x, y) = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)]
                    [usize::from(contact.facing)];
                let model = match (elevator, contact.source(), contact.calling) {
                    (true, true, _) => "create:block/elevator_contact/block_powered",
                    (true, false, true) => "create:block/elevator_contact/block_dim",
                    (true, false, false) => "create:block/elevator_contact/block",
                    (false, true, _) => "create:block/redstone_contact/block_powered",
                    (false, false, _) => "create:block/redstone_contact/block",
                };
                State {
                    properties: selector,
                    physical: STONE,
                    model,
                    rotating_model: "",
                    x,
                    y,
                    orientation: [0.0, 0.0, 0.0, 1.0],
                    shape: &[],
                }
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id,
                display_name: name,
                entity: "",
                item_model: item,
                states: &states,
            },
            if elevator {
                None
            } else {
                Some(crate::placed_state)
            },
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for (facing, name) in DIRECTIONS.iter().enumerate().skip(2) {
        ids.add(
            content,
            ID,
            &format!("facing={name}"),
            Some(Kind::Elevator(facing as u8)),
        );
    }
    for contact in contacts(false).into_iter().chain(contacts(true)) {
        ids.add(content, contact.block(), &contact.properties(), None);
        if let Some(id) = contact.state() {
            ids.contacts.insert(id, contact);
        }
    }
}

pub fn placed_pulley(yaw: f32) -> Option<u32> {
    let look = crate::nearest_looking_direction(yaw, 0.0);
    CONTENT.get()?.state(&format!(
        "{ID}[facing={}]",
        DIRECTIONS[usize::from(look ^ 1)]
    ))
}

pub fn placed_contact(args: &Placement<'_>, look: u8, states: &StateIds) -> Option<u32> {
    let toward_support = args.face ^ 1;
    let facing =
        if args.sneak || has_valid_contact(args.world, states, args.position, toward_support) {
            toward_support
        } else {
            look ^ 1
        };
    Contact {
        facing,
        elevator: false,
        powered: has_valid_contact(args.world, states, args.position, facing),
        powering: false,
        calling: false,
    }
    .state()
}

fn read_contact(world: World<'_>, states: &StateIds, position: Pos) -> Option<Contact> {
    let mut ids = [UNLOADED_STATE];
    world.read(&[position], &mut ids);
    states.contacts.get(&ids[0]).copied()
}

fn has_valid_contact(world: World<'_>, states: &StateIds, position: Pos, facing: u8) -> bool {
    read_contact(world, states, offset(position, FACES[usize::from(facing)]))
        .is_some_and(|contact| contact.facing == facing ^ 1)
}

fn write_contact(world: World<'_>, position: Pos, contact: Contact, flags: u8) {
    if let Some(state) = contact.state() {
        world.write(&[BlockUpdate {
            position,
            state,
            flags,
        }]);
    }
}

pub fn event(world: World<'_>, states: &StateIds, event: &content::ContentEvent) {
    if event.reason == content::UNLOADED
        || !(states.contacts.contains_key(&event.state)
            || states.contacts.contains_key(&event.previous))
    {
        return;
    }
    for position in
        std::iter::once(event.position).chain(FACES.map(|face| offset(event.position, face)))
    {
        let Some(mut contact) = read_contact(world, states, position) else {
            continue;
        };
        if !contact.elevator {
            let powered = has_valid_contact(world, states, position, contact.facing);
            if contact.powered != powered {
                contact.powered = powered;
                write_contact(world, position, contact, content::UPDATE_ALL);
            }
        }
    }
}

pub fn connects(id: u32, direction: u8) -> bool {
    crate::states()
        .and_then(|states| states.contacts.get(&id))
        .is_some_and(|contact| contact.source() && contact.facing != direction ^ 1)
}

pub fn signal(query: &RedstoneQuery<'_>) -> u8 {
    let Some(states) = crate::states() else {
        return 0;
    };
    let Some(contact) = states.contacts.get(&query.state) else {
        return 0;
    };
    if query.direct || !contact.source() || usize::from(query.direction) >= FACES.len() {
        return 0;
    }
    let target = offset(query.position, FACES[usize::from(query.direction ^ 1)]);
    if contact.elevator {
        if read_contact(query.world, states, target).is_some_and(|target| target.elevator) {
            return 0;
        }
    } else if contact.facing == query.direction ^ 1 {
        return 0;
    }
    15
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct ElevatorMotion {
    pub column: [i32; 2],
    pub contact_y: i32,
    side: u8,
    floors: Vec<i32>,
    pub target: i32,
    pub arrived: bool,
}

impl ElevatorMotion {
    pub fn from_blocks(blocks: &Blocks, anchor: Pos) -> Option<Self> {
        let mut contacts = blocks.block_list.iter().filter_map(|block| {
            let entry = blocks.palette.get(block.palette_index())?;
            let name = entry.properties.get("facing")?;
            let facing = DIRECTIONS.iter().position(|direction| direction == name)?;
            (entry.name == CONTACT && facing >= 2).then_some((block.local_pos(), facing))
        });
        let (local, facing) = contacts.next()?;
        if contacts.next().is_some() {
            return None;
        }
        let contact = offset(offset(anchor, local), FACES[facing]);
        Some(Self {
            column: [contact.x, contact.z],
            contact_y: local[1],
            side: facing as u8 ^ 1,
            floors: Vec::new(),
            target: contact.y,
            arrived: true,
        })
    }

    pub fn same_column(&self, other: &Self) -> bool {
        self.column == other.column && self.side == other.side
    }

    fn at(&self, y: i32) -> Pos {
        let [x, z] = self.column;
        Pos { x, y, z }
    }

    pub fn gather_all(&mut self, world: World<'_>, states: &StateIds, top: i32, range: i32) {
        // shortcut: gather within the pulley's reachable shaft until the ABI exposes build bounds.
        for y in top - range..=top {
            let pos = self.at(y);
            let Some(mut contact) = read_contact(world, states, pos) else {
                continue;
            };
            if self.side != contact.facing {
                continue;
            }
            if !contact.elevator {
                contact.elevator = true;
                write_contact(world, pos, contact, content::UPDATE_ALL);
            }
            self.floors.push(y);
        }
    }

    pub fn update_calls(&mut self, world: World<'_>, states: &StateIds) {
        let mut called = None;
        for &y in &self.floors {
            let pos = self.at(y);
            let Some(mut contact) = read_contact(world, states, pos) else {
                continue;
            };
            if !contact.elevator || contact.facing != self.side {
                continue;
            }
            let powered = world.has_neighbor_signal(pos);
            if contact.powered != powered {
                contact.powered = powered;
                if powered && !contact.calling {
                    called = Some(y);
                }
                write_contact(world, pos, contact, content::UPDATE_CLIENTS);
            }
        }
        if let Some(y) = called {
            self.target = y;
            self.arrived = false;
            for &floor in &self.floors {
                let pos = self.at(floor);
                if let Some(mut contact) = read_contact(world, states, pos)
                    && contact.elevator
                    && contact.facing == self.side
                {
                    contact.calling = floor == y;
                    if floor != y {
                        contact.powering = has_valid_contact(world, states, pos, self.side);
                    }
                    write_contact(world, pos, contact, content::UPDATE_ALL);
                }
            }
        }
    }

    pub fn trigger_contact(&self, world: World<'_>, states: &StateIds, powered: bool) {
        let pos = self.at(self.target);
        if let Some(mut contact) = read_contact(world, states, pos)
            && contact.elevator
            && contact.facing == self.side
            && contact.powering != powered
        {
            contact.powering = powered;
            write_contact(world, pos, contact, content::UPDATE_ALL);
        }
    }
}

/// ElevatorPulleyBlockEntity.getMovementSpeed.
pub fn movement_speed(rpm: f32, diff: f32, previous: f32) -> f32 {
    let limit = (rpm * 2.0 / 512.0).abs().min(1.99);
    let acceleration = 0.0075 + limit * (0.0175 - 0.0075);
    let deceleration = (2.0 * diff.abs() * acceleration).sqrt();
    diff.clamp(-limit, limit)
        .clamp(previous - acceleration, previous + acceleration)
        .clamp(-deceleration, deceleration)
}

pub const fn shaft_axis(facing: u8) -> u8 {
    face_axis(facing) ^ 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elevator_speeds_up_then_brakes_at_the_requested_floor() {
        let first = movement_speed(-128.0, -4.0, 0.0);
        assert!((first + 0.0125).abs() < 1e-6);
        assert_eq!(movement_speed(-128.0, -4.0, first), first * 2.0);
        assert_eq!(movement_speed(128.0, 0.0, first), 0.0);
    }
}
