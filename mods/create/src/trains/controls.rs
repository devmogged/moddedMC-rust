//! Create's horizontal, waterloggable Train Controls.
use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, Content, Placement, ShapeBox, State},
};

use crate::DIRECTIONS;

pub(crate) const ID: &str = "create:controls";

fn properties(facing: &str, open: bool, virtual_: bool, wet: bool) -> String {
    format!("facing={facing},open={open},virtual={virtual_},waterlogged={wet}")
}

pub(super) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let directions = [
        ("north", 0, [0, 0, 6], [16, 16, 16]),
        ("east", 90, [0, 0, 0], [10, 16, 16]),
        ("south", 180, [0, 0, 0], [16, 16, 10]),
        ("west", 270, [6, 0, 0], [16, 16, 16]),
    ];
    let mut rows = Vec::new();
    for (facing, y, min, max) in directions {
        for open in [false, true] {
            for virtual_ in [false, true] {
                for wet in [false, true] {
                    let model = match (open, virtual_) {
                        (_, true) => "create:block/controls/block_virtual",
                        (true, false) => "create:block/controls/block_open",
                        (false, false) => "create:block/controls/block_closed",
                    };
                    rows.push((
                        properties(facing, open, virtual_, wet),
                        y,
                        [ShapeBox { min, max }],
                        model,
                        wet,
                    ));
                }
            }
        }
    }
    let states: Vec<_> = rows
        .iter()
        .map(|(properties, y, shape, model, wet)| State {
            properties,
            physical: if *wet {
                "minecraft:cut_copper_slab[type=bottom,waterlogged=true]"
            } else {
                "minecraft:cut_copper_slab[type=bottom,waterlogged=false]"
            },
            model,
            rotating_model: "",
            x: 0,
            y: *y,
            orientation: [0.0, 0.0, 0.0, 1.0],
            shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Train Controls",
            item_model: "create:item/controls",
            entity: ID,
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn placed_state(args: &Placement<'_>, content: Content) -> Option<u32> {
    if content.state(ID)? != args.state {
        return None;
    }
    let face = [3, 4, 2, 5][((args.yaw / 90.0 + 0.5).floor() as i32 & 3) as usize]
        ^ usize::from(!args.sneak);
    let wet = args.world.fluid_id(args.position).as_deref() == Some("minecraft:water");
    let properties = properties(DIRECTIONS[face], false, false, wet);
    content.state(&format!("{ID}[{properties}]"))
}
