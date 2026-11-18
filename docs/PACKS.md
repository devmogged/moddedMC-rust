# Packs

A pack is a list of mods plus settings. The pack builder turns it into one server binary per target, with Pumpkin and the mods linked together. The app edits packs and calls the builder, but the builder is also a plain CLI so agents and CI can use it.

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

1. Resolves every mod to a crate: the index in `devmogged/mods` or `devmogged/mods-private`, a git rev, or a local path.
2. Generates a build crate in `target/packs/<id>/`: a `Cargo.toml` that depends on `pumpkin`, `moddedmc-host` and each mod, plus a `main.rs` like `packs/dev/src/main.rs`. It's generated and never committed.
3. Builds it with the release profile for each target. Windows builds natively. The Pi target uses `cargo zigbuild --target aarch64-unknown-linux-gnu`, so a laptop builds for the Pi and the Pi never compiles anything.
4. Writes `target/packs/<id>/<target>/` with the binary, a `pack.lock` (exact mod versions and git revs, the engine commit, the builder version) and the pack's client assets (once the companion exists).
5. Caches by the hash of `pack.lock` and target, so an unchanged pack is not rebuilt.

## Changing a pack mid-world

- **Adding a mod**: rebuild and restart. The world keeps working; new content shows up in new and existing chunks as the mod places it.
- **Removing a mod**: its blocks, items and entities are removed when chunks load (Forge behavior). The app takes a snapshot before every rebuild, so a removal can be undone by restoring it.
- Registries are saved per world (`moddedmc/registry.dat`: namespaced id to numeric id), so ids never shift when the mod list changes.
