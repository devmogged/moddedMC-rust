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

- Pending for review and implementation: `WS-2-01` fluid handlers, `WS-2-02` fluid-state operations, `WS-3-01` save-global data, `WS-5-01..08` combat/items/projectiles/boss bars/spawning/structures and additional requests. Verify against the available baseline interfaces before accepting a design. Implement with append-only guards, documentation and targeted scenarios, then republish binaries.
- No M1 code or parity checks completed yet.

## M2: Inter-mod services

- Pending. The host currently calls `Mod::start` while loading a library, before `Mod::register` stages content. Services registered during `register` cannot be required from `start` without an explicit two-phase lifecycle and dependency ordering design. Keep the ABI append-only.
