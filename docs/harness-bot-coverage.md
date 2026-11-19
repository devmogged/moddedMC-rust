# Headless player coverage

The bot joins the Java 26.3 play connection. Scenario actions send player packets. Console probes and snapshots check the server result. Local `expect pos` and `expect vel` check the bot's own state, independently of server coordinates.

## Movement verification

The 26.3 movement constants and order were checked against the decompiled `LivingEntity.aiStep`, `travelInAir`, `Entity.move` and `Entity.collide`. The bot advances local travel independently of server ticks. It uses the vanilla wire block IDs and Pumpkin's generated collision shapes and sends position and ground/collision flags on each simulated client tick. Standard Overworld, Nether and End chunk heights are supported.

| Check | Result |
| --- | --- |
| [Gravity fixture](../tests/harness/bot-gravity.txt), vanilla vs vanilla | Zero differences. The bot matched the four-tick airborne trajectory, landed at y=151 and reported the same position to the server. [JSON](../run/harness-movement/vanilla-final/scenario-EbqjMM/report.json) |
| Gravity fixture, vanilla vs pack-dev | The same assertions passed with zero differences. [JSON](../run/harness-movement/pack-final/scenario-q9IRxL/report.json) |
| Live pack-dev with Create 6.0.10 | The 16 RPM belt carried the bot from z=2011.5 to z=2012.295267690645 at the 1.5-second check. x=2050.5 and y=101 held. The realtime check allows scheduling tolerance because the server and client clocks run independently. |
| [Controls fixture](../tests/harness/bot-controls.txt) | Sprint acceleration, sneaking at an edge, jumping and Creative flight passed with zero differences on vanilla and pack-dev. [Vanilla self-test](../run/harness-controls/vanilla-final/scenario-60Ic8h/report.json), [pack comparison](../run/harness-controls/pack-final/scenario-F7RwGc/report.json) |
| Sneak on a live Create belt | The bot advanced 0.119 blocks as its existing motion decayed. Its horizontal velocity reached zero while sneak was held. Releasing sneak let the belt carry it another 0.513 blocks in one second. |
| Pack-profile unit tests | Check that another entity's motion leaves the player unchanged, explosion knockback adds to existing motion, and the client lands and steps up a slab. |

[The inventory fixture](../tests/harness/bot-inventory.txt) passed with zero differences against pack-dev. It checks half-stack pickup, cursor clearing, hotbar swaps and selection, main- and offhand placement, throwing an item, and item pickup. [JSON](../run/harness-inventory/verified/scenario-TxruIZ/report.json)

An earlier version also used `execute if items entity`. Vanilla returned counts of 2 and 3 for the selected stone and picked-up sticks. Pack-dev rejected both command predicates. The bot saw the same stacks on each connection. The fixture now verifies the selected stack by placing a block and the picked-up stack directly from the client's inventory packets. [Command differences](../run/harness-inventory/pack-3/scenario-QzLtMv/report.json)

[The held-actions fixture](../tests/harness/bot-mining.txt) passed with zero differences. It verified that aborting a Survival stone dig leaves the block intact, finishing after 23 ticks breaks it, and releasing a drawn bow uses one arrow. [JSON](../run/harness-mining/pack/scenario-Drz9SN/report.json)

## Port checks and remaining automation

| Priority | Behavior | Bot coverage and remaining work |
| --- | --- | --- |
| 1 | Create belt carries a standing player | The bot accepts its own motion packet and follows the vanilla dry-ground movement path. Use a local position assertion for progress and centering and a server probe to confirm the move was received. Scripts can hold movement or sneak and toggle Creative flight to check escape conditions. |
| 2 | Create belt connector, other targeted block uses, and inventory | The bot can target a face, use and release either hand, sneak-use, open and close a container, select a hotbar slot, and assert the active slot and cursor stacks. It also sends swap, double-click and throw menu actions. Ray picking, Creative slot editing, drag and full client stack prediction remain. |
| 3 | Movement and riding | Absolute move and look send player packets. Local physics covers held directional input, dry drag, gravity, static block shapes, steps, knockback, jumping, sprinting, sneaking at an edge and Creative flight. Dynamic collisions, fluid and climbing travel, pose/attribute/effect changes, and mounting, steering and dismounting vehicles remain. |
| 4 | Breaks and combat | Creative break sends start-dig. Survival mining supports start, cancel and finish on an explicitly timed scenario. Automatic mining duration from the active tool and effects remains. Attack targets a visible entity by its spawn anchor. Combat still needs ray picking, current entity positions, reach, cooldown and per-tick attack timing. |
| 5 | Item pickup, drops and other observable effects | A fixture verifies pickup with a local inventory assertion. Ground items can be checked in snapshots. Local health, effects, item model/name metadata and cooldown assertions remain. Check sound, particles, chat and action-bar text through decoded packets where the expected behavior is their delivery. |

## Documented play tests

| Source | Existing automation and remaining work |
| --- | --- |
| [Create play tests](../mods/create/README.md#stock-263-play-test) | P1a exercises pack transfer, block/item actions, RPM signs and ratios, restart and break. P1b exercises gearbox signs, clutch and gearshift under redstone, overstress, conflict break and restart. P1c exercises placement and segment teardown. Its owner can now add conveyance/centering, sneak-cancel and movement controls. Survival craft and inventory checks are still limited. The README's unported belt and kinetics features are separate port work. |
| [Content stages](CONTENT.md), [port backlog](BACKLOG.md) and [handoff](HANDOFF.md) | Verify stock resource-pack models, selection outlines and kinetic animations in the game. Overstress particles, moving parts inside casing, belt rendering and feedback are outstanding visual or audio checks as features land. Mechanics can be tested separately through packets, probes and snapshots. |
| [Status](STATUS.md) | The barrel's appearance needs a rendered client. Action-bar content and the dropped item's name can be checked as decoded text and item metadata. Their layout and appearance need a rendered client. |
| [Companion owner play test](COMPANION.md#owner-play-test) and [A3](BACKLOG.md) | Protocol negotiation, asset integrity and launcher file installation have automated checks. Launching through the actual profile, joining with the installed Fabric mod, loading only prefetched assets with HTTP blocked, returning to ordinary vanilla servers, and importing the instance ZIP remain functional integration checks. Those paths need a real launcher or Fabric runtime until they have automation of their own. |

The presentation checks that need a rendered or audible client include item and block models, outlines, rotation and interpolation, displayed text and screen layout, particles and sounds. Receiving a packet can verify the event and its parameters.
