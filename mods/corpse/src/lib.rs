//! The server-side port of Corpse 1.1.22+26.3.

mod records;
pub mod rules;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
};

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, Player, Pos, RegistrationError, Registry,
    content::{self, Content, ContentEvent, World},
    entities::{self, EntitiesExtension, EntityChange, SET_MOTION},
    inventory_menu::{self, Access, Change, MenuKind, Menus},
    native_entity::{self, Actor, ActorKind, Actors, Event, Model, Phase, SleepingVisual, Spawn},
    player_death::{self, Death, PlayerDeaths},
    saved_data::{self, Key, SavedData},
    stacks::{self, ItemStacks},
};
use serde::Deserialize;

use records::{Archive, Grave, SavedStack, blob, unblob};
use rules::{Lifetime, LifetimePolicy};

const INVENTORY_SLOTS: [i32; 45] = [
    39, 38, 37, 36, -1, 40, -1, -1, -1, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
    24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 0, 1, 2, 3, 4, 5, 6, 7, 8,
];
const NEVER_EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Interfaces {
    content: Content,
    actors: Actors,
    menus: Menus,
    death: PlayerDeaths,
    saved: SavedData,
    stacks: ItemStacks,
}

#[derive(Clone, Copy)]
struct Runtime {
    interfaces: Interfaces,
    key: Key,
    actor: ActorKind,
    menu: MenuKind,
}

static INTERFACES: OnceLock<Interfaces> = OnceLock::new();
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
static ARCHIVE_LOCK: Mutex<()> = Mutex::new(());
static SPILLING: LazyLock<Mutex<HashSet<[u8; 16]>>> = LazyLock::new(Mutex::default);
static INVENTORY_REVISION: AtomicU64 = AtomicU64::new(0);
type EmptyCache = HashMap<[u8; 16], (u64, bool)>;
static EMPTY: LazyLock<Mutex<EmptyCache>> = LazyLock::new(Mutex::default);

struct Corpse;

impl Mod for Corpse {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        entities::require()?.require_extension(EntitiesExtension::Player)?;
        let actors = native_entity::require()?;
        actors.require_visual()?;
        let _ = INTERFACES.set(Interfaces {
            content: content::require()?,
            actors,
            menus: inventory_menu::require()?,
            death: player_death::require()?,
            saved: saved_data::require()?,
            stacks: stacks::require()?,
        });
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let &interfaces = INTERFACES
            .get()
            .ok_or(RegistrationError::InvalidDefinition)?;
        let key = interfaces.saved.register(registry, "deaths")?;
        let actor = interfaces.actors.register(registry, actor_event)?;
        let menu = interfaces.menus.register(registry, inventory_changed)?;
        interfaces.death.on_drops(registry, capture)?;
        interfaces.content.on_tick(registry, retry_pending)?;
        registry.add_command(
            "corpse_recover",
            "Recover your saved items at a missing corpse",
            0,
        )?;
        let _ = RUNTIME.set(Runtime {
            interfaces,
            key,
            actor,
            menu,
        });
        Ok(())
    }

    fn command(&self, _: u64, command: Command<'_>) -> i32 {
        let (Some(&runtime), Some(player)) = (RUNTIME.get(), command.player) else {
            return 0;
        };
        let Some(profile) = runtime.interfaces.actors.player_profile(&player) else {
            return 0;
        };
        let Some(uuid) = profile_uuid(&profile) else {
            return 0;
        };
        i32::from(
            player.with_entity_context(|world, _| {
                let Some(dimension) = world.dimension_id() else {
                    return false;
                };
                let pos = player.position();
                let near = archive(runtime, world, |current| {
                    current
                        .deaths
                        .iter()
                        .rev()
                        .find(|grave| {
                            available(grave, dimension)
                                && !grave.is_empty()
                                && grave.owns(&uuid)
                                && distance_squared(grave.body_position, [pos.x, pos.y, pos.z])
                                    <= 16.0
                        })
                        .map(|grave| grave.id)
                })
                .flatten();
                if let Some(id) = near {
                    open_inventory(runtime, world, &player, id)
                } else {
                    player.send_message("You have no recoverable corpse nearby.");
                    false
                }
            }) == Some(true),
        )
    }
}

fn archive<T>(
    runtime: Runtime,
    world: World<'_>,
    visit: impl FnOnce(&mut Archive) -> T,
) -> Option<T> {
    // Dimensions in one save share the same key. Read the current save under the
    // same lock used for writes, including when another dimension changed it.
    let _guard = ARCHIVE_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
    let mut current = match runtime.interfaces.saved.read(&world, runtime.key) {
        Some(bytes) => fastnbt::from_bytes(&bytes).ok()?,
        None => Archive::default(),
    };
    Some(visit(&mut current))
}

