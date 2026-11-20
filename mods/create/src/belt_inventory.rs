//! Create 6.0.10's `BeltInventory`, `TransportedItemStack` and `ItemHandlerBeltSegment`: items
//! riding a belt chain, picked up from item entities and handed on, ejected or held at the end.
//!
//! Belt processing, funnels, tunnels and crushers aren't ported, so no item is ever held by
//! them. Items are drawn with item displays, since a stock client has no belt renderer.

use std::sync::atomic::{AtomicU64, Ordering};

use fastnbt::Value;
use moddedmc_api::{
    Pos,
    content::World,
    effects::SOUND_PLAYERS,
    entities::{ENTITY_ALIVE, Entity},
    interaction::{BlockUseResult, UseBlock},
    stacks::{Displays, ItemDisplay, ItemStacks},
};
use serde::{Deserialize, Serialize};

use crate::{
    DISPLAYS, EFFECTS, STACKS,
    belt_movement::{SPEED_DIVISOR, can_transport_objects, normalize},
    kinetics::{Belt, FACES, Kind, Kinetics, Slope, face_axis, positive},
};

/// `BeltInventory.tick`'s `spacing`.
const SPACING: f32 = 1.0;
/// `Direction.UP.get3DDataValue()`.
const UP: u8 = 1;
/// `ItemDisplayContext.FIXED`, which `BeltRenderer` draws items with.
const FIXED: u8 = 8;
/// Ticks a display takes to reach each new position.
const DISPLAY_INTERPOLATION: i32 = 2;
/// Save moving items' positions this often even when nothing else changed.
const SAVE_INTERVAL: u32 = 20;

/// `TransportedItemStack`, with its NBT field names.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct TransportedItemStack {
    /// Vanilla saved `ItemStack`.
    pub item: Value,
    #[serde(default)]
    pub pos: f32,
    #[serde(default)]
    pub prev_pos: f32,
    #[serde(default)]
    pub offset: f32,
    #[serde(default)]
    pub prev_offset: f32,
    #[serde(default)]
    pub in_segment: i32,
    #[serde(default)]
    pub angle: i32,
    #[serde(default)]
    pub in_direction: i32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked_externally: bool,
    /// The item display drawing this stack, and what it was last told.
    #[serde(skip)]
    display: Option<(i32, ItemDisplay)>,
    /// The item places a block, which `BeltRenderer` draws standing rather than lying flat.
    #[serde(skip)]
    block: Option<bool>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde passes a reference"
)]
const fn is_false(value: &bool) -> bool {
    !*value
}

static RANDOM: AtomicU64 = AtomicU64::new(0x2545_f491_4f6c_dd1d);

/// `R.nextInt(bound)`; the rolls aren't seed-reproducible, as in Create.
pub fn next_int(bound: u32) -> i32 {
    let mut x = RANDOM.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RANDOM.store(x, Ordering::Relaxed);
    (x % u64::from(bound)) as i32
}

pub fn count(item: &Value) -> i32 {
    match item {
        Value::Compound(map) => match map.get("count") {
            Some(Value::Int(count)) => *count,
            Some(Value::Byte(count)) => i32::from(*count),
            Some(Value::Short(count)) => i32::from(*count),
            // ItemStack.CODEC defaults the count to 1.
            None => 1,
            _ => 0,
        },
        _ => 0,
    }
}

pub fn with_count(item: &Value, count: i32) -> Value {
    let mut item = item.clone();
    if let Value::Compound(map) = &mut item {
        map.insert("count".to_owned(), Value::Int(count));
    }
    item
}

