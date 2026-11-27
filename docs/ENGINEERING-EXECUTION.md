# Engineering execution and delivery playbook

This playbook exists to maximize accepted BUJA gameplay per unit of engineering time. Use it at session start, at every handoff, before every expensive validation run, and before any commit or PR.

## 1. Optimize for accepted gameplay, not activity

The project outcome is a complete, client-compatible native implementation of the BUJA pack. Count progress only when a player-visible capability advances and the evidence level is explicit. Agent messages, changed line counts, generated fixtures, green compilation and a passing isolated unit test are not interchangeable with working gameplay.

Keep these evidence levels distinct:

1. **Inspected** — exact upstream release/source and vanilla behavior traced; parity requirements recorded.
2. **Implemented** — production code and data exist in the owned scope.
3. **Unit-verified** — focused deterministic tests pass.
4. **Compiled/linted** — the exact affected workspace/crate builds and lints.
5. **Scenario-verified** — a survival-style scenario passes against a freshly packaged artifact.
6. **Persistence-verified** — restart, multiplayer, or forced-process-crash cases pass as applicable. A graceful restart is not a crash test.
7. **Client-verified** — a stock Java 26.3 client checklist is completed for visible GUI, rendering, sound and interaction behavior.
8. **Delivered** — a coherent reviewed commit is merged or a PR is open with current base, evidence and explicit remaining gaps.

The BUJA acceptance ledger must state the highest level reached per capability, plus source release, implementation commit, engine gitlink, artifact hash, scenario/log, client kind, and result. Never mark a whole mod complete because one mechanic or one scenario passed.

## 2. Start with a state and ownership audit

Before editing, capture:

- repository root, exact worktree path, branch, HEAD and upstream tracking state;
- `git status --short` including untracked files, plus diff/stat summaries;
- current `origin/main`, branch divergence and relevant open PRs/issues;
- active workers, their exact owned paths, current handoff and blockers;
- active Cargo/rustc/package/server processes and the person/session that started them;
- the current acceptance-ledger rows and latest run artifacts.

Read `AGENTS.md`, `CONTRIBUTING.md`, the matching agent skill and relevant feature/ABI docs. Search issues and PRs before claiming work. An issue is the ownership record; do not infer that an unassigned dirty path is free. If ownership is ambiguous, stop edits to that path and coordinate.

**Shared checkout safety:** never run clean scripts, `git reset`, broad checkout/restore, stash, branch switches, recursive deletion or bulk staging against a shared/dirty worktree. Never assume an old worktree is disposable because its branch is behind. Preserve dirty and untracked work. Create a separate worktree from current `origin/main` for independent docs/fixes when useful. Clean only a named cache/artifact path after verifying it is generated, ignored and unused.

## 3. Assign work to reduce contention

- Give every worker one bounded vertical slice, one explicit owner and exact file/path boundaries.
- Split by independent mod crate or independent fixture family. Do not assign two workers the same files or the same acceptance failure.
- Shared API/host/Pumpkin, workspace manifests, packer and scenario-runner changes have one named integration owner. Mod workers request shared capability through an issue/handoff rather than editing shared code opportunistically.
- Keep one integration coordinator responsible for dependency order, merge readiness, the shared build gate and acceptance-ledger updates.
- Prefer work that unblocks multiple dependent features or closes a player-visible critical path. Do not spawn more workers when review, build or runtime capacity is saturated.
- Each worker handoff must include exact changed paths, behavior added, tests run with results, unvalidated behavior, remaining work and any dependency needed from another owner.

## 4. One expensive validation gate at a time

Cargo builds, packaging and live server scenarios contend for the same workspace/target outputs and can invalidate one another's evidence. The integration coordinator owns one exclusive gate:

1. Freeze source edits to the files being compiled; record the source HEAD and working-tree diff hash.
2. Run the narrowest useful compile/test first, then the required workspace checks.
3. Package the exact target commit/tree. Record artifact absolute path, size, SHA-256 and build command/result.
4. Start the scenario server from that artifact; record PID, run directory, scenario file and start time.
5. Preserve complete logs and extract the first causal failure, not only the last generic rejection.
6. After a fix, rebuild and rerun only the discriminating regression plus impacted acceptance cases.
7. Release the gate and tell workers exactly which source paths may resume.

