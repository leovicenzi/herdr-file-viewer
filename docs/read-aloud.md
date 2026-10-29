# Local read-aloud

This optional feature reads the **displayed text**, locally, using the Apache-2.0
[Kokoro 82M](https://huggingface.co/hexgrad/Kokoro-82M) model, in **English, Brazilian
Portuguese, Spanish, French, and Italian** (see [Languages](#languages)). Kokoro is much lighter than
voice-cloning/conversational models.
The bundled helper picks its inference backend by platform:

| Platform | Backend | Model |
| --- | --- | --- |
| macOS arm64 | [MLX Audio](https://github.com/Blaizzy/mlx-audio) on the Apple GPU | `mlx-community/Kokoro-82M-bf16` |
| Linux x86_64 | [`kokoro`](https://github.com/hexgrad/kokoro) on PyTorch: CUDA when an NVIDIA GPU is available, else CPU | `hexgrad/Kokoro-82M` |

Both use the same Misaki phonemizer and report the same token timestamps, so word
highlighting behaves identically.

## Languages

The language is detected automatically, **per block** (paragraph, heading, list item,
table or code row), so a document can mix sections in any of these languages:

| Language | Voice | Phonemizer |
| --- | --- | --- |
| English | `af_heart` | Misaki |
| Brazilian Portuguese | `pf_dora` | espeak-ng |
| Spanish | `ef_dora` | espeak-ng |
| French | `ff_siwis` | espeak-ng |
| Italian | `if_sara` | espeak-ng |

- A block of six or more words is classified by its text
  ([whatlang](https://github.com/greyblake/whatlang-rs), limited to these five languages).
- A shorter block switches language only on a letter unique to one of them (`ã õ`
  Portuguese, `ñ ¿ ¡` Spanish, `œ î ï û ë ÿ` French, `ì ò` Italian; shared letters such
  as `ç` or `à` decide nothing). Otherwise it inherits: a heading takes the language of
  the block after it (the section it introduces), anything else the block before it.
  The document's overall language is the fallback.
- Code is read in English and does not change the language of the prose around it.
  In rendered Markdown, code is recognized by indentation or by two or more syntax
  colors; a short, single-color code block (e.g. a one-line shell command) looks like
  prose there and takes the language of the text around it.
- The status strip shows the current phrase's language (`EN`/`PT`/`ES`/`FR`/`IT`). There is no
  manual override.

A phrase is spoken in a single language: foreign words inside a sentence ("faça o
commit") take the block language's pronunciation. Detection reads the displayed text
only; it never sees source metadata such as an HTML `lang` attribute.

Numbers are spelled out for the espeak-ng languages, which would otherwise read the
separator in `3.5` or `3,5` as a sentence pause and `1.000` digit by digit. Dot-separated
groups of exactly three digits are thousands (`1.000` reads "mil", `1.000,50` "mil
vírgula cinquenta"); any other `.`/`,` between digits is spoken as the language's word
for it (`3.5` "três ponto cinco", `3,5` "três vírgula cinco", `1.18.0` "um ponto dezoito
ponto zero"). This rewrite only affects synthesis. English numbers are left to Misaki.

Kokoro returns word timestamps only for English. For the other languages the helper
derives them from the model's per-phoneme durations, phonemizing each word alone so
spoken-out numbers ("1990") stay attached to their word. When the two disagree, that
phrase keeps only its phrase shading, never a guessed word.

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

Setup downloads packages, model weights, the five voices, and English language data
(the other languages use the bundled espeak-ng), then warms each language without
playing sound. Python (3.11 on macOS, 3.12 on Linux) is
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
the viewer exits (`Esc` stops playback but keeps the model warm).

The bundled runtime supports **macOS arm64 and Linux x86_64**. Other platforms still
build and browse normally, and can supply another backend using `tts_command`. Missing setup,
missing output devices, and synthesis failures produce an in-app notice rather
than terminating the viewer. The helper starts only on the first explicit read.

## Memory use

On macOS arm64, each helper limits MLX's reusable GPU allocation cache to **64 MiB**
(in `scripts/read_aloud.py`). It also clears unused cached GPU buffers when a synthesis
job returns, including after cancellation or an error. The model and live pipeline
arrays stay loaded for subsequent playback.

On Linux with CUDA, the helper does the same with PyTorch's allocator. It turns on
`expandable_segments` (unless you set `PYTORCH_CUDA_ALLOC_CONF` yourself), so phrases of
different lengths reuse one growable region instead of leaving behind differently sized
blocks. It also empties the CUDA cache when a synthesis job returns. Measured on Kokoro,
the GPU memory a helper holds peaks around 0.9 GB instead of 1.45 GB and falls back to
about 0.6 GB between reads. A CPU-only helper is unaffected.

None of this is a limit on total process memory: model weights, live intermediate
tensors, audio, and Python libraries use additional memory (about 2 GB of system RAM
for a PyTorch helper). Synthesis can still have higher temporary peaks. Each viewer runs
its own helper, so multiple viewers have independent models and caches.

A helper that has been neither loading nor playing for **5 minutes** is stopped, which
frees all of its memory. That covers reading that finished, was stopped, or stayed
paused. Nothing changes on screen: a paused session resumes from the start of its
current phrase, and seeking still works. The next read starts a new helper, which
first reloads the model (a few seconds).

Restart an already-running helper/viewer after updating to use the new policy.

## Controls

| Key | Action |
| --- | --- |
`t` is the only global read-aloud key (remappable as `toggle_speech`). It reads from the
**phrase at the top visible line** of the settled active preview and enters **reading
mode**; pressed again, it pauses/resumes. Reading mode lasts until `Esc`, including after
the document finishes, and owns these fixed, media-player keys:

| Key | Action |
| --- | --- |
| `Space` | Pause / resume |
| `←` / `→` | Previous / next speech phrase. Previous restarts the current phrase after two audible seconds; otherwise it goes back one |
| `↑` / `↓` | Previous / next paragraph/block. No wrap; previous at the first block restarts it |
| `=` (or `+`) / `-` | Adjust speed by 0.1×, from 0.5× to 2.5×, without changing pitch |
| `Enter` | Read from the phrase containing the selection, visible current search match, or viewport top, in that priority order |
| `f` | Toggle follow playback; on by default, with its state shown in the status strip |
| `Esc` / `q` | Stop, clear the highlight, and leave reading mode; the next `t` starts at the then-visible line |

Every other key keeps its global meaning, so `j`/`k`, `PgUp`/`PgDn`, `/` search, and `n`/`N`
browse while listening. Reading-mode keys need no Shift on US or ABNT2 layouts.

The status strip reports idle/loading/reading/paused/finished, the phrase's language, speed, phrase position, and follow state. With a
backend configured its row stays reserved even while idle, so starting speech or
changing speed cannot resize/reflow the preview and invalidate its coordinates. While reading
it also lists the reading-mode keys. A shaded phrase gives context; the current word is
yellow/bold. With follow on, the active preview scrolls when the spoken line leaves the
viewport. A finder, search prompt, help, line-select, or annotation modal owns its keys while
open; close it to use the reading-mode keys again. Line-select mode keeps its fixed `J`/`K`
extension controls and a fixed local `g` confirm to read from the selection.

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
not the entire index. For larger configured previews, `Enter` outside the indexed
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
At the end, the text index remains available for Previous; `Esc` clears it.

The index normally includes text before the initial starting point, so Previous
can go back before where you first pressed `t`. A phrase is a sentence or a bounded
clause (not necessarily a whole sentence). Paragraph navigation uses display-based
blocks: paragraphs, recognizable headings and list items, and isolated code/table
rows. In non-Markdown views speech still reads individual lines, but blank lines
separate paragraph-navigation groups. Custom renderer output can erase structure;
this is not a Markdown AST/source-position guarantee.

To choose somewhere else while listening:

1. Press `f` to turn follow **off**, so playback does not pull your viewport back.
2. Scroll (`j`/`k`, `PgUp`/`PgDn`) or search (`/`, `n`/`N`) to the passage. Either select text with the mouse, use the current
   visible search result, or leave the desired line at the top of the viewport.
   For keyboard line selection, `Tab` to content and press `L`, then move the marker.
   (`L` switches transformed views to source, which stops existing playback.)
3. Press `Enter` (or `g` in line-select mode). Reading begins at the **containing phrase**, which may repeat text
   before your caret; a selection is a starting anchor, not a playback end boundary.
   The selection is consumed so it does not obscure the spoken highlight.
4. Press `f` again if you want automatic following back on. It immediately reveals
   the current phrase, even when paused. Follow is temporarily suppressed while
   a modal or drag owns the browsing cursor.

`n`/`N`, `[`/`]`, and `j`/`k` keep their search/file/scroll meanings while reading; the arrows
belong to reading mode until `Esc`.

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

`? → Settings` shows both values. `[keys]` remaps `toggle_speech` like other actions; the
reading-mode keys are fixed.

Set `HERDR_FILE_VIEWER_NO_TTS=1` to disable the optional backend and its idle
status row (also used by non-audio PTY tests, independently of local runtime setup).

The helper accepts `HERDR_TTS_VOICE` in its environment to replace the English voice
(any American English Kokoro voice, e.g. `am_michael`); pre-cache a new voice by running
setup with it set. The other languages' voices are fixed.

## Backend protocol

`tts_command` launches one subprocess with piped stdin/stdout and nulled stderr.
Its working directory is the trusted launch directory, never the viewed tree.
The controller sends JSON lines (text is **never** in argv or interpolated into a
shell command):

```json
{"cmd":"speak","id":3,"document":1,"offset":0,"paused":false,"chunks":[{"text":"Hello.","line":0,"start":0,"end":6,"lang":"en"}],"speed":1.0}
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

Each chunk's `lang` is `en`, `pt`, `es`, `fr`, or `it`; backends that ignore it read everything in
one language. The bundled helper rejects any other value and treats a missing `lang`
as `en`. `start`/`end` are optional UTF-8 byte offsets **within the phrase**, not character
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
