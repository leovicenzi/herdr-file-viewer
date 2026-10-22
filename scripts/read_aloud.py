#!/usr/bin/env python3
"""Local Kokoro helper. stdin/stdout: JSON lines; stdout is protocol only.

MLX runs inference on Apple Silicon; elsewhere PyTorch does, on CUDA when present.
Audio and input text stay in memory. A single synthesis thread owns the model, keeps it
warm, and prefetches at most two phrases. The audio callback never synthesizes
or launches processes. Progress uses PortAudio's output DAC clock, not a text timer.
"""
from __future__ import annotations

import contextlib
import json
import math
import os
import platform
import queue
import subprocess
import sys
import threading
import time
from dataclasses import dataclass
from collections import OrderedDict, deque

PROTOCOL_OUT = sys.stdout
MODEL = "mlx-community/Kokoro-82M-bf16"
TORCH_MODEL = "hexgrad/Kokoro-82M"
SAMPLE_RATE = 24000
MAX_REQUEST = 2 * 1024 * 1024
MAX_TEXT_BYTES = 256 * 1024
MAX_CACHE_BYTES = 16 * 1024 * 1024
MAX_CACHE_PHRASES = 16
# Buffers queued ahead of the DAC: ~1 on CoreAudio, 2+ on ALSA/PipeWire at 20 ms blocks.
MAX_QUEUED_BUFFERS = 32


def emit(event, ident, **fields):
    print(json.dumps(dict(event=event, id=ident, **fields)), file=PROTOCOL_OUT, flush=True)


def speed_value(value):
    value = float(value)
    return min(2.5, max(0.5, value)) if math.isfinite(value) else 1.0


def word_marks(text, tokens):
    """Map Kokoro token timestamps to original UTF-8 bytes, conservatively.

    Never invent coordinates when the phonemizer normalizes a token differently.
    Those passages retain the phrase highlight instead of guessing a word.
    """
    marks, cursor = [], 0
    for token in tokens or []:
        value = token.text
        start, end = token.start_ts, token.end_ts
        if not value or start is None or end is None:
            continue
        at = text.find(value, cursor)
        if at < 0:
            continue
        cursor = at + len(value)
        marks.append((float(start), float(end), len(text[:at].encode()),
                      len(text[:cursor].encode())))
    return marks


def stretch(audio, speed, np):
    """Pitch-preserving time stretch, bounded to one short phrase, no temp files."""
    if abs(speed - 1.0) < 0.001 or len(audio) == 0:
        return audio
    factors = [2.0, speed / 2.0] if speed > 2.0 else [speed]
    proc = subprocess.run(
        ["ffmpeg", "-nostdin", "-hide_banner", "-loglevel", "error",
         "-f", "f32le", "-ar", str(SAMPLE_RATE), "-ac", "1", "-i", "pipe:0",
         "-af", ",".join(f"atempo={factor}" for factor in factors),
         "-f", "f32le", "pipe:1"],
        input=audio.astype("<f4", copy=False).tobytes(), capture_output=True,
        timeout=5, check=True, cwd=os.path.dirname(os.path.abspath(__file__)),
    )
    return np.frombuffer(proc.stdout, dtype="<f4").copy()


@dataclass
class Generated:
    ident: int
    index: int
    audio: object = None
    marks: object = None
    error: str | None = None
    done: bool = False


class AudioCache:
    """Document-scoped LRU of original-speed PCM + alignment, never disk-backed.

    Reset is called by the command thread; synthesis owns gets/puts. A canceled
    old-document generation cannot reinsert its audio after reset.
    """
    def __init__(self, max_bytes=MAX_CACHE_BYTES, max_phrases=MAX_CACHE_PHRASES):
        self.lock = threading.Lock()
        self.document = None
        self.entries = OrderedDict()
        self.bytes = 0
        self.max_bytes, self.max_phrases = max_bytes, max_phrases

    def reset(self, document):
        with self.lock:
            if document is None or document != self.document:
                self.document = document
                self.entries.clear()
                self.bytes = 0

    def get(self, document, index, text):
        with self.lock:
            if document != self.document:
                return None
            entry = self.entries.get(index)
            if entry is None or entry[0] != text:
                return None
            self.entries.move_to_end(index)
            return entry[1], entry[2]

    def put(self, document, index, text, audio, marks):
        with self.lock:
            if document != self.document or document is None:
                return
            old = self.entries.pop(index, None)
            if old is not None:
                self.bytes -= old[1].nbytes
            if audio.nbytes > self.max_bytes or self.max_phrases < 1:
                return
            self.entries[index] = (text, audio, marks)
            self.bytes += audio.nbytes
            while self.bytes > self.max_bytes or len(self.entries) > self.max_phrases:
                _, old = self.entries.popitem(last=False)
                self.bytes -= old[1].nbytes


