//! Partial native ports of Nature's Compass 3.3.1 and Explorer's Compass 3.3.2.
//! The original releases are CC BY-NC-SA 4.0. No upstream artwork is included.
pub mod biome_search;
pub mod biome_spiral;
pub mod group_search;
mod state;
pub mod structure_search;

use moddedmc_api::{
    InterfaceUnavailable, Menu, MenuButton, Mod, Player, Pos, RegistrationError, Registry,
    content::{self, Content, World, WorldExtension},
    entities::{self, Entities, EntitiesExtension},
    interaction::{self, Interaction, InteractionExtension, PlayerStackData, UseItem, UseResult},
    items::{self, Item, ItemDefaults},
    worldgen_lookup::{self, Lookup, StructureGroups, Structures, WorldgenLookup},
};
use state::{EXPLORER, NATURE, State, Status};
use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, OnceLock},
};

struct Host {
    items: ItemDefaults,
    content: Content,
    entities: Entities,
    interaction: Interaction,
    stacks: PlayerStackData,
    biomes: WorldgenLookup,
    structures: Structures,
    groups: Option<StructureGroups>,
}
static HOST: OnceLock<Host> = OnceLock::new();

const MAX_JOBS: usize = 32;
const MENU_TTL: u64 = 1200;
const PAGE: usize = 18;
const BIOME_STEP: i32 = 16 * 4;
const BIOME_RADIUS: i32 = 2500 * 4;
const BIOME_SAMPLES: u32 = 50_000;

struct Selection {
    item: &'static str,
    state: State,
    expected: Vec<u8>,
    targets: Vec<String>,
    groups: Vec<group_search::Group>,
    offered_groups: Vec<usize>,
    dimension: String,
    expires: u64,
}

enum Search {
    Biome(biome_search::Search),
    Structure(structure_search::Search),
    Group(group_search::Search),
}

struct Job {
    owner: String,
    entity: i32,
    item: &'static str,
    state: State,
    expected: Vec<u8>,
    search: Search,
}

#[derive(Default)]
struct Runtime {
    menus: HashMap<String, Selection>,
    jobs: Vec<Job>,
    in_flight: HashSet<u128>,
}
static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

fn with_runtime<T>(f: impl FnOnce(&mut Runtime) -> T) -> T {
    let mut state = RUNTIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(state.get_or_insert_with(Runtime::default))
}

fn open_menu(player: &Player<'_>, selection: &Selection, page: usize) {
    let nonce = selection.state.nonce;
    let start = page.saturating_mul(PAGE);
    let mut buttons: Vec<_> = selection
        .targets
        .iter()
        .enumerate()
        .skip(start)
        .take(PAGE)
        .map(|(index, id)| MenuButton {
            label: id.clone(),
            tooltip: "Search from your current position".into(),
            action: format!("compasses:pick/{nonce:x}/{index}"),
        })
        .collect();
    if page > 0 {
        buttons.push(MenuButton {
            label: "Previous page".into(),
            tooltip: String::new(),
            action: format!("compasses:page/{nonce:x}/{}", page - 1),
        });
    }
    if start + PAGE < selection.targets.len() {
        buttons.push(MenuButton {
            label: "Next page".into(),
            tooltip: String::new(),
            action: format!("compasses:page/{nonce:x}/{}", page + 1),
        });
    }
    if !selection.offered_groups.is_empty() {
        buttons.push(MenuButton {
            label: "Search groups".into(),
            tooltip: "Search all structures in a structure set".into(),
            action: format!("compasses:groups/{nonce:x}/0"),
        });
    }
    if !selection.state.target.is_empty() && selection.state.dimension == selection.dimension {
        buttons.push(MenuButton {
            label: "Find next / retry".into(),
            tooltip: selection.state.target.clone(),
            action: format!("compasses:next/{nonce:x}/0"),
        });
    }
    let body = match selection.state.status {
        Status::Found => selection
            .state
            .previous
            .last()
            .map_or_else(String::new, |p| {
                format!(
                    "{} in {}: X {}, Z {}. Select a target or find next.",
                    selection.state.target, selection.state.dimension, p.x, p.z
                )
            }),
        Status::Failed => format!(
            "Previous search for {} failed or was interrupted. Select a target to retry.",
            selection.state.target
        ),
        _ => "Select a target in this dimension.".into(),
    };
    player.show_menu(&Menu {
        title: if selection.item == NATURE {
            "Nature's Compass"
        } else {
            "Explorer's Compass"
        }
        .into(),
        body,
        buttons,
    });
}

