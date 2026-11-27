# Paid-search acceptance (new host capability; runtime verification pending)

The shared host now exposes a combined exact-stack data/tracker replacement and
XP-level debit, and Compass uses it when starting a search. The acceptance cases
are available through `run-compass.ps1 -Case xp-nature` and `-Case xp-explorer`.
These scripts have not yet been run against the rebuilt Compass package.
The host operation provides an in-memory atomic admission transaction; crash
consistency across an interrupted disk save is a separate concern.

The runner stages config files under `config/compasses/` in a new seed-42 world,
and stages `weighted-group` for Explorer. The insufficient-funds response
contains `Not enough XP levels`, with no state, wear or level mutation.

Nature checks insufficient funds before and after a result, successful
per-target debit, replay resistance, stale same-ID replacement, creative
exemption and a zero-cost override at zero levels. Explorer checks group
cost versus actual-member find-next cost, a paid exhausted search with no
extra wear, insufficient funds, and free navigation/reset. Every XP assertion
queries the authoritative server rather than assuming a client update.
