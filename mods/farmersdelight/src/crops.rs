use std::{collections::HashMap, sync::OnceLock};

use moddedmc_api::{
    InterfaceUnavailable, Pos, RegistrationError, Registry,
    content::{Block, BlockUpdate, Content, ContentEvent, State, World, WorldExtension},
    effects::{self, Effects},
    fluid_world::{self, FluidWorld},
    interaction::{
        self, BlockUseResult, Interaction, InteractionExtension, UseBlock, UseOnBlock, UseResult,
    },
    stacks::{self, ItemStacks},
    tags::{self, Tags},
};
use rand::Rng;
use serde::Deserialize;

static INTERACTION: OnceLock<Interaction> = OnceLock::new();
static FLUIDS: OnceLock<FluidWorld> = OnceLock::new();
static TAGS: OnceLock<Tags> = OnceLock::new();
static STACKS: OnceLock<ItemStacks> = OnceLock::new();
static EFFECTS: OnceLock<Effects> = OnceLock::new();
static STATES: OnceLock<States> = OnceLock::new();

const ONION: usize = 0;
const CABBAGE: usize = 1;
const RICE: usize = 2;
const PANICLES: usize = 3;
const BUDDING: usize = 4;
const TOMATO: usize = 5;
const HANGING: usize = 6;
const MAX_AGE: [u8; 7] = [7, 7, 3, 3, 3, 3, 3];

const NAMES: [&str; 7] = [
    "farmersdelight:onions",
    "farmersdelight:cabbages",
    "farmersdelight:rice",
    "farmersdelight:rice_panicles",
    "farmersdelight:budding_tomatoes",
    "farmersdelight:tomatoes",
    "farmersdelight:tomatoes_on_rope",
];

const MODELS: [&str; 7] = [
    "farmersdelight:item/onion",
    "farmersdelight:item/cabbage_seeds",
    "farmersdelight:item/rice",
    "farmersdelight:item/rice_panicle",
    "farmersdelight:item/tomato_seeds",
    "farmersdelight:item/tomato_seeds",
    "farmersdelight:item/tomato_seeds",
];

const VARIANTS: [&str; 7] = [
    include_str!("../resources/assets/farmersdelight/blockstates/onions.json"),
    include_str!("../resources/assets/farmersdelight/blockstates/cabbages.json"),
    include_str!("../resources/assets/farmersdelight/blockstates/rice.json"),
    include_str!("../resources/assets/farmersdelight/blockstates/rice_panicles.json"),
    include_str!("../resources/assets/farmersdelight/blockstates/budding_tomatoes.json"),
    include_str!("../resources/assets/farmersdelight/blockstates/tomatoes.json"),
    include_str!("../resources/assets/farmersdelight/blockstates/tomatoes_on_rope.json"),
];

#[derive(Deserialize)]
struct Variant {
    model: String,
}

#[derive(Deserialize)]
struct BlockStates {
    variants: std::collections::BTreeMap<String, Variant>,
}

#[derive(Clone, Copy)]
struct Crop {
    kind: usize,
    age: u8,
    alternate: bool,
}

struct States {
    crops: HashMap<u32, Crop>,
    by_age: [[u32; 8]; NAMES.len()],
    rice_supporting: [u32; 4],
    farmland: [Option<u32>; 8],
    air: u32,
}

fn states() -> Option<&'static States> {
    let content = *super::CONTENT.get()?;
    Some(STATES.get_or_init(|| {
        let mut crops = HashMap::new();
        let mut by_age = [[moddedmc_api::abi::UNLOADED_STATE; 8]; NAMES.len()];
        let mut rice_supporting = [moddedmc_api::abi::UNLOADED_STATE; 4];
        for (kind, block) in NAMES.iter().enumerate() {
            let max = if kind == BUDDING { 4 } else { MAX_AGE[kind] };
            for age in 0..=max {
                for &alternate in if matches!(kind, RICE | TOMATO) {
                    &[false, true][..]
                } else {
                    &[false][..]
                } {
                    let suffix = match kind {
                        RICE => format!("age={age},supporting={alternate}"),
                        TOMATO => format!("age={age},ropelogged={alternate}"),
                        _ => format!("age={age}"),
                    };
                    if let Some(state) = content.state(&format!("{block}[{suffix}]")) {
                        crops.insert(
                            state,
                            Crop {
                                kind,
                                age,
                                alternate,
                            },
                        );
                        if alternate && kind == RICE {
                            rice_supporting[usize::from(age)] = state;
                        } else if !alternate {
                            by_age[kind][usize::from(age)] = state;
                        }
                    }
                }
            }
        }
        States {
            crops,
            by_age,
            rice_supporting,
            farmland: std::array::from_fn(|i| {
                content.state(&format!("minecraft:farmland[moisture={i}]"))
            }),
            air: content.state("minecraft:air").unwrap_or_default(),
        }
    }))
}

