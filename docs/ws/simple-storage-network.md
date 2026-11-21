# WS-20 Simple Storage Network implementation plan and M0 survey draft

Goal: Port the Simple Storage Network 1.13.2 gameplay to the Rust server for Java 26.3 as a drop-in mod. Workstream #19 specifies mods/simple-storage-network on ws/simple-storage-network.

## Release evidence

- Latest official release found on both Modrinth and CurseForge: NeoForge 26.1.2-1.13.2, published August 23, 2026. Reference the release JAR rather than the old buja copy or the third-party 26.3 beta.
- Local reference JAR verified against Modrinth SHA-1 808a4080b885ea44b6a9716c4ea9482bc9a95354. JAR metadata declares storagenetwork 1.13.2, NeoForge >=26.1.2.78, Minecraft >=26.1.2, FLIB >=0.2.10, on both sides.
- Upstream Windows-independent reference: https://github.com/Lothrazar/Storage-Network/tree/2eef5d842cacbe04b0765331bd3a83b6a5ae4dc9 . This is the inspected 26.1.2 branch head, not a confirmed reproducible build of the JAR.
- Upstream branch LICENSE is MIT (Copyright 2026 Lothrazar); JAR metadata points to GitHub. Retain the MIT notice where required. Did not establish asset provenance separately. Keep original assets in a local reference JAR pending confirmation.
- archive-files.txt has the complete listing. release-inventory.txt is a filtered index of relevant entries.

## Release inventory and host mapping

- 16 blockstate definitions. Root (master), terminals (inventory, request, request_expanded), seven cable types (kabel, storage_kabel, filter_kabel, import_kabel, import_filter_kabel, export_kabel, process_kabel), and collector, cradle, drawer, exchange and receiver. Existing: moddedmc:content add_block supports IDs, states, physics templates, shapes and block entity IDs. Verify connected shapes and placement rules on the original before registering them.
- 31 item model definitions including the 16 block items, six remotes (builder, collector, crafting, expanded, inventory, picker), book, and eight upgrades (chunkload, operation, single, slow, speed, stack, stock, void). Existing: moddedmc:items add_item. Check actual item registration, defaults, stack limits and carried link data against registry source before treating model filenames as the final registry.
- 31 recipes, 16 loot tables and one tag JSON. Confirm type, recipe count and edition differences before translating data paths/ingredients. Existing: mod data loading. Observe crafted inputs and outputs on the Java release.
- Network root, topology, facing and cache: retain stable positions and exact upstream IDs; inspect master discovery, two roots joining, unloaded chunks, refresh and split/merge semantics. Existing: content placement/state and mod-owned block entity data; saved_data if the release requires persistent network-wide state. Avoid assembling persisted inventories as cached authoritative copies.
- Connected vanilla chests and sided automation: existing moddedmc:containers slots, stack, slot_limit, simulate/commit insert, insert_stacked and extract. Existing moddedmc:item_stacks for saved NBT and stack metadata. Cross-container atomic transfers and modded endpoint parity still need ABI review. Do not mutate the source based only on simulated destination capacity without a verified recovery path.
- Filter, cable process, collector, cradle, expanded inventory, crafting terminal and receiver screens are present. Missing from current public ABI: mod-owned interactive menus (WS-0 M4). The stock-client search UI, crafting interaction and mod packet compatibility also need a WS-8 decision. A chest UI alone cannot establish Java UI parity.
- 15 top-level Message classes for insert/request/recipe, recipe clearing, filter refresh, cable settings/IO/limits/process, collector and Curio keybinds, sort, settings sync, and stack refresh/response. Verify payload direction and server-side authorization from pinned implementation. Never accept client-supplied inventory contents or trust remote link coordinates without validation.
- Review upstream config keys and defaults and confirm whether entity, worldgen, or command registrations exist before closing M0. Class and data path indexes are evidence of archive contents, not runtime registration.

## Implementation order and verification

1. Close M0 after checking pinned registry and config source against the release and running an exact NeoForge 26.1.2 oracle with matching FLIB. Record each host capability and send specific gaps through the coordinator once leased.
2. Scaffold using scripts/new-mod.ps1. Start M1 with one measured passive content slice (base cable, matching item and its vanilla recipe and self-drop). Add a scenario first that checks the original counts, placement, restart and break, and observe failure before implementation. Once green, add the remaining simple registrations and recipes. Package with scripts/pack-mod.ps1 and run in the assigned sandbox ports.
3. M2: test loaded topology, reconnect/restart and one shared chest reached by two cables before enabling transfers. Validate no duplication, full-destination conservation, sided rules, item component preservation, and filter behavior. Compare saved block entities and inventories on both servers.
4. M2/M3: terminal search and crafting, filters and import/export, linked remotes, processing, collector and specialized storage, upgrades and config/packet behavior. Complete one observable feature per verified commit and record Java comparison and client limitations. Leave the current Create playground separate from the workstream.

## Base cable release findings

- The shipped ID is storagenetwork:kabel. The shaped recipe outputs eight cables. Pattern: sss / i i / sss, using six minecraft:stone_slab and two items from c:ingots/iron. Pumpkin ships the conventional tag; verify the effective tag when running the recipe test.
- The shipped block loot is one storagenetwork:kabel subject to survives_explosion. Measure tool behavior separately.
- The JAR blockstate is multipart across all six faces. Definitions distinguish none, blocked, cable and inventory and share the same model for cable/inventory. The shipped asset carries a separate InsomniaKitten copyright credit, so the initial code slice should not ship this asset. Confirm upstream gameplay transitions separately before promising geometry or connectivity parity.
- ws20-base-cable.txt is a draft scenario for the first slice. It unlocks the recipe but does not craft it; verify the eight output through the original Java client and a Rust client craft check or a recipe matching assertion. It checks loot after a restart.

- Bytecode inspection confirms BlockCable implements SimpleWaterloggedBlock, uses six directional enum properties plus waterlogged, creates a block entity, and updates shape on neighbor changes. Keep these as explicit follow-up behavior for a placement-only first slice. The nominal state product is 4^6 × 2 before checking which combinations are reachable. Do not reserve one stock-client carrier per possible state; consult the carrier sharing and resource-pack limits before adding connected variants.
- Baseline on the previous Lucius engine: ws20-base-cable-baseline.txt failed exactly at the missing registration assertion (exit 1). See F:/mmc/moddedMC-rust/run/ws20-baseline-a0aefdb7/scenario.log. The workstream scenario is tests/scenarios/simple-storage-network-cable.txt.
