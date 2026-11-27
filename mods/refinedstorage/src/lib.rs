//! Refined Storage v2.0.9-derived native prototype. Controller-owned networks,
//! eight-slot persistent storage disks, real 3x3 crafting-grid recipe matching
//! and host-mediated atomic external inventory automation.
mod autocrafting;
mod blocks;
mod craft;
mod crafter;
mod disk;
mod manager;
mod network;
mod processing;
mod stack;
mod transfer;
mod wireless;

use moddedmc_api::{
    InterfaceUnavailable, Mod, Player, Pos, RegistrationError, Registry,
    containers::{self, Containers, Transfers},
    content::{self, Content, ContentEvent, World, WorldExtension},
    crafting::{self, Crafting, RecipeMatch},
    entities::{self, EntitiesExtension},
    interaction::{
        self, BlockUseResult, Interaction, InteractionExtension, PlayerWillDestroy, UseBlock,
        UseItem, UseResult,
    },
    inventory_menu::{self, Access, Action, ActionSlots, Actions, Control, MenuKind},
    items::{self, Item, Items},
    stacks::{self, ItemStacks},
};
use network::{Network, Role};
use stack::{Request, Stack};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        LazyLock, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

const MENU_SLOTS: usize = 54;
const PER_PAGE: usize = 45;
const PREVIOUS: u64 = 500;
const NEXT: u64 = 501;
const CLEAR_PATTERN: u64 = 502;
const CRAFT_RECIPE: u64 = 503;
const PORT_TOGGLE: u64 = 504;
const PORT_POWER: u64 = 505;
const PORT_CLEAR: u64 = 506;
const CYCLE_RECIPE: u64 = 507;
const EXTERNAL_ALLOW: u64 = 600;
const EXTERNAL_INSERT: u64 = 601;
const EXTERNAL_EXTRACT: u64 = 602;
const EXTERNAL_PRIORITY_UP: u64 = 603;
const EXTERNAL_PRIORITY_DOWN: u64 = 604;
const EXTERNAL_POWER: u64 = 605;
const EXTERNAL_CLEAR: u64 = 606;
const AUTO_QUEUE: u64 = 900;
const AUTO_EJECT: u64 = 920;
const AUTO_ABORT: u64 = 940;
const AUTO_POWER: u64 = 950;
const PATTERN_SELECT: u64 = 960;
const AUTO_DEQUEUE: u64 = 980;
const CRAFTER_EJECT_PATTERN: u64 = 1200;
const CRAFTER_EJECT_UPGRADE: u64 = 1210;
const CRAFTER_MODE: u64 = 1220;
const PROCESSING_INPUT_SELECT: u64 = 1300;
const PROCESSING_OUTPUT_SELECT: u64 = 1320;
const PROCESSING_CLEAR_INPUTS: u64 = 1340;
const PROCESSING_CLEAR_OUTPUTS: u64 = 1341;
const PROCESSING_TOGGLE_MODE: u64 = 1342;
const PROCESSING_TOGGLE_EXACT: u64 = 1343;
const PROCESSING_TOGGLE_SIDE: u64 = 1344;
const PROCESSING_TOGGLE_PROCESSING: u64 = 1345;
const CRAFTER_MANAGER_SIZE: u64 = 1400;
const CRAFTER_MANAGER_SEARCH: u64 = 1401;
const REMOVE_FILTER: u64 = 700;
const EJECT_DISK: u64 = 800;
const REQUEST: u64 = 1000;
const SESSION_TTL: u64 = 20 * 60 * 5;

#[derive(Clone, Copy)]
struct Runtime {
    content: Content,
    containers: Containers,
    transfers: Transfers,
    crafting: Crafting,
    menus: Actions,
    stacks: ItemStacks,
    kind: MenuKind,
}
static RUNTIME: OnceLock<Runtime> = OnceLock::new();
// Menu-action callbacks currently fan out to every registered mod. Allocate a
// unique high-bit session namespace so another mod's stock menu cannot reuse
// the same numeric inventory id and accidentally authorize an old action.
static NEXT_SESSION: AtomicU64 = AtomicU64::new(0x5253_0000_0000_0001);
static OPERATIONS: Mutex<()> = Mutex::new(());
static SESSIONS: LazyLock<Mutex<HashMap<u64, Session>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// Loaded automation sites; rebuilt from ordered block events each start.
static PORTS: LazyLock<Mutex<HashMap<String, HashMap<Pos, u64>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy)]
struct Early {
    content: Content,
    containers: Containers,
    transfers: Transfers,
    crafting: Crafting,
    interaction: Interaction,
    menus: Actions,
    stacks: ItemStacks,
    items: Items,
}
static EARLY: OnceLock<Early> = OnceLock::new();

#[derive(Clone)]
struct Session {
    owner: String,
    dimension: String,
    site: Pos,
    role: Role,
    incarnation: u64,
    state: u32,
    created: u64,
    page: usize,
    remote: bool,
    requests: Vec<Request>,
    /// Exact mod block NBT at open time for actions that configure storage.
    snapshot: Vec<u8>,
}

struct RefinedStorage;
impl Mod for RefinedStorage {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_extension(WorldExtension::Dimension)?;
        content.require_extension(WorldExtension::ContentEntityTransaction)?;
        content.require_extension(WorldExtension::NeighborSignal)?;
        content.require_block_material()?;
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        interaction.require_extension(InteractionExtension::PlayerWillDestroy)?;
        interaction.require_extension(InteractionExtension::ImmediateItemUse)?;
        entities::require()?.require_extension(EntitiesExtension::Player)?;
        let _ = EARLY.set(Early {
            content,
            containers: containers::require()?,
            transfers: containers::require_transfers()?,
            crafting: crafting::require()?,
            interaction,
            menus: inventory_menu::require_actions()?,
            stacks: stacks::require()?,
            items: items::require()?,
        });
        Ok(())
    }
    fn register(&self, reg: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let early = *EARLY.get().ok_or(RegistrationError::InvalidDefinition)?;
        blocks::register(early.content, reg)?;
        for (id, label, carrier) in [
            (
                "refinedstorage:1k_storage_disk",
                "1k Storage Disk",
                "minecraft:music_disc_13",
            ),
            (
                "refinedstorage:4k_storage_disk",
                "4k Storage Disk",
                "minecraft:music_disc_cat",
            ),
            (
                "refinedstorage:16k_storage_disk",
                "16k Storage Disk",
                "minecraft:music_disc_blocks",
            ),
            (
                "refinedstorage:64k_storage_disk",
                "64k Storage Disk",
                "minecraft:music_disc_chirp",
            ),
            (
                "refinedstorage:1k_storage_part",
                "1k Storage Part",
                "minecraft:quartz",
            ),
            (
                "refinedstorage:4k_storage_part",
                "4k Storage Part",
                "minecraft:quartz",
            ),
            (
                "refinedstorage:16k_storage_part",
                "16k Storage Part",
                "minecraft:quartz",
            ),
            (
                "refinedstorage:64k_storage_part",
                "64k Storage Part",
                "minecraft:quartz",
            ),
            (
                "refinedstorage:storage_housing",
                "Storage Housing",
                "minecraft:iron_nugget",
            ),
            (
                "refinedstorage:quartz_enriched_iron",
                "Quartz Enriched Iron",
                "minecraft:iron_ingot",
            ),
            (
                "refinedstorage:quartz_enriched_copper",
                "Quartz Enriched Copper",
                "minecraft:copper_ingot",
            ),
            ("refinedstorage:silicon", "Silicon", "minecraft:quartz"),
            (
                "refinedstorage:machine_casing",
                "Machine Casing",
                "minecraft:iron_block",
            ),
            (
                "refinedstorage:processor_binding",
                "Processor Binding",
                "minecraft:slime_ball",
            ),
            (
                "refinedstorage:raw_basic_processor",
                "Raw Basic Processor",
                "minecraft:repeater",
            ),
            (
                "refinedstorage:raw_improved_processor",
                "Raw Improved Processor",
                "minecraft:repeater",
            ),
            (
                "refinedstorage:raw_advanced_processor",
                "Raw Advanced Processor",
                "minecraft:repeater",
            ),
            (
                "refinedstorage:basic_processor",
                "Basic Processor",
                "minecraft:comparator",
            ),
            (
                "refinedstorage:improved_processor",
                "Improved Processor",
                "minecraft:comparator",
            ),
            (
                "refinedstorage:advanced_processor",
                "Advanced Processor",
                "minecraft:comparator",
            ),
            (
                "refinedstorage:construction_core",
                "Construction Core",
                "minecraft:glowstone_dust",
            ),
            (
                "refinedstorage:destruction_core",
                "Destruction Core",
                "minecraft:redstone",
            ),
            ("refinedstorage:upgrade", "Upgrade", "minecraft:repeater"),
            (
                "refinedstorage:speed_upgrade",
                "Speed Upgrade",
                "minecraft:clock",
            ),
            (
                "refinedstorage:stack_upgrade",
                "Stack Upgrade",
                "minecraft:hopper",
            ),
            ("refinedstorage:pattern", "Pattern", "minecraft:paper"),
            (
                "refinedstorage:network_card",
                "Network Card",
                "minecraft:map",
            ),
            (
                "refinedstorage:wireless_grid",
                "Wireless Grid",
                "minecraft:compass",
            ),
            (
                "refinedstorage:range_upgrade",
                "Range Upgrade",
                "minecraft:repeater",
            ),
        ] {
            early.items.add_item(
                reg,
                &Item {
                    id,
                    display_name: label,
                    carrier,
                    model: "minecraft:item/compass",
                },
            )?;
        }
        let kind = early.menus.menus().register(reg, |_change| {})?;
        early.menus.on_action(reg, action)?;
        early.interaction.on_use_block(reg, used)?;
        early.interaction.on_use_item(reg, use_wireless)?;
        early.interaction.on_player_will_destroy(reg, destroyed)?;
        early.content.on_tick(reg, tick)?;
        reg.add_command("refinedstorage", "Give a Refined Storage Controller", 0)?;
        RUNTIME
            .set(Runtime {
                content: early.content,
                containers: early.containers,
                transfers: early.transfers,
                crafting: early.crafting,
                menus: early.menus,
                stacks: early.stacks,
                kind,
            })
            .map_err(|_| RegistrationError::DuplicateId)?;
        Ok(())
    }
    fn command(&self, _tag: u64, command: moddedmc_api::Command<'_>) -> i32 {
        command.player.map_or(0, |player| {
            i32::from(player.give_block("refinedstorage:controller"))
        })
    }
}

fn indicator(id: &str) -> Option<Vec<u8>> {
    fastnbt::to_bytes(&HashMap::from([
        ("id".to_owned(), fastnbt::Value::String(id.into())),
        ("count".to_owned(), fastnbt::Value::Int(1)),
    ]))
    .ok()
}

