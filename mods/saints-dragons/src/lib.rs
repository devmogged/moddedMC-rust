//! Saint's Dragons, ported from Saint's Dragons 0.9.88 (commit 13aeb2eb2c). See docs/P2-DRAGONS.md.
//!
//! Cindervane uses a vanilla carrier with its own saved state, AI, combat and rider controls.

mod abilities;
mod advancements;
mod ai;
mod allies;
mod appearance;
mod armor;
mod audio;
mod baby;
mod binders;
mod blocks;
mod breed;
mod catalog;
mod cindervane_wild;
mod codex;
mod combat;
mod combat_atroxiia;
mod combat_ignivorus;
mod combat_nulljaw;
mod combat_raevyx;
mod combat_stegonaut;
mod combat_varasuchus;
mod combat_volitans;
mod config;
mod consumables;
mod crucible;
mod dance;
mod discs;
mod eggs;
mod family;
mod flight;
mod flight_route;
mod hunger;
mod hunt;
mod ivy;
mod ivy_trades;
mod moop;
mod mossback;
mod nucleus;
mod pack_flight;
mod particles;
mod pillars;
mod player_moves;
mod roost;
mod roster;
mod spawners;
mod spawns;
mod stun;
mod swarm;
mod taming;
mod venom_arrow;
mod vfx;
mod visuals;
mod wild;
mod worldroot;

use std::sync::OnceLock;

use moddedmc_api::{
    Command, InterfaceUnavailable, Mod, RegistrationError, Registry,
    commands::{self, Argument, Commands, Node},
    content::{self, Content, WorldExtension},
    effects::{self, Effects},
    entities::{self, Entities, EntitiesExtension},
    equipment::{self, Equipment},
    interaction::{self, Interaction, InteractionExtension},
    items::{self, Items},
    mob_ai::{self, Goal, MobAi, MobAiExtension, goal_control},
    mobs::{self, Mob, Mobs, MobsExtension},
    resources::{self, Resources},
    stacks::{self, ItemStacks},
    tags::{self, Tags},
};

const CINDERVANE: &str = "saintsdragons:cindervane";
/// Stock clients see this until the model is drawn with displays (P2-DRAGONS.md).
const CINDERVANE_CARRIER: &str = "minecraft:ravager";
/// `CindervaneStatProfile` and its config defaults for an adult.
const CINDERVANE_ATTRIBUTES: &[(&str, f64)] = &[
    ("minecraft:max_health", 80.0),
    ("minecraft:armor", 4.0),
    ("minecraft:flying_speed", 0.60),
    ("minecraft:movement_speed", 0.30),
    ("minecraft:follow_range", 64.0),
    ("minecraft:knockback_resistance", 0.0),
    ("minecraft:attack_damage", 0.0),
];

const SPAWN: u64 = 1;
const FIND: u64 = 2;
/// How far `/cindervane find` looks from its position, in blocks.
const FIND_RADIUS: f64 = 16.0;

static CONTENT: OnceLock<Content> = OnceLock::new();
static CONTAINERS: OnceLock<moddedmc_api::containers::Containers> = OnceLock::new();
static EFFECTS: OnceLock<Effects> = OnceLock::new();
static COMMANDS: OnceLock<Commands> = OnceLock::new();
static ENTITIES: OnceLock<Entities> = OnceLock::new();
static EQUIPMENT: OnceLock<Equipment> = OnceLock::new();
static FISH_BUCKETS: OnceLock<mobs::FishBuckets> = OnceLock::new();
static MOB_HEALTH: OnceLock<mobs::MobHealth> = OnceLock::new();
static MOBS: OnceLock<Mobs> = OnceLock::new();
static STRUCTURE_MOBS: OnceLock<mobs::StructureMobs> = OnceLock::new();
static MOB_AI: OnceLock<MobAi> = OnceLock::new();
static INTERACTION: OnceLock<Interaction> = OnceLock::new();
static ITEM_DEFAULTS: OnceLock<items::ItemDefaults> = OnceLock::new();
static ITEMS: OnceLock<Items> = OnceLock::new();
static RESOURCES: OnceLock<Resources> = OnceLock::new();
static STACKS: OnceLock<ItemStacks> = OnceLock::new();
static TAGS: OnceLock<Tags> = OnceLock::new();

