# P2: Saint's Dragons

Port of [Saint's Dragons](https://modrinth.com/mod/saints-dragons) 0.9.88 (Fabric and Forge, Minecraft 1.20.1, released 2026-10-06), from [LilRicefield/saints-dragons](https://github.com/LilRicefield/saints-dragons) at commit `13aeb2eb2c` (the version bump the release was built from). The code is MIT. The [upstream license](https://github.com/LilRicefield/saints-dragons/blob/1.20.1/LICENSE.md) requires Leon Saint's written permission to reuse the models, textures, animations or sounds in this port, including reuse from a local JAR.

The [parity checklist](P2-DRAGONS-PARITY.md) tracks the full release, including all 101 creative-tab entries.

## Size

The released JAR contains 3,781 files. The release has five flying dragons (Raevyx, Cindervane, Volitans, Ignivorus and Nulljaw), four ground dragons (Stegonaut, Varasuchus, Atroxiia and Mossback) and the aquatic Moop. It also includes Ivy the merchant, the Draconian Swarm, projectiles and effects, eggs and breeding, equipment, crafting stations and world generation.

## P2a: one dragon, end to end

Cindervane (`RideableFlyingDragon`, 2.5 by 2.5 blocks, pack member): it spawns, walks and flies under its own AI, can be tamed, saddled and ridden in flight, takes and deals damage, dies with its loot, and survives a restart with its owner, saddle and health. Done when a harness scenario shows each of those against the same rig on Java Saint's Dragons 0.9.88.

Stock clients receive a five-part placeholder through the server resource pack. The [companion renderer](COMPANION.md#animated-displays) can replace it with the original adult rig from the 0.9.88 JAR. The invisible ravager still provides collision and riding. The display item carries the active clip for ground movement, takeoff, aerial movement, landing, Bite, Double Bite, Slash Grab and Magma Volley. Fire Body uses the separate flame effect while the movement or attack clip continues.

Build an asset-bearing local `.mmc` from the official 0.9.88 Fabric JAR using the asset reuse permission the maintainer confirmed.

```powershell
scripts\pack-saints-dragons.ps1 -Jar 'cache\saintsdragons-0.9.88+fabric-1.20.1.jar'
```

Install `target/mods/saints-dragons.mmc` on the updated server. Minecraft downloads the server resource pack when the player accepts it. With the companion, Fabric API and GeckoLib 5.5.7 installed, the pack supplies Leon Saint's original adult geometry, animations and base textures. Ignivorus's animation JSON is compacted to fit the host's asset limit with every JSON token preserved.

## P2b: adult roster

Use `/saintsdragons spawn <species> <x> <y> <z>` or a `saintsdragons:<species>_spawn_egg` on a block. All ten creatures have distinct saved types and their original adult rigs. The new types use the shared ground AI and autonomous flight where applicable. Moop uses the cod's swimming controller. Their movement selects clips from each species' animation file. Stock clients see the Cindervane fallback or a single display marker for the newer types. The port packages eight released entity loot tables and registers the six custom drops they use. Raw Mossback currently inherits the vanilla raw rabbit's food behavior.

The other nine creatures still need their release-specific attacks, AI, taming, riding, breeding and variants where applicable. Ivy, the Draconian Swarm, progression, world generation, equipment and complete loot and crafting also remain pending.

## P2c: stock item access and sound cues

Run `/saintsdragons items` as an operator on a stock Java 26.3 client. The dialog lists 61 registered items, including the ten spawn eggs. Select an item to see the `/give` command. The original 0.9.88 creative tab lists 101 entries. The full tab requires client-side UI and the remaining item mechanics.

Cindervane uses built-in sounds for idle calls, climbing wingbeats, feeding, damage, attack starts, Magma Volley and death. Cooked Mossback needs dynamic cooking support: Pumpkin currently loads datapack cooking definitions but its cooking blocks use the compiled vanilla recipes.

Build the Windows preview with `scripts\pack-mod.ps1 -Crate saints-dragons -OutDir target/mods/saints-dragons-preview`. This build uses stock models for loot and food, the Rust port's Cindervane display model, and no upstream art or audio. The asset build above includes Leon Saint's original adult dragon rigs, the models and textures for 45 inventory items, 254 sound definitions and 259 recordings. The host serves the recordings referenced by those definitions. Existing Cindervane actions emit the corresponding original sound events in that build.

The material group includes the seven scales, Volitans Spine, Draconian Flesh, and eight progression ingredients. The 0.9.88 Searing Coal recipe uses Cindervane Scale and coal. The six storage and ore blocks include released loot and compaction recipes where applicable. They use vanilla physical templates. Natural ore generation, exact tool tiers, and the remaining acquisition routes are tracked separately.

The Creative Dragon Taming Meal binds an unowned dragon to the player and starts owner following. It is reusable and requires Creative mode. Moop and Mossback are outside the dragon class. The taming advancement and each species? full owner behavior remain pending.

The normal and golden brushes shed 1?5 or 6?10 scales, respectively, from the seven tamed species with grooming loot. Brushes have 256 uses in Survival. The five-minute regrowth timer advances while the dragon is loaded, in one-second steps, and saves with the dragon. Once per cycle the owner can use the Scale Plucker to take three scales after brushing, at a cost of five health, 40 happiness, and one of 64 tool uses. Either brush can restore happiness during regrowth. A successful brush use starts a ten-tick cooldown shared by both brushes on that dragon. Sleep and hurt animation reactions and codex updates remain pending. The asset preview requires a server with the `moddedmc:mob_health` v1 interface.

The five Worldroot tools use the original 1,950 durability, default combat stats, diamond-tier mining rules and recipes. The host sends their custom defaults to stock clients. The Crucible processes Raw Worldroot into ingots. Worldroot damage bonuses against dragons and the config overrides remain pending. The tools require the `moddedmc:item_defaults` v1 interface.

All twelve Draconian, Blood Tempest and Dragonlord armor pieces use upstream base armor, toughness, durability and enchantability. Blood Tempest and Dragonlord also use their configured knockback modifiers. Dragonlord adds its released max-health modifiers. Draconian crafting recipes and all twelve inventory models are included. Equipped armor uses the stock client carrier visuals. The Crucible makes Blood Tempest and Dragonlord armor. Full-set abilities and Dragonlord armor fire and blast resistance remain pending.

The Blood Tempest Katana and Dragonlord Sword use the released damage, speed, reach, durability, enchantability and repair materials. Their original inventory models are included. The Katana's critical bonus and active ability and the Dragonlord Sword's on-hit fire and active ability remain pending.

Craft the Draconic Crucible with eight Ancient Dragonite Fragments around a blast furnace. With the companion installed, use the top as a 3?3 grid with the first row toward north. Right-click a cell with ingredients to deposit a stack, or with an empty hand to retrieve it. Use a side with fuel to load it. Use a side with an empty hand to start the next job or collect its output. Use the underside with an empty hand to retrieve queued fuel. Batches continue while the chunk stays loaded and resume after reload. The level-three recipes include the placeable Ignivorus Incubator. Ignivorus egg hatching, the Fabric screen and the two-block Crucible model remain pending.

Raw and cooked Moop and Mossback use the original food values and furnace, smoker and campfire recipes. Raw Mossback applies Poison, Nausea and Blindness for ten seconds. Moop drops cooked meat when killed while burning. Hearty Dragon Meal uses the released food values and stacks to 16.

Potion of Tideguard grants Water Breathing and Dolphin's Grace for one minute. Potion of Searing grants Strength III and Fire Resistance for eight minutes. Brew them by adding a Varasuchus Scale or Ignivorus Tooth, respectively, to an Awkward Potion. Drinking consumes the dose without returning a bottle.


## Interfaces P2a needs

Each ships with P2a and is pinned at v1 (INTERFACES-PLAN.md). Mechanics stay in the mod: flight physics, taming, pack logic and attack choice are Rust ports of the Java classes.

| Need | Where it comes from |
| --- | --- |
| Register an entity type: id, size, category, attributes, carrier, saved mod state | New. CONTENT.md step 3. |
| Spawn, find by id, move, remove | `abi/interfaces` `entities/1` (`spawn`, `position`, `move_to`, `remove`), rebuilt on `moddedmc:entities` with its borrowed-entity model. |
| AI goals with priorities and control flags, run in the entity tick | `abi/interfaces` `add_goal` and `ModApi::goal_event` (can start, continue, start, tick, stop; move, look, jump, target flags) over Pumpkin's goal selector. |
| Ground navigation to a point, holding the target | `abi/interfaces` `walk_to` (navigator re-issued each tick, arrival radius 1.5). Flight is the mod's own movement through `move_by`. |
| Target another entity | `abi/interfaces` `set_target` (`apply_mob_target`). |
| Hear damage (cancellable, with amount and cause) and death | `abi/interfaces` `DAMAGE` and `DEATH` entity events. |
| Riders: mount, dismount, the rider's movement input | `has_indirect_passenger` and the movement input in `body` exist; mount and dismount are new. |
| Owner, taming and saddle state across restarts | Saved mod state on the entity type (new, as above). |

## Order

1. Entity type registration with a carrier and saved state; a scenario spawns one, restarts, finds it again.
2. Goals, navigation and targeting from `abi/interfaces`; Cindervane's ground goals. Done: idle look and ground wander. Its targeting acts on damage, owners and packs, and following and sitting need an owner, so they come with steps 3 and 4.
3. Cindervane ignores fire, lava and falls, retaliates when attacked, and chooses Bite, Double Bite, Slash Grab or Magma Volley by range and cooldown. Fire Body runs alongside those attacks. Magma uses simulated projectiles, and Fire Body ignites living targets. Block griefing, cooking and the remaining targeting rules are pending.
4. Adult taming, feeding, owner commands, saddle and two seats. The owner UUID, order and saddle save with the mob. Equipment drops the saddle on death. Breeding, happiness, hunger and the chest inventory remain pending.
5. Rider takeoff, steering and landing use stock client jump, movement and look input. Wild adults take off during the day when there is clearance, cruise in a loaded area, and return toward the takeoff point. Weather-specific routing, pack flight and custom rider ability inputs remain pending.
6. The harness comparison against Java 0.9.88.
