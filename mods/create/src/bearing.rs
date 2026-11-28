//! Create 6.0.10's MechanicalBearingBlockEntity and BearingContraption.

use moddedmc_api::{
    Pos, RegistrationError, Registry,
    abi::DayTime,
    content::{Block, Content, State, World},
    interaction::{BlockUseResult, UseBlock},
};
use serde::{Deserialize, Serialize};

use crate::{
    DIRECTIONS, STONE, StateIds, collider,
    contraption::{
        BlockMovement, BlockMovementChecks, Contraption, ContraptionDisplays,
        MAX_ASSEMBLY_DATA_BYTES, StructureTransform,
    },
    kinetics::{FACES, Kind, Kinetics, Level, face_axis, offset, positive},
};

const ID: &str = "create:mechanical_bearing";
const WINDMILL: &str = "create:windmill_bearing";
const CLOCKWORK: &str = "create:clockwork_bearing";
const MINIMUM_SAIL_COUNT: i32 = 8;
const WINDMILL_SAILS_PER_RPM: i32 = 8;
const ROTATIONS: [(i16, i16); 6] = [(180, 0), (0, 0), (90, 0), (90, 180), (90, 270), (90, 90)];

pub fn register(content: Content, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let properties = DIRECTIONS.map(|face| format!("facing={face}"));
    for (id, display_name, model, item_model) in [
        (
            ID,
            "Mechanical Bearing",
            "create:block/mechanical_bearing",
            "create:item/mechanical_bearing",
        ),
        (
            WINDMILL,
            "Windmill Bearing",
            "create:block/windmill_bearing",
            "create:item/windmill_bearing",
        ),
        (
            CLOCKWORK,
            "Clockwork Bearing",
            "create:block/clockwork_bearing",
            "create:item/clockwork_bearing",
        ),
    ] {
        let states = std::array::from_fn::<_, 6, _>(|i| {
            let (x, y) = ROTATIONS[i];
            State {
                properties: &properties[i],
                physical: STONE,
                model,
                rotating_model: "",
                x,
                y,
                orientation: crate::relay_orientation(face_axis(i as u8)),
                shape: &[],
            }
        });
        content.add_block(
            registry,
            &Block {
                id,
                display_name,
                entity: id,
                item_model,
                states: &states,
            },
            Some(crate::placed_state),
        )?;
    }
    Ok(())
}

pub fn add_states(content: Content, states: &mut StateIds) {
    for (face, direction) in DIRECTIONS.iter().enumerate() {
        let properties = format!("facing={direction}");
        states.add(content, ID, &properties, Some(Kind::Bearing(face as u8)));
        states.add(
            content,
            WINDMILL,
            &properties,
            Some(Kind::Windmill(face as u8)),
        );
        states.add(
            content,
            CLOCKWORK,
            &properties,
            Some(Kind::Clockwork(face as u8)),
        );
    }
}

#[derive(Clone, Debug, Default)]
pub struct MechanicalBearing {
    pub data: BearingData,
    pub(crate) counter_clockwise: bool,
    displays: ContraptionDisplays,
    minute_displays: ContraptionDisplays,
    assemble_next_tick: bool,
    previous_speed: f32,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BearingData {
    running: bool,
    angle: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    contraption: Option<Contraption>,
    #[serde(skip_serializing_if = "Option::is_none")]
    clockwork: Option<ClockworkData>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "PascalCase")]
struct ClockworkData {
    minute_angle: f32,
    scroll_value: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    minute_hand: Option<Contraption>,
}

impl ClockworkData {
    fn assemble(
        world: World<'_>,
        checks: &BlockMovementChecks,
        controller: Pos,
        facing: u8,
        kinetics: &Kinetics,
        hour_hand: &Contraption,
    ) -> Option<Self> {
        let step = FACES[usize::from(facing)];
        let mut ignored: std::collections::HashSet<_> =
            hour_hand.blocks.positions(hour_hand.anchor()?).collect();
        ignored.insert(controller);
        let mut data = Self::default();
        for distance in 1..16 {
            let previous = offset(controller, step.map(|d| d * distance));
            let next = offset(previous, step);
            if !ignored.contains(&next) {
                if checks.classify(world, next)? != BlockMovement::Air {
                    data.minute_hand =
                        Some(checks.scan_ignoring(world, previous, facing, kinetics, &ignored)?);
                }
                break;
            }
        }
        Some(data)
    }

