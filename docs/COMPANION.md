# Client companion

The companion is an optional Fabric mod for Java Edition 26.3. Its own code is MIT OR Apache-2.0. The same JAR works with every pack. Mods supply data through the server's existing content interfaces.

## Build

With JDK 25 or newer, run `./gradlew build` from `companion/` (`gradlew.bat build` on Windows). Use `build/libs/moddedmc-companion-0.1.0.jar`. The JAR includes the Fabric networking modules it needs. Fabric Loader 0.20.1 and Minecraft 26.3 come from the launcher.

## Configuration negotiation

The Java configuration custom payload channel is `moddedmc:negotiate`. Integers are unsigned, big-endian. The host sends an empty challenge followed by a vanilla configuration ping. A companion replies to the challenge with a two-byte protocol version and four-byte capability mask. The server answers with its version and the intersection of client capabilities and features used by the running pack. Protocol 1 starts with no visual capabilities. Malformed hello lengths are ignored. Incompatible versions get the server's version and zero capabilities.

The ping orders the exchange before the pack offer or known packs. Stock clients ignore the challenge and answer the ping. On servers without a challenge the companion sends nothing. Every connection currently uses the stock block, item and display representation.
