//! Create 6.0.10 fluid pipe block definitions. Runtime fluid handlers and
//! neighbor updates are provided after the generic fluid world ABI arrives.

use moddedmc_api::{
    RegistrationError, Registry,
    content::{Block, Content, ShapeBox, State},
};

const PIPE: &str = "create:fluid_pipe";
const ENCASED_PIPE: &str = "create:encased_fluid_pipe";
const GLASS_PIPE: &str = "create:glass_fluid_pipe";
const PUMP: &str = "create:mechanical_pump";

const IDENTITY: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const PIPE_CORE: ShapeBox = ShapeBox {
    min: [4, 4, 4],
    max: [12, 12, 12],
};
const PIPE_ARMS: [ShapeBox; 6] = [
    ShapeBox {
        min: [4, 0, 4],
        max: [12, 4, 12],
    },
    ShapeBox {
        min: [4, 12, 4],
        max: [12, 16, 12],
    },
    ShapeBox {
        min: [4, 4, 0],
        max: [12, 12, 4],
    },
    ShapeBox {
        min: [4, 4, 12],
        max: [12, 12, 16],
    },
    ShapeBox {
        min: [0, 4, 4],
        max: [4, 12, 12],
    },
    ShapeBox {
        min: [12, 4, 4],
        max: [16, 12, 12],
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipeState {
    pub openings: [bool; 6],
    pub wet: bool,
}

impl PipeState {
    fn from_mask(mask: u8, wet: bool) -> Self {
        let openings = std::array::from_fn(|i| (mask & (1 << i)) != 0);
        Self { openings, wet }
    }

    fn properties(self, waterlogged: bool) -> String {
        let [down, up, north, south, west, east] = self.openings;
        let connections =
            format!("down={down},up={up},north={north},south={south},west={west},east={east}");
        if waterlogged {
            format!("{connections},waterlogged={}", self.wet)
        } else {
            connections
        }
    }

    fn collision_shape(self) -> Vec<ShapeBox> {
        let mut boxes = vec![PIPE_CORE];
        for (open, arm) in self.openings.into_iter().zip(PIPE_ARMS) {
            if open {
                boxes.push(arm);
            }
        }
        boxes
    }
}

struct RegisteredState {
    properties: String,
    physical: String,
    model: &'static str,
    x: i16,
    y: i16,
    shape: Vec<ShapeBox>,
}

fn register_block(
    content: Content,
    registry: &mut Registry<'_>,
    id: &'static str,
    name: &'static str,
    model: &'static str,
    rows: &[RegisteredState],
) -> Result<(), RegistrationError> {
    let states: Vec<_> = rows
        .iter()
        .map(|row| State {
            properties: &row.properties,
            physical: &row.physical,
            model: row.model,
            rotating_model: "",
            x: row.x,
            y: row.y,
            orientation: IDENTITY,
            shape: &row.shape,
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id,
            display_name: name,
            entity: id,
            item_model: model,
            states: &states,
        },
        None,
    )
}

fn pipe_rows() -> Vec<RegisteredState> {
    (0..64u8)
        .flat_map(|mask| {
            [false, true].map(move |wet| {
                let state = PipeState::from_mask(mask, wet);
                RegisteredState {
                    properties: state.properties(true),
                    physical: format!("minecraft:iron_chain[axis=y,waterlogged={wet}]"),
                    // Model parts depend on connectivity and need composite rendering.
                    model: "create:block/fluid_pipe/core_y",
                    x: 0,
                    y: 0,
                    shape: state.collision_shape(),
                }
            })
        })
        .collect()
}

fn encased_rows() -> Vec<RegisteredState> {
    (0..64u8)
        .map(|mask| RegisteredState {
            properties: PipeState::from_mask(mask, false).properties(false),
            physical: "minecraft:copper_block".into(),
            model: "create:block/fluid_pipe/casing",
            x: 0,
            y: 0,
            shape: Vec::new(),
        })
        .collect()
}

fn glass_rows() -> Vec<RegisteredState> {
    [
        ("y", [4, 0, 4], [12, 16, 12], 0, 0),
        ("x", [0, 4, 4], [16, 12, 12], 90, 90),
        ("z", [4, 4, 0], [12, 12, 16], 90, 0),
    ]
    .into_iter()
    .flat_map(|(axis, min, max, x, y)| {
        [false, true].into_iter().flat_map(move |alt| {
            [false, true].map(move |wet| RegisteredState {
                properties: format!("axis={axis},alt={alt},waterlogged={wet}"),
                physical: format!("minecraft:iron_chain[axis={axis},waterlogged={wet}]"),
                model: "create:block/fluid_pipe/window",
                x,
                y,
                shape: vec![ShapeBox { min, max }],
            })
        })
    })
    .collect()
}

fn pump_rows() -> Vec<RegisteredState> {
    [
        ("north", 0, 180),
        ("south", 0, 0),
        ("west", 0, 90),
        ("east", 0, 270),
        ("up", 270, 0),
        ("down", 90, 0),
    ]
    .into_iter()
    .flat_map(|(face, x, y)| {
        [false, true].map(move |wet| RegisteredState {
            properties: format!("facing={face},waterlogged={wet}"),
            physical: format!("minecraft:iron_chain[axis=y,waterlogged={wet}]"),
            model: "create:block/mechanical_pump/block",
            x,
            y,
            // Accurate pump collision shapes require AllShapes.PUMP's geometry.
            shape: vec![ShapeBox {
                min: [0; 3],
                max: [16; 3],
            }],
        })
    })
    .collect()
}

/// Register fluid content so its complete block-state IDs can be resolved.
/// Connections, persistence and fluid motion are still gated on the host ABI.
pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    register_block(
        content,
        registry,
        PIPE,
        "Fluid Pipe",
        "create:item/fluid_pipe",
        &pipe_rows(),
    )?;
    register_block(
        content,
        registry,
        ENCASED_PIPE,
        "Encased Fluid Pipe",
        "create:item/fluid_pipe",
        &encased_rows(),
    )?;
    register_block(
        content,
        registry,
        GLASS_PIPE,
        "Glass Fluid Pipe",
        "create:item/fluid_pipe",
        &glass_rows(),
    )?;
    register_block(
        content,
        registry,
        PUMP,
        "Mechanical Pump",
        "create:item/mechanical_pump",
        &pump_rows(),
    )
}

#[cfg(test)]
mod tests {
    use super::{PipeState, encased_rows, glass_rows, pipe_rows, pump_rows};

    #[test]
    fn independent_pipe_faces_and_waterlogged_states_have_unique_selectors() {
        let states = pipe_rows();
        let selectors: std::collections::HashSet<_> = states
            .iter()
            .map(|state| state.properties.as_str())
            .collect();
        assert_eq!(states.len(), 128);
        assert_eq!(selectors.len(), 128);
        assert_eq!(
            states[0].properties,
            "down=false,up=false,north=false,south=false,west=false,east=false,waterlogged=false"
        );
        let east_only = PipeState::from_mask(1 << 5, true);
        assert_eq!(east_only.collision_shape().len(), 2);
        assert!(east_only.properties(true).contains("east=true"));
        assert!(east_only.properties(true).contains("waterlogged=true"));
    }

    #[test]
    fn pipe_collision_boxes_cover_only_open_faces() {
        let mut state = PipeState::from_mask(0, false);
        assert_eq!(state.collision_shape()[0].min, [4, 4, 4]);
        assert_eq!(state.collision_shape()[0].max, [12, 12, 12]);
        state.openings[2] = true;
        state.openings[5] = true;
        let shape = state.collision_shape();
        assert_eq!(shape.len(), 3);
        assert!(
            shape
                .iter()
                .any(|box_| box_.min == [4, 4, 0] && box_.max == [12, 12, 4])
        );
        assert!(
            shape
                .iter()
                .any(|box_| box_.min == [12, 4, 4] && box_.max == [16, 12, 12])
        );
    }

    #[test]
    fn glass_axis_and_alt_variants_are_independent_of_waterlogged() {
        let glass = glass_rows();
        assert_eq!(glass.len(), 12);
        assert!(
            glass
                .iter()
                .any(|state| state.properties == "axis=x,alt=true,waterlogged=true")
        );
        assert!(
            glass
                .iter()
                .any(|state| state.properties == "axis=z,alt=false,waterlogged=false")
        );
        assert_eq!(encased_rows().len(), 64);
        assert_eq!(pump_rows().len(), 12);
    }
}
