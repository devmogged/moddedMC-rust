# Headless player coverage

The bot joins the Java 26.3 play connection. Scenario actions send player packets. Console probes and snapshots check the server result. Local `expect pos` and `expect vel` check the bot's own state, independently of server coordinates.

## Movement verification

The 26.3 movement constants and order were checked against the decompiled `LivingEntity.aiStep`, `travelInAir`, `Entity.move` and `Entity.collide`. The client package and drag are separate from the server's travel step. The bot uses the vanilla wire block IDs and Pumpkin's generated collision shapes. It currently sends position and ground/collision flags on each simulated client tick. Standard Overworld, Nether and End chunk heights are supported.

| Check | Result |
| --- | --- |
| [Gravity fixture](../tests/harness/bot-gravity.txt), vanilla vs vanilla | Zero differences. The bot matched the four-tick airborne trajectory, landed at y=151 and reported the same position to the server. [JSON](../run/harness-movement/vanilla-final/scenario-EbqjMM/report.json) |
| Gravity fixture, vanilla vs pack-dev | The same assertions passed with zero differences. [JSON](../run/harness-movement/pack-final/scenario-q9IRxL/report.json) |
| Live pack-dev with Create 6.0.10 | The 16 RPM belt carried the bot from z=2011.5 to z=2012.295267690645 at the 1.5-second check. x=2050.5 and y=101 held. The realtime check allows scheduling tolerance because the server and client clocks run independently. |
| Pack-profile unit tests | Check that another entity's motion leaves the player unchanged, explosion knockback adds to existing motion, and the client lands and steps up a slab. |

## Port checks and remaining automation

| Priority | Behavior | Bot coverage and remaining work |
| --- | --- | --- |
| 1 | Create belt carries a standing player | The bot accepts its own motion packet and follows the vanilla dry-ground movement path. Use a local position assertion for progress and centering and a server probe to confirm the move was received. Sneak input, held movement and flight are needed to check the escape conditions in the belt play test. |
| 2 | Create belt connector, other targeted block uses, and inventory | Basic placement and top-face use work. Exact ray hits, face and hand selection, sneak-use, held item use, creative selection and hotbar changes are missing. Left, right and shift menu clicks work with the server's current revision. Drag, swap, double-click, stack and cursor prediction, and client inventory assertions remain. These are needed for belt loading and unloading, crafting, and picking up the result. |
| 3 | Movement and riding | Absolute move and look already send player packets. Local physics covers gravity, dry drag, static block shapes, steps and knockback. It still needs held directional input, sneak/edge restraint, sprint, jump, creative flight, dynamic collision, fluid and climbing travel, pose/attribute/effect changes, and mounting, steering and dismounting vehicles. |
| 4 | Breaks and combat | Creative break sends start-dig, and attack targets a visible entity by its spawn anchor. Survival mining needs progress from the active tool and effects, cancel, and finish. Combat needs current entity positions, aim and reach, item cooldown and per-tick attack timing. |
| 5 | Item pickup, drops and other observable effects | Server probes/snapshots can check inventory and ground items. Add bot assertions for own slot and cursor contents, pickup, health, effects and cooldowns. Check sound, particles, chat, action-bar text, and item names through decoded packets where the expected behavior is their delivery. |

## Documented play tests

| Source | Existing automation and remaining work |
| --- | --- |
| [Create play tests](../mods/create/README.md#stock-263-play-test) | P1a exercises pack transfer, block/item actions, RPM signs and ratios, restart and break. P1b exercises gearbox signs, clutch and gearshift under redstone, overstress, conflict break and restart. P1c exercises placement and segment teardown. For the belt checklist, add player conveyance/centering with local motion, sneak-cancel, escape controls, survival craft and inventory checks. The README's unported belt and kinetics features are separate port work. |
| [Content stages](CONTENT.md), [port backlog](BACKLOG.md) and [handoff](HANDOFF.md) | Verify stock resource-pack models, selection outlines and kinetic animations in the game. Overstress particles, moving parts inside casing, belt rendering and feedback are outstanding visual or audio checks as features land. Mechanics can be tested separately through packets, probes and snapshots. |
| [Status](STATUS.md) | The barrel's appearance needs a rendered client. Action-bar content and the dropped item's name can be checked as decoded text and item metadata. Their layout and appearance need a rendered client. |
| [Companion owner play test](COMPANION.md#owner-play-test) and [A3](BACKLOG.md) | Protocol negotiation, asset integrity and launcher file installation have automated checks. Launching through the actual profile, joining with the installed Fabric mod, loading only prefetched assets with HTTP blocked, returning to ordinary vanilla servers, and importing the instance ZIP remain functional integration checks. Those paths need a real launcher or Fabric runtime until they have automation of their own. |

The presentation checks that need a rendered or audible client include item and block models, outlines, rotation and interpolation, displayed text and screen layout, particles and sounds. Receiving a packet can verify the event and its parameters.