impl TransportedItemStack {
    /// `new TransportedItemStack(stack)`. Upright items (fluid containers and the
    /// `create:upright_on_belt` tag) aren't known to the port, so every item gets a random angle.
    pub fn new(item: Value) -> Self {
        let angle = next_int(360);
        let offset = target_side_offset(angle);
        Self {
            item,
            pos: 0.0,
            prev_pos: 0.0,
            offset,
            prev_offset: offset,
            in_segment: 0,
            angle,
            in_direction: i32::from(UP),
            locked: false,
            locked_externally: false,
            display: None,
            block: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        count(&self.item) <= 0
    }

    pub fn bytes(&self) -> Option<Vec<u8>> {
        fastnbt::to_bytes(&self.item).ok()
    }

    pub fn resize(&mut self, world: World<'_>, count: i32) {
        self.item = with_count(&self.item, count);
        if let (Some((id, _)), Some(&displays), Some(bytes)) =
            (self.display, DISPLAYS.get(), self.bytes())
            && !displays.set_item(world, id, &bytes)
        {
            self.display = None;
        }
    }

    pub fn clear_display(&mut self) {
        self.display = None;
    }
}

/// `TransportedItemStack.getTargetSideOffset`.
fn target_side_offset(angle: i32) -> f32 {
    (angle - 180) as f32 / (360.0 * 3.0)
}

/// `BeltInventory`, saved as the controller's `Inventory`. The client-only `LazyItem` isn't
/// kept.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct BeltInventory {
    #[serde(default)]
    pub items: Vec<TransportedItemStack>,
    /// `beltMovementPositive`.
    #[serde(default)]
    pub positive_order: bool,
    #[serde(skip)]
    to_insert: Vec<TransportedItemStack>,
    #[serde(skip)]
    unsaved: u32,
}

/// A controller and the fields `BeltInventory` reads from it.
#[derive(Clone, Copy)]
pub struct Chain {
    pub pos: Pos,
    pub belt: Belt,
    pub length: i32,
    pub speed: f32,
}

impl Chain {
    fn of(kinetics: &Kinetics, pos: Pos) -> Option<Self> {
        let be = kinetics.get(pos)?;
        let Kind::Belt(belt) = be.kind else {
            return None;
        };
        Some(Self {
            pos,
            belt,
            length: be.belt_length,
            speed: be.get_speed(),
        })
    }

    /// `getBeltMovementSpeed`.
    fn movement_speed(self) -> f32 {
        self.speed / SPEED_DIVISOR
    }

    /// `getDirectionAwareBeltMovementSpeed`.
    fn direction_aware_speed(self) -> f32 {
        let mut offset = if positive(self.belt.facing) {
            1.0
        } else {
            -1.0
        };
        if face_axis(self.belt.facing) == 0 {
            offset *= -1.0;
        }
        self.movement_speed() * offset
    }

    /// `getMovementFacing`.
    fn movement_facing(self) -> u8 {
        movement_facing(self.belt, self.speed)
    }

    fn verticality(self) -> i32 {
        match self.belt.slope {
            Slope::Downward => -1,
            Slope::Upward => 1,
            _ => 0,
        }
    }

    /// `BeltHelper.getPositionForOffset`.
    fn position_for_offset(self, offset: i32) -> Pos {
        let [x, _, z] = FACES[usize::from(self.belt.facing)];
        Pos {
            x: self.pos.x + offset * x,
            y: self.pos.y + offset.clamp(0, (self.length - 1).max(0)) * self.verticality(),
            z: self.pos.z + offset * z,
        }
    }

    /// `BeltHelper.getVectorForOffset`.
    fn vector_for_offset(self, offset: f32) -> [f64; 3] {
        let mut vertical = self.verticality() as f32;
        if offset < 0.5 {
            vertical = 0.0;
        }
        vertical *= offset.min(self.length as f32 - 0.5) - 0.5;
        let along = if self.belt.slope == Slope::Vertical {
            0.0
        } else {
            f64::from(offset - 0.5)
        };
        let [x, _, z] = FACES[usize::from(self.belt.facing)];
        [
            f64::from(self.pos.x) + 0.5 + f64::from(x) * along,
            f64::from(self.pos.y) + 0.5 + f64::from(vertical),
            f64::from(self.pos.z) + 0.5 + f64::from(z) * along,
        ]
    }

