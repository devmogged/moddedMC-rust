# Compass server settings

The native port reads separate JSON files under `config/compasses/` when
the mod registers. Missing files use the original search defaults, except
for unsupported teleport (disabled). Invalid
JSON, unsupported keys, and out-of-range values reject registration with an
explicit log message. Restart the server after changing these files.

`naturescompass.json`:

```json
{
  "biomeBlacklist": [],
  "maxNextSearches": 25,
  "radiusModifier": 2500,
  "sampleSpaceModifier": 16,
  "maxSamples": 50000,
  "displayCoordinates": true,
  "allowTeleport": false,
  "defaultXpLevels": 0,
  "perBiomeXpLevels": [],
  "compassDurability": 0
}
```

`explorerscompass.json`:

```json
{
  "structureBlacklist": [],
  "maxNextSearches": 100,
  "maxRadius": 10000,
  "maxSamples": 100000,
  "displayCoordinates": true,
  "allowTeleport": false,
  "defaultXpLevels": 0,
  "perStructureXpLevels": [],
  "compassDurability": 0
}
```

Patterns match the entire namespace-qualified registry ID: `*` matches any
number of characters, and `?` matches one. Both compasses also hide direct
entries tagged `c:hidden_from_locator_selection` in the respective
`worldgen/biome` or `worldgen/structure` registry. Explorer's group membership
remains the structure set's full ordered membership, including hidden or
blacklisted members, matching upstream group search. A group is offered
through a visible first-group member.
Patterns support lowercase resource-ID characters and the documented
wildcards; arbitrary Java-regex syntax is rejected explicitly rather than
silently misinterpreted.

Set `maxNextSearches` to zero to disable find-next. The port currently accepts
0–25 for Nature and 0–100 for Explorer; the upstream upper bound of 10,000
cannot be represented in the existing bounded item-state ABI. A very long
custom identifier can reduce the available history further; the search
rejects before starting rather than losing a result.

Nature's biome-size multiplier is 4, as in upstream 3.3.1. The two modifiers
accept 0–1,000,000. A zero radius or sample-space modifier disables search.
Nature's sample cap accepts 0–1,000,000; Explorer's 0–100,000,000 and
radius 0–1,000,000. Explorer uses a strict sample cap and excludes radius
from ring search, except that radius zero disables searching entirely.
Nature uses an inclusive sample cap: `maxSamples: 0` still permits the
origin column probe. Explorer's zero sample cap permits no candidates.

With `displayCoordinates: false`, the stock-client dialog and result
message omit coordinates. The vanilla lodestone tracker remains on the item
so the needle can point to the target.

The stock-client dialog reports horizontal distance for a found target in
its original dimension. In another dimension it explains why the needle
cannot be followed. The stored target remains attached to its original
dimension until a new search or sneak-use reset. This avoids suggesting a
structure exists at the same coordinates in an unrelated dimension.
Every ordinary item use now reconciles the saved needle state even when
the current dimension has an empty or unavailable target catalog. The
legacy MCc1/MCc2 formats remain readable. MCc3 adds a two-byte
per-stack successful-search wear count when durability is enabled.
Malformed identifiers,
inconsistent status/history, and coordinates outside the host's supported
tracker bounds are rejected. An invalid terminal worldgen result is
converted to a failed search, not a false success.

Both upstream compass needle implementations fall back to the respawn
position when not found. The shared host now implements the stock 26.3
component-presence condition in source, choosing the spawn needle when
the lodestone component is absent and the lodestone needle otherwise.
Clearing a target removes the component atomically with the saved data.
The coordinated host build and strict component-presence wire checks
passed (negative, same-item, extended interrupted, extended dimension).
Actual graphical needle verification remains pending.
The client wire serializer overwrites the item-model component with the
registered custom item ID, because the reverse serializer uses that ID to
restore the mod item. A per-stack model override cannot implement the
fallback without breaking that identity contract. The native port therefore
does not write a synthetic spawn target or pretend a cleared tracker is
a found location.

Dialogs use short-lived, single-use page tokens independently of the
durable stack nonce used for inventory compare-and-replace. Navigation
rotates the page token without writing the stack. Only the displayed page's
choices and adjacent navigation actions are accepted. Opening or resetting
another compass supersedes the old dialog, not the other stack's search.
Page navigation revalidates the current visible catalog and ordered group
membership. If the host's live registry changes, reopen the compass.
Selecting a displayed group or retrying its found member cannot silently
switch to a changed membership or another first-registered group.
Using a searching compass explicitly reports its target and the live
worker's completed sample count and rounded search radius (500 blocks
for Nature, 250 for Explorer random-spread). Ring searches omit radius,
as in the original ring worker. A concurrent in-flight probe may report
only the target. No per-tick network updates or inventory writes are
performed just to display progress; this is a stock-client adaptation
of the original modded HUD.

These are native-port settings, not drop-in NeoForge TOML files.
Durability accepts 0–10,000 successful searches. Zero disables wear.
With positive durability, successful non-creative results atomically
update the custom wear count alongside the search data and tracker.
Failures, canceled jobs, creative successes, and sneak-reset do not
consume or restore wear. A broken compass remains in inventory but
cannot start a search; the shipped shapeless recipe (mod compass plus
one vanilla compass) produces a fresh compass with no history or wear.
The native stock-client presentation reports remaining uses in the
dialog and on successful search. It does not yet project a graphical
vanilla durability bar; the original mod uses its own custom damage
component rather than vanilla tool breakage.

XP charging and permission-checked teleport still need generic host
capabilities. Explicit disabled settings above (including valid per-target
`"pattern,0"` XP overrides) are accepted to aid migration from the Java
General config. A request to enable teleport or charge XP rejects
registration with a field-specific error rather than silently dropping
a gameplay cost. The parsed XP policy uses declaration order for
overlapping distinct globs and the last value for repeated identical
patterns; Java's HashMap iteration order is unspecified.
The upstream `allowTeleport` default is true; the native default remains
disabled until a permission-checked host action exists.
The stock-client dialog deliberately displays only searchable
targets in the current dimension; the full source registry and generating
dimensions need a generic registry metadata API. No partial cross-world
catalog is advertised as complete.
