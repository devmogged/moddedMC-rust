# P2: Saint's Dragons

Port of [Saint's Dragons](https://modrinth.com/mod/saints-dragons) 0.9.88 (Fabric and Forge, Minecraft 1.20.1, released 2026-10-06), from [LilRicefield/saints-dragons](https://github.com/LilRicefield/saints-dragons) at commit `13aeb2eb2c` (the version bump the release was built from; there is no tag). Code is MIT; art and audio are All Rights Reserved, so models, textures, animations and sounds live only in `mods-private`, like Create's.

## Size

About 3,200 files. Ten species (Raevyx, Cindervane, Volitans, Ignivorus, Varasuchus, Atroxiia and Nulljaw fly; Stegonaut and Mossback walk; Moop swims), their projectiles and effect entities, Ivy the merchant, 200 AI classes, 16 flight and rider classes, and GeckoLib 4.8.4 models and animations for every creature.

## P2a: one dragon, end to end

Cindervane (`RideableFlyingDragon`, 2.5 by 2.5 blocks, pack member): it spawns, walks and flies under its own AI, can be tamed, saddled and ridden in flight, takes and deals damage, dies with its loot, and survives a restart with its owner, saddle and health. Done when a harness scenario shows each of those against the same rig on Java Saint's Dragons 0.9.88.

Stock clients see a vanilla carrier entity sized like the dragon with its model as display entities (CONTENT.md, vanilla view). The GeckoLib rig comes with the companion (C5); P2a doesn't wait for it.

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
2. Goals, navigation and targeting from `abi/interfaces`; Cindervane's ground goals (wander, follow owner, sit).
3. Damage and death events; Cindervane's attacks and loot.
4. Taming, saddle and riders; ground riding.
5. Flight: takeoff, rider flight control, landing (`server/flight`).
6. The harness comparison against Java 0.9.88.