    /// `getBeltChainDirection`: `getMovementDirection(true, true)`.
    fn chain_direction(self) -> [f64; 3] {
        if self.speed == 0.0 {
            return [0.0; 3];
        }
        let axis = face_axis(self.belt.facing);
        let mut facing = if axis == 0 { 4 } else { 3 };
        if self.speed < 0.0 {
            facing = opposite(facing);
        }
        let [x, _, z] = FACES[usize::from(facing)];
        if self.belt.slope == Slope::Horizontal {
            return [f64::from(x), 0.0, f64::from(z)];
        }
        let up = if facing == self.belt.facing {
            Slope::Upward
        } else {
            Slope::Downward
        };
        let y = if self.belt.slope == up { 1.0 } else { -1.0 };
        [f64::from(x), y, f64::from(z)]
    }
}

/// `BeltBlockEntity.getMovementFacing` for a segment with this state and speed.
fn movement_facing(belt: Belt, speed: f32) -> u8 {
    let axis_x = face_axis(belt.facing) == 0;
    let negative = (speed / SPEED_DIVISOR < 0.0) ^ axis_x;
    match (axis_x, negative) {
        (true, true) => 4,
        (true, false) => 5,
        (false, true) => 2,
        (false, false) => 3,
    }
}

const fn opposite(face: u8) -> u8 {
    face ^ 1
}

/// `Direction.getAxisDirection().getStep()`.
fn step(face: u8) -> f32 {
    if positive(face) { 1.0 } else { -1.0 }
}

/// `BeltInventory.Ending`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Eject,
    Insert,
    Blocked,
}

impl Ending {
    const fn margin(self) -> f32 {
        match self {
            Self::Eject => 0.0,
            Self::Insert => 0.25,
            Self::Blocked => 0.45,
        }
    }
}

impl BeltInventory {
    /// `canInsertAtFromSide`. `facing` is the controller's movement facing.
    fn can_insert_at_from_side(&self, segment: i32, side: u8, facing: u8) -> bool {
        let mut segment_pos = segment as f32;
        if facing == opposite(side) {
            return false;
        }
        if facing != side {
            segment_pos += 0.5;
        } else if !self.positive_order {
            segment_pos += 1.0;
        }
        let blocking = |stack: &TransportedItemStack| {
            stack.in_segment == segment
                && stack.in_direction == i32::from(side)
                && if self.positive_order {
                    stack.pos <= segment_pos + 1.0
                } else {
                    stack.pos >= segment_pos - 1.0
                }
        };
        !self.items.iter().chain(&self.to_insert).any(blocking)
    }

    /// `insert`: keep the furthest item first.
    fn insert(&mut self, stack: TransportedItemStack) {
        let index = self
            .items
            .iter()
            .position(|other| (other.pos < stack.pos) == self.positive_order)
            .unwrap_or(self.items.len());
        self.items.insert(index, stack);
    }

