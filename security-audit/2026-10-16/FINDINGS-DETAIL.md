# Findings detail

## High: Submodule diff runs repository diff.external despite --no-ext-diff

Fingerprint: `src/git.rs:git_command:nested-submodule-unhardened-child`

### Trace

1. Entrypoint `src/controller/mod.rs:3713` `Controller::dispatch_render`. Git-status mode on a directory selects a working-tree diff whose pathspec covers that directory.
2. Propagation `src/controller/mod.rs:1220` `Controller::spawn_worker`. The render worker calls `diff_directory`. An empty relative path diffs the whole tree.
3. Propagation `src/app.rs:505` `LiveGit::diff_directory`. Forwards to `git::diff_directory`.
4. Propagation `src/git.rs:309` `diff_directory`. Argv is `diff --no-ext-diff --no-textconv --no-color <rev> --`.
5. Sink `src/git.rs:545` `git_command`. Adds `core.fsmonitor=false`, `core.hooksPath`, optional `--attr-source`, and filter overrides. It does not set `diff.submodule`.

### Evidence

- `src/git.rs:252` and `src/git.rs:309`: `--no-ext-diff` and `--no-textconv` are flags of the parent process only.
- `src/git.rs:530`: `base_git_command` does not set `diff.submodule`.
- `src/git.rs:566`: `--attr-source` is version-gated. It suppressed attribute textconv in the child and did not stop `diff.external`.
- `src/git.rs:1031`: the guard test does not require a `diff.submodule` override.
- `SECURITY.md:38`: queries are documented to refuse diff and textconv programs.

### Parties and result

- Attacker: author of the opened worktree.
- Affected resource: the operator's account, via a command the submodule config names.
- Native input: superproject config `diff.submodule=diff`; submodule config `diff.external=<marker script>`; a modified tracked file in the checked-out submodule.
- Command observed: `git -C <super> -c core.fsmonitor=false -c core.hooksPath=/dev/null --attr-source=4b825dc642cb6eb9a060e54bf8d69288fbee4904 diff --no-ext-diff --no-textconv --no-color HEAD --`
- Observed output: the marker script ran. Child argv was `git diff --submodule=diff` without `--no-ext-diff`. The child environment still had `core.fsmonitor=false` and `core.hooksPath=/dev/null`.
- Additional observation: without `--attr-source`, submodule `diff.<driver>.textconv` also ran. With `-c diff.submodule=short`, the marker did not run and Git printed only the short dirty-gitlink summary.

### Conditions

- Repository config sets `diff.submodule=diff` and the submodule sets `diff.external`.
- The submodule is checked out and dirty.
- The viewed diff pathspec is the submodule directory or the whole tree.

### Remediation

In `base_git_command`, pass `-c diff.submodule=short`. That override wins over the repository value, so Git does not spawn the inner content diff. Regression: a unix test with a marker `diff.external` in a submodule must show the marker was not created after `diff_directory`. Extend `git_command_applies_every_untrusted_repo_guard` to require the new argument.
