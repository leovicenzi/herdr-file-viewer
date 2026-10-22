#!/bin/sh
# Explicit, one-time provisioning. Playback itself is offline and installs nothing.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
if [ "$(uname -s)" != Darwin ] || [ "$(uname -m)" != arm64 ]; then
    echo 'Bundled MLX read-aloud requires Apple Silicon macOS.' >&2
    exit 1
fi
command -v uv >/dev/null || { echo 'Install uv first: brew install uv' >&2; exit 1; }
command -v ffmpeg >/dev/null || { echo 'Install FFmpeg first: brew install ffmpeg' >&2; exit 1; }
uv venv --python 3.11 --allow-existing "$ROOT/.venv-tts"
uv pip install --python "$ROOT/.venv-tts/bin/python" 'mlx-audio==0.6.1' 'misaki[en]==0.9.4' 
uv pip install --python "$ROOT/.venv-tts/bin/python" 
    'https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl'
"$ROOT/.venv-tts/bin/python" - <<'PY'
from mlx_audio.tts.utils import load_model
from mlx_audio.tts.models.kokoro.pipeline import KokoroPipeline
repo = 'mlx-community/Kokoro-82M-bf16'
model = load_model(repo)
pipeline = KokoroPipeline(lang_code='a', model=model, repo_id=repo)
# Warm the default voice without playing sound. Caches weights, voice, language data.
for result in pipeline('Local reading is ready.', voice='af_heart'):
    assert result.audio.size > 0
print('Kokoro/MLX ready. Playback is offline; press t in the viewer.')
PY