fn capture(mut death: Death<'_>) {
    let Some(&runtime) = RUNTIME.get() else {
        return;
    };
    let Some(dimension) = death.world.dimension_id() else {
        return;
    };
    if !death
        .drops()
        .all(|drop| runtime.interfaces.stacks.info(drop.stack).is_some())
    {
        return;
    }
    let Some(profile) = runtime.interfaces.actors.player_profile(&death.player) else {
        return;
    };
    let saved = archive(runtime, death.world, |current| {
        let id = current.next_id.checked_add(1)?;
        let grave = Grave::new(id, dimension, &death, profile);
        current.next_id = id;
        current.deaths.push(grave.clone());
        let bytes = fastnbt::to_bytes(current).ok()?;
        if !death.claim(runtime.key, &bytes) {
            return None;
        }
        Some(grave)
    })
    .flatten();
    if let Some(grave) = saved {
        spawn(runtime, death.world, &grave);
    }
}

fn retry_pending(world: World<'_>, tick: u64, _: &[ContentEvent]) {
    if !tick.is_multiple_of(20) {
        return;
    }
    let Some(&runtime) = RUNTIME.get() else {
        return;
    };
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    let Some(pending) = archive(runtime, world, |current| {
        current
            .deaths
            .iter()
            .filter(|grave| grave.pending && grave.present && grave.dimension == dimension)
            .cloned()
            .collect::<Vec<_>>()
    }) else {
        return;
    };
    for grave in &pending {
        spawn(runtime, world, grave);
    }
}

fn spawn(runtime: Runtime, world: World<'_>, grave: &Grave) {
    let Some(uuid) = grave.uuid() else {
        return;
    };
    let mut position = grave.body_position;
    position[1] = position[1].max(min_height(world));
    position[1] = clear_floor_overlap(world, position);
    runtime.interfaces.actors.spawn(
        &world,
        runtime.actor,
        &Spawn {
            uuid,
            position,
            width: 2.0,
            height: 0.5,
            physics: true,
            data: &actor_data(grave.id, Lifetime::default()),
        },
    );
}

fn clear_floor_overlap(world: World<'_>, [x, y, z]: [f64; 3]) -> f64 {
    let block = Pos {
        x: x.floor() as i32,
        y: y.floor() as i32,
        z: z.floor() as i32,
    };
    let x = x - f64::from(block.x);
    let local_y = y - f64::from(block.y);
    let z = z - f64::from(block.z);
    let Some(shapes) = world.collision_shape(block) else {
        return y;
    };
    // The dying player's feet can be just below the surface on the final movement
    // tick. Entity sweeps cannot land on a collision box they already overlap.
    shapes
        .into_iter()
        .filter_map(|shape| {
            let gap = shape.max[1] - local_y;
            ((0.0..=0.125).contains(&gap)
                && shape.min[0] <= x
                && x <= shape.max[0]
                && shape.min[2] <= z
                && z <= shape.max[2])
                .then_some(y + gap)
        })
        .fold(y, f64::max)
}

fn actor_data(id: i64, life: Lifetime) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[..8].copy_from_slice(&id.to_le_bytes());
    bytes[8..12].copy_from_slice(&life.age_ticks.to_le_bytes());
    bytes[12..].copy_from_slice(&life.first_empty_tick.unwrap_or(NEVER_EMPTY).to_le_bytes());
    bytes
}

fn read_actor_data(actor: Actor<'_>) -> Option<(i64, Lifetime)> {
    let bytes = actor.data()?;
    let id = i64::from_le_bytes(bytes.get(..8)?.try_into().ok()?);
    let age_ticks = u32::from_le_bytes(bytes.get(8..12)?.try_into().ok()?);
    let empty = u32::from_le_bytes(bytes.get(12..)?.try_into().ok()?);
    Some((
        id,
        Lifetime {
            age_ticks,
            first_empty_tick: (empty != NEVER_EMPTY).then_some(empty),
        },
    ))
}