fn open_group_menu(player: &Player<'_>, selection: &Selection, page: usize) {
    let nonce = selection.state.nonce;
    let start = page.saturating_mul(PAGE);
    let mut buttons: Vec<_> = selection
        .offered_groups
        .iter()
        .enumerate()
        .skip(start)
        .take(PAGE)
        .filter_map(|(index, &group)| {
            let group = selection.groups.get(group)?;
            Some(MenuButton {
                label: group.id.clone(),
                tooltip: format!("Search {} member structures", group.members.len()),
                action: format!("compasses:gpick/{nonce:x}/{index}"),
            })
        })
        .collect();
    if page > 0 {
        buttons.push(MenuButton {
            label: "Previous page".into(),
            tooltip: String::new(),
            action: format!("compasses:groups/{nonce:x}/{}", page - 1),
        });
    }
    if start + PAGE < selection.offered_groups.len() {
        buttons.push(MenuButton {
            label: "Next page".into(),
            tooltip: String::new(),
            action: format!("compasses:groups/{nonce:x}/{}", page + 1),
        });
    }
    buttons.push(MenuButton {
        label: "Search individual structures".into(),
        tooltip: String::new(),
        action: format!("compasses:page/{nonce:x}/0"),
    });
    player.show_menu(&Menu {
        title: "Explorer's Compass: Groups".into(),
        body: "Select a structure set to search all its members.".into(),
        buttons,
    });
}

fn use_item(ctx: &mut UseItem<'_>) -> UseResult {
    let Some(host) = HOST.get() else {
        return UseResult::Fail;
    };
    let item = match ctx.item {
        NATURE => NATURE,
        EXPLORER => EXPLORER,
        _ => return UseResult::Pass,
    };
    let Some(player) = ctx.player.and_then(|e| e.as_player()) else {
        return UseResult::Fail;
    };
    if ctx.stack.count() != 1 {
        player.send_message("Separate stacked compasses before use.");
        return UseResult::Fail;
    }
    let Some(dimension) = ctx.world.dimension_id() else {
        return UseResult::Fail;
    };
    let mut state = ctx
        .stack
        .data()
        .and_then(|bytes| State::decode(item, &bytes))
        .unwrap_or_else(State::blank);
    if state.status == Status::Searching {
        let active = with_runtime(|rt| {
            rt.in_flight.contains(&state.nonce)
                || rt
                    .jobs
                    .iter()
                    .any(|job| job.owner == player.name() && job.state.nonce == state.nonce)
        });
        if active {
            player.action_bar("Compass search in progress.");
            return UseResult::Success;
        }
        state.status = Status::Failed;
        state.previous.clear();
    }
    let targets = if item == NATURE {
        host.biomes.biomes(ctx.world)
    } else {
        host.structures.structures(ctx.world)
    };
    let Some(mut targets) = targets else {
        player.send_message("Compass lookup is unavailable in this world.");
        return UseResult::Fail;
    };
    targets.sort_unstable();
    targets.dedup();
    if targets.is_empty() {
        player.send_message("No searchable targets in this dimension.");
        return UseResult::Success;
    }
    let groups: Vec<_> = if item == EXPLORER {
        host.groups
            .and_then(|api| api.groups(ctx.world))
            .unwrap_or_default()
            .into_iter()
            .map(|g| group_search::Group {
                id: g.id,
                members: g.members,
            })
            .collect()
    } else {
        Vec::new()
    };
    let offered_groups = group_search::offered_groups(&groups, &targets);
    state.nonce = state::nonce();
    let Some(expected) = state.encode(item) else {
        return UseResult::Fail;
    };
    if !ctx.stack.set_data(&expected) {
        return UseResult::Fail;
    }
    let selection = Selection {
        item,
        state,
        expected,
        targets,
        groups,
        offered_groups,
        dimension: dimension.into(),
        expires: moddedmc_api::tick_count().saturating_add(MENU_TTL),
    };
    open_menu(&player, &selection, 0);
    with_runtime(|rt| {
        rt.menus.insert(player.name().to_owned(), selection);
    });
    UseResult::Success
}

