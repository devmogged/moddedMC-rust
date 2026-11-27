//! Atroxiia's 0.9.88 ground and water combat.
use moddedmc_api::{
    Pos, RegistrationError, Registry,
    content::{BlockUpdate, UPDATE_ALL, World},
    effects::SOUND_NEUTRAL,
    entities::{
        Entity, EntityBody, EntityChange, EntityView, SET_BODY_YAW, SET_HEAD_YAW, SET_MOTION,
        SET_YAW,
    },
    mob_ai::{Goal, goal_control, goal_phase},
    mobs::{PLAYER_INPUT_JUMP, PLAYER_INPUT_SPRINT},
    player_actions::{ACTION_HELD, ACTION_PRESSED, ACTION_RELEASED},
    tick_count,
};
use serde::{Deserialize, Serialize};

use crate::{CONTENT, EFFECTS, ENTITIES, MOB_AI, MOBS, abilities::enemy, taming};

pub const TARGETING: u64 = 100;
pub const COMBAT: u64 = 101;
const DATA: &str = "saintsdragons:atroxiia_combat";
const KIND: &str = "saintsdragons:atroxiia";
const FOLLOW: f64 = 32.0;
const FREEZE_THRESHOLD: i32 = 140;
const COMBO_WINDOW: i64 = 100;
const RIDER_DATA: &str = "saintsdragons:atroxiia_rider";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[repr(usize)]
enum Attack {
    Sweep,
    Quake,
    Gungnir,
    Precise,
    Slam,
    Swipe,
    Bite,
}

