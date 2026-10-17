# Findings detail

## Medium: Nested submodule git status runs a clean filter configured only in that submodule

Fingerprint: `src/git.rs:git_command:submodule-only-filter-on-nested-status`

### Trace

1. Entrypoint `src/controller/mod.rs:1184` `Controller::new`. Launch calls `refresh_git_state`. Re-root calls `git.status` from `dispatch_status_refresh` at line 1478. A diff render calls `git.diff` or `git.diff_directory` at lines 1216-1227.
2. Propagation `src/controller/git_apply.rs:169` `refresh_git_state`. Calls `self.git.status()` on launch, refresh, editor return, baseline toggle, and focus-gain.
3. Propagation `src/app.rs:475` `LiveGit::status`. Forwards to `git::status`.
4. Propagation `src/git.rs:79` `status`. Argv is `status --porcelain=v1 -z -uall` with no `--ignore-submodules`. `diff` at line 254 and `diff_directory` at line 311 omit it too. `diff_directory` calls `status` again at line 336.
5. Propagation `src/git.rs:555` `git_command`. Filter overrides cover the opened root only.
6. Propagation `src/git.rs:437` `configured_filter_overrides`. Inventory is `git config --get-regexp ^filter\.` in that root.
7. Propagation `src/git.rs:541` `base_git_command`. Forces `-c diff.submodule=short`. That stops a nested content diff. It is not `--ignore-submodules=dirty`.
8. Sink `src/git.rs:616` `run_bytes`. The spawned git process runs `git status --porcelain=2` inside the populated submodule. That child runs the submodule clean filter.

### Evidence

- `src/git.rs:79`: status argv has no `--ignore-submodules`. Adding the flag stopped the marker.
- `src/git.rs:254`: the same-length fixture's diff created the marker via child `git status --porcelain=2 -uno`.
- `src/git.rs:336`: directory diff also calls `status`.
- `src/git.rs:437`: overrides are computed for the opened root only.
- `src/git.rs:541`: `-c diff.submodule=short` was present on every firing command and stopped `diff.external`, not the clean filter.
- `src/git.rs:577`: `--attr-source` set to the empty tree stopped worktree `.gitattributes` and did not stop `$GIT_DIR/info/attributes`.
- `SECURITY.md:41`: documents this residual.
- `src/controller/git_apply.rs:169`: launch calls `git.status()`.

### Parties and result

- Attacker: author of the opened worktree.
- Affected resource: the operator's account, via the clean command named in the submodule config.
- Native input: submodule `filter.<driver>.clean=<marker>`; `* filter=<driver>` in the submodule git dir `info/attributes`; tracked file changed from `hello\n` to `world\n`.
- Command observed: `git -C <super> -c core.fsmonitor=false -c core.hooksPath=/dev/null -c diff.submodule=short --attr-source=4b825dc642cb6eb9a060e54bf8d69288fbee4904 status --porcelain=v1 -z -uall`
- Observed output: the marker script ran. Child argv was `git status --porcelain=2`. The same diff argv's child was `git status --porcelain=2 -uno`.
- Negative observations: a size-changing edit did not run the filter. Worktree `.gitattributes` did not run it while `--attr-source` was set, and did run it when that flag was omitted. `status --ignore-submodules=dirty` did not run it even with `submodule.<name>.ignore=none`. `diff.external` did not run while `-c diff.submodule=short` was set.

### Conditions

- Populated submodule and a filter command that the opened repository's inventory does not see.
- Attribute source is the submodule `$GIT_DIR/info/attributes`, or a worktree `.gitattributes` when `--attr-source` is omitted.
- File size matches the index and bytes or mtime do not. A size change is reported dirty without running the filter.
- On git 2.43.0 a racy index stat (mtime second equal to the index file) also ran the filter once.

### Remediation

In `git_command_with`, when the subcommand is `status` or `diff`, pass `--ignore-submodules=dirty`. That flag wins over `submodule.<name>.ignore=none`. Keep `-c diff.submodule=short`. Regression: a unix test with a marker clean filter, submodule `info/attributes`, `submodule.<name>.ignore=none`, and a same-length edit must show the marker was not created after `status` and `diff_directory`.