    fn targets(&self, time: DayTime, facing: u8) -> (f32, f32) {
        let ticks = (time.ticks % 24000) * if time.fixed_time { 24 } else { 1 } % 24000;
        let hours = (ticks / 1000 + 6) % 24;
        let minutes = (ticks % 1000) * 60 / 1000;
        let sign = if positive(facing) { -1.0 } else { 1.0 };
        let hour_12 = sign * 30.0 * (hours % 12) as f32;
        let minute = sign * 6.0 * minutes as f32;
        match self.scroll_value {
            1 => (minute, hour_12),
            2 => (sign * 15.0 * hours as f32, minute),
            _ => (hour_12, minute),
        }
    }
}

fn clockwork_angle(angle: f32, target: f32, speed: f32) -> f32 {
    let mut delta = (target - angle) % 360.0;
    if delta > 180.0 {
        delta -= 360.0;
    } else if delta < -180.0 {
        delta += 360.0;
    }
    (angle + delta.clamp(-speed, speed)) % 360.0
}

impl MechanicalBearing {
    pub fn get_generated_speed(&self) -> i32 {
        self.data.contraption.as_ref().map_or(0, |contraption| {
            let speed = (contraption.sails / WINDMILL_SAILS_PER_RPM).clamp(1, 16);
            if self.counter_clockwise {
                -speed
            } else {
                speed
            }
        })
    }

    fn assemble(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        pos: Pos,
        facing: u8,
        kind: Kind,
        kinetics: &mut Kinetics,
    ) -> bool {
        let Some(contraption) = blocks.scan(world, pos, facing, kinetics) else {
            return false;
        };
        if matches!(kind, Kind::Windmill(_)) && contraption.sails < MINIMUM_SAIL_COUNT {
            return false;
        }
        let Some(origin) = contraption.anchor() else {
            return false;
        };
        let Some(removals) = contraption.blocks.removals(origin, blocks) else {
            return false;
        };
        let mut remove: Vec<_> = removals.collect();
        let clockwork = if matches!(kind, Kind::Clockwork(_)) {
            let Some(mut clockwork) =
                ClockworkData::assemble(world, blocks, pos, facing, kinetics, &contraption)
            else {
                return false;
            };
            if let Some(current) = &self.data.clockwork {
                clockwork.scroll_value = current.scroll_value;
            }
            if let Some(minute) = &clockwork.minute_hand {
                let Some(removals) = minute
                    .anchor()
                    .and_then(|pos| minute.blocks.removals(pos, blocks))
                else {
                    return false;
                };
                remove.extend(removals);
            }
            Some(clockwork)
        } else {
            None
        };
        let mut saved = BearingData {
            running: true,
            angle: 0.0,
            contraption: Some(contraption),
            clockwork,
        };
        if !fastnbt::to_bytes(&saved).is_ok_and(|bytes| bytes.len() <= MAX_ASSEMBLY_DATA_BYTES)
            || world.write(&remove) != remove.len()
        {
            return false;
        }
        if let Some(contraption) = &mut saved.contraption {
            contraption.blocks.detach_glue(world);
        }
        if let Some(minute) = saved
            .clockwork
            .as_mut()
            .and_then(|clock| clock.minute_hand.as_mut())
        {
            minute.blocks.detach_glue(world);
        }
        crate::contraption::detach_moved_belts(world, &remove, kinetics);
        self.data = saved;
        true
    }

    pub(crate) fn disassemble(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        axis: u8,
    ) -> bool {
        let Some(contraption) = &self.data.contraption else {
            return false;
        };
        let Some(anchor) = contraption.anchor() else {
            return false;
        };
        let transform = StructureTransform {
            anchor,
            axis,
            angle: if self.data.clockwork.is_some() {
                0.0
            } else {
                self.data.angle
            },
        };
        let previous = collider::Frame::rotating(anchor, axis, self.data.angle);
        if let Some(clock) = &self.data.clockwork
            && let Some(minute) = &clock.minute_hand
        {
            let Some(minute_anchor) = minute.anchor() else {
                return false;
            };
            let mut both_hands = contraption.blocks.clone();
            both_hands.append_at(anchor, minute_anchor, &minute.blocks);
            if !both_hands.place_structure(world, blocks, transform, None) {
                return false;
            }
            contraption.blocks.settle_seats(world, transform, previous);
            minute.blocks.settle_seats(
                world,
                StructureTransform {
                    anchor: minute_anchor,
                    ..transform
                },
                collider::Frame::rotating(minute_anchor, axis, clock.minute_angle),
            );
        } else if !contraption
            .blocks
            .place(world, blocks, transform, None, previous)
        {
            return false;
        }
        self.clear_displays(world);
        let clockwork = self.data.clockwork.take().map(|clockwork| ClockworkData {
            scroll_value: clockwork.scroll_value,
            ..ClockworkData::default()
        });
        self.data = BearingData::default();
        self.data.clockwork = clockwork;
        self.assemble_next_tick = false;
        true
    }

