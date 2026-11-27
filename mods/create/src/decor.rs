//! Plain palette blocks and ingredients from Create's released registries.

#[rustfmt::skip]
#[path = "decor_data.rs"]
mod data;

#[rustfmt::skip]
#[path = "decor_slab_data.rs"]
mod slab_data;

use std::hash::{BuildHasher, RandomState};

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, BlockMaterial, Content, Placement, State},
    effects::SOUND_BLOCKS,
    interaction::{UseOnBlock, UseResult},
    items::Item,
};

use crate::{
    CONTENT, EFFECTS, ITEMS, TAGS,
    belt_tunnel::{read, shift},
    kinetics::face_axis,
};

const AXES: [&str; 3] = ["x", "y", "z"];
const AXIS_PROPERTIES: [&str; 3] = ["axis=x", "axis=y", "axis=z"];
const VERTICAL: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

pub(crate) fn is_stone_partial(id: &str) -> bool {
    let Some((base, "slab" | "stairs" | "wall")) = id
        .strip_prefix("create:")
        .and_then(|id| id.rsplit_once('_'))
    else {
        return false;
    };
    slab_data::SLABS
        .iter()
        .any(|(path, _, _, _)| path.strip_suffix("_slab") == Some(base))
}

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
        let stairs = path
            .strip_suffix("_slab")
            .ok_or(RegistrationError::InvalidDefinition)?;
        let stairs_name = name
            .strip_suffix(" Slab")
            .ok_or(RegistrationError::InvalidDefinition)?;
        register_stairs(content, registry, stairs, stairs_name, &material)?;
        register_wall(content, registry, stairs, stairs_name, &material)?;
    }
    for &(path, name, map_color) in data::PANES {
        register_connected_block(
            content,
            registry,
            path,
            name,
            "minecraft:glass_pane",
            map_color,
        )?;
    }
    for &(path, name, map_color) in data::BARS {
        register_connected_block(
            content,
            registry,
            path,
            name,
            "minecraft:copper_bars",
            Some(map_color),
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

pub fn is_pane(path: &str) -> bool {
    data::PANES.iter().any(|(id, ..)| *id == path)
}

pub fn is_display_pane(path: &str) -> bool {
    is_pane(path)
        && !matches!(
            path,
            "framed_glass_pane" | "horizontal_framed_glass_pane" | "tiled_glass_pane"
        )
}

pub(crate) fn uses_bar_display(path: &str) -> bool {
    path.ends_with("_bars") && data::BARS.iter().any(|(id, ..)| *id == path)
}

pub(crate) fn wrench_pickup(context: &UseOnBlock<'_>) -> Option<UseResult> {
    if !context.sneaking || !context.player.is_some_and(|player| player.may_build()) {
        return None;
    }
    let world = context.world;
    let pos = context.position;
    let mut state = [0];
    if !world.read(&[pos], &mut state) {
        return None;
    }
    let description = CONTENT.get()?.describe_state(state[0])?;
    let block = description.split('[').next()?;
    if !TAGS.get()?.contains("block", "create:wrench_pickup", block) {
        return None;
    }
    let drops = if context.creative {
        Vec::new()
    } else {
        world.block_drops(pos, None)?
    };
    if !world.destroy(pos, false) {
        return Some(UseResult::Fail);
    }
    for stack in drops {
        context.give(&stack);
    }
    if let Some(effects) = EFFECTS.get() {
        let coords = [pos.x, pos.y, pos.z];
        let sample = (RandomState::new().hash_one(coords) >> 48) as u16;
        let pitch = 0.75 * (0.5 + 0.5 * f32::from(sample) / 65_536.0);
        for sound in [
            "minecraft:entity.item.pickup",
            "minecraft:block.netherite_block.hit",
        ] {
            effects.play_sound(
                world,
                coords.map(|n| f64::from(n) + 0.5),
                sound,
                SOUND_BLOCKS,
                0.25,
                pitch,
            );
        }
    }
    Some(UseResult::Success)
}

fn register_connected_block(
    content: Content,
    registry: &mut Registry<'_>,
    path: &str,
    name: &str,
    template: &str,
    map_color: Option<&str>,
) -> Result<(), RegistrationError> {
    let id = format!("create:{path}");
    let model = path.strip_suffix("_bars").unwrap_or(path);
    let post = format!("create:block/{model}_post");
    let variants: [(String, String); 32] = std::array::from_fn(|index| {
        let flag = |bit: usize| index & (1usize << bit) != 0;
        let properties = format!(
            "east={},north={},south={},waterlogged={},west={}",
            flag(1),
            flag(0),
            flag(2),
            flag(4),
            flag(3)
        );
        let physical = format!("{template}[{properties}]");
        (properties, physical)
    });
    let states = variants.each_ref().map(|(properties, physical)| {
        let mut variant = state(properties, physical, &post, 0, 0);
        // Use three spare cutout carriers for the framed and tiled families.
        if is_display_pane(path) || uses_bar_display(path) {
            variant.model = "";
            variant.rotating_model = &post;
        }
        variant
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
    if map_color.is_some() {
        content.set_block_material(
            registry,
            &id,
            &BlockMaterial {
                template,
                hardness: None,
                map_color,
            },
        )?;
    }
    Ok(())
}

fn register_stairs(
    content: Content,
    registry: &mut Registry<'_>,
    base: &str,
    name: &str,
    material: &BlockMaterial<'_>,
) -> Result<(), RegistrationError> {
    let path = format!("{base}_stairs");
    let id = format!("create:{path}");
    let mut variants = Vec::with_capacity(80);
    for (facing, y) in [("north", 270i16), ("east", 0), ("south", 90), ("west", 180)] {
        for (half, x) in [("bottom", 0), ("top", 180)] {
            for (shape, suffix, left) in [
                ("straight", "", false),
                ("inner_left", "_inner", true),
                ("inner_right", "_inner", false),
                ("outer_left", "_outer", true),
                ("outer_right", "_outer", false),
            ] {
                let y = (y - if left { 90 } else { 0 }
                    + if x == 180 && !suffix.is_empty() {
                        90
                    } else {
                        0
                    })
                .rem_euclid(360);
                for wet in [false, true] {
                    let properties =
                        format!("facing={facing},half={half},shape={shape},waterlogged={wet}");
                    let physical = format!("minecraft:stone_stairs[{properties}]");
                    let model = format!("create:block/{path}{suffix}");
                    variants.push((properties, physical, model, x, y));
                }
            }
        }
    }
    let states: Vec<_> = variants
        .iter()
        .map(|(properties, physical, model, x, y)| state(properties, physical, model, *x, *y))
        .collect();
    content.add_block(
        registry,
        &Block {
            id: &id,
            display_name: &format!("{name} Stairs"),
            entity: "",
            item_model: &format!("create:item/{path}"),
            states: &states,
        },
        None,
    )?;
    content.set_block_material(registry, &id, material)
}

fn register_wall(
    content: Content,
    registry: &mut Registry<'_>,
    base: &str,
    name: &str,
    material: &BlockMaterial<'_>,
) -> Result<(), RegistrationError> {
    const SIDES: [&str; 3] = ["none", "low", "tall"];
    let path = format!("{base}_wall");
    let id = format!("create:{path}");
    // The registry singularizes brick names, while models use the full palette pattern.
    let model = if base.ends_with("_brick") {
        format!("create:block/{base}s_wall_post")
    } else {
        format!("create:block/{path}_post")
    };
    let mut variants = Vec::with_capacity(324);
    for up in [true, false] {
        for wet in [false, true] {
            for north in SIDES {
                for east in SIDES {
                    for south in SIDES {
                        for west in SIDES {
                            let props = format!(
                                "east={east},north={north},south={south},up={up},waterlogged={wet},west={west}"
                            );
                            let physical = format!("minecraft:cobblestone_wall[{props}]");
                            variants.push((props, physical));
                        }
                    }
                }
            }
        }
    }
    let states: Vec<_> = variants
        .iter()
        .map(|(props, physical)| state(props, physical, &model, 0, 0))
        .collect();
    content.add_block(
        registry,
        &Block {
            id: &id,
            display_name: &format!("{name} Wall"),
            entity: "",
            item_model: &format!("create:item/{path}"),
            states: &states,
        },
        None,
    )?;
    content.set_block_material(registry, &id, material)
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
