//! Create 6.0.10's Content Observer and its stock-client filter controls.

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
    CONTENT, DIRECTIONS, StateIds, WRENCH,
    belt_inventory::{self, TransportedItemStack},
    belt_tunnel::{read, shift},
    directed_detector::{self, Mount},
    filtering::{Extraction, Filter},
    kinetics::{Kind, Kinetics},
    processing_recipes::item_id,
    threshold_switch,
};

pub(crate) const ID: &str = "create:content_observer";
const DEFAULT_DELAY: u8 = 6;
const BLOCK_DELAY: u8 = 3;
const FUNNEL_DELAY: u8 = 4;

#[derive(Clone, Copy)]
pub(crate) struct ObserverState {
    mount: Mount,
    powered: bool,
}

impl ObserverState {
    fn all() -> impl Iterator<Item = Self> {
        Mount::all().flat_map(|mount| [false, true].map(|powered| Self { mount, powered }))
    }

    fn properties(self) -> String {
        format!(
            "facing={},powered={},target={}",
            DIRECTIONS[usize::from(self.mount.facing)],
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
    let rows: Vec<_> = ObserverState::all()
        .map(|state| (state, state.properties()))
        .collect();
    let states: Vec<_> = rows
        .iter()
        .map(|(state, properties)| {
            let pitch = match state.mount.target {
                0 => 90,
                1 => -90,
                _ => 0,
            };
            State {
                properties,
                physical: "minecraft:glass",
                model: "",
                rotating_model: if state.powered {
                    "create:block/content_observer/block_powered"
                } else {
                    "create:block/content_observer/block"
                },
                x: 0,
                y: 0,
                orientation: directed_detector::orientation(pitch, state.mount.yaw()),
                shape: &[],
            }
        })
        .collect();
    content.add_block(
        registry,
        &Block {
            id: ID,
            display_name: "Smart Observer",
            entity: ID,
            item_model: "create:item/content_observer",
            states: &states,
        },
        Some(crate::placed_state),
    )
}

pub(crate) fn add_states(content: Content, states: &mut StateIds) {
    for state in ObserverState::all() {
        states.add(content, ID, &state.properties(), None);
        if let Some(id) = state.id(states) {
            states.observers.insert(id, state);
        }
    }
}

pub(crate) fn place(args: &Placement<'_>, states: &StateIds) -> Option<u32> {
    let mount = Mount::preferred(args, |id| {
        threshold_switch::supports_stock(states, id)
            || states.funnels.contains_key(&id)
            || crate::fluids::runtime::has_transport(id)
    });
    ObserverState {
        mount,
        powered: false,
    }
    .id(states)
}

pub(crate) fn connects(state: ObserverState, direction: u8) -> bool {
    direction < 6 && direction != state.mount.facing ^ 1
}

pub(crate) fn signal(state: ObserverState, query: &RedstoneQuery<'_>) -> u8 {
    if !query.direct && state.powered && query.direction != state.mount.target ^ 1 {
        15
    } else {
        0
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
struct Data {
    turn_off: u8,
    #[serde(flatten)]
    filter: Filter,
}

#[derive(Default)]
pub(crate) struct Observer {
    data: Data,
    pending_funnel_pulse: bool,
    target_block: Option<(u32, bool)>,
    display: Option<TransportedItemStack>,
    dirty: bool,
}

impl Observer {
    pub(crate) fn load(world: World<'_>, pos: Pos) -> Self {
        let mut data = world
            .entity(pos)
            .as_deref()
            .and_then(|bytes| fastnbt::from_bytes::<Data>(bytes).ok())
            .unwrap_or_default();
        data.turn_off = data.turn_off.min(DEFAULT_DELAY + 1);
        Self {
            data,
            ..Self::default()
        }
    }

    fn detects_block(&mut self, id: u32) -> bool {
        if self.data.filter.stack.is_none() {
            return false;
        }
        if let Some((previous, matches)) = self.target_block
            && previous == id
        {
            return matches;
        }
        let matches = CONTENT
            .get()
            .and_then(|content| content.block_item(id))
            .is_some_and(|id| {
                self.data
                    .filter
                    .matches(&fastnbt::nbt!({"id": id, "count": 1i32}))
            });
        self.target_block = Some((id, matches));
        matches
    }
}

pub(crate) fn event(
    world: World<'_>,
    states: &StateIds,
    observers: &mut HashMap<Pos, Observer>,
    event: &ContentEvent,
) {
    let pos = event.position;
    if states.observers.contains_key(&event.state) && event.reason != content::UNLOADED {
        observers
            .entry(pos)
            .or_insert_with(|| Observer::load(world, pos));
    } else if let Some(observer) = observers.remove(&pos) {
        if let Some(display) = observer.display {
            belt_inventory::remove_display(world, &display);
        }
        if event.reason == content::STATE_CHANGED {
            observer.data.filter.drop_item(world, pos);
        }
    }
}

pub(crate) fn funnel_transfer(world: World<'_>, kinetics: &mut Kinetics, pos: Pos, item: &Value) {
    if kinetics.content_observers.is_empty() {
        return;
    }
    let Some(states) = crate::states() else {
        return;
    };
    for side in 0..6 {
        let next = shift(pos, side);
        if let Some(observer) = kinetics.content_observers.get_mut(&next)
            && states
                .observers
                .get(&read(world, next))
                .is_some_and(|state| state.mount.target == side ^ 1)
            && observer.data.filter.matches(item)
        {
            observer.pending_funnel_pulse = true;
        }
    }
}

fn source_signal(
    world: World<'_>,
    kinetics: &mut Kinetics,
    pos: Pos,
    state: u32,
    observer: &mut Observer,
) -> u8 {
    if state == content::UNLOADED_STATE {
        return 0;
    }
    if observer.detects_block(state) {
        return BLOCK_DELAY;
    }
    let filter = &observer.data.filter;
    if let Some(depot) = kinetics.depots.get(&pos) {
        return if depot
            .stock_slots()
            .0
            .is_some_and(|held| filter.matches(&held.item))
        {
            DEFAULT_DELAY
        } else {
            0
        };
    }
    let matches = if matches!(kinetics.kind(pos), Some(Kind::Belt(_))) {
        belt_inventory::centered_match(kinetics, pos, filter)
    } else if crate::fluids::runtime::has_transport(state) {
        crate::fluids::runtime::completed_inbound(world, pos, |fluid| filter.matches_fluid(fluid))
    } else {
        crate::funnel::extract_at(
            world,
            kinetics,
            pos,
            NO_FACE,
            Extraction {
                amount: 1,
                exact: false,
                filter: Some(filter),
            },
            true,
        )
        .is_some()
            || threshold_switch::fluid_tank(world, pos, state)
                .and_then(|tank| tank.fluid().map(|fluid| filter.matches_fluid(fluid)))
                .unwrap_or(false)
    };
    if matches { DEFAULT_DELAY } else { 0 }
}

pub(crate) fn tick(world: World<'_>, states: &StateIds, kinetics: &mut Kinetics) {
    let mut observers = std::mem::take(&mut kinetics.content_observers);
    for (&pos, observer) in &mut observers {
        let Some(&previous) = states.observers.get(&read(world, pos)) else {
            continue;
        };
        let target = shift(pos, previous.mount.target);
        let pulse = if std::mem::take(&mut observer.pending_funnel_pulse) {
            FUNNEL_DELAY
        } else {
            0
        };
        let signal_delay = source_signal(world, kinetics, target, read(world, target), observer);
        let delay = pulse.max(signal_delay);
        let remaining = if delay > 0 {
            delay + 1
        } else {
            observer.data.turn_off.saturating_sub(1)
        };
        observer.dirty |= observer.data.turn_off != remaining;
        observer.data.turn_off = remaining;
        let next = ObserverState {
            powered: remaining > 0,
            ..previous
        };
        if next.powered != previous.powered {
            write(world, states, pos, next);
        }
        if observer.dirty
            && let Ok(bytes) = fastnbt::to_bytes(&observer.data)
            && world.set_entity(pos, &bytes)
        {
            observer.dirty = false;
        }
        if observer.data.filter.stack.is_some() || observer.display.is_some() {
            directed_detector::render_filter(
                world,
                pos,
                next.mount,
                &observer.data.filter,
                &mut observer.display,
            );
        }
    }
    kinetics.content_observers = observers;
}

fn write(world: World<'_>, states: &StateIds, pos: Pos, next: ObserverState) {
    if let Some(state) = next.id(states) {
        world.write(&[BlockUpdate {
            position: pos,
            state,
            flags: content::UPDATE_ALL,
        }]);
    }
}

pub(crate) fn use_block(
    context: &mut UseBlock<'_>,
    states: &StateIds,
    state: ObserverState,
    observer: &mut Observer,
) -> BlockUseResult {
    if !context.main_hand || !context.player.is_some_and(|player| player.may_build()) {
        return BlockUseResult::TryEmptyHand;
    }
    let item = context
        .stack
        .read()
        .and_then(|bytes| fastnbt::from_bytes(&bytes).ok());
    if item.as_ref().and_then(item_id) == Some(WRENCH) {
        let mount = state.mount.rotated(context.face);
        if mount != state.mount {
            write(
                context.world,
                states,
                context.position,
                ObserverState { mount, ..state },
            );
        }
        return BlockUseResult::Success;
    }
    let (x, y) = state.mount.coordinates(context.face, context.hit);
    if state.mount.filter_face(context.face, false)
        && x.abs() <= 0.19
        && y.abs() <= 0.19
        && (item.is_some() || context.sneaking)
        && observer
            .data
            .filter
            .configure(context, item.as_ref(), 0.0, false)
    {
        observer.dirty = true;
        observer.target_block = None;
        return BlockUseResult::Success;
    }
    BlockUseResult::TryEmptyHand
}