struct SaintsDragons;
mod raevyx_weather;
mod rider_actions;

impl Mod for SaintsDragons {
    fn start(&self) -> Result<(), InterfaceUnavailable> {
        config::load();
        let containers = moddedmc_api::containers::require()?;
        containers.require_furnace_heat()?;
        let _ = CONTAINERS.set(containers);
        let content = content::require()?;
        content.require_extension(WorldExtension::Survival)?;
        content.require_extension(WorldExtension::CollisionShape)?;
        content.require_extension(WorldExtension::FluidId)?;
        content.require_extension(WorldExtension::Weather)?;
        content.require_extension(WorldExtension::Dimension)?;
        content.require_extension(WorldExtension::Surface)?;
        content.require_extension(WorldExtension::ContentEntityTransaction)?;
        content.require_extension(WorldExtension::RawBrightness)?;
        content.require_extension(WorldExtension::DescribeState)?;
        content.require_extension(WorldExtension::GameTime)?;
        content.require_extension(WorldExtension::BlockEntities)?;
        spawners::require()?;
        content.require_comparator_output()?;
        content.require_block_material()?;
        let _ = CONTENT.set(content);
        let effects = effects::require()?;
        effects.require_effect_veto()?;
        effects.require_encounter_display()?;
        effects.require_block_animation()?;
        effects.require_entity_audio()?;
        let _ = EFFECTS.set(effects);
        let commands = commands::require()?;
        commands.require_selectors()?;
        let _ = COMMANDS.set(commands);
        let entities = entities::require()?;
        combat_raevyx::require(content, entities)?;
        raevyx_weather::require()?;
        entities.require_extension(EntitiesExtension::Carriers)?;
        entities.require_extension(EntitiesExtension::Data)?;
        entities.require_extension(EntitiesExtension::Player)?;
        entities.require_extension(EntitiesExtension::Players)?;
        entities.require_extension(EntitiesExtension::Damage)?;
        entities.require_extension(EntitiesExtension::PlayerInventory)?;
        entities.require_extension(EntitiesExtension::PlayerFlight)?;
        entities.require_extension(EntitiesExtension::Attackable)?;
        entities.require_extension(EntitiesExtension::CanFreeze)?;
        entities.require_extension(EntitiesExtension::EntityEvents)?;
        if cfg!(feature = "original-assets") {
            entities.require_extension(EntitiesExtension::ScoreboardTags)?;
        }
        let _ = ENTITIES.set(entities);
        let equipment = equipment::require()?;
        equipment.require_successful_weapon_hit()?;
        let _ = EQUIPMENT.set(equipment);
        let _ = FISH_BUCKETS.set(mobs::require_fish_buckets()?);
        if cfg!(feature = "original-assets") {
            let _ = STRUCTURE_MOBS.set(mobs::require_structure_mobs()?);
        }
        let health = mobs::require_health()?;
        health.require_attributes()?;
        let _ = MOB_HEALTH.set(health);
        let mobs = mobs::require()?;
        mobs.require_extension(MobsExtension::Taming)?;
        mobs.require_extension(MobsExtension::CombatHistory)?;
        mobs.require_extension(MobsExtension::NaturalSpawns)?;
        mobs.require_extension(MobsExtension::Trading)?;
        mobs.require_extension(MobsExtension::MerchantClose)?;
        mobs.require_extension(MobsExtension::PetsSeats)?;
        mobs.require_extension(MobsExtension::RigBones)?;
        let _ = MOBS.set(mobs);
        let ai = mob_ai::require()?;
        ai.require_extension(MobAiExtension::Combat)?;
        ai.require_extension(MobAiExtension::DamageSource)?;
        ai.require_extension(MobAiExtension::HurtFinal)?;
        let _ = MOB_AI.set(ai);
        let interaction = interaction::require()?;
        interaction.require_extension(InteractionExtension::MobUse)?;
        interaction.require_extension(InteractionExtension::MeleeDamage)?;
        interaction.require_extension(InteractionExtension::BlockUse)?;
        interaction.require_extension(InteractionExtension::ImmediateItemUse)?;
        interaction.require_extension(InteractionExtension::PlayerPlaced)?;
        interaction.require_extension(InteractionExtension::BrewedPotion)?;
        codex::require()?;
        allies::require()?;
        crucible::require()?;
        ivy::require()?;
        nucleus::require()?;
        interaction.require_extension(InteractionExtension::PlayerWillDestroy)?;
        let _ = INTERACTION.set(interaction);
        let _ = ITEMS.set(items::require()?);
        let _ = ITEM_DEFAULTS.set(items::require_defaults()?);
        let _ = RESOURCES.set(resources::require()?);
        let stacks = stacks::require()?;
        stacks.require_extension(stacks::StacksExtension::Component)?;
        let _ = STACKS.set(stacks);
        let _ = TAGS.set(tags::require()?);
        visuals::start()?;
        venom_arrow::start()?;
        binders::start()?;
        rider_actions::start()?;
        Ok(())
    }

