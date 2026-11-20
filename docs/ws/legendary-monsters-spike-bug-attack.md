# WS-6 M2 groundwork — Spiky Bug attack timing (2.2.4)

Reference: **released** `cache/legendary_monsters-2.2.4-MC-1.21.1.jar`, class
`net.miauczel.legendary_monsters.entity.ai.SpikeBugAttackGoal`.
Verified by `C:\Program Files\Java\jdk-26.0.1\bin\javap.exe -classpath cache/legendary_monsters-2.2.4-MC-1.21.1.jar -p -c net.miauczel.legendary_monsters.entity.ai.SpikeBugAttackGoal`.
The older public Java source under `D:\mc-ref\legendary-monsters-2.2.4` was consulted for names only; bytecode of the released JAR is the authority.

## Exact source-observed callback contract

- Constructor fields: `attackDelay=8`, `ticksUntilNextAttack=8`, `shouldCountTillNextAttack=false`, `hasPlayedAttackSound=false`.
- `start()` calls superclass and resets the two *integer* fields to 8; it does not reset the two booleans.
- `checkAndPerformAttack(target, distSq)` checks `distSq <= 8.75` (**squared distance**, inclusive). In range it sets count flag, and when timer <= attackDelay with sound flag still false, sets entity ATTACKING true and remembers flag. If timer <= 0, it looks directly at target, clears sound flag, performs attack: calls `resetAttackCooldown()`, swings main hand and `doHurtTarget`.
- Out of range it computes cooldown `adjustedTickDelay(attackDelay * 2)`, clears count flag, sets ATTACKING false, sets entity `attackAnimationTimeout=5`, and clears sound flag.
- Override `tick()` calls superclass first, then decrements countdown only while count flag true, clamping at zero.
- Override `stop()` sets ATTACKING false, calls superclass, clears sound flag; it does **not** reset the timer or count flag.
- `resetAttackCooldown()` calls `adjustedTickDelay(16)`. Rust policy **accepts that computed delay from the future host** instead of assuming 16 forever. ATTACKING flag updates and effects remain modeled as emitted actions to be applied by host in the same order.

Implementation: `mods/legendary-monsters/src/spike_bug_attack.rs`, exposed as a pure Rust state machine. The bytecode comparison validates constants, branch conditions, side effects and method order; tests exercise the unambiguous arithmetic and state transitions. It is **not yet installed as a goal on an in-world entity**.

## Host/engine dependencies and verification limits

- WS-5-12 #65: creature size must be 0.7 × 0.5; do not ship a misleading vanilla-sized carrier.
- WS-5-01 #54: generic melee damage/death event and proper damage-source attribution to call `doHurtTarget`.
- WS-5-14 #67: exact player/iron-golem/snow-golem target acquisition and priority, visibility and inherited melee pathfinding. The Rust module does not simulate these.
- Need client animation sync and vanilla `adjustedTickDelay` contract at integration time.
- No world/bot scenario or original 2.2.4 NeoForge combat oracle is claimed for this module. Integrate only after interfaces land, then record actual attack ticks, health delta, target order, and persistence under repeatable scenarios.
