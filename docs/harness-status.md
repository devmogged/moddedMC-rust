# F2 status

The snapshot, packet comparison and synchronized bot action slices run on Windows. The calibrated client-effects and bot-action scenarios match vanilla against itself. The pack runs report packet, probe and saved-state differences. See [the baseline](harness-baseline.md) for the checks and JSON locations.

Successful runs keep `report.json` and remove their checkpoints by default. Use `--keep` to retain the worlds and logs. The remaining work covers cross-version mod-oracle launch and ID maps, then Linux verification. See [HARNESS.md](HARNESS.md) for the build and run commands.
