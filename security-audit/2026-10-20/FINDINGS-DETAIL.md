# Findings detail

No new confirmed findings.

The record below is the carried same-source confirmation `src/git.rs:git_command:submodule-only-filter-on-nested-status` from `security-audit/2026-10-17`. Overall severity remains medium. This run re-read the path at `97ba479f6ecd577ca5acaad2f32983c852355f9f` and did not re-execute it. Draft fixes already exist: pull requests 214 and 215.

## Nested submodule git status runs a clean filter configured only in that submodule

Lower-trust principal: the author of a worktree the operator opens. Affected resource: the operator account, via a clean filter command.

### Trace

1. Entrypoint `src/controller/mod.rs:1181` `Controller::new`. Launch calls `refresh_git_state`. Re-root calls `git.status` from `dispatch_status_refresh` at line 1474. A diff render calls `git.diff_directory` at line 1218 or `git.diff` at line 1223.
2. Propagation `src/controller/git_apply.rs:168` `refresh_git_state`. After the repo guard, calls `self.git.status()` on the opened root.
3. Propagation `src/app.rs:475` `LiveGit::status`. Forwards to `git::status`. `diff` and `diff_directory` forward the same way.
4. Propagation `src/git.rs:79` `status`. Argv is `status --porcelain=v1 -z -uall` through `run_bytes`, with no `--ignore-submodules`. `diff` at line 254 and `diff_directory` at line 311 omit that flag. `diff_directory` calls `status` again at line 336.
5. Propagation `src/git.rs:555` `git_command`. Loads filter overrides for the opened root only, then adds `--attr-source` when supported and the `-c filter.*` overrides. A submodule-only driver is absent.
6. Propagation `src/git.rs:437` `configured_filter_overrides`. Inventory is `git config --get-regexp ^filter\.` in the opened root.
7. Propagation `src/git.rs:541` `base_git_command`. Sets `GIT_OPTIONAL_LOCKS=0`, clears repo-redirecting env, sets `core.fsmonitor=false` and `core.hooksPath` to the null device, and forces `-c diff.submodule=short`.
8. Sink `src/git.rs:616` `run_bytes`. Spawns that git command. On git 2.43.0 the process spawns `git status --porcelain=2` in the populated submodule. The child runs the submodule clean filter while hashing a same-size file selected by `info/attributes`.

### Evidence

- `src/git.rs:79`. Prior replay of the status argv created the marker. Adding `--ignore-submodules=dirty` left it absent.
- `src/git.rs:254`. The diff argv created the marker through child `git status --porcelain=2 -uno`.
- `src/git.rs:336`. Directory diff calls `status`, so the nested spawn is on that path too.
- `src/git.rs:437`. Overrides are computed for the opened root only.
- `src/git.rs:541`. `-c diff.submodule=short` was present. It silenced `diff.external` and left the clean-filter child as `git status`.
- `src/git.rs:577`. `--attr-source` set to the empty tree stopped worktree `.gitattributes` and did not stop `info/attributes`.
- `SECURITY.md:41`. Documents this residual.
- `src/controller/git_apply.rs:168`. Launch refresh calls status on the superproject, so the operator does not have to open the submodule file.

### Bounded local reproduction (prior run, not repeated)

Dummy attacker: author of the opened worktree. Dummy resource: a marker file written by the submodule clean command.

1. Create a local superproject with a checked-out submodule. Put the clean command only in the submodule config and `* filter=<driver>` in that submodule's `$GIT_DIR/info/attributes`. Change a tracked file without changing its size.
2. Run `git -C <super> -c core.fsmonitor=false -c core.hooksPath=/dev/null -c diff.submodule=short --attr-source=4b825dc642cb6eb9a060e54bf8d69288fbee4904 status --porcelain=v1 -z -uall`.
3. Repeat with `diff --no-ext-diff --no-textconv --no-color HEAD --`. Repeat status with `--ignore-submodules=dirty` after `submodule.<name>.ignore=none`. Compare `diff.external` with and without `-c diff.submodule=short`.

Observed: with `info/attributes` and a same-length edit, the marker was created. The child was `git status --porcelain=2`. The marker was absent for a size-changing edit, for worktree `.gitattributes` while `--attr-source` was the empty tree, for `status --ignore-submodules=dirty` despite `ignore=none`, and for `diff.external` while `-c diff.submodule=short` was set. This run did not re-execute; `src/git.rs` is unchanged since that observation.

### Conditions

Populated submodule, clean driver only in the submodule config, `info/attributes` selecting it, and a same-size content or mtime mismatch. A size change does not run the filter. System git that implements submodule dirtiness by spawning `git status` in the submodule (observed on 2.43.0).

### Remediation

In `git_command_with`, after the `status` or `diff` subcommand, pass `--ignore-submodules=dirty`. Keep `-c diff.submodule=short`. Do not use `-c diff.ignoreSubmodules=dirty` in place of the argv flag: `submodule.<name>.ignore=none` defeats that config key. Regression: a submodule-only clean filter and a same-size edit must not create the marker on status or diff, and a changed gitlink commit must still be reported.
