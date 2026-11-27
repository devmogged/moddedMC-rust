//! Partial native ports of Nature's Compass 3.3.1 and Explorer's Compass 3.3.2.
//! The original releases are CC BY-NC-SA 4.0. No upstream artwork is included.
pub mod biome_search;
pub mod biome_spiral;
mod config;
pub mod group_search;
mod state;
pub mod structure_search;

use moddedmc_api::{
    InterfaceUnavailable, Menu, MenuButton, Mod, Player, Pos, RegistrationError, Registry,
    content::{self, Content, World, WorldExtension},
    entities::{self, Entities, EntitiesExtension},
    interaction::{self, Interaction, InteractionExtension, PlayerStackData, UseItem, UseResult},
    items::{self, Item, ItemDefaults},
    tags::{self, Tags},
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
    tags: Tags,
}
static HOST: OnceLock<Host> = OnceLock::new();
static CONFIG: OnceLock<(config::Options, config::Options)> = OnceLock::new();

fn options(item: &str) -> Option<&'static config::Options> {
    CONFIG
        .get()
        .map(|(nature, explorer)| if item == NATURE { nature } else { explorer })
}

fn visible_targets(
    host: &Host,
    world: World<'_>,
    item: &str,
    options: &config::Options,
) -> Option<Vec<String>> {
    let (targets, registry) = if item == NATURE {
        (host.biomes.biomes(world)?, "worldgen/biome")
    } else {
        (host.structures.structures(world)?, "worldgen/structure")
    };
    let hidden: HashSet<_> = host
        .tags
        .values(registry, "c:hidden_from_locator_selection")
        .unwrap_or_default()
        .into_iter()
        .collect();
    Some(filter_targets(targets, &hidden, options))
}

fn filter_targets(
    mut targets: Vec<String>,
    hidden: &HashSet<String>,
    options: &config::Options,
) -> Vec<String> {
    targets.retain(|id| !hidden.contains(id) && !options.blocked(id));
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn structure_groups(host: &Host, world: World<'_>) -> Vec<group_search::Group> {
    host.groups
        .and_then(|api| api.groups(world))
        .unwrap_or_default()
        .into_iter()
        .map(|g| group_search::Group {
            id: g.id,
            members: g.members,
        })
        .collect()
}

const MAX_JOBS: usize = 32;
const MENU_TTL: u64 = 1200;
const PAGE: usize = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuPage {
    Targets(usize),
    Groups(usize),
}

impl MenuPage {
    fn contains(page: usize, index: usize, count: usize) -> bool {
        index < count && index / PAGE == page
    }

    fn adjacent(page: usize, index: usize, count: usize) -> bool {
        index < count.div_ceil(PAGE) && page.abs_diff(index) == 1
    }
}

#[derive(Clone)]
struct Selection {
    item: &'static str,
    entity: i32,
    // Stack nonce identifies the inventory CAS; page token identifies the
    // exact displayed dialog. Navigating must not rewrite the inventory.
    token: u128,
    page: MenuPage,
    state: State,
    expected: Vec<u8>,
    targets: Vec<String>,
    groups: Vec<group_search::Group>,
    offered_groups: Vec<usize>,
    dimension: String,
    expires: u64,
}

impl Selection {
    fn group_for_action<'a>(
        &self,
        groups: &'a [group_search::Group],
        target: &str,
        next: bool,
    ) -> Option<&'a group_search::Group> {
        fn resolve<'a>(
            groups: &'a [group_search::Group],
            target: &str,
            next: bool,
        ) -> Option<&'a group_search::Group> {
            if next {
                group_search::group_for(groups, target)
            } else {
                groups.iter().find(|group| group.id == target)
            }
        }
        let shown = resolve(&self.groups, target, next)?;
        let live = resolve(groups, target, next)?;
        // Member order affects placement planning and first-match results.
        // A stale dialog must never authorize a changed set or a different
        // first-registered group for the same found member.
        (shown == live).then_some(live)
    }

    fn catalog_matches(&self, targets: &[String], groups: &[group_search::Group]) -> bool {
        self.targets.as_slice() == targets && self.groups.as_slice() == groups
    }

    fn can_next(&self) -> bool {
        let Some(options) = options(self.item) else {
            return false;
        };
        self.can_next_with_limit(options.max_next)
    }

    fn can_next_with_limit(&self, max_next: usize) -> bool {
        self.state.status == Status::Found
            && self.state.dimension == self.dimension
            && self.targets.contains(&self.state.target)
            && max_next > 0
            && self.state.previous.len() <= max_next
            && if self.state.is_group {
                group_search::group_for(&self.groups, &self.state.target).is_some_and(|group| {
                    self.offered_groups
                        .iter()
                        .any(|&i| self.groups.get(i) == Some(group))
                })
            } else {
                true
            }
    }

    fn allows(&self, kind: &str, index: usize) -> bool {
        match (self.page, kind) {
            (MenuPage::Targets(page), "pick") => {
                MenuPage::contains(page, index, self.targets.len())
            }
            (MenuPage::Targets(page), "page") => {
                MenuPage::adjacent(page, index, self.targets.len())
            }
            (MenuPage::Targets(_), "groups") => index == 0 && !self.offered_groups.is_empty(),
            (MenuPage::Targets(_), "next") => index == 0 && self.can_next(),
            (MenuPage::Groups(page), "gpick") => {
                MenuPage::contains(page, index, self.offered_groups.len())
            }
            (MenuPage::Groups(page), "groups") => {
                MenuPage::adjacent(page, index, self.offered_groups.len())
            }
            (MenuPage::Groups(_), "page") => index == 0 && !self.targets.is_empty(),
            _ => false,
        }
    }
}

