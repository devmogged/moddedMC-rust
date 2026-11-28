# Queue

Jobs for agents outside Create and Saints Dragons, in order. Take the first one nobody holds (see
the [work-loop skill](../.agents/skills/work-loop/SKILL.md)), finish it, then strike it here in
the same PR that lands it.

Unfinished work from the November 27 agents is saved as one untested snapshot: branch
`wip/noncreate-20261128` in the private `devmogged/mods-private` repo, with its engine half on
`wip/noncreate-engine-20261128` in `devmogged/Pumpkin`. Take code from it one job at a time,
verify it, and land it in that job's own PR. Never merge the snapshot as a whole.

~~0. **Companion build (#60).** `gradlew test` failed to resolve the floating Loom marker; pinned to 1.19.6, with the companion build and all 13 tests passing.~~
1. **Shared host APIs.** The container transfer API (#50) and atomic player-stack replacement
   with an XP debit (#51). Take the `api/` and `engine/moddedmc-host/` parts from the snapshot,
   keep them additive, test them and land them.
2. **Compass and Refined Storage.** Rebase draft PRs #45 and #49 once job 1 is on `main`, make
   them compile and pass their scenarios, and land them. #49 is a prototype: finish the parity
   gaps its README lists before closing #46.
3. **Finish the merged ports.** Lootr, Legendary Monsters, Polymorph and Chunky have unlanded
   changes in the snapshot. One mod per PR.
4. **Block-state budget.** With Create loaded, only 4,730 of the 65,536 runtime block-state IDs
   are free (docs/ws/create-gaps.md), and the big ports still need thousands. Measure what each
   remaining BUJA port needs and write `docs/STATE-IDS.md` with the options and their costs.
   Don't implement one until Noel-Alex picks it.
5. **Next BUJA ports,** one at a time, in work-loop order. The snapshot has early drafts of Easy
   Villagers, Musket Mod, Epic Knights, Sophisticated Backpacks and Storage, Small Ships, Siege
   Weapons, Recruits, Quark, Alex's Mobs and Alex's Caves. Treat them as notes, not finished
   work, and port block-heavy mods only after job 4 has a plan.

Not in this queue: Create and its add-ons (Noel-Alex), Saints Dragons (sting-raider), and
Waystones, Farmer's Delight, Corpse and Simple Storage Network (KaustuvMohapatra). The snapshot
has drafts of some of these. Leave them alone unless their owner hands them over.