    /// `BeltInventory.tick`. Returns whether the inventory changed (`notifyUpdate`).
    #[expect(
        clippy::too_many_lines,
        reason = "one Create method, kept whole for review against it"
    )]
    fn tick(&mut self, world: World<'_>, kinetics: &mut Kinetics, chain: Chain) -> bool {
        let mut changed = false;
        if !self.to_insert.is_empty() {
            for stack in std::mem::take(&mut self.to_insert) {
                self.insert(stack);
            }
            changed = true;
        }
        if chain.speed == 0.0 {
            return changed;
        }
        let belt_speed = chain.direction_aware_speed();
        if self.positive_order != (belt_speed > 0.0) {
            self.positive_order = !self.positive_order;
            self.items.reverse();
            changed = true;
        }
        let positive_order = self.positive_order;
        let movement_facing = chain.movement_facing();
        let horizontal = chain.belt.slope == Slope::Horizontal;
        let mut ending = None;
        let mut moved = false;
        // Create keeps an item it just ejected or handed on as the next one's stackInFront.
        let mut current: Option<f32> = None;
        let mut index = 0;
        while index < self.items.len() {
            let in_front = current;
            let item = &mut self.items[index];
            item.prev_pos = item.pos;
            item.prev_offset = item.offset;
            if item.is_empty() {
                let item = self.items.remove(index);
                remove_display(world, &item);
                current = None;
                continue;
            }
            current = Some(item.pos);
            index += 1;

            let mut movement = belt_speed;
            if item.locked_externally {
                item.locked_externally = false;
                continue;
            }
            let mut no_movement = false;
            let current_pos = item.pos;
            if let Some(front) = in_front {
                let diff = front - current_pos;
                if diff.abs() <= SPACING {
                    no_movement = true;
                }
                movement = if positive_order {
                    movement.min(diff - SPACING)
                } else {
                    movement.max(diff + SPACING)
                };
            }
            let mut diff_to_end = if positive_order {
                chain.length as f32 - current_pos
            } else {
                -current_pos
            };
            if diff_to_end.abs() < movement.abs() + 1.0 {
                let ending = *ending
                    .get_or_insert_with(|| resolve_ending(world, kinetics, chain, positive_order));
                diff_to_end += if positive_order {
                    -ending.margin()
                } else {
                    ending.margin()
                };
            }
            let limited = if positive_order {
                movement.min(diff_to_end)
            } else {
                movement.max(diff_to_end)
            };

            // handleBeltProcessingAndCheckIfRemoved: with no processing block above, a held item
            // is let go.
            if horizontal && item.locked {
                item.locked = false;
                changed = true;
            }
            if no_movement {
                continue;
            }

            item.pos += limited;
            let diff_to_middle = target_side_offset(item.angle) - item.offset;
            item.offset += (diff_to_middle * limited.abs() * 6.0)
                .clamp(-diff_to_middle.abs(), diff_to_middle.abs());
            current = Some(item.pos);
            moved |= limited != 0.0;
            if limited == movement {
                continue;
            }

            match ending {
                Some(Ending::Insert) => {
                    let next =
                        chain.position_for_offset(if positive_order { chain.length } else { -1 });
                    if let Some(depot) = kinetics.depots.get_mut(&next) {
                        let accepted = crate::depot::try_inserting_from_side(
                            world,
                            next,
                            depot,
                            item,
                            movement_facing,
                        );
                        let remaining = count(&item.item) - accepted;
                        if accepted > 0 {
                            if remaining == 0 {
                                self.items.remove(index - 1);
                                index -= 1;
                            } else {
                                item.resize(world, remaining);
                            }
                            changed = true;
                        }
                    } else if try_inserting_from_side(
                        kinetics,
                        chain.pos,
                        next,
                        item,
                        movement_facing,
                    ) {
                        // The display moves on with the stack.
                        self.items.remove(index - 1);
                        index -= 1;
                        changed = true;
                    }
                }
                Some(Ending::Eject) => {
                    let item = self.items.remove(index - 1);
                    index -= 1;
                    eject(world, chain, &item);
                    remove_display(world, &item);
                    changed = true;
                }
                Some(Ending::Blocked) | None => {}
            }
        }
        if moved {
            self.unsaved += 1;
        }
        if changed || self.unsaved >= SAVE_INTERVAL {
            self.unsaved = 0;
            changed = true;
        }
        changed
    }
}

/// `resolveEnding`. A depot exposes direct input even while it is occupied.
fn resolve_ending(
    world: World<'_>,
    kinetics: &Kinetics,
    chain: Chain,
    positive_order: bool,
) -> Ending {
    let next = chain.position_for_offset(if positive_order { chain.length } else { -1 });
    if matches!(kinetics.kind(next), Some(Kind::Belt(_))) || kinetics.depots.contains_key(&next) {
        return Ending::Insert;
    }
    if world.face_sturdy(next, opposite(chain.movement_facing())) {
        return Ending::Blocked;
    }
    Ending::Eject
}

