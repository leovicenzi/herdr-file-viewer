#!/bin/sh
# Explicit, one-time provisioning. Playback itself is offline and installs nothing.
# Apple Silicon macOS gets MLX; Linux gets PyTorch (CUDA build, CPU fallback at runtime).
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PY="$ROOT/.venv-tts/bin/python"
SPACY_EN='https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl'
case "$(uname -s)/$(uname -m)" in
    Darwin/arm64)
        command -v uv >/dev/null || { echo 'Install uv first: brew install uv' >&2; exit 1; }
        command -v ffmpeg >/dev/null || { echo 'Install FFmpeg first: brew install ffmpeg' >&2; exit 1; }
        uv venv --python 3.11 --allow-existing "$ROOT/.venv-tts"
        uv pip install --python "$PY" 'mlx-audio==0.6.1' 'misaki[en]==0.9.4' "$SPACY_EN"
        ;;
    Linux/x86_64)
        command -v uv >/dev/null || { echo 'Install uv first (https://docs.astral.sh/uv/)' >&2; exit 1; }
        command -v ffmpeg >/dev/null || { echo 'Install FFmpeg first' >&2; exit 1; }
        # kokoro supports Python <3.13. TTS_TORCH_INDEX=cpu skips the multi-GB CUDA wheels.
        INDEX="https://download.pytorch.org/whl/${TTS_TORCH_INDEX:-cu130}"
        uv venv --python 3.12 --allow-existing "$ROOT/.venv-tts"
        uv pip install --python "$PY" --index-url "$INDEX" 'torch==2.14.1'
        uv pip install --python "$PY" 'kokoro==0.9.4' 'misaki[en]==0.9.4' \
            'sounddevice==0.5.6' "$SPACY_EN"
        ;;
    *)
        echo 'Bundled read-aloud supports Apple Silicon macOS and x86_64 Linux; use tts_command elsewhere.' >&2
        exit 1
        ;;
esac
"$PY" - "$ROOT/scripts" <<'PY'
import sys
sys.path.insert(0, sys.argv[1])
import read_aloud
pipeline = read_aloud.load_pipeline()
# Warm the default voice without playing sound. Caches weights, voice, language data.
for result in pipeline('Local reading is ready.', voice='af_heart'):
    assert result.audio is not None and len(result.audio) > 0
backend = 'MLX' if read_aloud.use_mlx() else f'PyTorch/{getattr(pipeline.model, "device", "?")}'
print(f'Kokoro ({backend}) ready. Playback is offline; press t in the viewer.')
PY
