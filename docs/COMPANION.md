# Client companion

The companion is a required Fabric mod for joining moddedMC servers with loaded mods, on Java Edition 26.3. Its own code is MIT OR Apache-2.0. The same JAR works with every pack. Mods supply data through the server's existing content interfaces.

The companion implements shared client capabilities. A pack supplies rig descriptors, assets and state for the mods installed on that server. The animated display descriptor below is the existing example. A creative item catalog or new control bindings would also use shared code and per-pack definitions. A feature that needs its own client code can use an optional add-on.

For Saint's Dragons, the server runs the Rust `.mmc` and players use our 26.3 companion. The upstream 0.9.88 Fabric JAR targets Minecraft 1.20.1 and expects its Java-side registries and networking.

## Build

With JDK 25 or newer, run `./gradlew build` from `companion/` (`gradlew.bat build` on Windows). Use `build/libs/moddedmc-companion-0.1.0.jar`. The JAR includes the Fabric networking modules it needs. Fabric Loader 0.20.1 and Minecraft 26.3 come from the launcher.

## Configuration negotiation

The Java configuration custom payload channel is `moddedmc:negotiate`. Integers are unsigned and big-endian. The host sends an empty challenge followed by a vanilla configuration ping. A companion replies to the challenge with a two-byte protocol version and four-byte capability mask. The server answers with its version and the intersection of client capabilities and features used by the running pack. Malformed hello lengths and incompatible protocol versions are ignored.

| Protocol 1 flag | Meaning and extra bytes in the server answer |
| --- | --- |
| `0x00000001` | Hashed assets. Follow the six-byte header with 32 raw SHA-256 bytes for the manifest and 20 raw SHA-1 bytes tying the manifest to the vanilla ZIP offer. |
| `0x00000002` | Player actions. No extra configuration bytes; the companion requests the versioned key catalog in play. See [the ABI and wire format](MOD-ABI.md#player-actions-moddedmcplayer_actions-v1). |
| `0x00000004` | Moving block use. No extra configuration bytes. The companion uses the verified trapdoor catalog and the player-action session in play. |

The ping orders the exchange before the pack offer or known packs. When any mod is loaded, a client that has not supplied a valid hello by the ping's reply is disconnected with installation instructions. Bedrock clients cannot complete the Java exchange and are also disconnected. Servers with no loaded mods still permit vanilla clients. On servers without a challenge the companion sends nothing. Every connection currently uses the stock block, item and display representation. The headless scenario bot responds to the same challenge by default; set `MODDEDMC_BOT_COMPANION=0` to test a vanilla client, or set a nonzero protocol version to require a negotiation response.

## Asset delivery

The host uses the generated stock pack's file list. A manifest contains `pack_version` (the stock ZIP's SHA-256) and `assets` (objects with `path`, `sha256` and `size` in bytes). Hashes in JSON and URLs are lowercase hex. Only referenced files from the generated pack are served. Create textures are read from the owner's local JAR at startup.

The game status JSON advertises `moddedmc: {"protocol": 1, "pack_url": "...", "manifest_sha256": "..."}` when a catalog exists. Replace `{host}` in the URL with the game handshake host, bracketed for IPv6. Fetch `manifest-<sha256>.json` and `assets/<sha256>` relative to the ZIP URL. All three routes use the same pack listener. When `MODDEDMC_PACK_URL` points to a proxy, forward the manifest and blob routes alongside the ZIP. The status request and pack routes need no device-agent access.

The companion caches `<gameDir>/moddedmc/manifest-<sha256>.json` and `<gameDir>/moddedmc/assets/<sha256>`. It verifies hashes and lengths before using cache entries or publishing replacements. The manifest is limited to 2 MiB and 10,000 paths. Each asset is limited to 8 MiB and the total to 256 MiB. The companion uses verified blobs to assemble the pack locally on vanilla's pack download worker, after vanilla's pack consent. An invalid or missing blob that cannot be repaired fails the required pack load.

## Pack inventory screens (version 1)

