//! Plain palette blocks and ingredients from Create's released registries.

#[rustfmt::skip]
#[path = "decor_data.rs"]
mod data;

use std::f32::consts::FRAC_1_SQRT_2;

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, Content, Placement, State},
    items::Item,
};

use crate::{
    ITEMS,
    belt_tunnel::{read, shift},
    kinetics::face_axis,
};

const AXES: [&str; 3] = ["x", "y", "z"];
const AXIS_PROPERTIES: [&str; 3] = ["axis=x", "axis=y", "axis=z"];
const TURN: f32 = FRAC_1_SQRT_2;
const VERTICAL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn display_state<'a>(
    properties: &'a str,
    physical: &'a str,
    model: &'a str,
    orientation: [f32; 4],
) -> State<'a> {
    State {
        properties,
        physical,
        model: "",
        rotating_model: model,
        x: 0,
        y: 0,
        orientation,
        shape: &[],
    }
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for &(path, name, physical, model) in data::BLOCKS {
        // The palette shares the existing empty carrier and draws the released models as displays.
        let states = [display_state("", physical, model, VERTICAL)];
        content.add_block(
            registry,
            &Block {
                id: &format!("create:{path}"),
                display_name: name,
                entity: "",
                item_model: &format!("create:item/{path}"),
                states: &states,
            },
            None,
        )?;
    }
    for &(path, name, physical, vertical, horizontal) in data::PILLARS {
        let states = [
            display_state(AXIS_PROPERTIES[1], physical, vertical, VERTICAL),
            display_state(
                AXIS_PROPERTIES[0],
                physical,
                horizontal,
                [-0.5, -0.5, -0.5, 0.5],
            ),
            display_state(
                AXIS_PROPERTIES[2],
                physical,
                horizontal,
                [-TURN, 0.0, 0.0, TURN],
            ),
        ];
        content.add_block(
            registry,
            &Block {
                id: &format!("create:{path}"),
                display_name: name,
                entity: "",
                item_model: &format!("create:item/{path}"),
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    let items = ITEMS.get().ok_or(RegistrationError::InvalidDefinition)?;
    for &(path, display_name, carrier) in data::ITEMS {
        items.add_item(
            registry,
            &Item {
                id: &format!("create:{path}"),
                display_name,
                carrier,
                model: &format!("create:item/{path}"),
            },
        )?;
    }
    Ok(())
}

pub(crate) fn placed_state(args: &Placement<'_>, content: Content) -> Option<u32> {
    let default = content.describe_state(args.state)?;
    let (name, _) = default.split_once('[')?;
    let path = name.strip_prefix("create:")?;
    data::PILLARS
        .binary_search_by_key(&path, |entry| entry.0)
        .ok()?;
    let mut properties = AXIS_PROPERTIES[usize::from(face_axis(args.face))];
    if path != "rose_quartz_block"
        && !args.sneak
        && let Some(adjacent) =
            content.describe_state(read(args.world, shift(args.position, args.face ^ 1)))
        && let Some((neighbor, axis)) = adjacent.split_once('[')
        && neighbor == name
        && let Some(axis) = axis
            .strip_prefix("axis=")
            .and_then(|axis| axis.strip_suffix(']'))
        && let Some(index) = AXES.iter().position(|&property| property == axis)
    {
        properties = AXIS_PROPERTIES[index];
    }
    content.state(&format!("{name}[{properties}]"))
}
