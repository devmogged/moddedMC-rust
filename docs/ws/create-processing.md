# Create basin processing

The reference is Create 6.0.10 for NeoForge 1.21.1, tag `mc1.21.1-6.0.10`.

- The basin collects item entities and saves separate nine-slot input and output inventories. Empty-hand use returns the contents. A vertical cog drives a mechanical mixer two blocks above the basin. The mixer starts at 30 RPM and keeps its recipe and stroke progress in the block entity.
- `scripts/scenario.ps1 -Scenario tests/scenarios/create-mixer.txt -RunDir run/create-mixer` drops three andesite and two iron nuggets into the basin. At 16 RPM, the inputs survive a restart. At 64 RPM, the mixer produces two andesite alloy and leaves one andesite. The bot collects all three items and checks that removing the mixer removes its two displays.
- For the real-client check, place a mixer two blocks above a basin, power it with a vertical cog at 64 RPM and toss andesite and an iron nugget into the basin. Check that the whisk lowers into the basin, spins, then retracts. Retrieve one andesite alloy with an empty hand.
- The basin has two 1,000 mB input tanks and two output tanks. Pipes and containers share the same storage. `scripts/scenario.ps1 -Scenario tests/scenarios/create-mixer-fluid.txt -RunDir run/create-mixer-fluid` fills one input from a tank, verifies a full water input refuses another bucket, adds lava to the second input, and checks that both fluids survive restart. The mixer consumes 1,000 mB of water to make dough, leaves the lava available to pump out, and then makes mud from coarse dirt and a 250 mB water bottle. The mud recipe uses the renamed 26.3 block tag.
- Next are blaze burner heat, press compacting, shapeless and brewing recipes, belt and chute routing, and visible fluid levels.