fn start_search(
    player: &Player<'_>,
    selection: Selection,
    target: String,
    next: bool,
    is_group: bool,
) {
    let Some(host) = HOST.get() else {
        return;
    };
    let _ = player.with_entity_context(|world, entity| {
        if world.dimension_id() != Some(selection.dimension.as_str())
            || !host
                .stacks
                .contains(player, selection.item, &selection.expected)
        {
            player.send_message("Compass or dimension changed. Open the compass again.");
            return;
        }
        let group = if is_group && selection.item == EXPLORER {
            if next && selection.state.status == Status::Found {
                group_search::group_for(&selection.groups, &target)
            } else {
                selection.groups.iter().find(|g| g.id == target)
            }
        } else {
            None
        };
        if (is_group && group.is_none()) || (!is_group && !selection.targets.contains(&target)) {
            player.send_message("Target is not available in this dimension.");
            return;
        }
        let mut state = selection.state;
        let same = state.target == target
            && state.is_group == is_group
            && state.dimension == selection.dimension;
        if !next || !same || state.status != Status::Found {
            state.previous.clear();
        }
        if state.previous.len() > State::limit(selection.item) {
            player.send_message("Find-next limit reached. Select the target again to reset.");
            return;
        }
        state.target = group.map_or(target, |g| g.id.clone());
        state.is_group = is_group;
        state.dimension = selection.dimension;
        state.status = Status::Searching;
        let p = player.position();
        let origin = Pos {
            x: p.x.floor() as i32,
            y: p.y.floor() as i32,
            z: p.z.floor() as i32,
        };
        let mut longest_member = None;
        let search = if let Some(group) = group {
            group_search::plan(group, |member| host.structures.placements(world, member)).map(
                |plan| {
                    longest_member = plan
                        .iter()
                        .flat_map(|p| &p.members)
                        .max_by_key(|id| id.len())
                        .cloned();
                    Search::Group(group_search::Search::new(
                        origin,
                        state.previous.clone(),
                        plan,
                    ))
                },
            )
        } else if selection.item == NATURE {
            host.biomes
                .height_bounds(world)
                .and_then(|(min, max)| {
                    biome_search::Search::new(
                        (origin.x, origin.y, origin.z),
                        min,
                        max.saturating_sub(1),
                        BIOME_STEP,
                        BIOME_RADIUS,
                        BIOME_SAMPLES,
                        state.previous.iter().map(|p| (p.x, p.y, p.z)).collect(),
                    )
                })
                .map(Search::Biome)
        } else {
            host.structures
                .placements(world, &state.target)
                .map(|placements| {
                    Search::Structure(structure_search::Search::new(
                        origin,
                        state.previous.clone(),
                        placements,
                    ))
                })
        };
        let Some(search) = search else {
            player.send_message("Search unavailable.");
            return;
        };
        let Some(expected) = state.encode(selection.item) else {
            player.send_message("Compass history is too large.");
            return;
        };
        // Reserve room for a discovery including a nonzero vertical locate
        // offset. This also reserves the extended history encoding when the
        // first nonzero offset arrives after a sequence of zero offsets.
        state.previous.push(Pos {
            y: if selection.item == EXPLORER { 16 } else { 0 },
            ..Pos::default()
        });
        let fits = state.encode(selection.item).is_some()
            && longest_member.is_none_or(|member| {
                let original = std::mem::replace(&mut state.target, member);
                let fits = state.encode(selection.item).is_some();
                state.target = original;
                fits
            });
        state.previous.pop();
        if !fits {
            player.send_message("Compass history is too large.");
            return;
        }
        let accepted = with_runtime(|rt| {
            if rt.jobs.len() + rt.in_flight.len() >= MAX_JOBS {
                return false;
            }
            if !host.stacks.replace_lodestone(
                player,
                selection.item,
                &selection.expected,
                &expected,
                None,
            ) {
                return false;
            }
            rt.jobs.push(Job {
                owner: player.name().into(),
                entity: entity.id(),
                item: selection.item,
                state,
                expected,
                search,
            });
            true
        });
        player.send_message(if accepted {
            "Compass search started."
        } else {
            "Compass search could not start; retry."
        });
    });
}

