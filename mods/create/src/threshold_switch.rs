//! Create's Threshold Switch (stockpile_switch), with stock-client controls.

use std::collections::HashMap;

use fastnbt::Value;
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    containers::NO_FACE,
    content::{self, Block, BlockUpdate, Content, ContentEvent, Placement, State, World},
    interaction::{BlockUseResult, RedstoneQuery, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    CONTAINERS, DIRECTIONS, STACKS, StateIds, WRENCH,
    belt_inventory::{self, TransportedItemStack},
    belt_tunnel::{read, shift},
    directed_detector::{self, Mount},
    filtering::Filter,
    kinetics::Kinetics,
    processing_recipes::item_id,
};

pub(crate) const ID: &str = "create:stockpile_switch";
const SAMPLE_INTERVAL: u64 = 10;
const POWER_DELAY: u64 = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct SwitchState {
    mount: Mount,
    level: u8,
    powered: bool,
}

impl SwitchState {
    fn all() -> impl Iterator<Item = Self> {
        Mount::all().flat_map(|mount| {
            (0..=5).flat_map(move |level| {
                [false, true].map(|powered| Self {
                    mount,
                    level,
                    powered,
                })
            })
        })
    }

    fn properties(self) -> String {
        format!(
            "facing={},level={},powered={},target={}",
            DIRECTIONS[usize::from(self.mount.facing)],
            self.level,
            self.powered,
            self.mount.name()
        )
    }

    fn id(self, states: &StateIds) -> Option<u32> {
        states.lookup(&format!("{ID}[{}]", self.properties()))
    }
}

pub(crate) fn register(
    content: Content,
    registry: &mut Registry<'_>,
) -> Result<(), RegistrationError> {
    let rows: Vec<_> = SwitchState::all()
        .map(|state| {
            (
                state,
                state.properties(),
                format!(
                    "create:block/threshold_switch/block_{}_{}",
                    state.mount.name(),
                    state.level
                ),
            )
        })
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(state, properties, model)| State {
            properties,
            physical: "minecraft:glass",
            model: "",
            rotating_model: model,
            x: 0,
            y: 0,
            orientation: directed_detector::orientation(0, state.mount.yaw()),
            shape: &[],
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Threshold Switch",
            entity: ID,
            item_model: "create:item/stockpile_switch",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for state in SwitchState::all() {
        states.add(content, ID, &state.properties(), None);
        if let Some(id) = state.id(states) {
            states.switches.insert(id, state);
        }
    }
}

pub(crate) fn place(args: &Placement<'_>, states: &StateIds) -> Option<u32> {
    SwitchState {
        mount: Mount::preferred(args, |id| supports_stock(states, id)),
        level: 0,
        powered: false,
    }
    .id(states)
}

pub(crate) fn connects(id: u32, direction: u8) -> bool {
    crate::states()
        .and_then(|states| states.switches.get(&id))
        .is_some_and(|state| direction < 6 && state.mount.target != direction ^ 1)
}

pub(crate) fn signal(query: &RedstoneQuery<'_>) -> u8 {
    if query.direct || !connects(query.state, query.direction) {
        return 0;
    }
    crate::states()
        .and_then(|states| states.switches.get(&query.state))
        .map_or(0, |state| if state.powered { 15 } else { 0 })
}

#[derive(Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Data {
    on_above_amount: i32,
    off_below_amount: i32,
    current_amount: i32,
    current_max_amount: i32,
    powered: bool,
    inverted: bool,
    #[serde(flatten)]
    filter: Filter,
}

impl Default for Data {
    fn default() -> Self {
        Self {
            on_above_amount: 128,
            off_below_amount: 64,
            current_amount: -1,
            current_max_amount: -1,
            powered: false,
            inverted: false,
            filter: Filter::default(),
        }
    }
}

pub(crate) struct Switch {
    data: Data,
    scan_at: u64,
    output_at: Option<u64>,
    display: Option<TransportedItemStack>,
    dirty: bool,
}

impl Switch {
    pub(crate) fn load(world: World<'_>, pos: Pos) -> Self {
        let mut data = world
            .entity(pos)
            .as_deref()
            .and_then(|bytes| fastnbt::from_bytes::<Data>(bytes).ok())
            .unwrap_or_default();
        data.on_above_amount = data.on_above_amount.max(1);
        data.off_below_amount = data.off_below_amount.clamp(0, data.on_above_amount - 1);
        Self {
            data,
            scan_at: 0,
            output_at: None,
            display: None,
            dirty: false,
        }
    }
}

