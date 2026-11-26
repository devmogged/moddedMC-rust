//! Plain palette blocks and ingredients from Create's released registries.

#[rustfmt::skip]
#[path = "decor_data.rs"]
mod data;

#[rustfmt::skip]
#[path = "decor_slab_data.rs"]
mod slab_data;

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, BlockMaterial, Content, Placement, State},
    items::Item,
};

use crate::{
    ITEMS,
    belt_tunnel::{read, shift},
    kinetics::face_axis,
};

const AXES: [&str; 3] = ["x", "y", "z"];
const AXIS_PROPERTIES: [&str; 3] = ["axis=x", "axis=y", "axis=z"];
const VERTICAL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

fn state<'a>(properties: &'a str, physical: &'a str, model: &'a str, x: i16, y: i16) -> State<'a> {
    State {
        properties,
        physical,
        model,
        rotating_model: "",
        x,
        y,
        orientation: VERTICAL,
        shape: &[],
    }
}

/// `PaletteBlockPartial` copies the pattern block's `AllPaletteStoneTypes` settings.
fn stone_material(path: &str) -> Option<BlockMaterial<'static>> {
    let (template, hardness, map_color) = path.split('_').find_map(|part| {
        Some(match part {
            "andesite" => ("minecraft:andesite", None, None),
            "asurine" => ("minecraft:deepslate", Some(1.25), Some("color_blue")),
            "calcite" => ("minecraft:calcite", None, None),
            "crimsite" => ("minecraft:deepslate", Some(1.25), Some("color_red")),
            "deepslate" => ("minecraft:deepslate", None, None),
            "diorite" => ("minecraft:diorite", None, None),
            "dripstone" => ("minecraft:dripstone_block", None, None),
            "granite" => ("minecraft:granite", None, None),
            "limestone" => ("minecraft:sandstone", Some(1.25), Some("sand")),
            "ochrum" => ("minecraft:calcite", Some(1.25), Some("terracotta_yellow")),
            "scoria" => ("minecraft:blackstone", None, Some("color_brown")),
            "scorchia" => ("minecraft:blackstone", None, Some("terracotta_gray")),
            "tuff" => ("minecraft:tuff", None, None),
            "veridium" => ("minecraft:tuff", Some(1.25), Some("warped_nylium")),
            _ => return None,
        })
    })?;
    Some(BlockMaterial {
        template,
        hardness,
        map_color,
    })
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for &(path, name, physical, model) in data::BLOCKS {
        let mut state = state("", physical, model, 0, 0);
        // The stock leaf carrier draws cutout, so use a display for transparent glass.
        if physical == "minecraft:glass" {
            state.model = "";
            state.rotating_model = model;
        }
        let states = [state];
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
            state(AXIS_PROPERTIES[1], physical, vertical, 0, 0),
            state(AXIS_PROPERTIES[0], physical, horizontal, 90, 90),
            state(AXIS_PROPERTIES[2], physical, horizontal, 90, 0),
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
    for &(path, name, physical, double) in slab_data::SLABS {
        let id = format!("create:{path}");
        let material = stone_material(path).ok_or(RegistrationError::InvalidDefinition)?;
        let bottom = format!("create:block/{path}");
        let top = format!("create:block/{path}_top");
        let variants = [
            ("type=bottom,waterlogged=false", bottom.as_str()),
            ("type=bottom,waterlogged=true", bottom.as_str()),
            ("type=top,waterlogged=false", top.as_str()),
            ("type=top,waterlogged=true", top.as_str()),
            ("type=double,waterlogged=false", double),
            ("type=double,waterlogged=true", double),
        ];
        let templates = variants.map(|(properties, _)| format!("{physical}[{properties}]"));
        let states = std::array::from_fn::<_, 6, _>(|i| {
            let (properties, model) = variants[i];
            state(properties, &templates[i], model, 0, 0)
        });
        content.add_block(
            registry,
            &Block {
                id: &id,
                display_name: name,
                entity: "",
                item_model: &format!("create:item/{path}"),
                states: &states,
            },
            None,
        )?;
        content.set_block_material(registry, &id, &material)?;
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
