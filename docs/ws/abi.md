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
