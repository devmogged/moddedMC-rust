# F2 pause status

Local commit `38d2f14` on `main`. Publication is paused.

The vanilla 26.3 snapshot slice runs through `moddedmc-harness run`, `run-all` and `self-test`. It verifies tick deltas from the game clock, captures flushed Anvil snapshots, compares player NBT and console probes, and synchronizes the existing bot's place and break packets with server acknowledgements.

The five-scenario baseline recorded 11 differences, zero allows and zero run errors. Vanilla against itself matched. A deliberate mutation produced one stone-versus-dirt mismatch. The exact commands, artifact hashes and differences are in [the baseline](harness-baseline.md). Local JSON and logs are under `run/harness-investigation/`.

Remaining work covers packet normalization with entity-ID mapping, synchronized move/look/attack and inventory clicks, cross-version mod-oracle launch and ID maps, and Linux verification.

Resume with the build and self-test commands in [HARNESS.md](HARNESS.md), then rerun `run-all` against the current `pack-dev`. Use the pinned baseline to distinguish existing differences from new ones.