/// `BeltBlockEntity.tryInsertingFromSide` on the segment at `pos`, for a stack arriving through
/// `side`. True when it was taken.
fn try_inserting_from_side(
    kinetics: &mut Kinetics,
    ticking: Pos,
    pos: Pos,
    stack: &TransportedItemStack,
    side: u8,
) -> bool {
    let Some(segment) = kinetics.get(pos) else {
        return false;
    };
    let Kind::Belt(belt) = segment.kind else {
        return false;
    };
    let (speed, index) = (segment.get_speed(), segment.belt_index);
    let facing = movement_facing(belt, speed);
    // DirectBeltInputBehaviour.canInsertFromSide: BeltBlockEntity.canInsertFrom.
    if speed == 0.0 || !can_transport_objects(belt) || facing == opposite(side) {
        return false;
    }
    // A chain's own inventory is out of its block entity while it ticks.
    let Some(controller) = segment
        .controller
        .filter(|&controller| controller != ticking)
    else {
        return false;
    };
    let Some(chain) = Chain::of(kinetics, controller) else {
        return false;
    };
    let previous_is_belt = matches!(
        kinetics.kind(step_pos(pos, opposite(facing))),
        Some(Kind::Belt(_))
    );
    let direction_aware = Chain {
        pos,
        belt,
        length: 0,
        speed,
    }
    .direction_aware_speed();
    let Some(be) = kinetics.get_mut(controller) else {
        return false;
    };
    let inventory = be.inventory.get_or_insert_with(BeltInventory::default);
    // isOccupied.
    if !inventory.can_insert_at_from_side(index, side, chain.movement_facing()) {
        return false;
    }
    let mut stack = stack.clone();
    stack.pos = index as f32 + 0.5 - direction_aware.signum() / 16.0;
    if facing != side {
        stack.offset = step(side) * 0.675;
        if face_axis(side) == 0 {
            stack.offset *= -1.0;
        }
    } else {
        // A smoother transition from belt to belt.
        let extra = if stack.prev_pos != 0.0 && previous_is_belt {
            0.26
        } else {
            0.0
        };
        stack.pos = if direction_aware > 0.0 {
            index as f32 - extra
        } else {
            index as f32 + 1.0 + extra
        };
    }
    stack.prev_offset = stack.offset;
    stack.in_segment = index;
    stack.in_direction = i32::from(side);
    stack.prev_pos = stack.pos;
    inventory.to_insert.push(stack);
    be.dirty = true;
    true
}

fn step_pos(pos: Pos, face: u8) -> Pos {
    let [x, y, z] = FACES[usize::from(face)];
    Pos {
        x: pos.x + x,
        y: pos.y + y,
        z: pos.z + z,
    }
}

/// `BeltInventory.eject`.
fn eject(world: World<'_>, chain: Chain, item: &TransportedItemStack) {
    let (Some(&stacks), Some(bytes)) = (STACKS.get(), item.bytes()) else {
        return;
    };
    let out = chain.vector_for_offset(item.pos);
    let speed = f64::from(chain.movement_speed().abs().max(1.0 / 8.0));
    let [x, y, z] = chain.chain_direction();
    let motion = [x * speed, y * speed + 1.0 / 8.0, z * speed];
    let nudge = normalize(motion).map(|c| c * 0.001);
    let position = [
        out[0] + nudge[0],
        out[1] + nudge[1] + f64::from(6.0_f32 / 16.0),
        out[2] + nudge[2],
    ];
    let _ = stacks.spawn(world, position, motion, &bytes);
}

/// `BeltBlockEntity.destroy` on a controller: `ejectAll`.
pub fn eject_all(world: World<'_>, chain: Chain, inventory: &BeltInventory) {
    for item in &inventory.items {
        eject(world, chain, item);
    }
    remove_displays(world, inventory);
}

/// Take down every display an inventory owns, when its chunk unloads or it is ejected.
pub fn remove_displays(world: World<'_>, inventory: &BeltInventory) {
    for item in inventory.items.iter().chain(&inventory.to_insert) {
        remove_display(world, item);
    }
}

pub fn remove_display(world: World<'_>, item: &TransportedItemStack) {
    if let (Some(&displays), Some((id, _))) = (DISPLAYS.get(), item.display) {
        displays.remove(world, id);
    }
}

