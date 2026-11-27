# Saint's Dragons 0.9.88 parity checklist

The reference is the official Minecraft 1.20.1 Fabric JAR from [Modrinth](https://modrinth.com/mod/saints-dragons/versions), SHA-256 `1093be9e314248646162bf8362e30cee63f6c6ed57d1f6590ce784cc69aee532`. REA archive evidence `ev_dbd7214a5094dcb802068d8f9430b4c66b34d51a471a6633edef19c6136ff959` identifies the package and enumerates its members. The matching MIT source revision is `13aeb2eb2c820fbd89b519946d4c3a7f563b7ad4`. The user confirmed Leon Saint's permission for this port to use the upstream assets.

The JAR contains 3,781 files, including 1,712 compiled classes. The full member inventory and 254 sound event IDs are in the worktree's `target/saints-dragons-parity/`. Static inventory establishes the files and release definitions. Behavior needs source or bytecode tracing and a scenario on both releases.

## Gameplay

| Area | Current P2 coverage | Remaining parity work |
| --- | --- | --- |
| Adult creature roster | Fourteen creature types with carriers, saved identity and spawn eggs. The Draconian Swarm egg spawns a Latcher, Winged and Whettled together. Swarm target selection, retaliation and species-specific combat with bite, pull, dive, claw and horn charge. Ivy trades from the released table | Ivy's companion inventory and combat, resuming her dialogue after the trade screen closes, exact flight navigation and collision, and exact spawn restrictions and attributes |
| Movement and lifecycle | Shared ground AI, baseline flight and swimming, partial Cindervane wild AI, and released Moop idle, swim, fast swim and on-land animation selection, released gender and skin selection for adult spawn eggs and wild spawns, and natural spawning in the released biome pools for Cindervane, Atroxiia, Nulljaw, Moop and Mossback, and the custom Raevyx storm, Stegonaut lush cave and Volitans underwater spawners | Per-species brains, navigation, growth and breeding |
| Combat | Cindervane bite, double bite, slash grab, magma volley and fire body in varying degrees. Stegonaut AI bite, attributed chin slam, retaliation, owner defense, nearby pack and baby protection, and tame owner buff aura with effect removal. Worldroot deals 1.5? direct melee damage to the eight dragon types, Blood Tempest deals 3? to Raevyx, and Dragonlord deals 5? to Ignivorus | Remaining per-species combat, Stegonaut rider ground eating and ground slam/pillars and portable binder aura, boss phases, projectiles, status effects, damage types, AI targeting and terrain effects |
| Taming and riding | Cindervane feeding, ownership, orders, saddle, two seats and flight controls; stun taming and tame healing for Atroxiia, Ignivorus, Raevyx and Volitans; Stegonaut food taming and the Varasuchus rodeo; tamed Varasuchus food healing and wild roost sleep refusal; breeding and egg laying for Atroxiia, Cindervane, Ignivorus, Raevyx, Stegonaut and Varasuchus, underwater eggs for Volitans and live birth for Nulljaw; the Creative Dragon Taming Meal sets persistent ownership on the eight dragon species; hunger, happiness decay and slowdown, roster melee attribute scaling, juvenile food taming, growth feeding and raw-moop stunting; grooming hurt clips and roost damage wake-up reactions; breeding XP and stats, credited hunting and fresh-drop eating, owner-bed sleep | Nulljaw wild juvenile feeding, contextual breeding advancement, sensory disturbances, codex hunger display; equipment, inventories, Codex handling when a filled binder is destroyed, and additional recall behavior; special rider controls |
| Items and creative menu | 101 registered player-facing items, including the Draconic Crucible, the Ignivorus Incubator, six ore and storage blocks, Draconian Pellucida, and two Creative appearance tools. The companion lists all 101 in the **Server Mods** Creative tab in release order. Normal and golden brush scale drops and happiness recovery, the shared ten-tick brush cooldown, Scale Plucker, recipes, wear and persistent regrowth. Bucket of Moop with capture, restart persistence and release, Moop and Mossback foods, Hearty Dragon Meal nutrition, the Tideguard and Searing potion effects, both music discs, seven hatchable dragon eggs, the Arrow of Venom, and eight dragon binders with the released recipes, models and tooltips. The full Dragonlord set's fire, blast and short-fall immunity and the Dragonlord sword's fire on hit, and full Draconian armor swarm pacification with retaliation | Dragonlord double jump, flight and landing shockwave, the Blood Tempest dodge, Blood Tempest and Dragonlord sword active abilities and the katana's chain lightning, worn armor models, and remaining equipment components |
| Blocks and progression | Six ore and storage blocks, compaction recipes, partial food and loot, a persistent Draconic Crucible with a stock chest menu, explicit start, processing locks, fuel routing, live shared slot refresh and released recipes, including the Ignivorus Incubator, natural Deepslate Worldroot and Dragonheart ore, the End Nucleus structure, both dragon roosts with their sleep anchors, Cindervane and Raevyx egg trampling, and Ivy?s house | The advancements awarded by abilities that are not ported yet (Fire Hydrant, Tactical Nuke, cooking with a fire dragon, the Raevyx dive exit and hitting a baby) and recipe unlocks from advancement rewards, original Crucible screen artwork and animations, some legacy decorations, remaining custom crafting and advancement predicates, and Pellucida hardness, sound and swarm-specific collision |
| Audio | 259 released recordings and event definitions in the asset build, Cindervane action cues, and hurt and death cues for the roster except Moop. A connected bot verified Cindervane, Raevyx and Mossback. A Java 26.3 client with no companion verified Bleeding Bolt and Mossback Music playback and ejection | Remaining per-species playback, animation keyframe cues, footsteps, takeoff and landing, and moving or looping sources |
| Presentation | Adult rig path with Creative gender and skin changes in the Fabric companion, and stock display fallbacks | Juvenile rigs, overlays, particles, special renderers, equipment visuals, screens and translations |
| Integration | Stock 26.3 client fallback and versioned mod interfaces | Separate work for third-party integrations, configs, networking, and an upstream oracle for every mechanic |

The Crucible implements the released fuel tiers, nine-slot recipe matching, processing times, and pending output persisted with the block. The 14 upstream processing definitions are present. The Crucible also reads ordinary smelting recipes from the merged datapacks. A stock chest screen now exposes the grid, fuel, output and explicit start control, locks the grid during processing, routes shift-clicked fuel, and uses identity-bound reach checks. The original screen artwork, two-block shape, animation, sounds, and automation still need parity. [Stock-client scenario](../tests/scenarios/p2ax-saints-dragons-crucible.txt) (pending coordinator run).

Stegonaut AI combat selects bite or chin slam with the released 26-tick cadence,
individual cooldowns, tick-13 impact, hunger scaling and knockback. Chin slam
uses the new attributed mob-attack call for (8 + 4) times hunger scaling.
Its tame aura refreshes Resistance I and Absorption I on its owner and same-owner
dragons and explicitly removes both when a target leaves or the source dies,
unloads or is captured. [Combat scenario](../tests/scenarios/p2c-stegonaut-combat.txt)
checks chin-slam retaliation and aura removal with long-duration effects (pending
coordinator run). Remaining: semantic rider ability press/release and melee-mode inputs
for the ground-eating projectile and chained ground slam/pillars; inventory enumeration
for portable binder auras (the existing exact stack-data lookup cannot discover
bound binders); separate particle/icon visibility;
native pet ownership, ally lists, raid membership, exact visibility, last attack and
projectile damager history. The hurt callback cannot inspect the damage source entity
or transform incoming damage for 30% fall damage and wild Stegonaut immunity; a player
attack callback is needed for the pre-damage baby-hit advancement.

The Ignivorus Egg advances through three crack stages while on the Ignivorus
Incubator. Progress persists across restarts. The hatchling starts with 90 health,
uses the original juvenile rig in the companion, and grows over 24,000 loaded
mob ticks. The test seeds progress near stage boundaries. Hatch advancement and
natural acquisition in the Ignivorus roost remain pending.

Stegonaut, Varasuchus, and Atroxiia eggs use the same saved progression with
their released 30,000, 24,000, and 24,000 tick timers and respective starting
health of 50, 80, and 30. The original juvenile rigs are included in the asset
build. Varasuchus hatched in the End matures into the Void-Kissed variant.

The Volitans Egg requires waterlogging for its 18,000-tick incubation. It
preserves progress while dry, retains water when it hatches, and produces a
juvenile with 60 starting health.

A Cindervane clutch can hold up to three eggs, advances through three
crack stages in 12,000 loaded ticks, and produces one 40-health hatchling
per egg. The original clutch and juvenile models are included. Piebald appearance is rolled at hatch and applied at maturity. Trampling remains pending.

The Raevyx Egg incubates in 18,000 ticks normally and at the 9,600-tick
rate during thunder. Lightning within the release's three-block expanded
collision box hatches it immediately. The server scenario verified thunder
and summoned lightning. All seven egg blocks report the hatch stage (0 to 2)
to comparators. The Volitans scenario covers adjacent comparators and comparators
reading through a solid block. Trampling and the hatch advancements remain
pending.

The Arrow of Venom uses the released recipe and item model. Bows and crossbows
launch it at 1.35 times normal speed with no water drag. A damaging hit clears
the target's hurt cooldown and applies Poison II for 30 seconds and Blindness
for 5 seconds. The server scenario checks water motion, both weapons, and
picking up the spent arrow after a restart. The flight and impact renderer,
Ivy's friendly-hit rules, and Volitans venom suppression remain part of the
projectile and creature parity work.
[Arrow of Venom scenario](../tests/scenarios/p2z-saints-dragons-venom-arrow.txt)

The eight binders capture an owned dragon and release it with the same UUID,
health, name, and saved mod state. The stock-client ground checks cover all
eight species, blocked release, wild capture denial, and a restart with a
filled binder. Raevyx, Cindervane, Ignivorus, and Volitans also have a five-block
air release, also verified with a stock-protocol client. They descend using
the port's shared flight goal. Capture requires the dragon to be free of
passengers and vehicles. The original tamed flight AI,
Pocket Dragon advancement, and Codex tracking when a binder is destroyed remain
pending.
[Binder scenario](../tests/scenarios/p2aa-saints-dragons-binders.txt)

The three swarm spawn eggs use the released creature identities and starting
health of 12, 6, and 16. The group egg spawns all three in order along the
x-axis at two-block intervals and consumes one item in survival. The
companion pack includes all three original rigs and animation sets. A stock
client receives display fallbacks. The scenario verified the four eggs,
persistence, spawning without gravity, and original hurt and death audio.
Suspended wandering uses the released movement speeds and takes clear, straight
routes. Pathfinding around obstacles, the encounter's
spawn and death animations, and exact hitboxes remain pending. The stock
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
targeting. The direct flight controller approximates the original path and
collision geometry. The full Draconian armor set pacifies swarm members
unless the wearer is their last attacker. Target selection and attacks read
the four worn item IDs through the generic equipment interface.
[Swarm combat scenario](../tests/scenarios/p2aw-saints-dragons-swarm-combat.txt)

A player-placed Draconian Nucleus records its owner. The Controller toggles
owned encounters within 64 blocks, removes the active swarm when it stops an
encounter, and observes the 1,200-tick reactivation delay. An unowned Nucleus
activates for a nearby survival player. The server scenario verified all three
waves (6, 9 and 12 creatures), non-owner denial, persistence, both Controller
use paths, and the summon cue. Release parity still needs encounter music, boss
bars, advancement awards, exact spawn collision, and source combat. Controller summons keep an empty death loot
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
and keeps to its territory. Some legacy decorations still need parity.
[Roost and house scenario](../tests/scenarios/p2ai-saints-dragons-roosts.txt)

Cindervane, Atroxiia, Nulljaw, Moop and Mossback join the biome spawn pools of
their released biome tags with the 0.9.88 config weights and group sizes. The
natural spawner and chunk generation pick from vanilla and dragon entries by
weight. Each species keeps its `canSpawnHere` rule: dry, open ground, a
competing vanilla creature weight of at least five and no snowy slopes for
Cindervane, End Barrens for Nulljaw, and the `DragonSpawnRules` density limits
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
Successful breeding spawns 1–7 experience when mob loot is enabled and awards
the animals-bred statistic to the first available feeding player. The contextual
BRED_ANIMALS advancement trigger remains deferred.
[More breeding scenario](../tests/scenarios/p2at-saints-dragons-more-breeding.txt)

A wild adult Stegonaut is tamed by any of its foods. Clicking a wild adult
Varasuchus with an empty hand mounts it. It bucks in place for 90 ticks, then
rolls its 16.67 percent chance. On a success it accepts the rider and follows
them. On a failure it throws the rider off, turns on them unless they're in
Creative, and refuses another ride for five seconds.
[Wild taming scenario](../tests/scenarios/p2ap-saints-dragons-wild-taming.txt)

The server tracks the release's 41 advancements, now in the 26.3 layout, and
shows them in their own tab. Criteria from vanilla triggers, such as picking
up a Draconic Codex or a dragon egg, fire on their own. The mod awards the
ones the release grants from code: clicking a dragon, meeting Ivy, taming each
species, hatching each egg (the owner, or players within 32 blocks), a Raevyx
egg hatched by lightning, binding a dragon, and summoning, meeting and
defeating the Draconian Swarm. `/advancement` and `advancements={...}`
selectors accept their ids.
[Advancements scenario](../tests/scenarios/p2aq-saints-dragons-advancements.txt)

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

The seven roster dragons now scale their carrier attack damage attribute with
the same hunger multiplier as Cindervane's abilities, restoring their base damage
at 60 hunger or higher. Ignivorus keeps its juvenile zero damage and adult
base of 50. Cindervane, Raevyx, Ignivorus, Varasuchus and Volitans acquire their
released prey types at 60 hunger or less. After a credited kill, they look
for fresh food drops (at most 100 ticks old) around the kill for 60 ticks and
eat one every 20 ticks, restoring 10 hunger. The roster uses a generic melee
goal until its species combat is ported; dedicated aerial landing, item
particles and interaction animation blending are still pending.
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
The released tamed mounting interaction wakes the dragon immediately, rather
than rejecting the ride; tamed Varasuchus riding is outside this port.
[Varasuchus care scenario](../tests/scenarios/p2au-saints-dragons-varasuchus-care.txt)

A player wearing all four Dragonlord pieces ignores fire, explosions and falls
of 16 blocks or less. The Dragonlord sword sets what it hits on fire for eight
seconds. The server sees these through the generic `moddedmc:equipment` hooks.
[Dragonlord scenario](../tests/scenarios/p2as-saints-dragons-dragonlord-set.txt)

The Draconic Codex opens a stock-client dialog from either hand or when used
on a dragon. It reads owner-specific records from the save, updates loaded
dragons, and tracks taming, death, and binder transfers. The restart scenario
covers a named, injured dragon and an unrelated visitor. The original
companion tabs and renderer, entry removal, hunger and biome data, and
destroyed-binder handling remain pending.
[Codex scenario](../tests/scenarios/p2ad-saints-dragons-codex.txt)

Ivy's spawn egg creates the saved `saintsdragons:ivy_oleander` entity. The
asset pack includes her original rig. The server reads the 25 released
dialogue definitions and presents their English text and choices in stock
dialogs. The Cindervane quest can recruit Ivy, the owner can cycle her
follow, stay and wander orders, and the recruited dialogue offers a plushie
bound to Ivy and her owner. The plushie recalls loaded Ivy in the same
dimension and sends the teleport sound and particles. The stock-client
scenario followed the recruitment dialogue, earned the plushie, verified
its UUID after recall on a restarted server, and rejected a different player.
The original trade
and companion inventory screens, custom name input, cross-dimension recall,
merchant combat, and client dialogue animations remain
pending.
[Ivy recruitment scenario](../tests/scenarios/p2ae-saints-dragons-ivy.txt)

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