class Control:
    def __init__(self):
        self.ident = 0
        self.latest_id = 0
        self.cache = AudioCache()
        self.paused = False
        self.speed = 1.0
        self.closed = threading.Event()
        self.jobs = queue.Queue(maxsize=1)
        self.results = queue.Queue(maxsize=2)
        self.commands = queue.Queue()

    def submit(self, job):
        # Only the newest pending seek matters; never accumulate full text requests.
        while True:
            try:
                self.jobs.get_nowait()
            except queue.Empty:
                break
        self.jobs.put_nowait(job)

    def publish(self, result):
        while not self.closed.is_set() and self.ident == result.ident:
            try:
                self.results.put(result, timeout=0.05)
                return
            except queue.Full:
                pass


def read_commands(control):
    try:
        while not control.closed.is_set():
            line = sys.stdin.buffer.readline(MAX_REQUEST + 1)
            if not line or len(line) > MAX_REQUEST:
                break
            try:
                command = json.loads(line)
                if isinstance(command, dict):
                    control.commands.put(command)
            except (ValueError, UnicodeDecodeError):
                continue
    finally:
        control.closed.set()


def use_mlx():
    return sys.platform == "darwin" and platform.machine() == "arm64"


def torch_device(torch):
    """HERDR_TTS_DEVICE wins; otherwise CUDA when present, else CPU."""
    requested = os.environ.get("HERDR_TTS_DEVICE", "").strip()
    if requested:
        return requested
    return "cuda" if torch.cuda.is_available() else "cpu"


def load_pipeline():
    lang_code = os.environ.get("HERDR_TTS_LANG", "a")
    if lang_code in ("a", "b"):
        # Provisioning must remain explicit, never an implicit Misaki download.
        from importlib.metadata import version, PackageNotFoundError
        try:
            version("en-core-web-sm")
        except PackageNotFoundError:
            raise RuntimeError("English language data is missing; run scripts/setup-tts.sh")
    if use_mlx():
        from mlx_audio.tts.utils import load_model
        from mlx_audio.tts.models.kokoro.pipeline import KokoroPipeline
        model = load_model(MODEL)
        return KokoroPipeline(lang_code=lang_code, model=model, repo_id=MODEL)
    # Same misaki-based pipeline (graphemes + token timestamps), on PyTorch.
    import torch
    from kokoro import KPipeline
    return KPipeline(lang_code=lang_code, repo_id=TORCH_MODEL, device=torch_device(torch))


def generate_job(control, job, pipeline, voice, np):
    ident, document, offset = job["id"], job["document"], job["offset"]
    for index, chunk in enumerate(job["chunks"]):
        if ident != control.ident or control.closed.is_set():
            break
        text = chunk["text"]
        cached = control.cache.get(document, offset + index, text)
        if cached is not None:
            audio, marks = cached
        else:
            audios, marks, seconds, cursor = [], [], 0.0, 0
            for result in pipeline(text, voice=voice, speed=1.0):
                if ident != control.ident or control.closed.is_set():
                    break
                audio = result.audio
                if hasattr(audio, "cpu"):  # PyTorch tensor; MLX arrays convert directly
                    audio = audio.detach().cpu()
                audio = np.asarray(audio, dtype=np.float32).reshape(-1)
                segment = result.graphemes
                at = text.find(segment, cursor)
                if at >= 0:
                    byte_offset = len(text[:at].encode())
                    marks.extend((a+seconds, b+seconds, s+byte_offset, e+byte_offset)
                                 for a,b,s,e in word_marks(segment, result.tokens))
                    cursor = at + len(segment)
                audios.append(audio)
                seconds += len(audio) / SAMPLE_RATE
            # Never cache a partial phrase when generation was superseded mid-result.
            if ident != control.ident or control.closed.is_set():
                break
            if not audios:
                continue
            audio = np.concatenate(audios)
            control.cache.put(document, offset + index, text, audio, marks)
        control.publish(Generated(ident, index, audio, marks))
    if ident == control.ident:
        control.publish(Generated(ident, -1, done=True))