Other workers may continue static source work only in disjoint owned paths while the gate is occupied. They must not launch competing Cargo, pack or server runs. Do not kill a process by executable name; stop only a PID you started and recorded.

## 5. Make experiments discriminating; never loop

For every failing run, maintain a compact experiment record:

| Field | Required content |
| --- | --- |
| Run identity | Timestamp/run ID, scenario, server PID and run directory |
| Inputs | Source commit/tree hash, dirty diff identity, artifact path and SHA-256 |
| Observation | First causal error, relevant log lines and actual inventory/world state |
| Hypothesis | One falsifiable root-cause explanation |
| Test | Smallest test that distinguishes the hypothesis from alternatives |
| Result | Confirmed/rejected/inconclusive, with evidence |
| Next action | Exact code owner/path and one concrete next step |

Before rerunning, compare the experiment with the previous attempt. If source, artifact, fixture, configuration, world state and expected observation are unchanged, do not rerun. A rerun is justified only by a changed input or to establish reproducibility after a specific fix. Retain failed logs; never weaken a scenario merely to get green.

### Known failure patterns to prevent

- **Stale artifact / wrong binary:** a scenario can exercise an old pack even after source changes. Verify the artifact hash and pack timestamp, and prove the scenario runner launched that exact artifact before interpreting a pass or failure.
- **Misattributed failure:** distinguish menu validation, callback dispatch, mod archive CAS, host participant commit, player checkpoint lineage, filesystem persistence and recovery. Log the first rejected boundary and relevant identifiers/revisions; do not label every rejection as a generic menu bug.
- **Fixture drift:** validate that scenario actors, blocks, recipes, dimensions and prerequisites actually exist in the current pack. A fixture setup failure is not a gameplay pass or a production defect.
- **Build contention:** overlapping Cargo/pack/runtime jobs waste CPU, hold stale locks and make artifacts/evidence ambiguous. Use the exclusive gate above.
- **Repeated diagnosis without a code delta:** every investigation cycle must end in a discriminating test, a production fix, a documented external blocker, or a handoff to the owner. Repeating the same broad audit is not progress.
- **False completion:** unit tests, successful compile, bot output, restart persistence and real client behavior are separate claims. Report exactly what was observed and what remains untested.
- **Shared-tree damage:** never stage `git add -A`, mass-format, clean, reset or commit a shared dirty checkout. Stage exact owned paths only; inspect staged diff before committing.

## 6. Transaction and item-conservation discipline

For inventories, menus, storage networks, corpse recovery, Lootr and world drops:

- State the authoritative owner for every item at each transaction boundary.
- Use exact preimage/CAS checks; reject stale menus, changed carriers, duplicate identities and ambiguous state.
- Record durable intent before publishing an irreversible gameplay side effect.
- Define recovery for every crash boundary and prove idempotence by running recovery more than once.
- Assert conservation by exact item identifier, components/NBT and count across source, destination, cursor, overflow, entity and journal—not merely total stack count.
- Fail closed on malformed or missing authority. Never relax proof/lineage validation to make a scenario pass; fix the producer/consumer contract and add a regression.
- Exercise two players, two writers, restart, unload/reload and real process termination where the feature claims those guarantees.

## 7. Commit, PR and merge discipline

The repository's `CONTRIBUTING.md` remains authoritative. In particular, feature/ABI/architecture/dependency changes require maintainer approval through an issue first; one problem per PR; a maximum of three open PRs; engine changes follow the Pumpkin-fork workflow.

For each deliverable:

1. Work on an isolated branch based on current `origin/main` (or the correct engine branch).
2. Stage only exact owned files. Inspect `git diff --cached --stat` and the complete staged diff; verify no unrelated dirty/untracked files entered.
3. Run formatting, focused tests, applicable lint/build and relevant scenarios. State every skipped check.
4. Commit with a scoped conventional message, authored by the human maintainer; no agent/model attribution.
5. Rebase/update against current base, resolve conflicts without overwriting other work, then open/update the PR using `.github/pull_request_template.md`.
6. Include reproduction, root cause, behavior change, tests/log evidence, artifact identity and residual risks. Link the issue and maintainer approval where required.
7. Merge only when the required review/CI and repository policy allow it. Otherwise leave a reviewable PR and a clear owner/next step.
8. Update the issue and acceptance ledger with the commit/PR and exact evidence. A PR being open does not make the feature accepted.