fn current_block(api: Runtime, world: World<'_>, at: Pos, role: Role) -> bool {
    let mut state = [content::UNLOADED_STATE];
    world.read(&[at], &mut state)
        && blocks::kind(api.content, state[0]).is_some_and(|(def, _)| def.role == role)
}
fn within_reach(player: &Player<'_>, dim: &str, at: Pos) -> bool {
    if player.dimension() != dim {
        return false;
    }
    let player = player.position();
    let squared = (player.x - f64::from(at.x) - 0.5).powi(2)
        + (player.y - f64::from(at.y) - 0.5).powi(2)
        + (player.z - f64::from(at.z) - 0.5).powi(2);
    squared.is_finite() && squared <= 64.0
}
fn graph(api: Runtime, world: World<'_>, start: Pos) -> Result<Network, network::Error> {
    network::discover(
        |at| {
            let mut state = [content::UNLOADED_STATE];
            if !world.read(&[at], &mut state) || state[0] == content::UNLOADED_STATE {
                return Err(());
            }
            Ok(blocks::node(api.content, state[0]))
        },
        start,
    )
}

fn wireless_access(api: Runtime, world: World<'_>, player: &Player<'_>, root: Pos) -> bool {
    if world.dimension_id() != Some(player.dimension())
        || !current_block(api, world, root, Role::Controller)
    {
        return false;
    }
    let Ok(net) = graph(api, world, root) else {
        return false;
    };
    if net.controller != root {
        return false;
    }
    let position = player.position();
    net.transmitters.into_iter().any(|at| {
        wireless::load(world, at).is_some_and(|transmitter| {
            wireless::in_range(
                [position.x, position.y, position.z],
                at,
                transmitter.range(),
            )
        })
    })
}
/// Match each physical container at most once, even if two external storage
/// interfaces face the same barrel.
fn storage_block_tier(api: Runtime, world: World<'_>, at: Pos) -> Option<disk::Tier> {
    let mut state = [content::UNLOADED_STATE];
    if !world.read(&[at], &mut state) {
        return None;
    }
    let (definition, _) = blocks::kind(api.content, state[0])?;
    disk::Tier::from_block(definition.id)
}

fn matching_recipes(
    crafting: Crafting,
    world: World<'_>,
    pattern: &craft::Pattern,
) -> Option<Vec<RecipeMatch>> {
    let choices = crafting.enumerate_recipes(world, 3, 3, &pattern.grid(), 64)?;
    (!choices.truncated && !choices.matches.is_empty()).then_some(choices.matches)
}

fn selected_matching_recipe(
    crafting: Crafting,
    world: World<'_>,
    pattern: &craft::Pattern,
) -> Option<RecipeMatch> {
    let choices = matching_recipes(crafting, world, pattern)?;
    let selected = pattern.selected_recipe(&choices)?;
    // RecipeMatch is a borrowed API payload, not a Clone type. Own only the
    // fields needed after the choices vector is dropped.
    Some(RecipeMatch {
        result: selected.result.clone(),
        id: selected.id.clone(),
        serializer: selected.serializer.clone(),
    })
}

fn unique_recipe(
    crafting: Crafting,
    world: World<'_>,
    pattern: &autocrafting::RecipePattern,
) -> bool {
    selected_matching_recipe(crafting, world, &pattern.grid)
        .is_some_and(|recipe| pattern.matches(&recipe))
}

/// Refined Storage 1.12.4 crafting patterns are not arbitrary input/output
/// mappings: CraftingPatternFactory resolves a real live crafting recipe and
/// derives its output. This item-only native subset requires Exact=true because
/// the SDK does not expose reverse ingredient-tag alternatives.
fn v1124_crafting_pattern_matches(
    crafting: Crafting,
    world: World<'_>,
    pattern: &processing::CraftingPatternItem,
) -> bool {
    v1124_crafting_pattern_result(crafting, world, pattern).is_some()
}

fn v1124_crafting_pattern_result(
    crafting: Crafting,
    world: World<'_>,
    pattern: &processing::CraftingPatternItem,
) -> Option<Vec<u8>> {
    let mut grid = craft::Pattern::default();
    grid.slots = pattern.0.inputs.clone();
    let expected = pattern.0.outputs.iter().flatten().next();
    let mut choices = matching_recipes(crafting, world, &grid)?;
    if choices.len() != 1 {
        return None;
    }
    let choice = choices.pop()?;
    let actual = Stack::parse(&choice.result)?;
    expected
        .is_none_or(|expected| actual.same_item(expected) && actual.count == expected.count)
        .then_some(actual.raw)
}

fn installed_pattern_output(
    api: Runtime,
    world: World<'_>,
    pattern: &crafter::InstalledPattern,
) -> Option<Vec<u8>> {
    match pattern {
        crafter::InstalledPattern::Processing(pattern) => pattern
            .outputs
            .iter()
            .flatten()
            .next()
            .and_then(|item| item.with_count(item.count)),
        crafter::InstalledPattern::V1124Crafting(pattern) => {
            v1124_crafting_pattern_result(api.crafting, world, pattern)
        }
        crafter::InstalledPattern::Crafting(pattern) => unique_recipe(api.crafting, world, pattern)
            .then(|| pattern.result.with_count(pattern.result.count))?,
    }
}

fn installed_pattern_is_live(
    api: Runtime,
    world: World<'_>,
    pattern: &crafter::InstalledPattern,
) -> bool {
    installed_pattern_output(api, world, pattern).is_some()
}

fn visible_stacks(api: Runtime, world: World<'_>, net: &Network) -> Option<Vec<Stack>> {
    let mut result = Vec::new();
    for &site in &net.drives {
        for item in disk::load(world, site)?.records() {
            let mut left = item.count;
            while left > 0 {
                let amount = left.min(99 * 64) as u32;
                let raw = item.sample.with_count(amount)?;
                result.push(Stack::parse(&raw)?);
                left -= u64::from(amount);
                if result.len() > 5000 {
                    return None;
                }
            }
        }
    }
    for &site in &net.storage_blocks {
        let mut state = [content::UNLOADED_STATE];
        if !world.read(&[site], &mut state) {
            return None;
        }
        let (def, _) = blocks::kind(api.content, state[0])?;
        let tier = disk::Tier::from_block(def.id)?;
        let storage = disk::load_block(world, site, tier)?;
        for item in &storage.items {
            let mut left = item.count;
            while left > 0 {
                let amount = left.min(99 * 64) as u32;
                let raw = item.sample.with_count(amount)?;
                result.push(Stack::parse(&raw)?);
                left -= u64::from(amount);
                if result.len() > 5000 {
                    return None;
                }
            }
        }
    }
    let mut seen = HashSet::new();
    for (external, policy) in transfer::external_connections(world, net) {
        if !policy.can_extract || !seen.insert(external.pos) {
            continue;
        }
        let Some(container) = api.containers.at(world, external.pos, external.face) else {
            continue;
        };
        let Some(slots) = container.slots() else {
            continue;
        };
        if slots > 256 {
            return None;
        }
        for slot in 0..slots {
            if let Some(raw) = container.stack(slot) {
                let item = Stack::parse(&raw)?;
                if !policy.allows(&item) {
                    continue;
                }
                result.push(item);
                if result.len() > 5000 {
                    return None;
                }
            }
        }
    }
    Some(result)
}