def synthesize(control):
    pipeline = None
    while not control.closed.is_set():
        try:
            job = control.jobs.get(timeout=0.1)
        except queue.Empty:
            continue
        ident = job["id"]
        if ident != control.ident:
            continue
        try:
            with contextlib.redirect_stdout(sys.stderr):
                if pipeline is None:
                    pipeline = load_pipeline()
                import numpy as np
                generate_job(control, job, pipeline, os.environ.get("HERDR_TTS_VOICE", "af_heart"), np)
        except Exception as exc:
            control.publish(Generated(ident, -1, error=f"{type(exc).__name__}: {exc}"))


class Player:
    """Short callback buffers + DAC-clock cursor. Expensive work runs in main()."""
    def __init__(self, np):
        self.np = np
        self.lock = threading.Lock()
        self.audio = np.empty(0, dtype=np.float32)
        self.position = 0
        self.speed = 1.0
        self.origin = 0.0
        self.elapsed_origin = 0.0
        self.paused = False
        self.clocks = deque(maxlen=MAX_QUEUED_BUFFERS)
        self.finished_at = None

    def callback(self, out, frames, timing, status):
        out.fill(0)
        with self.lock:
            if self.paused or self.position >= len(self.audio):
                return
            count = min(frames, len(self.audio) - self.position)
            out[:count, 0] = self.audio[self.position:self.position+count]
            self.clocks.append((timing.outputBufferDacTime,
                          self.origin + self.position / SAMPLE_RATE * self.speed,
                          self.speed, count / SAMPLE_RATE,
                          self.elapsed_origin + self.position / SAMPLE_RATE))
            self.position += count
            if self.position >= len(self.audio):
                self.finished_at = timing.outputBufferDacTime + count / SAMPLE_RATE

    def load(self, audio, speed, origin=0.0, elapsed_origin=0.0):
        with self.lock:
            self.audio = audio
            self.position = 0
            self.speed = speed
            self.origin = origin
            self.elapsed_origin = elapsed_origin
            self.clocks.clear()
            self.finished_at = None

    def clear(self):
        self.load(self.np.empty(0, dtype=self.np.float32), 1.0)

    def _audible(self, now):
        """Newest buffer already at the DAC; later ones are still queued ahead of it."""
        for clock in reversed(self.clocks):
            if clock[0] <= now:
                return clock
        return None

    def time_in_phrase(self, now):
        with self.lock:
            clock = self._audible(now)
            if clock is None:
                return None
            dac, start, speed, duration, _ = clock
            return start + min(now - dac, duration) * speed

    def elapsed_in_phrase(self, now):
        with self.lock:
            clock = self._audible(now)
            if clock is None:
                return None
            dac, _, _, duration, elapsed = clock
            return elapsed + min(now - dac, duration)

    def next_elapsed_time(self):
        with self.lock:
            return self.elapsed_origin + self.position / SAMPLE_RATE

    def next_source_time(self):
        with self.lock:
            return self.origin + self.position / SAMPLE_RATE * self.speed

    def done(self, now):
        with self.lock:
            return len(self.audio) == 0 or (self.finished_at is not None and now >= self.finished_at)


def drain(items):
    while True:
        try:
            items.get_nowait()
        except queue.Empty:
            return