A mod opens an existing managed inventory menu with the title
`moddedmc:screen/<namespace>:<id>`. The companion recognizes this only on a negotiated
connection and loads `assets/<namespace>/moddedmc/screens.json` from the accepted pack.
The host includes this catalog for registered content namespaces and copies the
referenced PNG from the mod assets (or the owner's local release JAR). Missing
or invalid layouts close the marked menu with a client message.

Each catalog entry is keyed by the screen path and declares:

- `version` (1), `title`, `width`, `height`, `texture` (namespace-relative PNG path),
  `textureWidth`, `textureHeight`; the panel is centered and the atlas starts at (0, 0).
- `containerSlots` (9–54, multiple of nine), `stateSlot` (a reserved,
  non-transferable slot), `slots` (objects with native `index`, `x`, `y`, optional
  `hideWhen` state key). Unlisted container slots are hidden. The existing 36
  player slots follow the declared `inventory` origin (`x`, `y`, `hotbarY`),
  with 18-pixel spacing.
- `layers` (atlas source `u`, `v`, destination `x`, `y`, `width`, `height`).
  Optional `visibleWhen` gates a layer; optional `fill` clips it from the bottom
  to a state value measured in pixels.
- `buttons` (same rectangles, plus `hoverV`, `pressedV`, `slot`, `label` and
  optional `enabledWhen`). Each is backed by a reserved native menu control slot.
  Left-click goes through the existing pickup action; the server remains authoritative.
- `labels` (literal `text`, `x`, `y`, ARGB integer `color`). All coordinates
  are relative to the panel.

State is an NBT compound named `moddedmc:screen_state` in the
reserved stack's `minecraft:custom_data`. Missing keys read as zero. A managed
menu can refresh that stack using its existing shared inventory mutation API.
No ABI fields, extra packets, custom client item registries or mod-specific Java
code are needed. The client retains native slot IDs and inventory handling.
The server must deny ordinary transfer of all unlisted slots and control stacks,
and validate every action independently of presentation state.

An optional `preview` declares a rig item model, clipped rectangle and pixel
scale. Its state supplies the live carrier's integer `entity` ID and hex
`uuid` (without hyphens); both must match the loaded entity. Optional
`vitals` entries define health and armor atlas rectangles and value positions.
Health follows the carrier and armor follows the first four equipment slots.
The reserved preview marker must not be transferable or persisted as equipment.

The Saint's Dragons Crucible uses the 0.9.88 GUI atlas and coordinates, including
the charge gauge, processing panel, progress strips, locked grid and control
button. Package using `scripts/pack-saints-dragons.ps1` with the released JAR.
## Standalone pack screens (version 2)

Version 2 entries in the same screen catalog declare a background, named
atlases, tab hitboxes, scrollable lists, state-bound labels/layers, paged
text with section links, optional text input, and server-action controls.
The Codex uses the released 393×214 book and tab/icon textures; its ally
controls accept a player name, and refresh and removal use server-owned records.
Optional `previews`
entries declare a selected-row field and a clipped rectangle (`x`, `y`,
`width`, `height`, optional `tab`). The field contains a rig item-model ID
(`rig`), a named rig `texture`, pixel `scale`, and `offsetX`/`offsetY`.
The selected row ID may be a UUID without hyphens: a loaded matching carrier
uses its display clip, otherwise the preview uses a static pose. Preview
animation and rider-seat sampling are isolated from world rendering.
Layers can supply `frames` (atlas names) and `frameTicks` to cycle their
textures. Paged section objects can expose item IDs under the optional pages
`itemField`; the client draws vanilla or server Creative catalog stacks and
their display names.

An optional `dialogue` descriptor defines a text field, delay and character
interval in milliseconds, slide duration, optional voice sound ID and interval,
and speaker entity field and camera duration. The client reveals Unicode code
points on its monotonic clock, gates choices and input until the line is shown,
and lets a click or advance key skip the reveal. The optional arrow atlas
rectangle and normal/ellipsis fade timings replace single continuation choices.
The server's token and action checks are
unchanged.

## Pack wave overlays (version 3)

A version 3 entry in the screen catalog declares `titlePrefix`, optional
`titleKey` for a translated title with the remaining suffix as its argument, a namespace
relative `texture`, `width`, `height`, `textureHeight`, `fillV`, and a
per-game-tick `smoothing` coefficient. On negotiated connections a matching
leased vanilla boss bar is drawn using the pack atlas and interpolated progress.
Removing the lease decays its progress to zero; other boss bars retain their
vanilla rendering. The Saint's Dragons pack stages the released swarm bar.

The optional play channel `moddedmc:pack_screen` uses a two-byte version/opcode
prefix: `[1,0]` advertises support; `[1,1]` opens a screen with UTF-8 JSON
`{id,state,token,replyTo}`; `[1,2]` submits `{token,action,text}`.
The host accepts at most 256 KiB of open state and 1 KiB per click. A click
must match the connection's current one-use token and server-provided action
allowlist; text is restricted to 64 slash-free ASCII characters. Replies identify
the preceding token so a closed or replaced screen is not reopened.
The mod uses the existing menu callback ABI: a reserved
`moddedmc:pack_screen` button carries JSON `{id,state,actions}` in its
tooltip, where action IDs map to booleans indicating whether text is accepted.
The host removes this marker from fallback dialogs. Validated text is appended
to the action ID after a slash; the receiving mod must still check ownership,
input semantics, and live state. No ABI fields or inventory slots are added.

## Owner play test

1. Start a scratch pack server with `create.mmc` and your local Create JAR. Install the companion JAR into a Fabric 26.3 instance named **moddedMC** in your launcher.
2. Open that launcher. Select **moddedMC** and press **Play**. Open **Multiplayer** and add the server. Join it and accept the required pack if asked. Expect Create items and placed gears to appear. Check `logs/latest.log` in the instance for `active capabilities 3` and `Loaded verified moddedMC asset pack`.
3. Disconnect. Keep the game port reachable and block only the pack HTTP port from the test client. Rejoin from the prepared instance. Expect the same models and verified asset log using only the existing cache. Restore the pack HTTP route afterward.
4. Join the same server from a stock 26.3 installation. Expect a disconnect naming the moddedMC companion and how to obtain it. Then join a moddedMC server with no loaded mods from the stock installation and expect a normal join. Join an ordinary 26.3 server from **moddedMC** and expect a normal join with no companion negotiation log for that connection.

## Animated displays

With [GeckoLib 5.5.7](https://modrinth.com/mod/geckolib/version/5.5.7) and Fabric API installed alongside the companion, an item display can render a textured, animated GeckoLib rig. The display's position, transform and light still come from Minecraft. Each display has its own animation playback.

For an item model ID `example:hinge`, add `assets/example/moddedmc/rigs/hinge.json` to a client resource pack.

```json
{
  "geometry": "example:hinge",
  "animations": "example:hinge",
  "texture": "example:textures/entity/hinge.png",
  "clip": "animation.hinge.swing"
}
```

The geometry and animation IDs resolve to `assets/example/geckolib/models/hinge.geo.json` and `assets/example/geckolib/animations/hinge.animation.json`. The texture uses its full resource path. The default clip comes from the descriptor. A server can select another clip in the same animation file by setting the string `moddedmc:rig_clip` in the display item's `minecraft:custom_data`. An unknown clip uses the default. Without GeckoLib or a valid rig, the client renders the regular item model.

Optional `scale`, `y_offset` and `yaw_offset` fields adjust the rig relative to the item display. `transition_ticks` (0–20, default 3) controls the animation controller's blending duration. `hide_parts` lists item model IDs used only by the multipart stock model. The companion suppresses those IDs while the replacement rig's resources are available.

An optional `textures` object maps names to full texture resource IDs. The server can set `moddedmc:rig_texture` in the display stack's `minecraft:custom_data` to select one of those names. An unknown name or unavailable texture uses the descriptor's default `texture`.

Optional `preview_geometry` references a separate posed GeckoLib geometry for
an unloaded or stored rig in a standalone screen. The pack includes this
geometry with the ordinary rig resources.

An optional `rider_seats` array describes passenger seats in passenger-list order. Each entry requires a `bone` name; optional `x`, `y`, `z` are bone-local offsets in model pixels, `height` is an additional world-space vertical offset in blocks, and `max_age` limits a sample's age in client ticks (default: unlimited). The server associates the body display with its living carrier by setting the integer `moddedmc:rig_mount` in the display stack's `minecraft:custom_data` to the carrier's network entity ID. Missing or negative IDs, missing bones, unloaded displays and expired samples leave normal passenger positioning intact. The companion samples the animated bone with the display's actual transform and updates both rider positioning and interpolated rendering; server mounting and collision remain authoritative.

An optional `report_bone` names an animated bone whose world position is sent by the local first rider at most once per client tick. The negotiated player-actions channel uses version 1, opcode 6, signed big-endian entity ID, one-byte ASCII name length (1–64), name, then three big-endian doubles. The server validates the reporting player, tracked carrier, rate, coordinates and freshness before exposing the position to mods; the mod supplies the fallback and any tighter bounds. Reports do not move entities by themselves.

An `.mmc` can supply the descriptor and its referenced files under `assets/`. The host finds rigs for registered standalone item model IDs and adds the descriptor and its geometry, animations and texture to the generated server resource pack. This uses the same pack acceptance and hashed asset cache as stock models.

To exercise the renderer, enable the [test resource pack](../companion/src/test/resources/rig-pack) on a 26.3 Fabric client with the companion and GeckoLib. Run the following in a world with commands enabled.

```mcfunction
summon minecraft:item_display ~ ~ ~2 {Tags:["rig_test"],item:{id:"minecraft:paper",count:1,components:{"minecraft:item_model":"rig_test:hinge"}}}
data merge entity @e[type=minecraft:item_display,tag=rig_test,limit=1] {item:{id:"minecraft:paper",count:1,components:{"minecraft:item_model":"rig_test:hinge","minecraft:custom_data":{"moddedmc:rig_clip":"animation.hinge.park"}}}}
```

The first command shows a swinging arm and the second parks it upright. The item model is a gold block on a stock client with the test pack enabled.

### Procedural arc displays

An item display can also supply `moddedmc:arc` in its item's `minecraft:custom_data`.
This is a compound with float `x`, `y`, `z` (end minus start in blocks; the display
is placed at the midpoint), integer `lifetime` (1–200 client ticks), integer
`seed`, float `scale` (0.1–2), and numeric `grow` (zero for flicker/fade,
nonzero for grow/flash/retract). The `layers` list contains up to four lists
of seven floats in [0,1]: scale multiplier, width factor, jitter factor, red,
green, blue, alpha. The companion renders seven seeded segments per layer as
crossed lightning ribbons. The normal item is hidden and the server removes
the transient display at expiration. This path does not require GeckoLib.

The same display path accepts `moddedmc:ribbon`, with float segment offsets
`x`, `y`, `z`, integer `lifetime` (1–200), numeric `flat` (one plane
instead of two crossed planes), and up to four layer compounds. Each layer
specifies a full resource texture prefix without `.png`, integer `frames`
(1–64; numbered from zero when greater than one), float half-width `width`
in blocks (0–32), float `alpha`, optional float RGB tint `r`, `g`, `b`
in [0,1], and integer `repeats` (1–64). Textures are distributed through
the registered item's model dependencies. The server owns the display lease.

For a GeckoLib body rig, numeric `moddedmc:rig_hidden` in custom data
suppresses the body geometry while retaining animated rider-seat sampling.

## Item display parts

A mod can put an object mapping part paths to block or item models in `assets/<namespace>/moddedmc/display_parts.json`. For example, `{"hinge/arm": "example:block/hinge_arm"}` supplies the item model `example:moddedmc/parts/hinge/arm`. Put that ID in a vanilla item's `minecraft:item_model` component and show it with the existing item display interface. The generated resource pack includes the referenced model and textures. This works in stock clients and requires no additional registry entries.

## Scaffolding shapes

The verified pack can include `assets/moddedmc/moddedmc/scaffolding.json`. Each wire state ID maps to a bottom flag, the placed block item's model identifier, and an optional map of base texture identifiers to connected texture sheet sizes. The connected texture is the base identifier with `_connected` appended. Sheets with two tiles per side apply to horizontal faces and eight-tile sheets apply to vertical faces. The host uses dedicated trapdoor states for this content. The companion uses the flag to select the standing collision and outline shapes, and the model identifier to show the full placement outline when the player holds the matching block. The mapping belongs to the current connection and is released at disconnect.

## Trapdoor shapes and textures

The verified pack can include `assets/moddedmc/moddedmc/trapdoors.json`, an array of trapdoor carrier state IDs. The companion uses these IDs to let the server handle hand toggles and to hide the shared faces of matching trapdoors. The carrier's `powered` value selects the trapdoor material. The server keeps redstone power separately.

The optional `assets/moddedmc/moddedmc/trapdoor-textures.json` lists base texture identifiers with 8 by 8 connected sheets embedded in the pack. The companion uses the `_connected` version of the sprite on matching trapdoor quads. Closed trapdoors connect according to their placement half. Open trapdoors use their hinge axes and directions.

When moving block use was negotiated, a use click on a trapdoor display sends the display entity ID as one signed big-endian 32-bit integer on the play channel `moddedmc:block_display_use`. The companion uses the interpolated block transform and clips against the vanilla crosshair hit. The server reconstructs the ray from the player's current eye position, look direction and block reach. The owning mod checks the same contraption's block shapes and applies the state change.

## Server Mods Creative tab

A Creative player using the companion sees **Server Mods** when the connected server supplies a catalog. Entries carry the item names, models, and components sent by the server during normal play. Taking a stack uses the normal Creative inventory action. The server checks the player's game mode.

A pack can include `assets/<namespace>/moddedmc/creative.json` with a JSON array of item paths in display order. For example, `["bucket_of_moop", "dragonlord_sword"]` lists those two registered items within that namespace. The host includes registered block items too, skips missing and repeated entries, and orders namespaces alphabetically. Display models and other internal items stay out of the tab unless listed.

The host puts a versioned, bounded stack catalog in the verified resource pack. The companion uses it for the current connection and clears it on disconnect. Players can also obtain items through normal gameplay and commands.

## C5 extension

Add renderer capabilities to the configuration answer and carry the negotiated flags into play. Send versioned per-pack definitions and connection-local slot mappings. Keep the stock carrier path until a connection has accepted the relevant definitions. Kinetic rendering needs position, axis, signed RPM and a tick reference. Animated displays already use the item model ID and the server resource pack for rig delivery.
