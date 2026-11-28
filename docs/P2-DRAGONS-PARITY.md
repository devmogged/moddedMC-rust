# Saint's Dragons 0.9.88 parity checklist

The reference is the official Minecraft 1.20.1 Fabric JAR from [Modrinth](https://modrinth.com/mod/saints-dragons/versions), SHA-256 `1093be9e314248646162bf8362e30cee63f6c6ed57d1f6590ce784cc69aee532`. REA archive evidence `ev_dbd7214a5094dcb802068d8f9430b4c66b34d51a471a6633edef19c6136ff959` identifies the package and enumerates its members. The matching MIT source revision is `13aeb2eb2c820fbd89b519946d4c3a7f563b7ad4`. The user confirmed Leon Saint's permission for this port to use the upstream assets.

The JAR contains 3,781 files, including 1,712 compiled classes. The full member inventory and 254 sound event IDs are in the worktree's `target/saints-dragons-parity/`. Static inventory establishes the files and release definitions. Behavior needs source or bytecode tracing and a scenario on both releases.

## Gameplay

The companion exposes the eight released rider keybinds through a generic
versioned player-action interface, including held and released phases. The
[headless protocol smoke scenario](../tests/scenarios/p2-rider-actions.txt) checks
registration and absence of rider callbacks for a stock client. The dragon
callbacks route mounted input to species controllers. Mounted vanilla attack/use and the four
movement double taps (300 ms) now reach those callbacks without extra keybinds;
the server confirms mod-mob mounts and the companion suppresses vanilla attack/use.
Live companion rebinding and ability execution verification remain pending.
[Built-in registration scenario](../tests/scenarios/p2-rider-mouse.txt) checks the
six subscriptions.

| Area | Current P2 coverage | Remaining parity work |
| --- | --- | --- |
| Adult creature roster | Fourteen creature types with carriers, saved identity, spawn eggs and released adult base attribute values. The Draconian Swarm egg spawns a Latcher, Winged and Whettled together. Swarm weighted 26-neighbor flight route search with Pellucida exclusion, target selection, retaliation and species-specific combat with bite, pull, dive, claw and horn charge. Ivy trades from the released table, has persistent companion inventory, recruited melee and dash attacks, inventory totem self-rescue, lethal-hit downed entry, dialogue resumption after trading, saved downed/revival counters, owner offense targeting, passive cooked-food recovery, portable binder aura and named-tag refusal | Ivy's original inventory UI, combat retreat and milk recovery, water clutch and projectile combat, carrier-specific flight movement collision, carrier-independent spawn predicates and attribute presence; falling-owner rescue, juvenile/Ivy jukebox dancing and Ivy chatter (see ranked gaps below) |
| Movement and lifecycle | Shared ground AI, baseline flight and swimming, Cindervane wild ground and aerial pack following with formation slots, bounded asynchronous flight route search, and parent following, maternal and pack defense, ground drinking and companion drinking animation, and released Moop idle, swim, fast swim and on-land animation selection, Mossback passive goals, capture/throw, toxin, fall and poison immunity, pufferfish breeding and baby growth, released gender and skin selection for adult spawn eggs and wild spawns, and natural spawning in the released biome pools for Cindervane, Atroxiia, Nulljaw, Moop and Mossback, the custom Raevyx storm, Stegonaut lush cave and Volitans underwater spawners, and Raevyx autonomous clear/rain/thunder flight decisions, cruise and dive targets | Raevyx assigned-parent checks and exact flight/landing navigation; per-species brains, navigation, growth and breeding; Cindervane water escape and water chase; released flight tactic selection, combat learning and sensory investigation |
| Combat | Cindervane autonomous and mounted bite, double bite, slash grab, magma volley and held fire body with live furnace cooking, with rider crash damage, griefing-gated fire ignition and nine-block crash imprint, and flex. Raevyx retaliation, bite with attributed chain lightning, armor-aware horn gore and ground rend, dash, collider-ray lightning beam, roar, storm with independent weather timers and natural-thunder provenance, and timed supercharge, and inferred rider dive impact, plus mounted abilities and control locks. Atroxiia mounted melee, frost abilities and slither, autonomous melee, underwater bite, major attacks, Frost Impact effects and native freeze requests, Frost Walker water freezing, block raycast attack gating and quake-to-Gungnir follow-up selection. Ignivorus rider fire breath, charged fireball, Skyfall and melee; partial ground combat with retaliation, fire immunity, melee and ranged attacks, cooldowns, Skyfall phase transition, owned fireball projectiles with attributed charge-scaled explosions on intercepted impacts, and timed magma pillars with independent damage, fire and knockback, released magma/amethyst animated pillar models and procedural Raevyx lightning arcs. Volitans mounted bite/gore, breath, orb, burrow, ultimate, dash/dodge and AI bite, claw, horn gore, water/poison breath, owned poison-ball projectile carrier, poison clearing, roar and burrow approach/exit attack. Stegonaut mounted melee, ground eating, chained ground slam with independently timed amethyst pillars and AI bite, attributed chin slam, retaliation, owner defense, nearby pack and baby protection, and tame owner and portable binder buff auras with effect removal. Varasuchus mounted melee, tailguard parry, dash, phase shift and AI bite, phase-two bite, horn gore, claw, tail sweep, slash barrage and phase transition, horn armor penetration, armor-compensated tail sweep, resistance-aware horn knockback and land attack visibility. Nulljaw mounted bite, teleport and cloak, autonomous bite, aerial pack attack runs, retaliation, shulker-owned bullet interception with shooter target sensing, and saved cloak cleanup. Worldroot deals 1.5? direct melee damage to the eight dragon types, Blood Tempest deals 3? to Raevyx, and Dragonlord deals 5? to Ignivorus | Raevyx exact aerial combat tactics; remaining per-species combat, exact Ignivorus targeting and airborne abilities, cancellable projectile impact hooks and native custom collision, persistent nonliving pillar entities, exact ability/status/damage behavior, AI targeting and terrain effects; Nulljaw exact shulker bullet immunity and flight pathfinding; Varasuchus exact eye-height sensing; Volitans custom projectile collision, damage-source hooks, exact rider physics and navigation; Atroxiia freeze eligibility use and excess frozen ticks, exact eye heights and body collision checks, owner offense targeting and exact rider behavior |
| Taming and riding | Cindervane feeding, ownership, orders, saddle, two seats and flight controls; mount profiles for all eight dragons with saddle, age, happiness, ownership checks, neutral seat anchors, animated companion rider locators and safe dismount candidates; stun taming and tame healing for Atroxiia, Ignivorus, Raevyx and Volitans; Stegonaut food taming and the Varasuchus rodeo; tamed Varasuchus food healing and wild roost sleep refusal; breeding and egg laying for Atroxiia, Cindervane, Ignivorus, Raevyx, Stegonaut and Varasuchus, underwater eggs for Volitans and live birth for Nulljaw; the Creative Dragon Taming Meal sets persistent ownership on the eight dragon species; hunger, happiness decay and slowdown, roster melee attribute scaling, juvenile food taming, growth feeding and raw-moop stunting including Nulljaw; Codex hunger, gender, brushing and biome display, physiology/ecology companion screens and manual entry removal; direct saddle/chest attachment for Cindervane, Atroxiia and Stegonaut with binder persistence and death drops; grooming hurt clips and roost damage wake-up reactions; breeding XP and stats, credited hunting and fresh-drop eating, owner-bed sleep | contextual breeding advancement, sensory disturbances; rider inventory and attachment removal, Codex handling when a filled binder is destroyed, and additional recall behavior; exact rider movement and timing; Cindervane exact projectile physics, custom crash explosion and dragon-specific griefing settings |
| Items and creative menu | 101 registered player-facing items, including the Draconic Crucible, the Ignivorus Incubator, six ore and storage blocks, Draconian Pellucida, and two Creative appearance tools. The companion lists all 101 in the **Server Mods** Creative tab in release order. Normal and golden brush scale drops and happiness recovery, the shared ten-tick brush cooldown, Scale Plucker, recipes, wear and persistent regrowth. Bucket of Moop with capture, restart persistence and release, Moop and Mossback foods, Hearty Dragon Meal nutrition, the Tideguard and Searing potion effects, both music discs, seven hatchable dragon eggs, the Arrow of Venom, and eight dragon binders with the released recipes, models and tooltips, and a companion Codex physiology/ecology reader with owner-scoped entry removal. The full Dragonlord set's fire, blast and short-fall immunity and the Dragonlord sword's fire on hit, and full Draconian armor swarm pacification with retaliation; Dragonlord double jump, fall-flying and companion acceleration, landing shockwave with native owner/ally filtering, Blood Tempest directional dodge and eight-tick invulnerability, Dragonlord sword R with native cooldown and independently ticking magma pillars and Blood Tempest katana collision-limited dash and successful melee-hit chain lightning with the released red procedural arc | Custom fissures, worn armor models, and remaining equipment components (including Blood Tempest afterimages and camera impulses) |
| Blocks and progression | Six ore and storage blocks, compaction recipes, partial food and loot, a persistent Draconic Crucible with the released companion GUI, explicit start, processing locks, fuel routing, live shared slot refresh and released recipes, including the Ignivorus Incubator, natural Deepslate Worldroot and Dragonheart ore, the End Nucleus structure, both dragon roosts with their sleep anchors, Cindervane and Raevyx egg trampling, Ivy?s house with renamed grass and chain decorations and migrated banner, skull, bookshelf and framed egg NBT with styled names, all 84 advancement definitions including recipe rewards, Tactical Nuke crash, Fire Hydrant ([combat_volitans.rs](../mods/saints-dragons/src/combat_volitans.rs)), approximate Raevyx dive exit ([combat_raevyx.rs](../mods/saints-dragons/src/combat_raevyx.rs)), post-hit baby advancement ([combat_varasuchus.rs](../mods/saints-dragons/src/combat_varasuchus.rs)) and cooking with a fire dragon triggers and engine recipe unlock support, Folded Horizon recipe predicates, potion-specific brewing take criteria, and Pellucida hardness, slime sounds and swarm path clearance | Exact trigger parity for Raevyx dive exit and hitting a baby; Crucible block animations, two-block shape, sounds and automation, Ivy's rotated painting facing, and Pellucida swarm-specific native movement collision |
| Audio | 259 released recordings and event definitions in the asset build, Cindervane action cues, hurt and death cues including Moop's inherited generic sounds, adult dragon ambient and feeding profiles, swarm ambient calls, entity-bound one-shot dragon vocalizations, movement clip footstep and flap cues, and flight transition cues. A connected bot verified Cindervane, Raevyx and Mossback before the entity-bound changes. A Java 26.3 client with no companion verified Bleeding Bolt and Mossback Music playback and ejection | Exact client animation keyframe playback and overlapping vocal animations, full per-ability cues on the remaining combat and Ivy controllers, exact flight state transitions, timed/looping sound instances, carrier sound suppression |
| Presentation | Adult and eight dragon juvenile rigs in the Fabric companion, gender and skin changes, model/animation switches on growth, food item particles, released fireball/orb/spine rigs, textured breath and lightning beam layers, frost/impact/dash sprites and Nulljaw cloak visibility | Overlays, remaining custom particles, exact camera-facing beam and projectile renderers, equipment visuals, remaining screens and translations |
| Integration | Versioned mod interfaces; the companion is required when loaded mods require it | Separate work for third-party integrations, configs, networking, and an upstream oracle for every mechanic |
| Global dragon allies | Owner-scoped save-wide UUID list (ten players), Codex typed-name add/remove through the companion, typed /dragonally commands with server-wide lookup, shared dragon and Ivy combat trust, and ally buff eligibility; Codex rig portraits, ecology item icons and brushing animation | cross-dimension Codex discovery; client visual verification |

The companion now renders release-textured Ignivorus fire breath, Volitans water/poison breath, red/gold Raevyx beam layers, ice and impact/dash sprites, and released Cindervane fireball and Volitans orb/spine geometry on short-lived displays. Nulljaw cloak hides the body rig while preserving rider seat sampling. Display leases and hurt overlays use the world game clock, so frozen game ticks do not consume their lifetimes. The [ribbon scenario](../tests/scenarios/p2-vfx-ribbons.txt) exercises breath display creation, frozen lifetime preservation and cleanup after stepping the game (pending validation). Exact original particle emission, camera-facing and scrolling beam layers, projectile texture animation and native carrier visibility/collision remain incomplete and require live client verification.

Volitans and Mossback reject poison before effects are applied, including external applications, matching their released canBeAffected overrides. Other effects retain vanilla admission rules. [Effect veto scenario](../tests/scenarios/p2-effect-veto.txt) (pending coordinator run).

Successful feeding, breeding and autonomous hunting consumption emit the food's item particles with the released hunt-and-eat burst geometry (12 particles, bounding-box-relative spread, speed 0.08). The [feeding scenario](../tests/scenarios/p2-saints-dragons-eating-particles.txt) exercises the stock-client path; visual verification remains pending.

Mossback now captures into a stackable throwable item with its baby bit, lands with the released 200-tick poison/nausea/blindness area effect and landing/toxin sounds, emits toxin on hurt, ignores fall damage, and breeds from pufferfish into juveniles with 1 HP that mature at 10 HP. Babies die on thrown landing without the adult loot. Float, panic, breeding, wandering and looking use mod goals; baby and thrown/landed animation clips are staged. Remaining limits: exact accepted-hit timing (pre-hurt hook only), XP/stat and juvenile natural group spawning, vanilla navigation sampling, exact eye position, custom particle and held-item renderers. [Mossback scenario](../tests/scenarios/p2-mossback.txt) (pending coordinator run).

The Crucible implements the released fuel tiers, nine-slot recipe matching, processing times, and pending output persisted with the block. The 14 upstream processing definitions are present. The Crucible also reads ordinary smelting recipes from the merged datapacks. The companion draws the released GUI atlas, coordinates, charge gauge, progress strips and start button using a generic [pack inventory screen](COMPANION.md#pack-inventory-screens-version-1). Its native menu retains grid locks, shift-click fuel routing and identity-bound reach checks. The two-block shape, block animation, sounds, and automation still need parity. [Native-menu scenario](../tests/scenarios/p2ax-saints-dragons-crucible.txt) (pending coordinator run; client visual check pending).

Cindervane's released brain includes retaliation and pack/maternal targeting, water escape, idle look, hunger hunting, movement intent and ground navigation, ground/air pursuit and combat, water chase, breeding, pack and parent following, drinking, autonomous flight and wandering (plus owned-dragon follow/rescue). Ground social behavior now assigns persistent parent and pack UUIDs, uses male leader ranking and four-member packs, suppresses juvenile combat, and lets mothers defend assigned babies. Drinking uses dry stances beside source water, the 15-tick alignment and 80-tick released animation with its cooldown. The [wild AI scenario](../tests/scenarios/p2az-saints-dragons-cindervane-wild.txt) checks assignment NBT. The exact drink path preview/node limits, tactical flight, async swim navigation and water breach transition, air-supply restoration, raid context and aggression configuration remain pending. Hunger hunting is also implemented.

Cindervane wild pack followers join an airborne leader and return to the ground when it lands. Formation slots have the released wing/rear spacing, smoothed leader heading and velocity, a four-tick plan refresh, correction deadband, forward-speed floor and separation. The flight route planner supplies waypoints while local collision checks and avoidance guard immediate carrier motion; slot leases approximate cleanup, and the existing raw cruise speed determines the speed scale. The [pack flight scenario](../tests/scenarios/p2-pack-flight.txt) exercises takeoff and landing (pending coordinator run).

Raevyx combat follows the released ability timelines and range/cooldown rules where the host permits. Chain, beam and roar use attributed registered damage sources; gore and rend use the released armor and toughness formulas. Beam visibility uses collider rays. Storm starts supercharge at tick 60 on ground or 47 in the air, sets thunder at tick 125 or 110 respectively, and gives 1200 ticks of double damage and health with a 4800-tick cooldown. The operator command `saintsdragons storm x y z` remains available for testing. The [combat scenario](../tests/scenarios/p2c-raevyx-combat.txt) exercises fall/lightning immunity, retaliation, chain damage, storm charge and cooldown. The storm preserves the greater of the remaining active rain and thunder timers and 1200 ticks independently, and tracks natural thunder in save-global data for wild-spawn eligibility. Remaining gaps: exact beam origin and shield-blocking source, transient beam redstone power, owner attack/raid/prey and parental targeting, exact aerial tactical movement, exact rider control behavior and remaining lightning renderer details. The dive-exit award uses observed downward rider motion instead of the original dive input. These behaviors need live verification.

Raevyx autonomous flight uses the 0.9.88 clear/thunder decision intervals (25/2 ticks plus jitter), takeoff rolls (1/80 and 1/4), keep-flying rolls (199/200 and 2999/3000), landing cooldown (100/0 ticks), cruise ranges (50–130 blocks, local alternatives 6–24), 8-block retarget radius, 300-tick target age, and the 34-degree dive with a 10-block pullout. Touchdown retains the release's downward motion so native collision establishes ground contact, without a separate support probe. The [autonomous flight scenario](../tests/scenarios/p2-raevyx-autonomous-flight.txt) verified clearance, native ground contact and thunder takeoff; the combat scenario also passed without the probe. The carrier body uses bounded flight route waypoints and conservative immediate swept block checks; exact parent assignment, ground-path failure history and full takeoff/landing transitions remain unavailable. Rain-only profiles and clear-weather dives now use the raw weather flags.

Atroxiia selects Slam, Swipe, Devastating Sweep, Helheim Quake, Gungnir Stab,
Precise Strike and underwater bite using released pacing and damage values.
Frost Impact applies slowness and weakness, requests the released native freeze
counter (140 + twice the stun duration, preserving a larger existing counter),
and suppresses Cindervane fire body. Helheim Quake freezes nearby exposed source
water and reserves a 100-tick Gungnir follow-up for the same fully frozen target,
bypassing the major cooldown while observing cadence and Gungnir's own cooldown.
Block raycasts gate ground and underwater attack selection; eye heights and
body collision remain approximations. Pumpkin's setter clamps at 140 ticks and
the ABI exposes freeze eligibility but the mod has not yet wired it into this ability, so exact sustained freezing and the
normal quake-to-Gungnir execution still require host support. Global alliances
are implemented via [allies.rs](../mods/saints-dragons/src/allies.rs); rider-only Slither is implemented in [combat_atroxiia.rs](../mods/saints-dragons/src/combat_atroxiia.rs).
The [combat scenario](../tests/scenarios/p2c-atroxiia-combat.txt) exercises
close-range retaliation, sweep damage, obstructed attack selection and native
freeze application. The follow-up decision rules are unit tested.

Mounted Atroxiia routes attack to Slam/Swipe (X toggles) or underwater Bite,
R to Devastating Sweep, H to Helheim Quake (release before tick 22 cancels the
second impact unless pressed again), and G tap/hold to Gungnir Stab/Precise
Strike. Forward double-tap starts Slither with the released movement phases,
contact damage and per-target hit interval. Flex locks movement for 67 ticks.
Ground and swim rider movement use the released speeds, blended yaw, and vertical thrust. Rider ability cooldowns follow the release ability classes rather than AI
pacing. Built-in attack and double-tap callbacks use the shared mouse input integration; companion live testing, exact
sub-tick dual input timing, moving sounds and custom effects remain pending.
[Rider scenario](../tests/scenarios/p2c-atroxiia-rider.txt).

Ignivorus ground combat implements bite, body slam, wing swipe, stomp, fire breath,
magma-pillar roar, charged fireball, Skyfall and a partial leap, including melee
hunger scaling and saved combat state. [Combat scenario](../tests/scenarios/p2c-ignivorus-combat.txt)
covers fire immunity and retaliation; awaiting coordinator execution. Remaining
work includes exact score inputs and targeting, autonomous airborne combat,
bulldoze terrain carving, precise leap and breath sweeps, and terrain effects
(griefing-aware breaking and ignition, cooking acceleration, associated advancement).
Roar now spawns nine independent magma effects at ticks 10, 16 and 22, with
Java's stage-scaled rectangular impact volume, per-target UUID hit history, 5-tick
warmup, 34-tick lifetime, 9-tick subside, attributed damage, fire and horizontal
push. Stegonaut's amethyst effects also tick independently of the caster, with
the released 4/26/10 timing. Both are non-interactable world effects with
released model clips (see below). The host still needs a tickable saved
nonliving custom entity and companion animation sync for exact model and chunk
persistence; native team/pet ownership queries are used by [pillars.rs](../mods/saints-dragons/src/pillars.rs).
Dragonlord sword R now uses these same world-ticked effects: selected main-hand
stack, living non-spectator caster, horizontal look threshold, and a native
50-tick item cooldown. Three pillars spawn at 3/9/15 blocks on the
motion-blocking-no-leaves heightmap, with 18/22/26 damage and 0.9/1.1/1.3
knockback, player attack attribution, and native pet/team exclusion.
[Sword scenario](../tests/scenarios/p2-dragonlord-sword-r.txt) covers the
main-hand requirement, all three hits and cooldown (pending coordinator run).
Both pillar types use the native attackable predicate. The release's default
config values are used; configurable overrides, unloaded-column height lookup,
and persistent visual entities remain.
[Pillar scenario](../tests/scenarios/p2-magma-pillars.txt)
checks a victim entering after the roar's first wave spawns and expiration
(pending coordinator run).
The companion now renders both released pillar geometries, textures and
emerge/subside animation clips on transient item displays; their damage timing
is unchanged. The [VFX display scenario](../tests/scenarios/p2-vfx-displays.txt)
checks the magma clip transition and cleanup. A real client must verify both
models, orientation, the amethyst scale, and animation playback.
Charged fireball now spawns an owned vanilla fireball after the release windup,
tracks its flight and applies charge-scaled, dragon-attributed explosion damage,
fire and knockback on intercepted impacts. The scenario includes an off-axis
blast victim. The host lacks a cancellable projectile impact hook and custom
projectile geometry; native impacts between mod ticks and unloads can bypass
the custom explosion, and exact trajectory, terrain effects and visuals remain partial.
Volitans has hunger-scaled melee, medium-specific AI selection, persisted cooldowns and breath energy, swept breath/orb damage, poison and roar stun, and water extinguishing fire/solidifying lava with Fire Hydrant credit for its owner. The poison orb now launches an owned vanilla snowball carrier with the released speed, ramped gravity, drag, lifetime and mod-managed area impact. Externally applied poison is vetoed before admission (with existing poison cleanup) by the registered effect hook in [lib.rs](../mods/saints-dragons/src/lib.rs). The native carrier may collide before the custom sweep, and the custom orb entity/collision, persistent flight state, exact burrow collision, incoming attacker-specific reactions and full targeting/navigation remain pending. Breath and spines still use transient simulated trajectories. [Combat scenario](../tests/scenarios/p2c-volitans-combat.txt) covers melee damage, a live orb carrier, orb impact poison and poison removal (pending coordinator run).

Stegonaut AI combat selects bite or chin slam with the released 26-tick cadence,
individual cooldowns, tick-13 impact, hunger scaling and knockback. Chin slam
uses the new attributed mob-attack call for (8 + 4) times hunger scaling.
Its tame aura refreshes Resistance I and Absorption I on its owner, same-owner
dragons and native pets, and native allied players. It explicitly removes both when a target leaves or the source dies,
unloads or is captured. [Combat scenario](../tests/scenarios/p2c-stegonaut-combat.txt)
checks chin-slam retaliation and aura removal with long-duration effects (pending
coordinator run). Bound Stegonaut binders anywhere in the player's main inventory,
armor or offhand refresh portable Resistance I and Absorption I on the holder,
nearby players and owned mod dragons/native pets every 20 ticks, with dimension-wide target
cleanup when the source is removed. [Portable aura scenario](../tests/scenarios/p2-stegonaut-portable-aura.txt)
checks empty binders, main inventory, offhand and explicit cleanup (pending coordinator
run). Mounted Stegonaut supports X melee selection, rider-directed melee, G charge/release, H tap/hold/repress, Ctrl+4 flex, ground jumping and acceleration. Ground eating cancels before 29 ticks, fires 18 ticks after a full-charge release and cools down for 30 ticks. Ground slam hits at tick 18, chains at tick 23 and hits again after 25 ticks, spawning sixteen independent pillars with a four-tick warmup and 26-tick active lifetime. The [combat scenario](../tests/scenarios/p2c-stegonaut-combat.txt) also checks mounting and documents manual companion input tests. The selected melee uses the vanilla attack key through the mounted input callback. Remaining: original chunk renderer, exact collision and screen shake,
separate particle/icon visibility;
raid membership, exact visibility, last attack and
projectile damager history. The hurt callback exposes source entities, but the mod does not yet use them for this behavior
or transform incoming damage for 30% fall damage and wild Stegonaut immunity; a player
attack callback is needed for the pre-damage baby-hit advancement.

The appended pet/seat ABI exposes native ownership and scoreboard alliances.
Dragonlord landing shockwave and sword abilities share the same living-target
filter, protecting owned native pets, tame dragons and native allies; Cindervane
combat and fire-body aura also recognize native allies and same-owner pets.
All eight dragon mounts now supply their released neutral rider anchors, with
separate Cindervane owner/guest and slash-grab offsets. Dismounts search the
species-specific candidate offsets using native floor and clearance validation.
Ground dismounts approximate passenger width and hand preference; the engine
falls back to its normal safe dismount search. Global ally lists and animated client seats also apply. [Pet and seat scenario](../tests/scenarios/p2-pets-seats.txt)
covers portable pet buffs and mount/dismount lifecycle (pending coordinator run).

The released `DragonRiderSeat.positionAnimatedRider` uses a neutral yaw-relative
server attachment for ordinary riders and samples the actual animated bone
locator on the client. All eight neutral offsets are already ported; Cindervane's
slash-grab fallback now restores the ordinary seat when mounting fails or the
held passenger leaves. The companion now supports generic animated seats:
rig metadata identifies the bone and model-pixel offset, the body display
identifies its carrier, and sampled offsets follow the carrier's current yaw.
All eight dragons have their released locator bones and offsets, including
Cindervane's two seats, Nulljaw's hanging height and the ground dragons'
two-tick freshness limit. Cindervane's mounted slash grab uses the controlling
owner's reported `automountBoneRight` world position and the released neutral
anchor when a sample is missing or stale. The host additionally validates
visibility, reporter identity, finite coordinates and distance; the grab itself
clamps the offset to eight blocks. The [slash-grab scenario](../tests/scenarios/p2-slash-grab.txt)
exercises the companion report with a headless rider. The
[mount scenario](../tests/scenarios/p2-mount.txt) records manual companion
acceptance checks; the bot verifies the logical riding lifecycle only.
Visual seat and camera behavior still require a real companion client check.

Varasuchus AI uses the released half-health phase threshold, 21/33-tick
phase transition, 30-tick attack cadence and 14-hit slash barrage. Its
phase persists in NBT and AI melee scales with hunger. Owner retaliation,
nearby wild baby defense and prey targeting are implemented in part.
Horn gore compensates for three or five armor points according to phase and
uses the live attack, armor, toughness and knockback resistance attributes.
Varasuchus has a base attack attribute of 10; bite, gore and claw scale from
the live attribute, and hunger scales ability damage without rewriting it.
Tail sweep compensates for all armor reduction. On land, attack startup
requires an unobstructed collider ray; the eye positions approximate the
release's pose-specific eye heights using collision bounds. Claw terrain destruction, raid and roost defense, and exact
vision and mother assignment remain pending. The juvenile advancement currently
follows successful hits.
[Varasuchus combat scenario](../tests/scenarios/p2c-varasuchus-combat.txt)

Nulljaw's attack run uses the released orbit, staging, dive and egress phases,
with staggered pincers for groups of three or four. Its bite has the 5/2/10 tick
track, 12-tick recovery cooldown and 8 base damage scaled by hunger. Wild
retaliation alerts squads of up to four, and adults hunt visible shulkers.
Tamed adults scan within 32 blocks of their owner, require a Shulker shooter
and intercept its bullets when the shooter targets the owner or the bullet
approaches the owner's body center within eight blocks. Tracked bullets remain
valid within 40 blocks of a living owner. Saved cloak state counts loaded ticks
and removes invisibility from the dragon and recorded rider on expiry, dismount
or death. An adult owner can mount without a saddle above 30 happiness.
Secondary (H) presses toggle the saved five-minute cloak for the rider and
dragon. Primary (R) presses sample the mounted collision shell every half block
along the rider's look direction, stop at the first obstruction, and teleport to
the last safe point 1–16 blocks away with a 40-tick cooldown. The mounted bite
reuses the AI bite timeline, sound and damage, using the released body-facing
sweep rather than a single AI target. The rider attack uses the vanilla attack mapping.
Exact effect-presence checks, independent effect icons,
direct damage source immunity, teleport world-border/entity collision and custom
tracking packet, and strict-air navigation remain pending. The bot scenario
checks mounting and saved cloak cleanup; actual companion actions need a live
client, with manual checks listed in the scenario.
[Scenario](../tests/scenarios/p2c-nulljaw-combat.txt).


All eight adult dragons have the released ambient interval and weighted grumble
profiles and their own feeding sound, including juvenile feeding pitches.
Dragon hurt, feeding, Cindervane attacks and grumbles use the entity-bound
one-shot packet when supported; death sounds remain positional. Moop uses the
vanilla generic hurt and death sound inherited from its Java superclass.
The movement sound table follows the released animation keyframes rounded to
20 Hz and uses the current server-selected clip. Flight cues use the available
ground-contact and takeoff state. The exact playback rate, phase and blended
GeckoLib animation controllers need a client animation keyframe callback or
a synchronized animation clock. Timed or looping moving sounds need a generic
start/update/stop sound-instance interface with an entity emitter, duration,
loop flag and gain/pitch updates; vanilla entity-bound one-shots alone cannot
provide that. Suppressing unrelated native carrier sounds needs a generic
per-entity sound suppression operation. Ability-specific cues also depend on
the corresponding ability state machine. The packet scenario covers Stegonaut
feeding and ambient sounds and Moop hurt/death; real-client synchronization
and flight acoustics still need manual verification.
[Audio scenario](../tests/scenarios/p2c-audio.txt)

The Ignivorus Egg advances through three crack stages while on the Ignivorus
Incubator. Progress persists across restarts. The hatchling starts with 90 health,
uses the original juvenile rig in the companion, and grows over 24,000 loaded
mob ticks. The test seeds progress near stage boundaries. Hatching awards the
released `hatch_ignivorus` advancement. All four Ignivorus roost templates in the
0.9.88 JAR contain no Ignivorus egg or chest; the top-left template contains the
incubator and an adult dragon. The released egg sources are breeding and Ivy's
trade table, not a generated roost egg.

Stegonaut, Varasuchus, and Atroxiia eggs use the same saved progression with
their released 30,000, 24,000, and 24,000 tick timers and respective starting
health of 50, 80, and 30. The original juvenile rigs are included in the asset
build. Varasuchus hatched in the End matures into the Void-Kissed variant.

The Volitans Egg requires waterlogging for its 18,000-tick incubation. It
preserves progress while dry, retains water when it hatches, and produces a
juvenile with 60 starting health.

A Cindervane clutch can hold up to three eggs, advances through three
crack stages in 12,000 loaded ticks, and produces one 40-health hatchling
per egg. The original clutch and juvenile models are included. Piebald appearance is rolled at hatch and applied at maturity. Trampling is implemented in [eggs.rs](../mods/saints-dragons/src/eggs.rs).

The Raevyx Egg incubates in 18,000 ticks normally and at the 9,600-tick
rate during thunder. Lightning within the release's three-block expanded
collision box hatches it immediately. The server scenario verified thunder
and summoned lightning. All seven egg blocks report the hatch stage (0 to 2)
to comparators. The Volitans scenario covers adjacent comparators and comparators
reading through a solid block. Trampling and hatch advancements are implemented in [eggs.rs](../mods/saints-dragons/src/eggs.rs).

The Arrow of Venom uses the released recipe and item model. Bows and crossbows
launch it at 1.35 times normal speed with no water drag. A damaging hit clears
the target's hurt cooldown and applies Poison II for 30 seconds and Blindness
for 5 seconds. The server scenario checks water motion, both weapons, and
picking up the spent arrow after a restart. The flight and impact renderer,
Ivy's friendly-hit rules remain part of the projectile parity work. Volitans rejects poison via the effect veto; exact projectile-specific immunity remains to check.
[Arrow of Venom scenario](../tests/scenarios/p2z-saints-dragons-venom-arrow.txt)

The eight binders capture an owned dragon and release it with the same UUID,
health, name, and saved mod state. The stock-client ground checks cover all
eight species, blocked release, wild capture denial, and a restart with a
filled binder. Raevyx, Cindervane, Ignivorus, and Volitans also have a five-block
air release, also verified with a stock-protocol client. They descend using
the port's shared flight goal. Capture requires the dragon to be free of
passengers and vehicles. The original tamed flight AI and Codex tracking when a binder is destroyed remain pending. Pocket Dragon is awarded in [binders.rs](../mods/saints-dragons/src/binders.rs).
[Binder scenario](../tests/scenarios/p2aa-saints-dragons-binders.txt)

The three swarm spawn eggs use the released creature identities and starting
health of 12, 6, and 16. The group egg spawns all three in order along the
x-axis at two-block intervals and consumes one item in survival. The
companion pack includes all three original rigs and animation sets. A stock
client receives display fallbacks. The scenario verified the four eggs,
persistence, spawning without gravity, and original hurt and death audio.
Suspended wandering uses the released movement speeds and takes clear, straight
routes. The weighted route planner in [flight_route.rs](../mods/saints-dragons/src/flight_route.rs) now paths around obstacles; the encounter's spawn and death animations and exact native hitboxes remain pending. The stock
`/saintsdragons items` packet includes all eight binders and all four swarm
eggs. In a local 24-creature smoke check, the final tick query read 4.72 ms
with ground movement and 3.94 ms with suspended wandering on the same engine
build. This is a short run with different world seeds and an engine test build
in the background.
[Swarm spawn egg scenario](../tests/scenarios/p2ab-saints-dragons-swarm-eggs.txt)

Swarm members now use explicit target and combat goals: retaliation takes
priority, followed by players and other living entities; swarm members and
Endermen are excluded. Latcher bites, Winged pulls and dives after two successful
pulls, and Whettled claws or charges with their released damage and attack
timings. Reservations limit attacks on one target and the nucleus bounds
targeting. The route planner uses the released weighted Lazy Theta* search,
with continuous swept body clearance and Pellucida exclusion for all three
swarm species. Native carrier motion still approximates the original collision
geometry.
The full Draconian armor set pacifies swarm members
unless the wearer is their last attacker. Target selection and attacks read
the four worn item IDs through the generic equipment interface.
[Swarm combat scenario](../tests/scenarios/p2aw-saints-dragons-swarm-combat.txt)

A player-placed Draconian Nucleus records its owner. The Controller toggles
owned encounters within 64 blocks, removes the active swarm when it stops an
encounter, and observes the 1,200-tick reactivation delay. An unowned Nucleus
activates for a nearby survival player. The server scenario verified all three
waves (6, 9 and 12 creatures), non-owner denial, persistence, both Controller
use paths, and the summon cue. Release parity still needs encounter music, boss
bars, exact spawn collision, and exact source combat. Advancement awards are implemented in [nucleus.rs](../mods/saints-dragons/src/nucleus.rs); species combat is in [swarm.rs](../mods/saints-dragons/src/swarm.rs). Controller summons keep an empty death loot
table after restart. The live scenario killed a reloaded summon and found no
item drops, then confirmed that a naturally summoned Latcher retained its
released loot table.
[Nucleus and Controller scenario](../tests/scenarios/p2ac-saints-dragons-nucleus.txt)

The original Nucleus template and its jigsaw placement generate in End Barrens.
On a fresh seeded world, the scenario located the structure, read the original
dormant NBT, and confirmed a nearby survival player heard the summon cue and
started the first wave.
[Nucleus world generation scenario](../tests/scenarios/p2af-saints-dragons-nucleus-worldgen.txt)

Deepslate Worldroot Ore generates below ground in the Overworld. Dragonheart Ore
generates on the underside of End islands in Barrens, Midlands, and Highlands.
The port uses the release's feature definitions in the 26.3 registry directories
and biome modifiers for the release's Fabric feature registrations. The seeded
scenario found nine Worldroot blocks and two Dragonheart blocks in newly
generated chunks.
[Ore world generation scenario](../tests/scenarios/p2ag-saints-dragons-ore-worldgen.txt)

Adult spawn eggs save gender and roll the released skin weights. Cindervane and
Volitans each have a 15% chance of their alternate skin, Raevyx 10%, and
Ignivorus 5%. A Varasuchus spawn egg selects Void-Kissed in the End. The live
scenario verified the Overworld and End variants and retained the End
Varasuchus's appearance through a restart.
[Spawn egg appearance scenario](../tests/scenarios/p2ah-saints-dragons-spawn-appearance.txt)

The original Ignivorus and Varasuchus roosts and Ivy's house generate with their
released jigsaw placement and biome rules. The seeded scenario located all
three, verified that Ivy spawned and her chest retained its original loot table,
and checked the template dragons' health, skin and gender. Varasuchus retained
its persistence flag after a server restart. A template dragon keeps its spawn
block as home and finds the template's sleep anchor marker. At night it returns to
the anchor, settles and plays its fall asleep and sleep clips, and at day it wakes
and keeps to its territory. The Ivy house template's legacy grass and chain
palette entries are renamed to short grass and iron chain at pack time.
[Roost and house scenario](../tests/scenarios/p2ai-saints-dragons-roosts.txt)

The Java block and item registries have no additional legacy decoration IDs.
Ivy's house uses vanilla decoration NBT: pack staging now upgrades seven
banners (29 layers), seven player head profiles, nine bookshelves (30 books)
and the invisible frame's Armaralis Egg player head, preserving its texture,
name and -20% mainhand movement speed modifier. The five paintings retain
their released variant data. The egg retains its yellow non-italic styling;
structure placement still needs to rotate painting facing along with position.
[Legacy decoration scenario](../tests/scenarios/p2-legacy-decor.txt) (pending coordinator run).

Cindervane, Atroxiia, Nulljaw, Moop and Mossback join the biome spawn pools of
their released biome tags with the 0.9.88 config weights and group sizes. The
natural spawner and chunk generation pick from vanilla and dragon entries by
weight. Each species keeps its `canSpawnHere` rule: dry, open ground, a
competing vanilla creature weight of at least five and no snowy slopes for
Cindervane, End Barrens for Nulljaw, the animal-spawnable block tag and raw
brightness above eight for Mossback, and the `DragonSpawnRules` density limits
outside chunk generation. A wild adult rolls the released gender and skin
weights. The rule runs on its first AI tick, not at spawn. The scenario found
a generated Cindervane in the jagged peaks and checked its appearance.
[Natural spawn scenario](../tests/scenarios/p2aj-saints-dragons-natural-spawns.txt)

Raevyx, Stegonaut and Volitans use the release's custom spawners with the
0.9.88 config defaults. Every few seconds each one may pick a player standing
in its biomes and search around them: Raevyx on open, dry ground under the sky
while it thunders, Stegonaut on covered ground in lush caves at least 24 blocks
from players, and Volitans on the floor of a water column at least three blocks
deep. A spawn needs room for the species' hitbox and passes the density limits.
It also skips players with a wild one of that species nearby and regions it
already filled. Raevyx comes in packs of one or two and Stegonaut of one to
four. Wild dragons don't despawn far from players, as with Java's tamable
animals, and Volitans breathes underwater. The scenario builds each habitat and
checks the rolled appearance of each spawn.
[Custom spawner scenario](../tests/scenarios/p2ak-saints-dragons-custom-spawners.txt)

All fourteen adult registrations specify the released health, movement speed,
armor, follow range, knockback resistance and declared attack values, including
flying speed where declared. Carrier attack values absent in the Java attribute
supplier are neutralized to zero. Density checks use uncapped box queries; the
custom spawners use dimension build bounds and test individual block collision
boxes. Exact parity still requires host registration of species dimensions and
attribute presence, carrier-independent vanilla spawn-rule predicates (monster
light and difficulty, surface water animal), and a generic world
`no_collision(aabb, excluded_entity)` query for entity and border collisions.
Raevyx wild spawns now reject summoned-only thunder using the saved natural-thunder countdown. The following scenario covers representative
adult attributes and the hatch advancement.
[Spawn and attribute scenario](../tests/scenarios/p2c-spawns.txt)

A player who moves on a Cindervane or Raevyx egg without sneaking breaks it one
time in ten with the turtle egg break sound, as `stepOn` does in 0.9.88. A
Cindervane clutch loses one egg at a time. A player standing still doesn't
trample, since vanilla calls `stepOn` only from movement.
[Egg trampling scenario](../tests/scenarios/p2al-saints-dragons-egg-trampling.txt)

Ivy's `open_trade` dialogue nodes open the vanilla trading screen. The first
time, she rolls the released `ivy_trades` table: the fixed Hearty Dragon Meal
trade and four of the six weighted dragon egg trades, with their counts,
result pools and enchantments. Her villager carrier then counts uses and saves
the offers. Trades give no experience, as in 0.9.88. The villager's yes and no
sounds still play.
[Ivy trading scenario](../tests/scenarios/p2am-saints-dragons-ivy-trades.txt)

Closing Ivy's trade screen resumes the selected dialogue node three ticks later,
for the matching player and merchant, as in the release.
[Merchant close scenario](../tests/scenarios/p2-ivy-merchant-close.txt)

Atroxiia, Ignivorus, Raevyx and Volitans are tamed as in 0.9.88: food does
nothing until the dragon is weakened to its stun health (100 HP for Ignivorus,
60 for the others). A hit that would take it lower leaves it at that health and
holds it for 30 seconds. While held, it takes no target and doesn't move. Feeding
it then rolls the species' chance for that food. A failure heals it back to full
over a few seconds, and an unfed dragon recovers when the hold runs out. A
tamed dragon sits, and food heals it. The scenario covers the weakening message,
the held health, failed and successful feeds, and the timeout.
[Stun taming scenario](../tests/scenarios/p2an-saints-dragons-stun-taming.txt)

Crouch-feeding a tame adult its species' food puts it in love for 30 seconds. A
tame female and male of the same species in love find each other within the
species' partner range. A sitting dragon gets up when it has a mate. The pair
court for three to five seconds, then lay their species' egg on the nearest open
ground between them. The egg belongs to the mother's owner and holds a random
baby gender. Both parents then wait 6,000 ticks before they can breed again. The
scenario breeds two Raevyx, finds the egg and checks the cooldown message.
[Breeding scenario](../tests/scenarios/p2ao-saints-dragons-breeding.txt)

Volitans court only in water for at least 60 ticks and look for a solid
underwater floor within eight blocks horizontally and twelve down. Their egg
is waterlogged. Nulljaw eat chorus fruit and court for 60 ticks before giving
birth to a live juvenile with 70 health, 4 armor, a 0.55 hitbox scale, and the
coordinating parent's owner. Both use the existing 6,000-tick breeding cooldown.
Successful breeding spawns 1â€“7 experience when mob loot is enabled and awards
the animals-bred statistic to the first available feeding player. The contextual
BRED_ANIMALS advancement trigger remains deferred.
[More breeding scenario](../tests/scenarios/p2at-saints-dragons-more-breeding.txt)

A wild adult Stegonaut is tamed by any of its foods. Clicking a wild adult
Varasuchus with an empty hand mounts it. It bucks in place for 90 ticks, then
rolls its 16.67 percent chance. On a success it accepts the rider and follows
them. On a failure it throws the rider off, turns on them unless they're in
Creative, and refuses another ride for five seconds.
[Wild taming scenario](../tests/scenarios/p2ap-saints-dragons-wild-taming.txt)

The server tracks the release's gameplay advancements, now in the 26.3 layout, and
shows them in their own tab. Criteria from vanilla triggers, such as picking
up a Draconic Codex or a dragon egg, fire on their own. The mod awards the
ones the release grants from code: clicking a dragon, meeting Ivy, taming each
species, hatching each egg (the owner, or players within 32 blocks), a Raevyx
egg hatched by lightning, binding a dragon, and summoning, meeting and
defeating the Draconian Swarm. `/advancement` and `advancements={...}`
selectors accept their ids.
[Advancements scenario](../tests/scenarios/p2aq-saints-dragons-advancements.txt)

All 84 released advancement files are now included, including the 32 previously
missing recipe-reward definitions. Folded Horizon retains the release's seven
binder recipe IDs and supplies the current Pumpkin predicate parser's `recipe`
alias, since it does not read `recipe_id`. The 13 custom Crucible recipes
already use the mod's own shaped and smelting implementation.
Pellucida uses the release's zero hardness, purple map color and slime sounds,
while its glass collision shape remains solid. Recipe rewards are supported by the integrated Pumpkin recipe hooks (engine pointer commits `2c128c8`, `321b49a`).
The released brewing criteria are awarded when the player takes the corresponding custom potion from the bottle slot, including shift-click; the engine matches vanilla potion IDs and the mod maps its custom output IDs. [Brewing regression scenario](../tests/scenarios/p2-brew-trigger.txt) checks both custom outputs and excludes ordinary potions and brew completion. Pellucida
is ignored by mod-side swarm path clearance, but physical movement still needs
a generic entity-aware block collision filter (including native movement and
collision queries) in the engine. Ordinary entities must retain the solid shape.
Swarm entities inherit vanilla push behavior; their exact body dimensions need
carrier-independent width and height. Fabric dragon parts are noncollidable
and nonpushable, and adult dragon parents ignore push/knockback; exact part
and parent collision behavior requires native hitbox and push controls.
These engine changes are not part
of the drop-in mod. The progression scenario checks a newly included inventory
criterion and an immediate survival break and drop of Pellucida.
[Progression scenario](../tests/scenarios/p2bb-saints-dragons-progression.txt)

[Pellucida collision scenario](../tests/scenarios/p2bc-saints-dragons-part-collision.txt)
checks that an ordinary falling item still lands on Pellucida; actual swarm
pass-through remains blocked on the host filter. The mod unit test covers
swarm clearance through Pellucida and rejection of stone and unloaded cells.

Every dragon has 100 hunger saved with it. Hunger drops by one for each 4800
ticks, twice as fast while a player rides. At zero the dragon takes 2 starvation
damage every 80 ticks. Each meal adds 10, or 20 for a Hearty Dragon Meal, and
feeding a tamed dragon at full health with hunger to spare says "Feeding ...".
Below 60 hunger the Cindervane's bites and slashes weaken to a quarter of their
damage at 30. A tamed dragon loses 2 happiness every 9600 ticks, two or four
times as fast when hungry, gains 4 or 8 from meals and loses 2 when hit. At 60
happiness or less it slows, down to half speed at 30, and shows angry particles
when hurt.

A wild juvenile tames from its species' food with the adult's chance
(Cindervane, Raevyx, Ignivorus, Volitans, Atroxiia and Varasuchus) or always
(Stegonaut). Feeding a tamed juvenile brings it 2400 ticks closer to adulthood,
or 4800 for a Hearty Dragon Meal. Its owner can feed it raw moop to keep it
small for good.
[Hunger and juvenile scenario](../tests/scenarios/p2ar-saints-dragons-hunger-babies.txt)

The eight hatchling/live-birth dragon rigs use their released geometry, animation
files and gender textures. The display changes item model and transform as the
dragon grows, rebuilding the Cindervane's extra display parts when needed. A
juvenile ignores adult skin variants, preserving its gender texture until maturity.
[Juvenile rig scenario](../tests/scenarios/p2-juvenile-rigs.txt)

The seven roster dragons now scale their carrier attack damage attribute with
the same hunger multiplier as Cindervane's abilities, restoring their base damage
at 60 hunger or higher. Ignivorus keeps its juvenile zero damage and adult
base of 50. Cindervane, Raevyx, Ignivorus, Varasuchus and Volitans acquire their
released prey types at 60 hunger or less. After a credited kill, they look
for fresh food drops (at most 100 ticks old) around the kill for 60 ticks and
eat one every 20 ticks, restoring 10 hunger. Species combat controllers now exist in `combat_*.rs`; dedicated aerial hunting landing and interaction animation blending remain pending. Food item particles are implemented in [particles.rs](../mods/saints-dragons/src/particles.rs).
[Hunting and melee scaling scenario](../tests/scenarios/p2ba-saints-dragons-hunting.txt)

A tamed adult Varasuchus heals 10 health from its ordinary food or 35 from a
Hearty Dragon Meal, which also grants Regeneration II for 200 ticks. Meals
restore hunger and share the species' feeding cooldown with growth and breeding.
A wild Varasuchus refuses ordinary interactions while falling asleep, sleeping,
or waking at its roost. Follow AI is also gated on the sleep state. The seven
released sleep-capable dragons now follow their owner's actual bed sleep at
night and wake when the owner wakes or conditions no longer permit sleep.
The released underwater sleep-depth search and sensory disturbances remain
pending.
The tamed mounting interaction wakes the dragon immediately and allows the
adult owner to ride without a saddle, subject to happiness. Varasuchus now
handles rider ground/swim movement, double-tap dash, flex, phase shift,
phase-dependent claw/tail sweep and barrage, and held tailguard. The final-hurt
hook cancels qualifying close-range damage and applies the parry sweep.
Melee mode is persisted and bite/horn attack uses the shared mounted vanilla attack callback ([rider_actions.rs](../mods/saints-dragons/src/rider_actions.rs)).
Claw terrain destruction follows the mob-griefing rule; the separate release
config switch and waterlogged block handling are not reproduced.
[Varasuchus rider scenario](../tests/scenarios/p2-rider-varasuchus.txt)
[Varasuchus care scenario](../tests/scenarios/p2au-saints-dragons-varasuchus-care.txt)

The released saddle carriers are Cindervane, Atroxiia and Stegonaut. Owners
install a saddle in the main hand, consuming it outside Creative, and the
saddle drops on death. Ignivorus, Raevyx, Varasuchus, Volitans and Nulljaw
mount without one. Juveniles and owners of dragons at 30 happiness or below
cannot mount; Cindervane alone permits a second player once its owner is
aboard. The owner can reclaim the first seat. Ground dragons no longer use
the existing Cindervane flight movement. The host supplies ordinary dismount
and carrier seating with species-specific dismount candidates; the companion
samples the animated rider offsets. Nulljaw's collision nudge remains pending. Per-species rider movement and abilities are implemented in the species combat controllers; exact physics and live checks remain.
[Mounting scenario](../tests/scenarios/p2-mount.txt) (pending coordinator run).

Nulljaw juveniles accept chorus fruit, with a one-in-five wild taming roll and
no wild feeding cooldown or hunger reward. Once tame, chorus fruit advances
growth by 2,400 ticks and adds 10 hunger; raw moop stunts growth for the owner.
Both tame feeding paths use a 24-tick cooldown. The Codex records hunger for
live and bound dragons, defaulting old records to full hunger.
[Nulljaw juvenile and Codex hunger scenario](../tests/scenarios/p2av-saints-dragons-nulljaw-juvenile.txt)

Grooming uses the five-minute scale regrowth cycle, with a ten-tick brush
cooldown. A tame juvenile may be brushed but cannot be plucked. An owner can
pluck three scales once per shedding cycle, costing five health and 40 happiness,
with the released status messages, hurt sound and hurt clip. A hit wakes a
roosting Ignivorus immediately or starts the Varasuchus wake-up animation; both
are prevented from falling back asleep for 600 ticks. The original sensory
disturbance system requires sound-observation support, and the single-clip
display cannot layer hurt over the sleep exit animation.
[Grooming and sleep scenario](../tests/scenarios/p2ay-saints-dragons-grooming-sleep.txt)

The [player movement scenario](../tests/scenarios/p2-dragonlord-moves.txt) checks the Dragonlord midair jump and Blood Tempest double-tap dodge. Movement uses the vanilla input packet (four server ticks approximate the 200 ms tap window); optional named actions are unbound by default to avoid shadowing vanilla bindings. Dodge uses a 30-tick cooldown and eight invulnerable ticks. Landing after a midair jump tracks the four-block peak drop and deals 20 damage with 0.75 upward impulse within 7.5 blocks, with native owner/ally filtering (see [player_moves.rs](../mods/saints-dragons/src/player_moves.rs)). A second airborne jump starts native fall-flying and cancels the pending shockwave; subsequent jumps request a 40–51-tick companion boost on an 80-tick cooldown. The R action reads the selected hotbar slot on the server: Dragonlord sword produces three timed damage pillars (50-tick cooldown), while Blood Tempest katana dashes at most 20 blocks using native collision checks and applies 2.65x attack damage with a five-jump lightning chain (60-tick cooldown). Released pillar models and chain arcs render through the companion; other dash and impact visuals remain approximate. Exact movement contact checks and persistent pillar entities remain pending.

A player wearing all four Dragonlord pieces ignores fire, explosions and falls
of 16 blocks or less. The Dragonlord sword sets what it hits on fire for eight
seconds. The server sees these through the generic `moddedmc:equipment` hooks.
[Dragonlord scenario](../tests/scenarios/p2as-saints-dragons-dragonlord-set.txt)

The Draconic Codex opens a standalone companion book from either hand or when used
on a dragon, with a text dialog fallback. It reads owner-specific records from the save, persists refreshed
loaded dragon stats, and tracks taming, death, and binder transfers. Selecting
a record opens physiology and released ecology text; the owner can confirm
removal of a record without destroying its dragon or binder. Gender and
biome are included when available (the latter via the active generator's
biome list and a loaded-block membership check). Binder capture snapshots
the live dragon before discarding it, committing its bound record only after
discard succeeds. The companion draws the released background, tabs and stat
icons, supports scrolling, ecology sections, typed ally controls and periodic
refresh, and displays the effective armor attribute when available. Portraits
use the released species/age framing and mouse rotations, live display clips
or the released static baked poses with saved textures. Ecology pages draw
item stacks and the ready brushing indicator cycles every three ticks.
Cross-dimension discovery remains outstanding; in-client visual parity has
not been verified.
Destroyed-binder handling needs an item-stack destruction callback with its
NBT and removal cause, rather than guessing that a missing dropped item died.
Owned adult Cindervane, Atroxiia and Stegonaut accept saddles and then
chests, preserve both attachments in entity and binder data, and drop
the attachments on death. The original rider inventory still needs typed dynamic attachment slots and atomic entity-backed inventory transactions for cargo and equipment removal. Armor and sword active abilities use companion actions and equipped-slot reads ([player_moves.rs](../mods/saints-dragons/src/player_moves.rs)); katana chain lightning uses a synchronous successful direct melee hit callback
(once per attack, including lethal hits), and attackability, mutual alliance
and generic tame/ownable target checks. Attributed damage is available.
In 0.9.88, `DragonMeleeHitContext.observeResult` calls
`BloodTempestKatanaAbility.onSuccessfulKatanaHit`: the chain visits up to five
distinct targets, choosing the nearest bounding box within eight blocks of
each previous target's center, excluding electricity-immune targets. It deals
10 player-attributed attack damage initially, multiplied by 0.75 each jump;
if no first jump exists, it attempts the same damage on the original target
if still eligible. The custom five-tick Blood Tempest lightning arc now uses
the release's three red ribbon layers with grow/flash/retract; Raevyx uses
three blue-white layers with seeded flicker/fade. The release's electric spark
midpoint particles remain. [Chain regression scenario](../tests/scenarios/p2-katana-chain.txt).
The rendered appearance and timing need a real-client check.
Swarm pacification reads all four equipped slots in [swarm.rs](../mods/saints-dragons/src/swarm.rs).
[Codex scenario](../tests/scenarios/p2ad-saints-dragons-codex.txt)
[Standalone typed ally scenario](../tests/scenarios/p2-codex-screen.txt) (pending coordinator run)
[Codex portrait and ecology payload scenario](../tests/scenarios/p2-codex-portraits.txt) (pending coordinator run)
[Codex detail and binder removal scenario](../tests/scenarios/p2c-items.txt)

The Codex Ally tab saves up to ten online players by UUID per owner, shared
across all their dragons and dimensions and persisted through world saves.
All eight dragon combat filters and recruited Ivy honor the owner's list and
the ownership of mod companions; Cindervane's fire aura and Stegonaut's live
and portable buffs also use the released owner/ally rules. The companion accepts typed names for add/remove; the fallback dialog
uses a paged online-player selector and supports offline removal. Native pet
ownership and scoreboard alliances are also checked. Operator
level two /dragonally add, remove, list and clear use typed entity and string
arguments and server-wide player lookup.
[Dragon ally command scenario](../tests/scenarios/p2-dragonally-command.txt).
[Global ally scenario](../tests/scenarios/p2-ally-list.txt) (pending coordinator run).

Ivy's spawn egg creates the saved `saintsdragons:ivy_oleander` entity. The
asset pack includes her original rig. The server reads the 25 released
dialogue definitions and presents their English text and choices in stock
dialogs. The Cindervane quest can recruit Ivy, the owner can cycle her
follow, stay and wander orders, and the recruited dialogue offers a plushie
bound to Ivy and her owner. The plushie recalls loaded Ivy in the same
dimension and sends the teleport sound and particles. The stock-client
scenario followed the recruitment dialogue, earned the plushie, verified
its UUID after recall on a restarted server, and rejected a different player.
Her companion inventory uses a stock chest screen with four armor slots, a
weapon slot and 27 storage slots, restricted to her recruited owner within
eight blocks. Inventory transactions persist by Ivy's UUID. Equipment affects
melee damage and armor attributes. Recruited Ivy retaliates against attackers
and protects her owner while following, with the released boxing and sword
attack timings and melee animation clips. The stock menu does not provide the
original equipment slot layout or automatic shift-click routing. The released dash and lethal-hit downed entry, and merchant close dialogue resumption, are implemented in [ivy.rs](../mods/saints-dragons/src/ivy.rs). While following, Ivy now responds to fresh owner attack history, prioritizing retaliation and owner defense, and can resume targeting after recovery. Named tags are refused without consumption. Passive cooked edible food recovery starts below 20 health, consumes a storage item at tick 14 of a 20-tick animation, heals eight health, and has a 100-tick cooldown. A bound Stegonaut binder in her inventory supplies the same portable aura as a player's binder when Ivy is within the owner's 16-block search area. [Named-tag refusal and merchant close scenario](../tests/scenarios/p2-ivy-merchant-close.txt) (pending coordinator run). Equipment wear, milk and combat retreat recovery, water clutch, exact friendly-hit reactions, venom projectiles and chatter remain pending.
Custom name input, cross-dimension recall and client dialogue animations
remain pending.
[Ivy recruitment scenario](../tests/scenarios/p2ae-saints-dragons-ivy.txt)
[Ivy companion scenario](../tests/scenarios/p2az-saints-dragons-ivy-companion.txt)

Ivy's boxing and sword dashes use the released moving-target pressure,
retreat direction, action durations, impulses and cooldowns. Saved downed
state supports a 90-second bleed-out, three finishing hits, owner revival
interactions, healing and the 30-tick arise animation. The final damage hook
allows recruited Ivy to enter downed at one health on a lethal hit after
mitigation, except for damage that bypasses invulnerability. Varasuchus
halves drowning damage using the same hook. Tailguard parry is wired to rider ability input in [combat_varasuchus.rs](../mods/saints-dragons/src/combat_varasuchus.rs). Ivy consumes one stored Totem of Undying before entering
downed on lethal damage that does not bypass invulnerability, restores one
health, and receives the released regeneration, absorption and fire resistance
effects after the damage commits. Inventory revision checks prevent stale
open menus from restoring consumed items; the owner must reopen the menu.
Ivy broadcasts vanilla entity event 35, plays the released sound and clears all active effects. The Java release saves Ivy herself, not her owner.
[Ivy companion scenario](../tests/scenarios/p2az-saints-dragons-ivy-companion.txt)
also covers a persisted totem and the subsequent lethal hit without it.
The host also
lacks full aggro-memory clearing and heal-event interception. The
[Ivy downed scenario](../tests/scenarios/p2ba-saints-dragons-ivy-downed.txt)
seeds the saved state to exercise its supported behavior.
[Final damage scenario](../tests/scenarios/p2bc-saints-dragons-hurt-final.txt)
exercises natural downed entry and drowning damage.

## Released creative tab

Checked entries have an item registration in P2. Each still needs its original behavior and appearance checked.

- [x] `ModItems.RAEVYX_BINDER`
- [x] `ModItems.CINDERVANE_BINDER`
- [x] `ModItems.STEGONAUT_BINDER`
- [x] `ModItems.VARASUCHUS_BINDER`
- [x] `ModItems.IGNIVORUS_BINDER`
- [x] `ModItems.VOLITANS_BINDER`
- [x] `ModItems.NULLJAW_BINDER`
- [x] `ModItems.ATROXIIA_BINDER`
- [x] `ModItems.DRACONIAN_CONTROLLER`
- [x] `ModItems.DRACONIC_CODEX`
- [x] `ModItems.DRAGON_BRUSH`
- [x] `ModItems.GOLDEN_DRAGON_BRUSH`
- [x] `ModItems.SCALE_PLUCKER`
- [x] `ModItems.HEARTY_DRAGON_MEAL`
- [x] `ModItems.CREATIVE_DRAGON_TAME_MEAL`
- [x] `ModItems.CREATIVE_DRAGON_GENDER_WAND`
- [x] `ModItems.CREATIVE_DRAGON_VARIANT_BRUSH`
- [x] `ModItems.RAW_MOOP`
- [x] `ModItems.COOKED_MOOP`
- [x] `ModItems.BUCKET_OF_MOOP`
- [x] `ModItems.RAW_MOSSBACK`
- [x] `ModItems.COOKED_MOSSBACK`
- [x] `ModItems.RAEVYX_SCALE`
- [x] `ModItems.RAEVYX_WING_HIDE`
- [x] `ModItems.RAEVYX_WINGTALON`
- [x] `ModItems.CINDERVANE_SCALE`
- [x] `ModItems.STEGONAUT_SCALE`
- [x] `ModItems.VARASUCHUS_SCALE`
- [x] `ModItems.IGNIVORUS_SCALE`
- [x] `ModItems.IGNIVORUS_TOOTH`
- [x] `ModItems.IGNIVORUS_WING_HIDE`
- [x] `ModItems.IGNIVORUS_HEART`
- [x] `ModItems.VOLITANS_SCALE`
- [x] `ModItems.VOLITANS_SPINE`
- [x] `ModItems.ATROXIIA_SCALE`
- [x] `ModItems.DRACONIAN_FLESH`
- [x] `ModItems.RAEVYX_EGG`
- [x] `ModItems.CINDERVANE_EGG`
- [x] `ModItems.STEGONAUT_EGG`
- [x] `ModItems.VARASUCHUS_EGG`
- [x] `ModItems.IGNIVORUS_EGG`
- [x] `ModItems.VOLITANS_EGG`
- [x] `ModItems.ATROXIIA_EGG`
- [x] `ModItems.IVY_OCTOPUS_PLUSHIE`
- [x] `ModItems.SEARING_COAL`
- [x] `ModItems.ANCIENT_DRAGONITE_FRAGMENT`
- [x] `ModItems.DRAGON_SEAL_STONE`
- [x] `ModItems.DRAGON_BINDER_CORE`
- [x] `ModItems.RAW_WORLDROOT`
- [x] `ModItems.WORLDROOT_INGOT`
- [x] `ModItems.WORLDROOT_SWORD`
- [x] `ModItems.WORLDROOT_PICKAXE`
- [x] `ModItems.WORLDROOT_AXE`
- [x] `ModItems.WORLDROOT_SHOVEL`
- [x] `ModItems.WORLDROOT_HOE`
- [x] `ModItems.DRAGONHEART_CHUNK`
- [x] `ModItems.DRAGONHEART_ALLOY`
- [x] `ModItems.ARROW_OF_VENOM`
- [x] `ModArmors.DRACONIAN_HELMET`
- [x] `ModArmors.DRACONIAN_CHESTPLATE`
- [x] `ModArmors.DRACONIAN_LEGGINGS`
- [x] `ModArmors.DRACONIAN_BOOTS`
- [x] `ModArmors.BLOOD_TEMPEST_HELMET`
- [x] `ModArmors.BLOOD_TEMPEST_CHESTPLATE`
- [x] `ModArmors.BLOOD_TEMPEST_LEGGINGS`
- [x] `ModArmors.BLOOD_TEMPEST_BOOTS`
- [x] `ModItems.BLOOD_TEMPEST_KATANA`
- [x] `ModArmors.DRAGONLORD_HELMET`
- [x] `ModArmors.DRAGONLORD_CHESTPLATE`
- [x] `ModArmors.DRAGONLORD_LEGGINGS`
- [x] `ModArmors.DRAGONLORD_BOOTS`
- [x] `ModItems.DRAGONLORD_SWORD`
- [x] `ModPotionItems.POTION_OF_TIDEGUARD`
- [x] `ModPotionItems.POTION_OF_SEARING`
- [x] `ModItems.BLEEDING_BOLT_MUSIC_DISC`
- [x] `ModItems.MOSSBACK_MUSIC_MUSIC_DISC`
- [x] `ModItems.IGNIVORUS_INCUBATOR_BLOCK`
- [x] `ModItems.DRAGONHEART_ORE`
- [x] `ModItems.DRAGONHEART_ALLOY_BLOCK`
- [x] `ModItems.DRAGONHEART_BLOCK`
- [x] `ModItems.DEEPSLATE_WORLDROOT_ORE`
- [x] `ModItems.WORLDROOT_BLOCK`
- [x] `ModItems.RAW_WORLDROOT_BLOCK`
- [x] `ModItems.DRACONIAN_PELLUCIDA`
- [x] `ModItems.DRACONIAN_NUCLEUS`
- [x] `ModItems.DRACONIC_CRUCIBLE`
- [x] `ModItems.RAEVYX_SPAWN_EGG`
- [x] `ModItems.CINDERVANE_SPAWN_EGG`
- [x] `ModItems.STEGONAUT_SPAWN_EGG`
- [x] `ModItems.VARASUCHUS_SPAWN_EGG`
- [x] `ModItems.IGNIVORUS_SPAWN_EGG`
- [x] `ModItems.VOLITANS_SPAWN_EGG`
- [x] `ModItems.NULLJAW_SPAWN_EGG`
- [x] `ModItems.ATROXIIA_SPAWN_EGG`
- [x] `ModItems.MOSSBACK_SPAWN_EGG`
- [x] `ModItems.LATCHER_SPAWN_EGG`
- [x] `ModItems.WINGED_SPAWN_EGG`
- [x] `ModItems.WHETTLED_SPAWN_EGG`
- [x] `ModItems.DRACONIAN_SWARM_SPAWN_EGG`
- [x] `ModItems.MOOP_SPAWN_EGG`
- [x] `ModItems.IVY_THE_MERCHANT_SPAWN_EGG`

## JAR data folders

| Folder | Files |
| --- | ---: |
| `advancements` | 84 |
| `damage_type` | 1 |
| `dialogues` | 25 |
| `draconic_crucible` | 1 |
| `dragon_attributes` | 8 |
| `dragon_chest_loot` | 3 |
| `dragon_variants` | 8 |
| `ivy_chatter` | 9 |
| `ivy_trades` | 1 |
| `loot_tables` | 45 |
| `recipes` | 55 |
| `starcatcher` | 1 |
| `structures` | 7 |
| `tags` | 65 |
| `worldgen` | 19 |

## JAR resource folders

| Folder | Files |
| --- | ---: |
| `animations` | 35 |
| `blockstates` | 17 |
| `codex` | 16 |
| `geo` | 51 |
| `lang` | 2 |
| `models` | 146 |
| `particles` | 116 |
| `saintsdragons.png` | 1 |
| `shaders` | 3 |
| `sounds` | 259 |
| `sounds.json` | 1 |
| `textures` | 1046 |

Bound Stegonaut binders refresh portable Resistance I and Absorption I every 20 ticks with cleanup when removed. [Portable aura scenario](../tests/scenarios/p2-stegonaut-portable-aura.txt).

Dragon taming, hurt and combat particles, near-ground flight dust, Raevyx rend/dive dust and Atroxiia quake bursts are ported. [Particle scenario](../tests/scenarios/p2-particles.txt). Additional original particle emissions and renderers remain pending; several release textures are already used by [vfx.rs](../mods/saints-dragons/src/vfx.rs).

Moop swim and panic goals and water movement are ported. See the Moop scenario.

The host and companion expose semantic rider actions.

Raevyx rider abilities include storm tap, ground rend hold, beam, roar, flex and movement controls. [Scenario](../tests/scenarios/p2c-raevyx-combat.txt).

Volitans rider input and medium-specific movement are ported.

Varasuchus rider combat and movement are ported.

Nulljaw rider combat, cloak and teleport controls are ported.

Stegonaut rider movement, melee and ground abilities are ported.

Atroxiia rider melee, movement and frost abilities are ported.

Species-specific damage and terrain rules now use the host freeze-eligibility and damage-source calls: Atroxiia only freezes eligible targets; Nulljaw ignores direct shulker bullets; Volitans ignores pufferfish damage; wild Stegonauts cannot damage one another and their fall damage is scaled to 30% in the final-damage callback. Varasuchus rider claws break waterlogged solids subject to the same hardness cap as dry blocks, but leave liquid blocks alone. The host still clamps excess freeze duration, applies the fall multiplier after mitigation and lacks the full dragon-specific terrain configuration. [Incoming-damage scenario](../tests/scenarios/p2-species-details.txt) (pending coordinator run).

Ignivorus rider movement and abilities are ported.

Cindervane rider combat now dispatches the owner's first-seat left-click, X melee toggle, R double bite, H volley, G hold/release Fire Body and Ctrl+4 flex through the companion. The released timings, cooldowns, forward bite sweep, slash grab motion and animated grab locator, rider-aimed volleys, fire aura protection, landing/forward crash blast and flex movement lock are implemented. Remaining gaps: exact flight transitions, native projectile entity and physics, complete elemental immunity, custom crash explosion and exact custom visuals. [Registration and manual companion scenario](../tests/scenarios/p2c-cindervane-rider.txt).


Fire Body samples terrain six times per tick with a 12% ignition chance. Magma Volley ignites eligible air in its 3-by-3 impact layer; a crash carves the released nine-block-radius parabolic imprint, preserves bedrock and obsidian and awards Tactical Nuke. Terrain honors mob griefing. [Terrain scenario](../tests/scenarios/p2-cindervane-terrain.txt).

Grounded, ridden Fire Body advances vanilla furnaces by 4 ticks and smokers and blast furnaces by 6 per tick within a 3.5-block sphere, refreshes lit time to 20 and awards cook_with_fire_dragon on reaching total minus one. The serial content tick applies the generic host operation; exact flight transition modes and support probes still depend on the rider controller. [Furnace scenario](../tests/scenarios/p2-cindervane-furnace.txt).

Flight route search follows Java 0.9.88 AsyncDragonPathfinder: 128-block planning limit, 26 neighbors, weighted Euclidean costs, deferred shortcut validation and repair, 24-block shortcuts, four-block swept segments, 16-node goal sweep and 5000 confirmed node cap. The Rust content tick advances requests in round-robin slices (16 steps, 128 collision-cell reads, 350 microseconds), caching queried collision shapes rather than taking Java's whole-region snapshot. Swarm ignores Pellucida during planning; native movement remains carrier-dependent. Cindervane cruise/pack, Raevyx autonomous flight and swarm wander/pursuit consume the result, retaining immediate collision checks. [Flight route scenario](../tests/scenarios/p2-flight-route.txt) (pending coordinator run).

## Ranked remaining parity work (source audit at `f07c64d`)

This audit compares the 0.9.88 Java source with the Rust and companion implementation on main. "Implemented" means code exists, not that a scenario or live-client check passed. Historical scenario reports above are retained; the current manual acceptance list is in [CLIENT-CHECKS.md](CLIENT-CHECKS.md#saints-dragons-0988). Rank prioritizes player-visible correctness and shared dependencies.

1. **[host]** Native dragon dimensions, multipart hit/interaction routing, push rules, entity-aware Pellucida collision, exact eyes, world-border/entity clearance and spawn predicates. Java: `DragonPartManager`, `DragonPartEntity`, `MessageDragonPartAttack`, `DragonSpawnRules`.
2. **[host]** Cancellable projectile impact and custom projectile collision/ownership, precise freeze eligibility/counter, damage-source and pre/post-hit semantics, beam redstone, dragon-specific terrain/explosion hooks, persistent nonliving effects. Java: `IgnivorusFireballEntity`, `VolitansPoisonOrbEntity`, `GroundFissureEntity`, `DragonRedstonePowerManager`, `AtroxiiaFrostImpact`.
3. **[mod-only]** Exact combat AI and movement: aerial tactics, water entry/escape/chase, landing and recovery, target and pack/mother assignment, adaptive learning for Ignivorus and Volitans, remaining attack scoring and spacing. Cindervane ground attacks now score target motion, health, engaged enemies and the last two moves, respect sight and chase commitment; Raevyx tracks visible target UUIDs and beam windup responses separately on ground/in air, decays learned habits and uses ground beam spacing and attack weight. The direct-target Rust beam still lacks Java's predictive aim, shot grace and retreat, and the ABI does not expose exact target hurtTime (used to exclude forced movement). See [combat.rs](../mods/saints-dragons/src/combat.rs), [learning.rs](../mods/saints-dragons/src/combat_raevyx/learning.rs) and [combat sight scenario](../tests/scenarios/p2-combat-ai.txt) (not yet run). Java: `DragonCombatLearning`, `DragonTacticalPlannerBehaviour`, `IgnivorusAirCombatBehaviour`, `CindervaneAirCombatMovementBehaviour`, `VolitansWaterCombatMovement`; Rust: [flight_route.rs](../mods/saints-dragons/src/flight_route.rs), [combat_ignivorus.rs](../mods/saints-dragons/src/combat_ignivorus.rs), [combat_volitans.rs](../mods/saints-dragons/src/combat_volitans.rs).
4. **[host]** Owner last-attacked entity, raid context, sound/hearing and scent/vision observation inputs, falling-owner state and safe catch. Java: `DragonRescueFallingOwnerBehaviour`, `DragonHearingListener`, `DragonScentSensor`, `DragonTargetMemory`; port has ordinary follow but no falling-owner rescue controller.
5. **[host]** Rider cargo and saddle/chest removal, transactional entity-backed inventory, Ivy item wear/consumption and healing events, filled-binder destruction with stack identity and cause, cross-dimension entity lookup/recall. Java: `DragonChestCarrier`, `DragonSaddleCarrier`, `AbstractDragonBinderItem`, `IvyOctopusPlushieItem`.
6. **[mod/host/companion]** Ivy combat retreat recovery, milk recovery (needs active-effect enumeration with effect category and safe container remainder handling), water clutch (needs pre-movement fall-distance access), venom projectile combat (needs custom projectile spawning and impact interception), exact friendly-hit reactions and nine chatter pools including rename refusal dialogue (needs companion overhead chatter replication/rendering and clearing). Passive food recovery, owner offense, portable binder aura and named-tag refusal are implemented. Java: `IvyTheDragonMerchant`, `IvyCombatBrain`, `IvyChatterRegistry`, `IvyChatterReloadListener`.
7. **[mod-only]** Remaining species ability details, terrain and immunity rules: Ignivorus airborne attack selection and bulldoze, Raevyx beam/dive, Volitans burrow/attacker reactions and poison-related magic immunity, Varasuchus exact claw geometry and dragon-specific terrain configuration, Stegonaut projectile memory and pre-mitigation fall scaling, Nulljaw teleport clearance and retaliation pack alerts. Atroxiia freeze eligibility and direct shulker/pufferfish and wild Stegonaut damage vetoes are implemented; excess freeze duration remains host-dependent. Some remaining details need #1–2 first.
8. **[mod-only, with sensory dependency on #4]** Exact family spawning and defense, sleep pressure/depth and disturbance behavior, falling-owner rescue controller after #4, and exact Ivy dance eligibility (idle variants and climbing). Breeding love, cause and cooldown and stun hold/recovery use remaining loaded-entity ticks in saved entity NBT. Varasuchus rodeo counters are transient, as in Java; a loaded mounted wild ride starts a fresh attempt. Wild juvenile parent following now covers Cindervane, Raevyx, Ignivorus, Varasuchus, Stegonaut and Nulljaw. Juveniles and Ivy scan for actively playing jukeboxes (the saved playback counter distinguishes a stopped record), with the released 16-block cube and ten-tick cadence. A synthetic twenty-juvenile scan test measures the mod-side loop; actual host chunk-read cost remains unmeasured. A block-entity spatial query could reduce its cost. Java: `DragonEntity`, `DancingEntity`, `DragonSleepBehaviour`; see [family.rs](../mods/saints-dragons/src/family.rs), [dance.rs](../mods/saints-dragons/src/dance.rs), and the [restart scenario](../tests/scenarios/p2-care-state-restart.txt) (not yet run). Sensory disturbance still needs per-dragon observations with sound kind, position, confidence, source UUID and age.
9. **[companion]** Real-client acceptance of seats, grab, keybinds, screens, visuals, sound and totem; exact animation blending, camera and screen impulses, multipart targeting, afterimages, equipment and held-item models, custom particle/beam/projectile renderers. Java: `MessageCameraImpulse`, `MessageBloodTempestAfterimage`, `DragonEquipmentLayer`, `DragonSeatAnchoredCamera`, `ArrowOfVenomModel`, `DraconicCrucibleRenderer`.
10. **[host]** Crucible two-block shape and automation, painting facing rotation, exact natural spawn and Mossback juvenile group rules, contextual breeding and pre-hit advancement hooks. Java: `DraconicCrucibleBlock`, `DraconicCrucibleBlockEntity`, `DragonSpawnRegistry`, `DragonBreedingRules`.
11. **[companion]** Nucleus wave bar and battle music, encounter and block animations, Ivy custom dialogue/name and inventory screens, remaining localization. Java: `MessageSwarmWaveBar`, `MessageSwarmBattleMusic`, `MessageDialogueNameInput`, `DraconianNucleusAnimations`.
12. **[host]** Entity-bound looping/timed audio, independent effect icon/particle flags, carrier sound suppression and exact keyframe cues (companion clock). Java: `MessageDragonMovingSound`, species `SoundProfile` and animation handlers.
13. **[mod-only]** Release config and datapack reload parity (attributes, ability overrides, spawn settings, loot/trades/chatter, griefing), chest-loot injection and Starcatcher Moop fishing integration where the dependency exists. Java: `SaintsDragonsConfig`, `DragonAttributeConfigLoader`, `DragonChestLootReloadListener`, `IvyTradeReloadListener`; data: `starcatcher/fish/moop_fish.json`. Confirm host loot injection support before implementing.
14. **[companion]** Upstream diagnostic parity: optional brain/path/vision/ability-box debug messages; no corresponding release-specific UI in the port (`MessageDragonBrainDebug`, `MessageDragonPathDebug`, `MessageDragonVisionDebug`, `MessageDragonAbilityDebugBox`).
