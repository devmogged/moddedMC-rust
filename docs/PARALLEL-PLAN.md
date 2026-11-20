# Parallel workstreams

How many agents port the whole buja pack at once without getting in each other's way. Each
agent owns one workstream on its own branch and git worktree, checks its work against vanilla
26.3 and the original mod, and leaves a log the integrator reviews and merges later.

The goal for every line of work: a server that plays exactly like Java Minecraft 26.3 with the
original mods, and runs much faster with much less RAM (the target is a Raspberry Pi 5 with
10 players at 20 TPS). Parity comes first. Speed is checked on hot paths.

## 1. Starting an agent

Open a new chat and paste this, with the workstream id filled in:

```text
You are the agent for workstream <WS-ID> of moddedMC-rust, a Rust Minecraft Java 26.3 server
(a fork of Pumpkin) that runs Rust ports of Java mods. You work on the owner's Windows laptop
with shell access. Nobody gives you more context than what's in the files named below.

Before doing anything else, read in full and follow:
1. N:\git repos\moddedMC-rust\docs\PARALLEL-PLAN.md (this plan: rules, coordination, your
   workstream's scope and milestones).
2. C:\Users\noela\.claude\skills\rustcraft-engineering\SKILL.md (the engineering contract).
3. N:\git repos\moddedMC-rust\AGENTS.md and engine\pumpkin\AGENTS.md (porting rules).
4. The docs your workstream lists under "Read first".

Then set up your worktree as in section 3 of the plan, write your status file, and start
on your first unfinished milestone. Work milestone by milestone, verify each one as the
plan says, commit and push your branch, update your log, and keep going. Stop and ask the
owner only for things only they can decide or provide.
```

The owner can start as many as section 5 allows. One chat per workstream. Never two chats
on the same workstream.

## 2. Rules for every agent

These apply to every workstream. They're the owner's standing rules.

**Git**
- Commit as the owner: `git config user.name` is `Noel-Alex` and the email is already set in the
  repo config. Never mention AI, ChatGPT, Codex, Claude, DeepSeek or any tool in commits,
  branch names, code comments or docs. No `Co-Authored-By` lines.
- Conventional commits with a scope, subject line only: `feat(create): port the fluid pipe`,
  `fix(engine): ...`, `test(cataclysm): ...`. One logical change per commit.
- Stage explicit paths. Never `git add -A`, `git add .` or `git commit -a`.
- Work only on your own branch (`ws/<id>` in this repo, and `ws/<id>` in `engine/pumpkin` if your
  workstream edits the engine). Never commit to `main` or the engine's `moddedmc` branch, never
  merge into them, never force push, never rewrite commits you've already pushed.
- Push only to `origin` (github.com/devmogged/...). Never push to upstream Pumpkin-MC.
- Never open pull requests, issues, reviews or comments on GitHub, even if asked. Your work ends
  at a pushed branch and an updated log.
- Uncommitted changes you didn't make belong to someone else. Never discard, stash, reformat or
  commit them.

**Porting**
- Port each mod's **latest release** (check Modrinth or CurseForge first), whatever loader or
  Minecraft version it targets. Never port from the old jars in the buja modpack. Put downloaded
  jars in the repo's ignored `cache/` folder and write the version in the crate's description.
- Port behaviour from the real source code, method by method, keeping the original names so a
  reviewer can read the Rust next to the Java. Follow the whole call chain, every override,
  ordering and side effects (AGENTS.md "Porting"). Never port from memory, the wiki or guesses.
