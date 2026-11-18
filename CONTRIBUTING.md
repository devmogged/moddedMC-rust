# Contributing

moddedMC-rust is a small experimental project with a clear goal: modded Minecraft on a fast, light Rust server, where installing a mod is "drop a file in `mods/`". Contributions are welcome, but review time is limited, so read this first. A PR that follows these rules can still be closed; we do not owe a review or a merge.

## What helps most

- **Bug reports with a reproduction.** A crash, a regression, a mod that misbehaves, a real performance problem. Use the bug report form.
- **Bug fixes.** One problem per PR, with proof (see below).
- **Mods.** New Rust mods in `mods/` that exercise the mod API, and reports of Fabric JARs that fail to convert (name the mod and version, attach the server log).
- **Duplicates and dead ends.** Link duplicates to the original. If an open issue no longer happens on `main`, say so and give the commit.

A +1 on an existing PR is always better than another PR. Search open issues and PRs before you start. If one covers your problem, test it and comment with what you saw.

## Ask first

Open an issue with the **Idea** form and wait for a yes before you write code for:

- New features or new ABI functions
- Changes to the mod ABI (`api/src/abi.rs`) or the `.mmc` format
- Engine changes beyond bug fixes (commits in `devmogged/Pumpkin`)
- Architecture changes (new crates, new runtimes, new loaders, new platforms)
- New dependencies

Unrequested PRs of this kind are closed, however good the code is.

## Rules for a PR

**Fix one problem.** Include every change that fix needs: API, host, mods, scripts, tests and docs. Cleanup, refactors and a second fix go in their own PRs.

**Bring evidence.** Say how you found the problem, how you checked the fix, and what you saw. "Tests pass" is not evidence. Depending on the change:

- Logic bugs: a failing test first (in the existing test module where one fits), then the fix.
- Gameplay or server behavior: a scenario under `tests/scenarios/` that fails before and passes after, run with `scripts\scenario.ps1`. Paste the relevant log lines.
- Performance: before and after numbers from `tests/bench/` or `scripts\boot-measure.ps1`, on the same machine in the same session.
- Fabric compatibility: the oracle (`tests/oracle/`) passing on both a real Fabric server and ours.

**Own every line.** Code written with a coding agent is welcome, but you are responsible for all of it: read it, run it, and be ready to explain it. Commits and PRs carry your name only; no co-author or "generated with" lines.

**Keep it small and current.** You can have 3 open PRs; a fourth is closed. A PR that conflicts with `main` and sees no activity for 7 days is closed.

**Use a plain title** that says what changed for a user, e.g. `fix(host): broken counters no longer drop their state`.

## Project rules

- **Engine changes are commits in `devmogged/Pumpkin`** (branch `moddedmc`, checked out at `engine/pumpkin`). Commit and push there, then update the submodule pointer here. We merge upstream Pumpkin often, so keep engine changes small, generic and in the Pumpkin style, and put big new systems in new crates.
- **The mod ABI is a promise.** Mods built today must load on tomorrow's server. In `api/src/abi.rs`: append new fields to the end of `HostApi` and `ModApi`, never reorder, remove or change existing ones. A breaking change bumps `ABI_VERSION` and needs a maintainer's yes first. Update [docs/MOD-ABI.md](docs/MOD-ABI.md) in the same PR.
- **Drop-in mods stay outside the server build.** Crates in `api/`, `mods/` and `recompiler/fabric-host` must not depend on Pumpkin crates. Everything they need goes through the ABI.
- **Compiled-in ports use the engine directly.** Crates in `engine/ports/` depend on Pumpkin and link into pack servers (`engine/packs/`). Use them for ports the ABI can't express. See [docs/PORTS.md](docs/PORTS.md).
- **No panics across the boundary.** Host functions must not unwind into a mod, and SDK callbacks catch panics before returning to the server.
- **Unsafe code needs a `// SAFETY:` comment** saying why it is sound. The engine workspace's lints enforce this; follow it everywhere.
- **No copyrighted game files in git.** Minecraft jars, Mojang assets and downloaded mod jars stay in the ignored `cache/`.
- **Mark shortcuts.** A known limit gets a code comment `shortcut: <the limit>, <when to upgrade>`.
- **Licenses.** `engine/` and `patches/` are GPL-3.0 (Pumpkin's license). Everything else is MIT OR Apache-2.0. By contributing you agree your contribution is licensed under the license of the files it changes.

## Setup

Windows with PowerShell is the tested platform. You need Rust (the version in `rust-toolchain.toml`), git, and for the Fabric converter a JDK 25, Gradle and LLVM clang (see [recompiler/README.md](recompiler/README.md)).

```powershell
git submodule update --init
$env:PUMPKIN_MINECRAFT_SERVER_JAR = "<path to the 26.3 server.jar>"   # Pumpkin's build reads vanilla data from it
scripts\engine.ps1 build                    # apply the patch and build the server (release, ~17 min)
scripts\pack-mod.ps1 -Crate counter         # build a mod -> target\mods\counter.mmc
cargo build -p moddedmc-bot --manifest-path engine\pumpkin\Cargo.toml   # the test client
scripts\scenario.ps1 -Scenario tests\scenarios\m1-counter.txt -RunDir run\m1 -Fresh
```

Before you push:

- `cargo clippy --all-targets` and `cargo test` in the root workspace, and `cargo clippy -p moddedmc-host --tests` in `engine/pumpkin` if you touched the host.
- `cargo fmt`.
- The scenarios your change can affect.

Only stop processes you started, by the PID you captured when you started them. Never kill servers or agents by name: other servers may be running on the same machine.

## How issues and PRs are handled

Automation does the bookkeeping, so maintainers can spend their time on review:

- **Labels.** New issues get `needs-triage`. PRs get `area:*` labels from the paths they touch, `abi` when they touch `api/src/abi.rs`, and a `size:*` label from the diff (Cargo.lock excluded). Labels are defined in `.github/workflows/labels.yml`.
- **Ideas.** An `idea` issue becomes `accepted` when a maintainer agrees; only then is a PR for it welcome.
- **Needs more info.** An issue labeled `needs more info` is closed 7 days later if the reporter does not answer. Answer and it is reopened.
- **PR limits.** Authors outside [.github/TRIAGE_EXEMPTIONS.td](.github/TRIAGE_EXEMPTIONS.td) can have 3 open PRs; a fourth is closed. A PR that conflicts with `main` gets `needs-rebase` and is closed after 7 days without activity. These run in dry-run mode until the repository variable `PR_LIMITS_ENFORCE` is `true`.
- **CI** runs `cargo fmt --check`, `cargo clippy -D warnings` and `cargo test` for the root workspace on Windows, Linux and macOS, including the ABI layout tests. The server build needs Mojang's jar, so engine changes are checked locally.
- **Triage with an agent.** [.github/triage/PLAYBOOK.md](.github/triage/PLAYBOOK.md) tells a coding agent how to diagnose a broken server and write a good issue. Point your agent at it.

## Conduct and bans

Be kind and specific. If you spam the project with automation, or make maintainers' lives harder (repeated pings, fights over a closed PR, second accounts to get around a limit), you will be blocked.

## Security

Report security problems privately: see [SECURITY.md](.github/SECURITY.md). Do not open a public issue for them.