enum Search {
    Disabled,
    Biome(biome_search::Search),
    Structure(structure_search::Search),
    Group(group_search::Search),
}

impl Search {
    fn progress(&self) -> (u32, Option<i32>) {
        match self {
            Self::Disabled => (0, None),
            Self::Biome(search) => (search.samples(), Some(search.radius())),
            Self::Structure(search) => search.progress(),
            Self::Group(search) => search.progress(),
        }
    }
}

fn progress_message(item: &str, target: &str, progress: Option<(u32, Option<i32>)>) -> String {
    let mut message = format!("Compass search in progress. {target}");
    if let Some((samples, radius)) = progress {
        message.push_str(&format!("; {samples} samples"));
        if let Some(radius) = radius {
            let quantum = if item == NATURE { 500 } else { 250 };
            let rounded = (radius / quantum) * quantum;
            message.push_str(&format!("; radius {rounded} blocks"));
        }
    }
    message.push('.');
    message
}

struct Job {
    owner: String,
    entity: i32,
    item: &'static str,
    state: State,
    // Upstream find-next workers report the previous actual structure ID on
    // exhaustion, even while the searching stack identifies its group.
    failure_target: String,
    expected: Vec<u8>,
    search: Search,
}

impl Job {
    fn key(&self) -> (i32, u128) {
        (self.entity, self.state.nonce)
    }
}

#[derive(Default)]
struct Runtime {
    menus: HashMap<String, Selection>,
    // A page click consumes its displayed menu before inventory validation.
    // Retain a lease so a concurrent new use/reset can invalidate the page
    // even when it intentionally does not open a replacement dialog.
    navigation: HashMap<String, (u128, u64)>,
    jobs: Vec<Job>,
    in_flight: HashSet<(i32, u128)>,
}

impl Runtime {
    /// The same player can own multiple jobs with identical registry IDs
    /// and targets. Only the durable nonce of the used stack identifies
    /// the active worker. An in-flight worker may not have a readable
    /// progress snapshot, but must still be treated as active.
    fn active_progress(&self, key: (i32, u128)) -> Option<Option<(u32, Option<i32>)>> {
        self.jobs
            .iter()
            .find(|job| job.key() == key)
            .map(|job| Some(job.search.progress()))
            .or_else(|| self.in_flight.contains(&key).then_some(None))
    }

    fn clear_dialog(&mut self, owner: &str) {
        self.menus.remove(owner);
        self.navigation.remove(owner);
    }