/// `BeltBlock.entityInside` for an item entity on the segment at `pos`: centre it, then
/// `ItemHandlerBeltSegment.insertItem(0, stack, false)`.
pub fn entity_inside(kinetics: &mut Kinetics, pos: Pos, entity: Entity<'_>, motion_y: f64) {
    let Some(&stacks) = STACKS.get() else { return };
    // ItemHelper.fromItemEntity is empty for a dead entity.
    if entity.flags() & ENTITY_ALIVE == 0 || motion_y > 0.0 {
        return;
    }
    let Some(bytes) = stacks.entity_stack(entity) else {
        return;
    };
    // PackageEntity.centerPackage only acts on packages, which aren't ported.
    let Some(segment) = kinetics.get(pos) else {
        return;
    };
    // initializeItemHandler.
    let (Some(controller), index) = (segment.controller, segment.belt_index) else {
        return;
    };
    if segment.belt_length == 0 {
        return;
    }
    let Some(chain) = Chain::of(kinetics, controller) else {
        return;
    };
    let (Ok(item), Some(info)) = (fastnbt::from_bytes::<Value>(&bytes), stacks.info(&bytes)) else {
        return;
    };
    let Some(be) = kinetics.get_mut(controller) else {
        return;
    };
    let inventory = be.inventory.get_or_insert_with(BeltInventory::default);
    if !inventory.can_insert_at_from_side(index, UP, chain.movement_facing()) {
        return;
    }
    // ItemHelper.limitCountToMaxStackSize.
    let (count, max) = (info.count as i32, info.max_count as i32);
    let (inserted, remainder) = if count <= max {
        (count, 0)
    } else {
        (max, count - max)
    };
    let mut stack = TransportedItemStack::new(with_count(&item, inserted));
    stack.block = Some(info.block);
    stack.in_segment = index;
    let toward = if inventory.positive_order { -1.0 } else { 1.0 };
    stack.pos = index as f32 + 0.5 + toward / 16.0;
    stack.prev_pos = stack.pos;
    inventory.to_insert.push(stack);
    be.dirty = true;
    if remainder == 0 {
        stacks.set_entity_stack(entity, &[]);
    } else if remainder != count
        && let Ok(rest) = fastnbt::to_bytes(&with_count(&item, remainder))
    {
        stacks.set_entity_stack(entity, &rest);
    }
}

/// Where `BeltRenderer.renderItem` draws a stack, as an item display.
fn display_for(chain: Chain, item: &TransportedItemStack, block: bool) -> ItemDisplay {
    let [dx, _, dz] = FACES[usize::from(chain.belt.facing)].map(f64::from);
    let offset = item.pos;
    let vertical = if offset < 0.5 {
        0.0
    } else {
        chain.verticality() as f32 * (offset.min(chain.length as f32 - 0.5) - 0.5)
    };
    // beltStartOffset, then the offset along the chain.
    let mut position = [
        f64::from(chain.pos.x) + 0.5 + dx * (f64::from(offset) - 0.5),
        f64::from(chain.pos.y) + f64::from(15.0_f32 / 16.0) + f64::from(vertical),
        f64::from(chain.pos.z) + 0.5 + dz * (f64::from(offset) - 0.5),
    ];
    // alongX: the belt runs along Z.
    if face_axis(chain.belt.facing) == 2 {
        position[0] += f64::from(item.offset);
    } else {
        position[2] -= f64::from(item.offset);
    }
    item_display(position, item.angle, block)
}

pub fn item_display(mut position: [f64; 3], angle: i32, block: bool) -> ItemDisplay {
    let (sin_y, cos_y) = ((angle as f32).to_radians() * 0.5).sin_cos();
    let rotation = if block {
        [0.0, sin_y, 0.0, cos_y]
    } else {
        // Lie flat: Y by the angle, then X by 90 degrees, scaled by half first.
        position[1] -= 0.093_75 * 0.5;
        let half = std::f32::consts::FRAC_1_SQRT_2;
        [cos_y * half, sin_y * half, -sin_y * half, cos_y * half]
    };
    ItemDisplay {
        position,
        rotation,
        scale: 0.5,
        context: FIXED,
        interpolation: DISPLAY_INTERPOLATION,
    }
}

