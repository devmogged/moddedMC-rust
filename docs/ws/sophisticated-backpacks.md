# WS-18 — Sophisticated Backpacks (M0 license gate)

## Latest release checked 2026-11-20

- Source project: https://github.com/P3pp3rF1y/SophisticatedBackpacks
- CurseForge files: https://www.curseforge.com/minecraft/mc-mods/sophisticated-backpacks/files/all
- Latest Minecraft 26.3 NeoForge release listed November 18, 2026: `sophisticatedbackpacks-26.3-3.26.22.2260.jar`.
- CurseForge also lists `sophisticatedbackpacks-1.21.11-3.26.4.2261.jar` November 18, 2026. Version ordering across different Minecraft branches is not directly comparable; choose the actual latest published release after permission is secured.

## License gate — BLOCKED

The current upstream `LICENSE.md` at https://github.com/P3pp3rF1y/SophisticatedBackpacks/blob/HEAD/LICENSE.md is copyright 2025 P3pp3rF1y, All Rights Reserved (ARR). It expressly covers source code, assets and other project files. Sections 2 and 5 prohibit sharing modifications, unauthorized forks, adaptations and derivative works without express permission. Section 6 allows only limited code snippets (<20 lines) with attribution; this is not a grant to port the project. Translations are the one explicitly unrestricted class, but that does not authorize the code port or assets.

**Do not implement or publish a Rust translation of this source until written permission explicitly covering the proposed Rust port, distribution and assets is obtained and retained.** The owner's general statement that mod permissions were obtained is not a copy of the specific grant for this ARR project; verify the exact scope with the owner. A standalone clean-room alternative from independently developed behavior might have different legal considerations but is not the assigned source-faithful port and needs separate review.

## Unfinished survey and milestones

No code, jar contents, registry inventory, ABI matrix, config/packet inventory, scenarios or tests were added because the license gate precedes implementation. If permission arrives: save it privately; inspect the latest release jar and repository license at that specific revision (including `META-INF` and bundled dependencies); survey all items, blocks, upgrades, menus, recipes, tags, block entities, commands, configs, packets and assets; map each required host operation to `docs/MOD-ABI.md` or file WS-0 requests. Then generate `mods/sophisticated-backpacks` with `scripts/new-mod.ps1` and implement tiers and item-component inventory with source-based unit tests before menus.

## Verification

Upstream license read directly via GitHub on 2026-11-20; release metadata checked against current CurseForge files listing. No compilation or gameplay verification attempted. The correct blocker is permission, not technical feasibility.