    fn take_action(
        &mut self,
        owner: &str,
        token: u128,
        time: u64,
        kind: &str,
        index: usize,
    ) -> Option<Selection> {
        if !self
            .menus
            .get(owner)
            .is_some_and(|m| m.token == token && m.expires >= time && m.allows(kind, index))
        {
            return None;
        }
        let selection = self.menus.remove(owner)?;
        if matches!(kind, "page" | "groups") {
            self.navigation
                .insert(owner.into(), (token, selection.expires));
        }
        Some(selection)
    }

    fn publish_navigation(
        &mut self,
        owner: &str,
        old_token: u128,
        time: u64,
        selection: Selection,
    ) -> bool {
        if self.menus.contains_key(owner)
            || !self
                .navigation
                .get(owner)
                .is_some_and(|&(token, expires)| token == old_token && expires >= time)
        {
            return false;
        }
        self.navigation.remove(owner);
        self.menus.insert(owner.into(), selection);
        true
    }

    fn abandon_navigation(&mut self, owner: &str, old_token: u128) {
        if self
            .navigation
            .get(owner)
            .is_some_and(|&(token, _)| token == old_token)
        {
            self.navigation.remove(owner);
        }
    }
}
static RUNTIME: Mutex<Option<Runtime>> = Mutex::new(None);

fn with_runtime<T>(f: impl FnOnce(&mut Runtime) -> T) -> T {
    let mut state = RUNTIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(state.get_or_insert_with(Runtime::default))
}

fn horizontal_distance(player: moddedmc_api::Position, target: Pos) -> i32 {
    let dx = f64::from(target.x) - player.x.floor();
    let dz = f64::from(target.z) - player.z.floor();
    ((dx * dx + dz * dz) as f32).sqrt() as i32
}