- Vanilla 26.3 source (decompiled, readable): `D:\mc-ref\26.3\src\net\minecraft\`. Vanilla server
  jar: `D:\mc-ref\26.3\server-bundler.jar`. Create 6.0.10 source: `D:\mc-ref\create-src`. Put
  other sources (NeoForge, other mods) under `D:\mc-ref\<name>-<version>\`, outside the repo.
- Mechanics live in the mod (`mods/<id>`). The engine and host only provide generic operations
  through the stable C ABI in `api/` (docs/MOD-ABI.md). A mod never depends on the server's Rust
  types. If you need a host operation that doesn't exist, file an ABI request (section 4.3).
  Don't edit `api/` or `engine/` yourself unless your workstream owns them.
- Licences: never commit assets that are All Rights Reserved (Create's textures, models, sounds
  and lang, and any other mod whose assets aren't freely licensed). Check each mod's licence.
  ARR assets stay in the local jar copy, and the server reads them from there the way Create's
  are read. Code and data (recipes, loot tables, tags) are fine if the mod's code licence allows.
- Java edition only. Bedrock is out of scope.

**Machine**
- The owner is using this laptop. Never launch the Minecraft client, never simulate keystrokes,
  mouse clicks or take over the desktop. Bots (`moddedmc-bot`) and the harness are how you play.
- Disk is tight (about 235 GB free on N:). Build with the `pack` profile for the engine only.
  Never create extra engine target directories except where section 3 allows. Delete run
  directories once their scenario passes (`scripts/scenario.ps1` does this itself), delete
  scratch files you made, and never leave multi-GB leftovers.
- Stop only processes you started. Never kill a server, cargo or bot process another agent
  owns.
- Use your own port range for servers (section 4.4).

**Quality**
- Done means verified, not "it compiles". Each milestone needs:
  1. Unit tests for logic that breaks quietly (ordering, timing, off-by-one, NBT, parsing,
     recipe matching). No tests that repeat the implementation or test constants.
  2. A bot scenario (`tests/scenarios/<id>-<feature>.txt`) for every player-visible behaviour,
     including a save, restart and reload check for anything that persists.
  3. For vanilla behaviour: a differential fixture in `tests/harness/` that passes against the
     real vanilla server (docs/HARNESS.md).
  4. For mod behaviour: outcomes compared with the original mod (same inputs, same results),
     written down in your log with what you compared and how.
  5. `cargo fmt`, `cargo clippy` with no new warnings, all existing scenarios for the mods you
     touched still pass.
- Read your own diff like a maintainer before committing (AGENTS.md "Read your own diff").
- Say plainly what you could not verify. Never claim parity you didn't check.
- Be efficient: the owner wants the most real progress per unit of effort. Targeted test runs,
  no repeated full rebuilds, no side quests outside your scope. Note them in your log instead.

**Skills you can read** (they're instruction files; open and follow them when the task fits):
- `C:\Users\noela\.claude\skills\rustcraft-engineering\SKILL.md`: always.
- `C:\Users\noela\.claude\skills\systematic-debugging\SKILL.md`: any bug hunt.
- `C:\Users\noela\.claude\skills\test-driven-development\SKILL.md`: logic-heavy code.
- `C:\Users\noela\.claude\skills\impeccable\SKILL.md` (or `C:\Users\noela\.codex\skills\impeccable\SKILL.md`):
  any UI work, required.
- `C:\Users\noela\.claude\skills\ponytail\SKILL.md`: keeping changes minimal.
- `C:\Users\noela\.claude\skills\humanizer\SKILL.md`: before writing docs others read.

## 3. Branches, worktrees and builds

The main checkout `N:\git repos\moddedMC-rust` (branch `main`) belongs to the integrator and its
worker. Don't work in it. Each workstream gets its own worktree:

```powershell
cd "N:\git repos\moddedMC-rust"
git fetch origin
git worktree add "N:\mmc-wt\<id>" -b ws/<id> origin/main
cd "N:\mmc-wt\<id>"
git config user.name Noel-Alex
```

- **Mod-only workstreams** (ports, content, data) don't need the engine submodule or an engine
  build. Mods build in the root workspace: `cargo build --release -p <crate>` then
  `scripts/pack-mod.ps1 -Crate <crate>`. Run scenarios with the **shared engine binaries**:
  `scripts/scenario.ps1 -Scenario <file> -Exe <shared>\pack-dev.exe -BotExe <shared>\moddedmc-bot.exe -Port <yours> -RunDir run\<id>-<name>`.
- **Shared engine binaries** live in `N:\mmc-shared\bin\<engine-commit>\` (`pack-dev.exe`,
  `moddedmc-bot.exe`, `moddedmc-harness.exe`, plus `COMMIT` and `ABI.txt` naming the outer and
  engine commits). `N:\mmc-shared\bin\LATEST.txt` names the newest folder. WS-0 builds and
  publishes them (section 6). Until the first set exists, mod workstreams do their milestone 0
  work (inventory, source study, data, pure logic with unit tests), which needs no server.
- **Engine workstreams** (WS-0 and WS-7 only) also run `git submodule update --init engine/pumpkin`
  in their worktree, check out `ws/<id>` inside `engine/pumpkin` from `origin/moddedmc`, and build
  with `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness`.
  That gives each of them one engine target directory (about 31 GB each). No other workstream
  builds the engine.
- **Keep up with main**: at least once a day and before each milestone commit, `git fetch` and
  `git merge origin/main` into your branch (merge, not rebase, because your branch is pushed).
  Resolve conflicts keeping both sides' intent. If main changed the ABI you use, adapt.
- Push your branch after every milestone: `git push -u origin ws/<id>` (and the engine branch
  if you have one).
- When the workstream ends, leave the worktree in place for the integrator. Delete only your
  run directories and scratch files.

## 4. Coordination

### 4.1 Who owns what

| Area | Owner | Others |
| --- | --- | --- |
| `main`, engine `moddedmc`, merging branches | Integrator (Claude) and its worker (Sol) | never commit there |
| Create kinetics, belts, depot, chute, funnels, press, fan, millstone, crushing wheels (stage 4), contraptions (P1d), datapack worldgen (E2) | Integrator and Sol, on main | don't duplicate; read their code |
| `mods/saints-dragons`, `moddedmc:mobs`, `moddedmc:mob_ai` | sting-raider (a human collaborator) | use the mobs ABI, don't change it; request changes through WS-0 |
| `api/`, `engine/moddedmc-host/`, `docs/MOD-ABI.md` on branches | WS-0 | everyone else requests |
| `engine/pumpkin` vanilla behaviour on branches | WS-7 (and WS-0 for host-facing hooks) | request |
| Each `mods/<id>` crate | the workstream that created it | |
| `mods/create/src/fluids/**`, basin, mixer | WS-2 | |
| `mods/create/src/trains/**` | WS-3 | |
| `companion/` | WS-8 | |
| `app/`, `agent/` | WS-10 | |

Inside `mods/create`, workstreams other than the integrator add new files only (their own
subfolder) and touch shared files (`lib.rs`, `README.md`, `Cargo.toml`) only to add their
module and its registration call, each in one block marked with a comment naming the
workstream. That keeps merges with main small.

### 4.2 The coordination folder

`N:\mmc-coord\` is outside git so any agent can read and write it without merges.

- `status\<WS-ID>.md`: each agent writes only its own. Update it when you start, at every
  milestone, and whenever you're blocked. Format:

  ```text
  # WS-5 L_Ender's Cataclysm
  Updated: 2026-11-20 14:10 IST
  Branch: ws/cataclysm @ 1a2b3c4 (pushed)
  Milestone: M2 Ignitium items and blocks (in progress)
  Done: M0, M1 (see docs/ws/cataclysm.md)
  Blocked on: ABI request WS-5-03 (entity attributes)
  Next: M3 Ender Golem
  ```

- `requests\<WS-ID>-<NN>.md`: ABI and engine requests (section 4.3).
- `reviews\<WS-ID>\<commit>.md`: review notes (section 7).
- `notes\`: anything every agent should know (a gotcha, a harness trick). One file per topic.
  Read the folder when you start a session.
- `ports.md`: the port table (section 4.4).

### 4.3 Requests for the ABI or the engine

When a port needs something the host doesn't offer (an entity operation, a world query, a
recipe hook):

1. Check docs/MOD-ABI.md on `origin/main` and on `origin/ws/abi` first. It may already exist.
2. Write `N:\mmc-coord\requests\<WS-ID>-<NN>.md` with: what the original mod calls (class and
   method, with the Java snippet), the exact Rust signature and contract you propose (append-only,
   C ABI, what it returns for unloaded or invalid input, who owns memory), and how you'll test it.
3. Work around it meanwhile behind one function in your crate, marked
   `// TODO(abi): <request id>`, so swapping in the real call is one change.
4. WS-0 answers in the same file (`## Answer`: accepted, commit on `ws/abi`, or a counter-design)
   and publishes binaries. Merge `origin/ws/abi` into your branch to use it.

### 4.4 Ports and run directories

Each workstream uses the ports `26000 + 10 * n` to `26000 + 10 * n + 9`, where `n` is its number
(WS-5 uses 26050 to 26059). Run directories go under your worktree's `run\`, which is ignored.
The harness picks its own ports. Pass `--run-root run\harness-<id>` so its directories stay yours.

### 4.5 The log in your branch

Keep `docs/ws/<id>.md` in your branch. The integrator reads it before merging. For each
milestone: what was ported (source files and methods), commits, tests and scenarios with
their results, the comparisons with vanilla or the original mod, numbers if measured, what
isn't verified, and open issues. Keep it current. It's the handoff.

## 5. How many agents, and in what order

Start with these seven. Each is independent of the others' unfinished work:

1. WS-0 integration and ABI (needed early: it publishes the shared binaries)
2. WS-1 buja inventory and simple ports
3. WS-2 Create fluids
4. WS-4 Create addons
5. WS-5 L_Ender's Cataclysm
6. WS-7 engine vanilla parity
7. WS-R reviewer

Add WS-6, WS-8, WS-3 and WS-10 when the first ones are past milestone 1, and more WS-1
sub-streams (one chat per mod, as WS-1 lists them) as the inventory fills in. Keep engine
builders at two (WS-0, WS-7). Ten or so agents at once is reasonable on this laptop if most
of them are mod-only. Watch disk.

## 6. Workstreams

Each workstream lists what it reads first, what it owns, and its milestones in order. A
milestone is done when section 2's checks pass and it's pushed and logged.

### WS-0 Integration and ABI steward

Read first: docs/MOD-ABI.md, docs/INTERFACES-PLAN.md, docs/ARCHITECTURE.md, docs/CONTENT.md,
`api/src/abi.rs`, `engine/moddedmc-host/src/`.

Owns: branch `ws/abi` (outer) and `ws/abi` (engine), `api/`, `engine/moddedmc-host/`,
`docs/MOD-ABI.md` on that branch, `N:\mmc-shared\bin\`.

- **M0 Shared binaries.** Worktree with the engine submodule, `pack` build of `pack-dev`,
  `moddedmc-bot`, `moddedmc-harness` at `origin/main`. Copy them to
  `N:\mmc-shared\bin\<engine-short-sha>\` with `COMMIT` (outer and engine sha) and `ABI.txt`
  (ABI_VERSION and the list of interfaces and extensions), and point `LATEST.txt` at it.
  Smoke test: `scripts/scenario.ps1` with `p1a-create.txt` on your port using those binaries.
  Write `notes\shared-binaries.md` telling everyone how to use them.
- **M1 Request loop.** Watch `requests\`, answer each request within your session, implement
  accepted ones on `ws/abi` (append-only; follow the existing extension pattern: append the
  field, add an `*Extension` variant whose check is the field's end offset, document it in
  MOD-ABI.md, and add a host test where a host test can catch a real bug), publish new
  binaries, and answer in the request file. Merge `origin/main` into `ws/abi` often, because
  main also appends to the ABI. When both append to the same table, keep main's order first and
  put yours after.
- **M2 Inter-mod services.** Create addons (WS-4) need Create's internals: registering kinetic
  blocks in Create's networks, Create's recipe types, fluid handlers. Design and build a
  generic way for one `.mmc` to publish a named, versioned C table and another to require it,
  the same way host interfaces work (`register_service(name, version, table)` in `Mod::register`;
  `require_service` in `Mod::start`, mod load order from declared dependencies in `mod.json`,
  and a clear error and skip when a dependency is missing). Document it in MOD-ABI.md, prove it
  with two tiny test mods and a scenario. Then agree with WS-2 and WS-4 on the first Create
  service table (`create:kinetics` v1: add a kinetic source or consumer, read speed and stress).
  The Create side lives in `mods/create/src/services/` and has to be merged with the
  integrator's Create work, so keep it to new files plus one registration call.
- **M3 Comparator output for content blocks.** Comparators read
  `BlockState::has_analog_output_signal()`, which content states copy from their vanilla
  `physical` template. Add a per-block analog-output flag to content registration, and
  `set_analog_output(world, pos, strength)` that stores the value, notifies comparators like
  vanilla `updateNeighbourForOutputSignal`, and clears it on removal. A scenario with a test
  mod block and a comparator.
- **M4 Requests from the queue**, as they come (expect: entity attribute access, item entity
  creation with motion, sounds at entities, particles with data, block entity menus/screens
  for containers (`moddedmc:menus`: open a chest-like menu backed by mod storage), custom
  recipe types in the crafting book, loot table hooks, worldgen feature registration).
  Menus are the biggest. Plan that one in `docs/MENUS-PLAN.md` before building.

### WS-1 Buja inventory and simple ports

Read first: docs/PORTS.md, docs/ROADMAP.md, docs/BACKLOG.md (P6), `mods/wayshrine` (a small
port), `scripts/new-mod.ps1`.

Owns: branch `ws/buja`, `docs/BUJA.md`, and each simple mod crate it creates.

- **M0 Inventory.** Ask the owner where the buja pack is (launcher instance folder or modlist
  export). Don't guess, and don't treat other packs as buja. List every mod in `docs/BUJA.md`:
  name, mod id, buja's version, **latest** version and its Minecraft version and loader, code
  licence, asset licence, what it adds (a line), dependencies, and a category:
  - `port`: content or server behaviour, so it needs a Rust port (estimate S, M, L or XL).
  - `vanilla`: covered once the engine matches vanilla (performance mods such as Lithium,
    FerriteCore, ModernFix, C2ME go here; our server replaces them).
  - `client`: client only (shaders, minimaps, HUD, JEI/EMI, sounds, menus); not needed on the
    server. Note those the companion should cover later.
  - `library`: APIs other mods need (Architectury, GeckoLib, Curios, Cloth Config). Note which
    ports need a counterpart (GeckoLib models go to the companion renderers, C5).
  - `datapack`: worldgen or data only (Terralith, Tectonic, Incendium, Nullscape); waits on E2.
- **M1 Plan.** Order the `port` mods by how many players would notice, then by size, and
  group the small ones. Propose sub-streams (`WS-1a <mod>`, `WS-1b <mod>`...) at one chat per
  mod for S and M mods. Write them into `docs/BUJA.md` with a starting brief for each, in the
  same shape as the workstreams here. The owner starts those chats.
- **M2 onward.** Port S-sized content mods yourself, one crate each (`scripts/new-mod.ps1`):
  blocks, items, food, decoration, recipes, loot. Each with its scenario. File ABI requests for
  anything that needs entities, menus or worldgen, and move on.

### WS-2 Create fluids (P1e)

Read first: `mods/create/README.md`, `mods/create/src/` (kinetics, belts, depot), Create's
`content/fluids/**`, `content/processing/basin/**`, `content/kinetics/mixer/**`,
`content/kinetics/press/` (compacting), `content/fluids/spout/**`, `content/fluids/drain/**`.

Owns: branch `ws/create-fluids`, `mods/create/src/fluids/**`, basin and mixer.

Create's fluid system is self-contained: pipes, pumps and tanks move fluid stacks between
handlers. It touches vanilla where it places or drains fluid blocks, fills and empties buckets
and bottles, and reads vanilla fluid containers (cauldrons).

- **M0 Study.** Map Create 6.0.10's fluid classes (`FluidTransportBehaviour`, `PipeConnection`,
  `FluidNetwork`, `PumpBlockEntity`, `FluidTankBlockEntity`, `BasinBlockEntity`,
  `MechanicalMixerBlockEntity`, `SpoutBlockEntity`, `ItemDrainBlockEntity`,
  `OpenEndedPipe`, `HosePulley`) and list which host operations they need (fluid block reads
  and writes, `FluidState`, bucket item handling, vanilla fluid handlers for cauldrons).
  File requests for the missing ones (a fluid handler counterpart to `moddedmc:containers` is
  likely).
- **M1 Pipes and pumps.** Fluid pipes (all states, encased, glass), mechanical pump driven by
  Create kinetics (speed decides pressure), flow propagation as `FluidNetwork`, open-ended pipes
  draining and placing source blocks. Scenario: pump water from a pool through pipes into
  another basin of blocks.
- **M2 Fluid tanks.** Multiblock tanks (`ConnectivityHandler`), capacity, the window states,
  comparator output (after WS-0 M3), boiler is later. Scenario: fill and drain a 2x2x3 tank,
  restart, contents kept.
- **M3 Basin and mixer.** Basin inventory and fluids, heat level from blaze burners (port the
  blaze burner too), mechanical mixer and press compacting over a basin, recipes `create:mixing`
  and `create:compacting` from the data. Scenario: andesite alloy by mixing, compacting a
  block.
- **M4 Spout and item drain.** Filling and emptying items on belts and depots
  (`create:filling`, `create:emptying`), hooked into the belt processing Sol ported.
- **M5 Hose pulley, smart pipes, valves, portable fluid interface** (the last after P1d).

Performance: fluid networks tick a lot. Measure a 64-pipe network's tick cost against Java
Create on the same layout and record it.

### WS-3 Create trains (P1f)

Starts after the integrator's P1d contraptions land on main (watch BACKLOG.md). Before that,
M0 and M1 are independent.

Read first: docs/P1D-CONTRAPTIONS.md, Create's `content/trains/**`.

Owns: branch `ws/create-trains`, `mods/create/src/trains/**`.

- **M0 Study.** Map the track graph (`TrackGraph`, `TrackNode`, `TrackEdge`, `BezierConnection`),
  `GlobalRailwayManager`, signals, stations, schedules, and carriage contraptions. List the host
  operations needed (saved world data for the global graph, entities for carriages, player
  riding).
- **M1 Tracks and graph.** Track placement (straight, diagonal, curved via the track item's
  bezier), the graph building and saving (`create_tracks.dat` equivalent in world data),
  graph unit tests against Create's geometry.
- **M2 Stations, signals, observers** as blocks with their graph edge points.
- **M3 Carriages and trains** on top of P1d contraptions: assembly at a station, movement
  along the graph, derailment rules, collision.
- **M4 Schedules and conductors.**

### WS-4 Create addons (P5)

Read first: docs/BACKLOG.md P5, each addon's source, `mods/create/`, WS-0 M2 (services).

Owns: branch `ws/create-addons`, one crate per addon (`mods/create-<addon>`).

Addons: New Age, Create Crafts and Additions, Enchantment Industry, Central Kitchen, Steam 'n'
Rails, Ultimate Factory, More Recipes, Copper and Zinc, Recycle (confirm with WS-1's inventory
and the latest versions).

- **M0 Survey.** For each addon: latest version, what it adds, which of it is data only
  (recipes in Create's types, tags) and which hooks into Create's code (which classes and
  methods). Put the survey in `docs/ws/create-addons.md` and turn the hooks into a list of
  service functions for WS-0 M2.
- **M1 Data-only addons first.** More Recipes, Recycle and the recipe parts of others are
  datapack JSON in Create's recipe types. Make sure they load through the port (each `.mmc`
  ships its `data/`), and scenarios show one recipe of each type working in our Create.
- **M2 Content addons** once `create:kinetics` exists: Create Crafts and Additions
  (electricity, alternator, electric motor, rolling mill), New Age (energy, generators), Central
  Kitchen (food), Copper and Zinc. One crate each, milestone per addon.
- **M3 Steam 'n' Rails** after WS-3 trains.

### WS-5 L_Ender's Cataclysm (P3)

Read first: docs/P2-DRAGONS.md (how sting-raider ports mobs with `moddedmc:mobs` and
`moddedmc:mob_ai`), `mods/saints-dragons`, the mod's latest source.

Owns: branch `ws/cataclysm`, `mods/cataclysm`.

- **M0 Survey.** Latest version, licence of code and assets, every block, item, entity, boss,
  structure, enchantment, effect, recipe. Which need ABI the mobs interfaces don't have yet
  (projectiles, area attacks, boss bars, custom spawn rules, structure generation). File
  requests.
- **M1 Items, blocks, materials, recipes** (ignitium, witherite, ancient metal, weapons
  as items with their attributes and abilities where the ABI allows).
- **M2 Regular mobs** (in order of how often players meet them) with their goals and attacks,
  on the mobs ABI, one mob per commit, scenario per mob (spawns, attacks, drops, survives
  restart).
- **M3 Bosses** (Ender Guardian, Netherite Monstrosity, Ignis, Harbinger, Leviathan, Ancient
  Remnant, Maledictus, Scylla as present in the latest release): boss bars, phases, attack
  patterns ported tick for tick, loot.
- **M4 Structures** once E2e lands on main (datapack structures), or earlier if the mod
  places them in code.
- Models are GeckoLib: stock clients see the carrier mob until the companion renders them (WS-8).

### WS-6 Legendary Monsters (P4)

Same shape as WS-5: branch `ws/legendary-monsters`, `mods/legendary-monsters`, M0 survey, M1
items, M2 mobs, M3 bosses, M4 structures.

### WS-7 Engine vanilla parity

Read first: engine/pumpkin/AGENTS.md (all of it), engine/pumpkin/CONTRIBUTING.md (mob AI
section), docs/parity.md, docs/HARNESS.md, docs/harness-status.md.

Owns: branch `ws/parity` (outer: fixtures and docs) and `ws/parity` in `engine/pumpkin`. One
subsystem at a time, each proven by differential fixtures against the real vanilla server.

Order (what buja players hit first):
- **M1 Mobs (E5)**: spawning rules and caps, the common hostile and passive mobs' goals and
  brains, loot. Follow Pumpkin's mob AI design and the entity-island reach rules exactly
  (AGENTS.md "Mob AI and entity islands"). Coordinate with sting-raider's mobs ABI: don't
  change `moddedmc:mobs`/`mob_ai`, but the vanilla mobs they carry must behave like vanilla.
- **M2 Redstone (E7)** with vanilla update order: dust, repeaters, comparators, observers,
  pistons (quasi-connectivity, BUD), hoppers and droppers. Create machines sit on top of this.
- **M3 Enchanting, brewing, anvils, smithing (E9).**
- **M4 Villagers, trading, raids (E6).**
- **M5 Nether, End, portals, dragon fight (E8).**
- **M6 Advancements, statistics, remaining commands (E10).**
- **M7 Lighting parity (E4)** and **worldgen determinism across x86_64 and aarch64 (E3)**.

For every subsystem: list what's missing against the vanilla classes, write fixtures that fail,
fix them, keep the old fixtures passing, and bench anything on a hot path (MSPT before and
after). Engine changes that a mod needs to see go through WS-0.

### WS-8 Companion client renderers (C5, C6)

Read first: docs/COMPANION.md, `companion/`, docs/CONTENT.md.

Owns: branch `ws/companion`, `companion/`.

The companion is a Fabric 26.3 client mod. Stock clients see content blocks as their vanilla
carriers. The companion should draw them as the mod intends, from data the server sends.

- **M1 Block models**: content blocks drawn with their own models from the delivered assets.
- **M2 Kinetic parts**: rotating shafts, cogs, belts animated from the server's rotation state
  (the server already sends rotation for `rotating_model` parts).
- **M3 Moving contraptions** (after P1d): draw the contraption's blocks as one moving structure
  and collide the player with it client side, as Create's client does.
- **M4 Animated entity rigs**: GeckoLib-format models and animations for mod mobs (dragons,
  Cataclysm bosses).
- **M5 GUIs, HUD, keybinds, particles** (C6).

The agent can't run the Minecraft client (section 2). Test with codec and headless tests,
and write an exact play-test checklist for the owner in the log for each milestone.

### WS-9 Performance and Pi (F5, E11, W1, W2)

Later, as one engine slot freed by WS-7. Bench harness on the Pi (MSPT p50/p95/p99, RSS,
bytes per player), faster chunk generation (density functions, structure-references lock),
the compact world format. Numbers before and after, same seed and settings, on the laptop
and the Pi.

### WS-10 App and brand (A2, A4)

Read first: PRODUCT.md, DESIGN.md, docs/DEVICES.md, `app/`, the impeccable skill (required).
Optional; start only when the owner asks.

### WS-R Reviewer

Owns nothing. Reviews other workstreams' pushed commits the way a maintainer would.

- Pick the oldest unreviewed milestone (from `status\`), read the diff next to the Java source
  it ports, run its unit tests and scenarios on your own port range, and check the rules in
  section 2 (licences, no AI mentions, explicit names, append-only ABI).
- Write `N:\mmc-coord\reviews\<WS-ID>\<commit>.md`: verdict (`ok`, `fix needed`), each finding
  with file and line, what's wrong, the evidence (Java line, failing run), and what correct
  looks like.
- The author fixes `fix needed` findings in new commits and replies in the same file. A
  milestone counts as done only with an `ok` review.
- Also look across branches for duplicated work and for two workstreams heading into each
  other's files, and say so in `notes\`.

## 7. Verification ladder

From cheapest to most convincing. Each milestone climbs as high as its behaviour allows:

1. Compiles, clippy clean, formatted.
2. Unit tests on logic that can break quietly.
3. Bot scenario on our server, including restart for persistent state.
4. Differential harness against vanilla 26.3 (vanilla behaviour).
5. Same inputs on the original mod's own server (NeoForge or Fabric at its Minecraft version)
   with the outcome compared. Record versions and the exact steps in the log. Cross-version
   differences (1.21.1 mod vs 26.3 vanilla) get noted, not "fixed".
6. Owner play test with a real client: the log lists exactly what to do and what they should
   see. Only the owner does this.

## 8. Integration later

The integrator merges branches into `main` in dependency order: `ws/abi` first, then the
workstreams that used it, after reading each log and the reviewer's notes, re-running the
scenarios on main, and fixing conflicts with the main line of work. Branches that keep
merging `origin/main` in and keep their changes to their own files merge cleanly. Anything a
workstream couldn't verify stays listed in its log until someone does.
