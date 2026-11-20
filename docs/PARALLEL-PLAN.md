# Parallel workstreams

How many agents port the whole buja pack at once without getting in each other's way. Each
agent owns one workstream on its own branch and git worktree, checks its work against vanilla
26.3 and the original mod, and leaves a log the integrator reviews and merges later.

The goal for every line of work: a server that plays exactly like Java Minecraft 26.3 with the
original mods, and runs much faster with much less RAM (the target is a Raspberry Pi 5 with
10 players at 20 TPS). Parity comes first. Speed is checked on hot paths.

## 0. Where things stand (2026-11-20)

A merged slice is the first working part of a mod. It's not the finished port. The live
status of each workstream is on the issue board of `devmogged/mmc-coord`
(`node coord.mjs status` prints it).

| Mod or area | Merged to main so far | Still missing |
| --- | --- | --- |
| Create (integrator and Sol) | kinetics, belts, depot, chute, funnels, press, fan, millstone, crushing wheels, bearings, windmills, pistons, gantries, pulleys, chassis, stickers, riders | elevator and clockwork bearings, moving belts, Super Glue, seats, drills and harvesters, player carrying |
| Create trains (WS-3) | straight track placement, connection and the saved track graph | curves, stations, signals, trains, schedules |
| Create fluids (WS-2) | nothing yet | everything; blocked by a fluid conservation bug |
| Farmer's Delight (WS-1) | 7 storage blocks, 4 items, 13 recipes, 7 loot tables | crops, foods, cooking pot, stove, cutting board, knives |
| Create add-ons (WS-4) | More Recipes cooler, passive items of Crafts & Additions, New Age and Enchantment Industry | power, motors, rolling mill, generators, all machines |
| Legendary Monsters (WS-6) | 24 materials, 2 recipes | 46 blocks, 34 mobs, bosses |
| Cataclysm (WS-5) | private repo only (`devmogged/mods-private`) | most of it |
| ABI (WS-0) | saved world data, inter-mod services, mod dependencies | comparator output, menus |
| Engine parity (WS-7) | nothing yet | everything in its list |

## 1. Starting an agent

Every chat gets the same prompt, on any machine. It picks its own job, claims it, works for one
session (a chat lasts at most 80 minutes), hands off and releases it. Paste this, unchanged:

```text
You are one of many agents porting the buja Minecraft modpack (Create and about 60 other Java
mods) to moddedMC-rust, a Rust Minecraft Java 26.3 server forked from Pumpkin. Each chat does
one job for one session and hands off to the next. You have a shell on this machine, git and
the gh CLI. Nobody will give you more context than this.

1. Find the coordination checkout: N:\mmc-coord on the owner's laptop; otherwise
   $MMC_ROOT/mmc-coord if MMC_ROOT is set, else ~/mmc/mmc-coord. If it isn't there, run
   git clone https://github.com/devmogged/mmc-coord <that path>. Then run git pull in it.
2. Run: node <that path>/coord.mjs start
3. Do exactly what it prints. It chooses your job, claims it on GitHub, prepares your worktree,
   and tells you what to read, how to work, when to wrap up and how to finish.

You have at most 80 minutes. Before you stop, for any reason, push your work and run the
release (or verdict) command it gave you, with a handoff that lets the next chat continue.
Never end without it. Ask the person running you only for things only they can provide.
```

`coord.mjs start` does this:

1. Checks the machine (clone, engine submodule, reference sources, engine binaries current with
   `main`, git identity, `gh` sign-in). If something is missing, the chat's job is to set the
   machine up (section 3.1). Only one chat per machine does that; others are told to stop.
2. Reads the board and takes the first job that fits: reviews of submitted commits first, then
   free workstreams by priority. It skips workstreams that are held, blocked, `manual`, private
   (unless this GitHub account can read `devmogged/mods-private`), or too big for the free disk.
   A machine runs at most two engine jobs at once.
3. Claims it, waits 15 seconds and re-reads, so two chats can't win the same job.
4. Creates or reuses the worktree and prints the brief: the issue, the plan section, the last
   handoff, review findings to fix, requests, ports, deadlines and the exact commands.

