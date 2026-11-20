# WS-12 Corpse — session noelalex-ab5ef3 — handoff, 2026-11-20

Branch ws/corpse. This session completed an independent audit of the current Corpse 1.1.22+26.3 / Corelib 26.3 source's death-history format, actual-drop reconciliation, menu ordering and empty-despawn boundary. Added that evidence and two standalone generic host specifications. The source is All Rights Reserved: no original code, assets or binary contents were committed. This is an intentionally documentation-only slice; the existing Rust no-op registration and 10 policy tests were not changed.

Current blockers: accepted but unimplemented death/drop transaction issue #73; generic persistent nonliving custom entity and transactional player inventory / menu hooks still need coordinator requests. Source-specific follow-up: add a test for empty-despawn delay 0, then fix the first-empty if/else-if boundary in mods/corpse/src/rules.rs. Continue with authoritative NBT ItemStackWithSlot fixtures and running original NeoForge 26.3 oracle only after the proper host interfaces exist.

Verification: public upstream Java source inspection and this documentation diff; no cargo build, unit tests, scenarios, or runtime oracle executed in this session. The remote workstation connection reached its monthly command quota before the planned code or test operations could begin.

Coordinator state: session noelalex-ab5ef3 was claimed by coord.mjs start but NOT RELEASED; the remote-command service rejected further commands and specifically instructed not to retry until reset. Do not claim that submit, beat, or release succeeded. Its coordination lease should expire according to the board's lease policy if no one can run the release command. When the workstation becomes available, first fast-forward N:\mmc-wt\ws-12 to the current origin/ws/corpse, then release this session with:
node N:\mmc-coord\coord.mjs release --session noelalex-ab5ef3 --state free --handoff N:\mmc-wt\ws-12\docs\ws\corpse-ab5ef3-handoff.md

Then file both WS-0 requests via the coordinator request command, with the two request markdown files, and run the next normal coord.mjs start. Do not touch another agent's worktree or overwrite uncommitted data.