fn actor_event(event: Event<'_>) -> bool {
    let Some(&runtime) = RUNTIME.get() else {
        return false;
    };
    let Some((id, mut life)) = read_actor_data(event.actor) else {
        return false;
    };
    let Some(dimension) = event.world.dimension_id() else {
        return false;
    };
    let policy = LifetimePolicy::default();
    match event.phase {
        Phase::Loaded => {
            let grave = archive(runtime, event.world, |current| current.get(id).cloned()).flatten();
            let Some(grave) = grave.filter(|grave| {
                grave.uuid() == Some(event.actor.uuid) && grave.dimension == dimension
            }) else {
                moddedmc_api::info("Corpse archive record is unavailable. The carrier was kept.");
                return false;
            };
            if !grave.present {
                event.actor.entity.discard();
                return false;
            }
            if !render(event.actor, &grave, life.age_ticks >= policy.skeleton_after) {
                return false;
            }
            remember_position(runtime, &event, id);
        }
        Phase::Tick => {
            advance_motion(event.actor, event.world);
            let was_skeleton = life.age_ticks >= policy.skeleton_after;
            let Some(empty) = is_empty(runtime, &event, id) else {
                return false;
            };
            let result = life.tick(empty, policy);
            event.actor.set_data(&actor_data(id, life));
            if result.remove {
                event.actor.entity.discard();
            } else if result.is_skeleton != was_skeleton {
                let grave =
                    archive(runtime, event.world, |current| current.get(id).cloned()).flatten();
                if let Some(grave) = grave {
                    render(event.actor, &grave, true);
                }
            }
        }
        Phase::Used => {
            return event
                .player
                .as_ref()
                .is_some_and(|player| open_inventory(runtime, event.world, player, id));
        }
        Phase::Removed => spill(runtime, event, id),
        Phase::Unloaded => {
            EMPTY
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .remove(&event.actor.uuid);
            remember_position(runtime, &event, id);
        }
    }
    true
}

fn remember_position(runtime: Runtime, event: &Event<'_>, id: i64) {
    let Some(view) = event.actor.entity.view() else {
        return;
    };
    archive(runtime, event.world, |current| {
        let Some(grave) = current
            .get_mut(id)
            .filter(|grave| grave.present && grave.uuid() == Some(event.actor.uuid))
        else {
            return;
        };
        if !grave.pending && grave.body_position == view.position {
            return;
        }
        grave.pending = false;
        grave.body_position = view.position;
        if let Ok(bytes) = fastnbt::to_bytes(current) {
            runtime
                .interfaces
                .saved
                .write(&event.world, runtime.key, &bytes);
        }
    });
}

fn is_empty(runtime: Runtime, event: &Event<'_>, id: i64) -> Option<bool> {
    let revision = INVENTORY_REVISION.load(Ordering::Acquire);
    if let Some((read_at, empty)) = EMPTY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&event.actor.uuid)
        .copied()
        && read_at == revision
    {
        return Some(empty);
    }
    let empty = archive(runtime, event.world, |current| {
        current
            .get(id)
            .filter(|grave| grave.uuid() == Some(event.actor.uuid))
            .map(Grave::is_empty)
    })
    .flatten()?;
    EMPTY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(event.actor.uuid, (revision, empty));
    Some(empty)
}

fn distance_squared(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| (left - right).powi(2))
        .sum()
}

fn profile_uuid(profile: &[u8]) -> Option<[u8; 16]> {
    #[derive(Deserialize)]
    struct Identity {
        id: fastnbt::IntArray,
    }
    let identity = fastnbt::from_bytes::<Identity>(profile).ok()?;
    let values: &[i32; 4] = (&identity.id[..]).try_into().ok()?;
    let mut result = [0; 16];
    for (bytes, value) in result.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        bytes.copy_from_slice(&value.to_be_bytes());
    }
    Some(result)
}

fn advance_motion(actor: Actor<'_>, world: World<'_>) {
    let Some(view) = actor.entity.view() else {
        return;
    };
    let [x, y, z] = view.position;
    let [vx, vy, vz] = view.motion;
    let min_y = min_height(world);
    if y < min_y {
        actor.entity.set_position([x, min_y, z]);
    }
    let eye = Pos {
        x: x.floor() as i32,
        y: (y + 0.425).floor() as i32,
        z: z.floor() as i32,
    };
    let floating = matches!(
        world.fluid_id(eye).as_deref(),
        Some(
            "minecraft:water"
                | "minecraft:flowing_water"
                | "minecraft:lava"
                | "minecraft:flowing_lava"
        )
    );
    let vy = if floating {
        vy + if vy < 0.0 {
            0.01
        } else if vy < 0.03 {
            0.0005
        } else {
            0.0
        }
    } else if y > min_y {
        (vy - 0.0625).max(-2.0)
    } else {
        0.0
    };
    let motion = [vx * 0.75, vy, vz * 0.75];
    actor.entity.change(&EntityChange {
        mask: SET_MOTION,
        motion,
        ..EntityChange::default()
    });
    actor.entity.move_by(motion);
}

fn min_height(world: World<'_>) -> f64 {
    if world.dimension_id() == Some("minecraft:overworld") {
        -64.0
    } else {
        0.0
    }
}

#[derive(Deserialize)]
struct ProfileSettings {
    #[serde(default)]
    hotbar_slot: i32,
}