fn show(player: &Player<'_>, world: World<'_>, site: Pos, page: usize, remote: bool) -> bool {
    let Some(api) = RUNTIME.get().copied() else {
        return false;
    };
    let Some(dim) = world.dimension_id() else {
        return false;
    };
    if !(if remote {
        wireless_access(api, world, player, site)
    } else {
        within_reach(player, dim, site)
    }) {
        return false;
    }
    let Some(snapshot) = world.entity_snapshot(site) else {
        return false;
    };
    let Some((definition, _)) = blocks::kind(api.content, snapshot.state) else {
        return false;
    };
    let role = definition.role;
    if remote && role != Role::Controller {
        return false;
    }
    let mut slots = vec![None; MENU_SLOTS];
    let mut controls = Vec::<Control>::new();
    let mut requests = Vec::<Request>::new();
    let mut page = page;
    let title = match role {
        Role::Drive => {
            let Some(storage) = disk::load(world, site) else {
                return false;
            };
            for (index, disk) in storage.disks.iter().enumerate() {
                if let Some(disk) = disk {
                    // The menu only needs the disk tier icon. Serializing the
                    // entire on-disk contents into a display stack can exceed
                    // the host's per-slot item NBT bound for a full disk.
                    slots[index] = indicator(disk.tier.item());
                    controls.push(Control {
                        slot: index as u8,
                        tag: EJECT_DISK + index as u64,
                    });
                }
            }
            format!(
                "Disk Drive - {} / {} items",
                storage.used(),
                storage.capacity()
            )
        }
        Role::Importer | Role::Exporter => {
            let Some(policy) = transfer::load(world, site, role == Role::Exporter) else {
                return false;
            };
            for (index, item) in policy.filters.iter().enumerate() {
                slots[index] = item.with_count(1);
                controls.push(Control {
                    slot: index as u8,
                    tag: REMOVE_FILTER + index as u64,
                });
            }
            slots[45] = indicator(if policy.allowlist {
                "minecraft:lime_dye"
            } else {
                "minecraft:red_dye"
            });
            slots[49] = indicator(if policy.needs_signal {
                "minecraft:redstone_torch"
            } else {
                "minecraft:lever"
            });
            slots[53] = indicator("minecraft:barrier");
            controls.extend([
                Control {
                    slot: 45,
                    tag: PORT_TOGGLE,
                },
                Control {
                    slot: 49,
                    tag: PORT_POWER,
                },
                Control {
                    slot: 53,
                    tag: PORT_CLEAR,
                },
            ]);
            format!(
                "{} - {} / {} items per pulse",
                definition.label,
                if policy.allowlist { "Allow" } else { "Deny" },
                policy.rate
            )
        }
        Role::External => {
            let Some(policy) = transfer::load_external(world, site) else {
                return false;
            };
            for (index, item) in policy.filters.iter().enumerate() {
                slots[index] = item.with_count(1);
                controls.push(Control {
                    slot: index as u8,
                    tag: REMOVE_FILTER + index as u64,
                });
            }
            slots[45] = indicator(if policy.allowlist {
                "minecraft:lime_dye"
            } else {
                "minecraft:red_dye"
            });
            slots[46] = indicator(if policy.can_insert {
                "minecraft:hopper"
            } else {
                "minecraft:barrier"
            });
            slots[47] = indicator(if policy.can_extract {
                "minecraft:dropper"
            } else {
                "minecraft:barrier"
            });
            slots[48] = indicator("minecraft:gold_nugget");
            slots[49] = indicator("minecraft:iron_nugget");
            slots[50] = indicator(if policy.needs_signal {
                "minecraft:redstone_torch"
            } else {
                "minecraft:lever"
            });
            slots[53] = indicator("minecraft:barrier");
            controls.extend([
                Control {
                    slot: 45,
                    tag: EXTERNAL_ALLOW,
                },
                Control {
                    slot: 46,
                    tag: EXTERNAL_INSERT,
                },
                Control {
                    slot: 47,
                    tag: EXTERNAL_EXTRACT,
                },
                Control {
                    slot: 48,
                    tag: EXTERNAL_PRIORITY_UP,
                },
                Control {
                    slot: 49,
                    tag: EXTERNAL_PRIORITY_DOWN,
                },
                Control {
                    slot: 50,
                    tag: EXTERNAL_POWER,
                },
                Control {
                    slot: 53,
                    tag: EXTERNAL_CLEAR,
                },
            ]);
            format!(
                "External Storage P{} / {} / {} / {}",
                policy.priority,
                if policy.can_insert { "In" } else { "No In" },
                if policy.can_extract { "Out" } else { "No Out" },
                if policy.needs_signal {
                    "Power"
                } else {
                    "Always"
                },
            )
        }
        Role::Autocrafter => {
            let Some(state) = autocrafting::load(world, site) else {
                return false;
            };
            let mut stalled = 0usize;
            slots[53] = indicator(if state.needs_signal {
                "minecraft:redstone_torch"
            } else {
                "minecraft:lever"
            });
            controls.push(Control {
                slot: 53,
                tag: AUTO_POWER,
            });
            for (index, job) in state.jobs.iter().enumerate() {
                slots[index] = job.pattern.result.with_count(job.pattern.result.count);
                slots[18 + index] = indicator(if job.inflight {
                    "minecraft:redstone_torch"
                } else if job.pending > 0 {
                    "minecraft:clock"
                } else {
                    "minecraft:barrier"
                });
                if job.inflight {
                    stalled += 1;
                    slots[36 + index] = indicator("minecraft:barrier");
                    controls.push(Control {
                        slot: (36 + index) as u8,
                        tag: AUTO_ABORT + index as u64,
                    });
                } else {
                    controls.push(Control {
                        slot: index as u8,
                        tag: AUTO_QUEUE + index as u64,
                    });
                    if job.pending > 0 {
                        slots[27 + index] = indicator("minecraft:red_dye");
                        controls.push(Control {
                            slot: (27 + index) as u8,
                            tag: AUTO_DEQUEUE + index as u64,
                        });
                    }
                }
                if job.pending == 0 {
                    controls.push(Control {
                        slot: (18 + index) as u8,
                        tag: AUTO_EJECT + index as u64,
                    });
                }
            }
            format!(
                "Autocrafter - {} patterns / {} uncertain / {}",
                state.jobs.len(),
                stalled,
                if state.needs_signal {
                    "Powered"
                } else {
                    "Always"
                }
            )
        }
        Role::Crafter => {
            let Some(state) = crafter::load(world, site) else {
                return false;
            };
            for (index, pattern) in state.patterns.iter().enumerate() {
                if let Some(pattern) = pattern {
                    slots[index] = installed_pattern_output(api, world, pattern)
                        .or_else(|| indicator("minecraft:barrier"));
                    controls.push(Control {
                        slot: index as u8,
                        tag: CRAFTER_EJECT_PATTERN + index as u64,
                    });
                }
            }
            for index in 0..state.speed_upgrades as usize {
                slots[18 + index] = indicator("refinedstorage:speed_upgrade");
                controls.push(Control {
                    slot: (18 + index) as u8,
                    tag: CRAFTER_EJECT_UPGRADE + index as u64,
                });
            }
            slots[53] = indicator(match state.mode {
                crafter::Mode::Ignore => "minecraft:lever",
                crafter::Mode::SignalUnlocks => "minecraft:lime_dye",
                crafter::Mode::SignalLocks => "minecraft:red_dye",
                crafter::Mode::PulseInsertsNextSet => "minecraft:clock",
            });
            controls.push(Control {
                slot: 53,
                tag: CRAFTER_MODE,
            });
            let root = graph(api, world, site).ok().and_then(|net| {
                let crafters = net
                    .crafters
                    .iter()
                    .map(|(pos, face)| (*pos, crafter::CrafterNode { facing: *face }))
                    .collect();
                let network_nodes = net.nodes.keys().copied().collect();
                crafter::root_container(&crafters, &network_nodes, site)
            });
            format!(
                "Crafter - {} patterns / {} speed / {} ticks / root {:?}",
                state.patterns.iter().flatten().count(),
                state.speed_upgrades,
                state.update_interval(),
                root.map(|pos| (pos.x, pos.y, pos.z)),
            )
        }
        Role::CrafterManager => {
            let Ok(net) = graph(api, world, site) else {
                return false;
            };
            let Some(manager) = manager::load(world, site) else {
                return false;
            };
            let mut count = 0usize;
            let visible_limit = manager.visible_pattern_limit();
            for (crafter_pos, _) in &net.crafters {
                let Some(state) = crafter::load(world, *crafter_pos) else {
                    continue;
                };
                for pattern in state.patterns.iter().flatten() {
                    if !installed_pattern_is_live(api, world, pattern) {
                        continue;
                    }
                    let Some(output) = installed_pattern_output(api, world, pattern) else {
                        continue;
                    };
                    if count < visible_limit {
                        slots[count] = Some(output);
                    }
                    count += 1;
                }
            }
            slots[49] = indicator("minecraft:chest");
            slots[50] = indicator("minecraft:spyglass");
            controls.extend([
                Control {
                    slot: 49,
                    tag: CRAFTER_MANAGER_SIZE,
                },
                Control {
                    slot: 50,
                    tag: CRAFTER_MANAGER_SEARCH,
                },
            ]);
            format!(
                "Crafter Manager - {count} patterns / {} crafters / size {} / search {}",
                net.crafters.len(),
                manager.size,
                manager.search_box_mode
            )
        }
        Role::Grid | Role::CraftingGrid | Role::PatternGrid | Role::Controller => {
            let Ok(net) = graph(api, world, site) else {
                return false;
            };
            let Some(stacks) = visible_stacks(api, world, &net) else {
                return false;
            };
            requests = stack::aggregate(stacks);
            stack::sort_requests(&mut requests, stack::Sort::Name, false);
            let processing_editor = if role == Role::PatternGrid {
                let Some(editor) = processing::load_editor(world, site) else {
                    return false;
                };
                Some(editor)
            } else {
                None
            };
            let processing_mode = processing_editor
                .as_ref()
                .is_some_and(|editor| editor.enabled);
            let per_page = if processing_mode {
                27
            } else if matches!(role, Role::CraftingGrid | Role::PatternGrid) {
                36
            } else {
                PER_PAGE
            };
            let total_pages = requests.len().max(1).div_ceil(per_page);
            page = page.min(total_pages - 1);
            for (index, record) in requests
                .iter()
                .skip(page * per_page)
                .take(per_page)
                .enumerate()
            {
                let slot = if processing_mode { 18 + index } else { index };
                slots[slot] = record.sample.with_count(record.total.min(64) as u32);
                controls.push(Control {
                    slot: slot as u8,
                    tag: REQUEST + index as u64,
                });
            }
            if page > 0 {
                slots[45] = indicator("minecraft:arrow");
                controls.push(Control {
                    slot: 45,
                    tag: PREVIOUS,
                });
            }
            if page + 1 < total_pages {
                slots[53] = indicator("minecraft:arrow");
                controls.push(Control {
                    slot: 53,
                    tag: NEXT,
                });
            }
            if processing_mode {
                let Some(editor) = processing_editor.as_ref() else {
                    return false;
                };
                for (index, item) in editor.pattern.inputs.iter().enumerate() {
                    slots[index] = item.as_ref().map(|item| item.raw.clone());
                    controls.push(Control {
                        slot: index as u8,
                        tag: PROCESSING_INPUT_SELECT + index as u64,
                    });
                }
                for (index, item) in editor.pattern.outputs.iter().enumerate() {
                    slots[9 + index] = item.as_ref().map(|item| item.raw.clone());
                    controls.push(Control {
                        slot: (9 + index) as u8,
                        tag: PROCESSING_OUTPUT_SELECT + index as u64,
                    });
                }
                slots[46] = indicator("minecraft:barrier");
                slots[47] = indicator("minecraft:barrier");
                slots[48] = indicator("minecraft:crafting_table");
                slots[49] = indicator(if editor.pattern.exact {
                    "minecraft:enchanted_book"
                } else {
                    "minecraft:book"
                });
                slots[50] = indicator(if editor.editing_outputs() {
                    "minecraft:chest"
                } else {
                    "minecraft:hopper"
                });
                slots[51] = indicator(if editor.processing {
                    "minecraft:blast_furnace"
                } else {
                    "minecraft:crafting_table"
                });
                controls.extend([
                    Control {
                        slot: 46,
                        tag: PROCESSING_CLEAR_INPUTS,
                    },
                    Control {
                        slot: 47,
                        tag: PROCESSING_CLEAR_OUTPUTS,
                    },
                    Control {
                        slot: 48,
                        tag: PROCESSING_TOGGLE_MODE,
                    },
                    Control {
                        slot: 49,
                        tag: PROCESSING_TOGGLE_EXACT,
                    },
                    Control {
                        slot: 50,
                        tag: PROCESSING_TOGGLE_SIDE,
                    },
                    Control {
                        slot: 51,
                        tag: PROCESSING_TOGGLE_PROCESSING,
                    },
                ]);
                format!(
                    "{} Pattern - {} input / {} output / {}",
                    if editor.processing {
                        "Processing"
                    } else {
                        "Crafting"
                    },
                    editor.pattern.inputs.iter().flatten().count(),
                    editor.pattern.outputs.iter().flatten().count(),
                    if editor.editing_outputs() {
                        "Editing outputs"
                    } else {
                        "Editing inputs"
                    },
                )
            } else if matches!(role, Role::CraftingGrid | Role::PatternGrid) {
                let Some(pattern) = craft::load(world, site) else {
                    return false;
                };
                for (index, item) in pattern.slots.iter().enumerate() {
                    slots[36 + index] = item.as_ref().map(|item| item.raw.clone());
                    controls.push(Control {
                        slot: (36 + index) as u8,
                        tag: PATTERN_SELECT + index as u64,
                    });
                }
                slots[46] = indicator("minecraft:barrier");
                controls.push(Control {
                    slot: 46,
                    tag: CLEAR_PATTERN,
                });
                if let Some(choices) = matching_recipes(api.crafting, world, &pattern) {
                    if let Some(recipe) = pattern.selected_recipe(&choices) {
                        slots[49] = Some(recipe.result.clone());
                        if role == Role::CraftingGrid {
                            controls.push(Control {
                                slot: 49,
                                tag: CRAFT_RECIPE,
                            });
                        }
                    }
                    if choices.len() > 1 {
                        slots[47] = indicator("minecraft:arrow");
                        controls.push(Control {
                            slot: 47,
                            tag: CYCLE_RECIPE,
                        });
                    }
                }
                if role == Role::PatternGrid {
                    slots[48] = indicator("minecraft:hopper");
                    controls.push(Control {
                        slot: 48,
                        tag: PROCESSING_TOGGLE_MODE,
                    });
                }
                format!(
                    "{} - {} items / {} pages",
                    definition.label,
                    requests.len(),
                    total_pages
                )
            } else {
                format!(
                    "Refined Storage - {} types / {} pages",
                    requests.len(),
                    total_pages
                )
            }
        }
        _ => return false,
    };
    let id = NEXT_SESSION.fetch_add(1, Ordering::Relaxed);
    let session = Session {
        owner: player.name().into(),
        dimension: dim.into(),
        site,
        role,
        incarnation: snapshot.incarnation,
        state: snapshot.state,
        created: moddedmc_api::tick_count(),
        page,
        remote,
        requests,
        snapshot: snapshot.nbt,
    };
    let Ok(mut sessions) = SESSIONS.lock() else {
        return false;
    };
    let now = moddedmc_api::tick_count();
    sessions.retain(|_, existing| {
        existing.owner != session.owner && now.saturating_sub(existing.created) <= SESSION_TTL
    });
    sessions.insert(id, session);
    drop(sessions);
    let opened = api.menus.open(
        player,
        api.kind,
        id,
        &title,
        ActionSlots {
            world,
            access: &[Access {
                take: false,
                insert: false,
            }; MENU_SLOTS],
            controls: &controls,
        },
        || Some(slots),
    );
    if !opened {
        if let Ok(mut sessions) = SESSIONS.lock() {
            sessions.remove(&id);
        }
    }
    opened
}