pub fn render_stack(
    world: World<'_>,
    displays: Displays,
    item: &mut TransportedItemStack,
    shown: ItemDisplay,
) {
    match item.display {
        Some((_, last)) if last == shown => {}
        Some((id, _)) if displays.update(world, id, &shown) => {
            item.display = Some((id, shown));
        }
        _ => {
            item.display = item
                .bytes()
                .and_then(|bytes| displays.spawn_item(world, &bytes, &shown))
                .map(|id| (id, shown));
        }
    }
}

impl TransportedItemStack {
    pub fn places_block(&mut self, stacks: ItemStacks) -> bool {
        if let Some(block) = self.block {
            return block;
        }
        let block = self
            .bytes()
            .and_then(|bytes| stacks.info(&bytes))
            .is_some_and(|info| info.block);
        self.block = Some(block);
        block
    }
}

/// Show each stack where Create's client would draw it.
fn render(
    world: World<'_>,
    stacks: ItemStacks,
    displays: Displays,
    chain: Chain,
    inventory: &mut BeltInventory,
) {
    for item in &mut inventory.items {
        let block = item.places_block(stacks);
        let shown = display_for(chain, item, block);
        render_stack(world, displays, item, shown);
    }
}

/// Every controller's `BeltInventory.tick`, in ticker order, after the kinetic ticks.
pub fn tick(world: World<'_>, kinetics: &mut Kinetics) {
    for dropped in kinetics.take_dropped() {
        if dropped.eject {
            eject_all(world, dropped.chain, &dropped.inventory);
        } else {
            remove_displays(world, &dropped.inventory);
        }
    }
    let (Some(&stacks), Some(&displays)) = (STACKS.get(), DISPLAYS.get()) else {
        return;
    };
    for controller in kinetics.belt_controllers() {
        let Some(chain) = Chain::of(kinetics, controller) else {
            continue;
        };
        let Some(mut inventory) = kinetics
            .get_mut(controller)
            .and_then(|be| be.inventory.take())
        else {
            continue;
        };
        let changed = inventory.tick(world, kinetics, chain);
        render(world, stacks, displays, chain, &mut inventory);
        if let Some(be) = kinetics.get_mut(controller) {
            be.dirty |= changed;
            be.inventory = Some(inventory);
        }
    }
}

/// `BeltBlock.useItemOn`: an empty main hand takes every stack within reach of the clicked
/// segment (`applyToEachWithin(index + .5, .55)`). Dyes, water, the wrench, packages, shafts and
/// casings aren't ported. `player.mayBuild()` isn't checked.
pub fn use_item_on(kinetics: &mut Kinetics, context: &mut UseBlock<'_>) -> BlockUseResult {
    if context.sneaking {
        return BlockUseResult::TryEmptyHand;
    }
    let Some(segment) = kinetics.get(context.position) else {
        return BlockUseResult::TryEmptyHand;
    };
    if !context.main_hand || context.stack.read().is_some() {
        return BlockUseResult::TryEmptyHand;
    }
    let (Some(controller), index) = (segment.controller, segment.belt_index) else {
        return BlockUseResult::TryEmptyHand;
    };
    let Some(inventory) = kinetics
        .get_mut(controller)
        .and_then(|be| be.inventory.as_mut())
    else {
        return BlockUseResult::TryEmptyHand;
    };
    let position = index as f32 + 0.5;
    let mut taken = false;
    inventory.items.retain(|item| {
        if (position - item.pos).abs() >= 0.55 {
            return true;
        }
        if let Some(bytes) = item.bytes() {
            context.give(&bytes);
        }
        remove_display(context.world, item);
        taken = true;
        false
    });
    if taken {
        if let Some(be) = kinetics.get_mut(controller) {
            be.dirty = true;
        }
        if let Some(&effects) = EFFECTS.get() {
            let Pos { x, y, z } = context.position;
            let pitch = 1.0 + next_int(1 << 24) as f32 / (1 << 24) as f32;
            effects.play_sound(
                context.world,
                [f64::from(x) + 0.5, f64::from(y) + 0.5, f64::from(z) + 0.5],
                "minecraft:entity.item.pickup",
                SOUND_PLAYERS,
                0.2,
                pitch,
            );
        }
    }
    BlockUseResult::TryEmptyHand
}
