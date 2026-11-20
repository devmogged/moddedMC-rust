# Headless player coverage

The bot joins the Java 26.3 play connection. Scenario actions send player packets. Console probes and snapshots check the server result. Local `expect pos` and `expect vel` check the bot's own state, independently of server coordinates.

## Movement verification

The 26.3 movement constants and order were checked against the decompiled `LivingEntity.aiStep`, `travelInAir`, `Entity.move` and `Entity.collide`. The bot advances local travel independently of server ticks. It uses the vanilla wire block IDs and Pumpkin's generated collision shapes and sends position and ground/collision flags on each simulated client tick. Standard Overworld, Nether and End chunk heights are supported.

| Check | Result |
| --- | --- |
| [Gravity fixture](../tests/harness/bot-gravity.txt), vanilla vs vanilla | Zero differences. The bot matched the four-tick airborne trajectory, landed at y=151 and reported the same position to the server. [JSON](../run/harness-movement/vanilla-final/scenario-EbqjMM/report.json) |
| Gravity fixture, vanilla vs pack-dev | The same assertions passed with zero differences. [JSON](../run/harness-movement/pack-final/scenario-q9IRxL/report.json) |
| Live pack-dev with Create 6.0.10 | The 16 RPM belt carried the bot from z=2011.5 to z=2012.295267690645 at the 1.5-second check. x=2050.5 and y=101 held. The realtime check allows scheduling tolerance because the server and client clocks run independently. |
| One frozen Create server tick and one client tick | After two client ticks to settle on the belt, the server step delivered z velocity 0.033327229445156537. One client tick moved the bot from z=2011.5 to z=2011.5333272294451 at x=2050.5 and y=101. The difference from 16/480 is within the motion packet's quantization. |
| [Controls fixture](../tests/harness/bot-controls.txt) | Sprint acceleration, sneaking at an edge, jumping and Creative flight passed with zero differences on vanilla and pack-dev. [Vanilla self-test](../run/harness-controls/vanilla-final/scenario-60Ic8h/report.json), [pack comparison](../run/harness-controls/pack-final/scenario-F7RwGc/report.json) |
| Sneak on a live Create belt | The bot advanced 0.119 blocks as its existing motion decayed. Its horizontal velocity reached zero while sneak was held. Releasing sneak let the belt carry it another 0.513 blocks in one second. |
| Pack-profile unit tests | Check that another entity's motion leaves the player unchanged, explosion knockback adds to existing motion, and the client lands and steps up a slab. The supporting-block tests check ice friction while overhanging the edge and gravity compensation on a slime bounce. |

[The inventory fixture](../tests/harness/bot-inventory.txt) passed with zero differences against pack-dev. It checks half-stack pickup, cursor clearing, hotbar swaps and selection, main- and offhand placement, throwing an item, and item pickup. [JSON](../run/harness-inventory/verified/scenario-TxruIZ/report.json)

An earlier version also used `execute if items entity`. Vanilla returned counts of 2 and 3 for the selected stone and picked-up sticks. Pack-dev rejected both command predicates. The bot saw the same stacks on each connection. The fixture now verifies the selected stack by placing a block and the picked-up stack directly from the client's inventory packets. [Command differences](../run/harness-inventory/pack-3/scenario-QzLtMv/report.json)

[The held-actions fixture](../tests/harness/bot-mining.txt) passed with zero differences. It verified that aborting a Survival stone dig leaves the block intact, finishing after 23 ticks breaks it, and releasing a drawn bow uses one arrow. [JSON](../run/harness-mining/pack/scenario-Drz9SN/report.json)

[The riding fixture](../tests/harness/bot-riding.txt) passed with zero differences. The bot right-clicked an oak boat, saw itself in the passenger list, then dismounted with sneak. [JSON](../run/harness-riding/final/scenario-e6BjYB/report.json)

The [steering fixture](../tests/harness/bot-steering.txt) checks the server-accepted positions of a boat under forward input, a saddled horse after a movement-speed update, and a saddled pig steering with a carrot on a stick. The bot starts local simulation only in the controlling seat and uses the server's equipment and attribute updates. It sends the vehicle's position, rotation and ground flag each tick. The travel constants come from 26.3's `AbstractBoat.floatBoat` and `controlBoat`, `AbstractHorse.getRiddenInput`, `Pig.getRiddenSpeed`, and `LivingEntity.getFrictionInfluencedSpeed`. [Pack comparison](../run/harness-steering/updated/scenario-bK46cO/report.json)

[Initial horse attribute sync](../tests/harness/horse-attributes-on-track.txt) is a separate parity check. The original steering run put the horse at z=3.3604628040030105 on vanilla and z=3.4680206144349657 on pack-dev. Both servers saved a movement-speed base of 0.2. The vanilla packet capture included the horse's 0.2 speed when tracking began. The pack-dev capture contained the spawn and no attribute update for that horse. The steering fixture now uses an explicit speed update after tracking, which the server sends to the client. The initial sync fixture keeps the original assertion.

[The fluid fixture](../tests/harness/bot-fluid.txt) passed against pack-dev with zero differences. It checks rise from held jump in deep water and lava, sprint-swimming by pitch, and an eastward current computed from adjacent water levels. The constants and update order come from 26.3's `EntityFluidInteraction`, `FlowingFluid`, `LivingEntity.travelInFluid` and `Player.travel`. [JSON](../run/harness-fluid/final/scenario-N1cN1M/report.json)