pub fn start(content: Content) -> Result<(), InterfaceUnavailable> {
    content.require_random_tick()?;
    content.require_extension(WorldExtension::RawBrightness)?;
    content.require_extension(WorldExtension::DescribeState)?;
    let interaction = interaction::require()?;
    interaction.require_extension(InteractionExtension::VanillaItemUse)?;
    interaction.require_extension(InteractionExtension::BlockUse)?;
    let _ = INTERACTION.set(interaction);
    let _ = FLUIDS.set(fluid_world::require()?);
    let _ = TAGS.set(tags::require()?);
    let _ = STACKS.set(stacks::require()?);
    let _ = EFFECTS.set(effects::require()?);
    Ok(())
}

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    for (kind, name) in [
        "Onions",
        "Cabbages",
        "Rice",
        "Rice Panicles",
        "Budding Tomatoes",
        "Tomatoes",
        "Tomatoes on Rope",
    ]
    .into_iter()
    .enumerate()
    {
        let data: BlockStates = serde_json::from_str(VARIANTS[kind])
            .map_err(|_| RegistrationError::InvalidDefinition)?;
        let variants: Vec<_> = data
            .variants
            .iter()
            .map(|(properties, variant)| State {
                properties,
                physical: if kind == RICE {
                    "minecraft:small_dripleaf[waterlogged=true]"
                } else {
                    "minecraft:wheat"
                },
                model: &variant.model,
                rotating_model: "",
                x: 0,
                y: 0,
                orientation: [0., 0., 0., 1.],
                shape: &[],
            })
            .collect();
        content.add_block(
            registry,
            &Block {
                id: NAMES[kind],
                display_name: name,
                entity: "",
                item_model: MODELS[kind],
                states: &variants,
            },
            None,
        )?;
    }
    content.on_random_tick(registry, &NAMES, random_tick)?;
    content.on_tick(registry, check_support)?;
    let interaction = INTERACTION
        .get()
        .ok_or(RegistrationError::InvalidDefinition)?;
    interaction.on_use_on_block(registry, plant)?;
    interaction.on_use_block(registry, harvest)?;
    interaction.on_vanilla_use_on_block(registry, bonemeal)
}

fn at(world: World<'_>, position: Pos) -> Option<u32> {
    let mut out = [moddedmc_api::abi::UNLOADED_STATE];
    (world.read(&[position], &mut out) && out[0] != moddedmc_api::abi::UNLOADED_STATE)
        .then_some(out[0])
}

fn offset(pos: Pos, x: i32, y: i32, z: i32) -> Pos {
    Pos {
        x: pos.x + x,
        y: pos.y + y,
        z: pos.z + z,
    }
}

fn update(position: Pos, state: u32) -> BlockUpdate {
    BlockUpdate {
        position,
        state,
        flags: moddedmc_api::abi::UPDATE_ALL,
    }
}

fn farmland(ids: &States, state: u32) -> bool {
    ids.farmland.contains(&Some(state))
}

fn supports(world: World<'_>, pos: Pos, kind: usize, ids: &States) -> bool {
    let Some(below) = at(world, offset(pos, 0, -1, 0)) else {
        return false;
    };
    if kind == PANICLES {
        return ids.crops.get(&below).is_some_and(|crop| crop.kind == RICE);
    }
    if matches!(kind, TOMATO | HANGING)
        && ids
            .crops
            .get(&below)
            .is_some_and(|crop| matches!(crop.kind, TOMATO | HANGING))
    {
        return true;
    }
    if farmland(ids, below) {
        return true;
    }
    kind == RICE
        && super::CONTENT
            .get()
            .and_then(|content| content.describe_state(below))
            .zip(TAGS.get())
            .is_some_and(|(name, tags)| {
                tags.contains(
                    "block",
                    "minecraft:dirt",
                    name.split('[').next().unwrap_or(&name),
                )
            })
}