A claim is a lease. It lasts 85 minutes from the claim or the latest `beat`, so a chat that dies
without releasing frees its job after 85 minutes. Chats that run longer must `beat` at every
milestone. Agents that were already running before this system: run
`node coord.mjs adopt <WS-ID>` once, then use `beat`, `submit` and `release` like everyone else.

## 2. Rules for every agent

These apply to every workstream. They're the owner's standing rules.

**Git**
- On the owner's laptop, commit as the owner: `git config user.name` is `Noel-Alex` and the email
  is already set in the repo config. On anyone else's machine, commit as that person (their own
  name and GitHub email), never as the owner. Never mention AI, ChatGPT, Codex, Claude, DeepSeek
  or any tool in commits, branch names, code comments or docs. No `Co-Authored-By` lines.
- Conventional commits with a scope, subject line only: `feat(create): port the fluid pipe`,
  `fix(engine): ...`, `test(cataclysm): ...`. One logical change per commit.
- Stage explicit paths. Never `git add -A`, `git add .` or `git commit -a`.
- Work only on your own branch (`ws/<id>` in this repo, and `ws/<id>` in `engine/pumpkin` if your
  workstream edits the engine). Never commit to `main` or the engine's `moddedmc` branch, never
  merge into them, never force push, never rewrite commits you've already pushed.
- Push only to `origin` (github.com/devmogged/...). Never push to upstream Pumpkin-MC.
- Never open pull requests, issues, reviews or comments on GitHub, even if asked. The one
  exception is the board in `devmogged/mmc-coord`, and only through `coord.mjs`. Your work ends
  at a pushed branch, an updated log and a release on the board.
- Uncommitted changes you didn't make belong to someone else. Never discard, stash, reformat or
  commit them.

**Porting**
- Port each mod's **latest release** (check Modrinth or CurseForge first), whatever loader or
  Minecraft version it targets. Never port from the old jars in the buja modpack. Put downloaded
  jars in the repo's ignored `cache/` folder and write the version in the crate's description.
- Port behaviour from the real source code, method by method, keeping the original names so a
  reviewer can read the Rust next to the Java. Follow the whole call chain, every override,
  ordering and side effects (AGENTS.md "Porting"). Never port from memory, the wiki or guesses.
- Reference sources live in a folder outside the repo, called `<refs>` here. On the owner's
  laptop that's `D:\mc-ref`; elsewhere section 3.1 says how to fill it. Vanilla 26.3 source
  (decompiled, readable): `<refs>\26.3\src\net\minecraft\`. Vanilla server jar:
  `<refs>\26.3\server-bundler.jar`. Create 6.0.10 source: `<refs>\create-src`. Put other sources
  (NeoForge, other mods) under `<refs>\<name>-<version>\`.
- Mechanics live in the mod (`mods/<id>`). The engine and host only provide generic operations
  through the stable C ABI in `api/` (docs/MOD-ABI.md). A mod never depends on the server's Rust
  types. If you need a host operation that doesn't exist, file an ABI request (section 4.3).
  Don't edit `api/` or `engine/` yourself unless your workstream owns them.
- Licences: never commit assets that are All Rights Reserved (Create's textures, models, sounds
  and lang, and any other mod whose assets aren't freely licensed). Check each mod's licence.
  ARR assets stay in the local jar copy, and the server reads them from there the way Create's
  are read. Code and data (recipes, loot tables, tags) are fine if the mod's code licence allows.
- Java edition only. Bedrock is out of scope.
- Private mods stay private. L_Ender's Cataclysm, and any mod the owner marks private, is
  pushed only to the private repo `devmogged/mods-private`, never to `moddedMC-rust` under any
  branch name. Only machines the owner has given access to that repo take those workstreams.

**Machine**
- Someone is using the machine you run on. Never launch the Minecraft client, never simulate
  keystrokes, mouse clicks or take over the desktop. Bots (`moddedmc-bot`) and the harness are
  how you play.
- Watch disk. On the owner's laptop it's tight (about 235 GB free on N:): build the engine with
  the `pack` profile only, and never create extra engine target directories except where
  section 3 allows. On any machine, delete run directories once their scenario passes
  (`scripts/scenario.ps1` does this itself), delete scratch files you made, and never leave
  multi-GB leftovers.
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

**Skills** (instruction files; open and follow them when the task fits):
- Appendix A of this plan, the engineering contract: always.
- `systematic-debugging`: any bug hunt.
- `test-driven-development`: logic-heavy code.
- `impeccable`: any UI work, required.
- `ponytail`: keeping changes minimal.
- `humanizer`: before writing docs others read.

On the owner's laptop they're in `C:\Users\noela\.claude\skills\<name>\SKILL.md`. On other
machines, install them from the private `devmogged/skills` repo:
`git clone https://github.com/devmogged/skills`, then in it
`node scripts/skills.mjs install <name> --agent <your agent, e.g. codex>`. If you can't install
one, read `skills/<name>/SKILL.md` in that clone directly.

