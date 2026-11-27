# Safe permission-checked teleport acceptance (pending shared interface)

The positive config is deliberately not staged by the active runner. The
current mod rejects it, rather than exposing an unconditional teleport.
The pure sea-level landing selector has unit coverage, but these scenarios
require an authoritative permission query and generated-column snapshot.

Use the seed-42 birch result at (-287, 6) in the Overworld. Load the target
chunk by a console teleport, create an unobstructed two-block headroom at
Y=63/64 with full support at Y=62, then return the player to the established
search origin (-30.5, 83, 6.5). Check the actual server teleport position
when the player has permission, not just a success message. Use peaceful
difficulty to reduce unrelated movement/death noise.

Required negative cases:

- No privilege: no executable teleport button (including in creative mode).
- Grant privilege, open a found compass, revoke privilege, then click the
  still-displayed action. Recheck on the server; the player stays at origin.
- Grant privilege, open a found compass, replace that stack with a fresh
  same-ID compass, then click the stale action. No teleport may occur.
- Open an action and change dimensions before using it. Reject the stale
  action; never reinterpret saved coordinates in the new dimension.
- A current found target in another dimension follows the explicit policy
  of the final capability: either reject with no side effects or teleport
  into the saved dimension. Never teleport to those X/Z coordinates in the
  wrong dimension.
- If the target chunk is not loaded, return a distinct unavailable result or
  wait for the coordinated generation request; no unsafe fixed-Y fallback.
- Test a sealed column, fluid, fire, partial support, and insufficient
  headroom. A successful selection must come from the coherent generated
  block snapshot, remain within dimension height bounds, and be rechecked
  if the snapshot becomes stale before movement.

After integration, add stock-protocol executable checks for positive and
negative permission changes, exact-stack identity, and destination. Preserve
the pure selector tests for deterministic tie order and vertical bounds.
The final menu label and permission failure text depend on the host contract.
This document and config are unrun acceptance material, not a claim of
working teleport support.
