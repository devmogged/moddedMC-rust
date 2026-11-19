# Packs

A pack is a list of mods plus settings. The pack builder turns it into a stock server binary for each target and a set of drop-in mod files. The app edits packs and calls the builder, but the builder is also a plain CLI so agents and CI can use it.

## pack.toml

```toml
[pack]
id = "buja"
name = "Buja"
version = "3"                     # bump on any change; worlds record which pack version wrote them

[mods]
# id = version requirement and source. Sources: "mods" (devmogged/mods), "private"
# (devmogged/mods-private), { git = "...", rev = "..." }, or { path = "..." } for local work.
create = { version = "0.1", source = "mods" }
saints-dragons = { version = "0.1", source = "private" }
wayshrine = { path = "../../mods/wayshrine" }

[targets]
laptop = "x86_64-pc-windows-msvc"
pi = "aarch64-unknown-linux-gnu"

[server]                          # written into the server's config on deploy
view-distance = 10
simulation-distance = 8
online-mode = false
```

## What the builder does

```powershell
cargo run -p moddedmc-pack -- build packs/example/pack.toml [--target pi] [--profile release]
```

Working today: `path` mods (Rust `cdylib` crates) and prebuilt files (`{ mmc = "file.mmc" }`). The `mods`, `private` and `git` sources fail with a clear error until the mod index has entries. Targets other than the build machine go through `cargo zigbuild`, which needs `zig` on PATH and the target installed (`rustup target add aarch64-unknown-linux-gnu`). Every target gets `pack-<id>[.exe]`, even a pack that only uses the stock server, so agents always run the same name.

1. Builds each local Rust mod and packages it as a `.mmc`, or copies the requested prebuilt file.
2. Builds the stock server for the target and profile. Use `pack` for local testing and `release` for deploys.
3. Writes the binary, `mods/` and `pack.lock` to `target/packs/<id>/<target>/`. The lock records the mod versions and hashes, engine commit and builder version.
4. Caches the output by the hash of the lock and target.

## Changing a pack mid-world

- **Adding a mod.** Drop its `.mmc` into `mods/` and restart. The world keeps working; new content shows up in new and existing chunks as the mod places it.
- **Removing a mod.** Remove its `.mmc` and restart. The planned F4 registries clear the removed content when chunks load.
- Registries are saved per world (`moddedmc/registry.dat`: namespaced id to numeric id), so ids never shift when the mod list changes.