[The entity collision fixture](../tests/harness/bot-entity-collisions.txt) matched vanilla and pack-dev. It checks the side of a boat with headroom below the vanilla step height, the side of a closed shulker, and the impulse from an overlapping player. Collision boxes now use position updates, pose, and the shulker's peek and attachment metadata. Position tracking follows the 26.3 relative delta and absolute path codecs. The physics uses packet endpoints. Remote interpolation, team collision rules, and displacement by a growing shulker lid still need coverage. [JSON](../run/harness-entity-collisions/low-ceiling/scenario-NhuzKW/report.json)

The [Windows suite report](../run/harness-bot-regression/windows.json) recorded 89 expected engine differences across the existing scenarios. It also caught a half-block origin error during the supporting-block change. The corrected build passed [the ledge and controls fixture](../run/harness-dry-final/bot-controls/scenario-vQLgwb/report.json), [gravity](../run/harness-dry-final/bot-gravity/scenario-A3wx51/report.json), [mining](../run/harness-dry-final/bot-mining/scenario-8pJ5fm/report.json), and the single-tick Create belt check.

Both Windows and native Linux passed pack-profile Clippy and all 15 bot and harness unit tests. Ubuntu 24.04 under WSL2 ran the five new fixtures with vanilla 26.3 on both sides and produced zero differences. The pack-dev comparison used Windows. [Linux report](../run/harness-bot-linux/vanilla.json)

To reproduce the single-tick check, use the P1c belt setup through its 16 RPM assertion in a scratch scenario. Append these steps before the test tears down the belt.

```text
cmd tick freeze
wait 0.5
expect The game is frozen
bot physics manual
cmd tp tester 2050.5 101 2011.5 0 0
wait 0.5
bot physics-tick 2
bot expect pos 2050.5 101 2011.5 within 0.000001
wait 0.25
expect ^\[bot\] expect pos ok
cmd tick step 1
wait 0.5
expect Stepping 1 tick
bot expect vel 0 0 0.0333333333333333 within 0.00007
wait 0.25
expect ^\[bot\] expect vel ok
bot physics-tick 1
bot expect pos 2050.5 101 2011.5333333333333 within 0.00007
wait 0.25
expect ^\[bot\] expect pos ok
```

## Port checks and remaining automation

| Priority | Behavior | Bot coverage and remaining work |
| --- | --- | --- |
| 1 | Create belt carries a standing player | The bot accepts its own motion packet and follows the vanilla dry-ground movement path. Use a local position assertion for progress and centering and a server probe to confirm the move was received. Scripts can hold movement or sneak and toggle Creative flight to check escape conditions. |
| 2 | Create belt connector, other targeted block uses, and inventory | The bot can target a face, use and release either hand, sneak-use, open and close a container, select a hotbar slot, and assert the active slot and cursor stacks. It also sends swap, double-click and throw menu actions. Ray picking, Creative slot editing, drag and full client stack prediction remain. |
| 3 | Movement and riding | Absolute move and look send player packets. Local physics covers held directional input, gravity, block shapes, solid boat and shulker collision, entity pushes, steps, knockback, jumping, sprinting, sneak edges, Creative flight and ordinary water and lava travel. The bot can mount, dismount and steer an oak boat, a horse or a pig. Boat water friction and buoyancy are implemented. Water entry correction, horse jumps, pig boost, passenger seat position prediction, climbing, bubble columns and fluid-affecting effects remain. |
| 4 | Breaks and combat | Creative break sends start-dig. Survival mining supports start, cancel and finish on an explicitly timed scenario. Automatic mining duration from the active tool and effects remains. Attack targets a visible entity by its spawn anchor. Combat still needs ray picking, current entity positions, reach, cooldown and per-tick attack timing. |
| 5 | Item pickup, drops and other observable effects | A fixture verifies pickup with a local inventory assertion. Ground items can be checked in snapshots. Local health, effects, item model/name metadata and cooldown assertions remain. Check sound, particles, chat and action-bar text through decoded packets where the expected behavior is their delivery. |

## Documented play tests

| Source | Existing automation and remaining work |
| --- | --- |
| [Create play tests](../mods/create/README.md#stock-263-play-test) | P1a exercises pack transfer, block/item actions, RPM signs and ratios, restart and break. P1b exercises gearbox signs, clutch and gearshift under redstone, overstress, conflict break and restart. P1c exercises placement and segment teardown, mob transport and the belt item inventory. Its owner can now add player conveyance, centering, sneak-cancel and movement controls. Crafting and client-visible item model assertions still need coverage. The README's unported belt and kinetics features are separate port work. |
| [Content stages](CONTENT.md), [port backlog](BACKLOG.md) and [handoff](HANDOFF.md) | Verify stock resource-pack models, selection outlines and kinetic animations in the game. Overstress particles, moving parts inside casing, belt rendering and feedback are outstanding visual or audio checks as features land. Mechanics can be tested separately through packets, probes and snapshots. |
| [Status](STATUS.md) | The barrel's appearance needs a rendered client. Action-bar content and the dropped item's name can be checked as decoded text and item metadata. Their layout and appearance need a rendered client. |
| [Companion owner play test](COMPANION.md#owner-play-test) and [A3](BACKLOG.md) | Protocol negotiation, asset integrity and launcher file installation have automated checks. Launching through the actual profile, joining with the installed Fabric mod, loading only prefetched assets with HTTP blocked, returning to ordinary vanilla servers, and importing the instance ZIP remain functional integration checks. Those paths need a real launcher or Fabric runtime until they have automation of their own. |

The A1 physical Raspberry Pi install check in [the backlog](BACKLOG.md) concerns the deployment environment.

The presentation checks that need a rendered or audible client include item and block models, outlines, rotation and interpolation, displayed text and screen layout, particles and sounds. Receiving a packet can verify the event and its parameters.