fn render(actor: Actor<'_>, grave: &Grave, skeleton: bool) -> bool {
    let profile = grave.profile();
    let held =
        fastnbt::from_bytes::<ProfileSettings>(&profile).map_or(0, |profile| profile.hotbar_slot);
    let keys = [held, 40, 36, 37, 38, 39];
    let equipment = keys.map(|key| {
        grave
            .original
            .iter()
            .find(|stack| stack.slot == key)
            .map(SavedStack::bytes)
    });
    let yaw = grave.yaw + 90.0;
    let direction = f64::from(yaw).to_radians();
    actor.show_sleeping(&SleepingVisual {
        model: if skeleton {
            Model::Skeleton
        } else {
            Model::Player
        },
        yaw,
        offset: [0.85 * direction.cos(), 0.0, 0.85 * direction.sin()],
        profile: Some(&profile),
        equipment: std::array::from_fn(|index| equipment[index].as_deref()),
    })
}

fn available(grave: &Grave, dimension: &str) -> bool {
    grave.present
        && grave.dimension == dimension
        && grave.uuid().is_some_and(|uuid| {
            !SPILLING
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(&uuid)
        })
}

fn open_inventory(runtime: Runtime, world: World<'_>, player: &Player<'_>, id: i64) -> bool {
    let Some(dimension) = world.dimension_id() else {
        return false;
    };
    let owner = archive(runtime, world, |current| {
        current
            .get(id)
            .filter(|grave| available(grave, dimension))
            .map(|grave| grave.owner.clone())
    })
    .flatten();
    let Some(owner) = owner else { return false };
    let access = INVENTORY_SLOTS.map(|slot| Access {
        take: slot >= 0,
        insert: false,
    });
    runtime.interfaces.menus.open(
        player,
        runtime.menu,
        id as u64,
        &format!("Corpse of {owner}"),
        &access,
        || {
            archive(runtime, world, |current| {
                let grave = current
                    .get(id)
                    .filter(|grave| available(grave, dimension))?;
                Some(
                    INVENTORY_SLOTS
                        .iter()
                        .map(|&slot| {
                            grave
                                .items
                                .iter()
                                .find(|item| item.slot == slot)
                                .map(SavedStack::bytes)
                        })
                        .collect(),
                )
            })
            .flatten()
        },
    )
}

fn inventory_changed(mut change: Change<'_>) {
    let Some(&runtime) = RUNTIME.get() else {
        return;
    };
    let Some(dimension) = change.world.dimension_id() else {
        return;
    };
    archive(runtime, change.world, |current| {
        let Some(grave) = current
            .get_mut(change.inventory as i64)
            .filter(|grave| available(grave, dimension))
        else {
            return;
        };
        grave.items = INVENTORY_SLOTS
            .into_iter()
            .zip(change.slots())
            .filter_map(|(slot, stack)| {
                let stack = stack?;
                (slot >= 0).then(|| SavedStack::new(slot, stack))
            })
            .collect();
        grave.overflow.extend(change.overflow().map(blob));
        if let Ok(bytes) = fastnbt::to_bytes(current)
            && change.commit(runtime.key, &bytes)
        {
            INVENTORY_REVISION.fetch_add(1, Ordering::Release);
        }
    });
}

struct Spill([u8; 16]);

impl Drop for Spill {
    fn drop(&mut self) {
        SPILLING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.0);
    }
}

fn spill(runtime: Runtime, event: Event<'_>, id: i64) {
    EMPTY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&event.actor.uuid);
    let Some(view) = event.actor.entity.view() else {
        return;
    };
    let started = archive(runtime, event.world, |current| {
        let grave = current
            .get(id)
            .filter(|grave| grave.present && grave.uuid() == Some(event.actor.uuid))?;
        let mut spilling = SPILLING.lock().unwrap_or_else(PoisonError::into_inner);
        spilling
            .insert(event.actor.uuid)
            .then(|| (Spill(event.actor.uuid), grave.clone()))
    })
    .flatten();
    let Some((_spill, mut grave)) = started else {
        return;
    };
    let failed = |bytes: &[u8]| {
        runtime
            .interfaces
            .stacks
            .spawn(event.world, view.position, [0.0, 0.2, 0.0], bytes)
            .is_none()
    };
    grave.items.retain(|stack| failed(&stack.bytes()));
    grave.overflow.retain(|bytes| failed(&unblob(bytes)));
    grave.present = !grave.is_empty();
    grave.pending = grave.present;
    grave.body_position = view.position;
    archive(runtime, event.world, |current| {
        let Some(record) = current.get_mut(id) else {
            return;
        };
        *record = grave;
        if let Ok(bytes) = fastnbt::to_bytes(current)
            && runtime
                .interfaces
                .saved
                .write(&event.world, runtime.key, &bytes)
        {
            INVENTORY_REVISION.fetch_add(1, Ordering::Release);
        }
    });
}

moddedmc_api::export_mod!(Corpse);