fn open_menu(player: &Player<'_>, selection: &Selection, page: usize) {
    let nonce = selection.token;
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
    if selection.can_next() {
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
                let target = format!(
                    "{} in {}",
                    selection.state.target, selection.state.dimension
                );
                let suffix = if selection.state.dimension != selection.dimension {
                    "Return to the target dimension to follow the needle. Or select a new target here."
                } else if selection.can_next() {
                    "Select a target or find next."
                } else {
                    "Select a target."
                };
                if options(selection.item).is_some_and(|options| options.display_coordinates) {
                    let coordinates = format!("{target}: X {}, Z {}.", p.x, p.z);
                    if selection.state.dimension == selection.dimension {
                        format!("{coordinates} Distance: {} blocks. {suffix}", horizontal_distance(player.position(), *p))
                    } else {
                        format!("{coordinates} {suffix}")
                    }
                } else {
                    format!("{target}. {suffix}")
                }
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
    let nonce = selection.token;
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
    let Some(options) = options(item) else {
        return UseResult::Fail;
    };
    let Some(entity) = ctx.player else {
        return UseResult::Fail;
    };
    let Some(player) = entity.as_player() else {
        return UseResult::Fail;
    };
    // Every new use supersedes the previous dialog, even when the new
    // compass is already searching or has no targets in this dimension.
    // It must not cancel another stack's active worker.
    with_runtime(|rt| {
        rt.clear_dialog(player.name());
    });
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
    // Sneak-use clears the selected target without opening a dialog.
    // The new nonce also invalidates stale clicks and in-flight search results.
    if ctx.sneaking() {
        let Some(blank) = State::blank().encode(item) else {
            return UseResult::Fail;
        };
        if !ctx.stack.set_lodestone(&blank, None) {
            return UseResult::Fail;
        }
        with_runtime(|rt| {
            rt.jobs
                .retain(|job| job.key() != (entity.id(), state.nonce));
        });
        return UseResult::Success;
    }
    if state.status == Status::Searching {
        let active = with_runtime(|rt| {
            let key = (entity.id(), state.nonce);
            if state.dimension != dimension {
                // A cross-dimension use explicitly retires the old worker.
                // An already-running probe cannot publish over the new nonce.
                rt.jobs.retain(|job| job.key() != key);
                None
            } else {
                rt.active_progress(key)
            }
        });
        if let Some(progress) = active {
            player.action_bar(&progress_message(item, &state.target, progress));
            return UseResult::Success;
        }
        state.interrupt();
    } else {
        state.nonce = state::nonce();
    }
    let Some(expected) = state.encode(item) else {
        return UseResult::Fail;
    };
    // Keep the vanilla compass component consistent with the saved state, even
    // for a malformed, interrupted or migrated stack or an empty catalog.
    if !ctx.stack.set_lodestone(&expected, state.tracker()) {
        return UseResult::Fail;
    }
    let Some(targets) = visible_targets(host, ctx.world, item, options) else {
        player.send_message("Compass lookup is unavailable in this world.");
        return UseResult::Fail;
    };
    if targets.is_empty() {
        player.send_message("No searchable targets in this dimension.");
        return UseResult::Success;
    }
    let groups: Vec<_> = if item == EXPLORER {
        structure_groups(host, ctx.world)
    } else {
        Vec::new()
    };
    let offered_groups = group_search::offered_groups(&groups, &targets);
    let selection = Selection {
        item,
        entity: entity.id(),
        token: state::nonce(),
        page: MenuPage::Targets(0),
        state,
        expected,
        targets,
        groups,
        offered_groups,
        dimension: dimension.into(),
        expires: moddedmc_api::tick_count().saturating_add(MENU_TTL),
    };
    with_runtime(|rt| {
        rt.menus.insert(player.name().to_owned(), selection.clone());
    });
    open_menu(&player, &selection, 0);
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
    let Some(options) = options(selection.item) else {
        return;
    };
    let _ = player.with_entity_context(|world, entity| {
        if entity.id() != selection.entity
            || world.dimension_id() != Some(selection.dimension.as_str())
            || !host
                .stacks
                .contains(player, selection.item, &selection.expected)
        {
            player.send_message("Compass or dimension changed. Open the compass again.");
            return;
        }
        // A datapack reload may have changed visibility or group membership
        // since this dialog was sent. Authorize against the live registry.
        let Some(targets) = visible_targets(host, world, selection.item, options) else {
            player.send_message("Compass lookup is unavailable in this world.");
            return;
        };
        let groups = if is_group && selection.item == EXPLORER {
            structure_groups(host, world)
        } else {
            Vec::new()
        };
        let offered_groups = group_search::offered_groups(&groups, &targets);
        let group = if is_group && selection.item == EXPLORER {
            let Some(group) = selection.group_for_action(&groups, &target, next) else {
                player.send_message("Compass group changed. Open the compass again.");
                return;
            };
            Some(group)
        } else {
            None
        };
        if (is_group
            && !group.is_some_and(|g| offered_groups.iter().any(|&i| groups.get(i) == Some(g))))
            || ((!is_group || next) && !targets.contains(&target))
        {
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
        if state.previous.len() > options.max_next {
            player.send_message("Find-next limit reached. Select the target again to reset.");
            return;
        }
        let failure_target = target.clone();
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
        let search = if options.radius == 0 || (selection.item == NATURE && options.step == 0) {
            Some(Search::Disabled)
        } else if let Some(group) = group {
            group_search::plan(group, |member| host.structures.placements(world, member)).map(
                |plan| {
                    longest_member = plan
                        .iter()
                        .flat_map(|p| &p.members)
                        .max_by_key(|id| id.len())
                        .cloned();
                    Search::Group(group_search::Search::with_limits(
                        origin,
                        state.previous.clone(),
                        plan,
                        options.radius,
                        options.max_samples,
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
                        options.step,
                        options.radius,
                        options.max_samples,
                        state.previous.iter().map(|p| (p.x, p.y, p.z)).collect(),
                    )
                })
                .map(Search::Biome)
        } else {
            host.structures
                .placements(world, &state.target)
                .map(|placements| {
                    Search::Structure(structure_search::Search::with_limits(
                        origin,
                        state.previous.clone(),
                        placements,
                        options.radius,
                        options.max_samples,
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
        // Immediate item use holds the player's inventory while touching
        // Runtime. Never acquire inventory locks while holding Runtime.
        let key = (entity.id(), state.nonce);
        let reserved = with_runtime(|rt| {
            if rt.jobs.len() + rt.in_flight.len() >= MAX_JOBS {
                return false;
            }
            rt.in_flight.insert(key)
        });
        let accepted = reserved
            && host.stacks.replace_lodestone(
                player,
                selection.item,
                &selection.expected,
                &expected,
                None,
            );
        if reserved {
            with_runtime(|rt| {
                rt.in_flight.remove(&key);
                if accepted {
                    rt.jobs.push(Job {
                        owner: player.name().into(),
                        entity: entity.id(),
                        item: selection.item,
                        state,
                        failure_target,
                        expected,
                        search,
                    });
                }
            });
        }
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
        rt.navigation.retain(|_, (_, expires)| *expires >= time);
        let mut selected = Vec::new();
        let mut i = 0;
        while i < rt.jobs.len() && selected.len() < 4 {
            if rt.jobs[i].state.dimension == dimension {
                let job = rt.jobs.remove(i);
                rt.in_flight.insert(job.key());
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
                    || player.dimension() != job.state.dimension
                    || !host.stacks.contains(&player, job.item, &job.expected)
                {
                    return false;
                }
                let result = match &mut job.search {
                    Search::Disabled => Lookup::Absent,
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
                let mut message = match result {
                    Lookup::Pending => return true,
                    Lookup::Found(pos) => {
                        job.state.previous.push(pos);
                        job.state.status = Status::Found;
                        if options(job.item).is_some_and(|options| options.display_coordinates) {
                            format!(
                                "Found {} at X {}, Z {} in {}.",
                                job.state.target, pos.x, pos.z, job.state.dimension
                            )
                        } else {
                            format!("Found {} in {}.", job.state.target, job.state.dimension)
                        }
                    }
                    Lookup::Absent => {
                        job.state.target.clone_from(&job.failure_target);
                        job.state.fail();
                        format!("No {} found within the search limits.", job.state.target)
                    }
                    Lookup::Unavailable => {
                        job.state.target.clone_from(&job.failure_target);
                        job.state.fail();
                        format!("Search for {} unavailable.", job.state.target)
                    }
                };
                // A custom generator may return an unrepresentable result
                // (invalid resource ID, outside host tracker bounds, etc.).
                // Never leave the durable stack Searching or announce a
                // result whose tracker cannot be stored.
                let bytes = job.state.encode(job.item).or_else(|| {
                    job.state.target.clone_from(&job.failure_target);
                    job.state.fail();
                    message = format!("Search for {} unavailable.", job.state.target);
                    job.state.encode(job.item)
                });
                if let Some(bytes) = bytes
                    && host.stacks.replace_lodestone(
                        &player,
                        job.item,
                        &job.expected,
                        &bytes,
                        job.state.tracker(),
                    )
                {
                    player.send_message(&message);
                }
                false
            })
            .unwrap_or(false);
        with_runtime(|rt| {
            rt.in_flight.remove(&job.key());
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
        interaction.require_extension(InteractionExtension::HeldTracker)?;
        let _ = HOST.set(Host {
            items: items::require_defaults()?,
            content,
            entities,
            interaction,
            stacks: interaction::require_player_stack_data()?,
            biomes: worldgen_lookup::require()?,
            structures: worldgen_lookup::require_structures()?,
            groups: worldgen_lookup::require_groups().ok(),
            tags: tags::require()?,
        });
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let host = HOST.get().ok_or(RegistrationError::InvalidDefinition)?;
        let load = |nature| {
            config::Options::load(moddedmc_api::data_dir(), nature).map_err(|error| {
                moddedmc_api::error(&format!("Invalid compass configuration: {error}"));
                RegistrationError::InvalidDefinition
            })
        };
        let _ = CONFIG.set((load(true)?, load(false)?));
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
        let selection = with_runtime(|rt| {
            rt.take_action(
                player.name(),
                token,
                moddedmc_api::tick_count(),
                kind,
                index,
            )
        });
        let Some(mut selection) = selection else {
            return;
        };
        if kind == "page" || kind == "groups" {
            let valid = HOST
                .get()
                .and_then(|host| {
                    if !host
                        .stacks
                        .contains(&player, selection.item, &selection.expected)
                    {
                        return Some(Err("Compass or dimension changed. Open the compass again."));
                    }
                    player.with_entity_context(|world, entity| {
                        if entity.id() != selection.entity
                            || world.dimension_id() != Some(selection.dimension.as_str())
                        {
                            return Err("Compass or dimension changed. Open the compass again.");
                        }
                        let targets = options(selection.item)
                            .and_then(|opts| visible_targets(host, world, selection.item, opts))
                            .ok_or("Compass lookup is unavailable in this world.")?;
                        let groups = if selection.item == EXPLORER {
                            structure_groups(host, world)
                        } else {
                            Vec::new()
                        };
                        if !selection.catalog_matches(&targets, &groups) {
                            return Err("Compass catalog changed. Open the compass again.");
                        }
                        Ok(())
                    })
                })
                .unwrap_or(Err("Compass or dimension changed. Open the compass again."));
            if let Err(message) = valid {
                with_runtime(|rt| rt.abandon_navigation(player.name(), token));
                player.send_message(message);
                return;
            }
            selection.page = if kind == "groups" {
                MenuPage::Groups(index)
            } else {
                MenuPage::Targets(index)
            };
            selection.token = state::nonce();
            // If another held-item use already opened a new menu, never
            // overwrite it with the older page navigation.
            let published = with_runtime(|rt| {
                rt.publish_navigation(
                    player.name(),
                    token,
                    moddedmc_api::tick_count(),
                    selection.clone(),
                )
            });
            if published {
                match selection.page {
                    MenuPage::Groups(page) => open_group_menu(&player, &selection, page),
                    MenuPage::Targets(page) => open_menu(&player, &selection, page),
                }
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
            "next" if index == 0 && selection.can_next() => {
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

#[cfg(test)]
mod dialog_tests {
    use super::*;
    #[test]
    fn progress_matches_original_radius_rounding_and_omits_ring_radius() {
        assert_eq!(
            progress_message(NATURE, "minecraft:badlands", Some((19, Some(1499)))),
            "Compass search in progress. minecraft:badlands; 19 samples; radius 1000 blocks."
        );
        assert_eq!(
            progress_message(EXPLORER, "minecraft:villages", Some((8, Some(749)))),
            "Compass search in progress. minecraft:villages; 8 samples; radius 500 blocks."
        );
        assert_eq!(
            progress_message(EXPLORER, "minecraft:stronghold", Some((5, None))),
            "Compass search in progress. minecraft:stronghold; 5 samples."
        );
        assert_eq!(
            progress_message(EXPLORER, "minecraft:stronghold", None),
            "Compass search in progress. minecraft:stronghold."
        );
    }

    #[test]
    fn distance_uses_floored_position_and_ignores_height() {
        let player = moddedmc_api::Position {
            x: -0.1,
            y: 300.0,
            z: 3.9,
            ..Default::default()
        };
        assert_eq!(horizontal_distance(player, Pos { x: 2, y: -60, z: 7 }), 5);
    }

    #[test]
    fn pages_only_authorize_visible_indices_and_adjacent_navigation() {
        assert!(MenuPage::contains(0, PAGE - 1, PAGE + 1));
        assert!(!MenuPage::contains(0, PAGE, PAGE + 1));
        assert!(MenuPage::contains(1, PAGE, PAGE + 1));
        assert!(!MenuPage::contains(1, PAGE + 1, PAGE + 1));
        assert!(MenuPage::adjacent(0, 1, PAGE + 1));
        assert!(MenuPage::adjacent(1, 0, PAGE + 1));
        assert!(!MenuPage::adjacent(1, 1, PAGE + 1));
        assert!(!MenuPage::adjacent(0, 2, 3 * PAGE));
        assert!(!MenuPage::adjacent(1, 2, PAGE + 1));
    }

    fn test_selection(token: u128) -> Selection {
        let state = State::blank();
        Selection {
            item: EXPLORER,
            entity: 1,
            token,
            page: MenuPage::Targets(0),
            expected: state.encode(EXPLORER).unwrap(),
            state,
            targets: (0..PAGE + 1).map(|i| format!("test:target_{i}")).collect(),
            groups: vec![],
            offered_groups: vec![],
            dimension: "minecraft:overworld".into(),
            expires: 100,
        }
    }

    #[test]
    fn navigation_preserves_stack_and_rejects_replay_off_page_and_new_use() {
        let mut rt = Runtime::default();
        let initial = test_selection(10);
        let bytes = initial.expected.clone();
        rt.menus.insert("alice".into(), initial);
        assert!(rt.take_action("alice", 10, 1, "pick", PAGE).is_none());
        assert!(rt.take_action("alice", 10, 1, "gpick", 0).is_none());
        let mut next = rt.take_action("alice", 10, 1, "page", 1).unwrap();
        assert!(rt.take_action("alice", 10, 1, "page", 1).is_none());
        next.page = MenuPage::Targets(1);
        next.token = 11;
        assert_eq!(next.expected, bytes);
        assert!(rt.publish_navigation("alice", 10, 2, next));
        assert!(rt.take_action("alice", 10, 2, "pick", 0).is_none());
        assert!(rt.take_action("alice", 11, 2, "pick", 0).is_none());
        let mut back = rt.take_action("alice", 11, 3, "page", 0).unwrap();
        back.page = MenuPage::Targets(0);
        back.token = 12;
        rt.clear_dialog("alice");
        assert!(!rt.publish_navigation("alice", 11, 4, back));
        assert!(!rt.menus.contains_key("alice"));
    }

    #[test]
    fn group_navigation_cannot_cross_page_owner_or_expiry_boundaries() {
        let mut rt = Runtime::default();
        let mut menu = test_selection(20);
        menu.groups = (0..PAGE + 1)
            .map(|i| group_search::Group {
                id: format!("test:group_{i}"),
                members: vec![format!("test:target_{i}")],
            })
            .collect();
        menu.offered_groups = (0..menu.groups.len()).collect();
        rt.menus.insert("alice".into(), menu);
        // A forged action must not consume another player's menu or a
        // legitimate menu of the correct player.
        assert!(rt.take_action("bob", 20, 1, "groups", 0).is_none());
        assert!(rt.take_action("alice", 20, 1, "groups", 1).is_none());
        assert!(rt.take_action("alice", 20, 1, "gpick", 0).is_none());
        let mut groups = rt.take_action("alice", 20, 1, "groups", 0).unwrap();
        groups.page = MenuPage::Groups(0);
        groups.token = 21;
        assert!(rt.publish_navigation("alice", 20, 2, groups));
        assert!(rt.take_action("alice", 20, 2, "gpick", 0).is_none());
        assert!(rt.take_action("alice", 21, 2, "pick", 0).is_none());
        assert!(rt.take_action("alice", 21, 2, "gpick", PAGE).is_none());
        let mut page = rt.take_action("alice", 21, 99, "groups", 1).unwrap();
        page.page = MenuPage::Groups(1);
        page.token = 22;
        assert!(!rt.publish_navigation("alice", 21, 101, page));
        assert!(rt.take_action("alice", 22, 101, "gpick", PAGE).is_none());
        rt.abandon_navigation("alice", 21);
        assert!(rt.navigation.is_empty());
    }

    #[test]
    fn progress_is_keyed_to_exact_stack_and_player() {
        let mut rt = Runtime::default();
        let mut state = State::blank();
        state.target = "test:target".into();
        state.dimension = "minecraft:overworld".into();
        state.status = Status::Searching;
        let key = (1, state.nonce);
        rt.jobs.push(Job {
            owner: "alice".into(),
            entity: key.0,
            item: EXPLORER,
            expected: state.encode(EXPLORER).unwrap(),
            failure_target: state.target.clone(),
            state,
            search: Search::Structure(structure_search::Search::new(
                Pos::default(),
                vec![],
                vec![(0, 32)],
            )),
        });
        assert_eq!(rt.active_progress(key), Some(Some((0, Some(0)))));
        assert_eq!(rt.active_progress((2, key.1)), None);
        let other = (1, state::nonce());
        assert_eq!(rt.active_progress(other), None);
        rt.in_flight.insert(other);
        assert_eq!(rt.active_progress(other), Some(None));
        assert_eq!(rt.active_progress(key), Some(Some((0, Some(0)))));
        rt.in_flight.remove(&other);
        assert_eq!(rt.active_progress(other), None);
    }

    #[test]
    fn hidden_actual_member_disables_next_even_with_remaining_budget() {
        let mut menu = test_selection(30);
        let group = group_search::Group {
            id: "test:pair".into(),
            members: vec!["test:a".into(), "test:b".into()],
        };
        menu.groups = vec![group];
        menu.targets = vec!["test:b".into()];
        menu.offered_groups = group_search::offered_groups(&menu.groups, &menu.targets);
        assert_eq!(menu.offered_groups, vec![0]);
        menu.state.status = Status::Found;
        menu.state.dimension = menu.dimension.clone();
        menu.state.is_group = true;
        menu.state.previous = vec![Pos::default()];
        menu.state.target = "test:b".into();
        assert!(menu.can_next_with_limit(2));
        menu.state.target = "test:a".into();
        menu.state.previous.push(Pos { x: 32, y: 0, z: 32 });
        assert!(!menu.can_next_with_limit(2));
        // Restoring visibility allows the third search, showing that
        // the earlier rejection wasn't caused by history exhaustion.
        menu.targets.push("test:a".into());
        assert!(menu.can_next_with_limit(2));
        assert!(!menu.can_next_with_limit(1));
        assert!(!menu.can_next_with_limit(0));
    }

    #[test]
    fn visible_catalog_is_sorted_deduplicated_and_filters_both_sources() {
        let options = config::Options {
            blacklist: vec!["minecraft:*_forest".into(), "test:member_?".into()],
            max_next: 2,
            max_samples: 1,
            radius: 1,
            step: 1,
            display_coordinates: true,
        };
        let ids = [
            "test:z",
            "minecraft:birch_forest",
            "test:member_a",
            "test:hidden",
            "test:a",
            "test:z",
        ];
        let hidden = HashSet::from(["test:hidden".into()]);
        assert_eq!(
            filter_targets(
                ids.into_iter().map(str::to_owned).collect(),
                &hidden,
                &options
            ),
            vec!["test:a".to_owned(), "test:z".to_owned()]
        );
    }

    #[test]
    fn reload_cannot_redirect_displayed_group_or_navigation() {
        let mut menu = test_selection(40);
        let pair = group_search::Group {
            id: "test:pair".into(),
            members: vec!["test:a".into(), "test:b".into()],
        };
        menu.targets = vec!["test:b".into()];
        menu.groups = vec![pair.clone()];
        assert!(menu.catalog_matches(&menu.targets, &menu.groups));
        assert_eq!(
            menu.group_for_action(&menu.groups, "test:pair", false),
            Some(&pair)
        );
        assert_eq!(
            menu.group_for_action(&menu.groups, "test:b", true),
            Some(&pair)
        );
        let mut reordered = pair.clone();
        reordered.members.reverse();
        assert!(
            menu.group_for_action(&[reordered.clone()], "test:pair", false)
                .is_none()
        );
        assert!(
            menu.group_for_action(&[reordered], "test:b", true)
                .is_none()
        );
        let new_first = group_search::Group {
            id: "test:other".into(),
            members: vec!["test:b".into(), "test:c".into()],
        };
        let groups = vec![new_first, pair];
        assert!(menu.group_for_action(&groups, "test:b", true).is_none());
        assert!(menu.group_for_action(&groups, "test:pair", false).is_some());
        assert!(!menu.catalog_matches(&menu.targets, &groups));
        assert!(!menu.catalog_matches(&[], &menu.groups));
    }
}