## 3. Branches, worktrees and builds

### 3.1 Setting up a machine other than the owner's laptop

Do this once per machine. The person who owns the machine needs a GitHub account that the owner
has added to the `devmogged` organisation, with write access to `moddedMC-rust`, `Pumpkin` and
`mmc-coord`, and read access to `skills`. Without that, stop and ask them to get it.

1. Tools: Git, Rust (rustup; `rust-toolchain.toml` picks the version), Node.js 20+,
   PowerShell 7 (`pwsh`; on Linux and macOS the `.ps1` scripts run with it too), and a Java
   runtime for the vanilla reference server (the version the 26.3 version JSON names under
   `javaVersion`). Sign in with `gh auth login` so git can push.
2. Pick a work root with at least 80 GB free, called `<root>` here: `~/mmc` by default, or set
   the `MMC_ROOT` environment variable to another folder before starting chats. `coord.mjs`
   finds everything relative to `<root>\mmc-coord`. Clone the code, its engine and the
   coordination repo:

   ```powershell
   git clone --recurse-submodules https://github.com/devmogged/moddedMC-rust "<root>\moddedMC-rust"
   git clone https://github.com/devmogged/mmc-coord "<root>\mmc-coord"
   git -C "<root>\moddedMC-rust" config user.name "<your name>"
   git -C "<root>\moddedMC-rust" config user.email "<your GitHub email>"
   ```

   Set the same name and email in `<root>\mmc-coord` and in `engine\pumpkin`.