fn plant(args: &mut UseOnBlock<'_>) -> UseResult {
    let kind = match args.item {
        "farmersdelight:onion" => ONION,
        "farmersdelight:cabbage_seeds" => CABBAGE,
        "farmersdelight:rice" => RICE,
        "farmersdelight:tomato_seeds" => BUDDING,
        _ => return UseResult::Pass,
    };
    let Some(ids) = states() else {
        return UseResult::Fail;
    };
    let pos = offset(args.position, 0, 1, 0);
    let valid_target = if kind == RICE {
        FLUIDS
            .get()
            .and_then(|fluids| fluids.inspect(args.world, pos))
            .is_some_and(|fluid| {
                fluid.liquid_block
                    && fluid.source
                    && fluid.fluid.as_deref() == Some("minecraft:water")
            })
    } else {
        at(args.world, pos) == Some(ids.air)
    };
    if args.face != 1
        || !valid_target
        || !supports(args.world, pos, kind, ids)
        || (kind != RICE
            && !args
                .world
                .raw_brightness(pos, 0)
                .is_some_and(|light| light >= 8))
    {
        return if kind == ONION {
            UseResult::Pass
        } else {
            UseResult::Fail
        };
    }
    if args.world.write(&[update(pos, ids.by_age[kind][0])]) != 1 {
        return UseResult::Fail;
    }
    if !args.creative {
        args.stack.shrink(1);
    }
    UseResult::Success
}

fn growth_denominator(world: World<'_>, pos: Pos, kind: usize, ids: &States) -> u32 {
    let soil =
        std::array::from_fn::<_, 9, _>(|i| offset(pos, i as i32 / 3 - 1, -1, i as i32 % 3 - 1));
    let neighbors =
        std::array::from_fn::<_, 9, _>(|i| offset(pos, i as i32 / 3 - 1, 0, i as i32 % 3 - 1));
    let mut soil_ids = [moddedmc_api::abi::UNLOADED_STATE; 9];
    let mut neighbor_ids = soil_ids;
    world.read(&soil, &mut soil_ids);
    world.read(&neighbors, &mut neighbor_ids);
    let mut speed = 1.0_f32;
    for (i, state) in soil_ids.into_iter().enumerate() {
        if let Some(moisture) = ids
            .farmland
            .iter()
            .position(|candidate| *candidate == Some(state))
        {
            let weight = if moisture == 0 { 1.0 } else { 3.0 };
            speed += if i == 4 { weight } else { weight * 0.25 };
        }
    }
    let same = |i: usize| {
        ids.crops
            .get(&neighbor_ids[i])
            .is_some_and(|crop| crop.kind == kind)
    };
    if ((same(1) || same(7)) && (same(3) || same(5))) || [0, 2, 6, 8].into_iter().any(same) {
        speed *= 0.5;
    }
    (25.0 / speed) as u32 + 1
}

fn random_tick(world: World<'_>, pos: Pos, state: u32) {
    let Some(ids) = states() else { return };
    let Some(&crop) = ids.crops.get(&state) else {
        return;
    };
    if crop.kind == TOMATO && crop.alternate {
        grow(world, pos, crop, 0, ids);
        return;
    }
    let (light_pos, min_light) = match crop.kind {
        RICE => (offset(pos, 0, 1, 0), 6),
        _ => (pos, 9),
    };
    if (crop.age >= MAX_AGE[crop.kind]
        && !matches!(crop.kind, BUDDING | TOMATO | HANGING)
        && (crop.kind != RICE || crop.alternate))
        || !world
            .raw_brightness(light_pos, 0)
            .is_some_and(|light| light >= min_light)
    {
        return;
    }
    let chance = match crop.kind {
        RICE => 3,
        TOMATO | HANGING => 6,
        _ => growth_denominator(world, pos, crop.kind, ids),
    };
    if rand::rng().random_range(0..chance) == 0 {
        grow(world, pos, crop, 1, ids);
    }
}

fn grow(world: World<'_>, pos: Pos, crop: Crop, amount: u8, ids: &States) -> bool {
    let age = crop.age + amount;
    if crop.kind == TOMATO && crop.alternate {
        return world.write(&[update(pos, ids.by_age[HANGING][usize::from(crop.age)])]) == 1;
    }
    if crop.kind == BUDDING && age > 3 {
        return world.write(&[update(
            pos,
            ids.by_age[TOMATO][usize::from((age - 4).min(3))],
        )]) == 1;
    }
    if crop.kind == RICE && age > 3 {
        let upper = offset(pos, 0, 1, 0);
        match at(world, upper) {
            Some(state)
                if state == ids.air
                    && world
                        .raw_brightness(upper, 0)
                        .is_some_and(|light| light >= 8) =>
            {
                return world.write(&[
                    update(pos, ids.rice_supporting[3]),
                    update(upper, ids.by_age[PANICLES][usize::from((age - 4).min(3))]),
                ]) == 2;
            }
            Some(state)
                if ids
                    .crops
                    .get(&state)
                    .is_some_and(|top| top.kind == PANICLES && top.age < 3) =>
            {
                if let Some(&top) = ids.crops.get(&state) {
                    return grow(world, upper, top, rand::rng().random_range(2..=5) / 3, ids);
                }
            }
            _ => {}
        }
        return false;
    }
    let max = MAX_AGE[crop.kind];
    if crop.age >= max {
        return false;
    }
    amount == 0
        || world.write(&[update(
            pos,
            ids.by_age[crop.kind][usize::from(age.min(max))],
        )]) == 1
}

