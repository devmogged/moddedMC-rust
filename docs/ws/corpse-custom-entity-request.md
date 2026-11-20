# WS-12 to WS-0 — Nonliving persistent entity and item custody

## Source-backed need

Corpse 1.1.22+26.3, CorpseEntity.java at upstream commit 12881669fb0ef95e0a863a524bc92674cc2f5608, uses a persistent nonliving entity with 2-by-0.5 collision dimensions, custom synced owner UUID/name/model/skeleton/equipment, owner/admin-authorized interaction, and NBT fields Death, Age and optional EmptyAge.

The existing moddedmc:mobs interface registers only living entity carriers. Existing moddedmc:entities can inspect or manipulate entities the host already knows; it does not register an arbitrary nonliving entity, tie mod NBT to the entity's save lifecycle, or expose the original removal reason. A fake living mob or block is not an equivalent implementation.

## Requested generic append-only interface

Provide a versioned size-tagged moddedmc:custom_entities v1 C ABI, independent of internal Pumpkin Rust types, that supports registration and loaded-world spawning of nonliving entity types, dimensions/tracking/carrier rendering metadata, owned named-root saved NBT with bounded reads and writes, server-authoritative entity-use callbacks with borrowed player/world references, and server-to-client state synchronization.

Expose terminal removal reason distinctly from chunk unload and server restart. The custody of player items must be persistent and at-most-once and integrate with the separately accepted moddedmc:player_death issue #73; suppress vanilla drops only after host-verified durable corpse custody. Ordinary chunk unload must never scatter items. Discard/terminal removal must scatter each retained stack once.

## Required tests

Mixed main/armor/offhand death with exact conservation; two players dying in the same tick without crossing identities; save/restart/reload preserving the same corpse and contents; chunk unload causing no drops; owner/admin/skeleton permissions; forced timeout scattering once; failed spawn or corrupt NBT falling back to vanilla drops. Compare against Corpse's Minecraft 26.3 NeoForge runtime, noting safety departures when needed.

**Request status:** specification prepared, not yet filed through coordinator; no host implementation known.