fn used(ctx: &mut UseBlock<'_>) -> BlockUseResult {
    let Some(api) = RUNTIME.get().copied() else {
        return BlockUseResult::Fail;
    };
    if !ctx.main_hand {
        return BlockUseResult::Pass;
    }
    let Some((definition, _)) = blocks::kind(api.content, ctx.state) else {
        return BlockUseResult::Pass;
    };
    if !current_block(api, ctx.world, ctx.position, definition.role) {
        return BlockUseResult::Fail;
    }
    let Some(player) = ctx.player.and_then(|entity| entity.as_player()) else {
        return BlockUseResult::Fail;
    };
    let role = definition.role;
    let held = if ctx.with_item && ctx.stack.count() > 0 {
        ctx.stack.read().and_then(|bytes| Stack::parse(&bytes))
    } else {
        None
    };
    let Ok(_guard) = OPERATIONS.lock() else {
        return BlockUseResult::Fail;
    };
    match role {
        Role::Drive => {
            if let Some(item) = held {
                if disk::Tier::from_item(&item.id).is_some() {
                    let Some(cartridge) = disk::load_cartridge(&item) else {
                        ctx.action_bar("Invalid or damaged storage disk");
                        return BlockUseResult::Success;
                    };
                    if disk::update(ctx.world, ctx.position, |drive| {
                        drive.install(cartridge).then_some(())
                    })
                    .is_some()
                    {
                        if !ctx.creative {
                            ctx.stack.shrink(1);
                        }
                        ctx.action_bar("Storage disk installed and contents mounted");
                    } else {
                        ctx.action_bar("Drive is full, missing or changed");
                    }
                    return BlockUseResult::Success;
                }
                if let Some(tier) = disk::Tier::from_part(&item.id) {
                    let upgraded = disk::update(ctx.world, ctx.position, |drive| {
                        drive.upgrade_first(tier).then_some(())
                    })
                    .is_some();
                    if upgraded && !ctx.creative {
                        ctx.stack.shrink(1);
                    }
                    ctx.action_bar(if upgraded {
                        "Storage disk capacity upgraded; contents preserved"
                    } else {
                        "No lower-tier storage disk is installed"
                    });
                    return BlockUseResult::Success;
                }
                return BlockUseResult::TryEmptyHand;
            }
            if ctx.sneaking {
                // Ejection retains contents on the disk item itself. A failed
                // hand delivery attempts to put the disk back into the drive.
                let removed = disk::update(ctx.world, ctx.position, |drive| {
                    let index = drive.disks.iter().rposition(Option::is_some)?;
                    drive.eject(index)
                });
                if let Some(disk) = removed {
                    let item = disk::cartridge_item(&disk);
                    if !item.as_ref().is_some_and(|bytes| ctx.give(bytes)) {
                        let _ = disk::update(ctx.world, ctx.position, |drive| {
                            drive.install(disk).then_some(())
                        });
                        ctx.action_bar("Unable to deliver disk; restored to drive");
                    } else {
                        ctx.action_bar("Storage disk and contents ejected");
                    }
                } else {
                    ctx.action_bar("No storage disk installed");
                }
                return BlockUseResult::Success;
            }
            let _ = show(&player, ctx.world, ctx.position, 0, false);
            BlockUseResult::Success
        }
        Role::Grid | Role::CraftingGrid | Role::PatternGrid => {
            if let Some(item) = held {
                if role == Role::PatternGrid && item.id == "refinedstorage:pattern" && !ctx.sneaking
                {
                    if item.count != 1 {
                        ctx.action_bar("Encode one blank Pattern at a time");
                        return BlockUseResult::Success;
                    }
                    if processing::load_editor(ctx.world, ctx.position)
                        .is_some_and(|editor| editor.enabled)
                    {
                        let Some(editor) = processing::load_editor(ctx.world, ctx.position) else {
                            return BlockUseResult::Fail;
                        };
                        if !editor.processing {
                            let crafting_pattern =
                                processing::CraftingPatternItem(editor.pattern.clone());
                            if !v1124_crafting_pattern_matches(
                                api.crafting,
                                ctx.world,
                                &crafting_pattern,
                            ) {
                                ctx.action_bar("Crafting Pattern requires an exact live recipe and matching output");
                                return BlockUseResult::Success;
                            }
                        }
                        let Some(data) = editor.encoded_item_data() else {
                            ctx.action_bar("Pattern needs a valid output; crafting mode also requires Exact=true");
                            return BlockUseResult::Success;
                        };
                        if ctx.stack.set_data(&data) {
                            ctx.action_bar(if editor.processing {
                                "Refined Storage processing Pattern encoded"
                            } else {
                                "Refined Storage crafting Pattern encoded"
                            });
                        } else {
                            ctx.action_bar("Processing Pattern data could not be saved");
                        }
                        return BlockUseResult::Success;
                    }
                    let Some(grid) = craft::load(ctx.world, ctx.position) else {
                        return BlockUseResult::Fail;
                    };
                    let Some(recipe) = selected_matching_recipe(api.crafting, ctx.world, &grid)
                    else {
                        ctx.action_bar("No matching crafting recipe to encode");
                        return BlockUseResult::Success;
                    };
                    let Some(record) = autocrafting::RecipePattern::from_live(grid, &recipe) else {
                        return BlockUseResult::Fail;
                    };
                    let Some(data) = record.item_data() else {
                        return BlockUseResult::Fail;
                    };
                    if ctx.stack.set_data(&data) {
                        ctx.action_bar("Pattern encoded with actual recipe identity and output");
                    } else {
                        ctx.action_bar("Pattern encoding could not be persisted");
                    }
                    return BlockUseResult::Success;
                }
                if matches!(role, Role::CraftingGrid | Role::PatternGrid) && ctx.sneaking {
                    if role == Role::PatternGrid
                        && processing::load_editor(ctx.world, ctx.position)
                            .is_some_and(|editor| editor.enabled)
                    {
                        let assigned =
                            processing::update_editor(ctx.world, ctx.position, |editor| {
                                editor.assign(&item).then_some(())
                            })
                            .is_some();
                        ctx.action_bar(if assigned {
                            "Processing Pattern ingredient/output recorded"
                        } else {
                            "Processing Pattern slot unavailable"
                        });
                        return BlockUseResult::Success;
                    }
                    let updated =
                        craft::update(ctx.world, ctx.position, |pattern| pattern.add(&item));
                    ctx.action_bar(if updated {
                        "Crafting ghost ingredient recorded"
                    } else {
                        "Crafting grid full or unavailable; clear its pattern menu"
                    });
                    return BlockUseResult::Success;
                }
                if role == Role::PatternGrid {
                    return BlockUseResult::TryEmptyHand;
                }
                if disk::Tier::from_item(&item.id).is_some() {
                    ctx.action_bar("Install storage disks into a connected Disk Drive");
                    return BlockUseResult::Success;
                }
                let Ok(net) = graph(api, ctx.world, ctx.position) else {
                    ctx.action_bar("Connect a single Controller and a Disk Drive");
                    return BlockUseResult::Success;
                };
                let mut remainder = item.count;
                for drive in net.drives {
                    if remainder == 0 {
                        break;
                    }
                    let accepted = disk::update(ctx.world, drive, |storage| {
                        let moved = storage.insert(&item, remainder);
                        (moved > 0).then_some(moved)
                    })
                    .unwrap_or(0);
                    remainder -= accepted;
                }
                for at in net.storage_blocks {
                    if remainder == 0 {
                        break;
                    }
                    let Some(tier) = storage_block_tier(api, ctx.world, at) else {
                        continue;
                    };
                    let accepted = disk::update_block(ctx.world, at, tier, |cell| {
                        let moved = cell.insert(&item, remainder);
                        (moved > 0).then_some(moved)
                    })
                    .unwrap_or(0);
                    remainder -= accepted;
                }
                let accepted = item.count - remainder;
                if !ctx.creative {
                    ctx.stack.shrink(accepted);
                }
                ctx.action_bar(&format!("Stored {accepted} items in network storage"));
                return BlockUseResult::Success;
            }
            if ctx.with_item {
                return BlockUseResult::TryEmptyHand;
            }
            if matches!(role, Role::CraftingGrid | Role::PatternGrid) && ctx.sneaking {
                if role == Role::PatternGrid
                    && processing::load_editor(ctx.world, ctx.position)
                        .is_some_and(|editor| editor.enabled)
                {
                    let changed = processing::update_editor(ctx.world, ctx.position, |editor| {
                        editor.toggle_side();
                        Some(())
                    })
                    .is_some();
                    ctx.action_bar(if changed {
                        "Processing Pattern input/output side toggled"
                    } else {
                        "Processing Pattern editor unavailable"
                    });
                    return BlockUseResult::Success;
                }
                let updated = craft::update(ctx.world, ctx.position, |pattern| pattern.skip());
                ctx.action_bar(if updated {
                    "Crafting grid skipped one empty ghost cell"
                } else {
                    "Crafting grid already at final cell"
                });
                return BlockUseResult::Success;
            }
            if !show(&player, ctx.world, ctx.position, 0, false) {
                ctx.action_bar("Grid unavailable: exactly one controller is required");
            }
            BlockUseResult::Success
        }
        Role::Autocrafter => {
            if let Some(item) = held {
                if item.id != "refinedstorage:pattern" {
                    return BlockUseResult::TryEmptyHand;
                }
                if item.count != 1 {
                    ctx.action_bar("Install one encoded Pattern at a time");
                    return BlockUseResult::Success;
                }
                let Some(data) = ctx.stack.data() else {
                    ctx.action_bar("This Pattern is blank; encode it at a Pattern Grid");
                    return BlockUseResult::Success;
                };
                let Some(pattern) = autocrafting::RecipePattern::from_item_data(&data) else {
                    ctx.action_bar("Invalid saved Pattern recipe");
                    return BlockUseResult::Success;
                };
                if !unique_recipe(api.crafting, ctx.world, &pattern) {
                    ctx.action_bar("Pattern recipe is missing or has a conflicting definition");
                    return BlockUseResult::Success;
                }
                let installed = autocrafting::update(ctx.world, ctx.position, |state| {
                    state.install(pattern).then_some(())
                })
                .is_some();
                if installed && !ctx.creative {
                    ctx.stack.shrink(1);
                }
                ctx.action_bar(if installed {
                    "Autocrafter Pattern installed"
                } else {
                    "Autocrafter is full or already has this Pattern"
                });
                return BlockUseResult::Success;
            }
            if ctx.with_item {
                return BlockUseResult::TryEmptyHand;
            }
            if !show(&player, ctx.world, ctx.position, 0, false) {
                ctx.action_bar("Autocrafter configuration unavailable");
            }
            BlockUseResult::Success
        }
        Role::Crafter => {
            if let Some(item) = held {
                if item.id == "refinedstorage:pattern" {
                    let Some(pattern) = crafter::InstalledPattern::from_stack(&item) else {
                        ctx.action_bar("Invalid or unsupported Refined Storage Pattern");
                        return BlockUseResult::Success;
                    };
                    if !installed_pattern_is_live(api, ctx.world, &pattern) {
                        ctx.action_bar("Pattern does not match a supported live crafting recipe");
                        return BlockUseResult::Success;
                    }
                    let installed = crafter::update(ctx.world, ctx.position, |state| {
                        state.install_pattern(pattern).then_some(())
                    })
                    .is_some();
                    if installed && !ctx.creative {
                        ctx.stack.shrink(1);
                    }
                    ctx.action_bar(if installed {
                        "Crafter Pattern installed"
                    } else {
                        "Crafter pattern inventory is full or duplicate"
                    });
                    return BlockUseResult::Success;
                }
                if item.id == "refinedstorage:speed_upgrade" {
                    let installed = crafter::update(ctx.world, ctx.position, |state| {
                        state.install_speed_upgrade().then_some(())
                    })
                    .is_some();
                    if installed && !ctx.creative {
                        ctx.stack.shrink(1);
                    }
                    ctx.action_bar(if installed {
                        "Crafter speed upgraded"
                    } else {
                        "Crafter has four speed upgrades already"
                    });
                    return BlockUseResult::Success;
                }
                return BlockUseResult::TryEmptyHand;
            }
            if ctx.with_item {
                return BlockUseResult::TryEmptyHand;
            }
            if !show(&player, ctx.world, ctx.position, 0, false) {
                ctx.action_bar("Crafter configuration unavailable");
            }
            BlockUseResult::Success
        }
        Role::CrafterManager => {
            if ctx.with_item {
                return BlockUseResult::TryEmptyHand;
            }
            if !show(&player, ctx.world, ctx.position, 0, false) {
                ctx.action_bar("Crafter Manager requires one connected Controller");
            }
            BlockUseResult::Success
        }
        Role::Importer | Role::Exporter => {
            if let Some(item) = held {
                let exporter = role == Role::Exporter;
                if ctx.sneaking {
                    let result = transfer::update(ctx.world, ctx.position, exporter, |port| {
                        let Some(index) =
                            port.filters.iter().position(|entry| entry.same_item(&item))
                        else {
                            return false;
                        };
                        port.filters.remove(index);
                        true
                    });
                    ctx.action_bar(if result {
                        "Removed transfer filter"
                    } else {
                        "Filter unchanged"
                    });
                } else if item.id == "refinedstorage:stack_upgrade"
                    || item.id == "refinedstorage:speed_upgrade"
                {
                    let Some(upgrade) = transfer::Upgrade::from_item(&item.id) else {
                        return BlockUseResult::Fail;
                    };
                    let changed = transfer::update(ctx.world, ctx.position, exporter, |port| {
                        port.install(upgrade)
                    });
                    if changed && !ctx.creative {
                        ctx.stack.shrink(1);
                    }
                    ctx.action_bar(if changed {
                        "Transfer speed upgraded"
                    } else {
                        "Already at maximum speed"
                    });
                } else {
                    let changed =
                        transfer::update(ctx.world, ctx.position, exporter, |port| port.add(&item));
                    ctx.action_bar(if changed {
                        "Filter ghost item configured"
                    } else {
                        "Filter already present or full"
                    });
                }
                return BlockUseResult::Success;
            }
            if ctx.with_item {
                return BlockUseResult::TryEmptyHand;
            }
            let _ = show(&player, ctx.world, ctx.position, 0, false);
            BlockUseResult::Success
        }
        Role::Controller => {
            if let Some(item) = held {
                if item.id == "refinedstorage:wireless_grid" {
                    if item.count != 1 {
                        ctx.action_bar("Bind one Wireless Grid at a time");
                        return BlockUseResult::Success;
                    }
                    let Some(dim) = ctx.world.dimension_id() else {
                        return BlockUseResult::Fail;
                    };
                    let Some(binding) = wireless::bind(dim, ctx.position) else {
                        return BlockUseResult::Fail;
                    };
                    if ctx.stack.set_data(&binding) {
                        ctx.action_bar("Wireless Grid bound to this Controller");
                    } else {
                        ctx.action_bar("Wireless Grid binding unavailable");
                    }
                    return BlockUseResult::Success;
                }
            }
            if !ctx.with_item {
                if !show(&player, ctx.world, ctx.position, 0, false) {
                    ctx.action_bar("Controller network incomplete or conflicting");
                }
                BlockUseResult::Success
            } else {
                BlockUseResult::Pass
            }
        }
        Role::External => {
            if let Some(item) = held {
                let changed = transfer::update_external(ctx.world, ctx.position, |policy| {
                    if ctx.sneaking {
                        let Some(index) = policy
                            .filters
                            .iter()
                            .position(|entry| entry.same_item(&item))
                        else {
                            return false;
                        };
                        policy.filters.remove(index);
                        true
                    } else {
                        policy.add(&item)
                    }
                });
                ctx.action_bar(if changed {
                    "External Storage item filter changed"
                } else {
                    "External Storage item filter unchanged or full"
                });
                return BlockUseResult::Success;
            }
            if ctx.with_item {
                return BlockUseResult::TryEmptyHand;
            }
            if !show(&player, ctx.world, ctx.position, 0, false) {
                ctx.action_bar("External Storage configuration unavailable");
            }
            BlockUseResult::Success
        }
        Role::Storage => {
            let Some(tier) = disk::Tier::from_block(definition.id) else {
                return BlockUseResult::Fail;
            };
            if let Some(item) = held {
                let accepted = disk::update_block(ctx.world, ctx.position, tier, |cell| {
                    let moved = cell.insert(&item, item.count);
                    (moved > 0).then_some(moved)
                })
                .unwrap_or(0);
                if !ctx.creative {
                    ctx.stack.shrink(accepted);
                }
                ctx.action_bar(&format!(
                    "Stored {accepted} items inside this Storage Block"
                ));
                return BlockUseResult::Success;
            }
            let Some(cell) = disk::load_block(ctx.world, ctx.position, tier) else {
                return BlockUseResult::Fail;
            };
            ctx.action_bar(&format!(
                "{} / {} items inside Storage Block",
                cell.used(),
                tier.capacity()
            ));
            BlockUseResult::Success
        }
        Role::Wireless => {
            if let Some(item) = held {
                if item.id == "refinedstorage:range_upgrade" {
                    let changed = wireless::update(ctx.world, ctx.position, |transmitter| {
                        transmitter.install()
                    });
                    if changed && !ctx.creative {
                        ctx.stack.shrink(1);
                    }
                    ctx.action_bar(if changed {
                        "Wireless Transmitter range upgraded"
                    } else {
                        "Wireless Transmitter is at maximum range"
                    });
                    return BlockUseResult::Success;
                }
                return BlockUseResult::TryEmptyHand;
            }
            let range = wireless::load(ctx.world, ctx.position)
                .map_or(0, |transmitter| transmitter.range());
            ctx.action_bar(&format!("Wireless Transmitter range: {range} blocks"));
            BlockUseResult::Success
        }
        Role::Cable => BlockUseResult::Pass,
    }
}

