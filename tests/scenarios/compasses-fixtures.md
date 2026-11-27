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
group find-next (a different member), exhaustion with the last actual
member's ID, and that explicit re-selection clears history.

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
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case reset
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case visibility
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case multiplayer
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case interrupted
powershell -NoProfile -File tests/scenarios/run-compass.ps1 -Case same-item
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
The coordinated 26.3 bot adds `expect tracker <slot> absent`, which
requires the tracker component itself to be absent. A present-empty
tracker would still select the stock lodestone needle model, so use
`absent` when verifying idle/reset/search/failure needle behavior.
The optional three-coordinate form `expect tracker <slot> <dimension>
<x> <y> <z>` also verifies Y. The two offset scenarios use that form.

| Scenario | Required behavior |
| --- | --- |
| compasses-search.txt | Birch forest at (-287, 6); only the selected stack has a tracker, including after restart. |
| compasses-explorer.txt | Desert pyramid at (-1376, -1760), next at (-2944, -2368), stronghold at (1712, 768); each result updates the same stack's tracker. |
| compasses-negative.txt | A navigation action is single-use even without reopening the compass; stale page and pick actions do not reopen dialogs or start another search; a removed stack is rejected; the other player still searches. |
| compasses-dimension.txt | Offhand result persists; an old overworld action cannot execute in the Nether; the new dialog has Nether labels but no overworld-only target or find-next action. |
| compasses-inventory.txt | An active search survives a real hand swap; dropping another active compass does not alter the first compass or publish a result for the removed one. |
| compasses-absent.txt | The same target becomes unfindable under a datapack placement override, clears its tracker, then searches a custom structure using its namespaced label. |
| compasses-custom-rings.txt | Two distinct datapack ring layouts yield (0, 0) and (-160, -1136); the single-position first ring exhausts on find-next. |
| compasses-height.txt | Search from above the build limit still finds a known seed-42 biome after the sample origin is clamped. |
| compasses-weighted-group.txt | Weighted member B then A across restart; nonzero locate offsets; both positions excluded by the third search; failure identifies A and offers no find-next; a fresh group search finds B again. |
| compasses-offset.txt | Individual member B at (137, 875), with locate offset persisted across restart; next search rejects the position of member A. |
| compasses-reset.txt | Sneak-use clears the found tracker; an old dialog action cannot restart the cleared stack; restart retains reset; resetting an active Nature search preserves a different Explorer stack. |
| compasses-visibility.txt | Hidden biome and structure tags and wildcard blacklists filter the direct menu; hidden member A is still discovered through the group; find-next and coordinate display follow the per-mod JSON settings. |
| compasses-multiplayer.txt | Concurrent searches by two players; reset cancels only Alice's search; Bob's actual member tracker and find-next remain available. |
| compasses-interrupted.txt | A deterministic zero-frequency, spacing-one search remains active across a short observation window. Two simultaneous jobs on identical item IDs are isolated: offhand reset leaves the main-hand worker active and the offhand idle through restart. Save/restart retires the unavailable main-hand worker; cross-dimension use in a world with no visible targets persists the interruption; return and reset allow a fresh selection. |
| compasses-same-item.txt | Two identical Explorer item IDs independently locate members A/B, reject a stale click from the other stack and a consumed navigation action, swap hands, remove only one tracker component, and persist the distinct state across restart. Strict component-absence checks cover fresh, opened, reset, and reopened stacks. |

The baseline biome and vanilla structure coordinates were observed on the
earlier seed-42 protocol binary. The custom-ring, weighted-group and offset
coordinates are independently source-derived expectations. All are fixed
assertions; the runtime evidence below distinguishes verified behavior from
still-pending checks.

## Focused protocol run, 2026-11-27

The coordinated Compass-only release pack was rebuilt (212,901 bytes) and
run sequentially on port 26180, seed 42, with retained worlds and logs:

| Case | Outcome | Local log |
| --- | --- | --- |
| reset | PASS (exit 0) | `run/compass-high-reset-1127/scenario.log` |
| visibility | PASS (exit 0) | `run/compass-high-visibility-1127/scenario.log` |
| multiplayer | PASS (exit 0) | `run/compass-high-multiplayer-1127/scenario.log` |
| interrupted | FAIL (exit 1; restart recovery passed, three post-teleport checks missed) | `run/compass-high-interrupted-1127/scenario.log` |
| interrupted (corrected) | PASS (exit 0; all checks, including both dimension changes) | `run/compass-high-interrupted-final-1127/scenario.log` |

