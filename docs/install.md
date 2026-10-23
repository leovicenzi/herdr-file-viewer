# Install & updating

Requirements: **herdr 0.7.5+**, on **Linux** or **macOS** (native Windows
`x86_64-pc-windows-msvc` is a [preview](windows.md)). **Git** must be on `PATH` at
runtime. The viewer shells out to the system `git` CLI (read-only subcommands) for the
git-aware tree (status markers, changed-only filter, baseline toggle) and the diff view.
Without git the viewer still opens, but those features are degraded (no status colors, no
diffs). git 2.39 (Apple's Xcode git) is supported. The optional renderers (`glow` / `delta` /
`bat`) are separate.
The system `curl` is optional: without it, document retrieval is unavailable without an error.
See [external renderers](renderers.md).

> **This fork builds from source.** `herdr plugin install leovicenzi/herdr-file-viewer` compiles
> with `cargo` (Rust 1.96+), because the fork publishes no prebuilt binaries. The installer would
> download a prebuilt, SHA-256-verified binary matching the **version the source declares**, but it
> looks for releases in this fork, where there are none. Upstream's same-version prebuilt is never
> used, since it lacks [read-aloud](read-aloud.md).

**Install through herdr.** At install time, herdr runs the manifest's `[[build]]` step. The step
downloads a prebuilt binary or compiles from source and produces
`./target/release/herdr-file-viewer`, which the viewer pane launches:

```bash
# install (and update, re-run any time to get the latest):
herdr plugin install leovicenzi/herdr-file-viewer
# …optional: pin a specific older version for reproducibility:
herdr plugin install leovicenzi/herdr-file-viewer --ref v1.0.0

# or, for local development, link this checkout in place:
cargo build --release            # plugin link does NOT run the [[build]] step, so build first
herdr plugin link /path/to/herdr-file-viewer
```

> You don't need `--ref` to stay current. A bare install pulls the latest. See [Updating](#updating).

Confirm it registered with `herdr plugin list`. To build manually outside herdr:

```bash
cargo build --release
```

## After installing

herdr's install output is intentionally terse (`Installed …` / `Config: …`) and won't prompt you,
so two quick steps remain:

1. **Bind a key** to summon the viewer. See [Quick start](../README.md#quick-start) (or
   [Summoning the viewer](summoning.md) for split-vs-tab and the `--remote` caveat). No key bound
   yet? Open it once from the CLI:
   `herdr plugin action invoke open-file-viewer --plugin herdr-file-viewer`.
2. **(Optional) install the renderers** (`glow` / `delta` / `bat`) so markdown, diffs, and code are
   styled instead of plain text. See [external renderers](renderers.md). The viewer works without
   them (plain-text fallback).

## Updating

herdr has no plugin auto-update. An advisory status row can point to `?` for release details.
In this fork that check queries `leovicenzi/herdr-file-viewer`, never upstream (an upstream notice
would tell you to install upstream and lose read-aloud). The fork has no releases newer than the
version it declares, so the row stays quiet. Installation remains manual: re-run the install
yourself to get the latest:

```bash
herdr plugin install leovicenzi/herdr-file-viewer
```

- You **don't** need `--ref` to stay current; it only *pins* a specific version (and a pin stays
  pinned until you change it).
- Want a heads-up the moment a release ships? On GitHub, select **Watch**, then **Custom**, then
  **Releases**.
- Prefer no remote notices? Set [`update_check = false`](configuration.md), or set
  `HERDR_FILE_VIEWER_NO_UPDATE_CHECK` when the config key is absent. The check otherwise runs at
  most once per 24h and never blocks the viewer when offline.
