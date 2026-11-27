# Global catalog acceptance (pending coherent host snapshot)

This pack has two registered structures but no structure sets or placements.
`aaa_compass_test:unplaced` is eligible by the stronghold biome holder set;
`aaa_compass_test:never_eligible` uses a deliberately empty biome tag. The
fixture is not part of the active scenario runner, and its registry loading
has not yet been verified. Do not substitute successful local placement
enumeration for these global-registry checks.

When the host snapshot and mod UI are integrated:

1. In the Overworld, the global biome catalog contains `minecraft:birch_forest`
   and `minecraft:basalt_deltas`; metadata identifies the Overworld and Nether
   respectively. Search for birch succeeds at (-287, 6), while a request for
   basalt deltas must be rejected without starting a job or clearing the
   previous result. If the UI makes unavailable choices informational rather
   than actionable, assert the absence of an executable action instead.
2. In the Nether, the same global registry is present, but generation metadata
   and current-dimension search authorization distinguish the two biomes.
   Preserve the previous Overworld tracker and reject stale actions created
   before the dimension transfer. No search or charge occurs on denial.
3. Enable `catalog-only` and restart. Both custom structure IDs must be present
   in the Explorer global catalog. `unplaced` has a nonempty Overworld biome
   intersection but no locatable placement; `never_eligible` has a known-empty
   generating-dimension list. Neither may masquerade as a valid current-world
   placement. Compare metadata with registry/biome holders, not locator sets.
4. Hide the custom IDs through a nested `c:hidden_from_locator_selection`
   tag, reload, and verify stale page/pick rejection. Restore the tag and
   verify they are visible again. Registry additions/removals themselves
   require a coherent generator reload or restart, not merely tag reload.
5. Unit-test a successful empty global snapshot separately from an unavailable
   snapshot. Never advertise a partial world-by-world union as authoritative.

The exact dialog representation and executable scenario assertions depend on
the generic snapshot API and the final stock-client UI. This is fixture data
and acceptance intent, not evidence of completed catalog support.