3. Reference sources in `<root>\refs` (that's your `<refs>`):
   - Vanilla 26.3, as AGENTS.md "Getting the source" says: from
     `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json` take the 26.3 version
     JSON, download `downloads.server.url` to `<refs>\26.3\server-bundler.jar`, extract the
     real jar from `META-INF/versions/26.3/` to `<refs>\26.3\server-26.3.jar`, and decompile it
     with Vineflower into `<refs>\26.3\src`. Vineflower itself goes in `<refs>\tools`.
   - Create: `git clone --depth 1 --branch mc1.21.1-6.0.10 https://github.com/Creators-of-Create/Create "<refs>\create-src"`.
   - Whatever your workstream ports: its source for the latest release, under
     `<refs>\<name>-<version>`, and its released jar in the repo's ignored `cache\` folder.
4. Engine binaries: build once with
   `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness`
   (about 31 GB in `engine\target`). Rebuild only when `main` moves the `engine/pumpkin` pin or
   changes `engine/moddedmc-host`. Every workstream on this machine uses that one build.
5. Check the setup: `scripts/pack-mod.ps1 -Crate create`, then
   `scripts/scenario.ps1 -Scenario tests\scenarios\p1a-create.txt -Port 26900`. It has to end
   with "all checks passed".
6. Record the build with `node coord.mjs built --session <S>` (the setup brief prints it), so
   later chats know the binaries match `main`. They rebuild through the same setup job whenever
   `main` moves the engine pin or changes the host.

On another machine, `N:\git repos\moddedMC-rust` in this plan means `<root>\moddedMC-rust`,
`N:\mmc-coord` means `<root>\mmc-coord`, `N:\mmc-wt` means `<root>\wt`, and the shared binaries
of step 4 replace `N:\mmc-shared\bin`. Ports don't clash across machines, so use section 4.4's
ranges as they are.

### 3.2 Worktrees and builds

The main checkout `N:\git repos\moddedMC-rust` (branch `main`) belongs to the integrator and its
worker. Don't work in it. On another machine, keep your clone on `main`, untouched, and work in
worktrees too. Each workstream gets its own worktree, and `coord.mjs start` creates or reuses
it for you (`<root>\wt\<id>`, or the laptop's existing one). By hand it would be:

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
- **Engine workstreams** (on the owner's laptop, WS-0 and WS-7 only; on another machine, any
  engine workstream that machine claims) also run `git submodule update --init engine/pumpkin`
  in their worktree, check out `ws/<id>` inside `engine/pumpkin` from `origin/moddedmc`, and build
  with `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness`.
  That gives each of them one engine target directory (about 31 GB each). On the laptop, no
  other workstream builds the engine. On another machine, an engine workstream's worktree build
  is separate from the shared build of section 3.1, so plan for 31 GB more.
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
| Engine subsystems on branches (redstone, enchanting, villagers, dimensions, advancements, lighting) | WS-7 and WS-31 to WS-36, one subsystem each | request |
| Each new mod crate of WS-11 to WS-30 | that workstream | |

Inside `mods/create`, workstreams other than the integrator add new files only (their own
subfolder) and touch shared files (`lib.rs`, `README.md`, `Cargo.toml`) only to add their
module and its registration call, each in one block marked with a comment naming the
workstream. That keeps merges with main small.

### 4.2 The board

Coordination lives in the private repo `devmogged/mmc-coord`: its issues are the board, and
`coord.mjs` in it is the only way agents touch them. On the owner's laptop the checkout is
`N:\mmc-coord`; elsewhere, `<root>\mmc-coord`.

- **One issue per workstream** (label `ws`). The body holds its brief and a hidden meta line
  (id, kind, priority, branch, plan section). Labels (`state:*`, `review:*`, `kind:*`,
  `private`, `manual`) are a summary for people; the truth is the comments.
- **One issue per request** (label `request`, title `[WS-12 → WS-0] ...`), section 4.3.
- **Every action is a comment** written by `coord.mjs`, ending in a hidden `coord` line:
  `claim`, `beat`, `submit`, `release`, `verdict`, `answer`, and the integrator's `merged` and
  `set`. Every agent replays them in order with the same rules, so they all agree on who holds
  what. Never edit or delete these comments by hand.
- **The handoff is the release comment.** It replaces the old status files: what was done
  (commits), what was verified and how, what isn't verified, and exactly where to start next.
  The next chat on that workstream gets it in its brief, on whatever machine it runs.
- **States**: `free` (work left), `blocked` (until listed issues close, or until a condition the
  integrator clears), `finished` (every milestone done and submitted), closed when merged and
  done. A workstream with open requests addressed to it is offered even when finished.
- **Reviews**: `submit` puts a pushed commit in the review queue; a review job ends with a
  `verdict` (approve or changes). The work lease and the review lease are separate, so work
  continues while a review runs.
- Notes for everyone (a gotcha, a harness trick) still go in `notes\` of the checkout, one file
  per topic, synced with `.\sync.ps1 -Paths <file> -Message "<what>"`. Read
  `notes\INTEGRATOR-*.md` first: those are the integrator's decisions and override older notes.
- Large logs (over about 200 KB) go nowhere public. Keep the failing excerpt and the command.

`claims.md`, `status\` and `requests\` in the checkout are the old system, kept read-only for
history.

### 4.3 Requests for the ABI or the engine

When a port needs something the host doesn't offer (an entity operation, a world query, a
recipe hook):

1. Check docs/MOD-ABI.md on `origin/main` and on `origin/ws/abi` first. It may already exist.
2. Search the open `request` issues on the board and add to an existing one rather than filing a
   duplicate. Otherwise write the request to a file: what the original mod calls (class and
   method, with the Java snippet), the exact Rust signature and contract you propose (append-only,
   C ABI, what it returns for unloaded or invalid input, who owns memory), and how you'll test it.
   File it with `node coord.mjs request --session <S> --for WS-0 --title "<short>" --file <file>`
   (`--for WS-7`, or `WS-31` to `WS-36`, for vanilla engine behaviour).
3. Work around it meanwhile behind one function in your crate, marked
   `// TODO(abi): #<issue>`, so swapping in the real call is one change. If nothing useful is
   left without it, release with `--state blocked --blocked-by <issue>`. The workstream is
   offered again as soon as that issue closes.
4. The owner answers with `coord.mjs answer --issue <n> --file <file>` (accepted with the commit
   on `ws/abi`, or a counter-design), and closes it with `--close` once binaries that include it
   are published. Merge `origin/ws/abi` into your branch to use it.

### 4.4 Ports and run directories

Each workstream uses the ports `26000 + 10 * n` to `26000 + 10 * n + 9`, where `n` is its number
(WS-5 uses 26050 to 26059, WS-23 uses 26230 to 26239). Review jobs use `26400 + 10 * n` to
`26400 + 10 * n + 9`, and machine setup uses 26990 to 26999. The brief prints your range. Sub-streams like WS-1a share their
parent's range on different machines; on the same machine, ask the parent for two of its ports. Run directories go under your worktree's `run\`, which is ignored.
The harness picks its own ports. Pass `--run-root run\harness-<id>` so its directories stay yours.

### 4.5 The log in your branch

Keep `docs/ws/<id>.md` in your branch. The integrator reads it before merging. For each
milestone: what was ported (source files and methods), commits, tests and scenarios with
their results, the comparisons with vanilla or the original mod, numbers if measured, what
isn't verified, and open issues. Keep it current. It's the handoff.

## 5. How many agents, and in what order

The board hands out jobs in priority order, so nobody picks by hand: reviews first, then WS-0
to WS-7 (furthest along), then this table. Keep the laptop at about ten chats; the script allows
two engine jobs per machine.

The order after WS-0 to WS-7: Mod ports need only the shared engine build of section 3.1, so
they fit any machine with about 80 GB free. Engine workstreams need about 120 GB.

| Order | Workstream | Kind | Can start now? |
| --- | --- | --- | --- |
| 1 | WS-11 Waystones | mod, M | yes |
| 2 | WS-12 Corpse | mod, M | yes |
| 3 | WS-13 Lootr | mod, M | yes |
| 4 | WS-14 Explorer's and Nature's Compass | mod, M | yes (structure and biome search may need ABI) |
| 5 | WS-15 Epic Knights | mod, L | yes |
| 6 | WS-16 Musket Mod | mod, M | yes |
| 7 | WS-17 Polymorph | mod, M | yes |
| 8 | WS-18 Sophisticated Backpacks | mod, L | M0 and items now; menus after WS-0 M4 |
| 9 | WS-19 Sophisticated Storage | mod, L | same as WS-18 |
| 10 | WS-20 Simple Storage Network | mod, M | same as WS-18 |
| 11 | WS-21 Refined Storage | mod, XL | same as WS-18 |
| 12 | WS-22 Alex's Mobs | mod, XL | yes, on the mobs ABI |
| 13 | WS-23 Quark | mod, XL | yes, one module at a time |
| 14 | WS-24 Alex's Caves | mod, XL | items and blocks now; biomes after E2 |
| 15 | WS-25 Recruits | mod, XL | yes, on the mobs ABI |
| 16 | WS-26 Small Ships | mod, L | entity riding may need ABI |
| 17 | WS-27 Siege Weapons | mod, L | same as WS-26 |
| 18 | WS-28 Easy Villagers | mod, M | after WS-33 villagers |
| 19 | WS-29 Steam 'n' Rails | mod, L | after WS-3 M3 |
| 20 | WS-30 Worldgen datapacks | data | after E2 lands on main |
| 21 | WS-31 Redstone parity | engine | yes |
| 22 | WS-32 Enchanting, brewing, anvils, smithing | engine | yes |
| 23 | WS-33 Villagers, trading, raids | engine | yes |
| 24 | WS-34 Nether, End, portals, dragon | engine | yes |
| 25 | WS-35 Advancements, statistics, commands | engine | yes |
| 26 | WS-36 Lighting and worldgen determinism | engine | yes |
| 27 | WS-8, WS-9, WS-10 (section 6) | client, perf, app | WS-9 needs a Raspberry Pi 5 |

Farmer's Delight stays in WS-1 and the Create add-ons stay in WS-4. Either may hand a whole
mod to another machine as a sub-stream (`WS-1a Farmer's Delight`, `WS-4a New Age`) by writing
its brief in their log and asking for it in their handoff. The integrator adds the issue.

Client-only mods (AppleSkin, Jade, JEI, Embeddium, Mouse Tweaks and the like), performance mods
(FerriteCore, ModernFix, Canary and the like) and libraries (Architectury, Balm, Citadel,
Curios and the like) aren't ported on their own. Client features go to the companion (WS-8),
performance mods are covered by the engine, and a library's API is ported inside the first
mod that needs it, then moved to a service (WS-0 M2) when a second mod needs it. The full list
is in `docs/BUJA.md`.

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
- **M1 Request loop.** Work the open `request` issues addressed to WS-0 (your brief lists them),
  oldest and most-blocking first, answer each with `coord.mjs answer`, implement
  accepted ones on `ws/abi` (append-only; follow the existing extension pattern: append the
  field, add an `*Extension` variant whose check is the field's end offset, document it in
  MOD-ABI.md, and add a host test where a host test can catch a real bug), publish new
  binaries, then close the issue (`answer --close`). Merge `origin/main` into `ws/abi` often, because
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

### Reviews (formerly WS-R and WS-37)

Reviewing is a job, not a workstream. `coord.mjs start` hands out the oldest submitted commit
before any other work. The reviewer reads the diff next to the Java source it ports, runs the
unit tests and scenarios on its review ports, checks section 2's rules (licences, no AI
mentions, private mods, append-only ABI), and posts a `verdict`: approve, or changes with each
finding's file and line, what's wrong, the evidence (Java line, failing run) and what correct
looks like. The next work session on that workstream gets the findings in its brief and fixes
them first. A milestone counts as done only with an approving verdict, and the integrator merges
only approved commits. Reviewers also write in `notes\` when they see duplicated work or two
workstreams heading into each other's files.

### The mod port template (WS-11 to WS-30)

Every mod workstream below follows these milestones. Its own entry adds what's specific to it.
Its branch is `ws/<crate>` and its crate is `mods/<crate>` (create it with `scripts/new-mod.ps1`).

- **M0 Survey.** The latest release (Modrinth or CurseForge), its loader and Minecraft
  version, and the licence of code and of assets, checked against the files in the jar and the
  repo. Then the full inventory: every block, item, entity, menu, recipe type, worldgen
  feature, command, config option and network packet. For each, the host operations it needs,
  marked "exists" (with its MOD-ABI.md row) or "missing" (file a request). Write it to
  `docs/ws/<crate>.md`. If the code licence forbids a port, stop and tell the owner.
- **M1 Data and simple content.** Items, simple blocks, tags, recipes in vanilla recipe types,
  loot tables. A scenario that obtains, places, crafts, breaks and survives a restart.
- **M2 Behaviour.** Block entities, menus, interactions and entities, in the order players
  meet them, one feature per commit, each with its scenario and the comparison with the
  original mod (verification ladder, step 5).
- **M3 Everything else** the survey listed, until nothing is missing. Then a final log entry
  that lists every feature with how it was verified.
- Assets: only when the asset licence allows. Otherwise the server reads them from the
  original jar in `cache\`, the way Create's are read.

### WS-11 Waystones

Waystones, warp stones, warp scrolls, the waystone menu and teleport rules with their costs
and cooldowns, waystone generation in villages (after E2) and the global waystone list in
saved world data (`moddedmc:saved_data`). Needs the menus ABI (WS-0 M4). Do the data, blocks,
saved list and teleport logic with unit tests first.

### WS-12 Corpse

Death corpses holding the player's inventory, the corpse menu, despawn and ownership rules, and
the death history command. Hook player death through the host; file the request (a death event
that can take the drops) if it isn't there.

### WS-13 Lootr

Per-player loot containers that replace vanilla loot chests, barrels, shulkers and minecart
chests, their per-player inventories in saved data, the refresh and decay options, and the
break rules. Compare against vanilla loot generation with the harness: a Lootr chest must roll
the same loot table vanilla would.

### WS-14 Explorer's Compass and Nature's Compass

Both search the world for the nearest structure or biome on a worker thread and point the
compass at it. Needs structure and biome lookup from the host, like vanilla `/locate`; file one
request that serves both. Port the search ranges, timeouts and item behaviour exactly.

### WS-15 Epic Knights

Armour sets, shields, weapons with their attributes and special abilities, armour trims and
decorations, and the crafting recipes. Mostly items: check attribute modifiers, durability and
enchantability against the original for every item. Models and textures follow the asset
licence.

### WS-16 Musket Mod

Muskets, pistols and blunderbusses with reloading, ammunition, the bullet projectile and its
damage and penetration rules, and the bayonet. Projectiles need an entity or projectile ABI
from WS-0; ask early.

### WS-17 Polymorph

Lets a player choose between recipes that share the same input in crafting tables, furnaces
and smithing. Server side, that's the recipe selection and the packet the choice comes in on.
The stock client can't show the picker without the companion, so plan the server part and
write down what the companion (WS-8) has to draw.

### WS-18 Sophisticated Backpacks

Backpack tiers, upgrades (pickup, filter, magnet, feeding, crafting, smelting and the rest),
the backpack menu, wearing it, and placing it as a block. The inventory with its upgrades
lives in the item's components. Needs menus (WS-0 M4). Do items, tiers and the inventory model
with unit tests first.

### WS-19 Sophisticated Storage

Chests, barrels, shulker boxes and their tiers, upgrades, the storage menu, and tier upgrading
in place. Shares its upgrade system with WS-18, so agree with WS-18 on one shared crate for the
common code (`mods/sophisticated-core`) before either writes it.

### WS-20 Simple Storage Network

A network of cables, a master block and storage, a terminal menu that searches every connected
inventory, import and export cables with filters. Reads containers through `moddedmc:containers`.
Needs menus (WS-0 M4).

### WS-21 Refined Storage

Disks and drives, the controller and its energy use, grids, crafting autocrafting patterns,
importers, exporters, interfaces and wireless. The biggest storage mod: write the network and
storage model with unit tests and the throughput numbers against the original first. Needs
menus (WS-0 M4) and energy (agree with WS-4, whose New Age and Crafts & Additions need energy
too, on one energy service).

### WS-22 Alex's Mobs

About 90 mobs, each with its goals, attacks, drops and spawn rules, plus the items and blocks
they bring. Uses `moddedmc:mobs` and `moddedmc:mob_ai` the way `mods/saints-dragons` does
(read docs/P2-DRAGONS.md). One mob per commit, in order of how often players meet them, with a
scenario per mob. GeckoLib-style models need the companion (WS-8); stock clients see the
carrier mob.

### WS-23 Quark

Quark is many independent modules. Port it one module at a time, each module its own
milestone, in this order: building blocks and decoration, tweaks that change vanilla behaviour
(check each against the harness), automation, management, then the rest. Many of its client
tweaks go to the companion. Keep a table of every module and its state in the log.

### WS-24 Alex's Caves

Five cave biomes with their blocks, items, mobs and structures. Blocks and items come first.
Biomes and structures wait for runtime datapack worldgen (E2) on main. Mobs follow the WS-22
pattern.

### WS-25 Recruits

Hireable soldiers with commands, formations, groups, ranks, upkeep, and their menus. Mob work
on the mobs ABI, plus menus (WS-0 M4). Port the AI goal by goal against the original.

### WS-26 Small Ships

Ships as rideable vehicle entities with sails, cannons and storage. Needs vehicle movement and
riding for mod entities; ask WS-0 early and agree on one vehicle ABI with WS-27.

### WS-27 Siege Weapons

Catapults, ballistas, cannons and the like, as vehicle entities with projectiles. Shares the
vehicle ABI with WS-26.

### WS-28 Easy Villagers

Villagers picked up as items, villager blocks (breeder, converter, farmer, iron farm, trader,
auto trader). Waits for vanilla villagers and trading (WS-33) on main, since it relies on them.

### WS-29 Create: Steam 'n' Rails

Extra tracks, couplers, conductor whistles and the rest. Starts after WS-3 M3 (trains) is
merged to main, and works with WS-3 on the track types.

### WS-30 Worldgen datapacks

Terralith, Tectonic, Incendium, Nullscape and the other datapack mods in docs/BUJA.md. Starts
when runtime datapack worldgen (E2) is on main. For each pack: load its latest release, then
compare generated chunks with the vanilla server running the same pack on the same seed
(`pumpkin-world` AGENTS.md says how to compare worldgen). Fix what differs in the engine
through WS-36, or report it.

### WS-31 to WS-36 Engine subsystems

These split WS-7's later milestones so they can run at once on other machines. Each uses WS-7's
rules (read its entry), its own `ws/<id>` branches in the outer repo and in `engine/pumpkin`,
and differential fixtures against the real vanilla server. Agree with WS-7 in `notes\` before
touching shared engine files (`world/mod.rs`, `entity/mod.rs`) so two branches don't rewrite the
same function.

- **WS-31 Redstone (E7):** dust, repeaters, comparators, observers, pistons with
  quasi-connectivity and BUD behaviour, hoppers, droppers and dispensers, in vanilla update
  order. Create machines sit on top of this.
- **WS-32 Enchanting, brewing, anvils, smithing (E9).**
- **WS-33 Villagers, trading, raids (E6).**
- **WS-34 Nether, End, portals and the dragon fight (E8).**
- **WS-35 Advancements, statistics and the remaining commands (E10).**
- **WS-36 Lighting parity (E4) and worldgen determinism across x86_64 and aarch64 (E3).**

WS-7 keeps M1 (mobs). Its M2 to M7 now belong to WS-31 to WS-36.

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

The integrator merges approved commits into `main` (`node coord.mjs queue` lists them, and
`coord.mjs merged` records each merge) in dependency order: `ws/abi` first, then the
workstreams that used it, after reading each log and the reviewer's notes, re-running the
scenarios on main, and fixing conflicts with the main line of work. Branches that keep
merging `origin/main` in and keep their changes to their own files merge cleanly. Anything a
workstream couldn't verify stays listed in its log until someone does.

## Appendix A. Engineering contract

Every agent on every machine follows this.

**Before changing code**
1. Read AGENTS.md, CONTRIBUTING.md, docs/ROADMAP.md, docs/BACKLOG.md, docs/HANDOFF.md,
   docs/ARCHITECTURE.md, docs/MOD-ABI.md and your workstream's docs. Re-check the source and
   `HEAD` every session instead of trusting what you remember.
2. Decide whether the task is an audit (no code changes) or implementation.
3. Look at `git status --short --branch`, `git log -5`, who else is active (the board) and the
   `engine/pumpkin` submodule state. Keep uncommitted work you didn't make: no blind resets,
   stashes, checkouts or force pushes.
4. Use the pinned engine submodule `engine/pumpkin`, never a separate Pumpkin clone.
5. Name the exact next milestone and the smallest behaviour test that proves it.

**While working**
- The goal is the Java Minecraft and original-mod behaviour, much faster, with as little
  wasted effort as possible.
- Port behaviour from the official latest source of the mod, never from intuition. Follow its
  source code, state ids, NBT tags, lifecycle and client and server split.
- The engine and host provide generic operations. Mod mechanics live in the Rust `.mmc` mods.
  The C ABI is append-only, and a mod never depends on the server's Rust types or compiler
  version.
- Keep assets with restricted licences out of public git.
- One scenario per mechanic, with a persistence and restart check. Use the vanilla oracle
  harness wherever it applies, and note behaviour that differs between Minecraft versions.
- Run formatting, Clippy, the unit tests, the targeted scenarios and the existing regression
  scenarios of what you touched. Benchmark only hot paths you changed. A successful build is
  never a parity claim.
- Small, coherent diffs, conventional commits, explicit paths, no tool attribution in commits,
  never a push to upstream Pumpkin-MC.
- Stop only processes you started. No broad test reruns, giant rebuilds, environment installs
  or side tasks without a reason.

**Reporting**
Short and exact: commits and files changed, tests with PASS or FAIL and the exact commands,
parity and benchmark evidence, host gaps that block you, and the precise next step. Never
present unverified progress as done.