fn bonemeal(args: &mut UseOnBlock<'_>) -> UseResult {
    if args.item != "minecraft:bone_meal" {
        return UseResult::Pass;
    }
    let Some(ids) = states() else {
        return UseResult::Pass;
    };
    let Some(&crop) = at(args.world, args.position).and_then(|state| ids.crops.get(&state)) else {
        return UseResult::Pass;
    };
    if crop.kind == RICE
        && at(args.world, offset(args.position, 0, 1, 0))
            .and_then(|state| ids.crops.get(&state))
            .is_some_and(|top| top.kind == PANICLES && top.age == MAX_AGE[PANICLES])
    {
        return UseResult::Pass;
    }
    let amount = match crop.kind {
        RICE | BUDDING => rand::rng().random_range(1..=4),
        PANICLES => rand::rng().random_range(2..=5) / 3,
        TOMATO | HANGING => rand::rng().random_range(2..=5) / 2,
        _ => rand::rng().random_range(2..=5),
    };
    if !grow(args.world, args.position, crop, amount, ids) && crop.kind != RICE {
        return UseResult::Pass;
    }
    if !args.creative {
        args.stack.shrink(1);
    }
    UseResult::Success
}

fn harvest(args: &mut UseBlock<'_>) -> BlockUseResult {
    let (Some(ids), Some(stacks)) = (states(), STACKS.get()) else {
        return BlockUseResult::Pass;
    };
    let Some(&crop) = at(args.world, args.position).and_then(|state| ids.crops.get(&state)) else {
        return BlockUseResult::Pass;
    };
    if !matches!(crop.kind, TOMATO | HANGING) || crop.age != 3 {
        return BlockUseResult::Pass;
    }
    if args.with_item {
        return BlockUseResult::TryEmptyHand;
    }
    let mut rng = rand::rng();
    let count = rng.random_range(1..=2);
    let position = [
        f64::from(args.position.x) + 0.5,
        f64::from(args.position.y) + 0.5,
        f64::from(args.position.z) + 0.5,
    ];
    let drop = |item: &str, count: i32| {
        let data = fastnbt::to_bytes(&serde_json::json!({ "id": item, "count": count })).ok()?;
        stacks.spawn(args.world, position, [0., 0.1, 0.], &data)
    };
    if args
        .world
        .write(&[update(args.position, ids.by_age[crop.kind][0])])
        != 1
    {
        return BlockUseResult::Fail;
    }
    drop("farmersdelight:tomato", count);
    if rng.random_bool(0.05) {
        drop("farmersdelight:rotten_tomato", 1);
    }
    if let Some(effects) = EFFECTS.get() {
        // The release's pick_tomatoes sound delegates to this vanilla event.
        effects.play_sound(
            args.world,
            position,
            "minecraft:block.sweet_berry_bush.pick_berries",
            effects::SOUND_BLOCKS,
            1.0,
            0.8 + rng.random::<f32>() * 0.4,
        );
    }
    BlockUseResult::Success
}

fn check_support(world: World<'_>, _: u64, events: &[ContentEvent]) {
    let Some(ids) = states() else { return };
    for event in events {
        if event.reason != moddedmc_api::abi::NEIGHBOR_CHANGED {
            continue;
        }
        let Some(previous) = ids.crops.get(&event.state) else {
            continue;
        };
        let Some(&crop) = at(world, event.position).and_then(|state| ids.crops.get(&state)) else {
            continue;
        };
        if previous.kind != crop.kind {
            continue;
        }
        let has_light = crop.kind == RICE
            || world
                .raw_brightness(event.position, 0)
                .is_some_and(|light| light >= 8);
        if !has_light || !supports(world, event.position, crop.kind, ids) {
            world.destroy(event.position, true);
        } else if crop.kind == RICE {
            let upper = offset(event.position, 0, 1, 0);
            let supporting = at(world, upper)
                .and_then(|state| ids.crops.get(&state))
                .is_some_and(|top| top.kind == PANICLES);
            if supporting != crop.alternate {
                let age = usize::from(crop.age);
                let state = if supporting {
                    ids.rice_supporting[age]
                } else {
                    ids.by_age[RICE][age]
                };
                world.write(&[update(event.position, state)]);
            }
            if let Some(fluids) = FLUIDS.get() {
                fluids.schedule_default_tick(world, event.position, "minecraft:water");
            }
        }
    }
}