struct StructureResolver<'a> {
    host: Structures,
    world: World<'a>,
    target: &'a str,
}
impl structure_search::Resolver for StructureResolver<'_> {
    fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos> {
        self.host.spread_candidate(self.world, placement, x, z)
    }
    fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>> {
        self.host.ring_chunks(self.world, placement)
    }
    fn start(&mut self, placement: u32, x: i32, z: i32) -> Lookup<Pos> {
        self.host
            .structure_start(self.world, placement, self.target, x, z)
    }
}

impl group_search::Resolver for StructureResolver<'_> {
    fn candidate(&mut self, placement: u32, x: i32, z: i32) -> Option<Pos> {
        self.host.spread_candidate(self.world, placement, x, z)
    }
    fn rings(&mut self, placement: u32) -> Lookup<Vec<Pos>> {
        self.host.ring_chunks(self.world, placement)
    }
    fn start(&mut self, placement: u32, member: &str, x: i32, z: i32) -> Lookup<Pos> {
        self.host
            .structure_start(self.world, placement, member, x, z)
    }
}

fn tick(world: World<'_>, time: u64, _: &[content::ContentEvent]) {
    let Some(host) = HOST.get() else {
        return;
    };
    let Some(dimension) = world.dimension_id() else {
        return;
    };
    // Cap total work per world tick, not per player; rotate pending jobs for fairness.
    let jobs = with_runtime(|rt| {
        rt.menus.retain(|_, menu| menu.expires >= time);
        let mut selected = Vec::new();
        let mut i = 0;
        while i < rt.jobs.len() && selected.len() < 4 {
            if rt.jobs[i].state.dimension == dimension {
                let job = rt.jobs.remove(i);
                rt.in_flight.insert(job.state.nonce);
                selected.push(job);
            } else {
                i += 1;
            }
        }
        selected
    });
    for mut job in jobs {
        let keep = host
            .entities
            .with_entity(world, job.entity, |entity| {
                let Some(player) = entity.as_player() else {
                    return false;
                };
                if player.name() != job.owner
                    || !host.stacks.contains(&player, job.item, &job.expected)
                {
                    return false;
                }
                let result = match &mut job.search {
                    Search::Biome(search) => match search.advance(16, |x, y, z| {
                        host.biomes
                            .noise_biomes(world, &[Pos { x, y, z }])
                            .and_then(|ids| ids.first().map(|id| id == &job.state.target))
                    }) {
                        biome_search::SearchResult::Pending => Lookup::Pending,
                        biome_search::SearchResult::Found {
                            position: (x, y, z),
                            ..
                        } => Lookup::Found(Pos { x, y, z }),
                        biome_search::SearchResult::NotFound { .. } => Lookup::Absent,
                        biome_search::SearchResult::Unavailable => Lookup::Unavailable,
                    },
                    Search::Structure(search) => search.advance(&mut StructureResolver {
                        host: host.structures,
                        world,
                        target: &job.state.target,
                    }),
                    Search::Group(search) => {
                        match search.advance(&mut StructureResolver {
                            host: host.structures,
                            world,
                            target: "",
                        }) {
                            Lookup::Found(found) => {
                                job.state.target = found.structure;
                                Lookup::Found(found.position)
                            }
                            Lookup::Absent => Lookup::Absent,
                            Lookup::Pending => Lookup::Pending,
                            Lookup::Unavailable => Lookup::Unavailable,
                        }
                    }
                };
                let message = match result {
                    Lookup::Pending => return true,
                    Lookup::Found(pos) => {
                        job.state.previous.push(pos);
                        job.state.status = Status::Found;
                        format!(
                            "Found {} at X {}, Z {} in {}.",
                            job.state.target, pos.x, pos.z, job.state.dimension
                        )
                    }
                    Lookup::Absent => {
                        job.state.status = Status::Failed;
                        job.state.previous.clear();
                        format!("No {} found within the search limits.", job.state.target)
                    }
                    Lookup::Unavailable => {
                        job.state.status = Status::Failed;
                        job.state.previous.clear();
                        format!("Search for {} unavailable.", job.state.target)
                    }
                };
                if let Some(bytes) = job.state.encode(job.item)
                    && host.stacks.replace_lodestone(
                        &player,
                        job.item,
                        &job.expected,
                        &bytes,
                        if job.state.status == Status::Found {
                            job.state
                                .previous
                                .last()
                                .copied()
                                .map(|p| (job.state.dimension.as_str(), p))
                        } else {
                            None
                        },
                    )
                {
                    player.send_message(&message);
                }
                false
            })
            .unwrap_or(false);
        with_runtime(|rt| {
            rt.in_flight.remove(&job.state.nonce);
            if keep {
                rt.jobs.push(job);
            }
        });
    }
}

