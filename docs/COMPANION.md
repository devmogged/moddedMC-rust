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

The ping orders the exchange before the pack offer or known packs. When any mod is loaded, a client that has not supplied a valid hello by the ping's reply is disconnected with installation instructions. Bedrock clients cannot complete the Java exchange and are also disconnected. Servers with no loaded mods still permit vanilla clients. On servers without a challenge the companion sends nothing. Every connection currently uses the stock block, item and display representation. The headless scenario bot responds to the same challenge by default; set `MODDEDMC_BOT_COMPANION=0` to test a vanilla client, or set a nonzero protocol version to require a negotiation response.

## Asset delivery

The host uses the generated stock pack's file list. A manifest contains `pack_version` (the stock ZIP's SHA-256) and `assets` (objects with `path`, `sha256` and `size` in bytes). Hashes in JSON and URLs are lowercase hex. Only referenced files from the generated pack are served. Create textures are read from the owner's local JAR at startup.

The game status JSON advertises `moddedmc: {"protocol": 1, "pack_url": "...", "manifest_sha256": "..."}` when a catalog exists. Replace `{host}` in the URL with the game handshake host, bracketed for IPv6. Fetch `manifest-<sha256>.json` and `assets/<sha256>` relative to the ZIP URL. All three routes use the same pack listener. When `MODDEDMC_PACK_URL` points to a proxy, forward the manifest and blob routes alongside the ZIP. The status request and pack routes need no device-agent access.

The companion caches `<gameDir>/moddedmc/manifest-<sha256>.json` and `<gameDir>/moddedmc/assets/<sha256>`. It verifies hashes and lengths before using cache entries or publishing replacements. The manifest is limited to 2 MiB and 10,000 paths. Each asset is limited to 8 MiB and the total to 256 MiB. The companion uses verified blobs to assemble the pack locally on vanilla's pack download worker, after vanilla's pack consent. An invalid or missing blob that cannot be repaired fails the required pack load.

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

Optional `scale`, `y_offset` and `yaw_offset` fields adjust the rig relative to the item display. `hide_parts` lists item model IDs used only by the multipart stock model. The companion suppresses those IDs while the replacement rig's resources are available.

An optional `textures` object maps names to full texture resource IDs. The server can set `moddedmc:rig_texture` in the display stack's `minecraft:custom_data` to select one of those names. An unknown name or unavailable texture uses the descriptor's default `texture`.

An optional `rider_seats` array describes passenger seats in passenger-list order. Each entry requires a `bone` name; optional `x`, `y`, `z` are bone-local offsets in model pixels, `height` is an additional world-space vertical offset in blocks, and `max_age` limits a sample's age in client ticks (default: unlimited). The server associates the body display with its living carrier by setting the integer `moddedmc:rig_mount` in the display stack's `minecraft:custom_data` to the carrier's network entity ID. Missing or negative IDs, missing bones, unloaded displays and expired samples leave normal passenger positioning intact. The companion samples the animated bone with the display's actual transform and updates both rider positioning and interpolated rendering; server mounting and collision remain authoritative.

An `.mmc` can supply the descriptor and its referenced files under `assets/`. The host finds rigs for registered standalone item model IDs and adds the descriptor and its geometry, animations and texture to the generated server resource pack. This uses the same pack acceptance and hashed asset cache as stock models.

To exercise the renderer, enable the [test resource pack](../companion/src/test/resources/rig-pack) on a 26.3 Fabric client with the companion and GeckoLib. Run the following in a world with commands enabled.

```mcfunction
summon minecraft:item_display ~ ~ ~2 {Tags:["rig_test"],item:{id:"minecraft:paper",count:1,components:{"minecraft:item_model":"rig_test:hinge"}}}
data merge entity @e[type=minecraft:item_display,tag=rig_test,limit=1] {item:{id:"minecraft:paper",count:1,components:{"minecraft:item_model":"rig_test:hinge","minecraft:custom_data":{"moddedmc:rig_clip":"animation.hinge.park"}}}}
```

The first command shows a swinging arm and the second parks it upright. The item model is a gold block on a stock client with the test pack enabled.

## Item display parts

A mod can put an object mapping part paths to block or item models in `assets/<namespace>/moddedmc/display_parts.json`. For example, `{"hinge/arm": "example:block/hinge_arm"}` supplies the item model `example:moddedmc/parts/hinge/arm`. Put that ID in a vanilla item's `minecraft:item_model` component and show it with the existing item display interface. The generated resource pack includes the referenced model and textures. This works in stock clients and requires no additional registry entries.

## Scaffolding shapes

The verified pack can include `assets/moddedmc/moddedmc/scaffolding.json`. Each wire state ID maps to a bottom flag, the placed block item's model identifier, and an optional map of base texture identifiers to connected texture sheet sizes. The connected texture is the base identifier with `_connected` appended. Sheets with two tiles per side apply to horizontal faces and eight-tile sheets apply to vertical faces. The host uses dedicated trapdoor states for this content. The companion uses the flag to select the standing collision and outline shapes, and the model identifier to show the full placement outline when the player holds the matching block. The mapping belongs to the current connection and is released at disconnect.

## Server Mods Creative tab

A Creative player using the companion sees **Server Mods** when the connected server supplies a catalog. Entries carry the item names, models, and components sent by the server during normal play. Taking a stack uses the normal Creative inventory action. The server checks the player's game mode.

A pack can include `assets/<namespace>/moddedmc/creative.json` with a JSON array of item paths in display order. For example, `["bucket_of_moop", "dragonlord_sword"]` lists those two registered items within that namespace. The host includes registered block items too, skips missing and repeated entries, and orders namespaces alphabetically. Display models and other internal items stay out of the tab unless listed.

The host puts a versioned, bounded stack catalog in the verified resource pack. The companion uses it for the current connection and clears it on disconnect. Players can also obtain items through normal gameplay and commands.

## C5 extension

Add renderer capabilities to the configuration answer and carry the negotiated flags into play. Send versioned per-pack definitions and connection-local slot mappings. Keep the stock carrier path until a connection has accepted the relevant definitions. Kinetic rendering needs position, axis, signed RPM and a tick reference. Animated displays already use the item model ID and the server resource pack for rig delivery.