    pub fn clear_displays(&mut self, world: World<'_>) {
        self.displays.clear(world);
        self.minute_displays.clear(world);
    }

    fn show(&mut self, world: World<'_>, axis: u8) {
        let Some(contraption) = &self.data.contraption else {
            return;
        };
        if let Some(anchor) = contraption.anchor() {
            self.displays.show(
                world,
                &contraption.blocks,
                StructureTransform {
                    anchor,
                    axis,
                    angle: self.data.angle,
                },
                [0.0; 3],
            );
        }
    }

    fn tick_clockwork(&mut self, world: World<'_>, facing: u8, speed: f32) -> bool {
        let Some(clock) = &mut self.data.clockwork else {
            return false;
        };
        let Some(time) = world.day_time() else {
            return false;
        };
        let (hour_target, minute_target) = clock.targets(time, facing);
        let speed = (speed * 3.0 / 10.0).abs();
        let axis = face_axis(facing);
        let hour_before = self.data.angle;
        let minute_before = clock.minute_angle;
        self.data.angle = clockwork_angle(hour_before, hour_target, speed / 2.0);
        clock.minute_angle = clockwork_angle(minute_before, minute_target, speed);
        let mut dirty = false;
        for (hand, before, angle, displays) in [
            (
                self.data.contraption.as_mut(),
                hour_before,
                &mut self.data.angle,
                &mut self.displays,
            ),
            (
                clock.minute_hand.as_mut(),
                minute_before,
                &mut clock.minute_angle,
                &mut self.minute_displays,
            ),
        ] {
            if let Some(hand) = hand
                && let Some(anchor) = hand.anchor()
            {
                let stalled = hand.blocks.tick_breakers(
                    world,
                    collider::Frame::rotating(anchor, axis, before),
                    collider::Frame::rotating(anchor, axis, *angle),
                );
                if stalled {
                    *angle = before;
                }
                dirty |= stalled || *angle != before;
                collider::collide_rotating(world, &mut hand.blocks, anchor, axis, before, *angle);
                displays.show(
                    world,
                    &hand.blocks,
                    StructureTransform {
                        anchor,
                        axis,
                        angle: *angle,
                    },
                    [0.0; 3],
                );
            }
        }
        dirty
    }

    fn tick(
        &mut self,
        world: World<'_>,
        blocks: &BlockMovementChecks,
        pos: Pos,
        kind: Kind,
        speed: f32,
        kinetics: &mut Kinetics,
    ) -> bool {
        let used = self
            .data
            .contraption
            .as_mut()
            .is_some_and(|body| self.displays.apply_clicks(world, &mut body.blocks));
        let (facing, windmill) = match kind {
            Kind::Bearing(facing) => (facing, false),
            Kind::Windmill(facing) => (facing, true),
            Kind::Clockwork(facing) => {
                let minute_used = self
                    .data
                    .clockwork
                    .as_mut()
                    .and_then(|clock| clock.minute_hand.as_mut())
                    .is_some_and(|body| self.minute_displays.apply_clicks(world, &mut body.blocks));
                let previous = std::mem::replace(&mut self.previous_speed, speed);
                if std::mem::take(&mut self.assemble_next_tick) || previous != speed {
                    if !self.data.running {
                        return self.assemble(world, blocks, pos, facing, kind, kinetics);
                    }
                    if speed == 0.0 && self.disassemble(world, blocks, face_axis(facing)) {
                        return true;
                    }
                    return used || minute_used;
                }
                let moved = self.data.running && self.tick_clockwork(world, facing, speed);
                return moved || used || minute_used;
            }
            _ => return false,
        };
        let previous_speed = std::mem::replace(&mut self.previous_speed, speed);
        let mut dirty = used;
        if previous_speed != speed {
            self.assemble_next_tick |= !windmill;
            if previous_speed != 0.0
                && previous_speed.signum() != speed.signum()
                && self.data.running
            {
                self.data.angle = (self.data.angle + 0.5).floor();
                dirty = true;
            }
        }
        if std::mem::take(&mut self.assemble_next_tick)
            && !self.data.running
            && (windmill || speed != 0.0)
        {
            dirty |= self.assemble(world, blocks, pos, facing, kind, kinetics);
        }
        if self.data.running {
            let axis = face_axis(facing);
            if !windmill && speed == 0.0 && self.disassemble(world, blocks, axis) {
                return true;
            }
            let speed = if windmill && speed != 0.0 {
                self.get_generated_speed() as f32
            } else {
                speed
            };
            let before = self.data.angle;
            let angle = (before + speed * 360.0 / 60.0 / 20.0) % 360.0;
            self.data.angle = angle;
            if let Some(contraption) = &mut self.data.contraption
                && let Some(anchor) = contraption.anchor()
            {
                if contraption.blocks.tick_breakers(
                    world,
                    collider::Frame::rotating(anchor, axis, before),
                    collider::Frame::rotating(anchor, axis, angle),
                ) {
                    self.data.angle = before;
                }
                collider::collide_rotating(
                    world,
                    &mut contraption.blocks,
                    anchor,
                    axis,
                    before,
                    self.data.angle,
                );
            }
            self.show(world, axis);
            dirty |= speed != 0.0;
        }
        dirty
    }
}