fn use_wireless(ctx: &mut UseItem<'_>) -> UseResult {
    if ctx.item != "refinedstorage:wireless_grid" {
        return UseResult::Pass;
    }
    let Some(player) = ctx.player.and_then(|entity| entity.as_player()) else {
        return UseResult::Fail;
    };
    let Some((dimension, root)) = ctx.stack.data().as_deref().and_then(wireless::bound) else {
        moddedmc_api::info("refinedstorage: use Wireless Grid on a Controller to bind it");
        return UseResult::Success;
    };
    if player.dimension() != dimension || ctx.world.dimension_id() != Some(dimension.as_str()) {
        moddedmc_api::info("refinedstorage: Wireless Grid bound to a different dimension");
        return UseResult::Success;
    }
    if show(&player, ctx.world, root, 0, true) {
        UseResult::Consume
    } else {
        moddedmc_api::info(
            "refinedstorage: controller unavailable or outside powered transmitter range",
        );
        UseResult::Success
    }
}

/// Preserve each installed disk's item and full digital contents when a
/// survival player mines a Drive or a built-in Storage Block. The block item
/// itself is dropped by vanilla; only stored items need special recovery.
fn destroyed(ctx: &PlayerWillDestroy<'_>) {
    if ctx.creative {
        return;
    }
    let Some(api) = RUNTIME.get().copied() else {
        return;
    };
    let Some(role) = blocks::node(api.content, ctx.state).map(|node| node.role) else {
        return;
    };
    if !matches!(
        role,
        Role::Drive | Role::Storage | Role::Autocrafter | Role::Crafter
    ) || !current_block(api, ctx.world, ctx.position, role)
    {
        return;
    }
    let Ok(_guard) = OPERATIONS.lock() else {
        return;
    };
    if role == Role::Crafter {
        let Some((patterns, upgrades)) = crafter::update(ctx.world, ctx.position, |state| {
            let patterns = std::mem::replace(&mut state.patterns, vec![None; 9]);
            let upgrades = std::mem::replace(&mut state.speed_upgrades, 0);
            Some((patterns, upgrades))
        }) else {
            return;
        };
        for (index, pattern) in patterns.into_iter().enumerate() {
            let Some(pattern) = pattern else { continue };
            let Some(bytes) = pattern.item_stack() else {
                moddedmc_api::error(
                    "refinedstorage: could not encode Crafter Pattern during break",
                );
                continue;
            };
            let delivered = api
                .stacks
                .spawn(
                    ctx.world,
                    [
                        f64::from(ctx.position.x) + 0.5,
                        f64::from(ctx.position.y) + 0.7,
                        f64::from(ctx.position.z) + 0.5,
                    ],
                    [0., 0.05, 0.],
                    &bytes,
                )
                .is_some();
            if !delivered
                && crafter::update(ctx.world, ctx.position, |state| {
                    state.restore_pattern(index, pattern).then_some(())
                })
                .is_none()
            {
                moddedmc_api::error(
                    "refinedstorage: failed to restore undelivered Crafter Pattern after break",
                );
            }
        }
        for _ in 0..upgrades {
            let Some(bytes) = indicator("refinedstorage:speed_upgrade") else {
                continue;
            };
            let delivered = api
                .stacks
                .spawn(
                    ctx.world,
                    [
                        f64::from(ctx.position.x) + 0.5,
                        f64::from(ctx.position.y) + 0.7,
                        f64::from(ctx.position.z) + 0.5,
                    ],
                    [0., 0.05, 0.],
                    &bytes,
                )
                .is_some();
            if !delivered
                && crafter::update(ctx.world, ctx.position, |state| {
                    state.install_speed_upgrade().then_some(())
                })
                .is_none()
            {
                moddedmc_api::error(
                    "refinedstorage: failed to restore undelivered Crafter speed upgrade after break",
                );
            }
        }
        return;
    }
    if role == Role::Autocrafter {
        let Some(jobs) = autocrafting::update(ctx.world, ctx.position, |state| {
            if state.jobs.is_empty() {
                return None;
            }
            Some(std::mem::take(&mut state.jobs))
        }) else {
            return;
        };
        for job in jobs {
            let Some(bytes) = job.pattern.item_stack() else {
                moddedmc_api::error(
                    "refinedstorage: could not encode Pattern during Autocrafter break",
                );
                continue;
            };
            if api
                .stacks
                .spawn(
                    ctx.world,
                    [
                        f64::from(ctx.position.x) + 0.5,
                        f64::from(ctx.position.y) + 0.7,
                        f64::from(ctx.position.z) + 0.5,
                    ],
                    [0.0, 0.05, 0.0],
                    &bytes,
                )
                .is_none()
            {
                // The block is about to be removed by vanilla. Preserve the
                // original saved state for any external recovery system even
                // though this callback cannot veto the break.
                if autocrafting::update(ctx.world, ctx.position, |state| {
                    if state.jobs.len() >= 9
                        || state
                            .jobs
                            .iter()
                            .any(|saved| saved.pattern.same_recipe(&job.pattern))
                    {
                        return None;
                    }
                    state.jobs.push(job);
                    Some(())
                })
                .is_none()
                {
                    moddedmc_api::error(
                        "refinedstorage: Autocrafter Pattern could not be restored after failed break drop",
                    );
                }
            }
        }
        return;
    }
    if role == Role::Storage {
        let Some(tier) = storage_block_tier(api, ctx.world, ctx.position) else {
            return;
        };
        let Some(contents) = disk::update_block(ctx.world, ctx.position, tier, |cell| {
            if cell.used() == 0 {
                return None;
            }
            Some(std::mem::replace(cell, disk::Disk::empty(tier)))
        }) else {
            return;
        };
        let encoded = disk::cartridge_item(&contents);
        let delivered = encoded.as_ref().is_some_and(|bytes| {
            api.stacks
                .spawn(
                    ctx.world,
                    [
                        f64::from(ctx.position.x) + 0.5,
                        f64::from(ctx.position.y) + 0.7,
                        f64::from(ctx.position.z) + 0.5,
                    ],
                    [0.0, 0.05, 0.0],
                    bytes,
                )
                .is_some()
        });
        if !delivered
            && disk::update_block(ctx.world, ctx.position, tier, |cell| {
                if cell.used() != 0 {
                    return None;
                }
                *cell = contents;
                Some(())
            })
            .is_none()
        {
            moddedmc_api::error(
                "refinedstorage: Storage Block contents could not be restored after failed drop",
            );
        }
        return;
    }
    for index in 0..disk::DRIVE_SLOTS {
        let Some(disk) = disk::update(ctx.world, ctx.position, |drive| drive.eject(index)) else {
            continue;
        };
        let Some(encoded) = disk::cartridge_item(&disk) else {
            moddedmc_api::error("refinedstorage: disk content could not be encoded on break");
            if disk::update(ctx.world, ctx.position, |drive| {
                if !drive.disks.get(index).is_some_and(Option::is_none) {
                    return None;
                }
                drive.disks[index] = Some(disk);
                Some(())
            })
            .is_none()
            {
                moddedmc_api::error("refinedstorage: failed to restore unencoded break disk");
            }
            continue;
        };
        if api
            .stacks
            .spawn(
                ctx.world,
                [
                    f64::from(ctx.position.x) + 0.5,
                    f64::from(ctx.position.y) + 0.7,
                    f64::from(ctx.position.z) + 0.5,
                ],
                [0.0, 0.05, 0.0],
                &encoded,
            )
            .is_none()
        {
            if disk::update(ctx.world, ctx.position, |drive| {
                if !drive.disks.get(index).is_some_and(Option::is_none) {
                    return None;
                }
                drive.disks[index] = Some(disk);
                Some(())
            })
            .is_none()
            {
                moddedmc_api::error(
                    "refinedstorage: could not restore disk after failed break drop",
                );
            }
        }
    }
}