def apply_command(control, command, player=None):
    """Apply seek/pause atomically before any new phrase can enter the DAC queue."""
    cmd, ident = command.get("cmd"), command.get("id")
    if type(ident) is not int or ident < 1:
        return None
    if cmd == "speak":
        chunks = command.get("chunks")
        document, offset = command.get("document", ident), command.get("offset", 0)
        paused = command.get("paused", False)
        valid = (isinstance(chunks, list) and 0 < len(chunks) <= 2048
                 and type(document) is int and document > 0
                 and type(offset) is int and offset >= 0 and type(paused) is bool)
        if valid:
            valid = all(isinstance(c, dict) and isinstance(c.get("text"), str)
                        and 0 < len(c["text"]) <= 1024 for c in chunks)
        if valid:
            valid = sum(len(c["text"].encode()) for c in chunks) <= MAX_TEXT_BYTES
        try:
            speed = speed_value(command.get("speed", 1.0))
        except (TypeError, ValueError):
            valid = False
        if not valid:
            emit("error", ident, message="Invalid or oversized read request")
            return None
        control.ident = control.latest_id = ident
        control.cache.reset(document)
        control.speed, control.paused = speed, paused
        if player:
            player.clear()
        drain(control.results)
        control.submit(dict(command, document=document, offset=offset))
    elif ident == control.latest_id:
        if cmd == "stop":
            control.ident = 0
            control.paused = False
            control.cache.reset(None)
            drain(control.jobs)
            drain(control.results)
            if player:
                player.clear()
        elif cmd == "pause":
            control.paused = bool(command.get("paused"))
        elif cmd == "speed":
            try:
                control.speed = speed_value(command.get("speed", 1.0))
            except (TypeError, ValueError):
                return None
        else:
            return None
    else:
        return None
    return cmd


def main():
    control = Control()
    threading.Thread(target=read_commands, args=(control,), daemon=True).start()
    threading.Thread(target=synthesize, args=(control,), daemon=True).start()
    player = stream = current = None
    last_mark = None
    pending = None
    try:
        while not control.closed.is_set():
            while True:
                try:
                    command = control.commands.get_nowait()
                except queue.Empty:
                    break
                action = apply_command(control, command, player)
                if action in ("speak", "stop"):
                    current = pending = last_mark = None
            if player:
                with player.lock:
                    player.paused = control.paused
            if current and not control.paused:
                if abs(player.speed - control.speed) > 0.001:
                    # Stretch only the not-yet-queued suffix; the model stays warm
                    # and speed changes do not require regenerating speech.
                    origin = player.next_source_time()
                    offset = min(len(current.audio), round(origin * SAMPLE_RATE))
                    elapsed = player.next_elapsed_time()
                    player.load(stretch(current.audio[offset:], control.speed, np), control.speed, origin, elapsed)
                now = stream.time
                point = player.time_in_phrase(now)
                if point is not None:
                    mark = next(((s,e) for a,b,s,e in current.marks if a <= point < b), None)
                    elapsed = player.elapsed_in_phrase(now)
                    key = (current.index, mark, elapsed is not None and elapsed >= 2.0)
                    if key != last_mark:
                        fields = dict(chunk=current.index, elapsed=elapsed)
                        if mark is not None:
                            fields.update(start=mark[0], end=mark[1])
                        emit("progress", current.ident, **fields)
                        last_mark = key
                if player.done(now):
                    current = None
            if current is None and not control.paused:
                if pending is None:
                    try:
                        pending = control.results.get_nowait()
                    except queue.Empty:
                        pass
                if pending is not None:
                    result, pending = pending, None
                    if result.ident == control.ident:
                        if result.error:
                            emit("error", result.ident, message=result.error)
                            control.ident = 0
                        elif result.done:
                            emit("done", result.ident)
                            control.ident = 0
                        else:
                            if player is None:
                                import numpy as np
                                import sounddevice as sd
                                player = Player(np)
                                stream = sd.OutputStream(samplerate=SAMPLE_RATE, channels=1,
                                    dtype="float32", blocksize=480, latency="low", callback=player.callback)
                                stream.start()
                            player.load(stretch(result.audio, control.speed, np), control.speed)
                            current = result
                            last_mark = None
            time.sleep(0.01)
    except Exception as exc:
        emit("error", control.ident or 0, message=f"{type(exc).__name__}: {exc}")
    finally:
        control.closed.set()
        if stream:
            stream.abort()
            stream.close()


if __name__ == "__main__":
    # Nothing is downloaded automatically at playback: setup-tts.sh provisions the
    # model, voices, and language data once. Offline documents never go to a server.
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["HF_HUB_DISABLE_PROGRESS_BARS"] = "1"
    os.environ["TOKENIZERS_PARALLELISM"] = "false"
    main()