impl Attack {
    // DragonAbility starts its cooldown when the ability ends, unlike AI pacing.
    fn rider_cooldown(self) -> i64 {
        match self {
            Self::Sweep => 18,
            Self::Quake => 50,
            Self::Gungnir | Self::Precise => 30,
            Self::Slam | Self::Bite => 12,
            Self::Swipe => 8,
        }
    }
    fn pacing(self) -> (i64, i64, i64, i64) {
        // duration, cadence, per-ability cooldown, major cooldown
        match self {
            Self::Sweep => (26, 32, 240, 120),
            Self::Quake => (39, 50, 600, 240),
            Self::Gungnir => (37, 45, 400, 200),
            Self::Precise => (109, 110, 360, 180),
            Self::Slam => (25, 28, 32, 0),
            Self::Swipe => (20, 22, 24, 0),
            Self::Bite => (19, 24, 24, 0),
        }
    }
    fn clip(self, right: bool, elapsed: i64) -> &'static str {
        match self {
            Self::Sweep => "devastating_sweep",
            Self::Quake if elapsed >= 22 => "helheim_quake2",
            Self::Quake => "helheim_quake1",
            Self::Gungnir => "gungnir_stab",
            Self::Precise => "precise_strike",
            Self::Slam if right => "slam_right",
            Self::Slam => "slam_left",
            Self::Swipe if right => "swipe_right",
            Self::Swipe => "swipe_left",
            Self::Bite => "underwater_bite",
        }
    }
    fn cue(self, elapsed: i64) -> &'static str {
        match self {
            Self::Slam => "slam",
            Self::Swipe => "swipe",
            _ => self.clip(false, elapsed),
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct State {
    #[serde(skip_serializing_if = "Option::is_none")]
    active: Option<Attack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<Attack>,
    started: i64,
    cadence: i64,
    major: i64,
    cooldown: [i64; 7],
    decision: i64,
    kite: i32,
    right: bool,
    yaw: f32,
    last_hurt: i32,
    owner_hurt: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    combo_target: Option<[i8; 16]>,
    combo_until: i64,
}

fn now() -> i64 {
    tick_count() as i64
}
impl State {
    fn load(mob: Entity<'_>) -> Self {
        let mut s: Self = mob
            .data(DATA)
            .and_then(|v| fastnbt::from_bytes(&v).ok())
            .unwrap_or_default();
        // The host's tick counter starts again at boot; transient abilities must not resume.
        if s.started > now() || s.decision > now() + 6 {
            s = Self {
                last: s.last,
                ..Self::default()
            };
        }
        s
    }
    fn save(&self, mob: Entity<'_>) {
        if let Ok(bytes) = fastnbt::to_bytes(self) {
            mob.set_data(DATA, &bytes);
        }
    }
    fn ready(&self, attack: Attack, time: i64) -> bool {
        time >= self.cadence
            && time >= self.cooldown[attack as usize]
            && (attack.pacing().3 == 0 || time >= self.major)
    }
    fn begin(&mut self, attack: Attack, time: i64, yaw: f32) {
        let (_, cadence, cooldown, major) = attack.pacing();
        self.active = Some(attack);
        self.last = Some(attack);
        self.started = time;
        self.cadence = time + cadence;
        self.cooldown[attack as usize] = time + cooldown;
        if major != 0 {
            self.major = time + major;
        }
        self.right = !self.right;
        self.yaw = yaw;
    }
    fn clear_combo(&mut self) {
        self.combo_target = None;
        self.combo_until = 0;
    }
    fn reserve_combo(&mut self, target: Option<[i8; 16]>) {
        self.combo_target = target;
        self.combo_until = 0;
    }
    /// AtroxiiaGroundCombatBehaviour.armGungnirComboAfterQuake.
    fn arm_combo(&mut self, target: Option<[i8; 16]>, frozen: bool, time: i64) {
        if self.combo_target.is_some() && self.combo_until == 0 {
            if self.combo_target == target && frozen {
                self.combo_until = time + COMBO_WINDOW - 1;
            } else {
                self.clear_combo();
            }
        }
    }
    /// AtroxiiaGroundCombatBehaviour.handleGungnirCombo. A reserved follow-up
    /// ignores the global major cooldown, but obeys cadence and its own cooldown.
    fn combo_action(
        &mut self,
        target: Option<[i8; 16]>,
        time: i64,
        sight: bool,
        can_start: bool,
        snapshot: &Snapshot,
    ) -> Option<Option<Attack>> {
        self.combo_target?;
        if self.combo_target != target || !snapshot.frozen || time >= self.combo_until {
            self.clear_combo();
            return None;
        }
        if !sight || !snapshot.grounded {
            return Some(None);
        }
        if snapshot.gap <= 4.0 {
            self.clear_combo();
            return None;
        }
        if snapshot.gap > 8.0 || snapshot.facing < 0.82 {
            return Some(None);
        }
        if !snapshot.clear_lane {
            self.clear_combo();
            return None;
        }
        if !can_start || time < self.cadence || time < self.cooldown[Attack::Gungnir as usize] {
            return Some(None);
        }
        self.clear_combo();
        Some(Some(Attack::Gungnir))
    }
}

pub fn register(registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
    let ai = MOB_AI.get().ok_or(RegistrationError::InvalidDefinition)?;
    for (key, priority, controls) in [
        (TARGETING, 1, 0),
        (COMBAT, 1, goal_control::MOVE | goal_control::LOOK),
    ] {
        ai.add_goal(
            registry,
            &Goal {
                kind: KIND,
                key,
                priority,
                controls,
            },
        )?;
    }
    Ok(())
}

fn eligible(mob: Entity<'_>) -> bool {
    !crate::eggs::is_juvenile(mob)
        && taming::order_name(mob) != "stay"
        && mob.vehicle().is_none()
        && MOBS.get().is_some_and(|m| {
            m.passengers(mob).is_empty()
                && (taming::is_tamed(mob) || m.health(mob).is_some_and(|(hp, _)| hp > 60.0))
        })
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn length(v: [f64; 3]) -> f64 {
    v[0].hypot(v[1]).hypot(v[2])
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn forward(yaw: f32) -> [f64; 3] {
    let (s, c) = f64::from(yaw).to_radians().sin_cos();
    [-s, 0.0, c]
}
fn horizontal(v: [f64; 3], fallback: [f64; 3]) -> [f64; 3] {
    let n = v[0].hypot(v[2]);
    if n < 1e-6 {
        fallback
    } else {
        [v[0] / n, 0.0, v[2] / n]
    }
}
fn width(b: EntityBody) -> f64 {
    b.max[0] - b.min[0]
}
fn gap(a: EntityView, b: EntityView, aw: f64, bw: f64) -> f64 {
    (length(sub(a.position, b.position)) - (aw + bw) * 0.5).max(0.0)
}
fn wet(world: World<'_>, position: [f64; 3]) -> bool {
    crate::eggs::water_at(world, crate::flight::cell(position))
}
fn sight(world: World<'_>, mob: Entity<'_>, a: EntityBody, b: EntityBody) -> bool {
    // shortcut: eye heights are not exposed; approximate them from collision boxes.
    let eye = |body: EntityBody| {
        [
            (body.min[0] + body.max[0]) * 0.5,
            body.min[1] + (body.max[1] - body.min[1]) * 0.85,
            (body.min[2] + body.max[2]) * 0.5,
        ]
    };
    world
        .raycast(eye(a), eye(b), Some(mob.id()))
        .is_some_and(|(block, _)| block.is_none())
}
fn clear_lane(world: World<'_>, mut body: EntityBody, direction: [f64; 3], gap: f64) -> bool {
    body.min = inflate(body.min, [0.05, 0.02, 0.05]);
    body.max = inflate(body.max, [-0.05, -0.02, -0.05]);
    let distance = (gap - 0.35).clamp(0.0, 8.0);
    (1..distance.ceil() as i32)
        .all(|step| crate::flight::clear_body(world, body, direction.map(|v| v * f64::from(step))))
}
fn locate(
    world: World<'_>,
    mob: Entity<'_>,
    id: i32,
) -> Option<(EntityView, EntityBody, (f32, f32))> {
    ENTITIES.get()?.with_entity(world, id, |e| {
        if !hostile(world, mob, e) {
            return None;
        }
        Some((e.view()?, e.body()?, MOBS.get()?.health(e)?))
    })?
}

/// DragonEntity.isAlly: village defenders and companions of the same owner.
fn hostile(world: World<'_>, mob: Entity<'_>, target: Entity<'_>) -> bool {
    enemy(world, mob, target)
        && !matches!(
            target.type_id().trim_start_matches("minecraft:"),
            "villager" | "wandering_trader" | "iron_golem"
        )
        && !taming::owner(mob).is_some_and(|owner| taming::owner(target) == Some(owner))
}

/// AtroxiiaTargetingBehaviour: peaceful by default, then owner defense and retaliation.
pub fn targeting(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    if phase != goal_phase::TICK {
        return matches!(phase, goal_phase::CAN_START | goal_phase::CONTINUE);
    }
    let (Some(ai), Some(mobs), Some(entities), Some(view)) =
        (MOB_AI.get(), MOBS.get(), ENTITIES.get(), mob.view())
    else {
        return false;
    };
    if !eligible(mob) {
        if ai.target(mob).is_some() {
            ai.set_target(world, mob, None);
        }
        return false;
    }
    let usable = |id| {
        locate(world, mob, id)
            .is_some_and(|(v, _, _)| length(sub(v.position, view.position)) <= FOLLOW)
    };
    if ai.target(mob).is_some_and(|id| !usable(id)) {
        ai.set_target(world, mob, None);
    }
    let mut state = State::load(mob);
    let owner_threat = taming::owner(mob)
        .and_then(|u| mobs.by_uuid(world, &u))
        .and_then(|id| {
            entities
                .with_entity(world, id, |owner| ai.last_hurt_by(owner))
                .flatten()
        });
    if let Some((id, stamp)) = owner_threat
        && stamp != state.owner_hurt
        && usable(id)
    {
        state.owner_hurt = stamp;
        state.save(mob);
        ai.set_target(world, mob, Some(id));
    } else if let Some((id, stamp)) = ai.last_hurt_by(mob)
        && stamp != state.last_hurt
        && usable(id)
    {
        state.last_hurt = stamp;
        state.save(mob);
        ai.set_target(world, mob, Some(id));
    }
    // shortcut: host has no last-attacked-entity query for owner offense.
    false
}

struct Snapshot {
    gap: f64,
    facing: f64,
    enemies: usize,
    health: f32,
    ratio: f32,
    kite: i32,
    retreat: f64,
    lateral: f64,
    grounded: bool,
    clear_lane: bool,
    frozen: bool,
}

/// AtroxiiaGroundCombatBehaviour.selectAction.
fn choose(s: &State, t: i64, x: &Snapshot) -> Option<Attack> {
    let mut best = None;
    let mut score = 0.0;
    let mut offer = |a, condition, value| {
        if condition && s.ready(a, t) && value > score {
            best = Some(a);
            score = value;
        }
    };
    let g = x.gap;
    let k = f64::from(x.kite);
    let n = x.enemies as f64;
    let quake_available =
        (x.enemies >= 3 || x.kite >= 50) && g <= 18.0 && s.ready(Attack::Quake, t);
    offer(Attack::Sweep, g <= 2.5, 150.0 + n * 12.0 + (2.5 - g) * 10.0);
    offer(
        Attack::Quake,
        quake_available,
        125.0 + n * 10.0 + (k * 0.4).min(30.0),
    );
    offer(
        Attack::Gungnir,
        !quake_available
            && x.enemies == 1
            && x.clear_lane
            && x.health >= 12.0
            && x.grounded
            && !x.frozen
            && g > 4.0
            && g <= 8.0
            && (x.retreat >= 0.04 || x.kite >= 12)
            && x.lateral <= 0.16
            && x.facing >= 0.82,
        112.0 + (k * 0.45).min(18.0) + x.retreat.max(0.0) * 40.0,
    );
    offer(
        Attack::Precise,
        x.enemies == 1
            && x.health >= 18.0
            && x.ratio >= 0.35
            && x.facing >= 0.42
            && (3.0..=6.5).contains(&g),
        96.0 + f64::from(x.ratio) * 20.0,
    );
    offer(
        Attack::Slam,
        x.facing >= 0.42 && g <= 2.75,
        82.0 + (2.75 - g) * 8.0,
    );
    offer(
        Attack::Swipe,
        x.facing >= 0.42 && g <= 6.0,
        58.0 + (6.0 - g) * 2.0,
    );
    best
}

fn sound(world: World<'_>, mob: Entity<'_>, attack: Attack, elapsed: i64) {
    if let (Some(effects), Some(view)) = (EFFECTS.get(), mob.view()) {
        let cue = if cfg!(feature = "original-assets") {
            format!("saintsdragons:atroxiia_{}", attack.cue(elapsed))
        } else {
            "minecraft:entity.ravager.attack".into()
        };
        effects.play_sound(
            world,
            view.position,
            &cue,
            SOUND_NEUTRAL,
            if attack == Attack::Quake { 1.5 } else { 1.0 },
            1.0,
        );
    }
}

pub fn clip(mob: Entity<'_>) -> Option<&'static str> {
    let s = State::load(mob);
    s.active.map(|a| a.clip(s.right, now() - s.started))
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(into = "i32", try_from = "i32")]
enum RiderAbility {
    Combat(Attack),
    Slither,
    Flex,
}

impl From<RiderAbility> for i32 {
    fn from(value: RiderAbility) -> Self {
        value.slot() as i32
    }
}

impl TryFrom<i32> for RiderAbility {
    type Error = &'static str;
    fn try_from(value: i32) -> Result<Self, Self::Error> {
        Ok(match value {
            0 => Self::Combat(Attack::Sweep),
            1 => Self::Combat(Attack::Quake),
            2 => Self::Combat(Attack::Gungnir),
            3 => Self::Combat(Attack::Precise),
            4 => Self::Combat(Attack::Slam),
            5 => Self::Combat(Attack::Swipe),
            6 => Self::Combat(Attack::Bite),
            7 => Self::Slither,
            8 => Self::Flex,
            _ => return Err("invalid Atroxiia rider ability"),
        })
    }
}

impl RiderAbility {
    fn slot(self) -> usize {
        match self {
            Self::Combat(a) => a as usize,
            Self::Slither => 7,
            Self::Flex => 8,
        }
    }
    fn cooldown(self) -> i64 {
        match self {
            Self::Combat(a) => a.rider_cooldown(),
            Self::Slither => 30,
            Self::Flex => 120,
        }
    }
    fn duration(self) -> i64 {
        match self {
            Self::Combat(Attack::Quake) => 34,
            Self::Combat(a) => a.pacing().0,
            Self::Slither => 75,
            Self::Flex => 67,
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Contact {
    id: i32,
    until: i64,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct RiderState {
    #[serde(skip_serializing_if = "Option::is_none")]
    active: Option<RiderAbility>,
    started: i64,
    stamp: i64,
    cooldown: [i64; 9],
    swipe: bool,
    right: bool,
    released: bool,
    chain_requested: bool,
    chain_at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tertiary: Option<i64>,
    contact: Vec<Contact>,
    ascend: bool,
    descend: bool,
    accelerate: bool,
}

impl RiderState {
    fn load(mob: Entity<'_>) -> Self {
        let state: Self = mob
            .data(RIDER_DATA)
            .and_then(|v| fastnbt::from_bytes(&v).ok())
            .unwrap_or_default();
        if state.stamp > now() {
            Self::default()
        } else {
            state
        }
    }
    fn save(&mut self, mob: Entity<'_>) {
        self.stamp = now();
        if let Ok(bytes) = fastnbt::to_bytes(self) {
            mob.set_data(RIDER_DATA, &bytes);
        }
    }
    fn begin(&mut self, ability: RiderAbility, time: i64) -> bool {
        if self.active.is_some() || time < self.cooldown[ability.slot()] {
            return false;
        }
        self.active = Some(ability);
        self.started = time;
        self.released = false;
        self.chain_requested = false;
        self.chain_at = 0;
        self.contact.clear();
        if matches!(ability, RiderAbility::Combat(Attack::Slam | Attack::Swipe)) {
            self.right = !self.right;
        }
        true
    }
    fn end(&mut self, time: i64) {
        if let Some(ability) = self.active.take() {
            self.cooldown[ability.slot()] = if ability == RiderAbility::Flex {
                self.started + ability.cooldown()
            } else {
                time + ability.cooldown()
            };
        }
        self.contact.clear();
    }
    fn quake_chain(&self, elapsed: i64) -> bool {
        self.chain_at == 0 && elapsed >= 22 && (!self.released || self.chain_requested)
    }
}

fn controlling(mob: Entity<'_>, player: Entity<'_>) -> bool {
    taming::is_owner(mob, player)
        && !crate::eggs::is_juvenile(mob)
        && MOBS
            .get()
            .is_some_and(|m| m.passengers(mob).first() == Some(&player.id()))
}

fn grounded(world: World<'_>, mob: Entity<'_>, v: EntityView) -> bool {
    !wet(world, v.position)
        && v.motion[1] <= 0.08
        && (v.on_ground
            || mob
                .body()
                .is_some_and(|b| !crate::flight::clear_body(world, b, [0.0, -0.12, 0.0])))
}

fn begin_rider(world: World<'_>, mob: Entity<'_>, s: &mut RiderState, a: RiderAbility) {
    let Some(v) = mob.view() else { return };
    let allowed = if a == RiderAbility::Combat(Attack::Bite) {
        wet(world, v.position)
    } else {
        grounded(world, mob, v)
    };
    if !allowed || !s.begin(a, now()) {
        return;
    }
    if let Some(ai) = MOB_AI.get() {
        ai.stop(mob);
        ai.set_target(world, mob, None);
    }
    match a {
        RiderAbility::Combat(attack) => sound(world, mob, attack, 0),
        RiderAbility::Slither | RiderAbility::Flex => {
            let cue = if a == RiderAbility::Slither {
                "slither"
            } else {
                "flex"
            };
            if let Some(effects) = EFFECTS.get() {
                let sound = if cfg!(feature = "original-assets") {
                    format!("saintsdragons:atroxiia_{cue}")
                } else {
                    "minecraft:entity.ravager.roar".into()
                };
                effects.play_sound(world, v.position, &sound, SOUND_NEUTRAL, 1.4, 1.0);
            }
        }
    }
}

/// Atroxiia's released rider bindings. Built-in attack and double-tap-W
/// callbacks are connected by the shared mouse-input task.
pub fn rider_action(
    world: World<'_>,
    player: Entity<'_>,
    mob: Entity<'_>,
    action: &str,
    phase: u8,
) {
    if !controlling(mob, player) {
        return;
    }
    let Some(v) = mob.view() else { return };
    let mut s = RiderState::load(mob);
    let t = now();
    let a = match (action, phase) {
        ("saintsdragons:ascend", ACTION_PRESSED | ACTION_RELEASED) => {
            s.ascend = phase == ACTION_PRESSED;
            None
        }
        ("saintsdragons:descend", ACTION_PRESSED | ACTION_RELEASED) => {
            s.descend = phase == ACTION_PRESSED;
            None
        }
        ("saintsdragons:accelerate", ACTION_PRESSED | ACTION_RELEASED) => {
            s.accelerate = phase == ACTION_PRESSED;
            None
        }
        ("saintsdragons:toggle_melee", ACTION_PRESSED)
            if !wet(world, v.position) && s.active.is_none() =>
        {
            s.swipe = !s.swipe;
            None
        }
        ("saintsdragons:ability_primary", ACTION_PRESSED) => {
            Some(RiderAbility::Combat(Attack::Sweep))
        }
        ("saintsdragons:ability_secondary", ACTION_PRESSED) => {
            if s.active == Some(RiderAbility::Combat(Attack::Quake)) && s.chain_at == 0 {
                s.chain_requested = true;
            }
            Some(RiderAbility::Combat(Attack::Quake))
        }
        ("saintsdragons:ability_secondary", ACTION_RELEASED) => {
            if s.active == Some(RiderAbility::Combat(Attack::Quake)) {
                s.released = true;
            }
            None
        }
        ("saintsdragons:ability_tertiary", ACTION_PRESSED) => {
            s.tertiary = Some(t);
            None
        }
        ("saintsdragons:ability_tertiary", ACTION_HELD)
            if s.tertiary.is_some_and(|start| t - start >= 4) =>
        {
            s.tertiary = None;
            Some(RiderAbility::Combat(Attack::Precise))
        }
        ("saintsdragons:ability_tertiary", ACTION_RELEASED) => s.tertiary.take().map(|start| {
            RiderAbility::Combat(if t - start >= 4 {
                Attack::Precise
            } else {
                Attack::Gungnir
            })
        }),
        ("saintsdragons:flex", ACTION_PRESSED) => Some(RiderAbility::Flex),
        ("moddedmc:mounted_attack", ACTION_PRESSED) => {
            Some(RiderAbility::Combat(if wet(world, v.position) {
                Attack::Bite
            } else if s.swipe {
                Attack::Swipe
            } else {
                Attack::Slam
            }))
        }
        ("moddedmc:double_tap/forward", ACTION_PRESSED) => Some(RiderAbility::Slither),
        _ => return,
    };
    if let Some(a) = a {
        begin_rider(world, mob, &mut s, a);
    }
    s.save(mob);
}

fn slither_contact(world: World<'_>, mob: Entity<'_>, s: &mut RiderState, t: i64, v: EntityView) {
    let (Some(entities), Some(ai), Some(body)) = (ENTITIES.get(), MOB_AI.get(), mob.body()) else {
        return;
    };
    s.contact.retain(|c| c.until > t);
    for id in entities.entities_in(
        world,
        inflate(body.min, [-0.75, -0.35, -0.75]),
        inflate(body.max, [0.75, 0.35, 0.75]),
        mob.id(),
    ) {
        if s.contact.iter().any(|c| c.id == id) {
            continue;
        }
        entities.with_entity(world, id, |target| {
            if hostile(world, mob, target) && ai.attack(world, mob, id, 5.0) {
                if let Some(at) = target.view() {
                    push(
                        target,
                        horizontal(sub(at.position, v.position), forward(v.yaw).map(|x| -x)),
                        0.35,
                        0.0,
                    );
                }
                s.contact.push(Contact { id, until: t + 10 });
            }
        });
    }
    if let Some(effects) = EFFECTS.get() {
        effects.particles(
            world,
            "minecraft:cloud",
            v.position,
            [0.75, 0.1, 0.75],
            0.08,
            2,
        );
    }
}

/// AtroxiiaRiderController: the ground dragon must not use Cindervane's flight
/// controls. Apply the ability movement after the ordinary rider movement.
pub fn ride(world: World<'_>, mob: Entity<'_>, rider: Entity<'_>) {
    let (Some(v), Some(look), Some(body), Some(mobs)) =
        (mob.view(), rider.view(), rider.body(), MOBS.get())
    else {
        return;
    };
    if !controlling(mob, rider) {
        return;
    }
    let s = RiderState::load(mob);
    let keys = mobs.player_input(rider).unwrap_or(0);
    let sprint = s.accelerate || keys & PLAYER_INPUT_SPRINT != 0;
    let yaw = v.yaw + ((look.yaw - v.yaw + 180.0).rem_euclid(360.0) - 180.0) * 0.28;
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    let forward = body.input[2] * if body.input[2] < 0.0 { 0.5 } else { 1.0 };
    let strafe = body.input[0];
    let motion = if wet(world, v.position) {
        let up = s.ascend || keys & PLAYER_INPUT_JUMP != 0;
        let down = s.descend;
        let pitch = f64::from(if up {
            -25.0
        } else if down {
            25.0
        } else {
            look.pitch
        })
        .to_radians();
        let dir = [
            -sin * pitch.cos() * forward + cos * strafe * 0.36,
            if up || down {
                0.0
            } else {
                -pitch.sin() * forward * 0.65
            },
            cos * pitch.cos() * forward + sin * strafe * 0.36,
        ];
        let norm = length(dir);
        let speed = if sprint { 0.42 } else { 0.30 };
        let desired = if norm > 1e-6 {
            dir.map(|d| d / norm * speed)
        } else {
            [0.0, v.motion[1] * 0.9, 0.0]
        };
        let mut motion = std::array::from_fn(|i| {
            (v.motion[i] + (desired[i] - v.motion[i]) * 0.28) * if i == 1 { 0.96 } else { 0.94 }
        });
        if up {
            motion[1] = (motion[1] + 0.10).min(0.36);
        } else if down {
            motion[1] = (motion[1] - 0.12).max(-0.36);
        }
        motion
    } else {
        let speed = if sprint { 0.43 } else { 0.12 };
        let speed = speed * crate::hunger::speed_multiplier(taming::happiness(mob).unwrap_or(100));
        [
            (-sin * forward * 0.9 + cos * strafe * 0.5) * speed,
            v.motion[1].min(0.0),
            (cos * forward * 0.9 + sin * strafe * 0.5) * speed,
        ]
    };
    let locked = matches!(
        s.active,
        Some(RiderAbility::Flex | RiderAbility::Combat(Attack::Quake))
    );
    let yaw = if locked { v.yaw } else { yaw };
    mob.change(&EntityChange {
        mask: SET_YAW | SET_BODY_YAW | SET_HEAD_YAW | SET_MOTION,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        motion: if locked {
            [0.0, motion[1], 0.0]
        } else {
            motion
        },
        ..Default::default()
    });
    rider.reset_fall_distance();
    mob.reset_fall_distance();
    rider_tick(world, mob);
}

/// Ambient animation and cleanup when the controlling passenger dismounts.
pub fn rider_clip(mob: Entity<'_>) -> Option<&'static str> {
    let mut s = RiderState::load(mob);
    if MOBS.get().is_none_or(|m| m.passengers(mob).is_empty()) {
        if s.active.is_some() || s.tertiary.is_some() || s.ascend || s.descend || s.accelerate {
            s.end(now());
            s.tertiary = None;
            s.ascend = false;
            s.descend = false;
            s.accelerate = false;
            s.save(mob);
        }
        return None;
    }
    match s.active? {
        RiderAbility::Combat(Attack::Quake) => Some(if s.chain_at == 0 {
            "helheim_quake1"
        } else {
            "helheim_quake2"
        }),
        RiderAbility::Combat(a) => Some(a.clip(s.right, now() - s.started)),
        RiderAbility::Slither => Some("slither"),
        RiderAbility::Flex => Some("flex"),
    }
}

/// Tick mounted abilities within the controlling rider's movement goal.
fn rider_tick(world: World<'_>, mob: Entity<'_>) -> Option<&'static str> {
    let mut s = RiderState::load(mob);
    let a = s.active?;
    let t = now();
    let elapsed = t - s.started;
    let v = mob.view()?;
    let mounted = MOBS
        .get()
        .and_then(|m| m.passengers(mob).first().copied())
        .is_some_and(|id| {
            ENTITIES
                .get()
                .and_then(|e| e.with_entity(world, id, |player| controlling(mob, player)))
                == Some(true)
        });
    let duration = if a == RiderAbility::Combat(Attack::Quake) && s.chain_at != 0 {
        s.chain_at + 17
    } else {
        a.duration()
    };
    if !mounted
        || wet(world, v.position) != (a == RiderAbility::Combat(Attack::Bite))
        || elapsed >= duration
    {
        s.end(t);
        s.tertiary = None;
        s.save(mob);
        if a == RiderAbility::Slither {
            nudge(mob, v.yaw, 0.0);
        }
        return None;
    }
    let clip = match a {
        RiderAbility::Combat(attack) => {
            if attack == Attack::Quake {
                nudge(mob, v.yaw, 0.0);
                if elapsed == 18 {
                    execute(world, mob, attack, 18, None, v.yaw);
                }
                if s.quake_chain(elapsed) {
                    s.chain_at = elapsed;
                    execute(world, mob, attack, 22, None, v.yaw);
                }
                if s.chain_at != 0 && elapsed == s.chain_at + 14 {
                    execute(world, mob, attack, 36, None, v.yaw);
                }
                if s.chain_at != 0 {
                    "helheim_quake2"
                } else {
                    "helheim_quake1"
                }
            } else {
                if attack == Attack::Bite && elapsed == 9 {
                    hit(
                        world,
                        mob,
                        attack,
                        Hit {
                            range: 5.0,
                            horizontal: 3.0,
                            vertical: 2.75,
                            angle: 95.0,
                            damage: 10.0,
                            push: 0.2,
                            lift: 0.05,
                            stun: 0,
                        },
                        false,
                    );
                } else {
                    execute(world, mob, attack, elapsed, None, v.yaw);
                }
                attack.clip(s.right, elapsed)
            }
        }
        RiderAbility::Slither => {
            match elapsed {
                7..=21 => nudge(mob, v.yaw, 24.0 / 15.0),
                22..=66 => {
                    nudge(mob, v.yaw, 0.43 * 4.0);
                    slither_contact(world, mob, &mut s, t, v);
                }
                _ => nudge(mob, v.yaw, 0.0),
            }
            "slither"
        }
        RiderAbility::Flex => {
            nudge(mob, v.yaw, 0.0);
            "flex"
        }
    };
    s.save(mob);
    Some(clip)
}

fn turn(mob: Entity<'_>, view: EntityView, delta: [f64; 3], degrees: f32) -> f32 {
    if delta[0].hypot(delta[2]) < 1e-6 {
        return view.yaw;
    }
    let wanted = delta[2].atan2(delta[0]).to_degrees() as f32 - 90.0;
    let yaw =
        view.yaw + ((wanted - view.yaw + 180.0).rem_euclid(360.0) - 180.0).clamp(-degrees, degrees);
    mob.change(&EntityChange {
        mask: SET_YAW | SET_BODY_YAW | SET_HEAD_YAW,
        yaw,
        body_yaw: yaw,
        head_yaw: yaw,
        ..Default::default()
    });
    yaw
}

pub fn goal(phase: u32, world: World<'_>, mob: Entity<'_>) -> bool {
    let (Some(ai), Some(view), Some(body)) = (MOB_AI.get(), mob.view(), mob.body()) else {
        return false;
    };
    let target = ai
        .target(mob)
        .and_then(|id| locate(world, mob, id).map(|t| (id, t)));
    let mut s = State::load(mob);
    match phase {
        goal_phase::CAN_START | goal_phase::CONTINUE => {
            eligible(mob) && (target.is_some() || s.active.is_some())
        }
        goal_phase::STOP => {
            s.active = None;
            s.kite = 0;
            s.clear_combo();
            s.save(mob);
            ai.stop(mob);
            false
        }
        goal_phase::TICK => {
            let t = now();
            let swimming = wet(world, view.position);
            if let Some(a) = s.active {
                let elapsed = t - s.started;
                if elapsed >= a.pacing().0 || swimming != (a == Attack::Bite) {
                    s.active = None;
                    s.save(mob);
                } else {
                    if a != Attack::Bite {
                        ai.stop(mob);
                    }
                    if a == Attack::Precise
                        && let Some((_, (v, _, _))) = target
                    {
                        turn(mob, view, sub(v.position, view.position), 10.0);
                    }
                    execute(world, mob, a, elapsed, target.map(|(id, _)| id), s.yaw);
                    return false;
                }
            }
            let Some((id, (at, target_body, (health, max)))) = target else {
                s.clear_combo();
                s.save(mob);
                return false;
            };
            let delta = sub(at.position, view.position);
            let g = gap(view, at, width(body), width(target_body));
            let visible = sight(world, mob, body, target_body);
            let (uuid, frozen) = ENTITIES
                .get()
                .and_then(|entities| {
                    entities.with_entity(world, id, |target| {
                        (
                            MOBS.get()
                                .and_then(|mobs| mobs.uuid(target))
                                .map(|uuid| uuid.map(|byte| byte as i8)),
                            target
                                .ticks_frozen()
                                .is_some_and(|ticks| ticks >= FREEZE_THRESHOLD),
                        )
                    })
                })
                .unwrap_or_default();
            if swimming {
                s.clear_combo();
            } else {
                s.arm_combo(uuid, frozen, t);
            }
            ai.look_at(mob, at.position);
            let yaw = turn(mob, view, delta, if swimming { 8.0 } else { 12.0 });
            if swimming {
                ai.stop(mob);
                let norm = length(delta).max(0.01);
                mob.change(&EntityChange {
                    mask: SET_MOTION,
                    motion: delta.map(|v| v / norm * 0.30),
                    ..Default::default()
                });
            } else if g > 6.0 || !visible {
                if t % 5 == 0 || !ai.navigating(mob) {
                    ai.walk_to(mob, at.position, 1.45);
                }
            } else {
                ai.stop(mob);
            }
            s.kite = if visible && g > 7.5 {
                (s.kite + 1).min(100)
            } else {
                (s.kite - 2).max(0)
            };
            let dir = horizontal(delta, forward(yaw));
            let retreat = dot(dir, at.motion);
            let lateral = (at.motion[0] - dir[0] * retreat).hypot(at.motion[2] - dir[2] * retreat);
            let facing = dot(dir, forward(yaw));
            let mut snapshot = Snapshot {
                gap: g,
                facing,
                enemies: 1,
                health,
                ratio: health / max.max(1.0),
                kite: s.kite,
                retreat,
                lateral,
                grounded: !wet(world, at.position)
                    && at.motion[1] <= 0.08
                    && (at.on_ground
                        || !crate::flight::clear_body(world, target_body, [0.0, -0.12, 0.0])),
                clear_lane: false,
                frozen,
            };
            if !swimming && visible && (4.0..=8.0).contains(&g) {
                // shortcut: clear_body treats any occupied collision cell as solid.
                snapshot.clear_lane = clear_lane(world, body, dir, g);
            }
            let combo = s.combo_action(uuid, t, visible, view.on_ground, &snapshot);
            let attack = if let Some(action) = combo {
                action
            } else if !visible || t < s.decision || (!swimming && !view.on_ground) {
                None
            } else {
                s.decision = t + 6;
                if swimming {
                    (g <= 5.0 && s.ready(Attack::Bite, t)).then_some(Attack::Bite)
                } else {
                    snapshot.enemies = engaged(world, mob, id, body);
                    choose(&s, t, &snapshot)
                }
            };
            if let Some(a) = attack {
                s.begin(a, t, yaw);
                if a == Attack::Quake {
                    s.reserve_combo(uuid);
                }
                sound(world, mob, a, 0);
                if a != Attack::Bite {
                    ai.stop(mob);
                }
            }
            s.save(mob);
            false
        }
        _ => false,
    }
}

fn engaged(world: World<'_>, mob: Entity<'_>, primary: i32, body: EntityBody) -> usize {
    let (Some(entities), Some(ai)) = (ENTITIES.get(), MOB_AI.get()) else {
        return 1;
    };
    entities
        .entities_in(
            world,
            inflate(body.min, [-18.0, -6.0, -18.0]),
            inflate(body.max, [18.0, 6.0, 18.0]),
            mob.id(),
        )
        .into_iter()
        .filter(|&id| {
            entities
                .with_entity(world, id, |e| {
                    hostile(world, mob, e)
                        && (id == primary
                            || ai.target(e) == Some(mob.id())
                            || ai
                                .last_hurt_by(e)
                                .is_some_and(|(attacker, _)| attacker == mob.id()))
                })
                .unwrap_or(false)
        })
        .count()
        .max(1)
}
fn inflate(v: [f64; 3], by: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| v[i] + by[i])
}

fn push(target: Entity<'_>, direction: [f64; 3], strength: f64, lift: f64) {
    if let Some(v) = target.view() {
        target.change(&EntityChange {
            mask: SET_MOTION,
            motion: inflate(
                v.motion,
                [direction[0] * strength, lift, direction[2] * strength],
            ),
            ..Default::default()
        });
    }
}

fn frost_ticks(current: i32, duration: i32) -> i32 {
    current.max(FREEZE_THRESHOLD + duration.max(1) * 2)
}

/// AtroxiiaFrostImpact.apply.
fn frost(world: World<'_>, target: Entity<'_>, ticks: i32, particles: bool) {
    let ticks = ticks.max(1);
    target.add_effect("minecraft:slowness", ticks, 6, false, true);
    target.add_effect("minecraft:weakness", ticks, 1, false, true);
    // shortcut: the host does not expose canFreeze and currently clamps the
    // counter to 140; the release's excess freeze duration needs engine support.
    if let Some(current) = target.ticks_frozen() {
        target.set_ticks_frozen(frost_ticks(current, ticks));
    }
    if particles && let (Some(e), Some(v)) = (EFFECTS.get(), target.view()) {
        e.particles(
            world,
            "minecraft:snowflake",
            inflate(v.position, [0.0, 1.0, 0.0]),
            [0.4; 3],
            0.2,
            28,
        );
    }
    if MOBS.get().and_then(|m| m.kind(target)) == Some(crate::CINDERVANE) {
        crate::combat::suppress_fire(target);
    }
}

fn forward_hit(
    origin: [f64; 3],
    dir: [f64; 3],
    body: EntityBody,
    range: f64,
    inflate: f64,
    close: f64,
    angle: f64,
) -> bool {
    let nearest = std::array::from_fn(|i| origin[i].clamp(body.min[i], body.max[i]));
    let delta = sub(nearest, origin);
    let dist = length(delta);
    if dist > range + inflate {
        return false;
    }
    if dist <= close {
        return true;
    }
    let cosine = dot(delta, dir) / dist;
    cosine > 0.0 && (cosine >= angle.to_radians().cos() || dist < range * 0.35)
}

#[derive(Clone, Copy)]
struct Hit {
    range: f64,
    horizontal: f64,
    vertical: f64,
    angle: f64,
    damage: f32,
    push: f64,
    lift: f64,
    stun: i32,
}
fn hit(world: World<'_>, mob: Entity<'_>, attack: Attack, spec: Hit, pull: bool) {
    let (Some(entities), Some(ai), Some(v), Some(body)) =
        (ENTITIES.get(), MOB_AI.get(), mob.view(), mob.body())
    else {
        return;
    };
    let radial = matches!(attack, Attack::Sweep | Attack::Quake);
    let dir = forward(v.yaw);
    let offset = width(body) * 0.35 + if attack == Attack::Gungnir { 1.0 } else { 0.0 };
    let origin = inflate(
        v.position,
        [
            dir[0] * offset,
            (body.max[1] - body.min[1]) * 0.52,
            dir[2] * offset,
        ],
    );
    let end = inflate(origin, dir.map(|d| d * spec.range));
    let (min, max) = if radial {
        (
            inflate(body.min, [-spec.range, -spec.vertical, -spec.range]),
            inflate(body.max, [spec.range, spec.vertical, spec.range]),
        )
    } else {
        (
            std::array::from_fn(|i| {
                origin[i].min(end[i])
                    - if i == 1 {
                        spec.vertical
                    } else {
                        spec.horizontal
                    }
            }),
            std::array::from_fn(|i| {
                origin[i].max(end[i])
                    + if i == 1 {
                        spec.vertical
                    } else {
                        spec.horizontal
                    }
            }),
        )
    };
    for id in entities.entities_in(world, min, max, mob.id()) {
        entities.with_entity(world, id, |target| {
            let (Some(at), Some(b)) = (target.view(), target.body()) else {
                return;
            };
            if !hostile(world, mob, target) {
                return;
            }
            let delta = sub(at.position, v.position);
            let valid = match attack {
                Attack::Sweep => delta[0].hypot(delta[2]) <= spec.range,
                Attack::Quake => length(delta) <= spec.range,
                Attack::Gungnir => true,
                _ => forward_hit(
                    origin,
                    dir,
                    b,
                    spec.range,
                    spec.horizontal,
                    spec.range * 0.45,
                    spec.angle,
                ),
            };
            if !valid {
                return;
            }
            if pull {
                push(
                    target,
                    horizontal(sub(v.position, at.position), dir),
                    1.0,
                    0.12,
                );
                return;
            }
            let damaged = if attack == Attack::Gungnir {
                target.hurt("minecraft:magic", spec.damage)
            } else {
                ai.attack(
                    world,
                    mob,
                    id,
                    spec.damage * crate::hunger::melee_multiplier(mob),
                )
            };
            if attack == Attack::Quake && !damaged {
                return;
            }
            if spec.stun > 0 {
                frost(world, target, spec.stun, attack != Attack::Quake);
            }
            if spec.push != 0.0 {
                push(
                    target,
                    if radial { horizontal(delta, dir) } else { dir },
                    spec.push,
                    spec.lift,
                );
            }
        });
    }
}

/// AtroxiiaFrostWalker.freezeNearbyWater: source water exposed to air within radius 12,
/// scanning from the dragon's feet up to ten blocks down.
fn freeze_water(world: World<'_>, at: [f64; 3]) {
    let Some(content) = CONTENT.get() else { return };
    let (Some(water), Some(air), Some(ice)) = (
        content.state("minecraft:water[level=0]"),
        content.state("minecraft:air"),
        content.state("minecraft:ice"),
    ) else {
        return;
    };
    let c = crate::flight::cell(at);
    let mut positions = Vec::new();
    for x in -12..=12 {
        for z in -12..=12 {
            if x * x + z * z > 144 {
                continue;
            }
            for y in -10..=1 {
                positions.push(Pos {
                    x: c.x + x,
                    y: c.y + y,
                    z: c.z + z,
                });
            }
        }
    }
    let mut states = vec![0; positions.len()];
    if !world.read(&positions, &mut states) {
        return;
    }
    let mut updates = Vec::new();
    for (ps, ss) in positions
        .as_chunks::<12>()
        .0
        .iter()
        .zip(states.as_chunks::<12>().0)
    {
        if let Some(i) = (0..11).rev().find(|&i| ss[i] == water && ss[i + 1] == air) {
            updates.push(BlockUpdate {
                position: ps[i],
                state: ice,
                flags: UPDATE_ALL,
            });
        }
    }
    world.write(&updates);
}

fn nudge(mob: Entity<'_>, yaw: f32, speed: f64) {
    if let Some(v) = mob.view() {
        let d = forward(yaw);
        mob.change(&EntityChange {
            mask: SET_MOTION,
            motion: [d[0] * speed, v.motion[1], d[2] * speed],
            ..Default::default()
        });
    }
}

/// Ability tracks from the seven Atroxiia ability classes. Slither is rider-only.
fn execute(world: World<'_>, mob: Entity<'_>, a: Attack, t: i64, target: Option<i32>, yaw: f32) {
    if a == Attack::Quake {
        crate::particles::quake(world, mob, t);
    }
    let spec = match (a, t) {
        (Attack::Slam, 10) => Hit {
            range: 5.5,
            horizontal: 4.0,
            vertical: 4.0,
            angle: 100.0,
            damage: 16.0,
            push: 0.45,
            lift: 0.18,
            stun: 0,
        },
        (Attack::Swipe, 13) => Hit {
            range: 6.0,
            horizontal: 4.5,
            vertical: 3.5,
            angle: 115.0,
            damage: 12.0,
            push: 0.3,
            lift: 0.1,
            stun: 0,
        },
        (Attack::Sweep, 10) => Hit {
            range: 12.0,
            horizontal: 12.0,
            vertical: 6.0,
            angle: 180.0,
            damage: 13.0,
            push: 1.65,
            lift: 0.32,
            stun: 0,
        },
        (Attack::Precise, 11 | 25 | 45 | 64) => Hit {
            range: 8.5,
            horizontal: 10.5,
            vertical: 10.5,
            angle: 100.0,
            damage: 9.0,
            push: 0.75,
            lift: 0.16,
            stun: (148 - t) as i32,
        },
        (Attack::Gungnir, 17) => Hit {
            range: 20.0,
            horizontal: 3.0,
            vertical: 4.0,
            angle: 180.0,
            damage: 40.0,
            push: 0.0,
            lift: 0.0,
            stun: 60,
        },
        (Attack::Quake, 18 | 36) => Hit {
            range: 20.0,
            horizontal: 20.0,
            vertical: 6.0,
            angle: 180.0,
            damage: 25.0,
            push: if t == 18 { 0.75 } else { 1.8 },
            lift: if t == 18 { 0.2 } else { 0.5 },
            stun: 100,
        },
        (Attack::Bite, 9) => {
            if let (Some(id), Some(ai), Some(v), Some(b)) =
                (target, MOB_AI.get(), mob.view(), mob.body())
                && locate(world, mob, id)
                    .is_some_and(|(at, body, _)| gap(v, at, width(b), width(body)) <= 5.0)
            {
                ai.attack(world, mob, id, 10.0 * crate::hunger::melee_multiplier(mob));
                if let Some(entities) = ENTITIES.get() {
                    entities.with_entity(world, id, |e| push(e, forward(v.yaw), 0.2, 0.05));
                }
            }
            return;
        }
        (Attack::Quake, 22) => {
            sound(world, mob, a, 22);
            return;
        }
        (Attack::Gungnir, 12..=15) => {
            nudge(mob, yaw, 0.5);
            return;
        }
        (Attack::Gungnir, 18..=23) => {
            nudge(mob, yaw, 8.0 / 7.0);
            return;
        }
        (Attack::Precise, 12..=15 | 39..=43 | 53..=57) => {
            if let Some(v) = mob.view() {
                nudge(mob, v.yaw, 1.0);
            }
            return;
        }
        _ => return,
    };
    if a == Attack::Quake
        && t == 18
        && let Some(v) = mob.view()
    {
        freeze_water(world, v.position);
    }
    if a == Attack::Gungnir
        && let Some(v) = mob.view()
    {
        mob.change(&EntityChange {
            mask: SET_YAW | SET_BODY_YAW,
            yaw,
            body_yaw: yaw,
            ..Default::default()
        });
        nudge(mob, v.yaw, 8.0 / 7.0);
    }
    if a == Attack::Precise
        && t == 11
        && let Some(v) = mob.view()
    {
        nudge(mob, v.yaw, 1.0);
    }
    hit(world, mob, a, spec, a == Attack::Precise && t == 25);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rider_quake_release_chain_and_cooldown() {
        let mut s = RiderState::default();
        let quake = RiderAbility::Combat(Attack::Quake);
        assert!(s.begin(quake, 20));
        assert!(!s.quake_chain(21));
        assert!(s.quake_chain(22));
        s.released = true;
        assert!(!s.quake_chain(22));
        s.chain_requested = true;
        assert!(s.quake_chain(23));
        s.chain_at = 23;
        assert!(!s.quake_chain(24));
        s.end(60);
        assert!(!s.begin(quake, 109));
        assert!(s.begin(quake, 110));
        let encoded = fastnbt::to_bytes(&s).unwrap();
        let decoded: RiderState = fastnbt::from_bytes(&encoded).unwrap();
        assert_eq!(decoded.active, Some(quake));
    }
    #[test]
    fn rider_flex_cooldown_begins_on_activation() {
        let mut s = RiderState::default();
        assert!(s.begin(RiderAbility::Flex, 100));
        s.end(167);
        assert!(!s.begin(RiderAbility::Flex, 219));
        assert!(s.begin(RiderAbility::Flex, 220));
    }
    fn snap(gap: f64) -> Snapshot {
        Snapshot {
            gap,
            facing: 1.0,
            enemies: 1,
            health: 20.0,
            ratio: 1.0,
            kite: 0,
            retreat: 0.0,
            lateral: 0.0,
            grounded: true,
            clear_lane: true,
            frozen: false,
        }
    }
    #[test]
    fn scores_and_cooldowns() {
        let mut s = State::default();
        assert_eq!(choose(&s, 0, &snap(2.0)), Some(Attack::Sweep));
        assert_eq!(choose(&s, 0, &snap(4.0)), Some(Attack::Precise));
        let mut pressure = snap(10.0);
        pressure.kite = 50;
        assert_eq!(choose(&s, 0, &pressure), Some(Attack::Quake));
        let mut retreat = snap(7.0);
        retreat.retreat = 0.1;
        assert_eq!(choose(&s, 0, &retreat), Some(Attack::Gungnir));
        retreat.clear_lane = false;
        assert_eq!(choose(&s, 0, &retreat), None);
        retreat.clear_lane = true;
        retreat.frozen = true;
        assert_eq!(choose(&s, 0, &retreat), None);
        s.begin(Attack::Sweep, 0, 0.0);
        assert_eq!(choose(&s, 31, &snap(2.0)), None);
        assert_eq!(choose(&s, 32, &snap(2.0)), Some(Attack::Slam));
        assert_eq!(choose(&s, 240, &snap(2.0)), Some(Attack::Sweep));
    }
    #[test]
    fn frost_impact_preserves_existing_freeze_and_requests_full_duration() {
        assert_eq!(frost_ticks(0, 100), 340);
        assert_eq!(frost_ticks(400, 100), 400);
        assert_eq!(frost_ticks(0, 60), 260);
        assert_eq!(frost_ticks(0, 0), 142);
    }
    #[test]
    fn quake_combo_waits_for_cadence_and_bypasses_only_major_cooldown() {
        let mut s = State::default();
        let target = Some([42; 16]);
        s.begin(Attack::Quake, 0, 0.0);
        s.reserve_combo(target);
        s.arm_combo(target, true, 39);
        let mut x = snap(7.0);
        x.frozen = true;
        assert_eq!(s.combo_action(target, 39, true, true, &x), Some(None));
        assert_eq!(s.combo_action(target, 50, false, true, &x), Some(None));
        assert_eq!(s.combo_action(target, 51, true, false, &x), Some(None));
        x.grounded = false;
        assert_eq!(s.combo_action(target, 52, true, true, &x), Some(None));
        x.grounded = true;
        s.cooldown[Attack::Gungnir as usize] = 60;
        assert_eq!(s.combo_action(target, 59, true, true, &x), Some(None));
        assert!(!s.ready(Attack::Gungnir, 60));
        assert_eq!(
            s.combo_action(target, 60, true, true, &x),
            Some(Some(Attack::Gungnir))
        );
        assert_eq!(s.combo_target, None);
    }
    #[test]
    fn quake_combo_expires_or_cancels_on_target_change_thaw_and_blocked_lane() {
        let target = Some([1; 16]);
        let mut x = snap(7.0);
        x.frozen = true;
        let armed = || {
            let mut s = State::default();
            s.reserve_combo(target);
            s.arm_combo(target, true, 39);
            s
        };
        assert_eq!(armed().combo_action(target, 138, true, true, &x), None);
        assert_eq!(
            armed().combo_action(Some([2; 16]), 50, true, true, &x),
            None
        );
        x.frozen = false;
        assert_eq!(armed().combo_action(target, 50, true, true, &x), None);
        x.frozen = true;
        x.clear_lane = false;
        assert_eq!(armed().combo_action(target, 50, true, true, &x), None);
        x.clear_lane = true;
        x.gap = 4.0;
        assert_eq!(armed().combo_action(target, 50, true, true, &x), None);
    }
    #[test]
    fn forward_geometry_rejects_behind() {
        let body = |z| EntityBody {
            min: [-0.5, -0.5, z],
            max: [0.5, 0.5, z + 1.0],
            ..Default::default()
        };
        assert!(forward_hit(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            body(4.0),
            5.5,
            4.0,
            2.475,
            100.0
        ));
        assert!(!forward_hit(
            [0.0; 3],
            [0.0, 0.0, 1.0],
            body(-6.0),
            5.5,
            4.0,
            2.475,
            100.0
        ));
    }
    #[test]
    fn nbt_roundtrip() {
        let empty: State =
            fastnbt::from_bytes(&fastnbt::to_bytes(&State::default()).unwrap()).unwrap();
        assert_eq!(empty.active, None);
        let mut s = State::default();
        s.begin(Attack::Sweep, 12, 0.0);
        s.reserve_combo(Some([-2; 16]));
        let d: State = fastnbt::from_bytes(&fastnbt::to_bytes(&s).unwrap()).unwrap();
        assert_eq!(d.active, Some(Attack::Sweep));
        assert_eq!(d.cooldown[0], 252);
        assert_eq!(d.combo_target, Some([-2; 16]));
    }
}
