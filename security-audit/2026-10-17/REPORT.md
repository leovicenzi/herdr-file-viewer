# Security audit report

## Run

- Profile: standard, scoped to the diff `d89246d006e563f16b577468f04e13365d57fce6..f2114a9357021cb7df6851e6d25a2576f29ab5ad`. This is a partial pass.
- Source ref: `f2114a9357021cb7df6851e6d25a2576f29ab5ad`, clean worktree.
- Budget: unset. Agents used: 1 reconnaissance, 2 hunters, 1 post-wave critic, 1 final-clean critic, 2 candidate verifiers (the first record was discarded because it did not match the schema), 1 record verifier.
- Execution: sandboxed source and local git fixtures only. No live services. Marker checks ran system git 2.43.0 under a user, mount, and network namespace, with an empty allowlisted environment and resource limits. The crate was not executed.
- Prior run: `security-audit/2026-10-16` at `d89246d0`. No earlier `security-audit/*` branch. Carried confirmation `src/git.rs:git_command:nested-submodule-unhardened-child` was revalidated on changed source and rejected: `-c diff.submodule=short` stops that nested diff. #206 (`core.worktree` equality) and #207 (filter-inventory fail-closed) still hold.
- Out of scope: prebuilt install, the update HTTPS client, launcher pane-id checks, the OS opener, and prior covered paths whose files are not in this diff (renderer launch directory, input registry, update-check startup).
- Deferred: none. Both critics returned no in-scope missing units and no reassignments.

## Posture

#208 closes the submodule `diff.external` child. One new medium finding remains: Git still spawns `git status` inside a populated submodule, and that child runs a clean filter configured only in the submodule.

## Confirmed findings

| Severity | Title | Boundary | Observed result |
|---|---|---|---|
| medium | Nested submodule git status runs a clean filter configured only in that submodule | Opened-repo filter inventory vs Git's submodule status child | Marker script from the submodule clean filter ran under the viewer's status and diff argv |

### Nested submodule git status runs a clean filter configured only in that submodule

- Location: `src/git.rs` `status`, `diff`, `diff_directory`, and `git_command`, reached from `refresh_git_state`.
- Lower-trust principal: author of the opened worktree.
- Reproduction: submodule-only `filter.<driver>.clean` set to a marker, `* filter=<driver>` in the submodule `$GIT_DIR/info/attributes`, tracked file changed from `hello\n` to `world\n`, then the viewer argv `git -C <super> -c core.fsmonitor=false -c core.hooksPath=/dev/null -c diff.submodule=short --attr-source=<empty-tree> status --porcelain=v1 -z -uall`.
- Conditions: populated submodule; the driver is absent from the superproject filter inventory; info/attributes (or, without `--attr-source`, worktree `.gitattributes`); file size matches the index. A size-changing edit did not run the filter. `submodule.<name>.ignore=none` defeats `-c diff.ignoreSubmodules=dirty` and does not defeat the command flag `--ignore-submodules=dirty`.
- Result: the marker ran. The child was `git status --porcelain=2` (diff uses `--porcelain=2 -uno`). With `-c diff.submodule=short`, a submodule `diff.external` did not run.
- Impact: the repository-chosen command runs as the operator. Likelihood medium, impact high, overall medium.
- Fix: pass `--ignore-submodules=dirty` from `git_command_with` on `status` and `diff`. Keep `-c diff.submodule=short`.

## NEEDS VALIDATION

None from this pass. The prior run had no needs-validation records.

## Hardening and positive patterns

- Git 2.43.0 does not let `alias.status`, `alias.diff`, or `alias.config` replace those builtins. The viewer only invokes builtins.
- `agent_active` still returns a path only after canonical equality with a listed worktree. The flatten does not change the tiers.
- `handle` now matches every modal, including discard confirm, and drops leaked intents.
- `apply_input_effects` keeps clear-then-quit-then-redraw. Mouse input still cannot quit.
- Positive: #206 toplevel equality still holds. #207 inventory still fails closed at 2 seconds and 1 MiB. #208 stops `diff.external`.

## Coverage

| Status | Count |
|---|---|
| covered | 5 |
| candidate (confirmed finding) | 1 |
| blocked | 0 |
| deferred | 0 |
| out of scope | 7 |

Final-clean critic: no further in-scope units.