    fn register(&self, registry: &mut Registry<'_>) -> Result<(), RegistrationError> {
        let missing = RegistrationError::InvalidDefinition;
        rider_actions::register(registry)?;
        raevyx_weather::register(registry)?;
        CONTENT.get().ok_or(missing)?.on_tick(registry, tick)?;
        let interaction = INTERACTION.get().ok_or(missing)?;
        interaction.on_use_block(registry, |click| {
            let result = eggs::stack_clutch(click);
            if matches!(result, interaction::BlockUseResult::Pass) {
                crucible::use_block(click)
            } else {
                result
            }
        })?;
        interaction.on_player_will_destroy(registry, crucible::break_block)?;
        visuals::register(*ITEMS.get().ok_or(missing)?, registry)?;
        let defaults = *ITEM_DEFAULTS.get().ok_or(missing)?;
        catalog::register(*ITEMS.get().ok_or(missing)?, defaults, registry)?;
        venom_arrow::register(registry)?;
        binders::register(defaults, registry)?;
        interaction.on_use_item(registry, |click| match click.item {
            mossback::KIND => mossback::throw_item(click),
            codex::ID => codex::use_item(click),
            ivy::PLUSHIE => ivy::use_item(click),
            nucleus::CONTROLLER => nucleus::use_item(click),
            _ => binders::use_in_air(click),
        })?;
        codex::register(defaults, registry)?;
        allies::register(registry)?;
        ivy::register(defaults, registry)?;
        interaction.on_player_placed(registry, nucleus::placed)?;
        interaction.on_brewed_potion(registry, advancements::brewed_potion)?;
        worldroot::register(defaults, registry)?;
        interaction.on_melee_damage(registry, worldroot::melee_damage)?;
        armor::register(defaults, registry)?;
        let equipment = *EQUIPMENT.get().ok_or(missing)?;
        for piece in armor::DRAGONLORD.into_iter().chain(armor::BLOOD_TEMPEST) {
            equipment.on_worn_hurt(registry, piece, armor::worn_hurt)?;
        }
        equipment.on_weapon_hit(registry, worldroot::DRAGONLORD_SWORD, worldroot::weapon_hit)?;
        equipment.on_successful_weapon_hit(
            registry,
            "saintsdragons:blood_tempest_katana",
            player_moves::successful_katana_hit,
        )?;
        consumables::register(defaults, registry)?;
        discs::register(defaults, registry)?;
        let content = *CONTENT.get().ok_or(missing)?;
        blocks::register(content, registry)?;
        nucleus::register(content, defaults, registry)?;
        eggs::register(content, *ITEMS.get().ok_or(missing)?, registry)?;
        crucible::register(content, registry)?;
        MOBS.get().ok_or(missing)?.add_mob(
            registry,
            &Mob {
                id: CINDERVANE,
                carrier: CINDERVANE_CARRIER,
                attributes: CINDERVANE_ATTRIBUTES,
            },
        )?;
        let ai = MOB_AI.get().ok_or(missing)?;
        ai.on_goal(ai::HANDLER)?;
        ai.replace_goals(registry, CINDERVANE)?;
        ai.on_hurt(registry, CINDERVANE, combat::hurt)?;
        MOBS.get()
            .ok_or(missing)?
            .on_death(registry, CINDERVANE, taming::death)?;
        INTERACTION
            .get()
            .ok_or(missing)?
            .on_use_mob(registry, CINDERVANE, taming::interact)?;
        for (key, priority, controls) in [
            (
                taming::RIDE_OR_SIT,
                0,
                goal_control::MOVE | goal_control::LOOK | goal_control::JUMP,
            ),
            (
                combat::GROUND_COMBAT,
                1,
                goal_control::MOVE | goal_control::LOOK,
            ),
            (taming::FOLLOW, 2, goal_control::MOVE),
            (breed::GOAL, 1, goal_control::MOVE | goal_control::LOOK),
            (hunt::GOAL, 1, goal_control::MOVE | goal_control::LOOK),
            (
                cindervane_wild::SOCIAL,
                2,
                goal_control::MOVE | goal_control::LOOK,
            ),
            (
                cindervane_wild::DRINK,
                3,
                goal_control::MOVE | goal_control::LOOK,
            ),
            (flight::GOAL, 4, goal_control::MOVE | goal_control::LOOK),
            (ai::IDLE_LOOK, 5, goal_control::LOOK),
            (ai::GROUND_WANDER, 6, goal_control::MOVE),
            (combat::TARGETING, 1, goal_control::TARGET),
            (ai::AMBIENT, 0, 0),
        ] {
            ai.add_goal(
                registry,
                &Goal {
                    kind: CINDERVANE,
                    key,
                    priority,
                    controls,
                },
            )?;
        }
        let at = |parent, tag| {
            [
                number(Some(parent), "x", None),
                number(Some(parent + 1), "y", None),
                number(Some(parent + 2), "z", Some(tag)),
            ]
        };
        let mut nodes = vec![literal("spawn")];
        nodes.extend(at(0, SPAWN));
        nodes.push(literal("find"));
        nodes.extend(at(4, FIND));
        COMMANDS.get().ok_or(missing)?.add(
            registry,
            "cindervane",
            "Spawn or find Cindervanes",
            2,
            &nodes,
        )?;
        roster::register(registry)?;
        for kind in ["saintsdragons:volitans", mossback::KIND] {
            EFFECTS
                .get()
                .ok_or(missing)?
                .on_effect(registry, kind, can_be_affected)?;
        }
        spawns::register(
            registry,
            *MOBS.get().ok_or(RegistrationError::InvalidDefinition)?,
        )?;
        if let Some(structures) = STRUCTURE_MOBS.get() {
            for species in ["saintsdragons:ignivorus", "saintsdragons:varasuchus"] {
                structures.on_load(registry, species, appearance::spawned_from_template)?;
            }
        }
        FISH_BUCKETS.get().ok_or(missing)?.bind(
            registry,
            catalog::MOOP_BUCKET,
            "saintsdragons:moop",
        )
    }

