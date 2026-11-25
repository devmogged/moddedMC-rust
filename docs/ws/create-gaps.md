# Create survival gaps

Compared with Create 6.0.10 (`mc1.21.1-6.0.10`), ordered by when a player using stock Java 26.3 is likely to hit each gap. Small and Large Water Wheels, the Hand Crank, and the Windmill Bearing with reversible white sails and frames supply starter power in survival. Mechanical Crafters make Crushing Wheels and ordinary recipes and take input from linked Create or vanilla inventories. The Rotation Speed Controller, Encased Chain Drives, and Adjustable Chain Gearshift cover speed control. Core shafts, gearing, belts, press, millstone, mixer and brass, the Deployer sequence for Precision Mechanisms, basic fluids, and basic trains have server scenarios.

| Order | What a survival player hits |
| --- | --- |
| 1 | **Sorting and transfer.** Andesite Funnels, Brass Funnels with single-item filters and batch sizes, and plain Chutes work. Smart Chutes, list and attribute Filter items, Brass Tunnels, the Mechanical Arm, and inventory/redstone controls such as the Threshold Switch and Content Observer are missing. |
| 2 | **Farm and tree automation.** Mounted drills and harvesters work in the tested setups. Stationary drill mining, Mechanical Saws, Ploughs, mounted Deployers, and Portable Storage Interfaces are missing or incomplete. |
| 3 | **Belts and bulk processing.** Belts lack common wrench, casing and dye interactions. Sloped belts cannot carry stock players up. Bulk blasting and smoking do not have the full vanilla recipe set. |
| 4 | **Factory power and fluids.** Pipes, pumps, tanks, valves, drains and basic spout filling work. Steam Engines and boilers, Portable Fluid Interfaces, and custom spout-filling recipes are missing. |
| 5 | **Larger contraptions.** Pistons, bearings, pulleys, elevators, and seats work in the checked cases. Standing players cannot reliably ride moving structures. Cart Assemblers, Sequenced Gearshifts, movement-mode controls, and double-chest capture are missing or incomplete. |
| 6 | **Packages and remote logistics.** Packagers, Frogports, Stock Links, Stock Tickers, and package transport are missing. Display Links and Display Boards are also missing. |
| 7 | **Train upgrades.** Driving, parking, occupied signals and two-stop schedules work. Track Observers, station naming and editing, schedule conditions, and survival track placement across multiple inventory stacks are missing. |
| 8 | **Optional building and utility items.** Decorative palettes and utility items still need registration, placement and recipes. A stock player also lacks the in-world Ponder and Engineer's Goggles help. |

## Deferred

Visual and invisible parity includes exact particles and sounds, advancement triggers, original animation timing and stock carrier outlines. Track timing, stress flicker edge cases and detailed item-display offsets sit behind the survival gaps above. Sail dyes are tracked in [issue #14](https://github.com/devmogged/moddedMC-rust/issues/14). Linked Crafter casing textures are tracked in [issue #15](https://github.com/devmogged/moddedMC-rust/issues/15).

The Rotation Speed Controller cog bracket is tracked in [issue #20](https://github.com/devmogged/moddedMC-rust/issues/20).

The chain casing mining hardness is tracked in [issue #21](https://github.com/devmogged/moddedMC-rust/issues/21).
