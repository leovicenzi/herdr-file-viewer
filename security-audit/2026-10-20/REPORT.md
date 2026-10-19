# Security audit report

## Run

- Profile: standard, scoped delta. This is a partial pass.
- Source: `97ba479f6ecd577ca5acaad2f32983c852355f9f` (clean), baseline `f2114a9357021cb7df6851e6d25a2576f29ab5ad`.
- Budget: unset. Agents spent: 4 reconnaissance, 4 hunters (one malformed result was discarded and reassigned), 2 coverage critics, 2 verifiers.
- Execution: sandboxed source and local only. This host cannot enforce the full OS sandbox (no bubblewrap, mount and net namespaces refused, effective capabilities empty, worktree writable). No target process was run. The carried finding was not re-executed.
- Prior runs: `security-audit/2026-10-16` at `d89246d0` and `security-audit/2026-10-17` at `f2114a9`.
- Carried same-source confirmation: `src/git.rs:git_command:submodule-only-filter-on-nested-status`. Not a new finding. Draft PRs [#214](https://github.com/smarzban/herdr-file-viewer/pull/214) and [#215](https://github.com/smarzban/herdr-file-viewer/pull/215) already track fixes. This run opens no new pull request.
- Unchanged rejected claim, suppressed only for that fingerprint: `src/git.rs:git_command:nested-submodule-unhardened-child`. `src/git.rs` still forces `-c diff.submodule=short`.
- Out of scope: install/fetch-or-build, OS opener argv, renderer spawn, update HTTP client, update startup flag, root resolver, worktree picker. Those files are not in the diff.
- Deferred: none. Final-clean critic accepted no new units and no reassignment.

## Posture

The six commits do not change launcher argv, git invocation, path confinement, input decoding, clipboard sanitizing, or modal intent gating. The only production behavior edit collapses narrow-preview focus from three labels to a boolean and keeps the previous two outcomes: tree focus shows the tree, and every other focus shows the active preview. Security regression tests for `core.worktree`, the git config inventory timeout, `diff.submodule=diff` / `diff.external`, and submodule clean filters are not in the diff and still exist.

The previously confirmed submodule-only clean filter remains on this commit. It is the same root cause as the 2026-10-17 record, not a new variant. The durable fix is still one hardened git builder that applies every safe override, including `--ignore-submodules=dirty` on `status` and `diff`, instead of another caller-local patch.

## Confirmed findings

No new confirmed findings.

| Severity | Title | Boundary | Result |
| --- | --- | --- | --- |
| medium (carried) | Nested submodule git status runs a clean filter configured only in that submodule | `git_command` inventories filters for the opened repo only | Prior local run: submodule clean filter ran during launch status. Source path unchanged. Not re-executed here. |

### Carried: submodule-only clean filter

- Location: `src/git.rs` `configured_filter_overrides`, `base_git_command`, `git_command`, `run_bytes`; callers `src/controller/mod.rs` `Controller::new`, `src/controller/git_apply.rs` `refresh_git_state`, `src/app.rs` `LiveGit::status`.
- Principal: author of a worktree the operator opens.
- Reproduction: local superproject with a populated submodule, clean command only in that submodule config, `$GIT_DIR/info/attributes` selecting it, same-size content change. Viewer status argv without `--ignore-submodules`.
- Result observed on git 2.43.0 in the prior run: marker created by child `git status`. `--ignore-submodules=dirty` left the marker absent. `-c diff.submodule=short` stops the separate `diff.external` child and does not stop this status child.
- Impact: command execution as the operator. Overall severity medium because a same-size mismatch plus a submodule-only driver is a narrower precondition than any dirty file. Likelihood medium, impact high.
- Fix: in `git_command_with`, pass `--ignore-submodules=dirty` on `status` and `diff`, and keep `-c diff.submodule=short`. `-c diff.ignoreSubmodules=dirty` is not a substitute. Already proposed on draft PRs #214 and #215.

## Needs validation

None. See `NEEDS-VALIDATION.md`.

## Hardening and positive controls

- Tab launcher comments changed only. `is_flag_safe` still requires a non-empty ASCII token of `[A-Za-z0-9_.:-]` that does not start with `-`, and both shell scripts pass the id as one quoted argument.
- `decode`, `resolve_bindings`, and `REGISTRY` are unchanged. `intent_effects_never_mutate_files_or_git_and_classify_annotation_edits` still marks every intent non-mutating. The removed `decode_over_default_bindings_agrees_with_map_key` test restated `map_key` versus `decode` on default bindings. `all_lists_every_variant_once` still requires `Intent::ALL` to match the enum once each.
- The removed `help_composition_timeout_is_the_single_200ms_budget` test asserted a 200ms constant. It was not the modal gate. Open modals still return before the global intent match.
- `copy_line_reference`, `copy_path`, and OSC 52 still sanitize or base64-encode before the terminal. Comment deletions were not the guard.
- `SECURITY.md` still documents the submodule-only filter residual.

## Coverage

| Status | Count |
| --- | --- |
| covered | 6 |
| candidate (carried confirmation) | 1 |
| out of scope | 7 |
| blocked | 0 |
| deferred | 0 |

Post-wave critic and final-clean critic both returned no missing units and no reassignment. The git unit was reserved for the Phase 3 verifier and was not given to a hunter, so that root cause was not re-filed.