pub(crate) fn event(
    world: World<'_>,
    states: &StateIds,
    switches: &mut HashMap<Pos, Switch>,
    event: &ContentEvent,
) {
    let pos = event.position;
    if states.switches.contains_key(&event.state) && event.reason != content::UNLOADED {
        let entry = switches
            .entry(pos)
            .or_insert_with(|| Switch::load(world, pos));
        if event.reason == content::NEIGHBOR_CHANGED {
            entry.scan_at = 0;
        }
    } else if let Some(switch) = switches.remove(&pos) {
        if let Some(display) = switch.display {
            belt_inventory::remove_display(world, &display);
        }
        if event.reason == content::STATE_CHANGED {
            switch.data.filter.drop_item(world, pos);
        }
    }
}

#[derive(Default)]
struct Stock {
    amount: i32,
    capacity: i32,
}

impl Stock {
    fn item(&mut self, stack: Option<&Value>, limit: u32, max_stack: u32, filter: &Filter) {
        let space = limit.min(max_stack).min(i32::MAX as u32) as i32;
        if space == 0 {
            return;
        }
        self.capacity = self.capacity.saturating_add(space);
        if let Some(stack) = stack.filter(|stack| filter.matches(stack)) {
            self.amount = self.amount.saturating_add(belt_inventory::count(stack));
        }
    }
}

pub(crate) fn supports_stock(states: &StateIds, id: u32) -> bool {
    fluid_target(id)
        || states.chutes.contains_key(&id)
        || matches!(states.kind(id), Some(crate::kinetics::Kind::Belt(_)))
        || states
            .by_id
            .get(&id)
            .is_some_and(|state| matches!(state.block, crate::depot::ID))
}

pub(crate) fn fluid_tank(
    world: World<'_>,
    pos: Pos,
    state: u32,
) -> Option<crate::fluids::tank::TankInventory> {
    if crate::fluids::runtime::is_tank(state) {
        crate::fluids::tank_runtime::canonical(world, pos).map(|(_, tank)| tank)
    } else if crate::fluids::runtime::is_item_drain(state) {
        crate::fluids::item_drain::load(world, pos)
    } else if crate::fluids::runtime::is_spout(state) {
        crate::fluids::spout::load_tank(world, pos)
    } else {
        None
    }
}

fn fluid_target(state: u32) -> bool {
    crate::fluids::runtime::is_tank(state)
        || crate::fluids::runtime::is_item_drain(state)
        || crate::fluids::runtime::is_spout(state)
}

fn sample(
    world: World<'_>,
    kinetics: &Kinetics,
    pos: Pos,
    state: u32,
    filter: &Filter,
) -> Option<Stock> {
    if let Some(tank) = fluid_tank(world, pos, state) {
        return Some(Stock {
            amount: tank
                .fluid()
                .filter(|fluid| filter.matches_fluid(fluid))
                .map_or(0, |fluid| fluid.amount as i32),
            capacity: tank.capacity() as i32,
        });
    }
    let mut stock = Stock::default();
    if let Some(depot) = kinetics.depots.get(&pos) {
        let (held, outputs, slots) = depot.stock_slots();
        stock.item(
            held.map(|item| &item.item),
            64,
            held.map_or(64, |item| item.max_count().max(0) as u32),
            filter,
        );
        let mut filled = 0;
        for item in outputs {
            stock.item(Some(&item.item), 64, item.max_count().max(0) as u32, filter);
            filled += 1;
        }
        stock.capacity += (slots - filled) as i32 * 64;
        return Some(stock);
    }
    if let Some(chute) = kinetics.chutes.entries.get(&pos) {
        let item = chute.held();
        stock.item(
            item.map(|item| &item.item),
            64,
            item.map_or(64, |item| item.max_count().max(0) as u32),
            filter,
        );
        return Some(stock);
    }
    if matches!(kinetics.kind(pos), Some(crate::kinetics::Kind::Belt(belt)) if crate::belt_movement::can_transport_objects(belt))
    {
        let item = belt_inventory::stack_at_segment(kinetics, pos);
        stock.item(
            item.map(|item| &item.item),
            64,
            item.map_or(64, |item| item.max_count().max(0) as u32),
            filter,
        );
        return Some(stock);
    }
    let container = CONTAINERS.get()?.at(world, pos, NO_FACE)?;
    for slot in 0..container.slots()? {
        let bytes = container.stack(slot);
        let max = bytes
            .as_ref()
            .and_then(|bytes| STACKS.get()?.info(bytes))
            .map_or(64, |info| info.max_count);
        let item = bytes
            .as_ref()
            .and_then(|bytes| fastnbt::from_bytes(bytes).ok());
        stock.item(item.as_ref(), container.slot_limit(slot), max, filter);
    }
    Some(stock)
}

