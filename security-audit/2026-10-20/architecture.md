# Architecture (delta run 3)

Scoped standard audit of `smarzban/herdr-file-viewer` at `97ba479f6ecd577ca5acaad2f32983c852355f9f` (clean), baseline `f2114a9357021cb7df6851e6d25a2576f29ab5ad`. Partial pass: only the six-commit delta and the security paths those edits touch. Budget unset. Execution is sandboxed-source-and-local-only; this host cannot enforce the full OS sandbox (no bubblewrap, `unshare --net` and `--mount` fail, `CapEff` empty, target is writable), so hunters do not run target code.

## Product and principals

herdr-file-viewer is a read-only, keyboard-driven TUI plugin. The local operator views a git worktree. The lower-trust principal is the author of that worktree (repo config, attributes, file bytes). herdr supplies pane context and `pane list` JSON. Protected resources are the operator account (subprocess execution), files outside the opened root, and the terminal (escape/OSC). The plugin does not authenticate peers; trust is the OS user who launched herdr.

Comparable baseline, from repo docs: a pager-style viewer (glow/delta/bat) plus read-only `git` CLI, not libgit2. Same-account self-effect is not a cross-user finding. Repo-controlled git config that runs a command as the operator is the historical defect class.

## Stack and build limits

Rust 2024 / 1.96, ratatui, crossterm, ansi-to-tui, `ignore`, serde. Entry: `src/main.rs` to `run`, `--launch-decision-tab`, `--pick-root`. Install and release scripts are outside this diff. Offline `cargo test` would write `target/`; it is prohibited here because the sandbox controls are incomplete. No live herdr, curl, or release workflow.

## Delta

`git diff --name-status f2114a9..97ba479` touches 30 files. Executable changes are confined to:

- `src/preview_layout.rs` and `src/presenter.rs`: `PreviewFocus::{Tree,Pinned,Active}` becomes `narrow_shows_tree`. Narrow width still shows the tree only when focus is the tree; pinned and content focus both show the active column, matching the old `Pinned | Active` arm (`src/preview_layout.rs` `carve_unpinned`, `src/presenter.rs` `structural_layout`).
- Tests: `src/input.rs` drops `decode_over_default_bindings_agrees_with_map_key`; `src/controller/mod.rs` drops `help_composition_timeout_is_the_single_200ms_budget`; `src/intent.rs` replaces per-variant `Intent::ALL` tests with one exhaustive match. `intent_effects_never_mutate_files_or_git_and_classify_annotation_edits` remains.

Launcher scripts `scripts/open-file-viewer-tab.sh` and `.ps1` change header comments only. The shell still pipes `herdr pane list` into `--launch-decision-tab` and branches on `SWITCHTAB`/`FOCUS`/`CLOSE` (`scripts/open-file-viewer-tab.sh`). `src/controller/git_apply.rs`, `src/app.rs`, help, lineselect, infile, mouse, and picker are comment-only. `src/git.rs`, `src/root.rs`, `src/launch.rs`, `src/proc.rs`, and the security tests in `tests/git_baseline.rs`, `tests/git_attr_source_compat.rs`, and `tests/root.rs` are not in the diff.

## Trust boundaries in scope

- Tab launcher: pane-list JSON to herdr argv. Control: viewer `is_flag_safe` before the shell interpolates ids.
- Key decode and intent table: operator keys and `[keys]` config. Control: decode stays inside `Intent`; the read-only matrix is still the mutation gate.
- Narrow layout: same-operator pane visibility. No new subprocess.
- Clipboard and modal paths in files the diff comments: prior controls (sanitize before copy; modals drop leaked intents) are unchanged in executable lines.
- Git hardening: carried same-source confirmation. `git_command` still does not override a clean filter that exists only in a populated submodule. Hunters must not re-file that root cause. This run cannot re-execute the prior marker.

## Prior runs

`security-audit/2026-10-16` (d89246d0) and `security-audit/2026-10-17` (f2114a9). Carried confirmed, same source: `src/git.rs:git_command:submodule-only-filter-on-nested-status`. Unchanged rejection, claim suppressed only: `src/git.rs:git_command:nested-submodule-unhardened-child` (`-c diff.submodule=short` still present; `src/git.rs` not in the diff). Prior out-of-scope launch and input units are in scope because the launcher comments and input/intent tests changed. Install, update HTTP, renderer spawn, opener argv, root resolver, and worktree picker stay out of scope; their security tests were not edited. No needs_validation records were in the prior findings files.

## Companions

Selected because a real boundary is present, not because the language is Rust: `DESKTOP-MOBILE-AND-LOCAL-IPC.md` privileged-helper/local-file for launcher argv and for git/clipboard process and display sinks; `RESOURCE-EXHAUSTION-AND-AVAILABILITY.md` computational amplification and failure/recovery on the carried git unit (config inventory and child git). Excluded: HTTP, browser DOM, LLM tools, cloud IAM, RPC brokers, supply-chain release (install scripts not in the diff), memory-safety (no new unsafe), and tenant isolation (one local operator).