The interrupted log shows the bot sending item-use packets without a Compass
reply immediately after dimension changes. The engine's 60-tick client-load
timeout gates those packets. The fixture now waits six seconds after each
dimension change instead of two. One retry was cancelled upon notification
of a shared engine build. The later corrected run on the rebuilt engine
passed all checks (23,500-byte log), including the Nether's empty catalog,
return to the Overworld and a fresh search followed by cancellation.
The same-item scenario and the newly tightened per-page token checks were
prepared after this earlier run. Neither the cancelled retry nor
source-only checks count as protocol passes.
The shared conditional needle model and component-clearing host build
subsequently succeeded. The old logs above used the permissive `none`
assertion; they do not prove the new strict component-absence contract.
The latest negative/same-item/interrupted scenarios require the rebuilt
component-aware bot and newly repacked Compass mod. Actual graphical
needle verification remains pending.
The extended interrupted fixture also exercises two simultaneous jobs
for one player using the same registered item ID and target; this
same-player concurrency case was not part of the prior PASS.

## Latest scoped build

The coordinated `cargo test -p compasses` completed with 32 passed,
0 failed. The release package was rebuilt to 221,308 bytes
(build session 54264, exit 0). This includes state validation,
single-use dialog tokens, group navigation ownership and expiry,
config limits, and explicit-use search progress. The build gate was
released for the shared bot rebuild.

## Strict component and dialog protocol run, 2026-11-27

The 221,308-byte package, 126,614,016-byte optimized host, and
12,507,648-byte rebuilt bot were tested sequentially, seed 42,
port 26180, in fresh retained directories:

| Case | Session | Outcome | Log (bytes) |
| --- | --- | --- | --- |
| negative (initial) | 76944 | FAIL, one ordering assertion | `run/compass-strict-negative-1127/scenario.log` (58,113) |
| negative (corrected) | 39479 | PASS, exit 0 | `run/compass-strict-negative-retry-1127/scenario.log` (47,145) |
| same-item | 66077 | PASS, exit 0 | `run/compass-strict-same-item-1127/scenario.log` (56,381) |
| interrupted (extended) | 92447 | PASS, exit 0 | `run/compass-strict-interrupted-1127/scenario.log` (32,546) |
| dimension (extended) | 85917 | PASS, exit 0 | `run/compass-strict-dimension-1127/scenario.log` (30,475) |

The initial negative log records Bob's exact birch result on line 733,
before the replay acknowledgement on line 763. The assertion cursor
discarded that early result. The fixture now checks the two positive
messages with `expect-all`, and the corrected run passed without
changing gameplay code. Both same-ID stacks tracked independent
members with exact XYZ, and strict absence survived selective reset
and restart. The interrupted run verified simultaneous same-target jobs,
independent progress, selective cancellation, restart and cross-world
recovery. The dimension run preserved the birch tracker (-287, 6),
rejected stale actions, omitted cross-world distance/next, and restored
both on return. All four successful runs completed without reported
bot errors or panics. Zero remaining server/bot processes confirmed
before releasing the runtime gate. Actual graphical needle animation
still requires a stock client.

## Pending config and visibility revision

The visibility fixture now sets Explorer's `maxNextSearches` to 2
instead of 1: after member B and hidden member A, the history still
allows another search, so rejecting find-next demonstrates the hidden
actual-member rule rather than exhaustion. Both configs also specify
explicit zero-cost XP overrides. The mod accepts those no-op overrides
but rejects malformed and nonzero costs; the documented resource-ID
glob syntax is checked at load. Pure tests independently exercise
catalog filtering, deduplication, ordering, and hidden member eligibility.
These source changes are newer than the 221,308-byte package above;
they require a coordinated test/repack and visibility runtime gate.

The next revision also tests nested hidden tags, stale page navigation,
and ordered group membership changes. The new `visibility-reload`
scenario stages a tag-only pack, enables it after the original menu is
opened, checks stale navigation rejection and the filtered menu,
disables it to restore visibility, then enables it again to reject a
stale direct selection. After disabling the pack a second time, a
fresh selection must find the same seed-42 birch forest. This exercises
the live tag manager without assuming the generator hot-reloads its
dynamic registries. It has not been run yet.
