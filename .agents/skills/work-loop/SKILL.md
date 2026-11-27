---
name: work-loop
description: How an agent on moddedMC-rust picks its next job, claims it on GitHub issues, works it to the end, hands off and moves on, without locks and alongside other people's agents. Use at the start of every session, whenever a job finishes or gets blocked, and before a session ends.
---

# Work loop

Several people work on this repo at once, each with their own agent, and none of them use chat.
GitHub issues on `devmogged/moddedMC-rust` are the only shared state: who has what, what's
blocked, and where to resume. There is no lock or lease tool. The old `mmc-coord` board is
retired, so never run `coord.mjs`.

The goal is the whole buja pack (docs/BUJA.md) playing on this server like it does on Java, with
friends joining from a stock 26.3 client. Keep going until that's done. The person you work for
shouldn't have to steer you. Ask them only for things only they can do, like logging in or
testing in a real client.

## Who owns what

- Create and every Create add-on, Steam 'n' Rails included (`mods/create*`, `mods/createaddition`):
  Noel-Alex. Saints Dragons (`mods/saints-dragons`): sting-raider. Waystones, Farmer's Delight,
  Corpse and Simple Storage Network: KaustuvMohapatra.
- Everything else goes to whoever claims it first through an issue.
- Never edit a crate someone else owns. If you need something from it, open an issue and say
  what you need and why.

## Pick and claim

1. Read [docs/ENGINEERING-EXECUTION.md](../../../docs/ENGINEERING-EXECUTION.md) first. Inspect the exact worktree, branch, dirty/untracked paths, active workers and build processes. Start new work from fresh `origin/main` in an isolated worktree when the assigned checkout is dirty or shared. **Never run `scripts\clean.ps1`, reset, stash, checkout over, or remove files from a shared/dirty worktree** until every owner confirms it is disposable. Build-cache cleanup is a separate, scoped operation. Read your own open issues first: if you hold one, resume it from its last comment.
2. Otherwise take the first unclaimed job in [docs/QUEUE.md](../../../docs/QUEUE.md). When the
   queue is empty: bugs that players hit in mods nobody owns come first, then the next `port` row
   in docs/BUJA.md that most players will see, as long as the ports it depends on exist. If a port
   is blocked on host or engine support nobody is building, build that support yourself as its
   own job.
   Hold one job at a time and finish it before claiming another. Never start several mods at
   once: a half-started port blocks everyone else from claiming it and is worth less than none.
3. A job is taken when an open issue for it is assigned to someone and that issue, or the
   matching branch or commits, moved in the last three days. An assigned issue that has been
   quiet for longer is free: comment that you're taking it over.
4. Claim the job by opening an issue (`Port <mod>`, or the bug), or by commenting on the existing
   one, and assigning it to the person you work for. Before you start, check again that nobody
   claimed it in the meantime. If someone did, the earlier claim wins.

## While working

- Work in your own worktree under `N:\mmc-wt\<issue>-<slug>`, made from fresh `origin/main`.
  Never edit `N:\git repos\moddedMC-rust` itself: that is the Create worker's checkout.
- Port from the exact release source (the "Latest" column of docs/BUJA.md). Read the Java class
  and every override it inherits before implementing a behavior, and match it. When a scenario
  and the source disagree, the scenario is wrong.
- Make small, coherent commits on an owned feature branch. Fetch and rebase onto the current target before opening/updating a PR; never force-push `main` or the engine's `moddedmc`. There is no CI, so your checks are the gate: when they pass on the rebased branch, merge your own PR (`gh pr merge --squash --delete-branch`). A PR that doesn't compile on `main` stays a draft. Commit as the person you work for, with conventional commits that have a scope and no agent or tool attribution. Follow the repository's three-open-PR limit.
- Changes to shared code (`host/`, `api/`, `engine/pumpkin`, workspace `Cargo.toml`) stay
  generic, small and additive, and go in early so others build on them. The ABI is append-only.
  An engine pointer conflict means rebasing your engine commits onto the latest `moddedmc` and
  bumping the pointer again.
- Post a short comment on your issue at each milestone: what works, how you checked it, and what's
  next. That comment is the handoff. Another session, yours or anyone else's, has to be able to
  continue from it with nothing else.
- Before repeating a failed scenario or build, record its run ID, exact source commit/tree, artifact path/hash, server PID, scenario and log path, failure signature, hypothesis and what changed since the previous attempt. Do not rerun an unchanged experiment; choose a discriminating test or implement the identified root-cause fix.
- Build, pack and run scenarios only from your own worktree's target dir (see section 4 of the
  execution playbook), so you never wait on or overwrite another worker. Every runtime result
  must identify the exact artifact it exercised.
- Spend your time on the port. Don't write process docs about agents, don't open extra chats or
  workers, and don't re-audit the repo at the start of every turn.
- When you find a separate bug, open an issue for it rather than widening your job.

## Finishing

A job is done when it works in game and its scenarios pass. Write a real-client checklist for
anything a bot can't see, like rendering, GUIs or sounds, and post it on the issue for your person.
Then close the issue and pick the next job straight away. A source implementation, unit test, compile, bot scenario, forced-crash scenario and real stock-client check are separate evidence levels; never report one as another. Update the acceptance ledger when evidence changes.

Before a session ends for any reason (context, usage or time), commit and push only the owned, reviewed working slice and leave the handoff comment, so nothing lives only on your machine. Never stage the entire shared worktree or include another owner's dirty/untracked files.
