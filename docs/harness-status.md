# F2 status

The snapshot, packet comparison, synchronized bot actions and cross-version oracle slices passed their Windows and Linux checks. Windows runs compared against the pack. Linux runs exercised the native harness and bot against vanilla and the original Create oracle. See [the baseline](harness-baseline.md) for the recorded differences and reports.

Successful runs keep `report.json` and remove checkpoints. Use `--keep` to retain worlds and diagnostics. See [HARNESS.md](HARNESS.md) for the commands, oracle descriptor and comparison rules.

The 26.3 bot now accepts server impulses, simulates dry player movement and supports held controls, inventory checks, explicit Survival dig timing, and entity interactions. The [bot coverage audit](harness-bot-coverage.md) records the live Create check, regression fixtures and remaining gameplay and presentation checks.