pub(crate) fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics, time: u64) {
    let mut switches = std::mem::take(&mut kinetics.threshold_switches);
    for (&pos, switch) in &mut switches {
        if time < switch.scan_at && switch.output_at.is_none() && !switch.dirty {
            continue;
        }
        let id = read(world, pos);
        let Some(&previous) = states.switches.get(&id) else {
            continue;
        };
        let mut next = previous;
        if time >= switch.scan_at {
            let target = shift(pos, previous.mount.target);
            let target_state = read(world, target);
            if target_state != content::UNLOADED_STATE {
                let (amount, capacity) =
                    sample(world, kinetics, target, target_state, &switch.data.filter)
                        .map_or((-1, -1), |stock| {
                            (stock.amount.clamp(0, stock.capacity), stock.capacity)
                        });
                let powered = if switch.data.powered {
                    amount > switch.data.off_below_amount
                } else {
                    amount >= switch.data.on_above_amount
                };
                switch.dirty |= (amount, capacity, powered)
                    != (
                        switch.data.current_amount,
                        switch.data.current_max_amount,
                        switch.data.powered,
                    );
                switch.data.current_amount = amount;
                switch.data.current_max_amount = capacity;
                switch.data.powered = powered;
                next.level = if amount > 0 {
                    (1 + 4 * i64::from(amount) / i64::from(capacity)) as u8
                } else {
                    0
                };
            }
            switch.scan_at = time.saturating_add(SAMPLE_INTERVAL);
        }
        let power = switch.data.powered != switch.data.inverted;
        if previous.powered == power {
            switch.output_at = None;
        } else if time
            >= *switch
                .output_at
                .get_or_insert_with(|| time.saturating_add(POWER_DELAY))
        {
            next.powered = power;
        }
        update(world, states, pos, previous, next);
        if switch.dirty
            && let Ok(bytes) = fastnbt::to_bytes(&switch.data)
            && world.set_entity(pos, &bytes)
        {
            switch.dirty = false;
        }
        if switch.data.filter.stack.is_some() || switch.display.is_some() {
            directed_detector::render_filter(
                world,
                pos,
                next.mount,
                &switch.data.filter,
                &mut switch.display,
            );
        }
    }
    kinetics.threshold_switches = switches;
}

fn update(world: World<'_>, states: &StateIds, pos: Pos, previous: SwitchState, next: SwitchState) {
    if next != previous
        && let Some(state) = next.id(states)
    {
        world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: if next.powered != previous.powered || next.mount.target != previous.mount.target
            {
                content::UPDATE_ALL
            } else {
                content::UPDATE_CLIENTS
            },
        }]);
    }
}

pub(crate) fn use_block(
    context: &mut UseBlock<'_>,
    states: &StateIds,
    switch: &mut Switch,
) -> BlockUseResult {
    let Some(&state) = states.switches.get(&context.state) else {
        return BlockUseResult::TryEmptyHand;
    };
    let item = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok());
    let (x, y) = state.mount.coordinates(context.face, context.hit);
    let front = context.face == state.mount.target ^ 1;
    let world = context.world;
    let is_fluid = fluid_target(read(world, shift(context.position, state.mount.target)));
    let editable = context.main_hand && context.player.is_some_and(|player| player.may_build());
    if editable && item.as_ref().and_then(item_id) == Some(WRENCH) {
        if !front {
            let next = SwitchState {
                mount: state.mount.rotated(context.face),
                ..state
            };
            update(world, states, context.position, state, next);
        } else if x.abs() <= 0.14 && y.abs() <= 0.14 {
            switch.data.inverted = !switch.data.inverted;
            let next = SwitchState {
                powered: switch.data.powered != switch.data.inverted,
                ..state
            };
            update(world, states, context.position, state, next);
        } else {
            let step = if context.sneaking {
                if is_fluid { 1000 } else { 64 }
            } else {
                1
            };
            let delta = if x > 0.0 { step } else { -step };
            if y >= 0.0 {
                switch.data.on_above_amount = switch
                    .data
                    .on_above_amount
                    .saturating_add(delta)
                    .max(switch.data.off_below_amount + 1);
            } else {
                switch.data.off_below_amount = switch
                    .data
                    .off_below_amount
                    .saturating_add(delta)
                    .clamp(0, switch.data.on_above_amount - 1);
            }
        }
        switch.scan_at = 0;
        switch.dirty = true;
    } else if editable
        && x.abs() <= 0.19
        && y.abs() <= 0.19
        && state.mount.filter_face(context.face, true)
        && (item.is_some() || context.sneaking)
        && switch
            .data
            .filter
            .configure(context, item.as_ref(), 0.0, false)
    {
        switch.scan_at = 0;
        switch.dirty = true;
        return BlockUseResult::Success;
    } else if context.with_item {
        return BlockUseResult::TryEmptyHand;
    }
    let value = if switch.data.current_amount < 0 {
        "No stock".to_owned()
    } else {
        format!(
            "{} / {} {}",
            switch.data.current_amount,
            switch.data.current_max_amount,
            if is_fluid { "mB" } else { "items" }
        )
    };
    context.action_bar(&format!(
        "{value} | On at {}, off at {}{}",
        switch.data.on_above_amount,
        switch.data.off_below_amount,
        if switch.data.inverted {
            " | Inverted"
        } else {
            ""
        }
    ));
    BlockUseResult::Success
}
