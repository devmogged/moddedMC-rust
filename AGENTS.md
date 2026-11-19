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
5. **PowerShell 5.1 gotchas:** a native command's stderr under `$ErrorActionPreference = 'Stop'` becomes a terminating error, so run long builds through `powershell -File` from bash or without stderr redirection; `$PSScriptRoot` is empty inside `param()` defaults.

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