fn action(ctx: Action<'_>) {
    let Some(api) = RUNTIME.get().copied() else {
        return;
    };
    let Some(session) = SESSIONS
        .lock()
        .ok()
        .and_then(|all| all.get(&ctx.inventory).cloned())
    else {
        return;
    };
    if session.owner != ctx.player.name()
        || moddedmc_api::tick_count().saturating_sub(session.created) > SESSION_TTL
        || !(if session.remote {
            wireless_access(api, ctx.world, &ctx.player, session.site)
        } else {
            within_reach(&ctx.player, &session.dimension, session.site)
        })
    {
        return;
    }
    let Ok(_guard) = OPERATIONS.lock() else {
        return;
    };
    let Some(snapshot) = ctx.world.entity_snapshot(session.site) else {
        return;
    };
    if snapshot.state != session.state
        || snapshot.incarnation != session.incarnation
        || snapshot.nbt != session.snapshot
        || !current_block(api, ctx.world, session.site, session.role)
    {
        return;
    }
    match session.role {
        Role::Drive => {
            let Some(index) = ctx
                .tag
                .checked_sub(EJECT_DISK)
                .and_then(|index| usize::try_from(index).ok())
            else {
                return;
            };
            if index >= disk::DRIVE_SLOTS {
                return;
            }
            let removed = disk::update(ctx.world, session.site, |storage| storage.eject(index));
            let Some(disk) = removed else { return };
            let Some(item) = disk::cartridge_item(&disk) else {
                moddedmc_api::error("refinedstorage: could not encode ejected storage disk");
                return;
            };
            let pos = ctx.player.position();
            if api
                .stacks
                .spawn(
                    ctx.world,
                    [pos.x, pos.y + 0.4, pos.z],
                    [0.0, 0.07, 0.0],
                    &item,
                )
                .is_none()
            {
                if disk::update(ctx.world, session.site, |drive| {
                    if drive.disks.get(index).is_none_or(Option::is_some) {
                        return None;
                    }
                    drive.disks[index] = Some(disk);
                    Some(())
                })
                .is_none()
                {
                    moddedmc_api::error("refinedstorage: could not restore undelivered disk");
                }
                return;
            }
            ctx.player
                .action_bar("Storage disk ejected with its full contents");
            let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
        }
        Role::Grid | Role::CraftingGrid | Role::PatternGrid | Role::Controller => {
            if matches!(ctx.tag, PREVIOUS | NEXT) {
                let page = if ctx.tag == PREVIOUS {
                    session.page.saturating_sub(1)
                } else {
                    session.page + 1
                };
                let _ = show(&ctx.player, ctx.world, session.site, page, session.remote);
                return;
            }
            if session.role == Role::PatternGrid {
                let changed = match ctx.tag {
                    PROCESSING_TOGGLE_MODE => {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor.enabled = !editor.enabled;
                            Some(())
                        })
                        .is_some()
                    }
                    PROCESSING_TOGGLE_EXACT => {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor.toggle_exact();
                            Some(())
                        })
                        .is_some()
                    }
                    PROCESSING_TOGGLE_SIDE => {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor.toggle_side();
                            Some(())
                        })
                        .is_some()
                    }
                    PROCESSING_TOGGLE_PROCESSING => {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor.toggle_processing();
                            Some(())
                        })
                        .is_some()
                    }
                    PROCESSING_CLEAR_INPUTS => {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor.clear_inputs();
                            Some(())
                        })
                        .is_some()
                    }
                    PROCESSING_CLEAR_OUTPUTS => {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor.clear_outputs();
                            Some(())
                        })
                        .is_some()
                    }
                    tag if (PROCESSING_INPUT_SELECT..PROCESSING_INPUT_SELECT + 9)
                        .contains(&tag) =>
                    {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor
                                .select_input((tag - PROCESSING_INPUT_SELECT) as usize)
                                .then_some(())
                        })
                        .is_some()
                    }
                    tag if (PROCESSING_OUTPUT_SELECT..PROCESSING_OUTPUT_SELECT + 9)
                        .contains(&tag) =>
                    {
                        processing::update_editor(ctx.world, session.site, |editor| {
                            editor
                                .select_output((tag - PROCESSING_OUTPUT_SELECT) as usize)
                                .then_some(())
                        })
                        .is_some()
                    }
                    _ => false,
                };
                if changed {
                    let _ = show(
                        &ctx.player,
                        ctx.world,
                        session.site,
                        session.page,
                        session.remote,
                    );
                    return;
                }
            }
            if matches!(session.role, Role::CraftingGrid | Role::PatternGrid)
                && (PATTERN_SELECT..PATTERN_SELECT + 9).contains(&ctx.tag)
            {
                if craft::update(ctx.world, session.site, |pattern| {
                    pattern.select((ctx.tag - PATTERN_SELECT) as usize)
                }) {
                    ctx.player
                        .action_bar("Selected crafting cell; sneak-use a held item to assign it");
                    let _ = show(
                        &ctx.player,
                        ctx.world,
                        session.site,
                        session.page,
                        session.remote,
                    );
                }
                return;
            }
            if matches!(session.role, Role::CraftingGrid | Role::PatternGrid)
                && ctx.tag == CYCLE_RECIPE
            {
                let Some(pattern) = craft::load(ctx.world, session.site) else {
                    return;
                };
                let Some(choices) = matching_recipes(api.crafting, ctx.world, &pattern) else {
                    return;
                };
                if craft::update(ctx.world, session.site, |saved| {
                    saved.cycle_recipe(&choices)
                }) {
                    let _ = show(
                        &ctx.player,
                        ctx.world,
                        session.site,
                        session.page,
                        session.remote,
                    );
                }
                return;
            }
            if matches!(session.role, Role::CraftingGrid | Role::PatternGrid)
                && ctx.tag == CLEAR_PATTERN
            {
                if craft::update(ctx.world, session.site, |pattern| {
                    pattern.clear();
                    true
                }) {
                    let _ = show(
                        &ctx.player,
                        ctx.world,
                        session.site,
                        session.page,
                        session.remote,
                    );
                }
                return;
            }
            let Ok(net) = graph(api, ctx.world, session.site) else {
                return;
            };
            if session.role == Role::CraftingGrid && ctx.tag == CRAFT_RECIPE {
                let Some(pattern) = craft::load(ctx.world, session.site) else {
                    return;
                };
                let Some(recipe) = selected_matching_recipe(api.crafting, ctx.world, &pattern)
                else {
                    ctx.player
                        .action_bar("No live crafting recipe matches this pattern");
                    return;
                };
                let Some(result) = Stack::parse(&recipe.result) else {
                    return;
                };
                let Some(remainders) = pattern.remainders(|item| api.crafting.remainder(item))
                else {
                    return;
                };
                // All inputs and outputs are committed as a single drive NBT CAS.
                // A recipe split across drives cannot safely be consumed until
                // the host supplies an atomic multi-storage craft transaction.
                for drive in net.drives {
                    if disk::update(ctx.world, drive, |storage| {
                        storage
                            .craft(&pattern.slots, &result, &remainders)
                            .then_some(())
                    })
                    .is_some()
                    {
                        ctx.player
                            .action_bar("Crafted recipe using network disk ingredients");
                        let _ = show(
                            &ctx.player,
                            ctx.world,
                            session.site,
                            session.page,
                            session.remote,
                        );
                        return;
                    }
                }
                for at in net.storage_blocks {
                    let Some(tier) = storage_block_tier(api, ctx.world, at) else {
                        continue;
                    };
                    if disk::update_block(ctx.world, at, tier, |cell| {
                        cell.craft(&pattern.slots, &result, &remainders)
                            .then_some(())
                    })
                    .is_some()
                    {
                        ctx.player
                            .action_bar("Crafted recipe from a connected Storage Block");
                        let _ = show(
                            &ctx.player,
                            ctx.world,
                            session.site,
                            session.page,
                            session.remote,
                        );
                        return;
                    }
                }
                ctx.player
                    .action_bar("No single disk or Storage Block can atomically craft this recipe");
                return;
            }
            let Some(index) = ctx
                .tag
                .checked_sub(REQUEST)
                .and_then(|index| usize::try_from(index).ok())
            else {
                return;
            };
            let processing_mode = session.role == Role::PatternGrid
                && processing::load_editor(ctx.world, session.site)
                    .is_some_and(|editor| editor.enabled);
            let per_page = if processing_mode {
                27
            } else if matches!(session.role, Role::CraftingGrid | Role::PatternGrid) {
                36
            } else {
                PER_PAGE
            };
            if index >= per_page {
                return;
            }
            let Some(record) = session.requests.get(session.page * per_page + index) else {
                return;
            };
            let count = record.total.min(64) as u32;
            for drive in net.drives {
                let withdrawn = disk::update(ctx.world, drive, |storage| {
                    let taken = storage.withdraw(&record.sample, count);
                    (taken > 0).then_some(taken)
                });
                let Some(taken) = withdrawn else { continue };
                let Some(bytes) = record.sample.with_count(taken) else {
                    return;
                };
                let p = ctx.player.position();
                if api
                    .stacks
                    .spawn(ctx.world, [p.x, p.y + 0.4, p.z], [0., 0.07, 0.], &bytes)
                    .is_none()
                {
                    if disk::update(ctx.world, drive, |storage| {
                        (storage.insert(&record.sample, taken) == taken).then_some(())
                    })
                    .is_none()
                    {
                        moddedmc_api::error(
                            "refinedstorage: could not restore failed Grid delivery",
                        );
                    }
                    return;
                }
                ctx.player
                    .action_bar(&format!("Requested {taken} {}", record.sample.id));
                let _ = show(
                    &ctx.player,
                    ctx.world,
                    session.site,
                    session.page,
                    session.remote,
                );
                return;
            }
            for at in net.storage_blocks {
                let Some(tier) = storage_block_tier(api, ctx.world, at) else {
                    continue;
                };
                let withdrawn = disk::update_block(ctx.world, at, tier, |cell| {
                    let taken = cell.withdraw(&record.sample, count);
                    (taken > 0).then_some(taken)
                });
                let Some(taken) = withdrawn else { continue };
                let Some(bytes) = record.sample.with_count(taken) else {
                    return;
                };
                let p = ctx.player.position();
                if api
                    .stacks
                    .spawn(ctx.world, [p.x, p.y + 0.4, p.z], [0., 0.07, 0.], &bytes)
                    .is_none()
                {
                    if disk::update_block(ctx.world, at, tier, |cell| {
                        (cell.insert(&record.sample, taken) == taken).then_some(())
                    })
                    .is_none()
                    {
                        moddedmc_api::error(
                            "refinedstorage: Storage Block request delivery could not be restored",
                        );
                    }
                    return;
                }
                ctx.player
                    .action_bar(&format!("Requested {taken} {}", record.sample.id));
                let _ = show(
                    &ctx.player,
                    ctx.world,
                    session.site,
                    session.page,
                    session.remote,
                );
                return;
            }
            ctx.player
                .action_bar("Item is only in external storage; direct player transfer unavailable");
        }
        Role::Crafter => {
            if ctx.tag == CRAFTER_MODE {
                if crafter::update(ctx.world, session.site, |state| {
                    state.set_mode(state.mode.next());
                    Some(())
                })
                .is_some()
                {
                    let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                }
                return;
            }
            if let Some(index) = ctx
                .tag
                .checked_sub(CRAFTER_EJECT_PATTERN)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < 9)
            {
                let Some(pattern) = crafter::load(ctx.world, session.site)
                    .and_then(|state| state.patterns.get(index).cloned().flatten())
                else {
                    return;
                };
                let Some(bytes) = pattern.item_stack() else {
                    return;
                };
                let Some(removed) =
                    crafter::update(ctx.world, session.site, |state| state.eject_pattern(index))
                else {
                    return;
                };
                let position = ctx.player.position();
                if api
                    .stacks
                    .spawn(
                        ctx.world,
                        [position.x, position.y + 0.4, position.z],
                        [0., 0.08, 0.],
                        &bytes,
                    )
                    .is_none()
                {
                    if crafter::update(ctx.world, session.site, |state| {
                        state.restore_pattern(index, removed).then_some(())
                    })
                    .is_none()
                    {
                        moddedmc_api::error(
                            "refinedstorage: failed to restore undelivered Crafter Pattern",
                        );
                    }
                    return;
                }
                let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                return;
            }
            if let Some(index) = ctx
                .tag
                .checked_sub(CRAFTER_EJECT_UPGRADE)
                .and_then(|index| usize::try_from(index).ok())
                .filter(|index| *index < 4)
            {
                let Some(state) = crafter::load(ctx.world, session.site) else {
                    return;
                };
                if index >= state.speed_upgrades as usize {
                    return;
                }
                if crafter::update(ctx.world, session.site, |state| {
                    state.remove_speed_upgrade().then_some(())
                })
                .is_none()
                {
                    return;
                }
                let bytes = indicator("refinedstorage:speed_upgrade");
                let position = ctx.player.position();
                let delivered = bytes
                    .as_ref()
                    .and_then(|item| {
                        api.stacks.spawn(
                            ctx.world,
                            [position.x, position.y + 0.4, position.z],
                            [0., 0.08, 0.],
                            item,
                        )
                    })
                    .is_some();
                if !delivered
                    && crafter::update(ctx.world, session.site, |state| {
                        state.install_speed_upgrade().then_some(())
                    })
                    .is_none()
                {
                    moddedmc_api::error(
                        "refinedstorage: failed to restore undelivered Crafter speed upgrade",
                    );
                }
                let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                return;
            }
        }
        Role::CrafterManager => {
            let changed = match ctx.tag {
                CRAFTER_MANAGER_SIZE => manager::update(ctx.world, session.site, |state| {
                    state.cycle_size();
                    true
                }),
                CRAFTER_MANAGER_SEARCH => manager::update(ctx.world, session.site, |state| {
                    state.cycle_search_box_mode();
                    true
                }),
                _ => false,
            };
            if changed {
                let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
            }
        }
        Role::Autocrafter => {
            if ctx.tag == AUTO_POWER {
                if autocrafting::update(ctx.world, session.site, |state| {
                    state.needs_signal = !state.needs_signal;
                    Some(())
                })
                .is_some()
                {
                    let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                }
            } else if (AUTO_QUEUE..AUTO_QUEUE + 9).contains(&ctx.tag) {
                let index = (ctx.tag - AUTO_QUEUE) as usize;
                let queued = autocrafting::update(ctx.world, session.site, |state| {
                    state.queue(index).then_some(())
                })
                .is_some();
                if queued {
                    ctx.player.action_bar("Autocrafter recipe queued");
                    let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                } else {
                    ctx.player
                        .action_bar("Autocrafter job queue full or unavailable");
                }
            } else if (AUTO_DEQUEUE..AUTO_DEQUEUE + 9).contains(&ctx.tag) {
                let index = (ctx.tag - AUTO_DEQUEUE) as usize;
                if autocrafting::update(ctx.world, session.site, |state| {
                    state.dequeue(index).then_some(())
                })
                .is_some()
                {
                    ctx.player
                        .action_bar("One unstarted Autocrafter request cancelled");
                    let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                }
            } else if (AUTO_EJECT..AUTO_EJECT + 9).contains(&ctx.tag) {
                let index = (ctx.tag - AUTO_EJECT) as usize;
                let Some(pattern) = autocrafting::load(ctx.world, session.site)
                    .and_then(|state| state.jobs.get(index).cloned())
                    .filter(|job| job.pending == 0)
                    .map(|job| job.pattern)
                else {
                    return;
                };
                let Some(saved_item) = pattern.item_stack() else {
                    ctx.player
                        .action_bar("Pattern item data exceeds the host limit");
                    return;
                };
                let removed = autocrafting::update(ctx.world, session.site, |state| {
                    let saved = state.jobs.get(index)?;
                    if saved.pending > 0 || !saved.pattern.same_recipe(&pattern) {
                        return None;
                    }
                    state.eject(index)
                });
                let Some(taken) = removed else { return };
                let p = ctx.player.position();
                if api
                    .stacks
                    .spawn(
                        ctx.world,
                        [p.x, p.y + 0.4, p.z],
                        [0., 0.08, 0.],
                        &saved_item,
                    )
                    .is_none()
                {
                    if autocrafting::update(ctx.world, session.site, |state| {
                        if state.jobs.len() >= 9
                            || state.jobs.iter().any(|job| job.pattern.same_recipe(&taken))
                        {
                            return None;
                        }
                        state.jobs.insert(
                            index.min(state.jobs.len()),
                            autocrafting::Job {
                                pattern: taken,
                                pending: 0,
                                inflight: false,
                            },
                        );
                        Some(())
                    })
                    .is_none()
                    {
                        moddedmc_api::error(
                            "refinedstorage: unable to restore undelivered Pattern after eject",
                        );
                    }
                    return;
                }
                ctx.player.action_bar("Installed Pattern returned");
                let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
            } else if (AUTO_ABORT..AUTO_ABORT + 9).contains(&ctx.tag) {
                let index = (ctx.tag - AUTO_ABORT) as usize;
                if autocrafting::update(ctx.world, session.site, |state| {
                    state.cancel_uncertain(index).then_some(())
                })
                .is_some()
                {
                    ctx.player.action_bar(
                        "Uncertain Autocrafter job cancelled; previous output is preserved",
                    );
                    let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
                }
            }
        }
        Role::Importer | Role::Exporter => {
            let exporter = session.role == Role::Exporter;
            let changed = match ctx.tag {
                PORT_TOGGLE => transfer::update(ctx.world, session.site, exporter, |port| {
                    port.allowlist = !port.allowlist;
                    true
                }),
                PORT_POWER => transfer::update(ctx.world, session.site, exporter, |port| {
                    port.needs_signal = !port.needs_signal;
                    true
                }),
                PORT_CLEAR => transfer::update(ctx.world, session.site, exporter, |port| {
                    if port.filters.is_empty() {
                        return false;
                    }
                    port.filters.clear();
                    true
                }),
                tag if (REMOVE_FILTER..REMOVE_FILTER + 9).contains(&tag) => {
                    transfer::update(ctx.world, session.site, exporter, |port| {
                        let index = (tag - REMOVE_FILTER) as usize;
                        if index >= port.filters.len() {
                            return false;
                        }
                        port.filters.remove(index);
                        true
                    })
                }
                _ => false,
            };
            if changed {
                let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
            }
        }
        Role::External => {
            let changed = match ctx.tag {
                EXTERNAL_ALLOW => transfer::update_external(ctx.world, session.site, |policy| {
                    policy.allowlist = !policy.allowlist;
                    true
                }),
                EXTERNAL_INSERT => transfer::update_external(ctx.world, session.site, |policy| {
                    policy.can_insert = !policy.can_insert;
                    true
                }),
                EXTERNAL_EXTRACT => transfer::update_external(ctx.world, session.site, |policy| {
                    policy.can_extract = !policy.can_extract;
                    true
                }),
                EXTERNAL_PRIORITY_UP | EXTERNAL_PRIORITY_DOWN => {
                    transfer::update_external(ctx.world, session.site, |policy| {
                        let priority = if ctx.tag == EXTERNAL_PRIORITY_UP {
                            policy.priority.saturating_add(1).min(1000)
                        } else {
                            policy.priority.saturating_sub(1).max(-1000)
                        };
                        if priority == policy.priority {
                            return false;
                        }
                        policy.priority = priority;
                        true
                    })
                }
                EXTERNAL_POWER => transfer::update_external(ctx.world, session.site, |policy| {
                    policy.needs_signal = !policy.needs_signal;
                    true
                }),
                EXTERNAL_CLEAR => transfer::update_external(ctx.world, session.site, |policy| {
                    if policy.filters.is_empty() {
                        return false;
                    }
                    policy.filters.clear();
                    true
                }),
                tag if (REMOVE_FILTER..REMOVE_FILTER + 9).contains(&tag) => {
                    transfer::update_external(ctx.world, session.site, |policy| {
                        let index = (tag - REMOVE_FILTER) as usize;
                        if index >= policy.filters.len() {
                            return false;
                        }
                        policy.filters.remove(index);
                        true
                    })
                }
                _ => false,
            };
            if changed {
                let _ = show(&ctx.player, ctx.world, session.site, 0, session.remote);
            }
        }
        _ => {}
    }
}