    fn menu_click(&self, player: moddedmc_api::Player<'_>, action: &str) {
        if !ivy::click(&player, action) && !codex::click(&player, action) {
            catalog::click(&player, action);
        }
    }

    fn command(&self, tag: u64, command: Command<'_>) -> i32 {
        if let Some(result) = allies::command(tag, &command) {
            return result;
        }
        if tag == catalog::OPEN {
            return catalog::open(command);
        }
        let (Some(content), Some(commands), Some(entities), Some(mobs), Some(ai)) = (
            CONTENT.get(),
            COMMANDS.get(),
            ENTITIES.get(),
            MOBS.get(),
            MOB_AI.get(),
        ) else {
            return 0;
        };
        let (Some(x), Some(y), Some(z), Some(world)) = (
            commands.integer(&command, "x"),
            commands.integer(&command, "y"),
            commands.integer(&command, "z"),
            content.command_world(&command),
        ) else {
            return 0;
        };
        let at = [f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5];
        if let Some(kind) = roster::spawn_kind(tag) {
            let Some(id) = roster::spawn(world, &kind, at) else {
                command.reply(&format!("{kind}: could not spawn"));
                return 0;
            };
            command.reply(&format!("{kind}: spawned {id}"));
            return 1;
        }
        match tag {
            combat_raevyx::STORM_COMMAND => combat_raevyx::command_storm(world, at, &command),
            SPAWN => {
                let Some(id) = mobs.spawn(world, CINDERVANE, at) else {
                    command.reply("cindervane: could not spawn");
                    return 0;
                };
                let tagged = entities
                    .with_entity(world, id, |mob| mobs.set_state(mob, b"wild"))
                    .unwrap_or(false);
                command.reply(&format!("cindervane: spawned {id}, state set {tagged}"));
                1
            }
            FIND => {
                let min = at.map(|c| c - FIND_RADIUS);
                let max = at.map(|c| c + FIND_RADIUS);
                let mut around = Vec::new();
                let found: Vec<String> = entities
                    .entities_in(world, min, max, -1)
                    .into_iter()
                    .filter_map(|id| {
                        entities.with_entity(world, id, |mob| {
                            let Some(kind) = mobs.kind(mob).filter(|kind| *kind == CINDERVANE) else {
                                if !visuals::is_display(id) {
                                    around.push(mob.type_id().to_owned());
                                }
                                return None;
                            };
                            let state = mobs.state(mob)?;
                            let target = ai.target(mob).and_then(|t| {
                                entities.with_entity(world, t, |e| e.type_id().to_owned())
                            });
                            let health = mobs.health(mob).map_or(0.0, |(current, _)| current);
                            Some(format!(
                                "{kind} {} state={} ai={} health={health:.1} flight={} attack={} fire={} target={}",
                                mob.type_id(),
                                taming::describe(&state),
                                if ai::seen(id) { "on" } else { "off" },
                                if flight::active(id) { "active" } else { "idle" },
                                combat::last(mob).unwrap_or("none"),
                                if combat::fire_active(mob) { "active" } else { "idle" },
                                target.as_deref().unwrap_or("none")
                            ))
                        })?
                    })
                    .collect();
                around.sort();
                around.dedup();
                command.reply(&format!(
                    "cindervane: found {}: {}; around: {}",
                    found.len(),
                    found.join(", "),
                    around.join(",")
                ));
                i32::try_from(found.len()).unwrap_or(i32::MAX)
            }
            _ => 0,
        }
    }
}