pub fn use_item_on(
    kinetics: &mut Kinetics,
    checks: &BlockMovementChecks,
    context: &mut UseBlock<'_>,
) -> BlockUseResult {
    if context.sneaking {
        return BlockUseResult::Fail;
    }
    if let Some(bytes) = context.stack.read() {
        if let Some(be) = kinetics.get_mut(context.position)
            && matches!(be.kind, Kind::Windmill(_))
            && context.player.is_some_and(|player| player.may_build())
            && let Some(bearing) = &mut be.bearing
            && fastnbt::from_bytes::<fastnbt::Value>(&bytes)
                .ok()
                .as_ref()
                .and_then(crate::processing_recipes::item_id)
                == Some(crate::WRENCH)
        {
            bearing.counter_clockwise = !bearing.counter_clockwise;
            be.dirty = true;
            context.action_bar(if bearing.counter_clockwise {
                "Windmill direction is counterclockwise"
            } else {
                "Windmill direction is clockwise"
            });
            return BlockUseResult::Success;
        }
        return BlockUseResult::TryEmptyHand;
    }
    let Some(be) = kinetics.get_mut(context.position) else {
        return BlockUseResult::Pass;
    };
    if matches!(be.kind, Kind::Clockwork(_))
        && !context.player.is_some_and(|player| player.may_build())
    {
        return BlockUseResult::Fail;
    }
    let Some(bearing) = &mut be.bearing else {
        return BlockUseResult::Pass;
    };
    if bearing.data.running {
        be.dirty |= bearing.disassemble(context.world, checks, be.kind.axis());
    } else {
        bearing.assemble_next_tick = true;
    }
    BlockUseResult::Success
}

pub fn tick(
    world: World<'_>,
    checks: &BlockMovementChecks,
    kinetics: &mut Kinetics,
    level: &mut dyn Level,
) {
    for (pos, axis, mut bearing) in kinetics.dropped_bearings.drain(..) {
        if !bearing.disassemble(world, checks, axis) {
            bearing.clear_displays(world);
            if let Some(contraption) = bearing.data.contraption {
                contraption.blocks.drop_contents(world, pos, |item| item);
            }
            if let Some(minute) = bearing.data.clockwork.and_then(|clock| clock.minute_hand) {
                minute.blocks.drop_contents(world, pos, |item| item);
            }
        }
    }
    for pos in kinetics.positions(Kind::is_bearing) {
        let Some(be) = kinetics.get_mut(pos) else {
            continue;
        };
        let (speed, kind) = (be.get_speed(), be.kind);
        let Some(mut bearing) = be.bearing.take() else {
            continue;
        };
        let dirty = bearing.tick(world, checks, pos, kind, speed, kinetics);
        let generated = bearing.get_generated_speed();
        if let Some(be) = kinetics.get_mut(pos) {
            be.bearing = Some(bearing);
            be.dirty |= dirty;
            if matches!(kind, Kind::Windmill(_)) && be.generated != generated {
                kinetics.set_generated(level, pos, generated);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clockwork_targets_follow_facing_mode_and_fixed_time() {
        let time = DayTime {
            ticks: 21_750,
            fixed_time: false,
        };
        let mut clock = ClockworkData::default();
        assert_eq!(clock.targets(time, 1), (-90.0, -270.0));
        clock.scroll_value = 1;
        assert_eq!(clock.targets(time, 0), (270.0, 90.0));
        clock.scroll_value = 2;
        assert_eq!(clock.targets(time, 1), (-45.0, -270.0));
        assert_eq!(
            clock.targets(
                DayTime {
                    ticks: 21_500,
                    fixed_time: true
                },
                1
            ),
            (-270.0, 0.0)
        );
    }

    #[test]
    fn clockwork_takes_the_short_path_and_stops_at_the_target() {
        assert_eq!(clockwork_angle(350.0, -5.0, 2.25), 352.25);
        assert_eq!(clockwork_angle(350.0, -9.0, 2.25), 351.0);
    }
}
