# Create: Central Kitchen — unpacking model (incomplete)

Source basis: **Create: Central Kitchen 2.6.2b**, NeoForge Minecraft 1.21.1 by DragonsPlus, released under LGPL-3.0-or-later. The complete original license text is copied to `LICENSE.md`. Reference: `D:/mc-ref/central-kitchen-latest/src/main/java/plus/dragons/createcentralkitchen/common/packager/RecipeUnpacking.java` and compiled methods in the published `create-central-kitchen__2.6.2b.jar`.

The independent `src/recipe_unpacking.rs` ports the full planning order for packages and 1–9-slot inventory layouts:

- Combine equal item/component identities safely, reject overflowing packages and invalid inventory windows.
- Apply package-order crafting layouts when valid, then match occupied storage slots with consistent servings.
- Preserve the one-item-per-slot package layout before attempting bulk batching.
- Match overlapping ingredient choices via source-style subset-memoized backtracking (maximum 512 masks for nine slots).
- Reject a recipe or a full slot without trying an alternative that would violate source order.
- Snapshot every slot; simulate all insertions before any real slot is written; reject stale snapshots and incomplete simulated inserts.

`ItemHandler` mirrors NeoForge's required item handler operations. Callers supply canonical item components and the already-resolved `Ingredient::test` match sets. A runtime mod must obtain those from actual recipe and inventory APIs; arbitrary component-sensitive predicate behavior, recipe reload timing, and NeoForge handler side effects are **not** exercised by the pure Rust tests.

Verification:

```powershell
cargo fmt --package create-central-kitchen -- --check
cargo test -p create-central-kitchen
cargo clippy -p create-central-kitchen --all-targets -- -D warnings
```

Nine deterministic source behavior tests currently pass. This crate is deliberately an `rlib` and **not a runnable `.mmc`**, because neither Create's packager service nor Farmer's Delight cooking and other relevant item handlers are registered through the host. In-game scenarios, save/restart, original NeoForge differential oracle, and real-client UI tests remain outstanding.
