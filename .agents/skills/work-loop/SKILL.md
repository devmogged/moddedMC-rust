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
  Noel-Alex. Saints Dragons (`mods/saints-dragons`): sting-raider.
- Everything else goes to whoever claims it first through an issue.
- Never edit a crate someone else owns. If you need something from it, open an issue and say
  what you need and why.

## Pick and claim

1. Start from fresh `origin/main` and run `scripts\clean.ps1`. Build caches otherwise grow by
   tens of GB a day. Read your own open issues first: if you hold one, resume it
   from its last comment.
2. Otherwise take the most useful unclaimed job. Bugs that players hit in mods nobody owns come
   first, then the next `port` row in docs/BUJA.md that most players will see, as long as the
   ports it depends on exist. If a port is blocked on host or engine support nobody is building,
   build that support yourself as its own job.
3. A job is taken when an open issue for it is assigned to someone and that issue, or the
   matching branch or commits, moved in the last three days. An assigned issue that has been
   quiet for longer is free: comment that you're taking it over.
4. Claim the job by opening an issue (`Port <mod>`, or the bug), or by commenting on the existing
   one, and assigning it to the person you work for. Before you start, check again that nobody
   claimed it in the meantime. If someone did, the earlier claim wins.

## While working

- Push small working commits to `main` often. Fetch and rebase first. Never force push `main`
  or the engine's `moddedmc`. Commit as the person you work for, with conventional commits that
  have a scope and no agent or tool attribution.
- Changes to shared code (`host/`, `api/`, `engine/pumpkin`, workspace `Cargo.toml`) stay
  generic, small and additive, and go in early so others build on them. The ABI is append-only.
  An engine pointer conflict means rebasing your engine commits onto the latest `moddedmc` and
  bumping the pointer again.
- Post a short comment on your issue at each milestone: what works, how you checked it, and what's
  next. That comment is the handoff. Another session, yours or anyone else's, has to be able to
  continue from it with nothing else.
- When you find a separate bug, open an issue for it rather than widening your job.

## Finishing

A job is done when it works in game and its scenarios pass. Write a real-client checklist for
anything a bot can't see, like rendering, GUIs or sounds, and post it on the issue for your person.
Then close the issue and pick the next job straight away.

Before a session ends for any reason (context, usage or time), push what works and leave the
handoff comment, so nothing lives only on your machine.