struct Compasses;

impl Mod for Compasses {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        let content = content::require()?;
        content.require_extension(WorldExtension::Dimension)?;
        let entities = entities::require()?;
        entities.require_extension(EntitiesExtension::Player)?;
        entities.require_extension(EntitiesExtension::Movement)?;
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::ImmediateItemUse)?;
        let _ = HOST.set(Host {
            items: items::require_defaults()?,
            content,
            entities,
            interaction,
            stacks: interaction::require_player_stack_data()?,
            biomes: worldgen_lookup::require()?,
            structures: worldgen_lookup::require_structures()?,
            groups: worldgen_lookup::require_groups().ok(),
        });
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let host = HOST.get().ok_or(RegistrationError::InvalidDefinition)?;
        use fastnbt::Value;
        let defaults = fastnbt::to_bytes(&HashMap::from([
            ("id", Value::String("minecraft:compass".into())),
            (
                "components",
                Value::Compound(HashMap::from([(
                    "minecraft:max_stack_size".into(),
                    Value::Int(1),
                )])),
            ),
        ]))
        .map_err(|_| RegistrationError::InvalidDefinition)?;
        // Use a vanilla compass carrier until the licensed 32-frame models are delivered to clients.
        for (id, name) in [
            ("naturescompass:naturescompass", "Nature's Compass"),
            ("explorerscompass:explorerscompass", "Explorer's Compass"),
        ] {
            host.items.add_item(
                registry,
                &Item {
                    id,
                    display_name: name,
                    carrier: "minecraft:compass",
                    model: "minecraft:item/compass",
                },
                &defaults,
            )?;
        }
        host.interaction.on_use_item(registry, use_item)?;
        host.content.on_tick(registry, tick)?;
        Ok(())
    }

    fn menu_click(&self, player: Player<'_>, action: &str) {
        let Some(action) = action.strip_prefix("compasses:") else {
            return;
        };
        let mut parts = action.split('/');
        let (Some(kind), Some(token), Some(index)) = (parts.next(), parts.next(), parts.next())
        else {
            return;
        };
        if parts.next().is_some() {
            return;
        }
        let (Ok(token), Ok(index)) = (u128::from_str_radix(token, 16), index.parse::<usize>())
        else {
            return;
        };
        let selection =
            with_runtime(|rt| {
                if !rt.menus.get(player.name()).is_some_and(|m| {
                    m.state.nonce == token && m.expires >= moddedmc_api::tick_count()
                }) {
                    return None;
                }
                rt.menus.remove(player.name())
            });
        let Some(selection) = selection else {
            return;
        };
        if kind == "page" || kind == "groups" {
            let count = if kind == "groups" {
                selection.offered_groups.len()
            } else {
                selection.targets.len()
            };
            let valid = HOST.get().is_some_and(|host| {
                host.stacks
                    .contains(&player, selection.item, &selection.expected)
                    && player
                        .with_entity_context(|world, _| {
                            world.dimension_id() == Some(selection.dimension.as_str())
                        })
                        .unwrap_or(false)
            });
            if !valid {
                player.send_message("Compass or dimension changed. Open the compass again.");
                return;
            }
            if index < count.div_ceil(PAGE) {
                if kind == "groups" {
                    open_group_menu(&player, &selection, index);
                } else {
                    open_menu(&player, &selection, index);
                }
                with_runtime(|rt| {
                    rt.menus.insert(player.name().into(), selection);
                });
            }
            return;
        }
        let target = match kind {
            "pick" => selection.targets.get(index).cloned().map(|id| (id, false)),
            "gpick" => selection
                .offered_groups
                .get(index)
                .and_then(|&i| selection.groups.get(i))
                .map(|g| (g.id.clone(), true)),
            "next"
                if index == 0
                    && !selection.state.target.is_empty()
                    && selection.state.dimension == selection.dimension =>
            {
                Some((selection.state.target.clone(), selection.state.is_group))
            }
            _ => None,
        };
        if let Some((target, is_group)) = target {
            start_search(&player, selection, target, kind == "next", is_group);
        }
    }
}

moddedmc_api::export_mod!(Compasses);
