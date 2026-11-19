# moddedMC-rust

Modded Minecraft Java 26.3 on Pumpkin, a Rust server. Players use a stock client; mods run on the server. Server owners install mods by dropping `.mmc` (Rust) or `.jar` (Fabric) files into `mods/`. Read [docs/DECISIONS.md](docs/DECISIONS.md) first, then [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/MOD-ABI.md](docs/MOD-ABI.md) before changing the host, the API or a mod. [CONTRIBUTING.md](CONTRIBUTING.md) has the project rules; they apply to you too.

What we never compromise on:

- **Fast and light.** The server should start in well under a second and a mod call should cost microseconds. Measure before and after anything on a hot path (`tests/bench/`, `scripts\boot-measure.ps1`).
- **Drop-in mods.** A mod built once keeps working across server builds. Never make a mod depend on the server's build, compiler or Pumpkin's Rust types.
- **Simple.** Take the smallest change that solves the real problem. Do not add machinery for later.

## Rules for agents

1. **Stop only what you started.** Kill a server, bot or tunnel agent only by the PID you captured when you started it. Other servers may be running on this machine.
2. **Never edit `engine/pumpkin` in git.** Edit the checkout, then `scripts\engine.ps1 save`. `apply` and `build` reset the checkout to the pin plus the patch, which discards unsaved engine edits.
3. **The ABI is append-only.** In `api/src/abi.rs`, add new fields at the end of `HostApi` or `ModApi`; never reorder, remove or change existing ones. Update docs/MOD-ABI.md in the same change.
4. **No game files in git.** Minecraft jars, assets and downloaded mods live in the ignored `cache/`.
5. **PowerShell 5.1 gotchas:** a native command's stderr under `$ErrorActionPreference = 'Stop'` becomes a terminating error, so run long builds through `powershell -File` from bash or without stderr redirection; `$PSScriptRoot` is empty inside `param()` defaults.

## Verifying

- Root workspace (`api`, `mods/*`, `recompiler/fabric-host`, `host`): `cargo clippy --all-targets`, `cargo test`. Builds take seconds.
- Engine workspace (`engine/pumpkin`, with `engine/moddedmc-host` and `engine/moddedmc-bot`): `cargo clippy -p moddedmc-host --tests` and `cargo test -p moddedmc-host`. Its lints deny `unwrap`, `expect`, `panic` and undocumented `unsafe`. A release server build takes about 17 minutes: start it in the background and do something else.
- Behavior: `scripts\scenario.ps1` with the scenarios in `tests/scenarios/` (M1 counter, M2 wayshrine, `abi-world`, `abi-blocks`, `abi-entities`, `worldgen`, `resource-pack`, `menus` and `drivetrain` for the interfaces) and `tests/oracle/sethome.txt` (Fabric). Mods for scenarios come from `scripts\pack-mod.ps1 -Crate <name>` (into `target\mods\`).
- Report what you ran and what you saw. If you could not run something, say so.

## Issues and PRs

- Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening an issue or PR. Fill in the PR template (`.github/pull_request_template.md`).
- Diagnosing a user's broken server: follow [.github/triage/PLAYBOOK.md](.github/triage/PLAYBOOK.md).
- Never write a bare `@login` in an issue, PR or comment unless you mean to notify that person.
- Commits are authored by the human you work for. Never add co-author lines, "Generated with" lines or any other agent or model attribution to commits, PRs, issues or comments.
