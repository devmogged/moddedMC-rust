//! Create 6.0.10's StickerBlock redstone latch and contraption attachment.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{self, Block, BlockUpdate, Content, State, World},
};

use crate::{
    CONTENT, DIRECTIONS, STONE, StateIds,
    contraption::{PaletteEntry, StructureTransform},
    kinetics::{FACES, face_axis, offset},
};

pub const ID: &str = "create:sticker";
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];

#[derive(Clone, Copy)]
struct Sticker {
    facing: u8,
    extended: bool,
    powered: bool,
}

impl Sticker {
    fn variants() -> impl Iterator<Item = Self> {
        (0..6).flat_map(|facing| {
            [false, true].into_iter().flat_map(move |extended| {
                [false, true].into_iter().map(move |powered| Self {
                    facing,
                    extended,
                    powered,
                })
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "extended={},facing={},powered={}",
            self.extended,
            DIRECTIONS[usize::from(self.facing)],
            self.powered
        )
    }

    fn state(self) -> Option<u32> {
        CONTENT
            .get()?
            .state(&format!("{ID}[{}]", self.properties()))
    }

    fn from_entry(entry: &PaletteEntry) -> Option<Self> {
        if entry.name != ID {
            return None;
        }
        let facing = entry.properties.get("facing")?;
        let facing = DIRECTIONS.iter().position(|&name| facing == name)? as u8;
        Some(Self {
            facing,
            extended: entry
                .properties
                .get("extended")
                .is_some_and(|value| value == "true"),
            powered: entry
                .properties
                .get("powered")
                .is_some_and(|value| value == "true"),
        })
    }
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let definitions: Vec<_> = Sticker::variants()
        .map(|sticker| (sticker, sticker.properties()))
        .collect();
    let states: Vec<_> = definitions
        .iter()
        .map(|(sticker, properties)| {
            let (x, y) = ROTATIONS[usize::from(sticker.facing)];
            State {
                properties,
                physical: STONE,
                model: if sticker.powered {
                    "create:block/sticker/block_powered"
                } else {
                    "create:block/sticker/block"
                },
                rotating_model: "",
                x,
                y,
                orientation: crate::relay_orientation(face_axis(sticker.facing)),
                shape: &[],
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Sticker",
            entity: "",
            item_model: "create:item/sticker",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub fn add_states(content: Content, ids: &mut StateIds) {
    for sticker in Sticker::variants() {
        ids.add(content, ID, &sticker.properties(), None);
    }
}

pub fn placed_state(world: World<'_>, pos: Pos, facing: u8) -> Option<u32> {
    Sticker {
        facing,
        extended: false,
        powered: world.has_neighbor_signal(pos),
    }
    .state()
}

pub fn event(world: World<'_>, states: &StateIds, event: &content::ContentEvent) {
    if event.reason != content::NEIGHBOR_CHANGED
        || !states
            .by_id
            .get(&event.state)
            .is_some_and(|state| state.block == ID)
    {
        return;
    }
    let mut current = [content::UNLOADED_STATE];
    world.read(&[event.position], &mut current);
    let Some(mut sticker) = CONTENT
        .get()
        .and_then(|content| content.describe_state(current[0]))
        .and_then(|selector| Sticker::from_entry(&PaletteEntry::parse(&selector)))
    else {
        return;
    };
    let powered = world.has_neighbor_signal(event.position);
    if powered != sticker.powered {
        sticker.powered = powered;
        sticker.extended ^= powered;
        if let Some(state) = sticker.state() {
            world.write(&[BlockUpdate {
                position: event.position,
                state,
                flags: content::UPDATE_CLIENTS,
            }]);
        }
    }
}

pub fn attachment(entry: &PaletteEntry, pos: Pos) -> Option<Pos> {
    let sticker = Sticker::from_entry(entry)?;
    sticker
        .extended
        .then(|| offset(pos, FACES[usize::from(sticker.facing)]))
}

pub fn blocks_attachment(entry: &PaletteEntry, face: u8) -> bool {
    Sticker::from_entry(entry).is_some_and(|sticker| !sticker.extended && sticker.facing == face)
}

pub fn apply_rotation(entry: &PaletteEntry, transform: StructureTransform) -> Option<PaletteEntry> {
    let mut sticker = Sticker::from_entry(entry)?;
    sticker.facing = transform.rotate_face(sticker.facing);
    Some(PaletteEntry::parse(&format!(
        "{ID}[{}]",
        sticker.properties()
    )))
}