Never commit another worker's changes because they happen to be in the same worktree. If a broad dirty branch cannot be safely split, preserve it and create a path-scoped integration plan; do not force-push or create a giant unreviewable PR.

## 8. Progress reporting cadence

At each meaningful milestone, report in this compact form:

- **Delivered since last update:** exact behavior and files/commit/PR.
- **Evidence:** commands/scenarios, exact artifact identity and observed result.
- **Not yet proven:** explicit gameplay, persistence, crash or client gaps.
- **Blocker/owner:** one actionable dependency, if any.
- **Next slice:** one concrete implementation target, not a broad promise.

Do not report elapsed time, number of agents, number of files or activity as a substitute for shipped behavior. Update the durable issue/ledger before switching tasks or ending a session.

## 9. Throughput decisions

Prioritize in this order:

1. Reproducible player-visible correctness/data-loss/security defects.
2. Shared capability that unblocks several mods, assigned to the integration owner.
3. Complete a narrow player-visible vertical slice through implementation, persistence and scenario proof.
4. Expand to the next highest-frequency BUJA mechanic with verified upstream source parity.
5. Performance optimization after representative behavior is correct; measure before/after on the same machine and workload.

Batch work by dependency and build gate, not by chat arrival order. Avoid broad parallel runtime tests; keep independent source implementation moving while one owner validates. Stop investing in a path when a required upstream license, source artifact, client capability or maintainer decision is genuinely blocked; record the blocker and advance an independent slice.

## 10. Initial execution audit — 2026-11-27

This is a dated incident record, not a permanent statement of current repository state. Re-run the audit before acting on any of these branches or processes.

- `origin/main` was `3ad5ae3`. The shared `noncreate-compass-lookup` worktree was at `65408ec`, **3 commits ahead and 74 behind**. Its three committed changes were a narrow Compass slice, but its working tree also contained a large mixed set of dirty and untracked API, host, mod, script and scenario changes. Those uncommitted changes were not safely attributable or separable as one PR. The correct response was to preserve the checkout and isolate only the three already-committed Compass commits onto a clean current-main branch; never stage or publish the whole worktree.
- The separate `main` worktree was also dirty with Create metal-ladder, host and scenario work. It was left untouched. A third `noncreate-main-integration-20261127` worktree was **68 commits behind and zero commits ahead of `origin/main`**, with a dirty Pumpkin submodule pointer. It was not a useful forward integration branch and must not be mistaken for a deliverable candidate.
- At audit time, multiple `cargo clippy` and `cargo check` processes were compiling `pumpkin-core` and `moddedmc-host` against the same `N:\\mmc-target\\shared\\pack` target, while five server processes referenced the same `cache/create-ladder-final/pack-dev.exe`. Their ownership was not established, so no process was killed and no additional build/package/runtime job was started. The gate was treated as occupied until its owner could confirm release.
- Earlier scenario work had been vulnerable to stale-pack ambiguity. The new requirement is to record the exact source tree, package path/hash, scenario, run directory and server process for every result. A passing scenario without artifact identity is not accepted as evidence for the latest source.
- Delivery was lagging implementation: the current shared implementation work had no corresponding set of reviewable PRs. The first remediation was this playbook PR (#44). The three committed Compass changes were separately transplanted without the dirty working-tree changes onto current `origin/main` and opened as draft PR #45. Static checks passed, but Cargo/clippy/package/live scenarios were explicitly left pending the exclusive gate; the PR was not represented as merge-ready.
- The previous workflow's instruction to clean a checkout at session start was unsafe for a shared worktree. It has been replaced with state inspection, isolated worktrees and scoped cleanup. The previous instruction to push commits directly to `main` was replaced with feature branches and reviewed PRs, consistent with `CONTRIBUTING.md`.

**Follow-up:** update this record only when the state changes materially; put individual scenario experiment details in their run logs and issue comments. Do not turn a dated audit snapshot into an evergreen status claim.
