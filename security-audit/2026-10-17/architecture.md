# Architecture summary (scoped delta)

Run profile: standard, scoped to `d89246d006e563f16b577468f04e13365d57fce6..f2114a9357021cb7df6851e6d25a2576f29ab5ad`. Source ref `f2114a9357021cb7df6851e6d25a2576f29ab5ad`, clean worktree. Prior ledger and findings: `origin/security-audit/2026-10-16` (`security-audit/2026-10-16/`). No earlier `security-audit/*` branch exists. This is a partial pass.

## Product and principals

herdr-file-viewer is a local, keyboard-driven, read-only TUI. The operator opens a worktree that may be hostile. Protected resources are the viewed tree, the operator's other files, and the process integrity of git and helper programs. Lower-trust principal: repository files and git config, including a populated submodule's config and `$GIT_DIR`. The operator's config, `$EDITOR`, and herdr are higher trust. There is no cross-user authentication. A comparable baseline is a pager plus git's own posture that untrusted repo config must not run helpers. That shared pattern is why filter, ext-diff, and `core.worktree` controls exist.

## Stack and offline limits

Rust 1.96, ratatui, crossterm, system git. `cargo test` can run offline once deps are present. This run executed system git 2.43.0 only, under `unshare --user --map-root-user --net --mount`, an empty allowlisted environment (`PATH`, `HOME`, `TMPDIR`), and ulimit CPU, address, file, and process caps. The target crate was not executed. Full standing OS sandbox for arbitrary target builds was not used.

## Entry surfaces and sinks

Launch calls `refresh_git_state`, which calls `git.status` (`src/controller/git_apply.rs`). Diffs go through `git.diff` and `git.diff_directory`. Every repo git command uses `git_command`: scrubbed redirect env, `core.fsmonitor=false`, `core.hooksPath` on the null device, filter drivers of the opened repo forced empty, optional `--attr-source` empty tree, `--no-ext-diff`/`--no-textconv` on diffs, and, since #208, `-c diff.submodule=short`. Filter inventory stays fail-closed at 2s and 1 MiB. Root equality (`src/root.rs`) is unchanged.

## Trust boundaries

Strongest source-visible controls: absolute config load; root canonical equality; the single git builder; within-root path checks. #208 closes the nested `git diff --submodule=diff` that ran `diff.external`. It does not stop Git from spawning `git status` inside a populated submodule, and that child still runs a submodule-only clean filter.

## Starting paths

`src/git.rs`, `src/root.rs`, `src/worktree.rs`, `src/controller/mod.rs`, `src/controller/git_apply.rs`, `src/app.rs`, `SECURITY.md`, `tests/git_baseline.rs`.

## Prior coverage

Carried `src/git.rs:git_command:nested-submodule-unhardened-child` as changed-source revalidation, not a hunter exclusion. It does not hold: `-c diff.submodule=short` stops that child. #206 (`core.worktree` equality) and #207 (filter inventory fail-closed) still hold. Prior covered units whose files are outside this diff stay visible as out of scope. No prior needs-validation records existed.

## Companion selection

Selected: `DESKTOP-MOBILE-AND-LOCAL-IPC.md` for untrusted local git config and helper processes. `RESOURCE-EXHAUSTION-AND-AVAILABILITY.md` for the filter-inventory deadline. Ordinary classes: Injection on git, Resource and file handling on root, Business logic on worktree selection and modal gating, Obvious things on the effect-fold refactor.

Excluded from assignment: web, client, AI, cloud, RPC, supply-chain, memory-safety, and tenant-isolation companions. Those surfaces are out of scope, not covered.
