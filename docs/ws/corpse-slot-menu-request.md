# WS-12 to WS-0 — Transactional player slots and corpse inventory menus

## Source-backed need

Corpse 1.1.22+26.3 CorpseInventoryContainer.java displays 41 corpse slots in this order: armor 3, 2, 1, 0; offhand 0; main inventory 9..35; hotbar 0..8; followed by player inventory. Extra items occupy separate, pageable inventories. Restoring original slots displaces player-held stacks into overflow rather than overwriting or destroying them. History menus can be read-only and administrative recovery is permission-gated.

The current Player::show_menu button actions and moddedmc:containers world-block inventories cannot safely provide player slot reads/writes, component-preserving transfers, shift-click rules or server-authoritative atomic item swaps.

## Requested generic append-only interface

Provide size-tagged moddedmc:player_inventory and/or moddedmc:slot_menus tables with stable slot namespaces (36 main, 4 armor, 1 offhand, additional registered slots), bounded complete Java named-root ItemStack NBT including components, and borrowed lifetime-scoped player/world/menu references.

Read authoritative player and corpse slots; stage a batch of slot replacements with expected versions and capacities; atomically commit all or none on the owning world thread. Expose menu slot definitions, per-slot edit/read-only permissions, validated click/drag/shift-click callbacks, paging, disconnection, and an inventory-insert operation that returns unaccepted leftovers for durable corpse overflow. Require owner/admin authorization to act, and enforce history restrictions before any item transfer.

## Required tests

Exact 41-slot mapping; direct restore to empty player slots; occupied main, armor and offhand displaced with no loss; partial inventory and full-inventory overflow; double-click/retry/disconnect/restart without duplication; nonowner denial and skeleton exception; history read-only behavior; stock Java 26.3 interaction tests and original NeoForge differential outcomes.

**Request status:** specification prepared, not yet filed through coordinator; no host implementation known.