/// Process one real queued recipe per Autocrafter tick. The pending job is
/// marked in-flight and saved *before* any storage change. This prevents an
/// uncertain crash from automatically replaying an already-produced result.
/// It is a conservative stop, not a cross-block crash-durable transaction.
fn run_autocrafter(api: Runtime, world: World<'_>, site: Pos, net: &Network) {
    let Some(state) = autocrafting::load(world, site) else {
        return;
    };
    if !state.enabled(world.has_neighbor_signal(site)) {
        return;
    }
    for (index, job) in state.jobs.iter().enumerate() {
        if job.pending == 0 || job.inflight || !unique_recipe(api.crafting, world, &job.pattern) {
            continue;
        }
        let Some(recipe) = selected_matching_recipe(api.crafting, world, &job.pattern.grid) else {
            continue;
        };
        if !job.pattern.matches(&recipe) {
            continue;
        }
        let Some(remainders) = job
            .pattern
            .grid
            .remainders(|ingredient| api.crafting.remainder(ingredient))
        else {
            continue;
        };
        if autocrafting::update(world, site, |saved| {
            saved.begin(index, &job.pattern).then_some(())
        })
        .is_none()
        {
            continue;
        }
        let mut committed = false;
        for &drive in &net.drives {
            if disk::update(world, drive, |storage| {
                storage
                    .craft(&job.pattern.grid.slots, &job.pattern.result, &remainders)
                    .then_some(())
            })
            .is_some()
            {
                committed = true;
                break;
            }
        }
        if !committed {
            for &block in &net.storage_blocks {
                let Some(tier) = storage_block_tier(api, world, block) else {
                    continue;
                };
                if disk::update_block(world, block, tier, |cell| {
                    cell.craft(&job.pattern.grid.slots, &job.pattern.result, &remainders)
                        .then_some(())
                })
                .is_some()
                {
                    committed = true;
                    break;
                }
            }
        }
        if committed {
            if autocrafting::update(world, site, |saved| {
                saved.finish(index, &job.pattern).then_some(())
            })
            .is_none()
            {
                moddedmc_api::error(
                    "refinedstorage: recipe stored, but Autocrafter completion checkpoint failed; job left uncertain",
                );
            }
        } else if autocrafting::update(world, site, |saved| {
            saved
                .abandon_without_mutation(index, &job.pattern)
                .then_some(())
        })
        .is_none()
        {
            moddedmc_api::error(
                "refinedstorage: no recipe committed, but Autocrafter in-flight flag could not be cleared",
            );
        }
        return;
    }
}

