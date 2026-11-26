# Compass regression fixtures

All coordinate assertions use seed 42 and the default spawn of the fresh world.
The absent-target case uses a real datapack overriding the vanilla desert
pyramid placement: frequency zero means no starts can generate; the larger
spacing makes the search reach its configured radius quickly. The target is
still selectable because its biome remains possible in the dimension.
The scenario enables the staged pack and restarts before testing the override.
It also registers a separate, namespaced desert pyramid with spacing 1.
After checking the failed vanilla search, the bot teleports to the known
seed-42 valid pyramid chunk and selects the custom structure by label. This
exercises both custom structure registration and a non-vanilla placement.

From the repository root, stage into a **new** dedicated run directory:

```powershell
$run = 'run/worker3-compass-absent'
$dest = Join-Path $run 'world/datapacks'
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item -Recurse tests/scenarios/compasses-fixtures/zero-frequency $dest
powershell -NoProfile -File scripts/scenario.ps1 -Scenario tests/scenarios/compasses-absent.txt -RunDir $run -Keep -Port 26180 -Seed 42
```

Do not use `-Fresh` after staging: it would delete the fixture. Use a new
run directory for each run to ensure that the initial state is vanilla and the
pack is disabled. Coordinate with the build owner before running.

For the custom concentric-ring regression, stage the other fixture:

```powershell
$run = 'run/worker3-compass-custom-rings'
$dest = Join-Path $run 'world/datapacks'
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item -Recurse tests/scenarios/compasses-fixtures/custom-rings $dest
powershell -NoProfile -File scripts/scenario.ps1 -Scenario tests/scenarios/compasses-custom-rings.txt -RunDir $run -Keep -Port 26182 -Seed 42
```

The two custom sets each have one ring position and an empty preferred-biome
tag, so no biome adjustment changes the initial chunk. The first set has
distance 0, yielding chunk (0, 0). The second has distance 16: Java's legacy
RNG at seed 42 yields angle 4.5714174244200265 and next double
0.6832234717598454, radius 71.32893887039381 chunks, and rounded chunk
(-10, -71). These source-derived coordinates are independent of the vanilla
stronghold ring cache. The scenario also checks that find-next on the
one-position ring exhausts the set instead of returning the vanilla ring.

## Weighted group and locate offsets

The weighted-group fixture is staged in the same manner, under a new run
directory:

```powershell
$run = 'run/worker3-compass-weighted-group'
$dest = Join-Path $run 'world/datapacks'
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item -Recurse tests/scenarios/compasses-fixtures/weighted-group $dest
powershell -NoProfile -File scripts/scenario.ps1 -Scenario tests/scenarios/compasses-weighted-group.txt -RunDir $run -Keep -Port 26184 -Seed 42
```

This scenario requires the group SDK and menu integration. It is intentionally
not runnable against the earlier individual-only binary. The pair uses count 2,
spread 2, distance 16 and no preferred-biome adjustment. Applying the Java
LCG and ring algorithm at seed 42 gives chunks (-10,-71) and (8,55).
The large-feature seed for each chunk yields the initial nextInt(3) rolls
0 and 2 respectively. With weights A=1 and B=2, A wins the former and B
the latter if both structure starts validate. B's chunk is closer to the
default spawn. The locate offset [9,7,-5] makes their targets (-151,7,-1141)
and (137,7,875). These expectations are source-derived, not yet runtime
measurements. The scenario checks the actual member, restart persistence,
group find-next (a different member), and exhaustion.

The same pack also supports compasses-offset.txt, a narrower regression
which selects only member B and verifies its offset/persistence followed
by exhaustion. Run in a separate fresh directory by substituting that
scenario and a new run directory/port in the staging commands above.

## Regression run order

The companion runner stages the selected fixture automatically, requires a
new run directory, never invokes Cargo or repacks, and runs exactly one
scenario. From the repository root, after the prime grants the runtime gate:

```powershell
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case search
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case negative
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case dimension
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case explorer
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case inventory
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case height
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case absent
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case custom-rings
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case offset
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case weighted-group
```

Run these separately, reviewing each exit code before starting the next.
Passing worlds are removed unless `-Keep` is set; failing worlds and logs
are retained. Optional `-Port` and `-RunName` select an isolated port and
directory. The runner checks existence, not freshness, of build artifacts.

Use a fresh directory for each non-fixture scenario, seed 42, and one
server/bot scenario at a time. The following checks use the rebuilt bot with
the `expect tracker <slot> <dimension> <x> <z>` command; `expect tracker
<slot> none` checks that no lodestone target remains. Slots 36–44 are the
hotbar, and slot 45 is the offhand.
The optional three-coordinate form `expect tracker <slot> <dimension>
<x> <y> <z>` also verifies Y. The two offset scenarios use that form.

| Scenario | Required behavior |
| --- | --- |
| compasses-search.txt | Birch forest at (-287, 6); only the selected stack has a tracker, including after restart. |
| compasses-explorer.txt | Desert pyramid at (-1376, -1760), next at (-2944, -2368), stronghold at (1712, 768); each result updates the same stack's tracker. |
| compasses-negative.txt | Replayed stale page and pick actions do not start another search; a removed stack is rejected; the other player still searches. |
| compasses-dimension.txt | Offhand result persists; an old overworld action cannot execute in the Nether; the new dialog has Nether labels but no overworld-only target or find-next action. |
| compasses-inventory.txt | An active search survives a real hand swap; dropping another active compass does not alter the first compass or publish a result for the removed one. |
| compasses-absent.txt | The same target becomes unfindable under a datapack placement override, clears its tracker, then searches a custom structure using its namespaced label. |
| compasses-custom-rings.txt | Two distinct datapack ring layouts yield (0, 0) and (-160, -1136); the single-position first ring exhausts on find-next. |
| compasses-height.txt | Search from above the build limit still finds a known seed-42 biome after the sample origin is clamped. |
| compasses-weighted-group.txt | After group SDK integration: weighted member B then A across restart; nonzero locate offsets; both positions excluded by the third search. |
| compasses-offset.txt | Individual member B at (137, 875), with locate offset persisted across restart; next search rejects the position of member A. |

The coordinates above were observed on the earlier seed-42 protocol binary.
They are fixed assertions for the new binary, not evidence that the new
tracker, ring-cache, and datapack regressions have passed.
