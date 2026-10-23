# Local read-aloud

This optional feature reads the **displayed text**, locally, using the Apache-2.0
[Kokoro 82M](https://huggingface.co/hexgrad/Kokoro-82M) model, American English
voice `af_heart`. Kokoro is much lighter than voice-cloning/conversational models.
The bundled helper picks its inference backend by platform:

| Platform | Backend | Model |
| --- | --- | --- |
| macOS arm64 | [MLX Audio](https://github.com/Blaizzy/mlx-audio) on the Apple GPU | `mlx-community/Kokoro-82M-bf16` |
| Linux x86_64 | [`kokoro`](https://github.com/hexgrad/kokoro) on PyTorch: CUDA when an NVIDIA GPU is available, else CPU | `hexgrad/Kokoro-82M` |

Both use the same Misaki phonemizer and report the same token timestamps, so word
highlighting behaves identically.

## One-time setup

This fork (`leovicenzi/herdr-file-viewer`) publishes no prebuilt binaries:
`herdr plugin install leovicenzi/herdr-file-viewer` always compiles from source, so it
needs Rust 1.96+ (e.g. `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`).
An install whose viewer ignores `t` likely predates this and runs upstream's binary;
run `cargo build --release` in the plugin directory shown by `herdr plugin list`.

Install `uv` and FFmpeg, then provision the optional Python environment and model
from the plugin's **trusted checkout**, not from a browsed repository:

```sh
brew install uv ffmpeg            # macOS
sudo pacman -S uv ffmpeg portaudio  # Linux (or your package manager)
sh scripts/setup-tts.sh
cargo build --release
```

Setup downloads packages, model weights, voices, and English language data, then
warms the model without playing sound. Python (3.11 on macOS, 3.12 on Linux) is
isolated in `.venv-tts` (gitignored); no system Python packages are changed. On Linux,
setup installs the CUDA 13 PyTorch build (several GB, needs an NVIDIA driver that
supports CUDA 13). Set `TTS_TORCH_INDEX=cpu` before running setup for a much smaller
CPU-only install; Kokoro 82M still runs about real time on a modern CPU, though
first-phrase and uncached-seek latency is higher. Playback
sets `HF_HUB_OFFLINE=1`: it neither downloads data nor sends your documents to a
service. Text/audio remain in memory and are discarded on stop/exit. The weights
are cached by Hugging Face.

On Linux, `HERDR_TTS_DEVICE` (in the helper's environment) overrides the PyTorch
device, e.g. `cpu` to leave a hybrid laptop's discrete GPU asleep. A warm CUDA
context keeps the GPU powered while the helper runs, which is from the first `t` until
the viewer exits (`T` stops playback but keeps the model warm).

The bundled runtime supports **macOS arm64 and Linux x86_64**. Other platforms still
build and browse normally, and can supply another backend using `tts_command`. Missing setup,
missing output devices, and synthesis failures produce an in-app notice rather
than terminating the viewer. The helper starts only on the first explicit read.

## Controls

| Key | Action |
| --- | --- |
| `t` | Read from the **phrase at the top visible line** of the settled active preview; while reading, pause/resume |
| `T` (Shift+`t`) | Stop and clear the highlight; the next `t` starts at the then-visible line |
| `+` / `-` | Adjust speed by 0.1×, from 0.5× to 2.5×, without changing pitch |
| `(` / `)` | Previous / next speech phrase. Previous restarts the current phrase after two audible seconds; otherwise it goes back one |
| `K` / `J` | Previous / next paragraph/block (Shift+`k` / Shift+`j`). No wrap; previous at the first block restarts it |
| `g` | Read from the phrase containing the selection, visible current search match, or viewport top, in that priority order |
| `F` | Toggle follow playback (Shift+`f`); on by default, with its state shown in the status strip |

The status strip reports idle/loading/reading/paused/finished, speed, phrase position, and follow state. With a
backend configured its row stays reserved even while idle, so starting speech or
changing speed cannot resize/reflow the preview and invalidate its coordinates. A shaded
phrase gives context; the current word is yellow/bold. With follow on, the active
preview scrolls when the spoken line leaves the viewport. As with other global
keys, close a finder, search prompt, or help before using these controls.
Line-select mode keeps its fixed `J`/`K` extension controls and adds a fixed local
`g` confirm to read from the selection; other speech controls remain modal-gated.
`Space` retains its existing page-down behavior, not pause.

Speech follows the sanitized renderer output, including rendered Markdown and
code. Syntax-renderer line-number gutters and decorative-only lines are skipped.
In Markdown views, soft-wrapped prose lines are joined with spaces **before**
sentence splitting, so a visual newline does not introduce a mid-sentence pause.
Paragraph breaks, recognizable headings/list-item boundaries, tables, and source
code fences remain separate. The speech-only normalization does not modify the
file or preview; phrase shading and word highlights map back across all original
lines. Source Markdown also respects explicit hard breaks (two trailing spaces
or a backslash). In rendered Markdown, trailing spaces are renderer padding,
not hard-break syntax. Structural detection in rendered output is conservative:
custom renderers can erase boundaries, and a hard break visually identical to a
soft break cannot be recovered from display text alone.
Code/tables/diffs are not otherwise rewritten into prose. Non-Markdown reading
keeps its line-oriented behavior. Sentences longer than the bounded 240-character
phrase limit split after the **last comma that fits**, keeping the comma for
natural intonation (numeric separators such as `3,000` are not clause boundaries).
Without a suitable comma, splitting falls back to a word boundary, or a UTF-8-safe
character boundary for an unbroken long token. Sentences that already fit are not
split at commas. These bounded phrase seams can still introduce a pause.
Text indexing is bounded to 4 MiB / 32768 phrases of the settled preview. Audio
requests use rolling windows of at most 256 KiB / 2048 phrases and automatically
advance to the next window. Only the current/prefetched phrases are synthesized,
not the entire index. For larger configured previews, `g` outside the indexed
prefix opens a new local index there; backward navigation is then limited to
that index. Existing renderer preview/truncation limits still apply.

Changing files, refreshing/re-rendering, switching view/worktree, Markdown
reflow on resize, and handing off to an editor **stop speech**. This is intentional:
displayed coordinates must never be reused against a different document. Starting
from a pinned preview is not supported; switch to the active preview first.

## Navigation and browsing

All seeks preserve speed and **remain paused when paused**. A paused seek shades
the destination phrase without starting audio. While playing, a seek clears the
old audio and starts the destination; cached destinations are faster, but neither
cache hits nor new synthesis promise gapless/instant seeking. Each seek has a new
playback ID, so queued progress from the old position cannot repaint the target.
At the end, the text index remains available for Previous; `T` clears it.

The index normally includes text before the initial starting point, so Previous
can go back before where you first pressed `t`. A phrase is a sentence or a bounded
clause (not necessarily a whole sentence). Paragraph navigation uses display-based
blocks: paragraphs, recognizable headings and list items, and isolated code/table
rows. In non-Markdown views speech still reads individual lines, but blank lines
separate paragraph-navigation groups. Custom renderer output can erase structure;
this is not a Markdown AST/source-position guarantee.

To choose somewhere else while listening:

1. Press `F` to turn follow **off**, so playback does not pull your viewport back.
2. Scroll/search to the passage. Either select text with the mouse, use the current
   visible search result, or leave the desired line at the top of the viewport.
   For keyboard line selection, `Tab` to content and press `L`, then move the marker.
   (`L` switches transformed views to source, which stops existing playback.)
3. Press `g`. Reading begins at the **containing phrase**, which may repeat text
   before your caret; a selection is a starting anchor, not a playback end boundary.
   The selection is consumed so it does not obscure the spoken highlight.
4. Press `F` again if you want automatic following back on. It immediately reveals
   the current phrase, even when paused. Follow is temporarily suppressed while
   a modal or drag owns the browsing cursor.

`n`/`N`, `[`/`]`, and normal arrows keep their existing search/file/scroll meanings.
The new global actions are remappable; line-select's local controls remain fixed.

## Timing and performance

One worker keeps the model warm (MLX or PyTorch) and prefetches at most two phrases (maximum 240
characters each). Initial model/phonemizer loading and the first GPU generation
are slower; subsequent phrases reuse them. No model work runs on the UI thread.
A document-scoped LRU retains at most **16 phrases / 16 MiB of original-speed PCM**,
plus their small alignment maps. This is additional to current audio and two-item
prefetch buffers; no whole-document audio is retained. Rapid seeks coalesce in
both the non-blocking Rust command mailbox and the helper's one-job queue, so
holding a skip key does not accumulate full-text requests. Repeats can reuse cached
synthesis at any playback speed. Stop, document replacement, and process exit
release the cache. An in-flight model call cannot always be interrupted; its
stale output is discarded, but it may briefly delay an uncached destination.
Audio uses 20 ms PortAudio callbacks. Highlight position comes from the audio
output's DAC clock (the helper tracks every buffer queued ahead of the DAC, since
ALSA/PipeWire run two or more blocks ahead where CoreAudio runs about one) and Kokoro's predicted token timestamps, **not words-per-minute
estimates**. Phonemizer-normalized tokens that cannot be mapped back exactly keep
only the phrase highlight; no guessed word is painted.

FFmpeg's `atempo` stretches in-memory audio without changing pitch. A speed change
reprocesses only the unqueued suffix of the current phrase; it does not rerun the
model. There may be a short processing gap, and tempo-stretched word boundaries
are approximate because the audio filter does not produce a forced-alignment map.
Kokoro timestamps themselves are predicted, not a guarantee of phoneme-perfect
alignment. Bluetooth/output-device latency may add perceptual delay.

## Settings and voices

```toml
# Read once at startup; speed adjustments remain session-only.
tts_speed = 1.0
# Optional replacement; quote-aware argv, no shell expansion.
# tts_command = '/absolute/plugin/.venv-tts/bin/python -u /absolute/plugin/scripts/read_aloud.py'
```

`? → Settings` shows both values. `[keys]` supports `toggle_speech`, `stop_speech`,
`speech_faster`, `speech_slower`, `speech_previous_phrase`, `speech_next_phrase`,
`speech_previous_paragraph`, `speech_next_paragraph`, `speech_read_here`, and
`speech_toggle_follow` like other actions.

Set `HERDR_FILE_VIEWER_NO_TTS=1` to disable the optional backend and its idle
status row (also used by non-audio PTY tests, independently of local runtime setup).

The helper accepts `HERDR_TTS_VOICE` and `HERDR_TTS_LANG` in its environment.
For example, Kokoro uses `p` for Brazilian Portuguese and voices such as
`pf_dora`; the language and voice must agree. Only the default English setup is
provisioned/tested by `setup-tts.sh`; install/pre-cache any additional phonemizer
language data explicitly before using another language offline.

## Backend protocol

`tts_command` launches one subprocess with piped stdin/stdout and nulled stderr.
Its working directory is the trusted launch directory, never the viewed tree.
The controller sends JSON lines (text is **never** in argv or interpolated into a
shell command):

```json
{"cmd":"speak","id":3,"document":1,"offset":0,"paused":false,"chunks":[{"text":"Hello.","line":0,"start":0,"end":6}],"speed":1.0}
{"cmd":"pause","id":3,"paused":true}
{"cmd":"speed","id":3,"speed":1.5}
{"cmd":"stop","id":3}
```

A backend emits one JSON object per line on stdout (all diagnostics go to stderr):

```json
{"event":"progress","id":3,"chunk":0,"start":0,"end":5,"elapsed":0.5}
{"event":"done","id":3}
{"event":"error","id":3,"message":"Output device unavailable"}
```

`start`/`end` are optional UTF-8 byte offsets **within the phrase**, not character
indices, and identify the current word. Omitting them highlights only the phrase.
Markdown phrases may span multiple displayed lines; the viewer retains their
byte-exact source map locally. The command's `line`/`start`/`end` metadata describes
only the first displayed run; backends should synthesize `text` and report word
positions relative to that text, without interpreting display coordinates.
`document` scopes cached audio to a settled text index; `offset` identifies the
first phrase in that index. `chunk` in progress is relative to the current request,
not the document. Every seek sends a fresh `speak` with a new `id`, stable document
identity, and the destination offset. **Custom backends must honor `paused` on
`speak` atomically**, without briefly starting audio. `elapsed` is optional audible
seconds within the phrase, excluding pause time and accounting for speed changes;
without it Previous always goes to the preceding phrase. Older input without
`document`/`offset`/`paused` defaults to `id`/0/false in the bundled helper.
Session IDs are opaque; only current-session events are accepted. Invalid indices,
out-of-bounds/non-UTF-8 ranges, stale completions, and unknown events cannot repaint
another document. The owned helper is killed/reaped when the viewer exits.

## Design acceptance checklist

- Read-only: no viewed-file/Git mutations, shell expansion, or document temp files.
- Explicit opt-in provisioning; offline playback, optional runtime, no startup model load.
- Non-blocking UI, bounded requests/prefetch, warm model reuse.
- Pause/resume/stop and bounded pitch-preserving speed adjustment, all keyboard-reachable.
- Phrase/block navigation and selected/search/visible starts; paused seeks never play audio.
- Separate browsing/playback cursors, follow toggle, bounded text windows and raw-audio LRU.
- Display-coordinate phrase and model-timed word highlights; existing selection/search wins.
- Session isolation and cancellation on every content-coordinate change.
- Hermetic Rust/Python tests for chunking, Unicode, state/commands, stale events,
  callback timing/pause, word mapping, and controller cancellation. Real audio/model
  verification is separate from the deterministic test suite.
