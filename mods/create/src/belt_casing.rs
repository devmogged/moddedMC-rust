//! Casing changes a belt's outline and appearance without changing its kinetic kind.

use moddedmc_api::{
    Pos,
    content::{self, BlockUpdate, ShapeBox, World},
    effects::SOUND_BLOCKS,
    interaction::{BlockUseResult, UseBlock},
    stacks::ItemDisplay,
};

use crate::{
    BRASS_CASING, CASING, EFFECTS, StateIds, WRENCH, belt,
    display_model::ModelDisplay,
    kinetics::{Be, Belt, Kind, Kinetics, Part, Slope, face_axis, offset},
    processing_recipes::item_id,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Casing {
    #[default]
    None,
    Andesite,
    Brass,
}

impl Casing {
    pub(crate) fn load(name: Option<&str>) -> Self {
        match name {
            Some("ANDESITE") => Self::Andesite,
            Some("BRASS") => Self::Brass,
            _ => Self::None,
        }
    }

    pub(crate) fn saved(self) -> &'static str {
        match self {
            Self::Andesite => "ANDESITE",
            Self::Brass => "BRASS",
            Self::None => "NONE",
        }
    }

    fn material(self) -> Option<&'static str> {
        match self {
            Self::Andesite => Some("andesite"),
            Self::Brass => Some("brass"),
            Self::None => None,
        }
    }
}

pub(crate) fn shape(belt: Belt) -> Option<ShapeBox> {
    const FULL: ShapeBox = ShapeBox {
        min: [0, 0, 0],
        max: [16, 11, 16],
    };
    if matches!(belt.slope, Slope::Vertical | Slope::Sideways) {
        return None;
    }
    if belt.slope == Slope::Horizontal
        || (belt.part == Part::Start && belt.slope == Slope::Upward)
        || (belt.part == Part::End && belt.slope == Slope::Downward)
    {
        return Some(FULL);
    }
    let facing = if belt.slope == Slope::Upward {
        belt.facing
    } else {
        belt.facing ^ 1
    };
    let mut shape = FULL;
    let axis = usize::from(face_axis(facing));
    if facing == 3 || facing == 5 {
        shape.min[axis] = 5;
    } else {
        shape.max[axis] = 11;
    }
    Some(shape)
}

pub(crate) fn apply(
    context: &UseBlock<'_>,
    kinetics: &mut Kinetics,
    states: &StateIds,
    belt: Belt,
) -> Option<BlockUseResult> {
    if !context.with_item || context.sneaking || !context.player.is_some_and(|p| p.may_build()) {
        return None;
    }
    let item = fastnbt::from_bytes(&context.stack.read()?).ok()?;
    let pos = context.position;
    let be = kinetics.get_mut(pos)?;
    let (casing, sound) = match item_id(&item)? {
        CASING => (Casing::Andesite, "minecraft:block.stone.place"),
        BRASS_CASING => (Casing::Brass, "minecraft:block.metal.place"),
        WRENCH if be.belt_casing != Casing::None => (Casing::None, "minecraft:block.stone.break"),
        _ => return None,
    };
    if be.belt_casing == casing {
        return Some(BlockUseResult::Success);
    }
    let properties =
        belt::casing_properties(belt, states.wet(context.state), casing != Casing::None);
    let state = states.lookup(&format!("{}[{properties}]", belt::BELT))?;
    if state != context.state
        && context.world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: content::UPDATE_ALL,
        }]) != 1
    {
        return Some(BlockUseResult::Fail);
    }
    be.belt_casing = casing;
    be.dirty = true;
    if let Some(effects) = EFFECTS.get() {
        effects.play_sound(context.world, center(pos), sound, SOUND_BLOCKS, 1.0, 0.8);
    }
    Some(BlockUseResult::Success)
}

fn center(pos: Pos) -> [f64; 3] {
    [pos.x, pos.y, pos.z].map(|coordinate| f64::from(coordinate) + 0.5)
}

#[derive(Default)]
pub(crate) struct CasingDisplay {
    body: ModelDisplay,
    cover: ModelDisplay,
}

impl CasingDisplay {
    pub(crate) fn clear(&mut self, world: World<'_>) {
        self.body.clear(world);
        self.cover.clear(world);
    }
}

pub(crate) fn display(
    world: World<'_>,
    pos: Pos,
    be: &Be,
    kinetics: &mut Kinetics,
    states: &StateIds,
) -> bool {
    let Kind::Belt(belt) = be.kind else {
        return false;
    };
    let Some(material) = be.belt_casing.material() else {
        if let Some(mut visuals) = kinetics.belt_casing_displays.remove(&pos) {
            visuals.clear(world);
        }
        return false;
    };
    let vertical = belt.slope == Slope::Vertical;
    let sideways = belt.slope == Slope::Sideways;
    let negative = matches!(belt.facing, 2 | 4);
    let reverse = belt.slope == Slope::Downward || (vertical || sideways) && negative;
    let part = match (belt.part, reverse) {
        (Part::Start, true) => Part::End,
        (Part::End, true) => Part::Start,
        (part, _) => part,
    };
    let slope = match belt.slope {
        Slope::Horizontal => "horizontal",
        Slope::Vertical | Slope::Sideways => "sideways",
        _ => "diagonal",
    };
    let x = if vertical {
        -90.0
    } else if sideways && negative {
        -180.0
    } else {
        0.0
    };
    let mut y = belt::horizontal_angle(belt.facing);
    if belt.slope == Slope::Upward {
        y += 180;
    }
    if vertical {
        y -= 90;
    }
    let (sx, cx) = (f32::to_radians(x) * 0.5).sin_cos();
    let (sy, cy) = (f32::from(y).to_radians() * 0.5).sin_cos();
    let model = format!(
        "create:moddedmc/parts/belt_casing/{material}/{slope}_{}",
        part.name()
    );
    let shown = ItemDisplay {
        position: center(pos),
        rotation: [cy * sx, sy * cx, -sy * sx, cy * cx],
        scale: 1.0,
        context: 0,
        interpolation: 0,
    };
    let visuals = kinetics.belt_casing_displays.entry(pos).or_default();
    visuals.body.show(world, &model, &shown);
    let covered = belt.slope == Slope::Horizontal && covered(world, pos, states);
    if covered {
        let axis = if face_axis(belt.facing) == 0 {
            "x"
        } else {
            "z"
        };
        let model = format!("create:moddedmc/parts/belt_casing/{material}/cover_{axis}");
        visuals.cover.show(
            world,
            &model,
            &ItemDisplay {
                rotation: [0.0, 0.0, 0.0, 1.0],
                ..shown
            },
        );
    } else {
        visuals.cover.clear(world);
    }
    covered
}

fn covered(world: World<'_>, pos: Pos, states: &StateIds) -> bool {
    let above = offset(pos, [0, 1, 0]);
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[above], &mut state)
        || states.crusher_controllers.contains_key(&state[0])
        || states
            .funnels
            .get(&state[0])
            .is_some_and(|state| !state.covers_belt())
    {
        return false;
    }
    let Some(boxes) = world.collision_shape(above) else {
        return false;
    };
    let Some(first) = boxes.first() else {
        return false;
    };
    let (min, max) = boxes
        .iter()
        .fold((first.min, first.max), |(mut min, mut max), shape| {
            for axis in [0, 1, 2] {
                min[axis] = min[axis].min(shape.min[axis]);
                max[axis] = max[axis].max(shape.max[axis]);
            }
            (min, max)
        });
    min[1] <= 0.0 && max[0] - min[0] >= 0.5 && max[2] - min[2] >= 0.5
}