fn tick(world: World<'_>, now: u64, events: &[ContentEvent]) {
    let Some(api) = RUNTIME.get().copied() else {
        return;
    };
    let Some(dim) = world.dimension_id() else {
        return;
    };
    let sites = {
        let Ok(mut index) = PORTS.lock() else { return };
        let current = index.entry(dim.to_string()).or_default();
        for event in events {
            current.remove(&event.position);
            if event.reason != content::UNLOADED
                && event.reason != content::MOVED_STATE_CHANGED
                && blocks::node(api.content, event.state).is_some_and(|node| {
                    matches!(
                        node.role,
                        Role::Importer | Role::Exporter | Role::Autocrafter | Role::Crafter
                    )
                })
            {
                current.insert(event.position, now);
            }
        }
        if current.is_empty() {
            return;
        }
        let mut positions: Vec<_> = current.keys().copied().collect();
        positions.sort_by_key(|site| (site.x, site.y, site.z));
        let start = ((now as usize).wrapping_mul(64)) % positions.len();
        positions
            .iter()
            .cycle()
            .skip(start)
            .take(positions.len().min(64))
            .copied()
            .collect::<Vec<_>>()
    };
    let Ok(_guard) = OPERATIONS.try_lock() else {
        return;
    };
    let mut networks = Vec::<Network>::new();
    for site in sites {
        let mut state = [content::UNLOADED_STATE];
        if !world.read(&[site], &mut state) {
            continue;
        }
        let Some((definition, face)) = blocks::kind(api.content, state[0]) else {
            continue;
        };
        if !matches!(
            definition.role,
            Role::Importer | Role::Exporter | Role::Autocrafter | Role::Crafter
        ) {
            continue;
        }
        let policy = if matches!(definition.role, Role::Autocrafter | Role::Crafter) {
            None
        } else {
            let Some(policy) = transfer::load(world, site, definition.role == Role::Exporter)
            else {
                continue;
            };
            if !policy.enabled(world.has_neighbor_signal(site)) {
                continue;
            }
            Some(policy)
        };
        let due = {
            let Ok(mut current) = PORTS.lock() else {
                continue;
            };
            let Some(next) = current
                .get_mut(dim)
                .and_then(|positions| positions.get_mut(&site))
            else {
                continue;
            };
            if now < *next {
                false
            } else {
                *next = now.saturating_add(if definition.role == Role::Crafter {
                    1
                } else {
                    10
                });
                true
            }
        };
        if !due {
            continue;
        }
        if definition.role == Role::Crafter {
            let powered = world.has_neighbor_signal(site);
            let _ = crafter::update(world, site, |state| {
                let before = (state.locked, state.was_powered);
                state.update_power(powered);
                ((state.locked, state.was_powered) != before).then_some(())
            });
            continue;
        }
        let index = if let Some(index) = networks
            .iter()
            .position(|net| net.nodes.contains_key(&site))
        {
            index
        } else {
            let Ok(net) = graph(api, world, site) else {
                continue;
            };
            networks.push(net);
            networks.len() - 1
        };
        let net = &networks[index];
        if definition.role == Role::Autocrafter {
            run_autocrafter(api, world, site, net);
            continue;
        }
        let Some(policy) = policy else {
            continue;
        };
        let Some(attached) = transfer::endpoint(site, face) else {
            continue;
        };
        let external = transfer::external_connections(world, net);
        if external.is_empty() {
            continue;
        }
        let mut io = transfer::Live {
            containers: api.containers,
            transactions: api.transfers,
            world,
        };
        if definition.role == Role::Importer {
            let _ = transfer::route_configured(&mut io, attached, &external, &policy, None);
        } else {
            for (source, external_policy) in external {
                if transfer::route_configured(
                    &mut io,
                    source,
                    &[(attached, transfer::External::default())],
                    &policy,
                    Some(&external_policy),
                ) > 0
                {
                    break;
                }
            }
        }
    }
}

moddedmc_api::export_mod!(RefinedStorage);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stock_menus_keep_control_actions_outside_catalogue_pages() {
        assert_eq!(MENU_SLOTS, 54);
        assert!(PER_PAGE < MENU_SLOTS);
        assert!((REQUEST + 44) != CRAFT_RECIPE);
        assert!(EJECT_DISK + disk::DRIVE_SLOTS as u64 <= REQUEST);
    }
}