/// Volitans and Mossback canBeAffected overrides.
fn can_be_affected(
    _world: content::World<'_>,
    _mob: entities::Entity<'_>,
    effect: &str,
    _duration: i32,
    _amplifier: u8,
) -> bool {
    effect != "minecraft:poison"
}

fn tick(world: content::World<'_>, time: u64, events: &[content::ContentEvent]) {
    flight_route::tick(world, moddedmc_api::tick_count());
    pillars::tick(world, moddedmc_api::tick_count());
    vfx::tick(world);
    combat::heat_tick(world);
    ivy::trade_resume_tick(world);
    player_moves::tick(world, moddedmc_api::tick_count());
    crucible::tick(world, events);
    nucleus::tick(world, time, events);
    eggs::tick(world, events);
    eggs::trample(world);
    raevyx_weather::tick(world);
    spawners::tick(world);
    visuals::sweep(world, time, events);
    roost::sweep();
    dance::sweep(moddedmc_api::tick_count());
    hunt::sweep(time);
    combat_stegonaut::sweep_aura(world);
    combat_stegonaut::portable_aura(world, time);
}

const fn literal(name: &str) -> Node<'_> {
    Node {
        parent: None,
        name,
        argument: Argument::Literal,
        tag: None,
    }
}

const fn number(parent: Option<u32>, name: &str, tag: Option<u64>) -> Node<'_> {
    Node {
        parent,
        name,
        argument: Argument::Integer {
            min: -30_000_000,
            max: 30_000_000,
        },
        tag,
    }
}

moddedmc_api::export_mod!(SaintsDragons);
