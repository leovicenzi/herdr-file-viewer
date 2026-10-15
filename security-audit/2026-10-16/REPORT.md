# Security audit report

## Run

- Profile: standard, scoped to the diff `3996b88d6c6ca5608872abedf05067a803a168ec..d89246d006e563f16b577468f04e13365d57fce6`. This is a partial pass.
- Source ref: `d89246d006e563f16b577468f04e13365d57fce6`, clean worktree.
- Budget: unset. Agents used: 4 reconnaissance, 3 hunters, 1 post-wave critic, 1 final-clean critic, 1 candidate verifier, 1 record verifier.
- Execution: sandboxed source and local fixtures only. No live services. The decisive submodule check ran `git` 2.43.0 against dummy repositories under a user, mount, and network namespace, with a marker script that only appended a line in the scratch directory.
- Prior runs: no compatible `coverage-ledger.json` or `findings.json` on `security-audit/*` branches, in git history, or in local stores. Merged fixes #206 (`core.worktree`) and #207 (filter-config inventory) were revalidated in source. #207's fail-closed timeout still holds. #206's toplevel equality still holds. They are not new findings.
- Out of scope (unchanged in this diff): prebuilt install (`scripts/fetch-or-build.sh`), the update HTTPS client (`src/update/gateway.rs`), launcher pane-id checks (`src/launch.rs`), and the OS opener (`src/opener.rs`).
- Deferred: none. Both critics returned no missing units and no reassignments.

## Posture

The delta is mostly comment and dead-helper cleanup plus two merged hardenings. One new high finding remains: a submodule content diff spawned by Git does not inherit `--no-ext-diff`, so a repository `diff.external` still runs when the operator views a directory diff.

## Confirmed findings

| Severity | Title | Boundary | Observed result |
|---|---|---|---|
| high | Submodule diff runs repository diff.external despite --no-ext-diff | `git_command` top-level argv vs Git's submodule child | Marker script from the submodule `diff.external` ran under the viewer's `--no-ext-diff` argv |

### Submodule diff runs repository diff.external despite --no-ext-diff

- Location: `src/git.rs` `git_command` / `base_git_command`, reached from `diff` and `diff_directory`.
- Lower-trust principal: author of the opened worktree.
- Reproduction: superproject `diff.submodule=diff`, submodule `diff.external` set to a marker script, submodule file modified, then `git -C <super> -c core.fsmonitor=false -c core.hooksPath=/dev/null --attr-source=<empty-tree> diff --no-ext-diff --no-textconv --no-color HEAD --`.
- Conditions: checked-out dirty submodule; the diff pathspec is the submodule directory or the whole tree (git-status mode on a directory). A single-file pathspec inside the submodule did not spawn the child.
- Result: the marker ran. The child argv was `git diff --submodule=diff` without `--no-ext-diff`. With `--attr-source` omitted (git older than 2.40), a submodule `diff.<driver>.textconv` also ran. `-c diff.submodule=short` stopped the marker.
- Impact: the repository-chosen command runs as the operator viewing the diff. Likelihood and impact are high. Overall severity is high.
- Fix: in `base_git_command`, pass `-c diff.submodule=short` so command-line config overrides the repo and Git does not spawn the inner content diff.

## NEEDS VALIDATION

None from this pass. No prior ledger was available, so earlier needs-validation items were not re-checked by fingerprint and are not listed as open leads.

## Hardening and positive patterns

- `gitfile_target` reads a `.git` gitfile with no size cap. That can allocate in the operator's own viewer process. It does not cross into another principal, so it is not a finding.
- `trusted_toplevel` returns Git's pre-canonical path after the canonical paths match. A later symlink swap is a check-to-use window without a demonstrated cross-account result.
- Positive: filter inventory still fails closed at 2 seconds and 1 MiB (#207). A foreign `core.worktree` still degrades to the opened directory (#206). Renderers stay on stdin with a launch-directory cwd. Config is read only from an absolute path. Copy paths still strip terminal controls before OSC 52.

## Coverage

| Status | Count |
|---|---|
| covered | 5 |
| candidate (confirmed finding) | 1 |
| blocked | 0 |
| deferred | 0 |
| out of scope | 4 |

Final-clean critic: no further in-scope units.
