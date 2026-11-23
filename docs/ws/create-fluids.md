# Create fluids

The reference is Create 6.0.10 for NeoForge 1.21.1, tag `mc1.21.1-6.0.10`.

- WS-2 registration and pressure landed in `d22614c`. The fluid-state branch includes the original fluids branch, including the regression fix for multiple faces of one fluid handler. The four `ws2a-*` scenarios cover pipe placement, orientation, pump speed and pressure after restart.
- A powered pump moves water or lava between 8,000 mB single-block tanks. In Creative, buckets add or remove one bucket and the wrench switches the tank windows. Tank contents and window state survive restart. The fluid models load from the owner's Create JAR. The bot verified the data and interactions, and a real client still needs to check the static tank models.
- To reproduce the transfer check, build and package Create, then run `scripts/scenario.ps1 -Scenario tests/scenarios/create-fluid-tank.txt -RunDir run/create-fluid-tank`. On engine `2b66d5d13`, the bot moved 1,000 mB through two pipes, verified both ends, reloaded the receiving tank and checked that a water-filled tank refused lava. A second transfer succeeded once the water was emptied.
- In survival, apply a copper ingot to a stripped log or wood and craft the casing with iron bars to make an item drain. Right-clicking the drain with a full bucket swaps it for an empty one. The 1,500 mB buffer rejects partial exchanges, and pipes can extract through the sides or bottom. `scripts/scenario.ps1 -Scenario tests/scenarios/create-item-drain.txt` checks crafting, bucket exchange, transfer of 1,000 mB and persistence on both sides of the transfer.
- The next slice is filling containers with the spout. Multiblock controllers, world openings, automated item draining, mounted tanks and visible fluid levels remain open.
