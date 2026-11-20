# WS-0 Integration and ABI

## M0: Shared binaries (verified 2026-11-20)

- Outer source commit: `8f1b600` (`origin/main`); pinned Pumpkin engine commit: `d6e5e8d560709a2b0aedb49c28e0986c6f93b833` (`origin/moddedmc`). Worktrees and branches `ws/abi` for both. No code changed in this milestone.
- Build: `cargo build --manifest-path engine/Cargo.toml --profile pack -p pack-dev -p moddedmc-bot -p moddedmc-harness` **PASS** (pack profile, ~10m56s fresh build). Artifact target directory for this worktree: `target/pack/` (not `engine/target/pack/`).
- Test prerequisite: `powershell -NoProfile -File scripts/pack-mod.ps1 -Crate create` **PASS**; packaged `target/mods/create.mmc` (314043 bytes) without committing restricted assets.
- ABI tests: `cargo test -p moddedmc-api` **PASS**, six layout tests and no failed doc tests; `cargo fmt --all --check` **PASS**; `cargo clippy -p moddedmc-api --all-targets` **PASS**.
- Scenario: `scripts/scenario.ps1 -Scenario tests/scenarios/p1a-create.txt -Port 26000 -RunDir run/0-p1a -Fresh -Exe '..\\..\\mmc-shared\\bin\\d6e5e8d560\\pack-dev.exe' -BotExe '..\\..\\mmc-shared\\bin\\d6e5e8d560\\moddedmc-bot.exe'` with `MODDEDMC_ASSET_JARS` set to the locally held Create 6.0.10 jar **PASS**. Verified content block registration, pack SHA-1 receipt, +16 motor/shaft/large cog, -32 and +32 small cogs, changed 24 RPM setting and -48 RPM cog, restart preserved 24 and +48 RPM, breaking the shaft stopped dependent cog, no panic; run directory auto-cleaned.
- Published `pack-dev.exe`, `moddedmc-bot.exe`, `moddedmc-harness.exe`, `COMMIT`, `ABI.txt` under `N:\mmc-shared\bin\d6e5e8d560\`, with three binaries checked byte-identical to the build by SHA256; `LATEST.txt` points there. Shared usage note: `N:\mmc-coord\notes\shared-binaries.md`.
- Java Create oracle **not rerun** for M0; the test verifies our baseline smoke behavior, not full parity. No new gameplay functionality or changed hot path, so no performance comparison was made. No client launched.
- Request queue was empty at initial worktree setup; M1 backlog arrived during the builds.

## M1: ABI request loop

### WS-3-01: Save-global data (implemented and tested)

- Added ABI 1 table `moddedmc:saved_data` with fixed 64-bit offsets (size 32), opaque mod-scoped runtime keys and up to 8 MiB per key. No existing C-ABI offsets changed. The SDK requires the table at mod startup, stages a key in `Mod::register`, and reads/writes with a live world callback.
- Per-server-save file `<world>/moddedmc/saved_data.nbt` shares data across dimensions without per-block-entity storage. Binary NBT round-trips exactly; names are owner-qualified; file totals are bounded to 64 MiB and 4096 keys. Invalid payloads or corrupt reads fail closed. An atomic temporary-file replacement runs on world save and graceful shutdown; concurrent flushes are serialized. A crash before save can still lose recent writes.
- Corrected the named-root NBT reader to match the writer after a real restart scenario first exposed silent empty loads. The host unit test now covers this parsing boundary. Content tick callbacks now run for mods without custom blocks; the tiny test mod exercises that case.
- Unit tests: `cargo test -p moddedmc-api` **7 passed**; `cargo test --manifest-path engine/Cargo.toml -p moddedmc-host --lib saved_data::tests` **2 passed**; both cargo format checks and SDK Clippy with `-D warnings` **passed**.
- Scenario: `tests/fixtures/abi-saved-data/verify.ps1 -Port 26000` **PASS** on two fresh independent server saves, including separate process restart with exact binary bytes unchanged. Corrupt saved NBT was rejected with no write; SHA-256 of the file remained unchanged. Run directories were cleaned.
- Existing Create compatibility: `tests/scenarios/p1a-create.txt` **PASS** after saved-data host changes (gear ratios, resource pack, save/restart and breaking a shaft). The previously compiled Create `.mmc` was reused unchanged. No claim of Java Create parity from this regression.
- Host `cargo clippy --manifest-path engine/Cargo.toml -p moddedmc-host --lib -- -D warnings` **PASS**. The final pack build **PASS** and the saved-data verification suite was rerun successfully after the last code change. Shared artifact publication follows the pushed implementation commit.

### Other requests

- Pending: `WS-1-01..03`, `WS-2-01..02`, `WS-3-02`, `WS-4-01..05`, `WS-5-01..11`, and `WS-6-01..02`. Several overlap; share generic host surfaces instead of introducing mod-specific duplicates. A Cataclysm implementation remains blocked by the source rights requirement; generic host functionality can be independently developed and tested.

## M2: Inter-mod services

- Pending. The host currently calls `Mod::start` while loading a library, before `Mod::register` stages content. Services registered during `register` cannot be required from `start` without an explicit two-phase lifecycle and dependency ordering design. Keep the ABI append-only.

## M2: Versioned inter-mod service registry (verified 2026-11-20)

- Added `moddedmc:services` v1 table (64-bit layout: size at offset 0, publish at 8; total 16 bytes). `HostApi` and existing named interfaces are unchanged. SDK `moddedmc_api::services` provides `require_host`, unsafe publication, and size-checked typed `require`/`require_prefix`.
- A publisher stages a process-lifetime static C table during `Mod::register`; a failed registration leaves no advertised interface. Service names are restricted to their own mod's namespace; versions are exact and duplicates are rejected. `get_interface` resolves the published tables.
- The `.mmc` `mod.json` optional `dependencies` array determines a deterministic startup order. The host registers needed providers before starting dependents, including mods that would ordinarily wait until the first tick. Independent mods retain their previous startup timing. Missing, failed or cyclic dependencies skip only affected mods, with a logged reason.
- Unit/layout checks: API 8/8 passed, host 12/12 passed, SDK and host strict `cargo clippy -D warnings` passed, provider and consumer fixtures compiled and passed Clippy, all relevant rustfmt checks passed.
- Two independent native fixture mods in `tests/fixtures/abi-services/`, one providing `abi_provider:echo` v1 and one requiring it from `Mod::start`. Consumer's filename sorts before provider. `tests/fixtures/abi-services/verify.ps1` covers successfully resolving and invoking it (35 + 7 = 42), a fresh server restart, rejecting a missing provider, and rejecting a two-mod dependency cycle. The provider fixture also checks rejection of duplicate and foreign-namespace services, while consumer checks version 2 is unavailable. All checks passed on WS-0 port 26000. Scratch worlds were removed.
- Existing Create 26.3 `tests/scenarios/p1a-create.txt` and the new `tests/scenarios/create-press.txt` passed with the latest Create release package built on this branch. No original Java mod differential fixture was run for the shared ABI.
- The service table enables Create's future `create:kinetics` service. WS-2 and WS-4 should agree on the exact table fields and version before publishing; mod-specific logic stays in `mods/create`.

## M3: Comparator output from custom blocks

- Next milestone. Requires engine hook review, redstone signal propagation and a deterministic block/comparator scenario, not a cached mod-only approximation.

## M3: Custom content comparator output (verified behavior)

- Append-only ABI 1: `moddedmc:content` adds `add_analog_output(Reg, block_id)`
  at 64-bit byte offset 32 (table size 40); `moddedmc:block_world` adds
  `set_analog_output(WorldRef, Pos, strength)` at byte offset 128 (size 136).
  `WorldExtension::AnalogOutput` checks both table sizes at `Mod::start`.
- The content registration flag is scoped to the current mod's registered block.
  Signals are 0..15 and stored in the custom block entity's namespaced NBT,
  not process-global temporary state. Existing Pumpkin custom-data write-through
  and block-entity removal handle persistence and cleanup. The host uses the
  registered content *block id*, not its block-state id, for capability lookup.
- Pumpkin's native comparator calculates signals using the registered block's
  `get_comparator_output` even if its physical state lacks a vanilla analog flag.
  Vanilla analog blocks still use their existing behavior; the no-output
  vanilla fast path remains. Direct and through-solid reads are covered.
- `tests/fixtures/abi-analog` compiles a separate native .mmc. Its command
  fixture rejects duplicate and foreign registrations. The acceptance scenario
  `tests/scenarios/abi-analog.txt` passed on WS-0 port 26000:
  direct 0→7→0→7, through-solid 13, both 7 and 13 after a clean server
  restart, strength 16 rejected, absent block rejected, breaking/removing
  and replacing the source resets output to 0. All reads were actual vanilla
  comparator `OutputSignal` NBT values, not only host return values.
- `tests/scenarios/abi-comparator-vanilla.txt` passed the composter
  6→7 control. An A/B run on previous published host `ad86424`
  and the M3 build obtained identical vanilla outputs 6, then 7
  (coordination note `notes/WS-0-comparator-vanilla.md`).
- These tests establish the specified host interface behavior and sampled
  vanilla comparator continuity. They are not a full Minecraft 26.3 Java
  differential suite or a whole-mod parity claim. No controlled performance
  benchmark was run for comparator-heavy redstone builds.

## M4 — borrowed content World dimension identity (WS-3-04)

- Added append-only `BlockWorldV1.dimension(WorldRef) -> Str` at offset 136, table size 144 (64-bit). Host returns `World.dimension.minecraft_name` for the valid borrowed world and an empty string for null; no process-local pointer is used as a persistent key.
- SDK `WorldExtension::Dimension` size-guards the new function, and `World::dimension()` yields the canonical namespaced key. `ABI_VERSION` remains 1, prior fields/offsets unchanged; an older host cannot satisfy the new requirement.
- Built pack-dev, checked `moddedmc-host`, 8/8 `moddedmc-api` tests, fmt, API strict Clippy and host strict Clippy. Live `abi_analog_dimension` fixture returned `minecraft:overworld` before and after a clean restart, and the existing native comparator and vanilla composter fixtures passed (port 26000). Nether/End, independent save isolation and Java oracle comparisons remain untested, so this verifies the tested surface, not full multi-dimension railway parity.


## WS-0 M4 planning session (2026-11-20, noelalex-6d3625)

**Status: committed to GitHub, coordinator lease still needs release.** The authorized remote laptop terminal hit its monthly execution limit after the job was claimed, so the coordinator submit/release command could not be executed this session. Do not mistake this text for a release.

- **New plan:** `docs/MENUS-PLAN.md`, commit `a59bfb066748beb1ffa5261dd3994275acd7dbef`. This is an actionable design for a generic 26.3 inventory-menu provider with mod-owned storage, host-owned player inventory/cursor, authoritative vanilla click behavior, stale revision rejection, atomic moves, viewer isolation and session close/restart behavior. It deliberately does **not** freeze a C-ABI table or claim implemented menus.
- **Current implementation still unchanged:** previous ABI branch `a00f35ebc162e865a02e7bef57ede3467f00bcd6` holds comparator output and borrowed-world dimension (awaiting review at the time of this session); nested `devmogged/Pumpkin` `ws/abi` remained `51ac3a12806884666fda1723f196104d6d77af07`. No API, host or engine source changed in this session.
- **Evidence:** read AGENTS.md, engine AGENTS.md, project coordination and architecture plans, current `api/src/{abi,lib,containers,stacks}.rs`, host `{lib,containers,interaction}.rs`, and coordination issue #61; the menu plan distinguishes the existing dialogue/automation APIs from genuine per-player menus. Doc-only work: no compilation, gameplay scenario, live Java oracle or benchmark was run. Do not claim parity.
- **Coordination request #47:** its body already contains a detailed WS-0 response establishing generic `moddedmc:services` support and reserving `create:kinetics` for Create's own mod. It has not been acknowledged by this session through `coord.mjs answer`; do not mark it answered or closed until the coordinator action succeeds. Next agent should coordinate with Create owner on dynamic stress callback semantics.
- **Next implementation priority:** request #39, native default food and consumable component override. Unify #40/#63/#69 physical-property requests. Use `docs/MENUS-PLAN.md` Stage A: inspect vanilla 26.3 `AbstractContainerMenu` and Pumpkin packet handlers, and write the independent failing fixture before public ABI publication. Recheck review verdict for the earlier dimension implementation.
- **Laptop checkout:** `N:\mmc-wt\abi` was clean at `a00f35e` when checked. This log and menu plan were committed through GitHub rather than the laptop shell. The laptop therefore needs `git -C 'N:\mmc-wt\abi' pull --ff-only origin ws/abi` before a release handoff can point at this log. No other local changes were introduced.
- **Coordinator pending:** session `noelalex-6d3625`. Run `node N:\mmc-coord\coord.mjs beat --session noelalex-6d3625 --note 'Published native menu transaction plan on ws/abi; documentation-only milestone'`, optionally submit this documentation milestone for review, then **release** with `node N:\mmc-coord\coord.mjs release --session noelalex-6d3625 --state free --handoff 'N:\mmc-wt\abi\docs\ws\abi.md'`. No claim that these coordinator commands succeeded has been made.

