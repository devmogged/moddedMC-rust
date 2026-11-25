# moddedMC-rust

Modded Minecraft Java 26.3 on Pumpkin, a Rust server. Players use a stock client; mods run on the server. Server owners install mods by dropping `.mmc` (Rust) or `.jar` (Fabric) files into `mods/`. Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/MOD-ABI.md](docs/MOD-ABI.md) before changing the host, the API or a mod. [CONTRIBUTING.md](CONTRIBUTING.md) has the project rules; they apply to you too.

What we never compromise on:

- **Fast and light.** The server should start in well under a second and a mod call should cost microseconds. Measure before and after anything on a hot path (`tests/bench/`, `scripts\boot-measure.ps1`).
- **Drop-in mods.** A mod built once keeps working across server builds. Never make a drop-in mod (`api/`, `mods/*`) depend on the server's build, compiler or Pumpkin's Rust types.
- **All ports are drop-in mods.** Create, bosses, dragons and other large mods use the same stable ABI and request named interface versions before worlds load. Keep mechanics in the mod and expose generic engine operations through the host. See [docs/PORTS.md](docs/PORTS.md).
- **Simple.** Take the smallest change that solves the real problem. Do not add machinery for later.

## Rules for agents

1. **Stop only what you started.** Kill a server, bot or tunnel agent only by the PID you captured when you started it. Other servers may be running on this machine.
2. **Engine changes are commits in our fork.** `engine/pumpkin` is `devmogged/Pumpkin`, branch `moddedmc`. Commit there (conventional commits with a scope), push the branch, then commit the new submodule pointer here. Keep engine changes generic and in the Pumpkin style so upstreamable fixes can go to Pumpkin-MC; that PR is opened by a human, never an agent.
3. **The ABI is append-only.** In `api/src/abi.rs`, add new fields at the end of `HostApi` or `ModApi`; never reorder, remove or change existing ones. Update docs/MOD-ABI.md in the same change.
4. **No game files in git.** Minecraft jars, assets and downloaded mods live in the ignored `cache/`.
5. **Port the latest release.** The server is Minecraft Java 26.3. Each port follows the mod's newest release on Modrinth or CurseForge, whatever loader or Minecraft version that release targets. Check it before you start, take behaviour, data and assets from that release only (never from an older jar, such as one in a local modpack), and record the version in the crate's `description`. Don't add engine support for an older version's quirks unless a planned port's latest release needs it.
6. **PowerShell 5.1 gotchas:** a native command's stderr under `$ErrorActionPreference = 'Stop'` becomes a terminating error, so run long builds through `powershell -File` from bash or without stderr redirection; `$PSScriptRoot` is empty inside `param()` defaults.

## Working alongside other people

Several people work on this repo at once, each with their own agent. There is no lock, lease or claim tool; the old `mmc-coord` board and `coord.mjs` are retired, so don't run them. The issues of `devmogged/moddedMC-rust` are the board.

- **Taken areas.** Create and its add-ons (`mods/create*`, `mods/createaddition`) belong to Noel-Alex. Saints Dragons (`mods/saints-dragons`) belongs to sting-raider. Don't change those crates; if you need something from them, open an issue.
- **Picking work.** Take a mod from [docs/BUJA.md](docs/BUJA.md) or an open issue. First check that nobody has it: `gh issue list --state open --search "<mod>"`, and recent commits and branches (`git log origin/main --oneline -50`, `git branch -r`). If it's free, open an issue titled `Port <mod>` (or comment on the existing one) and assign it to the person you work for. That is the claim. If someone else is assigned and the issue had activity in the last few days, pick something else.
- **Progress.** Post short updates and blockers as comments on your issue. Close it when the port works in game.
- **Shared code** (`host/`, `api/`, `engine/pumpkin`, root `Cargo.toml`): keep changes small and additive, and push them soon so others aren't building on a stale base.
- **Pushing.** Commit as the person you work for. `git fetch` and rebase on `origin/main` before every push, then push to `main`. If the `engine/pumpkin` pointer conflicts, rebase your engine commits onto `origin/moddedmc` in the submodule, push that, and commit the new pointer. Never force push `main` or `moddedmc`.

## Verifying

- Root workspace (`api`, `mods/*`, `recompiler/fabric-host`, `host`): `cargo clippy --all-targets`, `cargo test`. Builds take seconds.
- Engine workspace (run in `engine/`). Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets` and `cargo test --workspace`. Build the stock pack binary with `scripts/engine.ps1 build`. The incremental `pack` profile is for local testing. Use `-Profile release` for deployment.
- Behavior: `scripts\scenario.ps1` with the scenarios in `tests/scenarios/` (M1 counter, M2 wayshrine) and `tests/oracle/sethome.txt` (Fabric). Mods for scenarios come from `scripts\pack-mod.ps1 -Crate <name>` (into `target\mods\`).
- Report what you ran and what you saw. If you could not run something, say so.

## Issues and PRs

- Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or PR. Fill in the PR template (`.github/pull_request_template.md`).
- Diagnosing a user's broken server: follow [.github/triage/PLAYBOOK.md](.github/triage/PLAYBOOK.md).
- Never write a bare `@login` in an issue, PR or comment unless you mean to notify that person.
- Commits are authored by the human you work for. Never add co-author lines, "Generated with" lines or any other agent or model attribution to commits, PRs, issues or comments.
