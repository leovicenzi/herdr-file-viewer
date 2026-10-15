# Architecture summary (scoped delta)

Run profile: standard, scoped to the diff `3996b88d6c6ca5608872abedf05067a803a168ec..d89246d006e563f16b577468f04e13365d57fce6`. Source ref `d89246d006e563f16b577468f04e13365d57fce6`, clean worktree. No compatible prior `coverage-ledger.json` or `findings.json` was found on `security-audit/*` branches, in git history, or in local stores. Merged fixes #206 (`core.worktree` containment) and #207 (bounded filter-config inventory) are revalidation targets, not carried confirmations. This is a partial pass.

## Product and principals

herdr-file-viewer is a local, keyboard-driven, read-only TUI plugin. The operator (and herdr, running as that user) opens a worktree that may be hostile. Protected resources are the viewed tree boundary, the operator's other files, and the process integrity of git/renderer/editor helpers. Lower-trust principals: repository files and git config, host context JSON, and remote notice bytes. The operator's config, `$EDITOR`, and herdr binary are higher trust. There is no cross-user authentication. A comparable baseline is a pager (`less`/`bat`) plus git's own safe-directory posture: untrusted repo config must not run helpers or retarget the worktree. That shared pattern is why filter and `core.worktree` controls exist; it does not dismiss a demonstrated bypass.

## Stack and offline limits

Rust 1.96, ratatui, crossterm, ansi-to-tui, `ignore`, serde. Git, optional glow/delta/bat, and optional curl are external CLIs. Deployment is a herdr pane command `./target/release/herdr-file-viewer` or `cargo run`. `cargo test` can run offline once deps are present; this run does not execute the target until a candidate needs a sandboxed local check. Full OS sandbox (empty env, read-only target, all resource limits) was not established as a standing execution environment. `unshare` user/net/mount is available on the host; that fact is outside the repository.

## Entry surfaces and sinks

Launch context (`HERDR_PLUGIN_CONTEXT_JSON`, `HERDR_FILE_VIEWER_ROOT`) selects the cwd. `root::resolve` accepts a git toplevel only when it canonically matches the directory whose `.git` entry resolves to git's `--git-dir` (`src/root.rs`). File reads and git pathspecs stay under that root (`src/render.rs` `classify`, `src/git.rs` `is_within_root`). Every repo git command goes through `git_command`: scrubbed redirect env, `core.fsmonitor=false`, `core.hooksPath` on the null device, filter drivers forced to empty non-required commands, optional `--attr-source` empty tree, and `--no-ext-diff`/`--no-textconv` on diffs. Filter names are inventoried with a 2s deadline and 1 MiB cap, fail-closed. Renderers, the editor, the opener, and herdr run via `proc::in_launch_dir` so the untrusted cwd is not their working directory. Content is neutralized before display. Update notices use fixed HTTPS URLs; the delta only changes that startup uses the already-resolved `update_check` flag.

## Trust boundaries

Strongest source-visible controls: absolute-only config load (`src/config.rs`); root equality check (`src/root.rs`); single git hardening builder (`src/git.rs`); within-root canonical path checks; stdin-fed renderers and ANSI neutralization; pane id `is_flag_safe` (unchanged, out of scope). Documented residual: concurrent config mutation between inventory and query (`SECURITY.md`).

## Starting paths

`src/root.rs`, `src/git.rs`, `src/proc.rs`, `src/render.rs`, `src/config.rs`, `src/update/mod.rs`, `src/editor.rs`, `src/herdr.rs`, `src/controller/mod.rs`, `src/controller/lineselect.rs`, `SECURITY.md`, `tests/root.rs`.

## Prior coverage

No prior ledger. Do not treat #206 or #207 as new findings if the current source still enforces the same invariant. Revalidate both. Comment-only edits in the delta (help text, dead helpers, dropped error strings) are in scope only to confirm they did not remove a control.

## Companion selection

Selected: `DESKTOP-MOBILE-AND-LOCAL-IPC.md` for untrusted local files and helper processes (root binding, gitfile reads, launch-dir). `RESOURCE-EXHAUSTION-AND-AVAILABILITY.md` for the filter-inventory deadline and caps. Ordinary classes: Resource and file handling, Injection, Business logic, Chained vulnerabilities, Obvious things.

Excluded from assignment: `WEB-PROTOCOL-AND-AUTH.md`, `CLIENT-SIDE.md`, `AI-AND-LLM.md`, `CLOUD-AND-DEPLOYMENT.md`, `PROTOCOLS-RPC-AND-MESSAGING.md` (no HTTP parser, browser, model, or cloud control plane in the delta). `SUPPLY-CHAIN-AND-RELEASE.md` (install/CI not in the diff). `MEMORY-SAFETY-AND-BINARY.md` (no new `unsafe`/FFI). `DATA-ISOLATION-AND-LIFECYCLE.md` (single operator, no tenant store). Those surfaces are `out_of_scope`, not covered.
