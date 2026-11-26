# Create survival gaps

Compared with Create 6.0.10 (`mc1.21.1-6.0.10`), ordered by when a player using stock Java 26.3 is likely to hit each gap. Small and Large Water Wheels, the Hand Crank, and the Windmill Bearing with reversible white sails and frames supply starter power in survival. Mechanical Crafters make Crushing Wheels and ordinary recipes and take input from linked Create or vanilla inventories. The Rotation Speed Controller, Encased Chain Drives, and Adjustable Chain Gearshift cover speed control. Core shafts, gearing, belts, press, millstone, mixer and brass, the Deployer sequence for Precision Mechanisms, basic fluids, and basic trains have server scenarios.

| Order | What a survival player hits |
| --- | --- |
| 1 | **Sorting and transfer.** Andesite Funnels and plain Chutes work. Andesite Tunnels craft and place on horizontal belts, split to side outputs and retain blocked stacks across restart. Brass Funnels and Smart Chutes support batch sizes, single-item samples, List Filters and Attribute Filters. Attribute rules cover tags, mod source, items and recipes, enchantments, books, shulker fill, names, colors and bucket fluids. The creative tab rule needs item membership data. Brass Tunnels link across parallel belts, filter by mouth, and save held stacks, distribution modes and the Round Robin cursor. The Mechanical Arm moves items between depots, fuels Blaze Burners, and restores a stack held across restart. Vanilla special-purpose Arm targets and mounted Arm configuration remain. The Threshold Switch filters stored items and fluids and preserves its redstone latch across restart. The Smart Observer filters stored items, fluid tanks, belt and funnel traffic, and completed inbound pipe flows. |
| 2 | **Farm and tree automation.** Stationary drills mine and collect cobblestone through a hopper, and save progress across restart. Mounted drills and harvesters work in the tested setups. A rotating Drill hurts entities that touch its head. Mechanical Saws, Ploughs, mounted Deployers, and Portable Storage Interfaces remain. |
| 3 | **Belts and bulk processing.** Andesite and brass casing application, covers, wrench removal and restart work. Belt slicing and dye interactions remain. Sloped belts cannot carry stock players up. Bulk blasting and smoking do not have the full vanilla recipe set. |
| 4 | **Factory power and fluids.** Pipes, pumps, tanks, valves, drains and basic spout filling work. Steam Engines and boilers, Portable Fluid Interfaces, and custom spout-filling recipes are missing. |
| 5 | **Larger contraptions.** Pistons, bearings, pulleys, elevators, and seats work in the checked cases. Standing players cannot reliably ride moving structures. Cart Assemblers, Sequenced Gearshifts, movement-mode controls, and double-chest capture are missing or incomplete. |
| 6 | **Packages and remote logistics.** Packagers, Frogports, Stock Links, Stock Tickers, and package transport are missing. Display Links and Display Boards are also missing. |
| 7 | **Train upgrades.** Driving, parking, occupied signals and two-stop schedules work. Track Observers, station naming and editing, schedule conditions, and survival track placement across multiple inventory stacks are missing. |
| 8 | **Optional building and utility items.** Decorative palettes and utility items still need registration, placement and recipes. A stock player also lacks the in-world Ponder and Engineer's Goggles help. |

## Deferred

Visual and invisible parity includes exact particles and sounds, advancement triggers, original animation timing and stock carrier outlines. Track timing, stress flicker edge cases and detailed item-display offsets sit behind the survival gaps above. Sail dyes are tracked in [issue #14](https://github.com/devmogged/moddedMC-rust/issues/14). Linked Crafter casing textures are tracked in [issue #15](https://github.com/devmogged/moddedMC-rust/issues/15).

The Rotation Speed Controller cog bracket is tracked in [issue #20](https://github.com/devmogged/moddedMC-rust/issues/20).

The chain casing mining hardness is tracked in [issue #21](https://github.com/devmogged/moddedMC-rust/issues/21).

Andesite Tunnel flap animation is tracked in [issue #25](https://github.com/devmogged/moddedMC-rust/issues/25). Connected Brass Tunnel textures are tracked in [issue #26](https://github.com/devmogged/moddedMC-rust/issues/26).

Stationary Drill cracks and hit sounds are tracked in [issue #27](https://github.com/devmogged/moddedMC-rust/issues/27).
